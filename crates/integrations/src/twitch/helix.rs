//! Helix calls with the broadcaster's user token. Every Helix request goes through here so rate limits and
//! token refresh are handled once: a 401 gets one refresh and one retry; a 429 reports how long to wait.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http::{HeaderMap, Method, Request, header};
use http_body_util::{BodyExt, Full};
use secrecy::ExposeSecret;
use tokio::time::timeout;

use super::auth::{AuthError, REQUEST_TIMEOUT, TwitchAuth};
use crate::tls::HttpsClient;

pub const HELIX_BASE: &str = "https://api.twitch.tv/helix";

/// Longest wait taken from a `Ratelimit-Reset` header; the bucket refills within a minute.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum HelixError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// 429. `None` when Twitch gave no reset time (some 429s are not the points bucket, e.g. EventSub limits).
    #[error("twitch.rate_limited")]
    RateLimited(Option<Duration>),
    #[error("twitch.request_refused")]
    Refused(u16),
}

#[derive(Clone)]
pub struct Helix {
    base: String,
    auth: TwitchAuth,
    http: HttpsClient,
}

impl Helix {
    pub fn new(base: &str, auth: TwitchAuth, http: HttpsClient) -> Self {
        Self {
            base: base.trim_end_matches('/').into(),
            auth,
            http,
        }
    }

    pub fn auth(&self) -> &TwitchAuth {
        &self.auth
    }

    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<(), HelixError> {
        self.send(Method::POST, path, Some(Bytes::from(body.to_string())))
            .await
            .map(|_| ())
    }

    /// `path` includes its query; the response body is parsed as JSON.
    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value, HelixError> {
        let body = self.send(Method::GET, path, None).await?;
        Ok(serde_json::from_slice(&body).map_err(|_| AuthError::InvalidResponse)?)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Bytes>,
    ) -> Result<Bytes, HelixError> {
        for attempt in 0..2 {
            let token = self.auth.token().ok_or(AuthError::SignInRequired)?;
            if let Some(wait) = self.auth.helix_wait(&token.user_id) {
                return Err(HelixError::RateLimited(Some(wait)));
            }
            let mut req = Request::builder()
                .method(method.clone())
                .uri(format!("{}{path}", self.base))
                .header("Client-Id", self.auth.client_id())
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", token.access.expose_secret()),
                );
            if body.is_some() {
                req = req.header(header::CONTENT_TYPE, "application/json");
            }
            let req = req
                .body(Full::new(body.clone().unwrap_or_default()))
                .map_err(|_| AuthError::InvalidResponse)?;
            let res = timeout(REQUEST_TIMEOUT, self.http.request(req))
                .await
                .ok()
                .and_then(Result::ok)
                .ok_or(AuthError::Unreachable)?;
            let status = res.status().as_u16();
            let reset = reset_wait(res.headers());
            let empty = res
                .headers()
                .get("Ratelimit-Remaining")
                .is_some_and(|n| n == "0");
            if (empty || status == 429)
                && let Some(wait) = reset
            {
                self.auth.pause_helix(&token.user_id, wait);
            }
            match status {
                200..=299 => {
                    return Ok(timeout(REQUEST_TIMEOUT, res.into_body().collect())
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .ok_or(AuthError::Unreachable)?
                        .to_bytes());
                }
                401 if attempt == 0 => self.auth.refresh(token.access.expose_secret()).await?,
                429 => return Err(HelixError::RateLimited(reset)),
                status => return Err(HelixError::Refused(status)),
            }
        }
        Err(HelixError::Refused(401))
    }
}

/// Time until `Ratelimit-Reset` (Unix seconds), at least 1 s and at most a minute.
fn reset_wait(headers: &HeaderMap) -> Option<Duration> {
    let reset: u64 = headers
        .get("Ratelimit-Reset")?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Duration::from_secs(reset.saturating_sub(now).max(1)).min(MAX_RATE_LIMIT_WAIT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    fn headers(reset: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("Ratelimit-Reset", HeaderValue::from_str(reset).unwrap());
        h
    }

    #[test]
    fn reset_wait_is_bounded() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(reset_wait(&HeaderMap::new()), None);
        assert_eq!(reset_wait(&headers("nope")), None);
        assert_eq!(
            reset_wait(&headers(&(now - 5).to_string())),
            Some(Duration::from_secs(1))
        );
        let soon = reset_wait(&headers(&(now + 10).to_string())).unwrap();
        assert!((Duration::from_secs(9)..=Duration::from_secs(10)).contains(&soon));
        assert_eq!(
            reset_wait(&headers(&(now + 3600).to_string())),
            Some(MAX_RATE_LIMIT_WAIT)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn clients_for_the_same_login_pause_after_the_last_point() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let reset = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 10)
            .to_string();
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/users",
                axum::routing::get(move || {
                    let reset = reset.clone();
                    async move {
                        (
                            [
                                ("Ratelimit-Remaining", "0".into()),
                                ("Ratelimit-Reset", reset),
                            ],
                            "{}",
                        )
                    }
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });
        let store = crate::secrets::memory::MemoryStore::default();
        *store.value.lock().unwrap() = Some(
            r#"{"client_id":"client","access_token":"a","refresh_token":"r","user_id":"42","login":"s","scopes":[]}"#
                .into(),
        );
        let http = crate::tls::https_client(crate::tls::client_config().unwrap());
        let auth = TwitchAuth::new(
            "client".into(),
            &base,
            http.clone(),
            std::sync::Arc::new(store),
        );
        let first = Helix::new(&base, auth.clone(), http.clone());
        let other = Helix::new(&base, auth, http);
        first.get_json("/users").await.unwrap();
        let Err(HelixError::RateLimited(Some(wait))) = other.get_json("/users").await else {
            panic!("the second client must honor the shared reset time");
        };
        tokio::time::advance(wait).await;
        other.get_json("/users").await.unwrap();
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
        let store = crate::secrets::memory::MemoryStore::default();
        *store.value.lock().unwrap() = Some(
            r#"{"client_id":"client","access_token":"a","refresh_token":"r","user_id":"42","login":"s","scopes":[]}"#
                .into(),
        );
        let http = crate::tls::https_client(crate::tls::client_config().unwrap());
        let auth = TwitchAuth::new(
            "client".into(),
            &base,
            http.clone(),
            std::sync::Arc::new(store),
        );
        let helix = Helix::new(&base, auth, http);
        let started = tokio::time::Instant::now();
        assert!(matches!(
            helix.get_json("/users").await,
            Err(HelixError::Auth(AuthError::Unreachable))
        ));
        assert!(started.elapsed() >= REQUEST_TIMEOUT);
    }
}
