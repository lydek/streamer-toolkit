//! Adapters: the admin API (behind the admin token, added by the host).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

use crate::application::{ClipError, ClipsService, Entry};

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn clip_error(e: ClipError) -> Response {
    let status = match &e {
        ClipError::InvalidInput => StatusCode::BAD_REQUEST,
        ClipError::NotFound => StatusCode::NOT_FOUND,
        ClipError::QueueFull => StatusCode::CONFLICT,
        ClipError::Twitch { status, .. } => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
    };
    error(status, &e.to_string())
}

/// A queued clip. `title` and `broadcasterName` are Twitch text: render them as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    /// Tells apart the same clip queued twice.
    pub entry: String,
    /// The Twitch clip ID.
    pub id: String,
    pub title: String,
    pub broadcaster_name: String,
    pub duration_ms: u64,
}

impl From<Entry> for QueueEntry {
    fn from(e: Entry) -> Self {
        Self {
            entry: e.entry,
            id: e.clip.id,
            title: e.clip.title,
            broadcaster_name: e.clip.broadcaster_name,
            duration_ms: e.clip.duration_ms,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ClipQueue {
    /// In play order; at most 50.
    pub queue: Vec<QueueEntry>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ClipInput {
    /// A Twitch clip URL (`clips.twitch.tv/<id>`, `clips.twitch.tv/embed?clip=<id>` or `twitch.tv/<channel>/clip/<id>`)
    /// or a clip ID.
    pub input: String,
}

/// The admin API.
pub fn routes(service: Arc<ClipsService>) -> Router {
    Router::new()
        .route(
            "/api/admin/clips",
            get(list_clips).post(add_clip).delete(clear_clips),
        )
        .route("/api/admin/clips/{entry}", delete(remove_clip))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/clips",
    tag = "admin",
    responses((status = 200, body = ClipQueue))
)]
pub(crate) async fn list_clips(State(s): State<Arc<ClipsService>>) -> Json<ClipQueue> {
    Json(ClipQueue {
        queue: s.list().into_iter().map(Into::into).collect(),
    })
}

/// Looks the clip up on Twitch (Helix Get Clips) and queues it at the end.
#[utoipa::path(
    post,
    path = "/api/admin/clips",
    tag = "admin",
    request_body = ClipInput,
    responses(
        (status = 200, body = QueueEntry),
        (status = 400, description = "`clip.invalid_input`: not a Twitch clip URL or ID; other `twitch.*` sign-in codes such as `twitch.sign_in_expired` or `twitch.invalid_response`", body = ErrorBody),
        (status = 404, description = "`clip.not_found`: Twitch has no such clip", body = ErrorBody),
        (status = 409, description = "`clip.queue_full` (50 clips), `twitch.client_id_required` or `twitch.sign_in_required`", body = ErrorBody),
        (status = 500, description = "`twitch.storage_failed`", body = ErrorBody),
        (status = 502, description = "`twitch.request_refused`, `twitch.rate_limited` or `twitch.unreachable`", body = ErrorBody),
    )
)]
pub(crate) async fn add_clip(
    State(s): State<Arc<ClipsService>>,
    body: Result<Json<ClipInput>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return clip_error(ClipError::InvalidInput);
    };
    match s.add(&b.input).await {
        Ok(entry) => Json(QueueEntry::from(entry)).into_response(),
        Err(e) => clip_error(e),
    }
}

/// Removes one queued clip; an entry that is not queued is ignored.
#[utoipa::path(
    delete,
    path = "/api/admin/clips/{entry}",
    tag = "admin",
    params(("entry" = String, Path)),
    responses((status = 204))
)]
pub(crate) async fn remove_clip(
    State(s): State<Arc<ClipsService>>,
    Path(entry): Path<String>,
) -> StatusCode {
    s.remove(&entry);
    StatusCode::NO_CONTENT
}

/// Empties the queue.
#[utoipa::path(
    delete,
    path = "/api/admin/clips",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn clear_clips(State(s): State<Arc<ClipsService>>) -> StatusCode {
    s.clear();
    StatusCode::NO_CONTENT
}

#[derive(OpenApi)]
#[openapi(paths(list_clips, add_clip, remove_clip, clear_clips))]
struct ClipsApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ClipsApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        LookupError,
        tests::{FakeLookup, KNOWN},
    };
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn app(failure: Option<LookupError>) -> Router {
        routes(Arc::new(ClipsService::new(FakeLookup::new(failure))))
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    fn add(body: &str) -> Request<Body> {
        Request::post("/api/admin/clips")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    #[tokio::test]
    async fn add_list_remove_and_clear() {
        let app = app(None);
        let url = format!(r#"{{"input":"https://www.twitch.tv/twitch/clip/{KNOWN}"}}"#);
        let (status, added) = send(&app, add(&url)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(added["id"], KNOWN);
        assert_eq!(added["broadcasterName"], "Twitch");
        assert_eq!(added["durationMs"], 30_000);
        send(&app, add(&url)).await;

        let list = || {
            Request::get("/api/admin/clips")
                .body(Body::empty())
                .unwrap()
        };
        let (_, queue) = send(&app, list()).await;
        assert_eq!(queue["queue"].as_array().unwrap().len(), 2);
        assert_eq!(queue["queue"][0], added);

        let entry = added["entry"].as_str().unwrap();
        for uri in [
            format!("/api/admin/clips/{entry}"),
            "/api/admin/clips/unknown".into(),
        ] {
            let req = Request::delete(uri).body(Body::empty()).unwrap();
            assert_eq!(send(&app, req).await.0, StatusCode::NO_CONTENT);
        }
        assert_eq!(
            send(&app, list()).await.1["queue"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let clear = Request::delete("/api/admin/clips")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, clear).await.0, StatusCode::NO_CONTENT);
        assert_eq!(send(&app, list()).await.1["queue"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn refusals_carry_their_codes() {
        let app = app(None);
        for (body, status, code) in [
            (
                r#"{"input":"https://example.com/clip"}"#,
                StatusCode::BAD_REQUEST,
                "clip.invalid_input",
            ),
            ("not json", StatusCode::BAD_REQUEST, "clip.invalid_input"),
            (
                r#"{"input":"GoneClip"}"#,
                StatusCode::NOT_FOUND,
                "clip.not_found",
            ),
        ] {
            assert_eq!(
                send(&app, add(body)).await,
                (status, serde_json::json!({ "code": code }))
            );
        }

        let signed_out = self::app(Some(LookupError::Twitch {
            status: 409,
            code: "twitch.sign_in_required".into(),
        }));
        let (status, body) = send(&signed_out, add(&format!(r#"{{"input":"{KNOWN}"}}"#))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "twitch.sign_in_required");
    }
}
