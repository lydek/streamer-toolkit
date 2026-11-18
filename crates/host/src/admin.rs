//! Admin API under `/api/admin`, reachable only with the per-launch admin token.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use alerts::{AlertKindName, AlertsService};
use axum::{
    Json, Router,
    extract::{Path, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chat::ChatService;
use checkin::CheckinService;
use clips::ClipsService;
use integrations::{
    secrets::SecretStore,
    tls::{ClientConfig, HttpsClient},
    twitch::{
        MVP_SCOPES,
        auth::{AuthError, AuthStatus, DeviceLogin, TwitchAuth},
        eventsub::EventSub,
        helix::{Helix, HelixError},
    },
};
use redemption::RedemptionService;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sound::SoundService;
use tokio::{sync::RwLock, task::AbortHandle};
use toolkit_core::{
    events::{Alert, AlertKind, ChatMessage, Fragment, Source},
    modules::{self, ModuleError, ModuleState, Modules},
    outbox::Outbox,
    settings,
};

use utoipa::ToSchema;

use crate::TwitchEndpoints;

pub(crate) struct AdminState {
    token: SecretString,
    data_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    endpoints: TwitchEndpoints,
    http: HttpsClient,
    tls: Arc<ClientConfig>,
    chat: Arc<ChatService>,
    alerts: Arc<AlertsService>,
    redemption: Arc<RedemptionService>,
    checkin: Arc<CheckinService>,
    sounds: Arc<SoundService>,
    clips: Arc<ClipsService>,
    outbox: Arc<Outbox>,
    modules: Arc<Modules>,
    /// Shared with [`TwitchProfiles`], which looks up profile images with the same sign-in.
    twitch: Arc<RwLock<TwitchAuth>>,
    /// The EventSub task belongs to the current TwitchAuth and is replaced with it.
    eventsub: Mutex<Option<AbortHandle>>,
}

impl AdminState {
    // One argument per dependency the composition root hands over; a wrapper struct would only rename them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        token: SecretString,
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        endpoints: TwitchEndpoints,
        tls: Arc<ClientConfig>,
        http: HttpsClient,
        twitch: Arc<RwLock<TwitchAuth>>,
        chat: Arc<ChatService>,
        alerts: Arc<AlertsService>,
        redemption: Arc<RedemptionService>,
        checkin: Arc<CheckinService>,
        sounds: Arc<SoundService>,
        clips: Arc<ClipsService>,
        outbox: Arc<Outbox>,
        modules: Arc<Modules>,
    ) -> Self {
        let current = twitch.try_read().expect("not locked yet").clone();
        let state = Self {
            token,
            data_dir,
            secrets,
            endpoints,
            http,
            tls,
            chat,
            alerts,
            redemption,
            checkin,
            sounds,
            clips,
            outbox,
            modules,
            twitch,
            eventsub: Mutex::new(None),
        };
        state.start_eventsub(current);
        state
    }

    /// Connects chat (and its check-in command), alerts and redemptions (and their reward sounds) for `auth` (it waits until signed in) and stops the task of the previous auth.
    fn start_eventsub(&self, auth: TwitchAuth) {
        let chat = self.chat.clone();
        let alerts = self.alerts.clone();
        let redemption = self.redemption.clone();
        let checkin = self.checkin.clone();
        let sounds = self.sounds.clone();
        let task = tokio::spawn(
            EventSub {
                url: self.endpoints.eventsub.clone(),
                helix: Helix::new(&self.endpoints.helix, auth, self.http.clone()),
                tls: self.tls.clone(),
                deliver: Arc::new(move |m| {
                    checkin.receive_chat(&m, Source::Twitch);
                    chat.receive(m, Source::Twitch)
                }),
                deliver_alert: Arc::new(move |a| alerts.receive(a, Source::Twitch)),
                // Reward sounds take the in-memory path, not the outbox: a sound played late is worse than none.
                deliver_redemption: Arc::new(move |r| {
                    sounds.receive_redemption(&r);
                    redemption.receive(r, Source::Twitch)
                }),
            }
            .run(),
        );
        if let Some(old) = self.eventsub.lock().unwrap().replace(task.abort_handle()) {
            old.abort();
        }
    }
}

pub(crate) fn routes(state: Arc<AdminState>) -> Router {
    Router::new()
        .route("/api/admin/twitch", get(twitch_overview))
        .route("/api/admin/twitch/client-id", put(set_client_id))
        .route("/api/admin/twitch/login", post(login))
        .route("/api/admin/twitch/logout", post(logout))
        .route("/api/admin/general", get(general).put(set_general))
        .route("/api/admin/modules", get(list_modules))
        .route("/api/admin/modules/{id}", put(set_module))
        .route("/api/admin/checkin/rewards", get(checkin_rewards))
        .route("/api/admin/checkin/outbox", get(checkin_outbox))
        .route(
            "/api/admin/checkin/outbox/{event_id}/retry",
            post(retry_dead_letter),
        )
        .route(
            "/api/admin/checkin/outbox/{event_id}/skip",
            post(skip_dead_letter),
        )
        .route("/api/admin/simulate/chat", post(simulate_chat))
        .route("/api/admin/simulate/alert", post(simulate_alert))
        .route("/api/admin/simulate/checkin", post(simulate_checkin))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

pub(crate) async fn require_token(
    State(s): State<Arc<AdminState>>,
    req: Request,
    next: Next,
) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| {
            constant_time_eq(given.as_bytes(), s.token.expose_secret().as_bytes())
        });
    if !ok {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(req).await
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Overview {
    client_id: String,
    status: AuthStatus,
}

#[utoipa::path(get, path = "/api/admin/twitch", tag = "admin", responses((status = 200, body = Overview)))]
async fn twitch_overview(State(s): State<Arc<AdminState>>) -> Json<Overview> {
    Json(Overview {
        client_id: settings::load(&s.data_dir).twitch_client_id,
        status: s.twitch.read().await.status(),
    })
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ClientIdBody {
    client_id: String,
}

/// Twitch Client IDs are short ASCII alphanumerics; anything else is rejected before it reaches settings.
fn valid_client_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[utoipa::path(
    put,
    path = "/api/admin/twitch/client-id",
    tag = "admin",
    request_body = ClientIdBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn set_client_id(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<ClientIdBody>,
) -> Response {
    let client_id = body.client_id.trim().to_owned();
    if !valid_client_id(&client_id) {
        return error(StatusCode::BAD_REQUEST, "admin.invalid_client_id");
    }
    // Write lock first so two saves cannot interleave; an unchanged ID keeps the current sign-in.
    let mut twitch = s.twitch.write().await;
    if twitch.client_id() == client_id {
        return StatusCode::NO_CONTENT.into_response();
    }
    if settings::update(&s.data_dir, |set| set.twitch_client_id = client_id.clone()).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    // Tokens belong to the previous app registration, so a new Client ID starts signed out.
    let _ = twitch.logout();
    *twitch = TwitchAuth::new(
        client_id,
        &s.endpoints.oauth,
        s.http.clone(),
        s.secrets.clone(),
    );
    s.start_eventsub(twitch.clone());
    StatusCode::NO_CONTENT.into_response()
}

#[utoipa::path(
    post,
    path = "/api/admin/twitch/login",
    tag = "admin",
    responses(
        (status = 200, body = DeviceLogin),
        (status = 400, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
async fn login(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    match twitch.start_login(MVP_SCOPES).await {
        Ok(device) => Json(device).into_response(),
        Err(e) => auth_error(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/twitch/logout",
    tag = "admin",
    responses(
        (status = 204),
        (status = 500, description = "`twitch.storage_failed`: signed out, but the stored token could not be deleted, so the next start would sign in again", body = ErrorBody),
    )
)]
async fn logout(State(s): State<Arc<AdminState>>) -> Response {
    match s.twitch.read().await.logout() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => auth_error(e),
    }
}

/// Settings → General. The desktop app reads the same `settings.json` flag on every ✕ of the admin window.
#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct GeneralSettings {
    /// Show the close hint on ✕; the hint's "Don't show again" turns it off.
    ask_before_close: bool,
    /// Read only (ignored on PUT): `settings.json` exists but cannot be read or parsed. Defaults are in use and
    /// no setting saves (`settings.save_failed`) until the file is fixed or removed.
    #[serde(default)]
    settings_unreadable: bool,
}

#[utoipa::path(get, path = "/api/admin/general", tag = "admin", responses((status = 200, body = GeneralSettings)))]
async fn general(State(s): State<Arc<AdminState>>) -> Json<GeneralSettings> {
    let read = settings::read(&s.data_dir);
    Json(GeneralSettings {
        settings_unreadable: read.is_err(),
        ask_before_close: !read.unwrap_or_default().skip_close_hint,
    })
}

#[utoipa::path(
    put,
    path = "/api/admin/general",
    tag = "admin",
    request_body = GeneralSettings,
    responses((status = 204), (status = 500, body = ErrorBody))
)]
async fn set_general(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<GeneralSettings>,
) -> Response {
    match settings::update(&s.data_dir, |set| {
        set.skip_close_hint = !body.ask_before_close
    }) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed"),
    }
}

#[utoipa::path(get, path = "/api/admin/modules", tag = "admin", responses((status = 200, body = Vec<ModuleState>)))]
async fn list_modules(State(s): State<Arc<AdminState>>) -> Response {
    Json(s.modules.list()).into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ModuleBody {
    enabled: bool,
    /// With `enabled: false`: skip the module's outbox events instead of keeping them for later ("close"; the
    /// default pauses). Enabling a closed module never delivers what was published while it was closed.
    #[serde(default)]
    close: bool,
}

#[utoipa::path(
    put,
    path = "/api/admin/modules/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = ModuleBody,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn set_module(
    State(s): State<Arc<AdminState>>,
    Path(id): Path<String>,
    Json(body): Json<ModuleBody>,
) -> Response {
    if body.enabled
        && s.modules.is_closed(&id)
        && let Err(e) = s.outbox.skip_pending(&id).await
    {
        tracing::error!(code = "outbox.storage_failed", error = %e, "closed-period events not skipped");
        return error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed");
    }
    let result = match (body.enabled, body.close) {
        (false, true) => s.modules.close(&id),
        (enabled, _) => s.modules.set_enabled(&id, enabled),
    };
    s.outbox.wake();
    match result {
        Ok(()) => {
            // Modules has no change signal, so the clip player is told here to stop what it is playing.
            if !body.enabled && id == modules::CLIPS {
                s.clips.stop();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e @ ModuleError::Unknown) => error(StatusCode::NOT_FOUND, &e.to_string()),
        Err(e @ ModuleError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}

/// A channel points reward of the signed-in channel, for picking the check-in reward. `title` is raw Twitch text.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct Reward {
    id: String,
    title: String,
}

#[derive(Deserialize)]
struct RewardList {
    data: Vec<Reward>,
}

/// Helix Get Custom Reward (scope `channel:read:redemptions`). A channel without channel points is refused by
/// Twitch: 502 `twitch.request_refused`.
#[utoipa::path(
    get,
    path = "/api/admin/checkin/rewards",
    tag = "admin",
    responses(
        (status = 200, body = Vec<Reward>),
        (status = 409, description = "`twitch.client_id_required` or `twitch.sign_in_required`", body = ErrorBody),
        (status = 502, description = "`twitch.request_refused`, `twitch.rate_limited` or `twitch.unreachable`", body = ErrorBody),
    )
)]
async fn checkin_rewards(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    let Some(user_id) = twitch
        .token()
        .map(|t| t.user_id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match helix
        .get_json(&format!(
            "/channel_points/custom_rewards?broadcaster_id={user_id}"
        ))
        .await
    {
        Ok(body) => match serde_json::from_value::<RewardList>(body) {
            Ok(list) => Json(list.data).into_response(),
            Err(_) => auth_error(AuthError::InvalidResponse),
        },
        Err(HelixError::Auth(e)) => auth_error(e),
        Err(e @ (HelixError::RateLimited(_) | HelixError::Refused(_))) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

/// Redemptions waiting for the check-in module (while it is paused), and those it failed to take.
#[derive(Serialize, ToSchema)]
pub(crate) struct CheckinOutbox {
    pending: u64,
    /// RFC 3339: when the oldest waiting redemption happened.
    oldest_pending_at: Option<String>,
    dead_letters: Vec<CheckinDeadLetter>,
}

/// A redemption the check-in module failed to take five times in a row. `user_name` and `reward_title` are raw
/// Twitch text.
#[derive(Serialize, ToSchema)]
pub(crate) struct CheckinDeadLetter {
    event_id: String,
    /// RFC 3339: when the redemption happened.
    occurred_at: String,
    /// RFC 3339: the last failed attempt.
    failed_at: String,
    attempts: i64,
    error: String,
    user_name: Option<String>,
    reward_title: Option<String>,
}

fn rfc3339(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/outbox",
    tag = "admin",
    responses((status = 200, body = CheckinOutbox), (status = 500, description = "`outbox.storage_failed`", body = ErrorBody))
)]
async fn checkin_outbox(State(s): State<Arc<AdminState>>) -> Response {
    let read = async {
        let backlog = s.outbox.backlog(modules::CHECKIN).await?;
        let dead = s.outbox.dead_letters(modules::CHECKIN).await?;
        Ok::<_, sqlx::Error>((backlog, dead))
    };
    match read.await {
        Ok((backlog, dead)) => Json(CheckinOutbox {
            pending: backlog.pending,
            oldest_pending_at: backlog.oldest.map(rfc3339),
            dead_letters: dead
                .into_iter()
                .map(|d| {
                    let r =
                        serde_json::from_value::<redemption::RedemptionRecorded>(d.event.payload)
                            .ok();
                    CheckinDeadLetter {
                        event_id: d.event.id,
                        occurred_at: rfc3339(d.event.occurred_at),
                        failed_at: rfc3339(d.failed_at),
                        attempts: d.attempts,
                        error: d.last_error,
                        user_name: r.as_ref().map(|r| r.user_name.clone()),
                        reward_title: r.map(|r| r.reward_title),
                    }
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(code = "outbox.storage_failed", error = %e, "outbox not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed")
        }
    }
}

fn dead_letter_result(result: sqlx::Result<bool>) -> Response {
    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "outbox.not_found"),
        Err(e) => {
            tracing::error!(code = "outbox.storage_failed", error = %e, "dead letter not changed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed")
        }
    }
}

/// Puts a dead letter back in line with a fresh set of attempts.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/outbox/{event_id}/retry",
    tag = "admin",
    params(("event_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`outbox.not_found`: no such dead letter", body = ErrorBody),
        (status = 500, description = "`outbox.storage_failed`", body = ErrorBody),
    )
)]
async fn retry_dead_letter(
    State(s): State<Arc<AdminState>>,
    Path(event_id): Path<String>,
) -> Response {
    dead_letter_result(s.outbox.retry(modules::CHECKIN, &event_id).await)
}

/// Gives up on a dead letter: that redemption never checks in.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/outbox/{event_id}/skip",
    tag = "admin",
    params(("event_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`outbox.not_found`: no such dead letter", body = ErrorBody),
        (status = 500, description = "`outbox.storage_failed`", body = ErrorBody),
    )
)]
async fn skip_dead_letter(
    State(s): State<Arc<AdminState>>,
    Path(event_id): Path<String>,
) -> Response {
    dead_letter_result(s.outbox.skip(modules::CHECKIN, &event_id).await)
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedChat {
    #[serde(default)]
    user_name: String,
    text: String,
}

const SIMULATED_TEXT_MAX: usize = 500;

/// Sends a chat message tagged `source=simulation` through the same path as Twitch chat.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/chat",
    tag = "admin",
    request_body = SimulatedChat,
    responses((status = 204), (status = 400, body = ErrorBody))
)]
async fn simulate_chat(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<SimulatedChat>,
) -> Response {
    let text = body.text.trim();
    if text.is_empty() || text.chars().count() > SIMULATED_TEXT_MAX {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_text");
    }
    let user_name = match body.user_name.trim() {
        "" => "Simulator",
        name => name,
    };
    s.chat.receive(
        ChatMessage {
            id: uuid::Uuid::now_v7().to_string(),
            user_id: "simulator".into(),
            user_login: "simulator".into(),
            user_name: user_name.chars().take(25).collect(),
            color: None,
            text: text.into(),
            fragments: vec![Fragment::Text(text.into())],
            sent_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
        Source::Simulation,
    );
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedAlert {
    kind: AlertKindName,
    /// Up to 25 characters; empty means "Simulator".
    #[serde(default)]
    user_name: String,
    /// Months (`resubscribe`), subs (`gift`, 1 to 100), Bits (`cheer`) or raiders (`raid`), 1 to 100000;
    /// ignored for the other kinds.
    #[serde(default)]
    amount: Option<u32>,
    /// Up to 500 characters (`resubscribe`, `cheer`); ignored for the other kinds.
    #[serde(default)]
    message: String,
}

const SIMULATED_NAME_MAX: usize = 25;
const SIMULATED_GIFT_MAX: u32 = 100;
const SIMULATED_AMOUNT_MAX: u32 = 100_000;

/// The alerts Twitch would send. A gift also brings one gifted sub per recipient, so the simulator exercises
/// the same grouping as real gifts.
fn simulated_alerts(body: SimulatedAlert) -> Result<Vec<Alert>, &'static str> {
    let name = body.user_name.trim();
    if name.chars().count() > SIMULATED_NAME_MAX {
        return Err("simulate.invalid_name");
    }
    let message = body.message.trim().to_owned();
    if message.chars().count() > SIMULATED_TEXT_MAX {
        return Err("simulate.invalid_text");
    }
    let max = match body.kind {
        AlertKindName::Gift => SIMULATED_GIFT_MAX,
        _ => SIMULATED_AMOUNT_MAX,
    };
    let amount = || {
        body.amount
            .filter(|n| (1..=max).contains(n))
            .ok_or("simulate.invalid_amount")
    };
    let kind = match body.kind {
        AlertKindName::Follow => AlertKind::Follow,
        AlertKindName::Subscribe => AlertKind::Subscribe {
            tier: 1,
            gifted: false,
        },
        AlertKindName::Resubscribe => AlertKind::Resubscribe {
            tier: 1,
            months: amount()?,
            message,
        },
        AlertKindName::Gift => AlertKind::Gift {
            tier: 1,
            count: amount()?,
        },
        AlertKindName::Cheer => AlertKind::Cheer {
            bits: amount()?,
            message,
        },
        AlertKindName::Raid => AlertKind::Raid { viewers: amount()? },
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let alert = |user_name: Option<String>, kind| Alert {
        id: uuid::Uuid::now_v7().to_string(),
        user_name,
        kind,
        occurred_at: now.clone(),
    };
    let recipients = match kind {
        AlertKind::Gift { count, .. } => count,
        _ => 0,
    };
    let mut alerts = vec![alert(
        Some(if name.is_empty() { "Simulator" } else { name }.into()),
        kind,
    )];
    alerts.extend((1..=recipients).map(|n| {
        alert(
            Some(format!("Recipient{n}")),
            AlertKind::Subscribe {
                tier: 1,
                gifted: true,
            },
        )
    }));
    Ok(alerts)
}

/// Sends a follow, sub, resub, gift, cheer or raid tagged `source=simulation` through the same path as Twitch.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/alert",
    tag = "admin",
    request_body = SimulatedAlert,
    responses((status = 204), (status = 400, body = ErrorBody))
)]
async fn simulate_alert(
    State(s): State<Arc<AdminState>>,
    body: Result<Json<SimulatedAlert>, JsonRejection>,
) -> Response {
    // An unknown kind or a malformed body gets an error code instead of axum's plain-text rejection.
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_alert");
    };
    match simulated_alerts(body) {
        Ok(alerts) => {
            for alert in alerts {
                s.alerts.receive(alert, Source::Simulation);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(code) => error(StatusCode::BAD_REQUEST, code),
    }
}

/// Profile images for the stamp card overlay, from Helix Get Users with the broadcaster's sign-in.
pub(crate) struct TwitchProfiles {
    pub(crate) helix: String,
    pub(crate) http: HttpsClient,
    pub(crate) twitch: Arc<RwLock<TwitchAuth>>,
}

impl checkin::ProfileImages for TwitchProfiles {
    fn profile_image<'a>(
        &'a self,
        twitch_user_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async move {
            if twitch_user_id.is_empty() || !twitch_user_id.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let auth = self.twitch.read().await.clone();
            let body = Helix::new(&self.helix, auth, self.http.clone())
                .get_json(&format!("/users?id={twitch_user_id}"))
                .await
                .ok()?;
            body["data"][0]["profile_image_url"]
                .as_str()
                .map(str::to_owned)
        })
    }
}

/// Clips for the SO clip queue, from Helix Get Clips with the broadcaster's sign-in (no scope needed).
pub(crate) struct TwitchClips {
    pub(crate) helix: String,
    pub(crate) http: HttpsClient,
    pub(crate) twitch: Arc<RwLock<TwitchAuth>>,
}

impl clips::ClipLookup for TwitchClips {
    fn clip<'a>(
        &'a self,
        id: &'a clips::ClipId,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<clips::Clip, clips::LookupError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let auth = self.twitch.read().await.clone();
            // `ClipId` only holds letters, digits, `_` and `-`, so it needs no escaping.
            let body = Helix::new(&self.helix, auth, self.http.clone())
                .get_json(&format!("/clips?id={}", id.as_str()))
                .await
                .map_err(|e| {
                    let status = match &e {
                        HelixError::Auth(a) => auth_status(a),
                        HelixError::RateLimited(_) | HelixError::Refused(_) => {
                            StatusCode::BAD_GATEWAY
                        }
                    };
                    clips::LookupError::Twitch {
                        status: status.as_u16(),
                        code: e.to_string(),
                    }
                })?;
            clip_from_helix(&body)
        })
    }
}

/// The first clip of a Get Clips response; an empty `data` means Twitch has no such clip.
fn clip_from_helix(body: &serde_json::Value) -> Result<clips::Clip, clips::LookupError> {
    let invalid = || clips::LookupError::Twitch {
        status: auth_status(&AuthError::InvalidResponse).as_u16(),
        code: AuthError::InvalidResponse.to_string(),
    };
    let data = body["data"].as_array().ok_or_else(invalid)?;
    let Some(clip) = data.first() else {
        return Err(clips::LookupError::NotFound);
    };
    let text = |key: &str| clip[key].as_str().map(str::to_owned).ok_or_else(invalid);
    let duration = clip["duration"]
        .as_f64()
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or_else(invalid)?;
    Ok(clips::Clip {
        id: text("id")?,
        title: text("title")?,
        broadcaster_name: text("broadcaster_name")?,
        duration_ms: (duration * 1000.0).round() as u64,
    })
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedCheckin {
    #[serde(default)]
    user_name: String,
    /// Stamps on the card, 1 to its slots; all of them shows the full-card effect.
    stamps: u32,
}

/// Shows a stamp card on `/overlay/checkin/` tagged `source=simulation`; nothing is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/checkin",
    tag = "admin",
    request_body = SimulatedCheckin,
    responses((status = 204), (status = 400, description = "`simulate.invalid_checkin` or `simulate.invalid_stamps`", body = ErrorBody))
)]
async fn simulate_checkin(
    State(s): State<Arc<AdminState>>,
    body: Result<Json<SimulatedCheckin>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_checkin");
    };
    let name: String = match body.user_name.trim() {
        "" => "Simulator".into(),
        name => name.chars().take(25).collect(),
    };
    if s.checkin.simulate_card(&name, body.stamps) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        error(StatusCode::BAD_REQUEST, "simulate.invalid_stamps")
    }
}

fn auth_status(e: &AuthError) -> StatusCode {
    match e {
        AuthError::ClientIdRequired | AuthError::SignInRequired => StatusCode::CONFLICT,
        AuthError::Unreachable => StatusCode::BAD_GATEWAY,
        AuthError::StorageFailed => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

fn auth_error(e: AuthError) -> Response {
    error(auth_status(&e), &e.to_string())
}

/// Body of the errors admin handlers return: a stable `<area>.<reason>` code the admin UI translates.
/// The missing-token 401 has an empty body, and malformed JSON gets axum's plain-text rejection.
#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

pub(crate) fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_validation() {
        assert!(valid_client_id("abc123XYZ"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("has space"));
        assert!(!valid_client_id("<script>"));
        assert!(!valid_client_id(&"a".repeat(65)));
    }

    fn simulate(json: serde_json::Value) -> Result<Vec<Alert>, &'static str> {
        simulated_alerts(serde_json::from_value(json).unwrap())
    }

    #[test]
    fn simulated_alerts_cover_every_kind_and_validate_input() {
        let kinds =
            |json| simulate(json).map(|a| a.into_iter().map(|a| a.kind).collect::<Vec<_>>());
        assert_eq!(
            kinds(serde_json::json!({"kind":"follow"})),
            Ok(vec![AlertKind::Follow])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"subscribe"})),
            Ok(vec![AlertKind::Subscribe {
                tier: 1,
                gifted: false
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"resubscribe","amount":12,"message":" hi "})),
            Ok(vec![AlertKind::Resubscribe {
                tier: 1,
                months: 12,
                message: "hi".into()
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"cheer","amount":100})),
            Ok(vec![AlertKind::Cheer {
                bits: 100,
                message: String::new()
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"raid","amount":42})),
            Ok(vec![AlertKind::Raid { viewers: 42 }])
        );

        let gift =
            simulate(serde_json::json!({"kind":"gift","amount":3,"userName":"Viewer"})).unwrap();
        assert_eq!(gift[0].kind, AlertKind::Gift { tier: 1, count: 3 });
        assert_eq!(gift[0].user_name.as_deref(), Some("Viewer"));
        assert_eq!(gift.len(), 4, "the gift and one gifted sub per recipient");
        assert!(gift[1..].iter().all(|a| a.kind
            == AlertKind::Subscribe {
                tier: 1,
                gifted: true
            }));
        let follow = simulate(serde_json::json!({"kind":"follow","userName":"  "})).unwrap();
        assert_eq!(follow[0].user_name.as_deref(), Some("Simulator"));

        for (json, code) in [
            (
                serde_json::json!({"kind":"follow","userName":"x".repeat(26)}),
                "simulate.invalid_name",
            ),
            (
                serde_json::json!({"kind":"cheer","amount":1,"message":"x".repeat(501)}),
                "simulate.invalid_text",
            ),
            (
                serde_json::json!({"kind":"gift","amount":101}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"raid","amount":0}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"cheer","amount":100_001}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"resubscribe"}),
                "simulate.invalid_amount",
            ),
        ] {
            assert_eq!(simulate(json).unwrap_err(), code);
        }
    }

    #[test]
    fn constant_time_eq_matches_only_equal_input() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokem"));
        assert!(!constant_time_eq(b"token", b"tok"));
    }

    #[test]
    fn get_clips_responses_become_clips_or_errors() {
        let clip = serde_json::json!({ "data": [{
            "id": "CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4", "title": "<b>F1</b>",
            "broadcaster_name": "Twitch", "duration": 28.4, "embed_url": "https://example.com/" }] });
        assert_eq!(
            clip_from_helix(&clip),
            Ok(clips::Clip {
                id: "CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4".into(),
                title: "<b>F1</b>".into(),
                broadcaster_name: "Twitch".into(),
                duration_ms: 28_400,
            })
        );
        assert_eq!(
            clip_from_helix(&serde_json::json!({ "data": [] })),
            Err(clips::LookupError::NotFound)
        );
        let invalid = Err(clips::LookupError::Twitch {
            status: 400,
            code: "twitch.invalid_response".into(),
        });
        for body in [
            serde_json::json!({}),
            serde_json::json!({ "data": [{ "id": "a", "title": "t", "broadcaster_name": "b" }] }),
            serde_json::json!({ "data": [{ "id": "a", "title": "t", "broadcaster_name": "b", "duration": -1 }] }),
            serde_json::json!({ "data": [{ "id": "a", "broadcaster_name": "b", "duration": 5 }] }),
        ] {
            assert_eq!(clip_from_helix(&body), invalid, "{body}");
        }
    }
}
