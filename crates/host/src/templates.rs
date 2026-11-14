//! User chat templates in `<data>/templates/chat/`: served read-only to overlays and watched for changes
//! (docs/architecture.md §本機 API 安全 → 使用者樣板).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
};
use utoipa::ToSchema;

use crate::OVERLAY_CSP;

/// The shipped default; also what overlays get while the user template is missing or broken.
#[derive(Embed)]
#[folder = "../../apps/web/src/overlay/chat/default-template"]
struct DefaultChat;

const ENTRY: &str = "template.html";
const MAX_ENTRY_BYTES: u64 = 1024 * 1024;
const POLL: Duration = Duration::from_secs(1);
const CHANGED_EVENT: &str = "template.changed";
const CHANGED_VERSION: u32 = 1;

/// A `template.changed` event exactly as `/ws/overlay` sends it; not kept in snapshots.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TemplateChangedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: TemplateChangedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: TemplateChangedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum TemplateChangedType {
    #[serde(rename = "template.changed")]
    TemplateChanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct TemplateChangedPayload {
    /// Which overlay's template changed; only `chat` for now.
    overlay: String,
}

/// `GET /api/admin/templates/chat`.
#[derive(Serialize, ToSchema)]
pub(crate) struct TemplateStatus {
    /// `template.missing`, `template.unreadable` or `template.invalid` while overlays get the default
    /// template instead of the user's; `null` when the user template is in use.
    #[schema(required)]
    error: Option<String>,
}

pub(crate) struct Templates {
    dir: PathBuf,
    /// Why the user template is not in use (an error code), or `None` when it is.
    error: RwLock<Option<&'static str>>,
}

impl Templates {
    /// Copies the default template on first run, checks the user template and starts watching it.
    pub(crate) fn start(data_dir: &Path, bus: Arc<EventBus>) -> Arc<Self> {
        let dir = data_dir.join("templates").join("chat");
        if !dir.exists() {
            // A failed copy shows up as `template.missing`; overlays still get the default.
            let _ = write_defaults(&dir);
        }
        // Fingerprint before the check, so an edit in between is caught by the first poll.
        let seen = fingerprint(&dir);
        let templates = Arc::new(Self {
            error: RwLock::new(check(&dir).err()),
            dir,
        });
        tokio::spawn(watch(templates.clone(), bus, seen));
        templates
    }

    pub(crate) fn error(&self) -> Option<&'static str> {
        *self.error.read().unwrap()
    }

    fn file(&self, path: &str) -> Response {
        let Some(content_type) = allowed_type(path) else {
            // Scripts and anything else outside the allowlist are never served.
            return StatusCode::NOT_FOUND.into_response();
        };
        if !path.split('/').all(plain_segment) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let data = match self.error() {
            None => read_inside(&self.dir, path),
            Some(_) => DefaultChat::get(path).map(|f| f.data.into_owned()),
        };
        match data {
            Some(data) => ([(header::CONTENT_TYPE, content_type)], data).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

/// Admin routes; the caller adds the token check.
pub(crate) fn admin_routes(t: Arc<Templates>) -> Router {
    Router::new()
        .route("/api/admin/templates/chat", get(chat_template))
        .with_state(t)
}

#[utoipa::path(
    get,
    path = "/api/admin/templates/chat",
    tag = "admin",
    responses((status = 200, body = TemplateStatus))
)]
pub(crate) async fn chat_template(State(t): State<Arc<Templates>>) -> Json<TemplateStatus> {
    Json(TemplateStatus {
        error: t.error().map(Into::into),
    })
}

/// `GET /templates/chat/{*path}`. Every answer, errors included, carries the Overlay CSP, `nosniff` and
/// `Access-Control-Allow-Origin: *` (the sandboxed preview iframe has a `null` origin).
pub(crate) async fn serve(
    State(t): State<Arc<Templates>>,
    UrlPath(path): UrlPath<String>,
) -> Response {
    let mut res = t.file(&path);
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(OVERLAY_CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// The non-script allowlist: HTML, CSS, images, fonts, audio.
fn allowed_type(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => return None,
    })
}

fn plain_segment(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['\\', ':', '\0'])
}

/// Reads `rel` only if it resolves to a file inside `base`; `canonicalize` resolves `..`, symlinks and
/// junctions, and Windows device names resolve outside `base`.
fn read_inside(base: &Path, rel: &str) -> Option<Vec<u8>> {
    let base = base.canonicalize().ok()?;
    let path = base.join(rel).canonicalize().ok()?;
    if !path.starts_with(&base) || !path.is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

fn write_defaults(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for name in DefaultChat::iter() {
        let file = DefaultChat::get(&name).expect("listed by iter");
        std::fs::write(dir.join(&*name), file.data)?;
    }
    Ok(())
}

/// The same markers the overlay binds to (`apps/web/src/overlay/chat/template.ts`).
fn check(dir: &Path) -> Result<(), &'static str> {
    let path = dir.join(ENTRY);
    let meta = std::fs::metadata(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "template.missing",
        _ => "template.unreadable",
    })?;
    if meta.len() > MAX_ENTRY_BYTES {
        return Err("template.invalid");
    }
    let html = std::fs::read_to_string(&path).map_err(|_| "template.unreadable")?;
    if html.contains("data-chat-list") && html.contains("data-chat-message") {
        Ok(())
    } else {
        Err("template.invalid")
    }
}

type FileStamp = (PathBuf, u64, Option<SystemTime>);

/// Paths, sizes and modification times of every file under `dir`; any difference means "re-apply".
fn fingerprint(dir: &Path) -> Vec<FileStamp> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            // DirEntry metadata does not follow symlinks, so a link loop cannot trap the walk.
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                dirs.push(entry.path());
            } else {
                files.push((entry.path(), meta.len(), meta.modified().ok()));
            }
        }
    }
    files.sort();
    files
}

/// Polls instead of using OS file events: no extra dependency, and a 1 s scan of a small folder is cheap.
async fn watch(t: Arc<Templates>, bus: Arc<EventBus>, mut last: Vec<FileStamp>) {
    loop {
        tokio::time::sleep(POLL).await;
        let dir = t.dir.clone();
        let Ok(now) = tokio::task::spawn_blocking(move || fingerprint(&dir)).await else {
            return;
        };
        if now != last {
            *t.error.write().unwrap() = check(&t.dir).err();
            publish_changed(&bus);
        }
        last = now;
    }
}

fn publish_changed(bus: &EventBus) {
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type: CHANGED_EVENT,
        version: CHANGED_VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: Source::System,
        payload: serde_json::to_value(TemplateChangedPayload {
            overlay: "chat".into(),
        })
        .expect("payload serializes"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn changed_event_type_matches_the_bus() {
        let bus = EventBus::default();
        let (_, mut events) = bus.subscribe();
        publish_changed(&bus);
        let wire: serde_json::Value = serde_json::from_str(&events.recv().await.unwrap()).unwrap();
        // No missing, extra or renamed fields.
        let typed: TemplateChangedEvent = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), wire);
        let doc = serde_json::to_value(crate::openapi()).unwrap();
        for (schema, value) in [
            ("TemplateChangedEvent", &wire),
            ("TemplateChangedPayload", &wire["payload"]),
        ] {
            for key in doc["components"]["schemas"][schema]["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{schema} has a required list"))
            {
                let key = key.as_str().unwrap();
                assert!(
                    value.get(key).is_some(),
                    "{schema}.{key} is required but not sent"
                );
            }
        }
    }

    #[test]
    fn segments_reject_escapes() {
        for bad in ["", ".", "..", "a\\b", "c:", "x\0"] {
            assert!(!plain_segment(bad), "{bad:?}");
        }
        assert!(plain_segment("style.css"));
    }

    #[test]
    fn only_non_script_types_are_allowed() {
        for blocked in [
            "a.js",
            "a.mjs",
            "a.JS",
            "a.wasm",
            "a.json",
            "a.map",
            "noext",
            "dir.css/x",
        ] {
            assert_eq!(allowed_type(blocked), None, "{blocked}");
        }
        assert_eq!(allowed_type("A.CSS"), Some("text/css; charset=utf-8"));
        assert_eq!(allowed_type("img/logo.svg"), Some("image/svg+xml"));
    }

    #[test]
    fn check_reports_missing_and_invalid_templates() {
        let dir = std::env::temp_dir().join(format!("st-tpl-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(check(&dir), Err("template.missing"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(ENTRY), "<p>no markers</p>").unwrap();
        assert_eq!(check(&dir), Err("template.invalid"));
        std::fs::write(dir.join(ENTRY), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(check(&dir), Err("template.unreadable"));
        write_defaults(&dir).unwrap();
        assert_eq!(check(&dir), Ok(()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
