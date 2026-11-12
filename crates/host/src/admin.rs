//! Admin API under `/api/admin`, reachable only with the per-launch admin token.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use integrations::{
    secrets::SecretStore,
    tls::HttpsClient,
    twitch::{
        MVP_SCOPES,
        auth::{AuthError, AuthStatus, TwitchAuth},
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::settings;

pub(crate) struct AdminState {
    token: SecretString,
    config_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    oauth_base: String,
    http: HttpsClient,
    twitch: RwLock<TwitchAuth>,
}

impl AdminState {
    pub(crate) fn new(
        token: SecretString,
        config_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        oauth_base: String,
        http: HttpsClient,
    ) -> Self {
        let client_id = settings::load(&config_dir).twitch_client_id;
        let twitch = TwitchAuth::new(client_id, &oauth_base, http.clone(), secrets.clone());
        Self {
            token,
            config_dir,
            secrets,
            oauth_base,
            http,
            twitch: RwLock::new(twitch),
        }
    }
}

pub(crate) fn routes(state: Arc<AdminState>) -> Router {
    Router::new()
        .route("/api/admin/twitch", get(twitch_overview))
        .route("/api/admin/twitch/client-id", put(set_client_id))
        .route("/api/admin/twitch/login", post(login))
        .route("/api/admin/twitch/logout", post(logout))
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

#[derive(Serialize)]
struct Overview {
    client_id: String,
    status: AuthStatus,
}

async fn twitch_overview(State(s): State<Arc<AdminState>>) -> Json<Overview> {
    Json(Overview {
        client_id: settings::load(&s.config_dir).twitch_client_id,
        status: s.twitch.read().await.status(),
    })
}

#[derive(Deserialize)]
struct ClientIdBody {
    client_id: String,
}

/// Twitch Client IDs are short ASCII alphanumerics; anything else is rejected before it reaches settings.
fn valid_client_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

async fn set_client_id(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<ClientIdBody>,
) -> Response {
    let client_id = body.client_id.trim().to_owned();
    if !valid_client_id(&client_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_client_id");
    }
    if settings::update(&s.config_dir, |set| {
        set.twitch_client_id = client_id.clone()
    })
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "save_failed");
    }
    // Tokens belong to the previous app registration, so a new Client ID starts signed out.
    let mut twitch = s.twitch.write().await;
    let _ = twitch.logout();
    *twitch = TwitchAuth::new(client_id, &s.oauth_base, s.http.clone(), s.secrets.clone());
    StatusCode::NO_CONTENT.into_response()
}

async fn login(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    match twitch.start_login(MVP_SCOPES).await {
        Ok(device) => Json(device).into_response(),
        Err(e) => auth_error(e),
    }
}

async fn logout(State(s): State<Arc<AdminState>>) -> Response {
    match s.twitch.read().await.logout() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => auth_error(e),
    }
}

fn auth_error(e: AuthError) -> Response {
    let status = match e {
        AuthError::ClientIdRequired | AuthError::SignInRequired => StatusCode::CONFLICT,
        AuthError::Unreachable => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(serde_json::json!({ "code": e.to_string() }))).into_response()
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(serde_json::json!({ "code": code }))).into_response()
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
