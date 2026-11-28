//! The control API under `/_control/…` on the fake Twitch's port, and the
//! `fake-services send|obs|eventsub|scenario|stats|fault` commands that call it. Only `127.0.0.1` ever serves it.

use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};

use crate::{
    events::{self, Params},
    metrics::Fault,
    obs::{FakeObs, Stats},
    twitch::FakeTwitch,
};

#[derive(Clone)]
struct Fakes {
    obs: FakeObs,
    twitch: FakeTwitch,
}

/// The control routes; merge them into the fake Twitch's router.
pub fn router(obs: FakeObs, twitch: FakeTwitch) -> Router {
    Router::new()
        .route("/_control/send", post(send))
        .route("/_control/obs", post(obs_control))
        .route("/_control/eventsub/drop", post(eventsub_drop))
        .route("/_control/scenario", post(scenario))
        .route("/_control/stats", post(stats))
        .route("/_control/fault", post(fault))
        .route("/_control/poll", post(poll))
        .route("/_control/prediction", post(prediction))
        .with_state(Fakes { obs, twitch })
}

/// `{"key", "status"?, "delay_ms"?, "for_ms"?, "close"?}` sets a fault for a call key (`GET /helix/users`, an OBS
/// request type, or `ws-connect eventsub|irc|obs`); `{"clear": true}` removes them all.
async fn fault(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    reply(fault_body(&fakes.obs, &fakes.twitch, &body))
}

/// Sets or clears faults as a `fault` command body describes (command or panel).
pub fn fault_body(obs: &FakeObs, twitch: &FakeTwitch, body: &Value) -> Result<Value, String> {
    let recorder = twitch.recorder();
    if body["clear"].as_bool() == Some(true) {
        recorder.clear_faults();
        return Ok(json!({ "cleared": true }));
    }
    let Some(key) = body["key"].as_str().filter(|k| !k.is_empty()) else {
        return Err("a fault needs a call key".into());
    };
    let status = number(&body["status"]).map(|s| s as u16);
    if status.is_some_and(|s| !(100..=999).contains(&s)) {
        return Err("the status must be 100–999".into());
    }
    let delay = Duration::from_millis(number(&body["delay_ms"]).unwrap_or(0.0).max(0.0) as u64);
    let lasts = Duration::from_millis(number(&body["for_ms"]).unwrap_or(60_000.0).max(0.0) as u64);
    let close = body["close"].as_bool() == Some(true);
    if status.is_none() && !close && delay.is_zero() {
        return Err("a fault needs --status, --delay or --close".into());
    }
    let until = std::time::Instant::now() + lasts;
    recorder.set_fault(
        key,
        Fault {
            status,
            close,
            delay,
            until,
        },
    );
    // Closing also ends the connections that are open now.
    if close {
        match key {
            "ws-connect eventsub" => twitch.drop_eventsub(),
            "ws-connect irc" => twitch.drop_irc(),
            "ws-connect obs" => obs.drop_connections(),
            _ => {}
        }
    }
    Ok(
        json!({"key": key, "status": status, "close": close, "delay_ms": delay.as_millis() as u64,
                "for_ms": lasts.as_millis() as u64}),
    )
}

fn reply(result: Result<Value, String>) -> Response {
    match result {
        Ok(result) => Json(result).into_response(),
        Err(e) => bad(e),
    }
}

/// Runs a control command (the words after `fake-services`) in this process, as the panel does: the same body the
/// command would post. Stats and scenarios are not run here (the panel reads the report and awaits scenarios).
pub fn run_command(obs: &FakeObs, twitch: &FakeTwitch, words: &[String]) -> Result<Value, String> {
    let (path, body) = command(words)?;
    match path {
        "/_control/send" => send_body(twitch, &body),
        "/_control/obs" => obs_body(obs, &body),
        "/_control/fault" => fault_body(obs, twitch, &body),
        "/_control/eventsub/drop" => {
            twitch.drop_eventsub();
            Ok(json!({}))
        }
        "/_control/poll" => poll_body(twitch, &body),
        "/_control/prediction" => prediction_body(twitch, &body),
        _ => Err(format!("{path} is not run in the panel")),
    }
}

/// `500ms`, `3s` or `2m` in milliseconds.
fn duration_ms(text: &str) -> Option<u64> {
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = number.parse().ok()?;
    match unit {
        "ms" => Some(n),
        "s" => n.checked_mul(1000),
        "m" => n.checked_mul(60_000),
        _ => None,
    }
}

fn bad(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

/// Sends the event a control body describes (`send` command or panel): `{"kind", "first"?, …Params}`.
pub fn send_body(twitch: &FakeTwitch, body: &Value) -> Result<Value, String> {
    let kind = body["kind"].as_str().unwrap_or_default();
    let first = body["first"].as_bool().unwrap_or(false);
    send_event(twitch, kind, &Params::from_json(body), first)
}

/// Sends one event (and what Twitch sends with it). Returns which Twitch types went out and which the app has no
/// subscription for.
pub fn send_event(
    twitch: &FakeTwitch,
    kind: &str,
    p: &Params,
    first: bool,
) -> Result<Value, String> {
    let twitch_type =
        events::twitch_type(kind).ok_or_else(|| format!("unknown event kind {kind}"))?;
    let (mut sent, mut skipped) = (Vec::new(), Vec::new());
    let id = twitch.id("fake");
    let mut out = |t: &str, event: Value| {
        if twitch.notify(t, event, Some(&id)) {
            sent.push(t.to_owned());
        } else {
            skipped.push(t.to_owned());
        }
    };
    match twitch_type {
        "stream.online" => twitch.set_live(true),
        "stream.offline" => twitch.set_live(false),
        _ => {}
    }
    let mut event = events::event(twitch_type, p, &id).expect("every kind has an event");
    if let (true, Some(at)) = (twitch_type == "stream.online", twitch.started_at()) {
        event["started_at"] = json!(at); // the same start as /helix/streams
    }
    match twitch_type {
        "channel.channel_points_custom_reward_redemption.add" => twitch.redeemed(&mut event),
        "channel.channel_points_custom_reward_redemption.update" => {
            twitch.redemption_changed(&event)
        }
        _ => {}
    }
    out(twitch_type, event);
    // A gift also sends one `channel.subscribe` per recipient, as Twitch does.
    if twitch_type == "channel.subscription.gift" {
        for n in 1..=p.count {
            let recipient = Params {
                user: format!("gift_recipient_{n}"),
                ..p.clone()
            };
            let mut event =
                events::event("channel.subscribe", &recipient, &id).expect("subscribe event");
            event["is_gift"] = json!(true);
            out("channel.subscribe", event);
        }
    }
    if twitch_type == "channel.chat.message" && first {
        twitch.first_chat(&id, &p.user, &p.message);
    }
    Ok(json!({"sent": sent, "skipped": skipped, "id": id}))
}

async fn send(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    reply(send_body(&fakes.twitch, &body))
}

/// A number sent as JSON or as text (the commands send text).
fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

/// `{"action": "drop" | "stats" | "output", …}`.
async fn obs_control(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    reply(obs_body(&fakes.obs, &body))
}

/// Drives the fake OBS as an `obs` command body describes (command or panel).
pub fn obs_body(obs: &FakeObs, body: &Value) -> Result<Value, String> {
    match body["action"].as_str() {
        Some("drop") => obs.drop_connections(),
        Some("stats") => {
            let current = obs.stats();
            obs.set_stats(Stats {
                cpu_percent: number(&body["cpu"]).unwrap_or(current.cpu_percent),
                dropped_percent: number(&body["dropped"])
                    .map_or(current.dropped_percent, |d| d.clamp(0.0, 100.0)),
                bitrate_kbps: number(&body["kbps"])
                    .map_or(current.bitrate_kbps, |k| k.max(0.0) as u64),
            });
        }
        Some("output") => obs.set_output(body["streaming"].as_bool(), body["recording"].as_bool()),
        Some("meters") => {
            let extra = number(&body["inputs"]).unwrap_or(0.0).max(0.0) as usize;
            let every = number(&body["interval"]).unwrap_or(50.0).max(0.0) as u64;
            obs.set_meters(extra, Duration::from_millis(every));
        }
        Some("burst") => {
            let count = number(&body["count"]).unwrap_or(10.0).max(0.0) as usize;
            obs.burst(body["kind"].as_str().unwrap_or_default(), count)?;
        }
        Some("sources") => {
            let count = number(&body["count"]).ok_or("obs sources needs a number")?;
            obs.set_stress_sources(count.max(0.0) as usize)?;
            return Ok(json!({ "sources": obs.stress_sources() }));
        }
        other => return Err(format!("unknown OBS action {other:?}")),
    }
    let s = obs.stats();
    Ok(json!({"cpu": s.cpu_percent, "dropped": s.dropped_percent, "kbps": s.bitrate_kbps}))
}

/// `{"action": "vote", "choice", "count"?}` or `{"action": "end"}` for the running poll.
pub fn poll_body(twitch: &FakeTwitch, body: &Value) -> Result<Value, String> {
    match body["action"].as_str() {
        Some("vote") => {
            let choice = number(&body["choice"]).ok_or("poll vote needs --choice <n>")? as usize;
            let count = number(&body["count"]).unwrap_or(1.0).max(0.0) as u64;
            twitch.poll_vote(choice, count)
        }
        Some("end") => twitch.poll_end("COMPLETED"),
        other => Err(format!("unknown poll action {other:?}")),
    }
}

async fn poll(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    reply(poll_body(&fakes.twitch, &body))
}

/// `{"action": "bet", "outcome", "points"?, "users"?}` or `{"action": "lock"}` for the open prediction.
pub fn prediction_body(twitch: &FakeTwitch, body: &Value) -> Result<Value, String> {
    match body["action"].as_str() {
        Some("bet") => {
            let outcome =
                number(&body["outcome"]).ok_or("prediction bet needs --outcome <n>")? as usize;
            let points = number(&body["points"]).unwrap_or(100.0).max(0.0) as u64;
            let users = number(&body["users"]).unwrap_or(1.0).max(0.0) as u64;
            twitch.prediction_bet(outcome, users, points)
        }
        Some("lock") => twitch.prediction_lock(),
        other => Err(format!("unknown prediction action {other:?}")),
    }
}

async fn prediction(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    reply(prediction_body(&fakes.twitch, &body))
}

async fn eventsub_drop(State(fakes): State<Fakes>) -> Response {
    fakes.twitch.drop_eventsub();
    Json(json!({})).into_response()
}

/// The API calls and events so far (`crate::metrics`); `{"reset": true}` starts over after answering.
async fn stats(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    let recorder = fakes.twitch.recorder();
    let report = recorder.report();
    if body["reset"].as_bool() == Some(true) {
        recorder.reset();
    }
    Json(report).into_response()
}

pub const SCENARIOS: &[(&str, &str)] = &[
    (
        "stream-start",
        "OBS starts streaming and Twitch reports the stream online",
    ),
    (
        "stream-end",
        "OBS stops streaming and Twitch reports the stream offline",
    ),
    ("gift-bomb", "one viewer gifts 20 subs at once"),
    (
        "redemption-flood",
        "30 viewers redeem Check in, 100 ms apart",
    ),
];

/// Runs a scenario to its end (the redemption flood takes about 3 s).
pub async fn run_scenario(obs: &FakeObs, twitch: &FakeTwitch, name: &str) -> Result<Value, String> {
    let p = Params::default();
    match name {
        "stream-start" => {
            obs.set_output(Some(true), None);
            send_event(twitch, "online", &p, false)
        }
        "stream-end" => {
            obs.set_output(Some(false), None);
            send_event(twitch, "offline", &p, false)
        }
        "gift-bomb" => send_event(
            twitch,
            "gift",
            &Params {
                user: "generous_viewer".into(),
                count: 20,
                ..p
            },
            false,
        ),
        "redemption-flood" => {
            for n in 1..=30 {
                let viewer = Params {
                    user: format!("viewer_{n}"),
                    ..Params::default()
                };
                let _ = send_event(twitch, "redeem", &viewer, false);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok(json!({"done": "redemption-flood"}))
        }
        _ => Err(format!("unknown scenario {name}")),
    }
}

async fn scenario(State(fakes): State<Fakes>, Json(body): Json<Value>) -> Response {
    let name = body["name"].as_str().unwrap_or_default().to_owned();
    // The flood runs on after answering, so the command returns at once.
    let result = if name == "redemption-flood" {
        tokio::spawn(async move { run_scenario(&fakes.obs, &fakes.twitch, &name).await });
        Ok(json!({"started": "redemption-flood"}))
    } else {
        run_scenario(&fakes.obs, &fakes.twitch, &name).await
    };
    match result {
        Ok(result) => Json(result).into_response(),
        Err(e) => bad(e),
    }
}

/// The JSON body and path for a control command (the words after `fake-services`), or a usage error.
pub fn command(args: &[String]) -> Result<(&'static str, Value), String> {
    let (word, rest) = args.split_first().ok_or("missing command")?;
    let mut body = json!({});
    let mut flags = |rest: &[String]| -> Result<(), String> {
        let mut it = rest.iter();
        while let Some(flag) = it.next() {
            let key = flag
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument {flag}"))?;
            if key == "first" {
                body["first"] = json!(true);
                continue;
            }
            let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
            // Sent as text; the fake reads numbers from text too (a tier like "2000" stays text).
            body[key] = json!(value);
        }
        Ok(())
    };
    match word.as_str() {
        "send" => {
            let (kind, rest) = rest.split_first().ok_or("send needs an event kind")?;
            events::twitch_type(kind).ok_or_else(|| format!("unknown event kind {kind}"))?;
            flags(rest)?;
            body["kind"] = json!(kind);
            Ok(("/_control/send", body))
        }
        "obs" => {
            let (action, rest) = rest
                .split_first()
                .ok_or("obs needs drop, stats, stream, record, meters, burst or sources")?;
            match action.as_str() {
                "drop" => body["action"] = json!("drop"),
                "stats" => {
                    flags(rest)?;
                    body["action"] = json!("stats");
                }
                "stream" | "record" => {
                    let on = match rest.first().map(String::as_str) {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err(format!("obs {action} needs on or off")),
                    };
                    body["action"] = json!("output");
                    body[if action == "stream" {
                        "streaming"
                    } else {
                        "recording"
                    }] = json!(on);
                }
                "meters" => {
                    flags(rest)?;
                    body["action"] = json!("meters");
                }
                "burst" => {
                    let (kind, rest) = rest
                        .split_first()
                        .ok_or("obs burst needs scenes or items")?;
                    if kind != "scenes" && kind != "items" {
                        return Err(format!("obs burst needs scenes or items, not {kind}"));
                    }
                    flags(rest)?;
                    body["action"] = json!("burst");
                    body["kind"] = json!(kind);
                }
                "sources" => {
                    let count = rest
                        .first()
                        .filter(|n| n.parse::<usize>().is_ok())
                        .ok_or("obs sources needs a number from 0 to 1000")?;
                    body["action"] = json!("sources");
                    body["count"] = json!(count);
                }
                _ => return Err(format!("unknown obs action {action}")),
            }
            Ok(("/_control/obs", body))
        }
        "poll" => {
            match rest.first().map(String::as_str) {
                Some("vote") => {
                    flags(&rest[1..])?;
                    body["action"] = json!("vote");
                }
                Some("end") => body["action"] = json!("end"),
                _ => return Err("poll needs vote --choice <n> [--count <n>] or end".into()),
            }
            Ok(("/_control/poll", body))
        }
        "prediction" => {
            match rest.first().map(String::as_str) {
                Some("bet") => {
                    flags(&rest[1..])?;
                    body["action"] = json!("bet");
                }
                Some("lock") => body["action"] = json!("lock"),
                _ => {
                    return Err(
                        "prediction needs bet --outcome <n> [--points <n>] [--users <n>] or lock"
                            .into(),
                    );
                }
            }
            Ok(("/_control/prediction", body))
        }
        "eventsub" if rest.first().map(String::as_str) == Some("drop") => {
            Ok(("/_control/eventsub/drop", body))
        }
        "stats" => {
            for flag in rest {
                match flag.as_str() {
                    "--reset" => body["reset"] = json!(true),
                    "--json" => body["json"] = json!(true),
                    _ => return Err(format!("unexpected argument {flag}")),
                }
            }
            Ok(("/_control/stats", body))
        }
        "fault" => {
            let (first, rest) = rest
                .split_first()
                .ok_or("fault needs a call key or --clear")?;
            if first == "--clear" {
                body["clear"] = json!(true);
                return Ok(("/_control/fault", body));
            }
            body["key"] = json!(first);
            let mut it = rest.iter();
            while let Some(flag) = it.next() {
                let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
                let time = |v: &String| {
                    duration_ms(v)
                        .ok_or_else(|| format!("{flag} needs a time like 500ms, 3s or 2m"))
                };
                match flag.as_str() {
                    "--status" => body["status"] = json!(value()?),
                    "--delay" => body["delay_ms"] = json!(time(value()?)?),
                    "--for" => body["for_ms"] = json!(time(value()?)?),
                    "--close" => body["close"] = json!(true),
                    _ => return Err(format!("unexpected argument {flag}")),
                }
            }
            Ok(("/_control/fault", body))
        }
        "scenario" => {
            let name = rest.first().ok_or("scenario needs a name")?;
            if !SCENARIOS.iter().any(|(n, _)| n == name) {
                return Err(format!("unknown scenario {name}"));
            }
            body["name"] = json!(name);
            Ok(("/_control/scenario", body))
        }
        _ => Err(format!("unknown command {word}")),
    }
}

/// POSTs a control command to the fake on `127.0.0.1:<port>` and returns the response body.
pub fn call(port: u16, path: &str, body: &Value) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("no fake-services on 127.0.0.1:{port}: {e}"))?;
    let body = body.to_string();
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (head, body) = response.split_once("\r\n\r\n").ok_or("no HTTP response")?;
    match head.split(' ').nth(1) {
        Some(status) if status.starts_with('2') => Ok(body.to_owned()),
        status => Err(format!("{} {body}", status.unwrap_or("?"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(list: &[&str]) -> Result<(&'static str, Value), String> {
        command(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn commands_become_control_requests() {
        let (path, body) = cmd(&["send", "gift", "--count", "20", "--user", "bob"]).unwrap();
        assert_eq!(path, "/_control/send");
        assert_eq!(body, json!({"kind": "gift", "count": "20", "user": "bob"}));
        let (_, body) = cmd(&["send", "chat", "--first"]).unwrap();
        assert_eq!(body["first"], true);
        let (path, body) = cmd(&["obs", "stats", "--dropped", "8"]).unwrap();
        assert_eq!(
            (path, number(&body["dropped"])),
            ("/_control/obs", Some(8.0))
        );
        assert_eq!(cmd(&["obs", "stream", "on"]).unwrap().1["streaming"], true);
        assert_eq!(
            cmd(&["eventsub", "drop"]).unwrap().0,
            "/_control/eventsub/drop"
        );
        assert_eq!(
            cmd(&["poll", "vote", "--choice", "2", "--count", "3"]).unwrap(),
            (
                "/_control/poll",
                json!({"action": "vote", "choice": "2", "count": "3"})
            )
        );
        assert_eq!(cmd(&["poll", "end"]).unwrap().1, json!({"action": "end"}));
        assert!(cmd(&["poll"]).is_err());
        assert_eq!(
            cmd(&["prediction", "bet", "--outcome", "1", "--points", "500"]).unwrap(),
            (
                "/_control/prediction",
                json!({"action": "bet", "outcome": "1", "points": "500"})
            )
        );
        assert_eq!(
            cmd(&["prediction", "lock"]).unwrap().1,
            json!({"action": "lock"})
        );
        assert_eq!(
            cmd(&["scenario", "gift-bomb"]).unwrap().1["name"],
            "gift-bomb"
        );
        let (path, body) = cmd(&[
            "fault",
            "GET /helix/users",
            "--status",
            "500",
            "--for",
            "2m",
            "--delay",
            "300ms",
        ])
        .unwrap();
        assert_eq!(path, "/_control/fault");
        assert_eq!(
            body,
            json!({"key": "GET /helix/users", "status": "500", "for_ms": 120_000, "delay_ms": 300})
        );
        assert_eq!(
            cmd(&["fault", "ws-connect obs", "--close"]).unwrap().1["close"],
            true
        );
        assert_eq!(
            cmd(&["fault", "--clear"]).unwrap().1,
            json!({"clear": true})
        );
        let (_, body) = cmd(&["obs", "meters", "--inputs", "30", "--interval", "50"]).unwrap();
        assert_eq!(
            body,
            json!({"action": "meters", "inputs": "30", "interval": "50"})
        );
        let (_, body) = cmd(&["obs", "burst", "items", "--count", "20"]).unwrap();
        assert_eq!(
            body,
            json!({"action": "burst", "kind": "items", "count": "20"})
        );
        assert_eq!(
            cmd(&["obs", "sources", "300"]).unwrap(),
            (
                "/_control/obs",
                json!({"action": "sources", "count": "300"})
            )
        );
        let (path, body) = cmd(&["stats", "--reset", "--json"]).unwrap();
        assert_eq!(
            (path, body),
            ("/_control/stats", json!({"reset": true, "json": true}))
        );
    }

    #[test]
    fn bad_commands_say_why() {
        for bad in [
            &["send"][..],
            &["send", "nope"],
            &["send", "chat", "--user"],
            &["send", "chat", "stray"],
            &["obs", "sources"],
            &["obs", "sources", "-1"],
            &["obs", "stream"],
            &["obs", "fly"],
            &["scenario", "nope"],
            &["stats", "--nope"],
            &["fault"],
            &["obs", "burst"],
            &["obs", "burst", "fireworks"],
            &["fault", "GET /helix/users", "--for", "soon"],
            &["fault", "GET /helix/users", "--status"],
            &["nope"],
        ] {
            assert!(cmd(bad).is_err(), "{bad:?}");
        }
    }
}
