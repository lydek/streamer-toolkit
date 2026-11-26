//! 頻道點數獎勵: the channel's custom rewards, read through Helix, and changed when the toolkit made them (Twitch lets
//! an app change only the rewards created with its own Client ID; `channel:manage:redemptions` to write).

use super::*;
use std::collections::HashSet;

/// Asked for before any write, so a Twitch 403 afterwards means the reward is not the toolkit's.
const MANAGE_SCOPE: &str = "channel:manage:redemptions";

/// A custom reward. `title` and `prompt` are raw Twitch text.
#[derive(Serialize, ToSchema)]
pub(crate) struct ChannelReward {
    id: String,
    title: String,
    prompt: String,
    cost: u64,
    /// `#RRGGBB`.
    background_color: String,
    is_enabled: bool,
    is_paused: bool,
    is_user_input_required: bool,
    /// Redemptions are marked fulfilled at once instead of waiting in the queue.
    skip_queue: bool,
    /// `None`: no cooldown.
    cooldown_seconds: Option<u64>,
    /// `None`: no limit.
    max_per_stream: Option<u64>,
    /// `None`: no limit.
    max_per_user_per_stream: Option<u64>,
    /// Made by the toolkit, so it can change the reward and its redemptions; others are read-only here.
    manageable: bool,
}

#[derive(Deserialize)]
struct HelixRewardList {
    data: Vec<HelixReward>,
}

#[derive(Deserialize)]
struct Setting {
    is_enabled: bool,
    #[serde(
        alias = "max_per_stream",
        alias = "max_per_user_per_stream",
        alias = "global_cooldown_seconds"
    )]
    value: u64,
}

impl Setting {
    fn get(&self) -> Option<u64> {
        self.is_enabled.then_some(self.value)
    }
}

#[derive(Deserialize)]
struct HelixReward {
    id: String,
    title: String,
    #[serde(default)]
    prompt: String,
    cost: u64,
    #[serde(default)]
    background_color: String,
    is_enabled: bool,
    is_paused: bool,
    is_user_input_required: bool,
    should_redemptions_skip_request_queue: bool,
    global_cooldown_setting: Setting,
    max_per_stream_setting: Setting,
    max_per_user_per_stream_setting: Setting,
}

impl HelixReward {
    fn into_reward(self, manageable: bool) -> ChannelReward {
        ChannelReward {
            cooldown_seconds: self.global_cooldown_setting.get(),
            max_per_stream: self.max_per_stream_setting.get(),
            max_per_user_per_stream: self.max_per_user_per_stream_setting.get(),
            id: self.id,
            title: self.title,
            prompt: self.prompt,
            cost: self.cost,
            background_color: self.background_color,
            is_enabled: self.is_enabled,
            is_paused: self.is_paused,
            is_user_input_required: self.is_user_input_required,
            skip_queue: self.should_redemptions_skip_request_queue,
            manageable,
        }
    }
}

/// Twitch reward and redemption IDs are UUIDs; checked before one goes into a Helix URL.
pub(super) fn valid_twitch_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub(super) fn rewards_error(e: HelixError) -> Response {
    match e {
        HelixError::Auth(e) => auth_error(e),
        // The scope was checked first: Twitch refuses a reward another app (or the dashboard) made.
        HelixError::Refused(403) => error(StatusCode::FORBIDDEN, "rewards.not_ours"),
        HelixError::Refused(404) => error(StatusCode::NOT_FOUND, "rewards.not_found"),
        HelixError::Refused(400) => error(StatusCode::BAD_REQUEST, "rewards.refused"),
        e @ (HelixError::RateLimited(_) | HelixError::Refused(_)) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

/// Why a reward cannot be changed before anything is sent to Twitch.
pub(super) enum Denied {
    SignedOut,
    NoScope,
}

impl Denied {
    pub(super) fn response(self) -> Response {
        match self {
            Denied::SignedOut => auth_error(AuthError::SignInRequired),
            Denied::NoScope => error(StatusCode::FORBIDDEN, "twitch.scope_required"),
        }
    }
}

/// The signed-in broadcaster, when the sign-in has `channel:manage:redemptions`.
pub(super) fn manager(twitch: &TwitchAuth) -> Result<String, Denied> {
    let token = twitch.token().ok_or(Denied::SignedOut)?;
    if token.user_id.is_empty() || !token.user_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Denied::SignedOut);
    }
    if !token.scopes.iter().any(|s| s == MANAGE_SCOPE) {
        return Err(Denied::NoScope);
    }
    Ok(token.user_id)
}

fn reward_list(body: serde_json::Value) -> Result<Vec<HelixReward>, HelixError> {
    serde_json::from_value::<HelixRewardList>(body)
        .map(|l| l.data)
        .map_err(|_| HelixError::Auth(AuthError::InvalidResponse))
}

/// Every custom reward of the channel, each marked whether the toolkit made it.
pub(super) async fn fetch_rewards(
    helix: &Helix,
    id: &str,
) -> Result<Vec<ChannelReward>, HelixError> {
    let path = format!("/channel_points/custom_rewards?broadcaster_id={id}");
    let all = reward_list(helix.get_json(&path).await?)?;
    let own: HashSet<String> = reward_list(
        helix
            .get_json(&format!("{path}&only_manageable_rewards=true"))
            .await?,
    )?
    .into_iter()
    .map(|r| r.id)
    .collect();
    Ok(all
        .into_iter()
        .map(|r| {
            let manageable = own.contains(&r.id);
            r.into_reward(manageable)
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/admin/rewards",
    tag = "admin",
    responses(
        (status = 200, body = Vec<ChannelReward>),
        (status = 400, description = "Not signed in", body = ErrorBody),
        (status = 502, description = "`twitch.request_refused` (e.g. a channel without channel points), `twitch.rate_limited` or `twitch.unreachable`", body = ErrorBody),
    )
)]
pub(super) async fn list_rewards(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    let Some(id) = twitch
        .token()
        .map(|t| t.user_id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match fetch_rewards(&helix, &id).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => rewards_error(e),
    }
}

/// What changes; fields left out stay as they are.
#[derive(Deserialize, ToSchema)]
pub(crate) struct RewardChange {
    is_enabled: Option<bool>,
    is_paused: Option<bool>,
}

#[utoipa::path(
    patch,
    path = "/api/admin/rewards/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = RewardChange,
    responses(
        (status = 204),
        (status = 400, description = "`rewards.invalid`, `rewards.refused`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` (no `channel:manage:redemptions`) or `rewards.not_ours` (made in the Twitch dashboard or by another app)", body = ErrorBody),
        (status = 404, description = "`rewards.not_found`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn update_reward(
    State(s): State<Arc<AdminState>>,
    Path(reward): Path<String>,
    Json(body): Json<RewardChange>,
) -> Response {
    if !valid_twitch_id(&reward) {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let mut patch = serde_json::Map::new();
    if let Some(v) = body.is_enabled {
        patch.insert("is_enabled".into(), v.into());
    }
    if let Some(v) = body.is_paused {
        patch.insert("is_paused".into(), v.into());
    }
    if patch.is_empty() {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match helix
        .patch_json(
            &format!("/channel_points/custom_rewards?broadcaster_id={id}&id={reward}"),
            &serde_json::Value::Object(patch),
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => rewards_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_twitch_id;

    #[test]
    fn only_uuid_like_ids_reach_a_helix_url() {
        assert!(valid_twitch_id("92af127c-7326-4483-a52b-b0da0be61c01"));
        for id in ["", "a&id=b", "a/b", "a b", &"a".repeat(65)] {
            assert!(!valid_twitch_id(id), "{id:?}");
        }
    }
}
