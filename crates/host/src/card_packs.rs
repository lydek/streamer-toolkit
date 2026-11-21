//! Stamp card animation packs: a zip with `pack.json`, `index.html` and non-script assets (CSS, images, fonts,
//! sounds), unpacked into the template directory's `checkin/<id>/` and served read-only under `/templates/checkin/`
//! with the template rules (Overlay CSP, `nosniff`). Packs carry no JavaScript; anything that could run is refused
//! with the file and place named. Admin routes; the caller adds the token check.

use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as UrlPath, State, rejection::BytesRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zip::ZipArchive;

use crate::{
    admin::{ErrorBody, error},
    templates::{file_response, read_inside},
};

/// The largest zip accepted.
pub(crate) const MAX_ZIP_BYTES: usize = 10 * 1024 * 1024;
/// What one pack may unpack to, in files and bytes (a zip bomb stops here, whatever the zip claims).
const MAX_ENTRIES: usize = 200;
const MAX_UNPACKED_BYTES: u64 = 20 * 1024 * 1024;
const NAME_MAX: usize = 100;

fn packs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("templates").join("checkin")
}

/// A pack folder name: a UUID the toolkit gave.
fn pack_id(s: &str) -> bool {
    s.len() == 36
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

#[derive(Debug, Deserialize)]
struct PackJson {
    name: String,
}

/// An installed pack. `name` is the pack's own text: show it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct CardPack {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// Why a zip was refused; `detail` names the file (and the line and column in `index.html`).
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
pub(crate) struct PackRefused {
    /// `card_pack.invalid`, `card_pack.too_large` or `card_pack.script`.
    pub(crate) code: String,
    pub(crate) detail: String,
}

fn refused(code: &str, detail: impl Into<String>) -> PackRefused {
    PackRefused {
        code: code.into(),
        detail: detail.into(),
    }
}

/// Assets a pack may hold besides `pack.json` and `index.html`, by extension; none can run.
fn asset_ok(name: &str, bytes: &[u8]) -> Result<(), PackRefused> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    let ok = match ext.as_deref() {
        Some("js" | "mjs") => return Err(refused("card_pack.script", name)),
        Some("css") => std::str::from_utf8(bytes).is_ok(),
        Some("png") => starts(b"\x89PNG\r\n\x1a\n"),
        Some("jpg" | "jpeg") => starts(&[0xFF, 0xD8, 0xFF]),
        Some("gif") => starts(b"GIF87a") || starts(b"GIF89a"),
        Some("webp") => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        Some("woff") => starts(b"wOFF"),
        Some("woff2") => starts(b"wOF2"),
        Some("ttf") => starts(&[0, 1, 0, 0]) || starts(b"true"),
        Some("otf") => starts(b"OTTO"),
        Some("mp3") => {
            starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
        }
        Some("ogg") => starts(b"OggS"),
        Some("wav") => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        _ => {
            return Err(refused(
                "card_pack.invalid",
                format!("{name}: not an allowed type"),
            ));
        }
    };
    if ok {
        Ok(())
    } else {
        Err(refused(
            "card_pack.invalid",
            format!("{name}: the content does not match its extension"),
        ))
    }
}

/// Line and column (1-based) of byte `at` in `text`.
fn place(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// Refuses `<script`, `on*=` attributes and `javascript:` anywhere in `index.html`, naming where.
/// shortcut: a text scan, not an HTML parser; it errs on the side of refusing, and the Overlay CSP blocks inline
/// script anyway.
fn html_ok(text: &str) -> Result<(), PackRefused> {
    let lower = text.to_ascii_lowercase();
    let mut found = [lower.find("<script"), lower.find("javascript:")]
        .into_iter()
        .flatten()
        .min();
    // ` on<letters>=` (or after a quote or tab/newline): an inline event handler.
    let bytes = lower.as_bytes();
    for (i, w) in bytes.windows(3).enumerate() {
        if w[1] == b'o' && b"\t\n\r \"'/".contains(&w[0]) && w[2] == b'n' {
            let rest = &bytes[i + 3..];
            let letters = rest.iter().take_while(|b| b.is_ascii_alphabetic()).count();
            let after = rest[letters..].iter().find(|b| !b.is_ascii_whitespace());
            if letters > 0 && after == Some(&b'=') {
                found = Some(found.map_or(i + 1, |f| f.min(i + 1)));
                break;
            }
        }
    }
    match found {
        Some(at) => {
            let (line, column) = place(text, at);
            Err(refused(
                "card_pack.script",
                format!("index.html:{line}:{column}"),
            ))
        }
        None => Ok(()),
    }
}

/// A path inside the zip as plain `/`-separated segments: no `..`, no absolute path, no `\` or `:`.
fn plain_path(name: &str) -> bool {
    !name.starts_with('/')
        && name
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains(['\\', ':', '\0']))
}

/// A pack's files: path inside the pack and content.
type PackFiles = Vec<(String, Vec<u8>)>;

/// Checks the whole zip and returns its files and the pack's name; nothing is written.
pub(crate) fn read_pack(zip: &[u8]) -> Result<(String, PackFiles), PackRefused> {
    let mut archive = ZipArchive::new(Cursor::new(zip))
        .map_err(|_| refused("card_pack.invalid", "not a zip file"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(refused(
            "card_pack.too_large",
            format!("more than {MAX_ENTRIES} files"),
        ));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| refused("card_pack.invalid", "unreadable entry"))?;
        let Ok(name) = entry.name().map(|n| n.into_owned()) else {
            return Err(refused(
                "card_pack.invalid",
                "a file name that is not Unicode",
            ));
        };
        if entry.is_dir() {
            continue;
        }
        if !plain_path(&name) {
            return Err(refused(
                "card_pack.invalid",
                format!("{name}: a path outside the pack"),
            ));
        }
        // Read at most what is left of the budget, plus one byte to notice going over.
        let left = MAX_UNPACKED_BYTES - total;
        let mut bytes = Vec::new();
        (&mut entry)
            .take(left + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| refused("card_pack.invalid", format!("{name}: unreadable")))?;
        total += bytes.len() as u64;
        if total > MAX_UNPACKED_BYTES {
            return Err(refused("card_pack.too_large", "unpacks to more than 20 MB"));
        }
        match name.as_str() {
            "pack.json" | "index.html" => {}
            _ => asset_ok(&name, &bytes)?,
        }
        files.push((name, bytes));
    }
    let file = |n: &str| files.iter().find(|(name, _)| name == n).map(|(_, b)| b);
    let html =
        file("index.html").ok_or_else(|| refused("card_pack.invalid", "index.html is missing"))?;
    let html = std::str::from_utf8(html)
        .map_err(|_| refused("card_pack.invalid", "index.html is not UTF-8"))?;
    html_ok(html)?;
    let pack: PackJson = file("pack.json")
        .ok_or_else(|| refused("card_pack.invalid", "pack.json is missing"))
        .and_then(|b| {
            serde_json::from_slice(b)
                .map_err(|_| refused("card_pack.invalid", "pack.json needs a \"name\""))
        })?;
    let name = pack.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(refused(
            "card_pack.invalid",
            "pack.json: the name must be 1–100 characters",
        ));
    }
    Ok((name, files))
}

/// Unpacks a checked pack into a new folder; written aside first and renamed into place, so a failure leaves
/// nothing half-written.
fn install(data_dir: &Path, files: &[(String, Vec<u8>)]) -> std::io::Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let packs = packs_dir(data_dir);
    let staging = packs.join(format!(".{id}"));
    let write = || -> std::io::Result<()> {
        for (name, bytes) in files {
            let path = staging.join(name);
            std::fs::create_dir_all(path.parent().expect("inside staging"))?;
            std::fs::write(path, bytes)?;
        }
        std::fs::rename(&staging, packs.join(&id))
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    Ok(id)
}

/// Installed packs, by name.
pub(crate) fn list(data_dir: &Path) -> Vec<CardPack> {
    let Ok(entries) = std::fs::read_dir(packs_dir(data_dir)) else {
        return Vec::new();
    };
    let mut packs: Vec<CardPack> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            if !pack_id(&id) {
                return None;
            }
            let json = std::fs::read(e.path().join("pack.json")).ok()?;
            let pack: PackJson = serde_json::from_slice(&json).ok()?;
            Some(CardPack {
                id,
                name: pack.name,
            })
        })
        .collect();
    packs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    packs
}

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route(
            "/api/admin/checkin/packs",
            get(list_packs)
                .post(upload_pack)
                .layer(DefaultBodyLimit::max(MAX_ZIP_BYTES)),
        )
        .route("/api/admin/checkin/packs/{id}", delete(delete_pack))
        .with_state(data_dir)
}

/// The read-only pack files: `GET /templates/checkin/{id}/{*path}`; the template headers come from
/// `templates::headers`.
pub(crate) fn file_routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/templates/checkin/{id}/{*path}", get(serve))
        .with_state(data_dir)
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/packs",
    tag = "admin",
    responses((status = 200, body = Vec<CardPack>))
)]
pub(crate) async fn list_packs(State(data_dir): State<PathBuf>) -> Json<Vec<CardPack>> {
    Json(list(&data_dir))
}

/// Installs an animation pack: a zip (at most 10 MB) with `pack.json` (`{"name": …}`), `index.html` and CSS,
/// images, fonts or sounds. The body is the zip itself.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/packs",
    tag = "admin",
    request_body(content = String, content_type = "application/zip", description = "The pack's zip file"),
    responses(
        (status = 200, body = CardPack),
        (status = 400, description = "`card_pack.invalid` or `card_pack.script`, with the file (and place) in `detail`", body = PackRefused),
        (status = 413, description = "`card_pack.too_large`", body = PackRefused),
        (status = 500, description = "`card_pack.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn upload_pack(
    State(data_dir): State<PathBuf>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let zip = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            let r = refused("card_pack.too_large", "larger than 10 MB");
            return (StatusCode::PAYLOAD_TOO_LARGE, Json(r)).into_response();
        }
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(refused("card_pack.invalid", "no zip")),
            )
                .into_response();
        }
    };
    let result = tokio::task::spawn_blocking(move || {
        let (name, files) = read_pack(&zip).map_err(Ok)?;
        let id = install(&data_dir, &files).map_err(Err)?;
        Ok::<_, Result<PackRefused, std::io::Error>>(CardPack { id, name })
    })
    .await;
    match result {
        Ok(Ok(pack)) => Json(pack).into_response(),
        Ok(Err(Ok(r))) => {
            let status = if r.code == "card_pack.too_large" {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(r)).into_response()
        }
        Ok(Err(Err(e))) => {
            tracing::error!(error = %e, "card pack not installed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "card_pack.save_failed")
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "card_pack.save_failed"),
    }
}

/// Removes a pack; a card style that used it falls back to the simple style.
#[utoipa::path(
    delete,
    path = "/api/admin/checkin/packs/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`card_pack.not_found`", body = ErrorBody),
        (status = 500, description = "`card_pack.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_pack(
    State(data_dir): State<PathBuf>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let dir = packs_dir(&data_dir).join(&id);
    if !pack_id(&id) || !dir.is_dir() {
        return error(StatusCode::NOT_FOUND, "card_pack.not_found");
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "card pack not removed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "card_pack.save_failed")
        }
    }
}

async fn serve(
    State(data_dir): State<PathBuf>,
    UrlPath((id, path)): UrlPath<(String, String)>,
) -> Response {
    if !pack_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let base = packs_dir(&data_dir).join(id);
    tokio::task::spawn_blocking(move || file_response(&path, |p| read_inside(&base, p)))
        .await
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests;
