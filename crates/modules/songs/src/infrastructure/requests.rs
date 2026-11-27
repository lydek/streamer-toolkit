//! Admin API for song requests (the 點歌 page); the Host puts these routes behind the admin token.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{commands::CommandTable, modules::Modules};
use utoipa::{OpenApi, ToSchema};

use super::{ErrorBody, db, db::SqliteSongs, error};
use crate::{
    application::requests::{Changed, RequestError, RequestsService, Say},
    domain::requests::Request,
};

pub type Requests = RequestsService<SqliteSongs>;

/// Song requests on `pool` (migrated by the Host): replies go out through `say`, and `changed` tells the page.
pub fn start_requests(
    pool: SqlitePool,
    modules: Arc<Modules>,
    commands: Arc<CommandTable>,
    data_dir: PathBuf,
    say: Say,
    changed: Changed,
) -> Arc<Requests> {
    Arc::new(RequestsService::new(
        SqliteSongs::new(pool),
        modules,
        commands,
        data_dir,
        say,
        changed,
    ))
}

fn request_error(e: RequestError) -> Response {
    let status = match e {
        RequestError::NotFound => StatusCode::NOT_FOUND,
        RequestError::Invalid => StatusCode::BAD_REQUEST,
        RequestError::AlreadyQueued | RequestError::QueueFull | RequestError::NotAllowed => {
            StatusCode::CONFLICT
        }
        RequestError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let code = match e {
        RequestError::Storage(_) => "songs.storage_failed".to_owned(),
        e => e.to_string(),
    };
    error(status, &code)
}

fn done(result: Result<(), RequestError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => request_error(e),
    }
}

/// A request. `title`, `artist` and `viewer` are text from the list or from chat, shown as text.
#[derive(Serialize, ToSchema)]
pub struct RequestView {
    id: String,
    /// `None` once the song was deleted from the list.
    #[schema(required)]
    song_id: Option<String>,
    title: String,
    artist: String,
    /// Who it is for (a display name); `None`: nobody.
    #[schema(required)]
    viewer: Option<String>,
    /// Requested by the streamer on the page.
    by_hand: bool,
    /// RFC 3339.
    requested_at: String,
    /// RFC 3339: when it was sung; `None` until then.
    #[schema(required)]
    finished_at: Option<String>,
}

impl From<Request> for RequestView {
    fn from(r: Request) -> Self {
        Self {
            id: r.id,
            song_id: r.song_id,
            title: r.title,
            artist: r.artist,
            viewer: r.viewer_name,
            by_hand: r.by_hand,
            requested_at: db::time(r.requested_at),
            finished_at: r.finished_at.map(db::time),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct RequestBoard {
    /// The song being sung.
    #[schema(required)]
    singing: Option<RequestView>,
    /// In order; the first one is next.
    queued: Vec<RequestView>,
    /// The latest first, at most 50.
    sung: Vec<RequestView>,
}

#[derive(Deserialize, ToSchema)]
pub struct ManualRequest {
    song_id: String,
    /// Who it is for (at most 25 characters); empty or absent: nobody.
    #[serde(default)]
    viewer: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct RequestOrder {
    /// Every queued request's ID once, in the new order.
    ids: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/song-requests",
    tag = "admin",
    responses((status = 200, body = RequestBoard), (status = 500, body = ErrorBody))
)]
async fn request_board(State(s): State<Arc<Requests>>) -> Response {
    match s.board().await {
        Ok(b) => Json(RequestBoard {
            singing: b.singing.map(RequestView::from),
            queued: b.queued.into_iter().map(RequestView::from).collect(),
            sung: b.sung.into_iter().map(RequestView::from).collect(),
        })
        .into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "songs.storage_failed"),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/song-requests",
    tag = "admin",
    request_body = ManualRequest,
    responses(
        (status = 204, description = "Queued; nothing is posted in chat"),
        (status = 400, description = "`songs.invalid`: the name is too long", body = ErrorBody),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 409, description = "`songs.already_queued` or `songs.queue_full`", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn request_song(State(s): State<Arc<Requests>>, Json(body): Json<ManualRequest>) -> Response {
    done(s.request_by_hand(&body.song_id, body.viewer).await)
}

#[utoipa::path(
    post,
    path = "/api/admin/song-requests/{id}/start",
    tag = "admin",
    params(("id" = String, Path, description = "A queued request")),
    responses(
        (status = 204, description = "Being sung"),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 409, description = "`songs.not_allowed`: another song is being sung", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn start_request(State(s): State<Arc<Requests>>, Path(id): Path<String>) -> Response {
    done(s.start(&id).await)
}

#[utoipa::path(
    post,
    path = "/api/admin/song-requests/finish",
    tag = "admin",
    responses(
        (status = 204, description = "Sung; the next queued one is being sung"),
        (status = 409, description = "`songs.not_allowed`: nothing is being sung", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn finish_request(State(s): State<Arc<Requests>>) -> Response {
    done(s.finish().await)
}

#[utoipa::path(
    delete,
    path = "/api/admin/song-requests/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "A queued or singing request")),
    responses(
        (status = 204, description = "Removed"),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn remove_request(State(s): State<Arc<Requests>>, Path(id): Path<String>) -> Response {
    done(s.remove(&id).await)
}

#[utoipa::path(
    put,
    path = "/api/admin/song-requests/order",
    tag = "admin",
    request_body = RequestOrder,
    responses(
        (status = 204, description = "Reordered"),
        (status = 400, description = "`songs.invalid`: not every queued request once", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn reorder_requests(
    State(s): State<Arc<Requests>>,
    Json(body): Json<RequestOrder>,
) -> Response {
    done(s.reorder(&body.ids).await)
}

pub fn request_routes(service: Arc<Requests>) -> Router {
    Router::new()
        .route(
            "/api/admin/song-requests",
            get(request_board).post(request_song),
        )
        .route("/api/admin/song-requests/finish", post(finish_request))
        .route("/api/admin/song-requests/order", put(reorder_requests))
        .route("/api/admin/song-requests/{id}/start", post(start_request))
        .route("/api/admin/song-requests/{id}", delete(remove_request))
        .with_state(service)
}

#[derive(OpenApi)]
#[openapi(paths(
    request_board,
    request_song,
    start_request,
    finish_request,
    remove_request,
    reorder_requests
))]
pub(super) struct RequestsApi;
