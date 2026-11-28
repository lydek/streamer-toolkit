//! 問題排解: connection checks, the recent log lines and a diagnostic report. The report is meant to be shared, so it
//! holds no token, password, webhook URL, chat, or viewer name or ID: log lines go in as time, level and error code
//! only. Admin routes; the caller adds the token check.

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use integrations::twitch::{
    auth::{AuthStatus, TwitchAuth},
    helix::Helix,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, watch};
use toolkit_core::{modules::Modules, streaming::StreamingState};
use utoipa::{IntoParams, ToSchema};

use integrations::tls::HttpsClient;
use toolkit_core::time::{self, Timestamp};

/// How many log lines the page and the report show at most.
const LOG_LINES: usize = 200;

#[derive(Clone)]
pub(crate) struct DiagnosticsState {
    pub(crate) data_dir: PathBuf,
    pub(crate) app_version: String,
    pub(crate) http: HttpsClient,
    pub(crate) helix: String,
    pub(crate) twitch: Arc<RwLock<TwitchAuth>>,
    pub(crate) eventsub_up: Arc<dyn Fn() -> bool + Send + Sync>,
    pub(crate) obs: watch::Receiver<StreamingState>,
    pub(crate) modules: Arc<Modules>,
}

/// What the connection checks found just now.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct Connections {
    pub(crate) twitch_signed_in: bool,
    /// Round trip of one Helix call; `null` when signed out or it failed.
    pub(crate) twitch_latency_ms: Option<u64>,
    pub(crate) eventsub_connected: bool,
    /// Subscriptions Twitch holds for this app and account; `null` when it could not be read.
    pub(crate) eventsub_subscriptions: Option<u64>,
    pub(crate) obs_connected: bool,
}

/// One log line. `message` stays on this page; the report never carries it.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct LogLine {
    /// RFC 3339, as written.
    pub(crate) at: String,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub(crate) level: String,
    /// The error code (`<area>.<reason>`) when the line has one.
    pub(crate) code: Option<String>,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct LogQuery {
    /// The least severe level to list: `error`, `warn` or `info` (default `info`).
    level: Option<String>,
}

/// A log line in the report: no message text, so no names, IDs or secrets.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct ReportLine {
    pub(crate) at: String,
    pub(crate) level: String,
    pub(crate) code: Option<String>,
}

/// The diagnostic report (匯出診斷報告).
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct DiagnosticReport {
    pub(crate) app_version: String,
    /// RFC 3339, UTC.
    pub(crate) generated_at: String,
    pub(crate) connections: Connections,
    /// Modules turned off.
    pub(crate) disabled_modules: Vec<String>,
    /// Newest first.
    pub(crate) log: Vec<ReportLine>,
}

pub(crate) fn routes(state: DiagnosticsState) -> Router {
    Router::new()
        .route("/api/admin/diagnostics/connections", get(connections))
        .route("/api/admin/diagnostics/logs", get(logs))
        .route("/api/admin/diagnostics/report", get(report))
        .with_state(state)
}

fn rank(level: &str) -> u8 {
    match level {
        "ERROR" => 0,
        "WARN" => 1,
        "INFO" => 2,
        "DEBUG" => 3,
        _ => 4,
    }
}

/// The newest `limit` lines at or above `level`, newest first, from the newest log files in `dir`.
pub(crate) fn recent_lines(dir: &Path, level: &str, limit: usize) -> Vec<LogLine> {
    let most = rank(&level.to_ascii_uppercase());
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "log"))
                .collect()
        })
        .unwrap_or_default();
    // Daily files are named by date, so name order is time order.
    files.sort();
    let mut out = Vec::new();
    for file in files.iter().rev() {
        let Ok(f) = std::fs::File::open(file) else {
            continue;
        };
        let mut lines: Vec<LogLine> = BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| {
                let v: serde_json::Value = serde_json::from_str(&l).ok()?;
                let level = v["level"].as_str()?.to_owned();
                (rank(&level) <= most).then(|| LogLine {
                    at: v["timestamp"].as_str().unwrap_or_default().to_owned(),
                    level,
                    code: v["fields"]["code"].as_str().map(str::to_owned),
                    message: v["fields"]["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect();
        lines.reverse();
        out.extend(lines);
        if out.len() >= limit {
            break;
        }
    }
    out.truncate(limit);
    out
}

async fn check(s: &DiagnosticsState) -> Connections {
    let auth = s.twitch.read().await.clone();
    let signed_in = matches!(auth.status(), AuthStatus::SignedIn { .. });
    let (latency, subscriptions) = if signed_in {
        let helix = Helix::new(&s.helix, auth, s.http.clone());
        let start = Instant::now();
        match helix
            .get_json("/eventsub/subscriptions?status=enabled")
            .await
        {
            Ok(body) => (
                Some(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
                body["total"].as_u64(),
            ),
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };
    Connections {
        twitch_signed_in: signed_in,
        twitch_latency_ms: latency,
        eventsub_connected: (s.eventsub_up)(),
        eventsub_subscriptions: subscriptions,
        // A connected OBS always has a scene.
        obs_connected: !s.obs.borrow().scenes.is_empty(),
    }
}

/// Checks the connections now (one Helix call when signed in).
#[utoipa::path(
    get,
    path = "/api/admin/diagnostics/connections",
    tag = "admin",
    responses((status = 200, body = Connections))
)]
pub(crate) async fn connections(State(s): State<DiagnosticsState>) -> Json<Connections> {
    Json(check(&s).await)
}

/// The latest 200 log lines at or above a level, newest first.
#[utoipa::path(
    get,
    path = "/api/admin/diagnostics/logs",
    tag = "admin",
    params(LogQuery),
    responses((status = 200, body = Vec<LogLine>))
)]
pub(crate) async fn logs(
    State(s): State<DiagnosticsState>,
    Query(q): Query<LogQuery>,
) -> Json<Vec<LogLine>> {
    let dir = s.data_dir.join("logs");
    let level = q.level.unwrap_or_else(|| "info".into());
    let lines = tokio::task::spawn_blocking(move || recent_lines(&dir, &level, LOG_LINES))
        .await
        .unwrap_or_default();
    Json(lines)
}

/// A report to share when asking for help: versions, connections, modules turned off and the latest warnings and
/// errors as time, level and code only.
#[utoipa::path(
    get,
    path = "/api/admin/diagnostics/report",
    tag = "admin",
    responses((status = 200, body = DiagnosticReport))
)]
pub(crate) async fn report(State(s): State<DiagnosticsState>) -> Json<DiagnosticReport> {
    let connections = check(&s).await;
    let dir = s.data_dir.join("logs");
    let lines = tokio::task::spawn_blocking(move || recent_lines(&dir, "warn", LOG_LINES))
        .await
        .unwrap_or_default();
    Json(DiagnosticReport {
        app_version: s.app_version.clone(),
        generated_at: time::secs(Timestamp::now()),
        connections,
        disabled_modules: s
            .modules
            .list()
            .into_iter()
            .filter(|m| !m.enabled)
            .map(|m| m.id.to_owned())
            .collect(),
        log: lines
            .into_iter()
            .map(|l| ReportLine {
                at: l.at,
                level: l.level,
                code: l.code,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_read_newest_first_by_level_and_the_report_drops_messages() {
        let dir = std::env::temp_dir().join(format!("st-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let line = |at: &str, level: &str, code: Option<&str>, message: &str| {
            let mut fields = serde_json::json!({ "message": message });
            if let Some(c) = code {
                fields["code"] = c.into();
            }
            serde_json::json!({ "timestamp": at, "level": level, "fields": fields }).to_string()
        };
        std::fs::write(
            dir.join("streamer-toolkit.2026-11-21.log"),
            [
                line(
                    "2026-11-21T10:00:00Z",
                    "ERROR",
                    Some("chat.storage_failed"),
                    "viewer 小魚 (12345) token=abc",
                ),
                line("2026-11-21T10:00:01Z", "INFO", None, "started"),
            ]
            .join("\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("streamer-toolkit.2026-11-22.log"),
            [
                line(
                    "2026-11-22T09:00:00Z",
                    "WARN",
                    Some("obs.unreachable"),
                    "obs",
                ),
                "not json".into(),
            ]
            .join("\n"),
        )
        .unwrap();

        let warn = recent_lines(&dir, "warn", 10);
        assert_eq!(
            warn.iter().map(|l| l.at.as_str()).collect::<Vec<_>>(),
            ["2026-11-22T09:00:00Z", "2026-11-21T10:00:00Z"]
        );
        assert_eq!(recent_lines(&dir, "info", 10).len(), 3);
        assert_eq!(recent_lines(&dir, "info", 1).len(), 1);

        let report: Vec<ReportLine> = warn
            .into_iter()
            .map(|l| ReportLine {
                at: l.at,
                level: l.level,
                code: l.code,
            })
            .collect();
        let text = serde_json::to_string(&report).unwrap();
        for secret in ["小魚", "12345", "token", "abc"] {
            assert!(!text.contains(secret), "{secret}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
