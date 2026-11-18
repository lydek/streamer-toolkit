//! The admin UI's tab bar (pinned groups in order, top or bottom), kept in `settings.json`. The Host checks the
//! format only; which group IDs exist is the admin UI's business (it skips unknown ones). Admin routes; the caller
//! adds the token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings::{self, TabBar};

use crate::admin::{ErrorBody, error};

const MAX_GROUPS: usize = 20;
const MAX_ID_LEN: usize = 32;

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/tab-bar", get(tab_bar).put(set_tab_bar))
        .with_state(data_dir)
}

/// The tab bar; `null` when never edited (every group pinned, on top).
#[utoipa::path(
    get,
    path = "/api/admin/tab-bar",
    tag = "admin",
    responses((status = 200, body = Option<TabBar>))
)]
pub(crate) async fn tab_bar(State(data_dir): State<PathBuf>) -> Json<Option<TabBar>> {
    Json(settings::load(&data_dir).tab_bar)
}

/// Saves the tab bar: up to 20 group IDs of 1-32 characters `[a-z0-9-]`, duplicates keep their first position;
/// `null` restores the defaults.
#[utoipa::path(
    put,
    path = "/api/admin/tab-bar",
    tag = "admin",
    request_body = Option<TabBar>,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_tab_bar(
    State(data_dir): State<PathBuf>,
    body: Result<Json<Option<TabBar>>, JsonRejection>,
) -> Response {
    // Malformed bodies get the same error code as bad IDs instead of axum's plain-text rejection.
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "tab_bar.invalid");
    };
    let bar = match body {
        None => None,
        Some(bar) => match checked(bar.groups) {
            Some(groups) => Some(TabBar { groups, ..bar }),
            None => return error(StatusCode::BAD_REQUEST, "tab_bar.invalid"),
        },
    };
    if settings::update(&data_dir, |s| s.tab_bar = bar).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The IDs without duplicates, or `None` when the list or an ID is out of format.
fn checked(ids: Vec<String>) -> Option<Vec<String>> {
    if ids.len() > MAX_GROUPS {
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
            checked(ids(&["home", "media", "home"])),
            Some(ids(&["home", "media"]))
        );
        assert_eq!(checked(Vec::new()), Some(Vec::new()));
        assert_eq!(checked(ids(&[&"a".repeat(33)])), None);
        assert_eq!(checked(ids(&["Home"])), None);
        assert_eq!(checked(ids(&["<b>"])), None);
        assert_eq!(checked(vec!["a".to_string(); 21]), None);
    }
}
