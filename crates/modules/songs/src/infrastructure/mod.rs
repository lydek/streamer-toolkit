//! Admin API for the song list; the Host puts these routes behind the admin token.

pub mod db;
mod import;

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{
    application::{SongInput, SongsError, SongsService, skip_code},
    domain::{Category, ImportPlan, Song},
};
use db::SqliteSongs;
use import::Format;

/// An imported file's largest size.
const IMPORT_MAX_BYTES: usize = 5 * 1024 * 1024;

pub type Songs = SongsService<SqliteSongs>;

/// The song list on `pool` (migrated by the Host).
pub fn start(pool: SqlitePool) -> Arc<Songs> {
    Arc::new(SongsService::new(SqliteSongs::new(pool)))
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn songs_error(e: SongsError) -> Response {
    let status = match e {
        SongsError::Invalid => StatusCode::BAD_REQUEST,
        SongsError::Duplicate => StatusCode::CONFLICT,
        SongsError::NotFound => StatusCode::NOT_FOUND,
        SongsError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let code = match e {
        SongsError::Storage(_) => "songs.storage_failed".to_owned(),
        e => e.to_string(),
    };
    error(status, &code)
}

/// A song on the list. `title`, `artist` and `youtube_url` are the broadcaster's text, shown as text.
#[derive(Serialize, ToSchema)]
pub struct SongView {
    id: String,
    title: String,
    /// Empty: no artist.
    artist: String,
    /// `None`: uncategorised.
    #[schema(required)]
    category_id: Option<String>,
    #[schema(required)]
    youtube_url: Option<String>,
    /// RFC 3339; `None`: never sung.
    #[schema(required)]
    last_sung_at: Option<String>,
    request_count: u32,
}

impl From<Song> for SongView {
    fn from(s: Song) -> Self {
        Self {
            id: s.id,
            title: s.title,
            artist: s.artist,
            category_id: s.category_id,
            youtube_url: s.youtube_url,
            last_sung_at: s.last_sung_at.map(db::time),
            request_count: s.request_count,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CategoryView {
    id: String,
    name: String,
    requestable: bool,
    /// How many songs are in it.
    songs: u32,
}

#[derive(Serialize, ToSchema)]
pub struct SongLibrary {
    songs: Vec<SongView>,
    categories: Vec<CategoryView>,
}

fn category_view(c: Category, songs: &[Song]) -> CategoryView {
    let count = songs
        .iter()
        .filter(|s| s.category_id.as_deref() == Some(c.id.as_str()))
        .count();
    CategoryView {
        songs: u32::try_from(count).unwrap_or(u32::MAX),
        id: c.id,
        name: c.name,
        requestable: c.requestable,
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/song-list",
    tag = "admin",
    responses((status = 200, body = SongLibrary), (status = 500, body = ErrorBody))
)]
async fn song_library(State(s): State<Arc<Songs>>) -> Response {
    match s.list().await {
        Ok((songs, categories)) => Json(SongLibrary {
            categories: categories
                .into_iter()
                .map(|c| category_view(c, &songs))
                .collect(),
            songs: songs.into_iter().map(SongView::from).collect(),
        })
        .into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "songs.storage_failed"),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct SongBody {
    /// 1–100 characters.
    title: String,
    /// Up to 100 characters; empty for none.
    #[serde(default)]
    artist: String,
    category_id: Option<String>,
    /// Empty for none; otherwise a `youtube.com/watch?v=` or `youtu.be/` address.
    #[serde(default)]
    youtube_url: String,
}

impl From<SongBody> for SongInput {
    fn from(b: SongBody) -> Self {
        SongInput {
            title: b.title,
            artist: b.artist,
            category_id: b.category_id,
            youtube_url: b.youtube_url,
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/song-list/songs",
    tag = "admin",
    request_body = SongBody,
    responses(
        (status = 200, body = SongView),
        (status = 400, description = "`songs.invalid`: an empty or over-long title or artist, a link that is not a YouTube address, an unknown category, or 5,000 songs already", body = ErrorBody),
        (status = 409, description = "`songs.duplicate`: the same song (title and artist) is on the list", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn create_song(State(s): State<Arc<Songs>>, Json(body): Json<SongBody>) -> Response {
    match s.save_song(None, body.into()).await {
        Ok(song) => Json(SongView::from(song)).into_response(),
        Err(e) => songs_error(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/admin/song-list/songs/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = SongBody,
    responses(
        (status = 200, body = SongView),
        (status = 400, description = "`songs.invalid`", body = ErrorBody),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 409, description = "`songs.duplicate`", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn update_song(
    State(s): State<Arc<Songs>>,
    Path(id): Path<String>,
    Json(body): Json<SongBody>,
) -> Response {
    match s.save_song(Some(&id), body.into()).await {
        Ok(song) => Json(SongView::from(song)).into_response(),
        Err(e) => songs_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/admin/song-list/songs/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, description = "`songs.not_found`", body = ErrorBody), (status = 500, body = ErrorBody))
)]
async fn delete_song(State(s): State<Arc<Songs>>, Path(id): Path<String>) -> Response {
    match s.delete_song(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => songs_error(e),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CategoryBody {
    /// 1–30 characters, unique ignoring case.
    name: String,
    requestable: bool,
}

#[utoipa::path(
    post,
    path = "/api/admin/song-list/categories",
    tag = "admin",
    request_body = CategoryBody,
    responses(
        (status = 200, body = CategoryView),
        (status = 400, description = "`songs.invalid`: an empty or over-long name, or 100 categories already", body = ErrorBody),
        (status = 409, description = "`songs.duplicate`: another category has the name", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn create_song_category(
    State(s): State<Arc<Songs>>,
    Json(body): Json<CategoryBody>,
) -> Response {
    match s.save_category(None, &body.name, body.requestable).await {
        Ok(c) => Json(category_view(c, &[])).into_response(),
        Err(e) => songs_error(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/admin/song-list/categories/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = CategoryBody,
    responses(
        (status = 200, body = CategoryView),
        (status = 400, description = "`songs.invalid`", body = ErrorBody),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 409, description = "`songs.duplicate`", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn update_song_category(
    State(s): State<Arc<Songs>>,
    Path(id): Path<String>,
    Json(body): Json<CategoryBody>,
) -> Response {
    match s
        .save_category(Some(&id), &body.name, body.requestable)
        .await
    {
        Ok(c) => {
            let songs = s.list().await.map(|(songs, _)| songs).unwrap_or_default();
            Json(category_view(c, &songs)).into_response()
        }
        Err(e) => songs_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/admin/song-list/categories/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204, description = "Deleted; its songs are uncategorised now"),
        (status = 404, description = "`songs.not_found`", body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
async fn delete_song_category(State(s): State<Arc<Songs>>, Path(id): Path<String>) -> Response {
    match s.delete_category(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => songs_error(e),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// `xlsx` or `csv`.
    format: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportSkipped {
    /// The sheet's name; `None` for CSV.
    #[schema(required)]
    sheet: Option<String>,
    /// 1-based, as a spreadsheet shows it.
    row: u32,
    /// `no_title_column`, `no_title`, `too_long`, `bad_link`, `duplicate_in_file`, `too_many_songs` or
    /// `too_many_categories`.
    reason: String,
}

/// What an import does (preview) or did.
#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    added: u32,
    updated: u32,
    /// Rows naming a song already there, with nothing to change.
    unchanged: u32,
    new_categories: Vec<String>,
    skipped: Vec<ImportSkipped>,
}

impl From<ImportPlan> for ImportResult {
    fn from(p: ImportPlan) -> Self {
        let n = |len: usize| u32::try_from(len).unwrap_or(u32::MAX);
        Self {
            added: n(p.add.len()),
            updated: n(p.update.len()),
            unchanged: n(p.unchanged),
            new_categories: p.new_categories,
            skipped: p
                .skipped
                .into_iter()
                .map(|s| ImportSkipped {
                    sheet: s.sheet,
                    row: n(s.row),
                    reason: skip_code(s.reason).into(),
                })
                .collect(),
        }
    }
}

/// The file's sheets; `None` for an unknown format or a file that is not one.
fn sheets(query: &ImportQuery, body: &[u8]) -> Option<Vec<crate::domain::Sheet>> {
    let format = match query.format.as_str() {
        "xlsx" => Format::Xlsx,
        "csv" => Format::Csv,
        _ => return None,
    };
    import::read(body, format)
}

#[utoipa::path(
    post,
    path = "/api/admin/song-list/import/preview",
    tag = "admin",
    params(ImportQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The file, up to 5 MB"),
    responses(
        (status = 200, description = "What importing it would do; nothing is written", body = ImportResult),
        (status = 400, description = "`songs.unreadable_file`: not a readable `.xlsx` or UTF-8 CSV", body = ErrorBody),
        (status = 413, description = "Over 5 MB"),
        (status = 500, body = ErrorBody),
    )
)]
async fn preview_song_import(
    State(s): State<Arc<Songs>>,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    let Some(sheets) = sheets(&q, &body) else {
        return error(StatusCode::BAD_REQUEST, "songs.unreadable_file");
    };
    match s.preview_import(&sheets).await {
        Ok(plan) => Json(ImportResult::from(plan)).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "songs.storage_failed"),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/song-list/import",
    tag = "admin",
    params(ImportQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The file, up to 5 MB"),
    responses(
        (status = 200, description = "What it did, in one transaction", body = ImportResult),
        (status = 400, description = "`songs.unreadable_file`", body = ErrorBody),
        (status = 413, description = "Over 5 MB"),
        (status = 500, body = ErrorBody),
    )
)]
async fn apply_song_import(
    State(s): State<Arc<Songs>>,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    let Some(sheets) = sheets(&q, &body) else {
        return error(StatusCode::BAD_REQUEST, "songs.unreadable_file");
    };
    match s.import(&sheets).await {
        Ok(plan) => Json(ImportResult::from(plan)).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "songs.storage_failed"),
    }
}

pub fn routes(service: Arc<Songs>) -> Router {
    let imports = Router::new()
        .route(
            "/api/admin/song-list/import/preview",
            post(preview_song_import),
        )
        .route("/api/admin/song-list/import", post(apply_song_import))
        .layer(DefaultBodyLimit::max(IMPORT_MAX_BYTES));
    Router::new()
        .route("/api/admin/song-list", get(song_library))
        .route("/api/admin/song-list/songs", post(create_song))
        .route(
            "/api/admin/song-list/songs/{id}",
            put(update_song).delete(delete_song),
        )
        .route(
            "/api/admin/song-list/categories",
            post(create_song_category),
        )
        .route(
            "/api/admin/song-list/categories/{id}",
            put(update_song_category).delete(delete_song_category),
        )
        .merge(imports)
        .with_state(service)
}

#[derive(OpenApi)]
#[openapi(paths(
    song_library,
    create_song,
    update_song,
    delete_song,
    create_song_category,
    update_song_category,
    delete_song_category,
    preview_song_import,
    apply_song_import
))]
struct SongsApi;

pub fn openapi() -> utoipa::openapi::OpenApi {
    SongsApi::openapi()
}

#[cfg(test)]
mod tests;
