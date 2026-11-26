//! 投票與預測: Twitch's own polls (`channel:manage:polls`) and predictions (`channel:manage:predictions`), started
//! and ended through Helix; their live state arrives through EventSub and is announced as `twitch.contest`
//! (crate::contests). Saved polls stay in `settings.json`.

use super::rewards::broadcaster_with;
use super::*;
use crate::contests::ContestPayload;
use toolkit_core::{events::Contest, settings::PollTemplate};

const SCOPE: &str = "channel:manage:polls";
const PREDICTION_SCOPE: &str = "channel:manage:predictions";
/// Twitch's limits for a prediction.
const PREDICTION_TITLE_MAX_CHARS: usize = 45;
const OUTCOMES: std::ops::RangeInclusive<usize> = 2..=10;
const WINDOW_SECONDS: std::ops::RangeInclusive<u32> = 30..=1800;
/// Twitch's limits for a poll, checked here so the broadcaster is told before anything is sent.
const TITLE_MAX_CHARS: usize = 60;
const CHOICE_MAX_CHARS: usize = 25;
const CHOICES: std::ops::RangeInclusive<usize> = 2..=5;
const SECONDS: std::ops::RangeInclusive<u32> = 15..=1800;
const TEMPLATES_MAX: usize = 20;

#[derive(Deserialize)]
struct PollList {
    data: Vec<serde_json::Value>,
}

fn polls_error(e: HelixError) -> Response {
    match e {
        HelixError::Auth(e) => auth_error(e),
        // E.g. another poll is still running.
        HelixError::Refused(400) => error(StatusCode::BAD_REQUEST, "polls.refused"),
        // The scope was checked first: a channel Twitch offers no polls to (not an affiliate or partner).
        HelixError::Refused(403) => error(StatusCode::FORBIDDEN, "polls.unavailable"),
        e @ (HelixError::RateLimited(_) | HelixError::Refused(_)) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

fn first(answer: serde_json::Value) -> Option<serde_json::Value> {
    serde_json::from_value::<PollList>(answer)
        .ok()
        .and_then(|l| l.data.into_iter().next())
}

/// Hands the poll in a Helix answer to the contest board, then answers with what is on.
fn take(s: &AdminState, answer: serde_json::Value) -> Response {
    if let Some(poll) = first(answer).and_then(integrations::twitch::contests::poll_from_json) {
        s.contests.receive(Contest::Poll(poll));
    }
    Json(s.contests.current()).into_response()
}

/// Hands the prediction in a Helix answer to the contest board.
fn keep_prediction(s: &AdminState, answer: serde_json::Value) {
    if let Some(p) =
        first(answer).and_then(|p| integrations::twitch::contests::prediction_from_json(p, false))
    {
        s.contests.receive(Contest::Prediction(p));
    }
}

/// [`keep_prediction`], then answers with what is on.
fn take_prediction(s: &AdminState, answer: serde_json::Value) -> Response {
    keep_prediction(s, answer);
    Json(s.contests.current()).into_response()
}

fn predictions_error(e: HelixError) -> Response {
    match e {
        HelixError::Auth(e) => auth_error(e),
        // E.g. another prediction is still running, or it was already locked or ended.
        HelixError::Refused(400) => error(StatusCode::BAD_REQUEST, "predictions.refused"),
        HelixError::Refused(403) => error(StatusCode::FORBIDDEN, "polls.unavailable"),
        e @ (HelixError::RateLimited(_) | HelixError::Refused(_)) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

/// A poll's question and choices within Twitch's limits, trimmed; `None` when they are not.
fn checked(title: &str, choices: &[String], seconds: u32) -> Option<(String, Vec<String>)> {
    let title = title.trim();
    let choices: Vec<String> = choices.iter().map(|c| c.trim().to_owned()).collect();
    let ok = !title.is_empty()
        && title.chars().count() <= TITLE_MAX_CHARS
        && CHOICES.contains(&choices.len())
        && choices
            .iter()
            .all(|c| !c.is_empty() && c.chars().count() <= CHOICE_MAX_CHARS)
        && SECONDS.contains(&seconds);
    ok.then(|| (title.to_owned(), choices))
}

#[utoipa::path(
    get,
    path = "/api/admin/polls",
    tag = "admin",
    responses(
        (status = 200, description = "What is on (the latest poll and prediction), read from Twitch and announced as `twitch.contest`", body = ContestPayload),
        (status = 400, description = "Not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` (both scopes are needed) or `polls.unavailable`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn current_polls(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    let id = match broadcaster_with(&twitch, SCOPE)
        .and_then(|id| broadcaster_with(&twitch, PREDICTION_SCOPE).map(|_| id))
    {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match helix
        .get_json(&format!("/predictions?broadcaster_id={id}&first=1"))
        .await
    {
        Ok(answer) => keep_prediction(&s, answer),
        Err(e) => return predictions_error(e),
    }
    match helix
        .get_json(&format!("/polls?broadcaster_id={id}&first=1"))
        .await
    {
        Ok(answer) => take(&s, answer),
        Err(e) => polls_error(e),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewPrediction {
    /// Up to 45 characters.
    title: String,
    /// 2–10, each up to 25 characters.
    outcomes: Vec<String>,
    /// How long predicting stays open: 30–1800.
    seconds: u32,
}

#[utoipa::path(
    post,
    path = "/api/admin/predictions",
    tag = "admin",
    request_body = NewPrediction,
    responses(
        (status = 200, description = "What is on, with the new prediction", body = ContestPayload),
        (status = 400, description = "`predictions.invalid` (over Twitch's limits), `predictions.refused` (e.g. one is already running), or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` or `polls.unavailable`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn start_prediction(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<NewPrediction>,
) -> Response {
    let title = body.title.trim();
    let outcomes: Vec<&str> = body.outcomes.iter().map(|o| o.trim()).collect();
    let ok = !title.is_empty()
        && title.chars().count() <= PREDICTION_TITLE_MAX_CHARS
        && OUTCOMES.contains(&outcomes.len())
        && outcomes
            .iter()
            .all(|o| !o.is_empty() && o.chars().count() <= CHOICE_MAX_CHARS)
        && WINDOW_SECONDS.contains(&body.seconds);
    if !ok {
        return error(StatusCode::BAD_REQUEST, "predictions.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match broadcaster_with(&twitch, PREDICTION_SCOPE) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let request = serde_json::json!({
        "broadcaster_id": id,
        "title": title,
        "outcomes": outcomes.iter().map(|o| serde_json::json!({ "title": o })).collect::<Vec<_>>(),
        "prediction_window": body.seconds,
    });
    match helix.post_json_read("/predictions", &request).await {
        Ok(answer) => take_prediction(&s, answer),
        Err(e) => predictions_error(e),
    }
}

/// What happens to a prediction: no more predictions, a winner (Twitch pays out), or canceled (Twitch refunds).
#[derive(Deserialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum PredictionEnd {
    Lock,
    Resolve { winning_outcome_id: String },
    Cancel,
}

#[utoipa::path(
    post,
    path = "/api/admin/predictions/{id}/end",
    tag = "admin",
    params(("id" = String, Path, description = "Twitch's prediction ID")),
    request_body = PredictionEnd,
    responses(
        (status = 200, description = "What is on, with the prediction locked, resolved or canceled", body = ContestPayload),
        (status = 400, description = "`predictions.invalid`, `predictions.refused` (e.g. already resolved), or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn end_prediction(
    State(s): State<Arc<AdminState>>,
    Path(prediction): Path<String>,
    Json(body): Json<PredictionEnd>,
) -> Response {
    let winner = match &body {
        PredictionEnd::Resolve { winning_outcome_id } => Some(winning_outcome_id.as_str()),
        _ => None,
    };
    if !super::rewards::valid_twitch_id(&prediction)
        || winner.is_some_and(|w| !super::rewards::valid_twitch_id(w))
    {
        return error(StatusCode::BAD_REQUEST, "predictions.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match broadcaster_with(&twitch, PREDICTION_SCOPE) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let mut request = serde_json::json!({ "broadcaster_id": id, "id": prediction });
    request["status"] = match body {
        PredictionEnd::Lock => "LOCKED",
        PredictionEnd::Resolve { .. } => "RESOLVED",
        PredictionEnd::Cancel => "CANCELED",
    }
    .into();
    if let Some(w) = winner {
        request["winning_outcome_id"] = w.into();
    }
    match helix.patch_json_read("/predictions", &request).await {
        Ok(answer) => take_prediction(&s, answer),
        Err(e) => predictions_error(e),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewPoll {
    /// Up to 60 characters.
    title: String,
    /// 2–5, each up to 25 characters.
    choices: Vec<String>,
    /// 15–1800.
    seconds: u32,
}

#[utoipa::path(
    post,
    path = "/api/admin/polls",
    tag = "admin",
    request_body = NewPoll,
    responses(
        (status = 200, description = "What is on, with the new poll", body = ContestPayload),
        (status = 400, description = "`polls.invalid` (over Twitch's limits), `polls.refused` (e.g. a poll is already running), or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required` or `polls.unavailable`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn start_poll(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<NewPoll>,
) -> Response {
    let Some((title, choices)) = checked(&body.title, &body.choices, body.seconds) else {
        return error(StatusCode::BAD_REQUEST, "polls.invalid");
    };
    let twitch = s.twitch.read().await.clone();
    let id = match broadcaster_with(&twitch, SCOPE) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let request = serde_json::json!({
        "broadcaster_id": id,
        "title": title,
        "choices": choices.iter().map(|c| serde_json::json!({ "title": c })).collect::<Vec<_>>(),
        "duration": body.seconds,
    });
    match helix.post_json_read("/polls", &request).await {
        Ok(answer) => take(&s, answer),
        Err(e) => polls_error(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/polls/{id}/end",
    tag = "admin",
    params(("id" = String, Path, description = "Twitch's poll ID")),
    responses(
        (status = 200, description = "What is on, with the poll ended (its result still shows on Twitch)", body = ContestPayload),
        (status = 400, description = "`polls.invalid`, `polls.refused` (e.g. it already ended), or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn end_poll(
    State(s): State<Arc<AdminState>>,
    Path(poll): Path<String>,
) -> Response {
    if !super::rewards::valid_twitch_id(&poll) {
        return error(StatusCode::BAD_REQUEST, "polls.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let id = match broadcaster_with(&twitch, SCOPE) {
        Ok(id) => id,
        Err(denied) => return denied.response(),
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let request = serde_json::json!({ "broadcaster_id": id, "id": poll, "status": "TERMINATED" });
    match helix.patch_json_read("/polls", &request).await {
        Ok(answer) => take(&s, answer),
        Err(e) => polls_error(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/polls/templates",
    tag = "admin",
    responses((status = 200, body = Vec<PollTemplate>))
)]
pub(super) async fn poll_templates(State(s): State<Arc<AdminState>>) -> Json<Vec<PollTemplate>> {
    Json(settings::load(&s.data_dir).poll_templates)
}

#[utoipa::path(
    put,
    path = "/api/admin/polls/templates",
    tag = "admin",
    request_body = Vec<PollTemplate>,
    responses(
        (status = 200, description = "The saved polls as kept (trimmed)", body = Vec<PollTemplate>),
        (status = 400, description = "`polls.invalid`: one is over Twitch's limits, or more than 20", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
pub(super) async fn set_poll_templates(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<Vec<PollTemplate>>,
) -> Response {
    if body.len() > TEMPLATES_MAX {
        return error(StatusCode::BAD_REQUEST, "polls.invalid");
    }
    let mut templates = Vec::with_capacity(body.len());
    for t in body {
        let Some((title, choices)) = checked(&t.title, &t.choices, t.seconds) else {
            return error(StatusCode::BAD_REQUEST, "polls.invalid");
        };
        templates.push(PollTemplate {
            title,
            choices,
            seconds: t.seconds,
        });
    }
    match settings::update(&s.data_dir, |set| set.poll_templates = templates.clone()) {
        Ok(_) => Json(templates).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::checked;

    #[test]
    fn polls_follow_twitch_limits() {
        let two = ["晴天".to_owned(), " 小幸運 ".to_owned()];
        assert_eq!(
            checked(" 下一首？ ", &two, 120),
            Some((
                "下一首？".to_owned(),
                vec!["晴天".to_owned(), "小幸運".to_owned()]
            ))
        );
        assert!(checked("", &two, 120).is_none());
        assert!(checked(&"字".repeat(61), &two, 120).is_none());
        assert!(checked("t", &two[..1], 120).is_none());
        assert!(checked("t", &vec!["a".to_owned(); 6], 120).is_none());
        assert!(checked("t", &["a".to_owned(), "字".repeat(26)], 120).is_none());
        assert!(checked("t", &["a".to_owned(), " ".to_owned()], 120).is_none());
        assert!(checked("t", &two, 14).is_none());
        assert!(checked("t", &two, 1801).is_none());
    }
}
