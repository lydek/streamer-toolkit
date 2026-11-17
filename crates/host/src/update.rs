//! New-version notice: looks up the newest release at start and every 24 h while the check is on, and tells the
//! admin UI whether to show the notice. Admin routes; the caller adds the token check.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use integrations::{
    releases::{self, Version},
    tls::HttpsClient,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use toolkit_core::settings;
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct Updates {
    data_dir: PathBuf,
    current: String,
    /// The newest release found so far; a failed check keeps the last one.
    latest: Mutex<Option<Version>>,
    /// Checks at once when the check is turned back on.
    wake: Notify,
}

/// Starts the check loop; without a `url` nothing is ever checked.
pub(crate) fn start(
    data_dir: PathBuf,
    current: String,
    http: HttpsClient,
    url: Option<String>,
) -> Arc<Updates> {
    let updates = Arc::new(Updates {
        data_dir,
        current,
        latest: Mutex::new(None),
        wake: Notify::new(),
    });
    let Some(url) = url else {
        return updates;
    };
    let state = updates.clone();
    tokio::spawn(async move {
        loop {
            if settings::load(&state.data_dir).update_check
                && let Some(found) = releases::latest_version(&http, &url).await
            {
                *state.latest.lock().unwrap() = Some(found);
            }
            tokio::select! {
                _ = tokio::time::sleep(CHECK_EVERY) => {}
                _ = state.wake.notified() => {}
            }
        }
    });
    updates
}

pub(crate) fn routes(updates: Arc<Updates>) -> Router {
    Router::new()
        .route("/api/admin/update", get(update))
        .route("/api/admin/update/check", put(set_check))
        .route("/api/admin/update/dismiss", put(dismiss))
        .with_state(updates)
}

/// The notice shows only a release newer than both the running version and the one the broadcaster skipped.
fn newer(current: &str, latest: Option<Version>, dismissed: Option<&str>) -> Option<Version> {
    let latest = latest?;
    let current = releases::parse_version(current);
    let dismissed = dismissed.and_then(releases::parse_version);
    (Some(latest) > current && Some(latest) > dismissed).then_some(latest)
}

fn text((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

#[derive(Serialize, ToSchema)]
pub(crate) struct UpdateState {
    /// The running version.
    current: String,
    /// The newer release to tell the broadcaster about; `null`: no notice.
    available: Option<String>,
    /// Whether the new-version check is on (Settings → General).
    check: bool,
}

#[utoipa::path(get, path = "/api/admin/update", tag = "admin", responses((status = 200, body = UpdateState)))]
pub(crate) async fn update(State(s): State<Arc<Updates>>) -> Json<UpdateState> {
    let set = settings::load(&s.data_dir);
    let latest = *s.latest.lock().unwrap();
    Json(UpdateState {
        available: set
            .update_check
            .then(|| newer(&s.current, latest, set.dismissed_version.as_deref()))
            .flatten()
            .map(text),
        current: s.current.clone(),
        check: set.update_check,
    })
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CheckBody {
    enabled: bool,
}

/// Turns the new-version check on or off; turning it on checks at once.
#[utoipa::path(
    put,
    path = "/api/admin/update/check",
    tag = "admin",
    request_body = CheckBody,
    responses((status = 204), (status = 500, body = ErrorBody))
)]
pub(crate) async fn set_check(
    State(s): State<Arc<Updates>>,
    Json(body): Json<CheckBody>,
) -> Response {
    if settings::update(&s.data_dir, |set| set.update_check = body.enabled).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    if body.enabled {
        s.wake.notify_one();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct DismissBody {
    /// The version in the notice, `major.minor.patch`.
    version: String,
}

/// "Not this one": hides the notice until a newer release than `version` comes out.
#[utoipa::path(
    put,
    path = "/api/admin/update/dismiss",
    tag = "admin",
    request_body = DismissBody,
    responses((status = 204), (status = 400, body = ErrorBody), (status = 500, body = ErrorBody))
)]
pub(crate) async fn dismiss(
    State(s): State<Arc<Updates>>,
    body: Result<Json<DismissBody>, JsonRejection>,
) -> Response {
    let Some(version) = body
        .ok()
        .and_then(|Json(b)| releases::parse_version(&b.version))
    else {
        return error(StatusCode::BAD_REQUEST, "update.invalid_version");
    };
    match settings::update(&s.data_dir, |set| {
        set.dismissed_version = Some(text(version))
    }) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn notice_shows_only_releases_newer_than_running_and_skipped() {
        assert_eq!(newer("0.1.0", Some((0, 2, 0)), None), Some((0, 2, 0)));
        assert_eq!(newer("0.1.0", Some((0, 1, 0)), None), None);
        assert_eq!(newer("0.2.0", Some((0, 1, 9)), None), None);
        assert_eq!(newer("0.1.0", None, None), None);
        assert_eq!(newer("0.1.0", Some((0, 2, 0)), Some("0.2.0")), None);
        assert_eq!(
            newer("0.1.0", Some((0, 3, 0)), Some("0.2.0")),
            Some((0, 3, 0))
        );
        // A dev build's version is not semver-only; any release still counts as newer.
        assert_eq!(newer("0.0.0-e2e", Some((0, 1, 0)), None), Some((0, 1, 0)));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("st-host-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A fake GitHub that counts requests and always offers `v9.0.0`.
    async fn fake_github(hits: Arc<AtomicUsize>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                hits.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0; 4096];
                let _ = socket.read(&mut buf).await;
                let body = r#"{"tag_name":"v9.0.0","draft":false,"prerelease":false}"#;
                let res = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(res.as_bytes()).await;
            }
        });
        format!("http://{address}/releases/latest")
    }

    async fn state(updates: &Arc<Updates>) -> (Option<String>, bool) {
        let Json(s) = update(State(updates.clone())).await;
        (s.available, s.check)
    }

    #[tokio::test]
    async fn the_check_off_makes_no_request_and_turning_it_on_checks_at_once() {
        let dir = temp_dir("off");
        settings::update(&dir, |s| s.update_check = false).unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let http = integrations::tls::https_client(integrations::tls::client_config().unwrap());
        let updates = start(
            dir.clone(),
            "0.1.0".into(),
            http,
            Some(fake_github(hits.clone()).await),
        );

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(state(&updates).await, (None, false));

        let res = set_check(State(updates.clone()), Json(CheckBody { enabled: true })).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        tokio::time::timeout(Duration::from_secs(5), async {
            while state(&updates).await.0.is_none() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("checked after turning the check on");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(state(&updates).await, (Some("9.0.0".into()), true));

        // Off again: the notice hides at once, even with a release already found.
        set_check(State(updates.clone()), Json(CheckBody { enabled: false })).await;
        assert_eq!(state(&updates).await, (None, false));
    }

    #[tokio::test]
    async fn dismissing_hides_that_release_and_rejects_non_versions() {
        let dir = temp_dir("dismiss");
        let updates = Arc::new(Updates {
            data_dir: dir.clone(),
            current: "0.1.0".into(),
            latest: Mutex::new(Some((0, 2, 0))),
            wake: Notify::new(),
        });
        assert_eq!(state(&updates).await.0.as_deref(), Some("0.2.0"));

        for bad in ["0.2", "v0.2.0-rc.1", "https://x"] {
            let body = Json(DismissBody {
                version: bad.into(),
            });
            let res = dismiss(State(updates.clone()), Ok(body)).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert_eq!(settings::load(&dir).dismissed_version, None);

        let ok = Json(DismissBody {
            version: "0.2.0".into(),
        });
        assert_eq!(
            dismiss(State(updates.clone()), Ok(ok)).await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            settings::load(&dir).dismissed_version.as_deref(),
            Some("0.2.0")
        );
        assert_eq!(state(&updates).await.0, None);

        *updates.latest.lock().unwrap() = Some((0, 3, 0));
        assert_eq!(state(&updates).await.0.as_deref(), Some("0.3.0"));
    }
}
