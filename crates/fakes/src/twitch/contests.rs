//! Polls and predictions: `GET`/`POST`/`PATCH /helix/polls` and `/helix/predictions` with the `channel.poll.*` and
//! `channel.prediction.*` events, as Twitch sends them. Votes and bets come from the panel or `fake-services poll
//! vote` / `prediction bet`; a poll ends, and a prediction locks, on its own when its time is up.

use super::*;

/// A poll as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PollView {
    pub id: String,
    pub title: String,
    /// Helix's: `ACTIVE`, `COMPLETED`, `TERMINATED` or `ARCHIVED`.
    pub status: String,
    /// `(title, votes)`.
    pub choices: Vec<(String, u64)>,
}

/// The latest poll, as Helix returns it; it stays after it ends, as Get Polls lists it.
pub(super) struct Poll {
    pub helix: Value,
}

impl Poll {
    fn active(&self) -> bool {
        self.helix["status"] == "ACTIVE"
    }

    pub(super) fn view(&self) -> PollView {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        PollView {
            id: text(&self.helix["id"]),
            title: text(&self.helix["title"]),
            status: text(&self.helix["status"]),
            choices: self.helix["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| (text(&c["title"]), c["votes"].as_u64().unwrap_or_default()))
                .collect(),
        }
    }

    /// The EventSub event body: `begin` without votes, `progress` with them, `end` with the status as well.
    fn event(&self, kind: &str) -> Value {
        let h = &self.helix;
        let choices: Vec<Value> = h["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| match kind {
                "begin" => json!({"id": c["id"], "title": c["title"]}),
                _ => json!({"id": c["id"], "title": c["title"], "votes": c["votes"],
                            "channel_points_votes": 0, "bits_votes": 0}),
            })
            .collect();
        let mut event = json!({
            "id": h["id"], "broadcaster_user_id": BROADCASTER_ID, "broadcaster_user_login": BROADCASTER_LOGIN,
            "broadcaster_user_name": BROADCASTER_NAME, "title": h["title"], "choices": choices,
            "bits_voting": {"is_enabled": false, "amount_per_vote": 0},
            "channel_points_voting": {"is_enabled": false, "amount_per_vote": 0},
            "started_at": h["started_at"],
        });
        if kind == "end" {
            event["status"] = json!(
                h["status"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            );
            event["ended_at"] = h["ended_at"].clone();
        } else {
            event["ends_at"] = h["ends_at_"].clone();
        }
        event
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl FakeTwitch {
    /// Adds `count` votes to choice `choice` (1-based) of the running poll and sends `channel.poll.progress`.
    pub fn poll_vote(&self, choice: usize, count: u64) -> Result<Value, String> {
        let event = {
            let mut w = self.0.world.lock().unwrap();
            let poll = w
                .poll
                .as_mut()
                .filter(|p| p.active())
                .ok_or("no poll is running")?;
            let votes = poll.helix["choices"]
                .get_mut(choice.wrapping_sub(1))
                .ok_or_else(|| format!("the poll has no choice {choice}"))?;
            votes["votes"] = json!(votes["votes"].as_u64().unwrap_or_default() + count);
            poll.event("progress")
        };
        let id = event["id"].as_str().map(str::to_owned);
        self.notify("channel.poll.progress", event, id.as_deref());
        Ok(json!({ "voted": choice }))
    }

    /// Ends the running poll as `status` (`COMPLETED`: its time is up) and sends `channel.poll.end`.
    pub fn poll_end(&self, status: &str) -> Result<Value, String> {
        let (helix, event) = {
            let mut w = self.0.world.lock().unwrap();
            let poll = w
                .poll
                .as_mut()
                .filter(|p| p.active())
                .ok_or("no poll is running")?;
            poll.helix["status"] = json!(status);
            poll.helix["ended_at"] = json!(now());
            (poll.helix.clone(), poll.event("end"))
        };
        self.notify("channel.poll.end", event, helix["id"].as_str());
        Ok(helix)
    }
}

fn helix_poll(helix: &Value) -> Value {
    let mut out = helix.clone();
    // Kept only for the events; Helix gives the start and the duration instead.
    out.as_object_mut().map(|o| o.remove("ends_at_"));
    out
}

/// `?broadcaster_id=…&first=…[&id=…]`: the latest poll.
pub(super) async fn get_polls(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
) -> Response {
    let w = fake.0.world.lock().unwrap();
    let id = param(&query, "id");
    let data: Vec<Value> = w
        .poll
        .iter()
        .filter(|p| id.is_none_or(|id| p.helix["id"] == id))
        .map(|p| helix_poll(&p.helix))
        .collect();
    Json(json!({"data": data, "pagination": {}})).into_response()
}

/// `{"broadcaster_id", "title", "choices": [{"title"}], "duration"}`; one poll at a time, as on Twitch.
pub(super) async fn create_poll(
    State(fake): State<FakeTwitch>,
    Json(body): Json<Value>,
) -> Response {
    if body["broadcaster_id"] != BROADCASTER_ID {
        return rejected(
            StatusCode::UNAUTHORIZED,
            "The broadcaster_id must match the token.",
        );
    }
    let titles: Vec<&str> = body["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["title"].as_str())
        .collect();
    let duration = body["duration"].as_u64().unwrap_or_default();
    let title = body["title"].as_str().unwrap_or_default();
    if title.trim().is_empty()
        || !(2..=5).contains(&titles.len())
        || !(15..=1800).contains(&duration)
    {
        return rejected(StatusCode::BAD_REQUEST, "The poll is not valid.");
    }
    let id = fake.id("fake-poll");
    let started = now_secs();
    let helix = json!({
        "id": id, "broadcaster_id": BROADCASTER_ID, "broadcaster_name": BROADCASTER_NAME,
        "broadcaster_login": BROADCASTER_LOGIN, "title": title,
        "choices": titles.iter().enumerate().map(|(n, t)| json!({
            "id": format!("{id}-choice-{}", n + 1), "title": t, "votes": 0, "channel_points_votes": 0, "bits_votes": 0,
        })).collect::<Vec<_>>(),
        "bits_voting_enabled": false, "bits_per_vote": 0, "channel_points_voting_enabled": false,
        "channel_points_per_vote": 0, "status": "ACTIVE", "duration": duration,
        "started_at": rfc3339(started), "ended_at": null, "ends_at_": rfc3339(started + duration),
    });
    let event = {
        let mut w = fake.0.world.lock().unwrap();
        if w.poll.as_ref().is_some_and(Poll::active) {
            return rejected(StatusCode::BAD_REQUEST, "A poll is already running.");
        }
        let poll = Poll {
            helix: helix.clone(),
        };
        let event = poll.event("begin");
        w.poll = Some(poll);
        event
    };
    fake.notify("channel.poll.begin", event, Some(&id));
    // Time up: the poll completes unless it was ended first.
    let timer = fake.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(duration)).await;
        let still = timer
            .0
            .world
            .lock()
            .unwrap()
            .poll
            .as_ref()
            .is_some_and(|p| p.active() && p.helix["id"] == id);
        if still {
            let _ = timer.poll_end("COMPLETED");
        }
    });
    Json(json!({ "data": [helix_poll(&helix)] })).into_response()
}

/// `{"broadcaster_id", "id", "status": "TERMINATED" | "ARCHIVED"}`: only the running poll.
pub(super) async fn end_poll(State(fake): State<FakeTwitch>, Json(body): Json<Value>) -> Response {
    let status = match body["status"].as_str() {
        Some(s @ ("TERMINATED" | "ARCHIVED")) => s,
        _ => {
            return rejected(
                StatusCode::BAD_REQUEST,
                "The status must be TERMINATED or ARCHIVED.",
            );
        }
    };
    let running = fake
        .0
        .world
        .lock()
        .unwrap()
        .poll
        .as_ref()
        .is_some_and(|p| p.active() && p.helix["id"] == body["id"]);
    if !running {
        return rejected(StatusCode::BAD_REQUEST, "The poll is not running.");
    }
    match fake.poll_end(status) {
        Ok(helix) => Json(json!({ "data": [helix_poll(&helix)] })).into_response(),
        Err(e) => rejected(StatusCode::BAD_REQUEST, &e),
    }
}

/// A prediction as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictionView {
    pub id: String,
    pub title: String,
    /// Helix's: `ACTIVE`, `LOCKED`, `RESOLVED` or `CANCELED`.
    pub status: String,
    /// `(title, users, channel points)`.
    pub outcomes: Vec<(String, u64, u64)>,
}

/// The latest prediction, as Helix returns it; it stays after it ends, as Get Predictions lists it.
pub(super) struct Prediction {
    pub helix: Value,
    /// When predicting closes (RFC 3339), for the events; Helix gives the start and the window instead.
    locks_at: String,
}

impl Prediction {
    /// Still taking bets or waiting for a result: no new prediction may start.
    fn running(&self) -> bool {
        matches!(self.helix["status"].as_str(), Some("ACTIVE" | "LOCKED"))
    }

    pub(super) fn view(&self) -> PredictionView {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        PredictionView {
            id: text(&self.helix["id"]),
            title: text(&self.helix["title"]),
            status: text(&self.helix["status"]),
            outcomes: self.helix["outcomes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| {
                    let n = |k: &str| o[k].as_u64().unwrap_or_default();
                    (text(&o["title"]), n("users"), n("channel_points"))
                })
                .collect(),
        }
    }

    /// The EventSub event body for `begin`, `progress`, `lock` or `end`.
    fn event(&self, kind: &str) -> Value {
        let h = &self.helix;
        let outcomes: Vec<Value> = h["outcomes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| {
                let color = json!(o["color"].as_str().unwrap_or_default().to_ascii_lowercase());
                match kind {
                    "begin" => json!({"id": o["id"], "title": o["title"], "color": color}),
                    _ => json!({"id": o["id"], "title": o["title"], "color": color, "users": o["users"],
                                "channel_points": o["channel_points"], "top_predictors": []}),
                }
            })
            .collect();
        let mut event = json!({
            "id": h["id"], "broadcaster_user_id": BROADCASTER_ID, "broadcaster_user_login": BROADCASTER_LOGIN,
            "broadcaster_user_name": BROADCASTER_NAME, "title": h["title"], "outcomes": outcomes,
            "started_at": h["created_at"],
        });
        match kind {
            "lock" => event["locked_at"] = h["locked_at"].clone(),
            "end" => {
                event["status"] = json!(
                    h["status"]
                        .as_str()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                );
                event["winning_outcome_id"] = h["winning_outcome_id"].clone();
                event["ended_at"] = h["ended_at"].clone();
            }
            _ => event["locks_at"] = json!(self.locks_at),
        }
        event
    }
}

impl FakeTwitch {
    /// Viewers bet `points` (as `users` new predictors) on outcome `outcome` (1-based) of the open prediction, and
    /// `channel.prediction.progress` goes out.
    pub fn prediction_bet(&self, outcome: usize, users: u64, points: u64) -> Result<Value, String> {
        let event = {
            let mut w = self.0.world.lock().unwrap();
            let p = w
                .prediction
                .as_mut()
                .filter(|p| p.helix["status"] == "ACTIVE")
                .ok_or("no prediction is taking bets")?;
            let o = p.helix["outcomes"]
                .get_mut(outcome.wrapping_sub(1))
                .ok_or_else(|| format!("the prediction has no outcome {outcome}"))?;
            o["users"] = json!(o["users"].as_u64().unwrap_or_default() + users);
            o["channel_points"] = json!(o["channel_points"].as_u64().unwrap_or_default() + points);
            p.event("progress")
        };
        let id = event["id"].as_str().map(str::to_owned);
        self.notify("channel.prediction.progress", event, id.as_deref());
        Ok(json!({ "bet": outcome }))
    }

    /// Closes predicting on the open prediction (its window is over, or someone locked it) and sends
    /// `channel.prediction.lock`.
    pub fn prediction_lock(&self) -> Result<Value, String> {
        self.prediction_change("LOCKED", None)
    }

    /// `LOCKED`, `RESOLVED` (with the winner) or `CANCELED`, as Twitch allows from where the prediction stands.
    fn prediction_change(&self, status: &str, winner: Option<&str>) -> Result<Value, String> {
        let (helix, kind, event) = {
            let mut w = self.0.world.lock().unwrap();
            let p = w
                .prediction
                .as_mut()
                .filter(|p| p.running())
                .ok_or("no prediction is running")?;
            match status {
                "LOCKED" if p.helix["status"] != "ACTIVE" => {
                    return Err("the prediction is already locked".into());
                }
                "RESOLVED" => {
                    let known = p.helix["outcomes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|o| winner.is_some_and(|w| o["id"] == w));
                    if !known {
                        return Err("winning_outcome_id is not one of the outcomes".into());
                    }
                    p.helix["winning_outcome_id"] = json!(winner);
                }
                _ => {}
            }
            p.helix["status"] = json!(status);
            let kind = if status == "LOCKED" {
                p.helix["locked_at"] = json!(now());
                "lock"
            } else {
                p.helix["ended_at"] = json!(now());
                "end"
            };
            (p.helix.clone(), kind, p.event(kind))
        };
        self.notify(
            &format!("channel.prediction.{kind}"),
            event,
            helix["id"].as_str(),
        );
        Ok(helix)
    }
}

/// `?broadcaster_id=…&first=…[&id=…]`: the latest prediction.
pub(super) async fn get_predictions(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
) -> Response {
    let w = fake.0.world.lock().unwrap();
    let id = param(&query, "id");
    let data: Vec<Value> = w
        .prediction
        .iter()
        .filter(|p| id.is_none_or(|id| p.helix["id"] == id))
        .map(|p| p.helix.clone())
        .collect();
    Json(json!({"data": data, "pagination": {}})).into_response()
}

/// `{"broadcaster_id", "title", "outcomes": [{"title"}], "prediction_window"}`; one at a time, as on Twitch.
pub(super) async fn create_prediction(
    State(fake): State<FakeTwitch>,
    Json(body): Json<Value>,
) -> Response {
    if body["broadcaster_id"] != BROADCASTER_ID {
        return rejected(
            StatusCode::UNAUTHORIZED,
            "The broadcaster_id must match the token.",
        );
    }
    let titles: Vec<&str> = body["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o["title"].as_str())
        .collect();
    let window = body["prediction_window"].as_u64().unwrap_or_default();
    let title = body["title"].as_str().unwrap_or_default();
    if title.trim().is_empty()
        || !(2..=10).contains(&titles.len())
        || !(30..=1800).contains(&window)
    {
        return rejected(StatusCode::BAD_REQUEST, "The prediction is not valid.");
    }
    let id = fake.id("fake-prediction");
    let started = now_secs();
    let helix = json!({
        "id": id, "broadcaster_id": BROADCASTER_ID, "broadcaster_name": BROADCASTER_NAME,
        "broadcaster_login": BROADCASTER_LOGIN, "title": title, "winning_outcome_id": null,
        "outcomes": titles.iter().enumerate().map(|(n, t)| json!({
            "id": format!("{id}-outcome-{}", n + 1), "title": t, "users": 0, "channel_points": 0,
            "top_predictors": null, "color": if n == 0 { "BLUE" } else { "PINK" },
        })).collect::<Vec<_>>(),
        "prediction_window": window, "status": "ACTIVE", "created_at": rfc3339(started),
        "ended_at": null, "locked_at": null,
    });
    let event = {
        let mut w = fake.0.world.lock().unwrap();
        if w.prediction.as_ref().is_some_and(Prediction::running) {
            return rejected(StatusCode::BAD_REQUEST, "A prediction is already running.");
        }
        let p = Prediction {
            helix: helix.clone(),
            locks_at: rfc3339(started + window),
        };
        let event = p.event("begin");
        w.prediction = Some(p);
        event
    };
    fake.notify("channel.prediction.begin", event, Some(&id));
    // The window closes: the prediction locks unless it was locked or ended first.
    let timer = fake.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(window)).await;
        let open = timer
            .0
            .world
            .lock()
            .unwrap()
            .prediction
            .as_ref()
            .is_some_and(|p| p.helix["status"] == "ACTIVE" && p.helix["id"] == id);
        if open {
            let _ = timer.prediction_lock();
        }
    });
    Json(json!({ "data": [helix] })).into_response()
}

/// `{"broadcaster_id", "id", "status": "LOCKED" | "RESOLVED" | "CANCELED", "winning_outcome_id"?}`.
pub(super) async fn end_prediction(
    State(fake): State<FakeTwitch>,
    Json(body): Json<Value>,
) -> Response {
    let status = match body["status"].as_str() {
        Some(s @ ("LOCKED" | "RESOLVED" | "CANCELED")) => s,
        _ => {
            return rejected(
                StatusCode::BAD_REQUEST,
                "The status must be LOCKED, RESOLVED or CANCELED.",
            );
        }
    };
    let current = fake
        .0
        .world
        .lock()
        .unwrap()
        .prediction
        .as_ref()
        .is_some_and(|p| p.helix["id"] == body["id"]);
    if !current {
        return rejected(StatusCode::BAD_REQUEST, "No such prediction is running.");
    }
    match fake.prediction_change(status, body["winning_outcome_id"].as_str()) {
        Ok(helix) => Json(json!({ "data": [helix] })).into_response(),
        Err(e) => rejected(StatusCode::BAD_REQUEST, &e),
    }
}
