//! Admin home tile order, kept in `settings.json`. The Host checks the format only; which IDs exist is the admin
//! UI's business (it skips unknown ones). Admin routes; the caller adds the token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings;

use crate::admin::{ErrorBody, error};

const MAX_TILES: usize = 50;
const MAX_ID_LEN: usize = 32;
/// The tiles of the old 「直播中」 group, which home showed before the others while it grouped its tiles.
const OLD_LIVE_GROUP: [&str; 3] = ["chat", "alerts", "feed"];

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/home-tiles", get(home_tiles).put(set_home_tiles))
        .with_state(data_dir)
}

/// Tile IDs in display order; `null` when never arranged (the admin UI shows its defaults).
#[utoipa::path(
    get,
    path = "/api/admin/home-tiles",
    tag = "admin",
    responses((status = 200, body = Option<Vec<String>>))
)]
pub(crate) async fn home_tiles(State(data_dir): State<PathBuf>) -> Json<Option<Vec<String>>> {
    let saved = settings::load(&data_dir);
    if saved.home_tiles.is_none() || saved.home_tiles_flat {
        return Json(saved.home_tiles);
    }
    // A list from the grouped home: saved in the order it was shown, once. If saving fails, the next read tries again.
    let mut shown = saved.home_tiles.map(as_shown);
    if let Ok(now) = settings::update(&data_dir, |s| {
        if !s.home_tiles_flat {
            s.home_tiles = s.home_tiles.take().map(as_shown);
            s.home_tiles_flat = true;
        }
    }) {
        shown = now.home_tiles;
    }
    Json(shown)
}

/// The old grouped order: the 「直播中」 tiles first, each group keeping its saved order.
fn as_shown(ids: Vec<String>) -> Vec<String> {
    let (live, rest): (Vec<_>, Vec<_>) = ids
        .into_iter()
        .partition(|id| OLD_LIVE_GROUP.contains(&id.as_str()));
    live.into_iter().chain(rest).collect()
}

/// Saves the tile order: up to 50 IDs of 1-32 characters `[a-z0-9-]`; duplicates keep their first position.
/// `null` restores the defaults; `[]` keeps the home empty.
#[utoipa::path(
    put,
    path = "/api/admin/home-tiles",
    tag = "admin",
    request_body = Option<Vec<String>>,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_home_tiles(
    State(data_dir): State<PathBuf>,
    body: Result<Json<Option<Vec<String>>>, JsonRejection>,
) -> Response {
    // Malformed bodies get the same error code as bad IDs instead of axum's plain-text rejection.
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "home.invalid_tiles");
    };
    let tiles = match body.map(checked) {
        None => None,
        Some(Some(tiles)) => Some(tiles),
        Some(None) => return error(StatusCode::BAD_REQUEST, "home.invalid_tiles"),
    };
    if settings::update(&data_dir, |s| {
        // Defaults (`None`) have no order to mark, so restoring them still writes nothing.
        s.home_tiles_flat = tiles.is_some();
        s.home_tiles = tiles;
    })
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The IDs without duplicates, or `None` when the list or an ID is out of format.
fn checked(ids: Vec<String>) -> Option<Vec<String>> {
    if ids.len() > MAX_TILES {
        return None;
    }
    let mut unique = Vec::with_capacity(ids.len());
    for id in ids {
        let valid = (1..=MAX_ID_LEN).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return None;
        }
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    Some(unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn format_check() {
        assert_eq!(
            checked(ids(&["chat", "check-in2", "chat"])),
            Some(ids(&["chat", "check-in2"]))
        );
        assert_eq!(checked(Vec::new()), Some(Vec::new()));
        assert_eq!(
            checked(ids(&[&"a".repeat(32)])),
            Some(ids(&[&"a".repeat(32)]))
        );
        assert_eq!(checked(ids(&[&"a".repeat(33)])), None);
        assert_eq!(checked(ids(&[""])), None);
        assert_eq!(checked(ids(&["Chat"])), None);
        assert_eq!(checked(ids(&["chat_box"])), None);
        assert_eq!(checked(ids(&["<script>"])), None);
        assert_eq!(checked(vec!["a".to_string(); 50]), Some(ids(&["a"])));
        assert_eq!(checked(vec!["a".to_string(); 51]), None);
    }
}
