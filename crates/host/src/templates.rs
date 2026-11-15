//! Chat overlay templates: the built-in ones shipped with the app, or the user's own in `<data>/templates/chat/`
//! (selected as `custom`), served read-only to overlays; the user folder is watched for changes
//! (docs/architecture.md §本機 API 安全 → 使用者樣板).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};

use axum::{
    Json, Router,
    extract::{Path as UrlPath, Request, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    settings,
};
use utoipa::ToSchema;

use crate::{
    OVERLAY_CSP,
    admin::{ErrorBody, error},
};

/// The built-in templates, one folder per [`ChatTemplate`] ID. `default` is also what overlays get while the
/// user template is missing or broken.
#[derive(Embed)]
#[folder = "../../apps/web/src/overlay/chat/templates"]
struct BuiltIn;

/// Which template the chat overlay shows. `custom` (the data folder's `templates/chat/`) has no UI entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChatTemplate {
    Default,
    Compact,
    Bubble,
    Custom,
}

impl ChatTemplate {
    const ALL: [Self; 4] = [Self::Default, Self::Compact, Self::Bubble, Self::Custom];

    fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Compact => "compact",
            Self::Bubble => "bubble",
            Self::Custom => "custom",
        }
    }

    /// A hand-edited `settings.json` may hold anything; unknown values mean the default.
    fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|t| t.id() == id)
            .unwrap_or(Self::Default)
    }
}

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
    selected: ChatTemplate,
    /// With `custom` selected: `template.missing`, `template.unreadable` or `template.invalid` while overlays
    /// get the default template instead of the user's; otherwise `null`.
    #[schema(required)]
    error: Option<String>,
}

/// `PUT /api/admin/templates/chat`.
#[derive(Deserialize, ToSchema)]
pub(crate) struct TemplateChoice {
    selected: ChatTemplate,
}

pub(crate) struct Templates {
    data_dir: PathBuf,
    dir: PathBuf,
    bus: Arc<EventBus>,
    selected: RwLock<ChatTemplate>,
    switching: Mutex<()>,
    /// Why the user template is not usable (an error code), or `None` when it is.
    error: RwLock<Option<&'static str>>,
}

impl Templates {
    /// Reads the selection, checks the user template and starts watching it.
    pub(crate) fn start(data_dir: &Path, bus: Arc<EventBus>) -> Arc<Self> {
        let dir = data_dir.join("templates").join("chat");
        // Fingerprint before the check, so an edit in between is caught by the first poll.
        let seen = fingerprint(&dir);
        let templates = Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            selected: RwLock::new(ChatTemplate::from_id(
                &settings::load(data_dir).chat_template,
            )),
            switching: Mutex::new(()),
            error: RwLock::new(check(&dir).err()),
            dir,
            bus,
        });
        tokio::spawn(watch(templates.clone(), seen));
        templates
    }

    fn selected(&self) -> ChatTemplate {
        *self.selected.read().unwrap()
    }

    /// Why the selected template is not in use; only the user's can fail.
    pub(crate) fn error(&self) -> Option<&'static str> {
        match self.selected() {
            ChatTemplate::Custom => *self.error.read().unwrap(),
            _ => None,
        }
    }

    /// A file of the selected template.
    fn file(&self, path: &str) -> Response {
        // Read once, so a switch in the middle cannot mix two selections.
        let selected = self.selected();
        file_response(path, |path| match selected {
            ChatTemplate::Custom => match *self.error.read().unwrap() {
                None => read_inside(&self.dir, path),
                Some(_) => built_in(ChatTemplate::Default, path),
            },
            t => built_in(t, path),
        })
    }
}

fn built_in(t: ChatTemplate, path: &str) -> Option<Vec<u8>> {
    BuiltIn::get(&format!("{}/{path}", t.id())).map(|f| f.data.into_owned())
}

/// Checks `path` against the allowlist and its segments, then answers with what `read` finds.
fn file_response(path: &str, read: impl FnOnce(&str) -> Option<Vec<u8>>) -> Response {
    let Some(content_type) = allowed_type(path) else {
        // Scripts and anything else outside the allowlist are never served.
        return StatusCode::NOT_FOUND.into_response();
    };
    if !path.split('/').all(plain_segment) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match read(path) {
        Some(data) => ([(header::CONTENT_TYPE, content_type)], data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Admin routes; the caller adds the token check.
pub(crate) fn admin_routes(t: Arc<Templates>) -> Router {
    Router::new()
        .route(
            "/api/admin/templates/chat",
            get(chat_template).put(select_chat_template),
        )
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
        selected: t.selected(),
        error: t.error().map(Into::into),
    })
}

/// Switches the chat overlay template and saves the choice; overlays reload it at once (`template.changed`).
#[utoipa::path(
    put,
    path = "/api/admin/templates/chat",
    tag = "admin",
    request_body = TemplateChoice,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn select_chat_template(
    State(t): State<Arc<Templates>>,
    body: Result<Json<TemplateChoice>, JsonRejection>,
) -> Response {
    // Unknown IDs and malformed bodies get one error code instead of axum's plain-text rejection.
    let Ok(Json(TemplateChoice { selected })) = body else {
        return error(StatusCode::BAD_REQUEST, "template.unknown");
    };
    let saved = tokio::task::spawn_blocking(move || {
        // One switch at a time, so the file and memory never disagree; readers of `selected` never wait on
        // the save.
        let _turn = t.switching.lock().unwrap();
        settings::update(&t.data_dir, |s| s.chat_template = selected.id().into())?;
        *t.selected.write().unwrap() = selected;
        publish_changed(&t.bus);
        std::io::Result::Ok(())
    })
    .await;
    match saved {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed"),
    }
}

/// `GET /templates/chat/{*path}`, the selected template; the headers come from [`headers`].
pub(crate) async fn serve(
    State(t): State<Arc<Templates>>,
    UrlPath(path): UrlPath<String>,
) -> Response {
    // File reads block; keep them off the async workers.
    tokio::task::spawn_blocking(move || t.file(&path))
        .await
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `GET /templates/built-in/chat/{id}/{*path}`: any built-in template, for the admin UI's previews.
pub(crate) async fn serve_built_in(UrlPath((id, path)): UrlPath<(String, String)>) -> Response {
    match ChatTemplate::ALL.into_iter().find(|t| t.id() == id) {
        Some(t) if t != ChatTemplate::Custom => file_response(&path, |path| built_in(t, path)),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Every answer under `/templates`, including the ones axum or the guard produce before [`serve`] runs
/// (bad percent-encoding, wrong method, unknown path, rejected Host), carries the Overlay CSP, `nosniff`
/// and `Access-Control-Allow-Origin: *` (the sandboxed preview iframe has a `null` origin).
pub(crate) async fn headers(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let is_template = path == "/templates" || path.starts_with("/templates/");
    let mut res = next.run(req).await;
    if !is_template {
        return res;
    }
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

/// Checks for the markers the overlay binds to (`apps/web/src/overlay/chat/template.ts`).
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
    if has_markers(&html) {
        Ok(())
    } else {
        Err("template.invalid")
    }
}

/// Outside comments: a `data-chat-list` attribute and a `<template>` tag carrying `data-chat-message`.
/// shortcut: a text scan, not an HTML parser; the overlay still falls back if markup fools it.
fn has_markers(html: &str) -> bool {
    let mut code = String::new();
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        code.push_str(&rest[..start]);
        let body = &rest[start + 4..];
        rest = body.find("-->").map_or("", |end| &body[end + 3..]);
    }
    code.push_str(rest);
    let code = code.to_ascii_lowercase();
    code.contains("data-chat-list")
        && code.match_indices("<template").any(|(i, _)| {
            code[i..]
                .split('>')
                .next()
                .is_some_and(|tag| tag.contains("data-chat-message"))
        })
}

type FileStamp = (PathBuf, u64, Option<SystemTime>);

/// Paths, sizes and modification times of the servable files under `dir`; any difference means "re-apply".
/// Files the route never serves (editor swap files, `Thumbs.db`) are left out so they trigger no reload.
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
            } else if entry.file_name().to_str().and_then(allowed_type).is_some() {
                files.push((entry.path(), meta.len(), meta.modified().ok()));
            }
        }
    }
    files.sort();
    files
}

/// Polls instead of using OS file events: no extra dependency, and a 1 s scan of a small folder is cheap.
async fn watch(t: Arc<Templates>, mut last: Vec<FileStamp>) {
    loop {
        tokio::time::sleep(POLL).await;
        let dir = t.dir.clone();
        let Ok(now) = tokio::task::spawn_blocking(move || fingerprint(&dir)).await else {
            return;
        };
        if now != last {
            let dir = t.dir.clone();
            let error = tokio::task::spawn_blocking(move || check(&dir).err())
                .await
                .unwrap_or(Some("template.unreadable"));
            *t.error.write().unwrap() = error;
            // Overlays showing a built-in template have nothing to reload.
            if t.selected() == ChatTemplate::Custom {
                publish_changed(&t.bus);
            }
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
pub(crate) mod tests {
    use super::*;

    /// A user template folder holding a copy of the built-in default.
    pub(crate) fn write_default(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for name in ["template.html", "style.css"] {
            std::fs::write(
                dir.join(name),
                built_in(ChatTemplate::Default, name).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn every_built_in_template_is_usable_and_listed() {
        let mut folders: Vec<String> = BuiltIn::iter()
            .map(|f| f.split('/').next().unwrap().to_owned())
            .collect();
        folders.sort();
        folders.dedup();
        let listed: Vec<&str> = ChatTemplate::ALL
            .into_iter()
            .filter(|t| *t != ChatTemplate::Custom)
            .map(ChatTemplate::id)
            .collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(folders, sorted);
        for id in listed {
            let t = ChatTemplate::from_id(id);
            assert_eq!(t.id(), id);
            let html = String::from_utf8(built_in(t, "template.html").unwrap()).unwrap();
            assert!(has_markers(&html), "{id}");
            assert!(built_in(t, "style.css").is_some(), "{id}");
        }
        assert_eq!(ChatTemplate::from_id("nope"), ChatTemplate::Default);
        assert_eq!(ChatTemplate::from_id("custom"), ChatTemplate::Custom);
    }

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

    #[tokio::test]
    async fn switching_saves_and_tells_overlays() {
        let dir = std::env::temp_dir().join(format!("st-tpl-select-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Arc::new(EventBus::default());
        let t = Templates::start(&dir, bus.clone());
        let (_, mut events) = bus.subscribe();
        let choice = TemplateChoice {
            selected: ChatTemplate::Compact,
        };
        let res = select_chat_template(State(t.clone()), Ok(Json(choice))).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(t.selected(), ChatTemplate::Compact);
        assert_eq!(settings::load(&dir).chat_template, "compact");
        let wire: serde_json::Value = serde_json::from_str(&events.recv().await.unwrap()).unwrap();
        assert_eq!(wire["type"], CHANGED_EVENT);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_save_keeps_the_template_and_tells_nobody() {
        let dir = std::env::temp_dir().join(format!("st-tpl-save-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A folder where the temp settings file goes makes the save fail.
        std::fs::create_dir_all(dir.join("settings.json.tmp")).unwrap();
        let bus = Arc::new(EventBus::default());
        let t = Templates::start(&dir, bus.clone());
        let (_, mut events) = bus.subscribe();
        let choice = TemplateChoice {
            selected: ChatTemplate::Bubble,
        };
        let res = select_chat_template(State(t.clone()), Ok(Json(choice))).await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(t.selected(), ChatTemplate::Default);
        assert!(events.try_recv().is_err(), "no template.changed");
        let _ = std::fs::remove_dir_all(dir);
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
        write_default(&dir);
        assert_eq!(check(&dir), Ok(()));
        // The default's header comment names both markers; without the real elements it is still invalid.
        let default = std::fs::read_to_string(dir.join(ENTRY)).unwrap();
        let (start, end) = (
            default.find("<ul").unwrap(),
            default.find("</ul>").unwrap() + 5,
        );
        std::fs::write(
            dir.join(ENTRY),
            format!("{}{}", &default[..start], &default[end..]),
        )
        .unwrap();
        assert_eq!(check(&dir), Err("template.invalid"));
        let misspelled = default.replace(
            "<template data-chat-message>",
            "<template data-chat-mesage>",
        );
        std::fs::write(dir.join(ENTRY), misspelled).unwrap();
        assert_eq!(check(&dir), Err("template.invalid"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
