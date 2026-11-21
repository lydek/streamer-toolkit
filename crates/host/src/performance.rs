//! 效能 (troubleshooting): this computer's CPU and memory, this app's, and OBS's CPU, dropped frames and stream
//! bitrate, sampled every 2 seconds. Dropped frames and bitrate come from the last 60 seconds of OBS counters; above
//! 5 % dropped the page warns. Admin route; the caller adds the token check.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use toolkit_core::streaming::{StreamingSoftware, StreamingStats};
use utoipa::ToSchema;

use crate::obs::ObsLink;

const EVERY: Duration = Duration::from_secs(2);
/// The window dropped frames and bitrate are worked out over.
const WINDOW: Duration = Duration::from_secs(60);
/// Dropped frames above this share of the window warn.
pub(crate) const WARN_PERCENT: f64 = 5.0;

/// OBS's numbers; absent while OBS is not connected.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub(crate) struct ObsPerformance {
    pub(crate) cpu_percent: f64,
    /// Frames dropped (rendering and the stream output) in the last 60 s, in percent; `null` before two readings.
    pub(crate) dropped_percent: Option<f64>,
    /// The stream output's bitrate over the last 60 s; `null` while not streaming.
    pub(crate) bitrate_kbps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub(crate) struct Performance {
    pub(crate) cpu_percent: f64,
    pub(crate) memory_used_mb: u64,
    pub(crate) memory_total_mb: u64,
    /// This app's own process.
    pub(crate) app_cpu_percent: f64,
    pub(crate) app_memory_mb: u64,
    #[schema(required)]
    pub(crate) obs: Option<ObsPerformance>,
    /// Dropped frames above 5 % in the last 60 s.
    pub(crate) warning: bool,
}

/// One OBS reading and when it was taken.
#[derive(Debug, Clone, Copy)]
struct Reading {
    at: Instant,
    stats: StreamingStats,
}

/// Dropped-frame percent and bitrate between the oldest and newest readings in the window.
fn rates(readings: &VecDeque<Reading>) -> (Option<f64>, Option<u64>) {
    let (Some(first), Some(last)) = (readings.front(), readings.back()) else {
        return (None, None);
    };
    let (a, b) = (first.stats, last.stats);
    let skipped = (b.render_skipped.saturating_sub(a.render_skipped)
        + b.stream_skipped.saturating_sub(a.stream_skipped)) as f64;
    let total = (b.render_total.saturating_sub(a.render_total)
        + b.stream_total.saturating_sub(a.stream_total)) as f64;
    let dropped = (total > 0.0).then(|| skipped / total * 100.0);
    let secs = last.at.duration_since(first.at).as_secs_f64();
    let bitrate = (b.streaming && a.streaming && secs > 0.0 && b.stream_bytes >= a.stream_bytes)
        .then(|| ((b.stream_bytes - a.stream_bytes) as f64 * 8.0 / secs / 1000.0) as u64);
    (dropped, bitrate)
}

#[derive(Clone)]
pub(crate) struct PerformanceState {
    latest: Arc<Mutex<Option<Performance>>>,
}

/// Samples every 2 seconds. Needs a Tokio runtime.
pub(crate) fn start(obs: Arc<ObsLink>) -> PerformanceState {
    let state = PerformanceState {
        latest: Arc::default(),
    };
    let latest = state.latest.clone();
    tokio::spawn(async move {
        let mut system = match System::new_with_specifics(RefreshKind::nothing()) {
            Ok(system) => system,
            Err(e) => {
                tracing::warn!(error = %e, "performance monitor not started");
                return;
            }
        };
        let me = Pid::from_u32(std::process::id());
        let mut readings: VecDeque<Reading> = VecDeque::new();
        let mut tick = tokio::time::interval(EVERY);
        loop {
            tick.tick().await;
            system.refresh_cpu_usage();
            system.refresh_memory();
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[me]),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
            let obs_now = match obs.stats().await {
                Ok(stats) => {
                    let now = Instant::now();
                    // A restarted OBS starts its counters again: begin a new window.
                    if readings
                        .back()
                        .is_some_and(|r| stats.render_total < r.stats.render_total)
                    {
                        readings.clear();
                    }
                    readings.push_back(Reading { at: now, stats });
                    while readings
                        .front()
                        .is_some_and(|r| now.duration_since(r.at) > WINDOW)
                    {
                        readings.pop_front();
                    }
                    let (dropped, bitrate) = rates(&readings);
                    Some(ObsPerformance {
                        cpu_percent: stats.cpu_percent,
                        dropped_percent: dropped,
                        bitrate_kbps: bitrate,
                    })
                }
                Err(_) => {
                    readings.clear();
                    None
                }
            };
            let process = system.process(me);
            let mb = |bytes: u64| bytes / (1024 * 1024);
            let sample = Performance {
                cpu_percent: f64::from(system.global_cpu_usage()),
                memory_used_mb: mb(system.used_memory()),
                memory_total_mb: mb(system.total_memory()),
                app_cpu_percent: process.map_or(0.0, |p| f64::from(p.cpu_usage())),
                app_memory_mb: process.map_or(0, |p| mb(p.memory())),
                warning: obs_now
                    .as_ref()
                    .and_then(|o| o.dropped_percent)
                    .is_some_and(|d| d > WARN_PERCENT),
                obs: obs_now,
            };
            *latest.lock().unwrap() = Some(sample);
        }
    });
    state
}

pub(crate) fn routes(state: PerformanceState) -> Router {
    Router::new()
        .route("/api/admin/performance", get(performance))
        .with_state(state)
}

/// The latest sample; `null` until the first one (2 seconds after start).
#[utoipa::path(
    get,
    path = "/api/admin/performance",
    tag = "admin",
    responses((status = 200, body = Option<Performance>))
)]
pub(crate) async fn performance(State(s): State<PerformanceState>) -> Json<Option<Performance>> {
    Json(s.latest.lock().unwrap().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(
        at: Instant,
        render: (u32, u32),
        stream: (u32, u32),
        bytes: u64,
        streaming: bool,
    ) -> Reading {
        Reading {
            at,
            stats: StreamingStats {
                cpu_percent: 3.0,
                render_skipped: render.0,
                render_total: render.1,
                streaming,
                stream_skipped: stream.0,
                stream_total: stream.1,
                stream_bytes: bytes,
            },
        }
    }

    #[test]
    fn rates_come_from_the_window_and_the_warning_threshold() {
        let t0 = Instant::now();
        let mut window = VecDeque::new();
        assert_eq!(rates(&window), (None, None));
        window.push_back(reading(t0, (0, 1000), (0, 1000), 0, true));
        window.push_back(reading(
            t0 + Duration::from_secs(10),
            (60, 1600),
            (6, 1600),
            7_500_000,
            true,
        ));
        let (dropped, bitrate) = rates(&window);
        // 66 of 1200 frames.
        assert!((dropped.unwrap() - 5.5).abs() < 1e-9);
        assert!(dropped.unwrap() > WARN_PERCENT);
        assert_eq!(bitrate, Some(6000));

        let mut idle = VecDeque::new();
        idle.push_back(reading(t0, (0, 100), (0, 0), 0, false));
        idle.push_back(reading(
            t0 + Duration::from_secs(2),
            (1, 200),
            (0, 0),
            0,
            false,
        ));
        let (dropped, bitrate) = rates(&idle);
        assert!((dropped.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(bitrate, None, "not streaming");
    }
}
