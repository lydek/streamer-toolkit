//! Adapters: the admin API (behind the admin token, added by the host) and the read-only sound file route.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{BytesRejection, JsonRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use toolkit_core::settings::SoundEntry;
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{
    application::{SoundError, SoundService},
    domain::MAX_BYTES,
};

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn sound_error(e: SoundError) -> Response {
    let status = match &e {
        SoundError::UnsupportedType | SoundError::Invalid => StatusCode::BAD_REQUEST,
        SoundError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        SoundError::NotFound => StatusCode::NOT_FOUND,
        SoundError::SaveFailed(io) => {
            tracing::error!(code = "sound.save_failed", error = %io, "sound library not saved");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    error(status, &e.to_string())
}

/// One imported sound. `label` is broadcaster text: render it as text. Its file is `GET /sounds/{id}`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Sound {
    pub id: String,
    pub label: String,
    /// 0–100 (%).
    pub volume: u8,
}

impl From<SoundEntry> for Sound {
    fn from(e: SoundEntry) -> Self {
        Self {
            id: e.id,
            label: e.label,
            volume: e.volume,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SoundLibrary {
    pub sounds: Vec<Sound>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// The original file name: its extension must match the content; the rest becomes the label.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SoundUpdate {
    /// 1–100 characters after trimming.
    pub label: String,
    /// 0–100 (%).
    pub volume: u8,
}

/// The admin API.
pub fn routes(service: Arc<SoundService>) -> Router {
    Router::new()
        .route(
            "/api/admin/sounds",
            get(list_sounds)
                .post(import_sound)
                .layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route(
            "/api/admin/sounds/{id}",
            put(update_sound).delete(delete_sound),
        )
        .with_state(service)
}

/// The read-only sound file route for overlays and the admin UI's preview; it needs no token.
pub fn file_routes(service: Arc<SoundService>) -> Router {
    Router::new()
        .route("/sounds/{id}", get(sound_file))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/sounds",
    tag = "admin",
    responses((status = 200, body = SoundLibrary))
)]
pub(crate) async fn list_sounds(State(s): State<Arc<SoundService>>) -> Json<SoundLibrary> {
    Json(SoundLibrary {
        sounds: s.list().into_iter().map(Sound::from).collect(),
    })
}

/// Copies an MP3, WAV or OGG file (at most 10 MB) into the library at 100% volume. The body is the file itself.
#[utoipa::path(
    post,
    path = "/api/admin/sounds",
    tag = "admin",
    params(ImportQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The sound file"),
    responses(
        (status = 200, body = Sound),
        (status = 400, description = "`sound.unsupported_type`: not an MP3, WAV or OGG file (by extension and content)", body = ErrorBody),
        (status = 413, description = "`sound.too_large`: larger than 10 MB", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn import_sound(
    State(s): State<Arc<SoundService>>,
    query: Result<Query<ImportQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "sound.unsupported_type");
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return sound_error(SoundError::TooLarge);
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "sound.unsupported_type"),
    };
    match s.import(&q.name, &bytes) {
        Ok(entry) => Json(Sound::from(entry)).into_response(),
        Err(e) => sound_error(e),
    }
}

/// Renames a sound or changes its volume.
#[utoipa::path(
    put,
    path = "/api/admin/sounds/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = SoundUpdate,
    responses(
        (status = 200, body = Sound),
        (status = 400, description = "`sound.invalid`: an empty label or a volume over 100", body = ErrorBody),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn update_sound(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
    body: Result<Json<SoundUpdate>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "sound.invalid");
    };
    match s.update(&id, &b.label, b.volume) {
        Ok(entry) => Json(Sound::from(entry)).into_response(),
        Err(e) => sound_error(e),
    }
}

/// Deletes a sound and its file.
#[utoipa::path(
    delete,
    path = "/api/admin/sounds/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_sound(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => sound_error(e),
    }
}

/// A library sound's file with the Content-Type of its format, never sniffed.
async fn sound_file(State(s): State<Arc<SoundService>>, Path(id): Path<String>) -> Response {
    let Some((path, format)) = s.file(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut res = Response::new(Body::from(bytes));
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[derive(OpenApi)]
#[openapi(paths(list_sounds, import_sound, update_sound, delete_sound))]
struct SoundApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    SoundApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use std::path::PathBuf;
    use tower::ServiceExt;

    pub(crate) const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";

    fn data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-sound-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn app(dir: &std::path::Path) -> Router {
        let service = Arc::new(SoundService::new(dir.to_path_buf()));
        routes(service.clone()).merge(file_routes(service))
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Response) {
        let res = app.clone().oneshot(req).await.unwrap();
        (res.status(), res)
    }

    async fn json(res: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn upload(name: &str, body: impl Into<Body>) -> Request<Body> {
        Request::post(format!("/api/admin/sounds?name={name}"))
            .body(body.into())
            .unwrap()
    }

    #[tokio::test]
    async fn import_copies_the_file_under_a_new_name() {
        let dir = data_dir("import");
        let app = app(&dir);
        let (status, res) = send(&app, upload("..%2F..%2Fbell.WAV", WAV)).await;
        assert_eq!(status, StatusCode::OK);
        let sound = json(res).await;
        assert_eq!(sound["label"], "bell");
        assert_eq!(sound["volume"], 100);
        let id = sound["id"].as_str().unwrap();
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
        let files: Vec<_> = std::fs::read_dir(dir.join("sounds"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            files,
            [format!("{id}.wav")],
            "named by the ID, never the file name"
        );
        assert!(!dir.join("bell.WAV").exists() && !dir.parent().unwrap().join("bell.WAV").exists());

        let (status, res) = send(
            &app,
            Request::get(format!("/sounds/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let h = res.headers();
        assert_eq!(h[header::CONTENT_TYPE], "audio/wav");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], WAV);

        let (_, res) = send(
            &app,
            Request::get("/api/admin/sounds")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(json(res).await["sounds"][0]["id"], id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn only_mp3_wav_and_ogg_up_to_10_mb_are_taken() {
        let dir = data_dir("refuse");
        let app = app(&dir);
        for (name, body) in [
            ("x.mp3", WAV.to_vec()),
            ("x.html", b"<script>".to_vec()),
            ("x.wav", Vec::new()),
        ] {
            let (status, res) = send(&app, upload(name, body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
            assert_eq!(json(res).await["code"], "sound.unsupported_type");
        }
        let mut big = WAV.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        let (status, res) = send(&app, upload("x.wav", big.clone())).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(json(res).await["code"], "sound.too_large");
        big.truncate(MAX_BYTES);
        assert_eq!(
            send(&app, upload("x.wav", big)).await.0,
            StatusCode::OK,
            "exactly 10 MB"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rename_change_volume_and_delete() {
        let dir = data_dir("edit");
        let app = app(&dir);
        let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let put = |body: serde_json::Value| {
            Request::put(format!("/api/admin/sounds/{id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let (status, res) = send(
            &app,
            put(serde_json::json!({ "label": " Ding ", "volume": 40 })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json(res).await,
            serde_json::json!({ "id": id, "label": "Ding", "volume": 40 })
        );
        for bad in [
            serde_json::json!({ "label": " ", "volume": 40 }),
            serde_json::json!({ "label": "a", "volume": 101 }),
        ] {
            assert_eq!(send(&app, put(bad)).await.0, StatusCode::BAD_REQUEST);
        }

        let delete = || {
            Request::delete(format!("/api/admin/sounds/{id}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(send(&app, delete()).await.0, StatusCode::NO_CONTENT);
        assert_eq!(send(&app, delete()).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            std::fs::read_dir(dir.join("sounds")).unwrap().count(),
            0,
            "file removed"
        );
        let file = Request::get(format!("/sounds/{id}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, file).await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_file_route_serves_library_sounds_only() {
        let dir = data_dir("route");
        let app = app(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{}").unwrap();
        for path in [
            "/sounds/settings.json",
            "/sounds/..%2Fsettings.json",
            "/sounds/x",
        ] {
            let req = Request::get(path).body(Body::empty()).unwrap();
            assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND, "{path}");
        }
        // An edited settings.json cannot point the route outside `sounds/`.
        toolkit_core::settings::update(&dir, |s| {
            s.sound.library.push(SoundEntry {
                id: "../settings".into(),
                label: "x".into(),
                volume: 100,
                format: "json".into(),
            })
        })
        .unwrap();
        let req = Request::get("/sounds/..%2Fsettings")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }
}
