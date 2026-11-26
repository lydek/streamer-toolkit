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
    /// Like `post_json`, for the requests whose answer matters (e.g. Send Chat Message says whether it went out).
    pub async fn post_json_read(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, HelixError> {
        let body = self
            .send(Method::POST, path, Some(Bytes::from(body.to_string())))
            .await?;
        Ok(serde_json::from_slice(&body).map_err(|_| AuthError::InvalidResponse)?)
    }

    pub async fn patch_json(&self, path: &str, body: &serde_json::Value) -> Result<(), HelixError> {
        self.send(Method::PATCH, path, Some(Bytes::from(body.to_string())))
            .await
            .map(|_| ())
    }

    pub async fn delete(&self, path: &str) -> Result<(), HelixError> {
        self.send(Method::DELETE, path, None).await.map(|_| ())
    }

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
            let remaining: Option<u64> = res
                .headers()
                .get("Ratelimit-Remaining")
                .and_then(|n| n.to_str().ok()?.parse().ok());
            // A 429 with points left is not the rate limit (EventSub answers its subscription limits so): only that
            // request is refused, nothing else pauses. Without a readable count it is taken as the rate limit.
            if status == 429 && remaining.is_some_and(|n| n > 0) {
                return Err(HelixError::Refused(429));
            }
            if (remaining == Some(0) || status == 429)
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

    /// A Helix that answers `/limited` with a 429 and these headers, and `/users` with `{}`.
    async fn answering_429(headers: Vec<(&'static str, String)>) -> Helix {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let limited = move || {
                let mut map = HeaderMap::new();
                for (name, value) in &headers {
                    map.insert(*name, HeaderValue::from_str(value).unwrap());
                }
                async move { (http::StatusCode::TOO_MANY_REQUESTS, map, "{}") }
            };
            let app = axum::Router::new()
                .route("/limited", axum::routing::get(limited))
                .route("/users", axum::routing::get(|| async { "{}" }));
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
        Helix::new(&base, auth, http)
    }

    #[tokio::test]
    async fn a_429_with_points_left_refuses_only_that_request() {
        let reset = || {
            (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 10)
                .to_string()
        };
        let helix = answering_429(vec![
            ("Ratelimit-Remaining", "799".into()),
            ("Ratelimit-Reset", reset()),
        ])
        .await;
        assert!(matches!(
            helix.get_json("/limited").await,
            Err(HelixError::Refused(429))
        ));
        helix.get_json("/users").await.unwrap();

        // No points left, or no readable count: the rate limit, and every request waits for the reset.
        for remaining in [Some("0"), Some("lots"), None] {
            let mut headers = vec![("Ratelimit-Reset", reset())];
            headers.extend(remaining.map(|r| ("Ratelimit-Remaining", r.to_owned())));
            let helix = answering_429(headers).await;
            assert!(
                matches!(
                    helix.get_json("/limited").await,
                    Err(HelixError::RateLimited(Some(_)))
                ),
                "{remaining:?}"
            );
            assert!(
                matches!(
                    helix.get_json("/users").await,
                    Err(HelixError::RateLimited(Some(_)))
                ),
                "{remaining:?}: paused"
            );
        }

        // Without a usable reset time nothing pauses.
        for reset in [None, Some("soon")] {
            let mut headers = vec![("Ratelimit-Remaining", "0".to_owned())];
            headers.extend(reset.map(|r| ("Ratelimit-Reset", r.to_owned())));
            let helix = answering_429(headers).await;
            assert!(matches!(
                helix.get_json("/limited").await,
                Err(HelixError::RateLimited(None))
            ));
            helix.get_json("/users").await.unwrap();
        }
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
