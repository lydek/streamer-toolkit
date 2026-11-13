//! Helix calls with the broadcaster's user token. Every Helix request goes through here so rate limits and
//! token refresh are handled once: a 401 gets one refresh and one retry; a 429 reports how long to wait.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http::{HeaderMap, Method, Request, header};
use http_body_util::Full;
use secrecy::ExposeSecret;

use super::auth::{AuthError, TwitchAuth};
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
        let body = Bytes::from(body.to_string());
        for attempt in 0..2 {
            let token = self.auth.token().ok_or(AuthError::SignInRequired)?;
            let req = Request::builder()
                .method(Method::POST)
                .uri(format!("{}{path}", self.base))
                .header("Client-Id", self.auth.client_id())
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", token.access.expose_secret()),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Full::new(body.clone()))
                .map_err(|_| AuthError::InvalidResponse)?;
            let res = self
                .http
                .request(req)
                .await
                .map_err(|_| AuthError::Unreachable)?;
            match res.status().as_u16() {
                200..=299 => return Ok(()),
                401 if attempt == 0 => self.auth.refresh(token.access.expose_secret()).await?,
                429 => return Err(HelixError::RateLimited(reset_wait(res.headers()))),
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
}
