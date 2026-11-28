//! Twitch sign-in with the Device Code Flow (public client, no client secret) and single-flight token refresh.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use http::{Method, Request, header};
use http_body_util::{BodyExt, Full};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{secrets::SecretStore, tls::HttpsClient};

pub const OAUTH_BASE: &str = "https://id.twitch.tv/oauth2";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// How long a device code can be used (Twitch documents 30 minutes).
const DEVICE_CODE_LIFETIME: Duration = Duration::from_secs(30 * 60);
/// How long to poll for a device code: what Twitch says, but never longer than a device code lives.
fn code_lifetime(expires_in_secs: u64) -> Duration {
    Duration::from_secs(expires_in_secs).min(DEVICE_CODE_LIFETIME)
}

/// A Twitch request with no answer by then fails as unreachable, so a dead connection (sleep, a network switch)
/// never holds the refresh lock or EventSub forever.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Stable error codes; the admin UI translates them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("twitch.client_id_required")]
    ClientIdRequired,
    #[error("twitch.sign_in_required")]
    SignInRequired,
    #[error("twitch.sign_in_expired")]
    SignInExpired,
    #[error("twitch.sign_in_refused")]
    SignInRefused,
    #[error("twitch.unreachable")]
    Unreachable,
    #[error("twitch.invalid_response")]
    InvalidResponse,
    #[error("twitch.storage_failed")]
    StorageFailed,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct DeviceLogin {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_secs: u64,
}

/// What the admin UI may see; never contains tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthStatus {
    SignedOut,
    Pending {
        user_code: String,
        verification_uri: String,
    },
    SignedIn {
        user_id: String,
        login: String,
    },
    Failed {
        code: String,
    },
}

#[derive(Clone)]
pub struct Token {
    pub access: SecretString,
    refresh: SecretString,
    pub user_id: String,
    pub login: String,
    pub scopes: Vec<String>,
}

/// Serialized form kept only in the secret store.
#[derive(Serialize, Deserialize)]
struct StoredToken {
    /// The app registration the token was issued to; empty for tokens stored before this field existed.
    #[serde(default)]
    client_id: String,
    access_token: String,
    refresh_token: String,
    user_id: String,
    login: String,
    scopes: Vec<String>,
}

impl Token {
    fn to_stored(&self, client_id: &str) -> StoredToken {
        StoredToken {
            client_id: client_id.into(),
            access_token: self.access.expose_secret().into(),
            refresh_token: self.refresh.expose_secret().into(),
            user_id: self.user_id.clone(),
            login: self.login.clone(),
            scopes: self.scopes.clone(),
        }
    }

    fn from_stored(s: StoredToken) -> Self {
        Self {
            access: s.access_token.into(),
            refresh: s.refresh_token.into(),
            user_id: s.user_id,
            login: s.login,
            scopes: s.scopes,
        }
    }
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct ValidateResponse {
    user_id: String,
    login: String,
    #[serde(default)]
    scopes: Vec<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    #[serde(default)]
    message: String,
}

#[derive(Default)]
struct State {
    token: Option<Token>,
    status_override: Option<AuthStatus>,
    /// Bumped on every new login or logout so a stale polling task cannot overwrite newer state.
    generation: u64,
}

struct Inner {
    client_id: String,
    oauth_base: String,
    http: HttpsClient,
    store: Arc<dyn SecretStore>,
    state: Mutex<State>,
    refresh_lock: tokio::sync::Mutex<()>,
    helix_resets: Mutex<HashMap<String, tokio::time::Instant>>,
    /// Signed-in user ID; adapters such as EventSub watch it to connect after login and stop after logout.
    account: tokio::sync::watch::Sender<Option<String>>,
}

#[derive(Clone)]
pub struct TwitchAuth(Arc<Inner>);

impl TwitchAuth {
    /// Loads a previously stored token; an unreadable entry, a missing Client ID or a token issued to another
    /// Client ID counts as signed out. The entry itself is left alone, so nothing is deleted by mistake.
    pub fn new(
        client_id: String,
        oauth_base: &str,
        http: HttpsClient,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        let token = store
            .read()
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<StoredToken>(&json).ok())
            .filter(|t| {
                !client_id.is_empty() && (t.client_id.is_empty() || t.client_id == client_id)
            })
            .map(Token::from_stored);
        let account = tokio::sync::watch::Sender::new(token.as_ref().map(|t| t.user_id.clone()));
        Self(Arc::new(Inner {
            client_id,
            oauth_base: oauth_base.trim_end_matches('/').into(),
            http,
            store,
            state: Mutex::new(State {
                token,
                ..State::default()
            }),
            refresh_lock: tokio::sync::Mutex::new(()),
            helix_resets: Mutex::default(),
            account,
        }))
    }

    pub fn client_id(&self) -> &str {
        &self.0.client_id
    }

    /// Changes whenever the user signs in or out.
    pub fn account(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.0.account.subscribe()
    }

    pub fn token(&self) -> Option<Token> {
        self.0.state.lock().unwrap().token.clone()
    }

    pub(super) fn helix_wait(&self, user_id: &str) -> Option<Duration> {
        let now = tokio::time::Instant::now();
        let mut resets = self.0.helix_resets.lock().unwrap();
        resets.retain(|_, until| *until > now);
        resets.get(user_id).map(|until| *until - now)
    }

    pub(super) fn pause_helix(&self, user_id: &str, wait: Duration) {
        let until = tokio::time::Instant::now() + wait;
        let mut resets = self.0.helix_resets.lock().unwrap();
        let reset = resets.entry(user_id.into()).or_insert(until);
        *reset = (*reset).max(until);
    }

    pub fn status(&self) -> AuthStatus {
        let state = self.0.state.lock().unwrap();
        if let Some(s) = &state.status_override {
            return s.clone();
        }
        match &state.token {
            // Still signed in (chat keeps working), but the newer scopes need one more sign-in.
            Some(t)
                if !super::MVP_SCOPES
                    .iter()
                    .all(|s| t.scopes.iter().any(|g| g == s)) =>
            {
                AuthStatus::Failed {
                    code: "twitch.scopes_missing".into(),
                }
            }
            Some(t) => AuthStatus::SignedIn {
                user_id: t.user_id.clone(),
                login: t.login.clone(),
            },
            None => AuthStatus::SignedOut,
        }
    }

    /// Starts the Device Code Flow and polls in the background until the user approves or the code expires.
    pub async fn start_login(&self, scopes: &[&str]) -> Result<DeviceLogin, AuthError> {
        if self.0.client_id.is_empty() {
            return Err(AuthError::ClientIdRequired);
        }
        // This attempt's number, taken before asking Twitch, so a logout or a newer login meanwhile wins over its
        // answer; the code an older attempt showed goes away at once.
        let generation = {
            let mut state = self.0.state.lock().unwrap();
            state.generation += 1;
            state.status_override = None;
            state.generation
        };
        let scope_list = scopes.join(" ");
        let (status, body) = self
            .form(
                "device",
                &[("client_id", &self.0.client_id), ("scopes", &scope_list)],
            )
            .await?;
        if !(200..300).contains(&status) {
            return Err(AuthError::SignInRefused);
        }
        let device: DeviceResponse = parse(&body)?;
        let login = DeviceLogin {
            user_code: device.user_code.clone(),
            verification_uri: device.verification_uri.clone(),
            expires_in_secs: device.expires_in,
        };

        {
            let mut state = self.0.state.lock().unwrap();
            if state.generation != generation {
                // Superseded while Twitch answered: nothing shows this code or asks for its token.
                return Err(AuthError::SignInExpired);
            }
            state.status_override = Some(AuthStatus::Pending {
                user_code: device.user_code,
                verification_uri: device.verification_uri,
            });
        }
        let auth = self.clone();
        tokio::spawn(async move {
            let result = auth
                .poll(
                    &device.device_code,
                    &scope_list,
                    device.interval,
                    device.expires_in,
                    generation,
                )
                .await;
            let mut state = auth.0.state.lock().unwrap();
            if state.generation == generation {
                state.status_override = result.err().map(|e| AuthStatus::Failed {
                    code: e.to_string(),
                });
            }
        });
        Ok(login)
    }

    async fn poll(
        &self,
        device_code: &str,
        scopes: &str,
        interval_secs: u64,
        expires_in_secs: u64,
        generation: u64,
    ) -> Result<(), AuthError> {
        let deadline = tokio::time::Instant::now() + code_lifetime(expires_in_secs);
        // Never faster than once a second, whatever Twitch sends.
        let interval = Duration::from_secs(interval_secs.max(1));
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                break;
            }
            tokio::time::sleep(interval.min(left)).await;
            // Superseded by a logout or a newer login: no more questions to Twitch.
            if self.0.state.lock().unwrap().generation != generation {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            let fields = [
                ("client_id", self.0.client_id.as_str()),
                ("scopes", scopes),
                ("device_code", device_code),
                ("grant_type", DEVICE_GRANT),
            ];
            let (status, body) = self.form("token", &fields).await?;
            if (200..300).contains(&status) {
                let tokens: TokenResponse = parse(&body)?;
                let identity = self
                    .validate(&tokens.access_token)
                    .await?
                    .ok_or(AuthError::InvalidResponse)?;
                let token = Token {
                    access: tokens.access_token.into(),
                    refresh: tokens.refresh_token.into(),
                    user_id: identity.user_id,
                    login: identity.login,
                    scopes: identity.scopes,
                };
                // After any refresh in flight, so the refresh can never write over this newer login.
                let _refreshing = self.0.refresh_lock.lock().await;
                if self.0.state.lock().unwrap().generation != generation {
                    return Ok(()); // superseded by a newer login or a logout
                }
                self.persist(&token).await?;
                let user_id = token.user_id.clone();
                {
                    let mut state = self.0.state.lock().unwrap();
                    if state.generation != generation {
                        // A logout or newer login ran while the token was being written.
                        self.undo_write(&state);
                        return Ok(());
                    }
                    state.token = Some(token);
                }
                self.0.account.send_replace(Some(user_id));
                return Ok(());
            }
            let error: ErrorResponse = serde_json::from_slice(&body).unwrap_or(ErrorResponse {
                message: String::new(),
            });
            if error.message != "authorization_pending" {
                return Err(AuthError::SignInRefused);
            }
        }
        Err(AuthError::SignInExpired)
    }

    /// Refreshes after Twitch rejected `rejected_access`. Only one refresh runs at a time; callers that waited
    /// behind it see the token already changed and return without spending the single-use refresh token again.
    /// The result is kept only while `rejected_access` is still the token in use: never over a logout or a newer
    /// login. A sign-in started meanwhile keeps waiting for its own approval.
    pub async fn refresh(&self, rejected_access: &str) -> Result<(), AuthError> {
        let _guard = self.0.refresh_lock.lock().await;
        let token = self.0.state.lock().unwrap().token.clone();
        let token = token.ok_or(AuthError::SignInRequired)?;
        if token.access.expose_secret() != rejected_access {
            return Ok(());
        }
        let still_current = |state: &State| {
            state
                .token
                .as_ref()
                .is_some_and(|t| t.access.expose_secret() == rejected_access)
        };
        let fields = [
            ("client_id", self.0.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", token.refresh.expose_secret()),
        ];
        let (status, body) = self.form("token", &fields).await?;
        if matches!(status, 400 | 401) {
            let mut state = self.0.state.lock().unwrap();
            if still_current(&state) {
                state.token = None;
                if let Err(e) = self.0.store.delete() {
                    tracing::error!(code = "twitch.storage_failed", error = %e, "rejected token left stored");
                }
                drop(state);
                self.0.account.send_replace(None);
            }
            return Err(AuthError::SignInRequired);
        }
        if !(200..300).contains(&status) {
            return Err(AuthError::InvalidResponse);
        }
        let tokens: TokenResponse = parse(&body)?;
        let updated = Token {
            access: tokens.access_token.into(),
            refresh: tokens.refresh_token.into(),
            ..token
        };
        {
            let mut state = self.0.state.lock().unwrap();
            if !still_current(&state) {
                return Err(AuthError::SignInRequired); // signed out while refreshing
            }
            // The old refresh token is already spent: keep the new pair in memory even if persisting fails.
            state.token = Some(updated.clone());
        }
        let persisted = self.persist(&updated).await;
        self.undo_write(&self.0.state.lock().unwrap());
        persisted
    }

    /// Called after writing a token that a logout may have superseded: if the user is now signed out, the entry
    /// just written must not survive. A newer login keeps its own entry.
    fn undo_write(&self, state: &State) {
        if state.token.is_none()
            && let Err(e) = self.0.store.delete()
        {
            tracing::error!(code = "twitch.storage_failed", error = %e, "signed-out token left stored");
        }
    }

    /// Signs out in memory at once; fails with [`AuthError::StorageFailed`] when the stored token could not be
    /// deleted, which would sign in again on the next start.
    pub fn logout(&self) -> Result<(), AuthError> {
        let mut state = self.0.state.lock().unwrap();
        state.token = None;
        state.status_override = None;
        state.generation += 1;
        let deleted = self.0.store.delete();
        drop(state);
        self.0.account.send_replace(None);
        deleted.map_err(|e| {
            tracing::error!(code = "twitch.storage_failed", error = %e, "stored token not deleted");
            AuthError::StorageFailed
        })
    }

    /// Writes the token to the secret store, retrying briefly before giving up. Credential Manager calls block,
    /// so they run on the blocking pool instead of an async worker.
    async fn persist(&self, token: &Token) -> Result<(), AuthError> {
        let json = serde_json::to_string(&token.to_stored(&self.0.client_id))
            .map_err(|_| AuthError::StorageFailed)?;
        let store = self.0.store.clone();
        tokio::task::spawn_blocking(move || {
            for attempt in 0..3 {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if store.write(&json).is_ok() {
                    return Ok(());
                }
            }
            Err(AuthError::StorageFailed)
        })
        .await
        .map_err(|_| AuthError::StorageFailed)?
    }

    async fn validate(&self, access: &str) -> Result<Option<ValidateResponse>, AuthError> {
        let req = Request::builder()
            .uri(format!("{}/validate", self.0.oauth_base))
            .header(header::AUTHORIZATION, format!("OAuth {access}"))
            .body(Full::new(Bytes::new()))
            .map_err(|_| AuthError::InvalidResponse)?;
        let (status, body) = self.send(req).await?;
        match status {
            401 => Ok(None),
            200..=299 => Ok(Some(parse(&body)?)),
            _ => Err(AuthError::InvalidResponse),
        }
    }

    async fn form(
        &self,
        endpoint: &str,
        fields: &[(&str, &str)],
    ) -> Result<(u16, Bytes), AuthError> {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("{}/{endpoint}", self.0.oauth_base))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| AuthError::InvalidResponse)?;
        self.send(req).await
    }

    async fn send(&self, req: Request<Full<Bytes>>) -> Result<(u16, Bytes), AuthError> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            let res = self
                .0
                .http
                .request(req)
                .await
                .map_err(|_| AuthError::Unreachable)?;
            let status = res.status().as_u16();
            let body = res
                .into_body()
                .collect()
                .await
                .map_err(|_| AuthError::Unreachable)?
                .to_bytes();
            Ok((status, body))
        })
        .await
        .map_err(|_| AuthError::Unreachable)?
    }
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, AuthError> {
    serde_json::from_slice(body).map_err(|_| AuthError::InvalidResponse)
}

/// The fake Twitch's broadcaster (`crates/fakes` keeps the same values; its tests check they match).
pub const FAKE_USER_ID: &str = "1000";
pub const FAKE_LOGIN: &str = "fake_streamer";

/// A stored token for the fake Twitch (`crates/fakes`) only: its broadcaster, every scope the app asks for and no
/// Client ID, so any Client ID accepts it. Never written for a real account.
pub fn fake_stored_token() -> String {
    let token = StoredToken {
        client_id: String::new(),
        access_token: "fake-access".into(),
        refresh_token: "fake-refresh".into(),
        user_id: FAKE_USER_ID.into(),
        login: FAKE_LOGIN.into(),
        scopes: super::MVP_SCOPES
            .iter()
            .chain(super::FEATURE_SCOPES)
            .map(|s| s.to_string())
            .collect(),
    };
    serde_json::to_string(&token).expect("a stored token serializes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_token_signs_in_with_every_scope() {
        let store = Arc::new(crate::secrets::InMemory::with(fake_stored_token()));
        let http = tls::https_client(tls::client_config().unwrap());
        let auth = TwitchAuth::new("any-client".into(), "http://127.0.0.1:1", http, store);
        assert_eq!(
            auth.status(),
            AuthStatus::SignedIn {
                user_id: "1000".into(),
                login: "fake_streamer".into()
            }
        );
    }
    use crate::{secrets::memory::MemoryStore, tls};
    use axum::{
        Form, Json, Router,
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU32, Ordering},
    };

    /// Fake id.twitch.tv: the first token poll is pending, the second succeeds; refresh rotates or rejects.
    async fn fake_oauth(refresh_status: StatusCode) -> String {
        let polls = Arc::new(AtomicU32::new(0));
        let app = Router::new()
            .route("/device", post(|| async {
                Json(json!({"device_code":"dev","user_code":"ABCD-EFGH","verification_uri":"https://www.twitch.tv/activate","expires_in":30,"interval":0}))
            }))
            .route("/token", post(move |Form(f): Form<HashMap<String, String>>| {
                let polls = polls.clone();
                async move {
                    if f.get("grant_type").map(String::as_str) == Some("refresh_token") {
                        return if refresh_status.is_success() {
                            Json(json!({"access_token":"access-2","refresh_token":"refresh-2"})).into_response()
                        } else {
                            (refresh_status, Json(json!({"message":"Invalid refresh token"}))).into_response()
                        };
                    }
                    if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                        (StatusCode::BAD_REQUEST, Json(json!({"message":"authorization_pending"}))).into_response()
                    } else {
                        Json(json!({"access_token":"access-1","refresh_token":"refresh-1"})).into_response()
                    }
                }
            }))
            .route("/validate", get(|| async { Json(json!({"user_id":"42","login":"streamer","scopes":crate::twitch::MVP_SCOPES})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn auth(base: &str, store: Arc<MemoryStore>) -> TwitchAuth {
        TwitchAuth::new(
            "client".into(),
            base,
            tls::https_client(tls::client_config().unwrap()),
            store,
        )
    }

    async fn wait_signed_in(auth: &TwitchAuth) {
        for _ in 0..100 {
            if matches!(auth.status(), AuthStatus::SignedIn { .. }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("not signed in: {:?}", auth.status());
    }

    #[tokio::test]
    async fn device_flow_signs_in_and_persists_the_token() {
        let store = Arc::new(MemoryStore::default());
        let auth = auth(&fake_oauth(StatusCode::OK).await, store.clone());

        let login = auth.start_login(crate::twitch::MVP_SCOPES).await.unwrap();
        assert_eq!(login.user_code, "ABCD-EFGH");
        assert!(matches!(auth.status(), AuthStatus::Pending { .. }));

        wait_signed_in(&auth).await;
        assert_eq!(
            auth.status(),
            AuthStatus::SignedIn {
                user_id: "42".into(),
                login: "streamer".into()
            }
        );
        let stored = store.value.lock().unwrap().clone().unwrap();
        assert!(stored.contains("refresh-1"));
    }

    #[tokio::test]
    async fn refresh_rotates_and_skips_a_stale_rejection() {
        let store = Arc::new(MemoryStore::default());
        let auth = auth(&fake_oauth(StatusCode::OK).await, store.clone());
        auth.start_login(&[]).await.unwrap();
        wait_signed_in(&auth).await;

        auth.refresh("access-1").await.unwrap();
        assert_eq!(auth.token().unwrap().access.expose_secret(), "access-2");
        assert!(
            store
                .value
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("refresh-2")
        );

        // A second caller rejected with the old token must not spend the new refresh token again.
        auth.refresh("access-1").await.unwrap();
        assert_eq!(auth.token().unwrap().access.expose_secret(), "access-2");
    }

    #[tokio::test]
    async fn rejected_refresh_signs_out_and_clears_the_store() {
        let store = Arc::new(MemoryStore::default());
        let auth = auth(&fake_oauth(StatusCode::BAD_REQUEST).await, store.clone());
        auth.start_login(&[]).await.unwrap();
        wait_signed_in(&auth).await;

        assert_eq!(
            auth.refresh("access-1").await,
            Err(AuthError::SignInRequired)
        );
        assert_eq!(auth.status(), AuthStatus::SignedOut);
        assert!(store.value.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn refreshed_tokens_stay_in_memory_when_persisting_fails() {
        let store = Arc::new(MemoryStore::default());
        let auth = auth(&fake_oauth(StatusCode::OK).await, store.clone());
        auth.start_login(&[]).await.unwrap();
        wait_signed_in(&auth).await;

        store.fail_writes.store(true, Ordering::SeqCst);
        assert_eq!(
            auth.refresh("access-1").await,
            Err(AuthError::StorageFailed)
        );
        assert_eq!(auth.token().unwrap().access.expose_secret(), "access-2");
    }

    #[tokio::test]
    async fn login_without_client_id_is_refused() {
        let auth = TwitchAuth::new(
            String::new(),
            "http://127.0.0.1:1",
            tls::https_client(tls::client_config().unwrap()),
            Arc::new(MemoryStore::default()),
        );
        assert_eq!(
            auth.start_login(&[]).await.unwrap_err(),
            AuthError::ClientIdRequired
        );
    }

    fn store_with(stored: serde_json::Value) -> Arc<MemoryStore> {
        let store = Arc::new(MemoryStore::default());
        *store.value.lock().unwrap() = Some(stored.to_string());
        store
    }

    #[tokio::test]
    async fn stored_token_needs_the_matching_client_id() {
        let token = json!({"access_token":"a","refresh_token":"r","user_id":"42","login":"streamer","scopes":[]});
        let mut other = token.clone();
        other["client_id"] = json!("other-app");
        for (client_id, stored, signed_in) in [
            ("", token.clone(), false),
            ("client", token.clone(), true), // stored before client_id was recorded
            ("client", other, false),
        ] {
            let store = store_with(stored);
            let auth = TwitchAuth::new(
                client_id.into(),
                "http://127.0.0.1:1",
                tls::https_client(tls::client_config().unwrap()),
                store.clone(),
            );
            assert_eq!(auth.token().is_some(), signed_in, "client_id {client_id:?}");
            assert_eq!(auth.account().borrow().is_some(), signed_in);
            assert!(
                store.value.lock().unwrap().is_some(),
                "the entry is never deleted here"
            );
        }
    }

    /// Signing in again as the same user (to grant a newer scope) must still wake EventSub, so it subscribes
    /// with the new token; `send_if_modified` would skip an unchanged user ID.
    #[tokio::test]
    async fn signing_in_again_as_the_same_user_tells_watchers() {
        let store = store_with(
            json!({"client_id":"client","access_token":"old","refresh_token":"r",
                                      "user_id":"42","login":"streamer","scopes":[]}),
        );
        let auth = auth(&fake_oauth(StatusCode::OK).await, store);
        let mut account = auth.account();
        assert_eq!(account.borrow_and_update().as_deref(), Some("42"));
        auth.start_login(crate::twitch::MVP_SCOPES).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), account.changed())
            .await
            .expect("watchers are told about the new sign-in")
            .unwrap();
        assert_eq!(account.borrow().as_deref(), Some("42"));
        wait_signed_in(&auth).await;
    }

    #[tokio::test]
    async fn token_without_a_newer_scope_asks_to_sign_in_again() {
        let mut scopes = crate::twitch::MVP_SCOPES.to_vec();
        scopes.retain(|s| *s != "bits:read");
        let auth = TwitchAuth::new(
            "client".into(),
            "http://127.0.0.1:1",
            tls::https_client(tls::client_config().unwrap()),
            store_with(
                json!({"client_id":"client","access_token":"a","refresh_token":"r",
                              "user_id":"42","login":"streamer","scopes":scopes}),
            ),
        );
        assert_eq!(
            auth.status(),
            AuthStatus::Failed {
                code: "twitch.scopes_missing".into()
            }
        );
        assert!(auth.token().is_some(), "chat keeps using the old token");
    }

    #[tokio::test]
    async fn logout_during_refresh_stays_signed_out() {
        let app = Router::new().route(
            "/token",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                Json(json!({"access_token":"access-2","refresh_token":"refresh-2"}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let store = store_with(
            json!({"client_id":"client","access_token":"access-1","refresh_token":"refresh-1",
                   "user_id":"42","login":"streamer","scopes":[]}),
        );
        let auth = auth(&base, store.clone());

        let refreshing = tokio::spawn({
            let auth = auth.clone();
            async move { auth.refresh("access-1").await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        auth.logout().unwrap();

        assert_eq!(refreshing.await.unwrap(), Err(AuthError::SignInRequired));
        assert!(auth.token().is_none());
        assert_eq!(auth.status(), AuthStatus::SignedOut);
        assert!(
            store.value.lock().unwrap().is_none(),
            "nothing written back after logout"
        );
    }

    /// Fake id.twitch.tv for races: a refresh answers after `refresh_delay`; a device poll signs in at once.
    async fn racing_oauth(refresh_delay: Duration) -> String {
        let app = Router::new()
            // Reads the body: a server that leaves it unread closes the connection, which the client may reuse.
            .route("/device", post(|Form(_): Form<HashMap<String, String>>| async {
                Json(json!({"device_code":"dev","user_code":"ABCD-EFGH","verification_uri":"https://www.twitch.tv/activate","expires_in":30,"interval":1}))
            }))
            .route("/token", post(move |Form(f): Form<HashMap<String, String>>| async move {
                if f.get("grant_type").map(String::as_str) == Some("refresh_token") {
                    tokio::time::sleep(refresh_delay).await;
                    Json(json!({"access_token":"access-2","refresh_token":"refresh-2"}))
                } else {
                    Json(json!({"access_token":"access-new","refresh_token":"refresh-new"}))
                }
            }))
            .route("/validate", get(|| async { Json(json!({"user_id":"42","login":"streamer","scopes":crate::twitch::MVP_SCOPES})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn signed_in_store() -> Arc<MemoryStore> {
        store_with(
            json!({"client_id":"client","access_token":"access-1","refresh_token":"refresh-1",
                   "user_id":"42","login":"streamer","scopes":crate::twitch::MVP_SCOPES}),
        )
    }

    async fn wait_for_access(auth: &TwitchAuth, access: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while auth
                .token()
                .is_none_or(|t| t.access.expose_secret() != access)
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("never got {access}"));
    }

    #[tokio::test]
    async fn a_sign_in_finishing_during_a_refresh_is_the_token_kept() {
        let store = signed_in_store();
        let auth = auth(
            &racing_oauth(Duration::from_millis(1500)).await,
            store.clone(),
        );
        // The device poll answers after its 1 s interval, while the refresh is still waiting.
        auth.start_login(&[]).await.unwrap();
        let refreshing = tokio::spawn({
            let auth = auth.clone();
            async move { auth.refresh("access-1").await }
        });
        refreshing.await.unwrap().unwrap();
        wait_for_access(&auth, "access-new").await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(auth.token().unwrap().access.expose_secret(), "access-new");
        assert!(
            store
                .value
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("refresh-new")
        );
    }

    #[tokio::test]
    async fn a_refresh_survives_a_sign_in_started_meanwhile() {
        let store = signed_in_store();
        let auth = auth(
            &racing_oauth(Duration::from_millis(300)).await,
            store.clone(),
        );
        let refreshing = tokio::spawn({
            let auth = auth.clone();
            async move { auth.refresh("access-1").await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        auth.start_login(&[]).await.unwrap();
        // The spent refresh token is replaced, not lost while the new sign-in waits for approval.
        assert_eq!(refreshing.await.unwrap(), Ok(()));
        assert_eq!(auth.token().unwrap().access.expose_secret(), "access-2");
        assert!(matches!(auth.status(), AuthStatus::Pending { .. }));
        wait_for_access(&auth, "access-new").await;
        assert!(
            store
                .value
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("refresh-new")
        );
    }

    #[tokio::test]
    async fn logout_reports_a_token_it_could_not_delete() {
        let store = signed_in_store();
        let auth = auth("http://127.0.0.1:1", store.clone());
        store.fail_deletes.store(true, Ordering::SeqCst);
        assert_eq!(auth.logout(), Err(AuthError::StorageFailed));
        assert!(auth.token().is_none(), "signed out in memory all the same");
        assert_eq!(auth.status(), AuthStatus::SignedOut);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_without_an_answer_fails_as_unreachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // Accepts and never answers.
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                held.push(listener.accept().await.unwrap());
            }
        });
        let auth = auth(&base, Arc::new(MemoryStore::default()));
        let started = tokio::time::Instant::now();
        assert_eq!(
            auth.start_login(&[]).await.unwrap_err(),
            AuthError::Unreachable
        );
        assert!(started.elapsed() >= REQUEST_TIMEOUT);
    }

    /// Fake id.twitch.tv for sign-in attempts: each `/device` gets the next device code (`dev1`, `dev2`, …) and
    /// can be held or refused; `/token` stays pending and counts the polls per device code.
    #[derive(Clone, Default)]
    struct Attempts {
        devices: Arc<AtomicU32>,
        polls: Arc<std::sync::Mutex<HashMap<String, u32>>>,
        /// `/device` answers only once this is notified.
        hold: Option<Arc<tokio::sync::Notify>>,
        /// Notified as each `/device` request arrives.
        arrived: Arc<tokio::sync::Notify>,
        /// `/device` from the second request on answers 500.
        refuse_after_first: bool,
        expires_in: u64,
        interval: u64,
    }

    impl Attempts {
        fn polls(&self, device_code: &str) -> u32 {
            self.polls
                .lock()
                .unwrap()
                .get(device_code)
                .copied()
                .unwrap_or(0)
        }
    }

    async fn attempts_oauth(a: Attempts) -> String {
        let device = {
            let a = a.clone();
            move || {
                let a = a.clone();
                async move {
                    a.arrived.notify_one();
                    if let Some(hold) = &a.hold {
                        hold.notified().await;
                    }
                    let n = a.devices.fetch_add(1, Ordering::SeqCst) + 1;
                    if a.refuse_after_first && n > 1 {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    Json(
                        json!({"device_code": format!("dev{n}"), "user_code": format!("CODE-{n}"),
                        "verification_uri": "https://www.twitch.tv/activate",
                        "expires_in": a.expires_in, "interval": a.interval}),
                    )
                    .into_response()
                }
            }
        };
        let token = move |Form(f): Form<HashMap<String, String>>| {
            let a = a.clone();
            async move {
                let code = f.get("device_code").cloned().unwrap_or_default();
                *a.polls.lock().unwrap().entry(code).or_default() += 1;
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"message":"authorization_pending"})),
                )
            }
        };
        let app = Router::new()
            .route("/device", post(device))
            .route("/token", post(token));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    /// Waits (up to 10 s) until `done`.
    async fn until(what: &str, done: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    fn pending_code(auth: &TwitchAuth) -> Option<String> {
        match auth.status() {
            AuthStatus::Pending { user_code, .. } => Some(user_code),
            _ => None,
        }
    }

    #[tokio::test]
    async fn a_logout_while_twitch_answers_the_device_request_wins() {
        let hold = Arc::new(tokio::sync::Notify::new());
        let fake = Attempts {
            hold: Some(hold.clone()),
            expires_in: 30,
            ..Attempts::default()
        };
        let auth = auth(
            &attempts_oauth(fake.clone()).await,
            Arc::new(MemoryStore::default()),
        );
        let started = tokio::spawn({
            let auth = auth.clone();
            async move { auth.start_login(&[]).await }
        });
        fake.arrived.notified().await;
        auth.logout().unwrap();
        hold.notify_one();
        assert!(matches!(
            started.await.unwrap(),
            Err(AuthError::SignInExpired)
        ));
        assert_eq!(auth.status(), AuthStatus::SignedOut);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(fake.polls("dev1"), 0, "never polled");
    }

    #[tokio::test]
    async fn a_newer_sign_in_stops_the_older_one_and_a_failed_one_leaves_no_old_code() {
        let fake = Attempts {
            expires_in: 30,
            ..Attempts::default()
        };
        let auth = auth(
            &attempts_oauth(fake.clone()).await,
            Arc::new(MemoryStore::default()),
        );
        auth.start_login(&[]).await.unwrap();
        until("a first poll", || fake.polls("dev1") >= 1).await;
        auth.start_login(&[]).await.unwrap();
        assert_eq!(pending_code(&auth).as_deref(), Some("CODE-2"));
        let first = fake.polls("dev1");
        until("the newer sign-in polling twice", || {
            fake.polls("dev2") >= 2
        })
        .await;
        assert!(
            fake.polls("dev1") <= first + 1,
            "the older sign-in stopped asking (one poll may already have been on its way)"
        );

        // A newer sign-in whose device request fails: the older code is not left showing.
        let refusing = Attempts {
            refuse_after_first: true,
            expires_in: 30,
            ..Attempts::default()
        };
        let auth = super::tests::auth(
            &attempts_oauth(refusing).await,
            Arc::new(MemoryStore::default()),
        );
        auth.start_login(&[]).await.unwrap();
        assert_eq!(pending_code(&auth).as_deref(), Some("CODE-1"));
        assert!(matches!(
            auth.start_login(&[]).await,
            Err(AuthError::SignInRefused)
        ));
        assert_eq!(pending_code(&auth), None);
    }

    #[tokio::test]
    async fn an_expired_code_is_never_polled_and_a_huge_lifetime_is_capped() {
        // Expires before the first poll would go out.
        let fake = Attempts {
            expires_in: 1,
            interval: 5,
            ..Attempts::default()
        };
        let auth = auth(
            &attempts_oauth(fake.clone()).await,
            Arc::new(MemoryStore::default()),
        );
        auth.start_login(&[]).await.unwrap();
        until("the code to expire", || {
            matches!(auth.status(), AuthStatus::Failed { .. })
        })
        .await;
        assert_eq!(fake.polls("dev1"), 0);
        assert_eq!(
            auth.status(),
            AuthStatus::Failed {
                code: "twitch.sign_in_expired".into()
            }
        );

        let huge = Attempts {
            expires_in: u64::MAX,
            ..Attempts::default()
        };
        let auth = super::tests::auth(
            &attempts_oauth(huge.clone()).await,
            Arc::new(MemoryStore::default()),
        );
        auth.start_login(&[]).await.unwrap();
        until("a poll", || huge.polls("dev1") >= 1).await;
        assert_eq!(pending_code(&auth).as_deref(), Some("CODE-1"));
        assert_eq!(code_lifetime(u64::MAX), DEVICE_CODE_LIFETIME);
        assert_eq!(code_lifetime(5), Duration::from_secs(5));
    }
}
