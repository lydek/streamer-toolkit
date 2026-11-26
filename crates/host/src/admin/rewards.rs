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

/// Errors reading the channel's rewards. A 403 there means a channel without channel points (not an affiliate or
/// partner), not someone else's reward; a 401 after the refresh means the sign-in lacks the read scope.
fn list_error(e: HelixError) -> Response {
    match e {
        HelixError::Auth(e) => auth_error(e),
        HelixError::Refused(401) => error(StatusCode::FORBIDDEN, "twitch.scope_required"),
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

fn rewards_path(id: &str) -> String {
    format!("/channel_points/custom_rewards?broadcaster_id={id}")
}

/// Every custom reward of the channel as Helix has it: one request, for checking titles and the limit.
async fn channel_rewards(helix: &Helix, id: &str) -> Result<Vec<HelixReward>, HelixError> {
    reward_list(helix.get_json(&rewards_path(id)).await?)
}

/// Every custom reward of the channel, each marked whether the toolkit made it.
pub(super) async fn fetch_rewards(
    helix: &Helix,
    id: &str,
) -> Result<Vec<ChannelReward>, HelixError> {
    let path = rewards_path(id);
    let all = channel_rewards(helix, id).await?;
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
        Err(e) => list_error(e),
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

/// Another reward of the channel (`(id, title)` pairs) already has this title (Twitch refuses a second one,
/// ignoring case).
fn title_taken<'a>(
    rewards: impl IntoIterator<Item = (&'a str, &'a str)>,
    title: &str,
    except: Option<&str>,
) -> bool {
    let title = title.trim().to_lowercase();
    rewards
        .into_iter()
        .any(|(id, t)| Some(id) != except && t.trim().to_lowercase() == title)
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

/// A reward made on Twitch.
pub(super) struct Created {
    id: String,
    /// It was to start paused, but pausing it failed: viewers can redeem it until it is paused.
    pause_failed: bool,
}

/// Makes a reward on Twitch after checking the limit and the title against `existing` (the channel's rewards as
/// `(id, title)`, read by the caller). Create Custom Reward takes no `is_paused`, so a paused one is paused right
/// after.
pub(super) async fn create_on_twitch(
    helix: &Helix,
    id: &str,
    existing: &[(&str, &str)],
    mut body: serde_json::Map<String, serde_json::Value>,
) -> Result<Created, CreateError> {
    if existing.len() >= MAX_REWARDS {
        return Err(CreateError::Limit);
    }
    let title = body
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or_default();
    if title_taken(existing.iter().copied(), title, None) {
        return Err(CreateError::DuplicateTitle);
    }
    let paused = body.remove("is_paused").and_then(|v| v.as_bool()) == Some(true);
    let created = helix
        .post_json_read(&rewards_path(id), &serde_json::Value::Object(body))
        .await
        .map_err(CreateError::Helix)?;
    let new_id = reward_list(created)
        .ok()
        .and_then(|l| l.into_iter().next())
        .map(|r| r.id)
        .ok_or(CreateError::Helix(HelixError::Auth(
            AuthError::InvalidResponse,
        )))?;
    let mut pause_failed = false;
    if paused {
        let pause = helix
            .patch_json(
                &format!("{}&id={new_id}", rewards_path(id)),
                &serde_json::json!({ "is_paused": true }),
            )
            .await;
        if let Err(e) = pause {
            tracing::warn!(error = %e, "a new reward could not be paused");
            pause_failed = true;
        }
    }
    Ok(Created {
        id: new_id,
        pause_failed,
    })
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
    let rewards = match channel_rewards(&helix, &id).await {
        Ok(list) => list,
        Err(e) => return list_error(e),
    };
    let existing: Vec<(&str, &str)> = rewards
        .iter()
        .map(|r| (r.id.as_str(), r.title.as_str()))
        .collect();
    match create_on_twitch(&helix, &id, &existing, patch).await {
        Ok(created) => Json(CreatedReward { id: created.id }).into_response(),
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
        match channel_rewards(&helix, &id).await {
            Ok(list)
                if title_taken(
                    list.iter().map(|r| (r.id.as_str(), r.title.as_str())),
                    title,
                    Some(&reward),
                ) =>
            {
                return error(StatusCode::CONFLICT, "rewards.duplicate_title");
            }
            Ok(_) => {}
            Err(e) => return list_error(e),
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct TakeoverBody {
    /// The new reward's title; Twitch refuses the original's while that one exists.
    title: String,
}

/// A setting that used the original reward.
#[derive(Serialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RewardUse {
    Checkin,
    Sound,
    Media,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TakenOver {
    /// The new reward's ID.
    id: String,
    /// Settings now pointing at the new reward.
    moved: Vec<RewardUse>,
    /// Settings that still point at the original: saving them failed, so the broadcaster picks the new reward there.
    not_moved: Vec<RewardUse>,
    /// The original is paused, but pausing the new reward failed: viewers can redeem it until it is paused here.
    pause_failed: bool,
}

#[utoipa::path(
    post,
    path = "/api/admin/rewards/{id}/takeover",
    tag = "admin",
    params(("id" = String, Path, description = "The reward made in the Twitch dashboard")),
    request_body = TakeoverBody,
    responses(
        (status = 200, body = TakenOver),
        (status = 400, description = "`rewards.invalid` (a bad title, or the reward is already the toolkit's), `rewards.refused`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`", body = ErrorBody),
        (status = 404, description = "`rewards.not_found`", body = ErrorBody),
        (status = 409, description = "`rewards.duplicate_title` (still the original's, or another reward's) or `rewards.limit`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
/// Makes a toolkit reward with the original's settings under a new title, then points check-in, the reward sound
/// and the reward video or GIF at it. The original is left as it is: the toolkit cannot change it.
pub(super) async fn take_over_reward(
    State(s): State<Arc<AdminState>>,
    Path(original): Path<String>,
    Json(body): Json<TakeoverBody>,
) -> Response {
    if !valid_twitch_id(&original) {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let rewards = match fetch_rewards(&helix, &id).await {
        Ok(list) => list,
        Err(e) => return list_error(e),
    };
    let Some(from) = rewards.iter().find(|r| r.id == original) else {
        return error(StatusCode::NOT_FOUND, "rewards.not_found");
    };
    if from.manageable {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let change = RewardChange {
        title: Some(body.title),
        prompt: Some(from.prompt.clone()),
        cost: Some(from.cost),
        background_color: Some(from.background_color.clone()).filter(|c| !c.is_empty()),
        is_enabled: Some(from.is_enabled),
        is_paused: Some(from.is_paused),
        is_user_input_required: Some(from.is_user_input_required),
        skip_queue: Some(from.skip_queue),
        cooldown_seconds: Some(from.cooldown_seconds.unwrap_or(0)),
        max_per_stream: Some(from.max_per_stream.unwrap_or(0)),
        max_per_user_per_stream: Some(from.max_per_user_per_stream.unwrap_or(0)),
    };
    let Some(patch) = change.helix_body() else {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    };
    let existing: Vec<(&str, &str)> = rewards
        .iter()
        .map(|r| (r.id.as_str(), r.title.as_str()))
        .collect();
    let Created {
        id: new_id,
        pause_failed,
    } = match create_on_twitch(&helix, &id, &existing, patch).await {
        Ok(created) => created,
        Err(e) => return e.response(),
    };
    let (mut moved, mut not_moved) = (Vec::new(), Vec::new());
    let mut note = |what: RewardUse, result: Result<bool, String>| match result {
        Ok(true) => moved.push(what),
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(error = %e, "a setting still points at a reward taken over");
            not_moved.push(what);
        }
    };
    note(
        RewardUse::Checkin,
        s.checkin.repoint_reward(&original, &new_id),
    );
    let (old_slot, new_slot) = (format!("reward.{original}"), format!("reward.{new_id}"));
    if let Some(sound) = s.sounds.slots().get(&old_slot).cloned() {
        let result = s
            .sounds
            .set_slot(&new_slot, Some(&sound))
            .and_then(|()| s.sounds.set_slot(&old_slot, None))
            .map(|()| true)
            .map_err(|e| e.to_string());
        note(RewardUse::Sound, result);
    }
    let rule = s
        .screen_tools
        .media_rules()
        .into_iter()
        .find(|r| r.reward_id == original);
    if let Some(mut rule) = rule {
        rule.reward_id = new_id.clone();
        let result = match s.screen_tools.set_media_rule(rule).await {
            Ok(_) => s
                .screen_tools
                .delete_media_rule(&original)
                .await
                .map(|()| true)
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        note(RewardUse::Media, result);
    }
    Json(TakenOver {
        id: new_id,
        moved,
        not_moved,
        pause_failed,
    })
    .into_response()
}

/// What a redemption becomes: fulfilled, or refunded (Twitch gives the points back).
#[derive(Deserialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RedemptionOutcome {
    Fulfilled,
    Canceled,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RedemptionStatusBody {
    reward_id: String,
    status: RedemptionOutcome,
}

#[utoipa::path(
    put,
    path = "/api/admin/redemptions/{twitch_id}/status",
    tag = "admin",
    params(("twitch_id" = String, Path, description = "Twitch's redemption ID")),
    request_body = RedemptionStatusBody,
    responses(
        (status = 204, description = "Done on Twitch; the record follows"),
        (status = 400, description = "`rewards.invalid`, or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` or `rewards.not_ours` (Twitch lets only the app that made a reward handle its redemptions)", body = ErrorBody),
        (status = 404, description = "`redemptions.not_pending`: not found, or already fulfilled or refunded", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
/// Marks a redemption of one of the toolkit's rewards fulfilled or refunds it on Twitch, then hands Twitch's answer to
/// the redemption records as an update, so the record changes at once (the EventSub update that follows is a
/// repeat).
pub(super) async fn set_redemption_status(
    State(s): State<Arc<AdminState>>,
    Path(redemption): Path<String>,
    Json(body): Json<RedemptionStatusBody>,
) -> Response {
    if !valid_twitch_id(&redemption) || !valid_twitch_id(&body.reward_id) {
        return error(StatusCode::BAD_REQUEST, "rewards.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match manager(&twitch) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let status = match body.status {
        RedemptionOutcome::Fulfilled => "FULFILLED",
        RedemptionOutcome::Canceled => "CANCELED",
    };
    let path = format!(
        "/channel_points/custom_rewards/redemptions?id={redemption}&broadcaster_id={id}&reward_id={}",
        body.reward_id
    );
    match helix
        .patch_json_read(&path, &serde_json::json!({ "status": status }))
        .await
    {
        Ok(answer) => {
            let updated = answer
                .get("data")
                .and_then(|d| d.get(0))
                .cloned()
                .and_then(integrations::twitch::eventsub::helix_redemption);
            match updated {
                Some(r) => s.redemption.receive(r, Source::Twitch),
                // Done on Twitch all the same; the EventSub update brings the record along.
                None => {
                    tracing::warn!("could not read Twitch's answer to a redemption status change")
                }
            }
            StatusCode::NO_CONTENT.into_response()
        }
        // Twitch answers 404 for a redemption that is no longer waiting, too.
        Err(HelixError::Refused(404)) => error(StatusCode::NOT_FOUND, "redemptions.not_pending"),
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
