//! Admin home tile order, one per stream stage (開播前, 直播中, 下播後), kept in `settings.json`. The Host checks the
//! format only; which IDs exist is the admin UI's business (it skips unknown ones). Admin routes; the caller adds the
//! token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use toolkit_core::settings::{self, Settings};
use utoipa::{IntoParams, ToSchema};

use crate::admin::{ErrorBody, error};

const MAX_TILES: usize = 50;
const MAX_ID_LEN: usize = 32;
/// The tiles of the old 「直播中」 group, which home showed before the others while it grouped its tiles.
const OLD_LIVE_GROUP: [&str; 3] = ["chat", "alerts", "feed"];
/// The 直播中 blocks that joined the grid in version 2, in their default order; before, they always showed above
/// it. The saved list was the only home then, so it became the 直播中 one.
const STAGE_BLOCKS: [&str; 3] = ["pending", "stats", "queue"];
/// The home `home_tiles` is saved for now (`Settings::home_tiles_version`).
const VERSION: u8 = 3;

/// Which stage's home.
#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HomeStage {
    Before,
    #[default]
    Live,
    After,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct StageQuery {
    /// `before`, `live` (the default) or `after`.
    #[param(inline)]
    stage: Option<HomeStage>,
}

fn stage_of(query: Result<Query<StageQuery>, QueryRejection>) -> Option<HomeStage> {
    query.ok().map(|Query(q)| q.stage.unwrap_or_default())
}

fn saved_for(s: &mut Settings, stage: HomeStage) -> &mut Option<Vec<String>> {
    match stage {
        HomeStage::Before => &mut s.home_tiles_before,
        HomeStage::Live => &mut s.home_tiles,
        HomeStage::After => &mut s.home_tiles_after,
    }
}

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/home-tiles", get(home_tiles).put(set_home_tiles))
        .with_state(data_dir)
}

/// A stage's tile IDs in display order; `null` when never arranged (the admin UI shows that stage's defaults).
#[utoipa::path(
    get,
    path = "/api/admin/home-tiles",
    tag = "admin",
    params(StageQuery),
    responses(
        (status = 200, body = Option<Vec<String>>),
        (status = 400, description = "`home.invalid_tiles`: unknown stage", body = ErrorBody),
    )
)]
pub(crate) async fn home_tiles(
    State(data_dir): State<PathBuf>,
    query: Result<Query<StageQuery>, QueryRejection>,
) -> Response {
    let Some(stage) = stage_of(query) else {
        return error(StatusCode::BAD_REQUEST, "home.invalid_tiles");
    };
    let mut saved = settings::load(&data_dir);
    if !matches!(stage, HomeStage::Live) {
        return Json(saved_for(&mut saved, stage).take()).into_response();
    }
    if saved.home_tiles.is_none() || saved.home_tiles_version >= VERSION {
        return Json(saved.home_tiles).into_response();
    }
    // A list saved for an older home: brought up to date and saved, once. If saving fails, the next read tries again.
    let from = saved.home_tiles_version;
    let mut shown = saved.home_tiles.map(|ids| upgraded(ids, from));
    if let Ok(now) = settings::update(&data_dir, |s| {
        if s.home_tiles_version < VERSION {
            let from = s.home_tiles_version;
            s.home_tiles = s.home_tiles.take().map(|ids| upgraded(ids, from));
            s.home_tiles_version = VERSION;
        }
    }) {
        shown = now.home_tiles;
    }
    Json(shown).into_response()
}

/// `ids` as the current home shows what the home of version `from` showed.
fn upgraded(ids: Vec<String>, from: u8) -> Vec<String> {
    let mut ids = ids;
    if from < 1 {
        // The grouped order: the 「直播中」 tiles first, each group keeping its saved order.
        let (live, rest): (Vec<_>, Vec<_>) = ids
            .into_iter()
            .partition(|id| OLD_LIVE_GROUP.contains(&id.as_str()));
        ids = live.into_iter().chain(rest).collect();
    }
    if from < 2 {
        // The stage blocks always showed above the tiles, so they come first, even on an emptied home.
        let mut first: Vec<String> = STAGE_BLOCKS
            .iter()
            .filter(|b| !ids.iter().any(|id| id == *b))
            .map(|b| (*b).to_owned())
            .collect();
        first.append(&mut ids);
        ids = first;
        // Still a list `PUT` accepts, so what is read can be saved back: the last IDs give way to the blocks (with
        // only 14 tiles today, those are IDs the admin UI does not know and skips anyway).
        ids.truncate(MAX_TILES);
    }
    if from == 2 {
        // Version 2 (one home for every stage) also put 開播前檢查 and 本場摘要 first; they now live in the 開播前 and
        // 下播後 homes, so the 直播中 one drops them.
        ids.retain(|id| id != "checklist" && id != "summary");
    }
    ids
}

/// Saves a stage's tile order: up to 50 IDs of 1-32 characters `[a-z0-9-]`; duplicates keep their first position.
/// `null` restores that stage's defaults; `[]` keeps it empty. The other stages are left as they are.
#[utoipa::path(
    put,
    path = "/api/admin/home-tiles",
    tag = "admin",
    params(StageQuery),
    request_body = Option<Vec<String>>,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_home_tiles(
    State(data_dir): State<PathBuf>,
    query: Result<Query<StageQuery>, QueryRejection>,
    body: Result<Json<Option<Vec<String>>>, JsonRejection>,
) -> Response {
    // Malformed bodies and stages get the same error code as bad IDs instead of axum's plain-text rejection.
    let (Some(stage), Ok(Json(body))) = (stage_of(query), body) else {
        return error(StatusCode::BAD_REQUEST, "home.invalid_tiles");
    };
    let tiles = match body.map(checked) {
        None => None,
        Some(Some(tiles)) => Some(tiles),
        Some(None) => return error(StatusCode::BAD_REQUEST, "home.invalid_tiles"),
    };
    if settings::update(&data_dir, |s| {
        if matches!(stage, HomeStage::Live) {
            // Defaults (`None`) have no order to mark, so restoring them still writes nothing.
            s.home_tiles_version = if tiles.is_some() { VERSION } else { 0 };
        }
        *saved_for(s, stage) = tiles;
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
