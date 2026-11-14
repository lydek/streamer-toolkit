//! Admin API under `/api/admin`, reachable only with the per-launch admin token.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chat::ChatService;
use integrations::{
    secrets::SecretStore,
    tls::{ClientConfig, HttpsClient},
    twitch::{
        MVP_SCOPES,
        auth::{AuthError, AuthStatus, DeviceLogin, TwitchAuth},
        eventsub::EventSub,
        helix::Helix,
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::{sync::RwLock, task::AbortHandle};
use toolkit_core::{
    events::{ChatMessage, Fragment, Source},
    modules::{ModuleError, ModuleState, Modules},
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
    modules: Arc<Modules>,
    twitch: RwLock<TwitchAuth>,
    /// The EventSub task belongs to the current TwitchAuth and is replaced with it.
    eventsub: Mutex<Option<AbortHandle>>,
}

impl AdminState {
    pub(crate) fn new(
        token: SecretString,
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        endpoints: TwitchEndpoints,
        tls: Arc<ClientConfig>,
        chat: Arc<ChatService>,
        modules: Arc<Modules>,
    ) -> Self {
        let http = integrations::tls::https_client(tls.clone());
        let client_id = settings::load(&data_dir).twitch_client_id;
        let twitch = TwitchAuth::new(client_id, &endpoints.oauth, http.clone(), secrets.clone());
        let state = Self {
            token,
            data_dir,
            secrets,
            endpoints,
            http,
            tls,
            chat,
            modules,
            twitch: RwLock::new(twitch.clone()),
            eventsub: Mutex::new(None),
        };
        state.start_eventsub(twitch);
        state
    }

    /// Connects chat for `auth` (it waits until signed in) and stops the task of the previous auth.
    fn start_eventsub(&self, auth: TwitchAuth) {
        let chat = self.chat.clone();
        let task = tokio::spawn(
            EventSub {
                url: self.endpoints.eventsub.clone(),
                helix: Helix::new(&self.endpoints.helix, auth, self.http.clone()),
                tls: self.tls.clone(),
                deliver: Arc::new(move |m| chat.receive(m, Source::Twitch)),
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
        .route("/api/admin/modules", get(list_modules))
        .route("/api/admin/modules/{id}", put(set_module))
        .route("/api/admin/simulate/chat", post(simulate_chat))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

async fn require_token(State(s): State<Arc<AdminState>>, req: Request, next: Next) -> Response {
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
    // TwitchAuth::logout never fails today; auth_error stays as the guard if that changes.
    responses((status = 204))
)]
async fn logout(State(s): State<Arc<AdminState>>) -> Response {
    match s.twitch.read().await.logout() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => auth_error(e),
    }
}

#[utoipa::path(get, path = "/api/admin/modules", tag = "admin", responses((status = 200, body = Vec<ModuleState>)))]
async fn list_modules(State(s): State<Arc<AdminState>>) -> Response {
    Json(s.modules.list()).into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ModuleBody {
    enabled: bool,
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
    match s.modules.set_enabled(&id, body.enabled) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e @ ModuleError::Unknown) => error(StatusCode::NOT_FOUND, &e.to_string()),
        Err(e @ ModuleError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
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

fn auth_error(e: AuthError) -> Response {
    let status = match e {
        AuthError::ClientIdRequired | AuthError::SignInRequired => StatusCode::CONFLICT,
        AuthError::Unreachable => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_REQUEST,
    };
    error(status, &e.to_string())
}

/// Body of the errors admin handlers return: a stable `<area>.<reason>` code the admin UI translates.
/// The missing-token 401 has an empty body, and malformed JSON gets axum's plain-text rejection.
#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
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

    #[test]
    fn constant_time_eq_matches_only_equal_input() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokem"));
        assert!(!constant_time_eq(b"token", b"tok"));
    }
}
