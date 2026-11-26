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

/// Twitch's limits for custom rewards, checked here so the broadcaster is told before anything is sent.
const MAX_REWARDS: usize = 50;
const TITLE_MAX_CHARS: usize = 45;
const PROMPT_MAX_CHARS: usize = 200;
const COOLDOWN_MAX_SECONDS: u64 = 604_800;
/// Twitch takes these as 32-bit integers.
const COUNT_MAX: u64 = i32::MAX as u64;

/// A reward's settings; fields left out stay as they are (when editing) or take Twitch's defaults (when adding,
/// which needs `title` and `cost`).
#[derive(Deserialize, ToSchema, Default)]
pub(crate) struct RewardChange {
    /// Up to 45 characters; unique in the channel (Twitch ignores case).
    title: Option<String>,
    /// Up to 200 characters.
    prompt: Option<String>,
    /// At least 1.
    cost: Option<u64>,
    /// `#RRGGBB`.
    background_color: Option<String>,
    is_enabled: Option<bool>,
    is_paused: Option<bool>,
    is_user_input_required: Option<bool>,
    skip_queue: Option<bool>,
    /// 0: no cooldown; at most 7 days.
    cooldown_seconds: Option<u64>,
    /// 0: no limit.
    max_per_stream: Option<u64>,
    /// 0: no limit.
    max_per_user_per_stream: Option<u64>,
}

impl RewardChange {
    /// The Helix body, or `None` when a value is outside Twitch's limits.
    fn helix_body(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        if let Some(t) = &self.title {
            let t = t.trim();
            if t.is_empty() || t.chars().count() > TITLE_MAX_CHARS {
                return None;
            }
            m.insert("title".into(), t.into());
        }
        if let Some(p) = &self.prompt {
            let p = p.trim();
            if p.chars().count() > PROMPT_MAX_CHARS {
                return None;
            }
            m.insert("prompt".into(), p.into());
        }
        if let Some(c) = self.cost {
            if c == 0 || c > COUNT_MAX {
                return None;
            }
            m.insert("cost".into(), c.into());
        }
        if let Some(c) = &self.background_color {
            let hex = c
                .strip_prefix('#')
                .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()));
            m.insert(
                "background_color".into(),
                format!("#{}", hex?.to_ascii_uppercase()).into(),
            );
        }
        for (key, v) in [
            ("is_enabled", self.is_enabled),
            ("is_paused", self.is_paused),
            ("is_user_input_required", self.is_user_input_required),
            ("should_redemptions_skip_request_queue", self.skip_queue),
        ] {
            if let Some(v) = v {
                m.insert(key.into(), v.into());
            }
        }
        if let Some(s) = self.cooldown_seconds {
            if s > COOLDOWN_MAX_SECONDS {
                return None;
            }
            m.insert("is_global_cooldown_enabled".into(), (s > 0).into());
            if s > 0 {
                m.insert("global_cooldown_seconds".into(), s.into());
            }
        }
        for (flag, key, v) in [
            (
                "is_max_per_stream_enabled",
                "max_per_stream",
                self.max_per_stream,
            ),
            (
                "is_max_per_user_per_stream_enabled",
                "max_per_user_per_stream",
                self.max_per_user_per_stream,
            ),
        ] {
            if let Some(n) = v {
                if n > COUNT_MAX {
                    return None;
                }
                m.insert(flag.into(), (n > 0).into());
                if n > 0 {
                    m.insert(key.into(), n.into());
                }
            }
        }
        Some(m)
    }
}

/// Another reward of the channel already has this title (Twitch refuses a second one, ignoring case).
fn title_taken(rewards: &[ChannelReward], title: &str, except: Option<&str>) -> bool {
    let title = title.trim().to_lowercase();
    rewards
        .iter()
        .any(|r| Some(r.id.as_str()) != except && r.title.trim().to_lowercase() == title)
}

/// Why a new reward was not made.
pub(super) enum CreateError {
    Limit,
    DuplicateTitle,
    Helix(HelixError),
}

impl CreateError {
    pub(super) fn response(self) -> Response {
        match self {
            CreateError::Limit => error(StatusCode::CONFLICT, "rewards.limit"),
            CreateError::DuplicateTitle => error(StatusCode::CONFLICT, "rewards.duplicate_title"),
            CreateError::Helix(e) => rewards_error(e),
        }
    }
}

/// Makes a reward on Twitch after checking the limit and the title against the channel's rewards; its new ID.
pub(super) async fn create_on_twitch(
    helix: &Helix,
    id: &str,
    body: serde_json::Map<String, serde_json::Value>,
) -> Result<String, CreateError> {
    let rewards = fetch_rewards(helix, id).await.map_err(CreateError::Helix)?;
    if rewards.len() >= MAX_REWARDS {
        return Err(CreateError::Limit);
    }
    let title = body
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or_default();
    if title_taken(&rewards, title, None) {
        return Err(CreateError::DuplicateTitle);
    }
    let created = helix
        .post_json_read(
            &format!("/channel_points/custom_rewards?broadcaster_id={id}"),
            &serde_json::Value::Object(body),
        )
        .await
        .map_err(CreateError::Helix)?;
    reward_list(created)
        .ok()
        .and_then(|l| l.into_iter().next())
        .map(|r| r.id)
        .ok_or(CreateError::Helix(HelixError::Auth(
            AuthError::InvalidResponse,
        )))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct CreatedReward {
    id: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/rewards",
    tag = "admin",
    request_body = RewardChange,
    responses(
        (status = 200, body = CreatedReward),
        (status = 400, description = "`rewards.invalid` (no title or cost, or over Twitch's limits), `rewards.refused`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`", body = ErrorBody),
        (status = 409, description = "`rewards.limit` (the channel has 50 rewards) or `rewards.duplicate_title`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn create_reward(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<RewardChange>,
) -> Response {
    let Some(patch) = body
        .helix_body()
        .filter(|m| m.contains_key("title") && m.contains_key("cost"))
    else {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    };
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match create_on_twitch(&helix, &id, patch).await {
        Ok(id) => Json(CreatedReward { id }).into_response(),
        Err(e) => e.response(),
    }
}

#[utoipa::path(
    delete,
    path = "/api/admin/rewards/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 400, description = "`rewards.invalid`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` or `rewards.not_ours`", body = ErrorBody),
        (status = 404, description = "`rewards.not_found`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn delete_reward(
    State(s): State<Arc<AdminState>>,
    Path(reward): Path<String>,
) -> Response {
    if !valid_twitch_id(&reward) {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match helix
        .delete(&format!(
            "/channel_points/custom_rewards?broadcaster_id={id}&id={reward}"
        ))
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => rewards_error(e),
    }
}

#[utoipa::path(
    patch,
    path = "/api/admin/rewards/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = RewardChange,
    responses(
        (status = 204),
        (status = 400, description = "`rewards.invalid` (nothing to change, or over Twitch's limits), `rewards.refused`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` (no `channel:manage:redemptions`) or `rewards.not_ours` (made in the Twitch dashboard or by another app)", body = ErrorBody),
        (status = 404, description = "`rewards.not_found`", body = ErrorBody),
        (status = 409, description = "`rewards.duplicate_title`", body = ErrorBody),
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
    let Some(patch) = body.helix_body().filter(|m| !m.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    };
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    // A new title must not be another reward's.
    if let Some(title) = patch.get("title").and_then(|t| t.as_str()) {
        match fetch_rewards(&helix, &id).await {
            Ok(list) if title_taken(&list, title, Some(&reward)) => {
                return error(StatusCode::CONFLICT, "rewards.duplicate_title");
            }
            Ok(_) => {}
            Err(e) => return rewards_error(e),
        }
    }
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
    use super::{RewardChange, valid_twitch_id};

    #[test]
    fn reward_settings_follow_twitch_limits_and_names() {
        let body = RewardChange {
            title: Some(" 喝水 ".into()),
            cost: Some(200),
            background_color: Some("#1f69ff".into()),
            skip_queue: Some(true),
            cooldown_seconds: Some(0),
            max_per_user_per_stream: Some(3),
            ..RewardChange::default()
        }
        .helix_body()
        .unwrap();
        assert_eq!(
            serde_json::Value::Object(body),
            serde_json::json!({
                "title": "喝水", "cost": 200, "background_color": "#1F69FF",
                "should_redemptions_skip_request_queue": true, "is_global_cooldown_enabled": false,
                "is_max_per_user_per_stream_enabled": true, "max_per_user_per_stream": 3
            })
        );
        for bad in [
            RewardChange {
                title: Some(" ".into()),
                ..RewardChange::default()
            },
            RewardChange {
                title: Some("字".repeat(46)),
                ..RewardChange::default()
            },
            RewardChange {
                prompt: Some("字".repeat(201)),
                ..RewardChange::default()
            },
            RewardChange {
                cost: Some(0),
                ..RewardChange::default()
            },
            RewardChange {
                background_color: Some("blue".into()),
                ..RewardChange::default()
            },
            RewardChange {
                cooldown_seconds: Some(604_801),
                ..RewardChange::default()
            },
        ] {
            assert!(bad.helix_body().is_none());
        }
    }

    #[test]
    fn only_uuid_like_ids_reach_a_helix_url() {
        assert!(valid_twitch_id("92af127c-7326-4483-a52b-b0da0be61c01"));
        for id in ["", "a&id=b", "a/b", "a b", &"a".repeat(65)] {
            assert!(!valid_twitch_id(id), "{id:?}");
        }
    }
}
