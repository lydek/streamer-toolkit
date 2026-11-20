use super::*;

/// A channel points reward of the signed-in channel, for picking the check-in reward. `title` is raw Twitch text.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct Reward {
    id: String,
    title: String,
}

#[derive(Deserialize)]
struct RewardList {
    data: Vec<Reward>,
}

/// Helix Get Custom Reward (scope `channel:read:redemptions`). A channel without channel points is refused by
/// Twitch: 502 `twitch.request_refused`.
#[utoipa::path(
    get,
    path = "/api/admin/checkin/rewards",
    tag = "admin",
    responses(
        (status = 200, body = Vec<Reward>),
        (status = 409, description = "`twitch.client_id_required` or `twitch.sign_in_required`", body = ErrorBody),
        (status = 502, description = "`twitch.request_refused`, `twitch.rate_limited` or `twitch.unreachable`", body = ErrorBody),
    )
)]
pub(super) async fn checkin_rewards(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    let Some(user_id) = twitch
        .token()
        .map(|t| t.user_id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    match helix
        .get_json(&format!(
            "/channel_points/custom_rewards?broadcaster_id={user_id}"
        ))
        .await
    {
        Ok(body) => match serde_json::from_value::<RewardList>(body) {
            Ok(list) => Json(list.data).into_response(),
            Err(_) => auth_error(AuthError::InvalidResponse),
        },
        Err(HelixError::Auth(e)) => auth_error(e),
        Err(e @ (HelixError::RateLimited(_) | HelixError::Refused(_))) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

/// Redemptions waiting for the check-in module (while it is paused), and those it failed to take.
#[derive(Serialize, ToSchema)]
pub(crate) struct CheckinOutbox {
    pending: u64,
    /// RFC 3339: when the oldest waiting redemption happened.
    oldest_pending_at: Option<String>,
    dead_letters: Vec<CheckinDeadLetter>,
}

/// A redemption the check-in module failed to take five times in a row. `user_name` and `reward_title` are raw
/// Twitch text.
#[derive(Serialize, ToSchema)]
pub(crate) struct CheckinDeadLetter {
    event_id: String,
    /// RFC 3339: when the redemption happened.
    occurred_at: String,
    /// RFC 3339: the last failed attempt.
    failed_at: String,
    attempts: i64,
    error: String,
    user_name: Option<String>,
    reward_title: Option<String>,
}

fn rfc3339(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/outbox",
    tag = "admin",
    responses((status = 200, body = CheckinOutbox), (status = 500, description = "`outbox.storage_failed`", body = ErrorBody))
)]
pub(super) async fn checkin_outbox(State(s): State<Arc<AdminState>>) -> Response {
    let read = async {
        let backlog = s.outbox.backlog(modules::CHECKIN).await?;
        let dead = s.outbox.dead_letters(modules::CHECKIN).await?;
        Ok::<_, sqlx::Error>((backlog, dead))
    };
    match read.await {
        Ok((backlog, dead)) => Json(CheckinOutbox {
            pending: backlog.pending,
            oldest_pending_at: backlog.oldest.map(rfc3339),
            dead_letters: dead
                .into_iter()
                .map(|d| {
                    let r =
                        serde_json::from_value::<redemption::RedemptionRecorded>(d.event.payload)
                            .ok();
                    CheckinDeadLetter {
                        event_id: d.event.id,
                        occurred_at: rfc3339(d.event.occurred_at),
                        failed_at: rfc3339(d.failed_at),
                        attempts: d.attempts,
                        error: d.last_error,
                        user_name: r.as_ref().map(|r| r.user_name.clone()),
                        reward_title: r.map(|r| r.reward_title),
                    }
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(code = "outbox.storage_failed", error = %e, "outbox not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed")
        }
    }
}

fn dead_letter_result(result: sqlx::Result<bool>) -> Response {
    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "outbox.not_found"),
        Err(e) => {
            tracing::error!(code = "outbox.storage_failed", error = %e, "dead letter not changed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed")
        }
    }
}

/// Puts a dead letter back in line with a fresh set of attempts.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/outbox/{event_id}/retry",
    tag = "admin",
    params(("event_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`outbox.not_found`: no such dead letter", body = ErrorBody),
        (status = 500, description = "`outbox.storage_failed`", body = ErrorBody),
    )
)]
pub(super) async fn retry_dead_letter(
    State(s): State<Arc<AdminState>>,
    Path(event_id): Path<String>,
) -> Response {
    dead_letter_result(s.outbox.retry(modules::CHECKIN, &event_id).await)
}

/// Gives up on a dead letter: that redemption never checks in.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/outbox/{event_id}/skip",
    tag = "admin",
    params(("event_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`outbox.not_found`: no such dead letter", body = ErrorBody),
        (status = 500, description = "`outbox.storage_failed`", body = ErrorBody),
    )
)]
pub(super) async fn skip_dead_letter(
    State(s): State<Arc<AdminState>>,
    Path(event_id): Path<String>,
) -> Response {
    dead_letter_result(s.outbox.skip(modules::CHECKIN, &event_id).await)
}
