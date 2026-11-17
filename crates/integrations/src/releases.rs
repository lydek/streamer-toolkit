//! The newest published release on GitHub, for the new-version notice. Only the version is read from the
//! response; nothing else in it (URLs above all) is used.

use std::time::Duration;

use http::{Request, header};
use http_body_util::{BodyExt, Full, Limited};
use serde::Deserialize;

use crate::tls::HttpsClient;

/// Skips drafts and pre-releases on GitHub's side.
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/lydek/streamer-toolkit/releases/latest";

const TIMEOUT: Duration = Duration::from_secs(15);
/// A release object is a few KB; anything far larger is not one.
const MAX_BODY: usize = 1024 * 1024;

/// `major.minor.patch`, ordered by those numbers.
pub type Version = (u64, u64, u64);

/// `1.2.3` or `v1.2.3`; anything else (pre-release or build suffixes included) is `None`.
pub fn parse_version(text: &str) -> Option<Version> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let mut next = || {
        parts
            .next()
            .filter(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest release's version, or `None` on any failure (no network, timeout, rate limit, unexpected
/// response): the notice stays hidden and no error is shown. Sends no user data, only a fixed User-Agent.
pub async fn latest_version(http: &HttpsClient, url: &str) -> Option<Version> {
    let req = Request::get(url)
        // GitHub rejects requests without a User-Agent.
        .header(header::USER_AGENT, "streamer-toolkit")
        .header(header::ACCEPT, "application/vnd.github+json")
        .body(Full::default())
        .ok()?;
    let body = tokio::time::timeout(TIMEOUT, async {
        let res = http.request(req).await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        Limited::new(res.into_body(), MAX_BODY)
            .collect()
            .await
            .ok()
            .map(|b| b.to_bytes())
    })
    .await
    .ok()??;
    let release: Release = serde_json::from_slice(&body).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    parse_version(&release.tag_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::{client_config, https_client};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn versions_parse_only_plain_major_minor_patch() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10.0"), Some((0, 10, 0)));
        for bad in [
            "v1.2",
            "1.2.3.4",
            "v1.2.3-rc.1",
            "1.2.x",
            "",
            "v",
            "1..3",
            "+1.2.3",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
    }

    /// Answers every request with `status` and `body`; returns the URL.
    async fn fake_github(status: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 4096];
                let _ = socket.read(&mut buf).await;
                let res = format!(
                    "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(res.as_bytes()).await;
            }
        });
        format!("http://{address}/releases/latest")
    }

    #[tokio::test]
    async fn reads_the_version_of_a_published_release_only() {
        let http = https_client(client_config().unwrap());
        let cases = [
            (
                "200 OK",
                r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,"html_url":"https://evil.example"}"#,
                Some((0, 2, 0)),
            ),
            (
                "200 OK",
                r#"{"tag_name":"v0.3.0","draft":false,"prerelease":true}"#,
                None,
            ),
            ("200 OK", r#"{"tag_name":"v0.3.0-rc.1"}"#, None),
            ("200 OK", "not json", None),
            ("403 Forbidden", r#"{"message":"rate limited"}"#, None),
            ("404 Not Found", r#"{"message":"Not Found"}"#, None),
        ];
        for (status, body, want) in cases {
            let url = fake_github(status, body).await;
            assert_eq!(latest_version(&http, &url).await, want, "{status} {body}");
        }
    }

    #[tokio::test]
    async fn no_service_means_no_version() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let http = https_client(client_config().unwrap());
        assert_eq!(
            latest_version(&http, &format!("http://{address}/")).await,
            None
        );
    }
}
