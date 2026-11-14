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
    Json(settings::load(&data_dir).home_tiles)
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
    if settings::update(&data_dir, |s| s.home_tiles = tiles).is_err() {
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
