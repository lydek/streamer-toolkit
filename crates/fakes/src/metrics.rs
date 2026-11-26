//! Every request the app sends to the fakes, every event the fakes send, and what they say about the app: counts,
//! gaps, handling time, how many calls an event sets off, and warnings for patterns that look wrong. Warnings never
//! fail anything; they only list what to look at.

use std::{
    collections::{BTreeMap, HashMap},
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

// shortcut: fixed thresholds, make them flags only if a real run needs different values.
/// Two identical requests closer than this look like a duplicate. Below the app's own 2 s performance sampling, so
/// steady polling with timer jitter is not flagged.
pub const DUPLICATE_WINDOW: Duration = Duration::from_millis(1500);
/// This many identical requests in a row, each under `TIGHT_GAP` apart, look like a loop.
pub const TIGHT_RUN: usize = 5;
pub const TIGHT_GAP: Duration = Duration::from_secs(1);
/// Calls counted as set off by an event when they come within this time after it.
pub const REACTION_WINDOW: Duration = Duration::from_secs(1);
/// More calls than this per event, on average, is an amplification warning.
pub const AMPLIFICATION_LIMIT: f64 = 10.0;
/// More requests than this in flight at once is a warning.
pub const MAX_IN_FLIGHT: u32 = 4;
/// Reconnects closer than this are one retry run; within a run each gap must be at least 0.9 × the previous one
/// (`integrations::backoff` doubles up to 60 s; timer jitter makes equal capped gaps measure slightly shorter).
pub const RETRY_RUN: Duration = Duration::from_secs(61);
pub const BACKOFF_FACTOR: f64 = 0.9;
/// This many reconnects in a row under `TIGHT_GAP` apart are too frequent.
pub const RECONNECT_BURST: usize = 3;
/// Twitch's default Helix bucket: 800 points a minute.
pub const HELIX_POINTS_PER_MINUTE: f64 = 800.0;
/// Twitch asks apps to validate tokens at least hourly.
pub const VALIDATE_EVERY: Duration = Duration::from_secs(3600);
/// After a failure, each retry gap must be at least this many times the previous one until `RETRY_CAP`: steady or
/// shrinking retries are no backoff (`integrations::backoff` doubles).
pub const RETRY_GROWTH: f64 = 1.5;
/// Gaps this long count as capped, so equal capped gaps are fine.
pub const RETRY_CAP: Duration = Duration::from_secs(55);
/// More retries than this in one run look like retrying without end.
pub const RETRY_LIMIT: usize = 20;
/// shortcut: the oldest half is dropped past this many entries; enough for a 10-minute load run.
const MAX_ENTRIES: usize = 500_000;

/// One request the app sent to a fake.
#[derive(Debug, Clone)]
pub struct Call {
    pub at: Instant,
    pub service: &'static str,
    /// `GET /helix/users` (no query values), an OBS `requestType`, or `ws-connect <socket>`.
    pub key: String,
    /// The parameters' hash: query and body, or OBS `requestData`.
    pub params: u64,
    pub status: u16,
    pub handled: Duration,
    /// Requests of the same service in flight when this one arrived, itself included.
    pub in_flight: u32,
    /// A missing header or an unknown token.
    pub problem: Option<String>,
}

/// One event a fake sent to the app.
#[derive(Debug, Clone)]
pub struct Sent {
    pub at: Instant,
    pub service: &'static str,
    pub kind: String,
    /// The message, redemption or stream an event is about, when it has one.
    pub id: Option<String>,
    /// `false` for an event the app had no subscription for, so it never went out.
    pub delivered: bool,
}

/// Which way an activity entry went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// A request or connection from the app to a fake.
    In,
    /// An event from a fake to the app.
    Out,
}

/// One line of the activity log: a call the app made or an event a fake sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at: Instant,
    pub service: &'static str,
    pub direction: Direction,
    pub key: String,
    /// The answer's status, for calls.
    pub status: Option<u16>,
    pub handled: Option<Duration>,
    pub id: Option<String>,
    /// `false` only for an event that never went out.
    pub delivered: bool,
}

struct Log {
    since: Instant,
    calls: Vec<Call>,
    events: Vec<Sent>,
    /// `InputVolumeMeters` sent; counted, not logged, since no call is expected after them.
    meters: u64,
    in_flight: HashMap<&'static str, u32>,
    /// Faults set with `fake-services fault`, by call key.
    faults: HashMap<String, Fault>,
}

/// A fault for one call key: answer with `status`, close the connection, and/or wait `delay` first, until `until`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fault {
    pub status: Option<u16>,
    pub close: bool,
    pub delay: Duration,
    pub until: Instant,
}

/// Shared by the fake OBS and the fake Twitch, so one timeline holds both.
#[derive(Clone)]
pub struct Recorder(Arc<Mutex<Log>>);

impl Default for Recorder {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Log {
            since: Instant::now(),
            calls: Vec::new(),
            events: Vec::new(),
            meters: 0,
            in_flight: HashMap::new(),
            faults: HashMap::new(),
        })))
    }
}

/// Counts a request as in flight until dropped.
pub struct InFlight {
    recorder: Recorder,
    service: &'static str,
    pub count: u32,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(n) = self
            .recorder
            .0
            .lock()
            .unwrap()
            .in_flight
            .get_mut(self.service)
        {
            *n = n.saturating_sub(1);
        }
    }
}

pub fn hash(parts: &[&[u8]]) -> u64 {
    let mut h = DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

fn trim<T>(list: &mut Vec<T>) {
    if list.len() > MAX_ENTRIES {
        list.drain(..MAX_ENTRIES / 2);
    }
}

impl Recorder {
    pub fn start(&self, service: &'static str) -> InFlight {
        let mut log = self.0.lock().unwrap();
        let n = log.in_flight.entry(service).or_default();
        *n += 1;
        InFlight {
            recorder: self.clone(),
            service,
            count: *n,
        }
    }

    pub fn call(&self, call: Call) {
        let mut log = self.0.lock().unwrap();
        log.calls.push(call);
        trim(&mut log.calls);
    }

    /// A WebSocket connection, logged as the call `ws-connect <socket>`.
    pub fn connect(&self, service: &'static str, socket: &str) {
        self.call(Call {
            at: Instant::now(),
            service,
            key: format!("ws-connect {socket}"),
            params: 0,
            status: 101,
            handled: Duration::ZERO,
            in_flight: 0,
            problem: None,
        });
    }

    pub fn event(&self, service: &'static str, kind: &str) {
        self.event_with(service, kind, None, true);
    }

    /// An event with the ID it is about; `delivered: false` for one the app had no subscription for.
    pub fn event_with(&self, service: &'static str, kind: &str, id: Option<&str>, delivered: bool) {
        let mut log = self.0.lock().unwrap();
        if kind == "InputVolumeMeters" {
            log.meters += 1;
            return;
        }
        log.events.push(Sent {
            at: Instant::now(),
            service,
            kind: kind.into(),
            id: id.map(str::to_owned),
            delivered,
        });
        trim(&mut log.events);
    }

    /// The latest `n` calls and events, oldest first.
    pub fn recent(&self, n: usize) -> Vec<Entry> {
        let log = self.0.lock().unwrap();
        let calls = log.calls.iter().rev().take(n).map(|c| Entry {
            at: c.at,
            service: c.service,
            direction: Direction::In,
            key: c.key.clone(),
            status: Some(c.status),
            handled: Some(c.handled),
            id: None,
            delivered: true,
        });
        let events = log.events.iter().rev().take(n).map(|e| Entry {
            at: e.at,
            service: e.service,
            direction: Direction::Out,
            key: e.kind.clone(),
            status: None,
            handled: None,
            id: e.id.clone(),
            delivered: e.delivered,
        });
        let mut all: Vec<Entry> = calls.chain(events).collect();
        all.sort_by_key(|e| e.at);
        let skip = all.len().saturating_sub(n);
        all.split_off(skip)
    }

    /// The faults that still last, with the time left.
    pub fn faults(&self) -> Vec<(String, Fault, Duration)> {
        let mut log = self.0.lock().unwrap();
        let now = Instant::now();
        log.faults.retain(|_, f| f.until > now);
        let mut list: Vec<_> = log
            .faults
            .iter()
            .map(|(k, f)| (k.clone(), *f, f.until - now))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    /// Ends the fault for `key`; `false` if there was none.
    pub fn remove_fault(&self, key: &str) -> bool {
        self.0.lock().unwrap().faults.remove(key).is_some()
    }

    /// Makes calls with this key fail, close or slow down until `fault.until`.
    pub fn set_fault(&self, key: &str, fault: Fault) {
        self.0.lock().unwrap().faults.insert(key.into(), fault);
    }

    pub fn clear_faults(&self) {
        self.0.lock().unwrap().faults.clear();
    }

    /// The fault for `key` while it lasts.
    pub fn fault(&self, key: &str) -> Option<Fault> {
        let mut log = self.0.lock().unwrap();
        let now = Instant::now();
        log.faults.retain(|_, f| f.until > now);
        log.faults.get(key).copied()
    }

    pub fn reset(&self) {
        let mut log = self.0.lock().unwrap();
        log.since = Instant::now();
        log.calls.clear();
        log.events.clear();
        log.meters = 0;
    }

    pub fn report(&self) -> Value {
        let log = self.0.lock().unwrap();
        let mut report = analyze(&log.calls, &log.events, log.since, Instant::now());
        report["meters_sent"] = json!(log.meters);
        report
    }
}

fn secs(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0).round() / 1000.0
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

/// Most entries inside any `window`, for sorted times.
fn peak(times: &[Instant], window: Duration) -> usize {
    let (mut best, mut start) = (0, 0);
    for end in 0..times.len() {
        while times[end].duration_since(times[start]) >= window {
            start += 1;
        }
        best = best.max(end - start + 1);
    }
    best
}

fn gaps(times: &[Instant]) -> Vec<Duration> {
    times
        .windows(2)
        .map(|w| w[1].duration_since(w[0]))
        .collect()
}

fn warning(kind: &str, key: &str, at: Duration, detail: String) -> Value {
    json!({"kind": kind, "key": key, "at_s": secs(at), "detail": detail})
}

/// The report for one timeline: per-call statistics, events and what they set off, Helix use, token checks and
/// warnings. Times in the output are seconds since `since` (`at_s`) or milliseconds (`*_ms`).
pub fn analyze(calls: &[Call], events: &[Sent], since: Instant, now: Instant) -> Value {
    let elapsed = now.duration_since(since).max(Duration::from_secs(1));
    let per_minute = |n: usize| (n as f64 * 60.0 / elapsed.as_secs_f64() * 10.0).round() / 10.0;
    let mut calls: Vec<&Call> = calls.iter().collect();
    calls.sort_by_key(|c| c.at);
    // Undelivered events never reached the app, so nothing follows them.
    let mut events: Vec<&Sent> = events.iter().filter(|e| e.delivered).collect();
    events.sort_by_key(|e| e.at);
    let mut warnings = Vec::new();

    let mut by_key: BTreeMap<&str, Vec<&Call>> = BTreeMap::new();
    for c in &calls {
        by_key.entry(&c.key).or_default().push(c);
    }
    let mut rows = Vec::new();
    for (key, list) in &by_key {
        let times: Vec<Instant> = list.iter().map(|c| c.at).collect();
        let g = gaps(&times);
        let handled: Duration = list.iter().map(|c| c.handled).sum();
        rows.push(json!({
            "key": key, "service": list[0].service, "total": list.len(), "per_minute": per_minute(list.len()),
            "peak_10s": peak(&times, Duration::from_secs(10)),
            "avg_gap_ms": (!g.is_empty()).then(|| ms(g.iter().sum::<Duration>() / g.len() as u32)),
            "min_gap_ms": g.iter().min().map(|d| ms(*d)),
            "avg_handled_ms": ms(handled / list.len() as u32),
            "max_in_flight": list.iter().map(|c| c.in_flight).max(),
        }));
    }

    // Duplicates and loops: identical requests, never connections (those have their own rule).
    let mut identical: BTreeMap<(&str, u64), Vec<&Call>> = BTreeMap::new();
    for c in calls.iter().filter(|c| !c.key.starts_with("ws-connect")) {
        identical.entry((&c.key, c.params)).or_default().push(c);
    }
    for ((key, _), list) in &identical {
        let times: Vec<Instant> = list.iter().map(|c| c.at).collect();
        let g = gaps(&times);
        let close: Vec<usize> = (0..g.len()).filter(|i| g[*i] < DUPLICATE_WINDOW).collect();
        if let Some(first) = close.first() {
            let at = times[*first + 1].duration_since(since);
            let detail = format!(
                "{} identical request(s) within {} s of the previous one",
                close.len(),
                DUPLICATE_WINDOW.as_secs_f64()
            );
            warnings.push(warning("duplicate", key, at, detail));
        }
        let mut run = 1;
        for (i, gap) in g.iter().enumerate() {
            run = if *gap < TIGHT_GAP { run + 1 } else { 1 };
            if run == TIGHT_RUN {
                let detail = format!(
                    "{TIGHT_RUN} identical requests in a row, each under 1 s apart: a loop?"
                );
                warnings.push(warning(
                    "tight_loop",
                    key,
                    times[i + 1].duration_since(since),
                    detail,
                ));
                break;
            }
        }
    }

    // What each event set off: calls of the same service within the reaction window, given to the latest event.
    let mut set_off: Vec<Vec<Duration>> = vec![Vec::new(); events.len()];
    for c in calls.iter().filter(|c| !c.key.starts_with("ws-connect")) {
        let latest = events
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.service == c.service && e.at <= c.at);
        if let Some((i, e)) = latest
            && c.at.duration_since(e.at) <= REACTION_WINDOW
        {
            set_off[i].push(c.at.duration_since(e.at));
        }
    }
    let mut by_kind: BTreeMap<&str, (usize, usize, Vec<Duration>, Instant)> = BTreeMap::new();
    for (e, after) in events.iter().zip(&set_off) {
        let entry = by_kind.entry(&e.kind).or_insert((0, 0, Vec::new(), e.at));
        entry.0 += 1;
        entry.1 += after.len();
        if let Some(first) = after.iter().min() {
            entry.2.push(*first);
        }
    }
    let mut event_rows = Vec::new();
    for (kind, (count, calls_after, reactions, first_at)) in &by_kind {
        let amplification = *calls_after as f64 / *count as f64;
        event_rows.push(json!({
            "kind": kind, "sent": count, "calls_after": calls_after,
            "calls_per_event": (amplification * 10.0).round() / 10.0,
            "avg_reaction_ms": (!reactions.is_empty())
                .then(|| ms(reactions.iter().sum::<Duration>() / reactions.len() as u32)),
        }));
        if amplification > AMPLIFICATION_LIMIT {
            let detail = format!(
                "{amplification:.1} calls per event on average (limit {AMPLIFICATION_LIMIT})"
            );
            warnings.push(warning(
                "amplification",
                kind,
                first_at.duration_since(since),
                detail,
            ));
        }
    }

    // Requests waiting on each other.
    let mut worst: BTreeMap<&str, &Call> = BTreeMap::new();
    for c in &calls {
        let w = worst.entry(c.service).or_insert(c);
        if c.in_flight > w.in_flight {
            *w = c;
        }
    }
    for (service, c) in worst {
        if c.in_flight > MAX_IN_FLIGHT {
            let detail = format!(
                "{} {service} requests in flight at once (limit {MAX_IN_FLIGHT})",
                c.in_flight
            );
            warnings.push(warning(
                "in_flight",
                &c.key,
                c.at.duration_since(since),
                detail,
            ));
        }
    }

    // Reconnects: within a retry run, gaps must not shrink, and must not come in bursts.
    for (key, list) in by_key.iter().filter(|(k, _)| k.starts_with("ws-connect")) {
        let times: Vec<Instant> = list.iter().map(|c| c.at).collect();
        let g = gaps(&times);
        let mut burst = 0;
        let mut flagged = false;
        for i in 0..g.len() {
            burst = if g[i] < TIGHT_GAP { burst + 1 } else { 0 };
            let shrinking = i > 0
                && g[i] < RETRY_RUN
                && g[i].as_secs_f64() < BACKOFF_FACTOR * g[i - 1].as_secs_f64();
            if !flagged && (shrinking || burst == RECONNECT_BURST) {
                let detail = if shrinking {
                    format!(
                        "reconnected after {} s, sooner than the {} s before",
                        secs(g[i]),
                        secs(g[i - 1])
                    )
                } else {
                    format!("{RECONNECT_BURST} reconnects in a row under 1 s apart")
                };
                warnings.push(warning(
                    "reconnect_backoff",
                    key,
                    times[i + 1].duration_since(since),
                    detail,
                ));
                flagged = true;
            }
        }
    }

    // Retries: identical requests after a failed answer must wait longer each time, and stop at some point.
    // Failures worth retrying: server errors and 429. OBS's 600s are answers about the resource (e.g. 604: this
    // input has no audio), which the app asks on purpose, so they never start a retry run.
    let failed = |c: &Call| {
        (c.status >= 500 || c.status == 429)
            && !(c.service == "obs" && (600..700).contains(&c.status))
    };
    let mut retries = Vec::new();
    for ((key, _), list) in &identical {
        let mut i = 0;
        while i < list.len() {
            if !failed(list[i]) {
                i += 1;
                continue;
            }
            // A run: the failed call and every identical call that followed a failure.
            let mut end = i;
            while end + 1 < list.len() && failed(list[end]) {
                end += 1;
            }
            let run = &list[i..=end];
            i = end + 1;
            if run.len() < 2 {
                continue;
            }
            let times: Vec<Instant> = run.iter().map(|c| c.at).collect();
            let g = gaps(&times);
            let start = times[0].duration_since(since);
            retries.push(
                json!({"key": key, "at_s": secs(start), "retries": run.len() - 1,
                                "gaps_ms": g.iter().take(20).map(|d| ms(*d)).collect::<Vec<_>>()}),
            );
            let steady = g.windows(2).position(|w| {
                w[0] < RETRY_CAP && w[1].as_secs_f64() < RETRY_GROWTH * w[0].as_secs_f64()
            });
            if let Some(at) = steady {
                let detail = format!(
                    "retried after {} s following a {} s wait: no backoff",
                    secs(g[at + 1]),
                    secs(g[at])
                );
                warnings.push(warning(
                    "retry_backoff",
                    key,
                    times[at + 2].duration_since(since),
                    detail,
                ));
            }
            if run.len() - 1 > RETRY_LIMIT {
                let detail = format!(
                    "{} retries without stopping (limit {RETRY_LIMIT})",
                    run.len() - 1
                );
                warnings.push(warning("retry_without_end", key, start, detail));
            }
        }
    }

    // Headers and tokens.
    let mut problems: BTreeMap<(&str, &str), (usize, Instant)> = BTreeMap::new();
    for c in &calls {
        if let Some(p) = &c.problem {
            problems.entry((&c.key, p)).or_insert((0, c.at)).0 += 1;
        }
    }
    for ((key, problem), (count, at)) in problems {
        warnings.push(warning(
            "headers",
            key,
            at.duration_since(since),
            format!("{problem} ({count}×)"),
        ));
    }

    // Helix use and token checks.
    let helix: Vec<Instant> = calls
        .iter()
        .filter(|c| c.key.contains(" /helix/"))
        .map(|c| c.at)
        .collect();
    let helix_peak = peak(&helix, Duration::from_secs(60));
    let validates: Vec<Instant> = calls
        .iter()
        .filter(|c| c.key == "GET /oauth2/validate")
        .map(|c| c.at)
        .collect();
    let vg = gaps(&validates);
    if elapsed >= VALIDATE_EVERY {
        let since_last = validates
            .last()
            .map_or(elapsed, |last| now.duration_since(*last));
        if since_last > VALIDATE_EVERY || vg.iter().any(|g| *g > VALIDATE_EVERY) {
            let detail = "the token was not validated for over an hour".to_owned();
            warnings.push(warning("validate", "GET /oauth2/validate", elapsed, detail));
        }
    }
    warnings.sort_by(|a, b| {
        a["at_s"]
            .as_f64()
            .partial_cmp(&b["at_s"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    json!({
        "elapsed_s": secs(elapsed),
        "calls": rows,
        "events": event_rows,
        "helix": {"total": helix.len(), "peak_per_minute": helix_peak,
                  "bucket_percent": (helix_peak as f64 / HELIX_POINTS_PER_MINUTE * 1000.0).round() / 10.0},
        "validate": {"total": validates.len(),
                     "avg_interval_s": (!vg.is_empty()).then(|| secs(vg.iter().sum::<Duration>() / vg.len() as u32))},
        "retries": retries,
        "warnings": warnings,
    })
}

/// The report as text for `fake-services stats`.
pub fn text(report: &Value) -> String {
    let mut out = format!("since reset: {} s\n\ncalls\n", report["elapsed_s"]);
    let n = |v: &Value| {
        if v.is_null() {
            "-".into()
        } else {
            v.to_string()
        }
    };
    out.push_str("  total  /min  peak10s  avg gap ms  min gap ms  handled ms  in flight  key\n");
    for c in report["calls"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>5}  {:>4}  {:>7}  {:>10}  {:>10}  {:>10}  {:>9}  {}\n",
            n(&c["total"]),
            n(&c["per_minute"]),
            n(&c["peak_10s"]),
            n(&c["avg_gap_ms"]),
            n(&c["min_gap_ms"]),
            n(&c["avg_handled_ms"]),
            n(&c["max_in_flight"]),
            c["key"].as_str().unwrap_or_default()
        ));
    }
    out.push_str("\nevents sent\n   sent  calls after  per event  reaction ms  kind\n");
    for e in report["events"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>5}  {:>11}  {:>9}  {:>11}  {}\n",
            n(&e["sent"]),
            n(&e["calls_after"]),
            n(&e["calls_per_event"]),
            n(&e["avg_reaction_ms"]),
            e["kind"].as_str().unwrap_or_default()
        ));
    }
    out.push_str(&format!(
        "  (audio meter events sent: {})\n\nhelix: {} calls, peak {} a minute = {}% of 800\n",
        n(&report["meters_sent"]),
        report["helix"]["total"],
        report["helix"]["peak_per_minute"],
        report["helix"]["bucket_percent"]
    ));
    out.push_str(&format!(
        "token checks (/validate): {}, every {} s on average\n",
        report["validate"]["total"],
        n(&report["validate"]["avg_interval_s"])
    ));
    let retries = report["retries"].as_array().cloned().unwrap_or_default();
    if !retries.is_empty() {
        out.push_str("\nretries after a failure\n");
    }
    for r in retries {
        out.push_str(&format!(
            "  {:>8} s  {}: {} retries, gaps ms {}\n",
            n(&r["at_s"]),
            r["key"].as_str().unwrap_or_default(),
            n(&r["retries"]),
            n(&r["gaps_ms"])
        ));
    }
    out.push_str("\nwarnings\n");
    let warnings = report["warnings"].as_array().cloned().unwrap_or_default();
    if warnings.is_empty() {
        out.push_str("  none\n");
    }
    for w in warnings {
        out.push_str(&format!(
            "  {:>8} s  {}  {}: {}\n",
            n(&w["at_s"]),
            w["kind"].as_str().unwrap_or_default(),
            w["key"].as_str().unwrap_or_default(),
            w["detail"].as_str().unwrap_or_default()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn call(base: Instant, ms_at: u64, key: &str, params: u64) -> Call {
        Call {
            at: at(base, ms_at),
            service: if key.contains('/') { "twitch" } else { "obs" },
            key: key.into(),
            params,
            status: 200,
            handled: Duration::from_millis(2),
            in_flight: 1,
            problem: None,
        }
    }

    fn kinds(report: &Value) -> Vec<String> {
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["kind"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn counts_gaps_and_peaks() {
        let base = Instant::now();
        let calls: Vec<Call> = (0..4)
            .map(|i| call(base, i * 3000, "GET /helix/users", i))
            .collect();
        let r = analyze(&calls, &[], base, at(base, 60_000));
        let row = &r["calls"][0];
        assert_eq!(
            (row["total"].as_u64(), row["per_minute"].as_f64()),
            (Some(4), Some(4.0))
        );
        assert_eq!(
            (row["avg_gap_ms"].as_f64(), row["min_gap_ms"].as_f64()),
            (Some(3000.0), Some(3000.0))
        );
        assert_eq!(row["peak_10s"], 4);
        assert_eq!(r["helix"]["peak_per_minute"], 4);
        assert!(kinds(&r).is_empty(), "different parameters, 3 s apart: {r}");
    }

    #[test]
    fn polling_every_two_seconds_is_no_duplicate() {
        let base = Instant::now();
        let calls: Vec<Call> = (0..10)
            .map(|i| call(base, i * 1990, "GetStats", 7))
            .collect();
        assert!(kinds(&analyze(&calls, &[], base, at(base, 30_000))).is_empty());
    }

    #[test]
    fn duplicates_and_loops_are_flagged() {
        let base = Instant::now();
        let calls: Vec<Call> = (0..6)
            .map(|i| call(base, i * 200, "GET /helix/users", 1))
            .collect();
        assert_eq!(
            kinds(&analyze(&calls, &[], base, at(base, 5000))),
            ["duplicate", "tight_loop"]
        );
    }

    #[test]
    fn undelivered_events_set_off_nothing() {
        let base = Instant::now();
        let events = vec![Sent {
            at: base,
            service: "twitch",
            kind: "channel.cheer".into(),
            id: Some("fake-1".into()),
            delivered: false,
        }];
        let calls: Vec<Call> = (1..=12)
            .map(|i| call(base, i * 10, "GET /helix/users", i))
            .collect();
        let r = analyze(&calls, &events, base, at(base, 5000));
        assert!(r["events"].as_array().unwrap().is_empty(), "{r}");
    }

    #[test]
    fn recent_entries_are_merged_by_time_and_capped() {
        let r = Recorder::default();
        r.call(call(Instant::now(), 0, "GetStats", 1));
        r.event_with("twitch", "channel.chat.message", Some("fake-7"), true);
        r.event_with("twitch", "channel.cheer", None, false);
        r.call(call(Instant::now(), 0, "GET /helix/users", 2));
        let all = r.recent(10);
        let keys: Vec<&str> = all.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "GetStats",
                "channel.chat.message",
                "channel.cheer",
                "GET /helix/users"
            ]
        );
        assert_eq!(
            (all[1].direction, all[1].id.as_deref()),
            (Direction::Out, Some("fake-7"))
        );
        assert!(!all[2].delivered);
        assert_eq!(r.recent(2).len(), 2);
        assert_eq!(r.recent(2)[1].key, "GET /helix/users", "the latest ones");
    }

    #[test]
    fn faults_are_listed_with_time_left_and_removed_one_by_one() {
        let r = Recorder::default();
        let until = Instant::now() + Duration::from_secs(30);
        for key in ["GET /helix/users", "GetStats"] {
            r.set_fault(
                key,
                Fault {
                    status: Some(500),
                    close: false,
                    delay: Duration::ZERO,
                    until,
                },
            );
        }
        let list = r.faults();
        assert_eq!(list.len(), 2);
        assert!(list[0].2 <= Duration::from_secs(30) && list[0].2 > Duration::from_secs(25));
        assert!(r.remove_fault("GetStats"));
        assert!(!r.remove_fault("GetStats"));
        assert_eq!(r.faults().len(), 1);
        assert!(r.fault("GET /helix/users").is_some());
    }

    #[test]
    fn many_calls_after_each_event_are_amplification() {
        let base = Instant::now();
        let events = vec![Sent {
            at: base,
            service: "obs",
            kind: "SceneItemCreated".into(),
            id: None,
            delivered: true,
        }];
        let calls: Vec<Call> = (1..=12)
            .map(|i| call(base, i * 10, "GetSceneItemList", i))
            .collect();
        let r = analyze(&calls, &events, base, at(base, 5000));
        assert_eq!(kinds(&r), ["amplification"]);
        assert_eq!(r["events"][0]["avg_reaction_ms"], 10.0);
    }

    #[test]
    fn reconnects_must_back_off() {
        let base = Instant::now();
        let connects = |times: &[u64]| -> Vec<Call> {
            times
                .iter()
                .map(|t| call(base, *t, "ws-connect eventsub", 0))
                .collect()
        };
        let backing_off = connects(&[0, 1000, 3000, 7000, 15000]);
        assert!(kinds(&analyze(&backing_off, &[], base, at(base, 20_000))).is_empty());
        let burst = connects(&[0, 200, 400, 600]);
        assert_eq!(
            kinds(&analyze(&burst, &[], base, at(base, 5000))),
            ["reconnect_backoff"]
        );
        let shrinking = connects(&[0, 4000, 6000]);
        assert_eq!(
            kinds(&analyze(&shrinking, &[], base, at(base, 10_000))),
            ["reconnect_backoff"]
        );
    }

    #[test]
    fn header_problems_and_crowding_are_flagged() {
        let base = Instant::now();
        let mut bad = call(base, 0, "GET /helix/users", 1);
        bad.problem = Some("missing Client-Id".into());
        let mut crowded = call(base, 3000, "GetStats", 2);
        crowded.in_flight = 9;
        assert_eq!(
            kinds(&analyze(&[bad, crowded], &[], base, at(base, 5000))),
            ["headers", "in_flight"]
        );
    }

    #[test]
    fn an_hour_without_a_token_check_is_flagged() {
        let base = Instant::now();
        let calls = [call(base, 0, "GET /oauth2/validate", 0)];
        assert!(kinds(&analyze(&calls, &[], base, at(base, 1000))).is_empty());
        assert_eq!(
            kinds(&analyze(&calls, &[], base, at(base, 3_700_000))),
            ["validate"]
        );
    }

    fn failing(base: Instant, times: &[u64], key: &str) -> Vec<Call> {
        times
            .iter()
            .map(|t| Call {
                status: 500,
                ..call(base, *t, key, 1)
            })
            .collect()
    }

    #[test]
    fn retries_must_back_off_and_stop() {
        let base = Instant::now();
        let doubling = failing(base, &[0, 1000, 3000, 7000, 15000], "GET /helix/users");
        let r = analyze(&doubling, &[], base, at(base, 20_000));
        assert_eq!(r["retries"][0]["retries"], 4);
        assert_eq!(
            r["retries"][0]["gaps_ms"],
            json!([1000.0, 2000.0, 4000.0, 8000.0])
        );
        assert!(!kinds(&r).contains(&"retry_backoff".to_owned()), "{r}");

        let steady = failing(base, &[0, 2000, 4000, 6000], "GET /helix/users");
        assert!(
            kinds(&analyze(&steady, &[], base, at(base, 10_000)))
                .contains(&"retry_backoff".to_owned())
        );

        let endless: Vec<u64> = (0..25).map(|i| i * 60_000).collect();
        let r = analyze(
            &failing(base, &endless, "GET /helix/users"),
            &[],
            base,
            at(base, 1_600_000),
        );
        let found = kinds(&r);
        assert!(found.contains(&"retry_without_end".to_owned()), "{found:?}");
        assert!(
            !found.contains(&"retry_backoff".to_owned()),
            "capped gaps are fine, the count is not"
        );
    }

    #[test]
    fn obs_resource_answers_are_no_retries() {
        let base = Instant::now();
        let probes: Vec<Call> = (0..30)
            .map(|i| Call {
                status: 604,
                ..call(base, i * 2000, "GetInputMute", 1)
            })
            .collect();
        let r = analyze(&probes, &[], base, at(base, 70_000));
        assert!(r["retries"].as_array().unwrap().is_empty(), "{r}");
        assert!(kinds(&r).is_empty(), "{r}");
    }

    #[test]
    fn the_text_lists_every_section() {
        let base = Instant::now();
        let r = analyze(&[call(base, 0, "GetStats", 0)], &[], base, at(base, 1000));
        let t = text(&r);
        for part in [
            "calls",
            "events sent",
            "helix:",
            "token checks",
            "warnings",
            "none",
            "GetStats",
        ] {
            assert!(t.contains(part), "{part} in\n{t}");
        }
    }
}
