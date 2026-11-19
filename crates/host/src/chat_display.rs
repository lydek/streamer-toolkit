//! How the admin UI shows chat: Twitch badge images, viewers' avatars, and the display settings kept in
//! `settings.json`. Admin routes; the caller adds the token check.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use integrations::{
    tls::HttpsClient,
    twitch::{auth::TwitchAuth, helix::Helix},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};
use toolkit_core::{
    settings::{self, ChatDisplay},
    twitch::is_twitch_image,
};
use utoipa::{IntoParams, ToSchema};

use crate::admin::{ErrorBody, error};

/// The most viewers one avatar request asks about.
const AVATARS: usize = 200;

/// The broadcaster the badges were read for, and the badges.
type BadgeCache = Arc<Mutex<Option<(String, Vec<BadgeImage>)>>>;

#[derive(Clone)]
pub(crate) struct DisplayState {
    data_dir: PathBuf,
    pool: SqlitePool,
    helix: String,
    http: HttpsClient,
    twitch: Arc<RwLock<TwitchAuth>>,
    /// The badges of the broadcaster they were read for, kept for this launch.
    badges: BadgeCache,
}

pub(crate) fn routes(
    data_dir: PathBuf,
    pool: SqlitePool,
    helix: String,
    http: HttpsClient,
    twitch: Arc<RwLock<TwitchAuth>>,
) -> Router {
    Router::new()
        .route(
            "/api/admin/chat-display",
            get(chat_display).put(set_chat_display),
        )
        .route("/api/admin/chat/badges", get(badges))
        .route("/api/admin/viewers/avatars", get(avatars))
        .with_state(DisplayState {
            data_dir,
            pool,
            helix,
            http,
            twitch,
            badges: Arc::default(),
        })
}

/// The chat display settings; `null` when never changed (the admin UI shows its defaults).
#[utoipa::path(
    get,
    path = "/api/admin/chat-display",
    tag = "admin",
    responses((status = 200, body = Option<ChatDisplay>))
)]
pub(crate) async fn chat_display(State(s): State<DisplayState>) -> Json<Option<ChatDisplay>> {
    Json(settings::load(&s.data_dir).chat_display)
}

/// Saves the chat display settings; `textSize` is 80-150 in steps of 10.
#[utoipa::path(
    put,
    path = "/api/admin/chat-display",
    tag = "admin",
    request_body = ChatDisplay,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_chat_display(
    State(s): State<DisplayState>,
    body: Result<Json<ChatDisplay>, JsonRejection>,
) -> Response {
    let Ok(Json(display)) = body else {
        return error(StatusCode::BAD_REQUEST, "chat_display.invalid");
    };
    if !(80..=150).contains(&display.text_size) || display.text_size % 10 != 0 {
        return error(StatusCode::BAD_REQUEST, "chat_display.invalid");
    }
    if settings::update(&s.data_dir, |st| st.chat_display = Some(display)).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// One Twitch chat badge version and its image.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BadgeImage {
    set_id: String,
    id: String,
    /// Twitch's name for it, e.g. `Moderator` (raw text).
    title: String,
    /// On Twitch's image CDN only.
    url: String,
}

/// Twitch's global chat badges and the broadcaster's own (subscriber badges and the like), read once per launch;
/// empty while signed out or when Twitch cannot be reached, so chat just shows no badge images.
#[utoipa::path(
    get,
    path = "/api/admin/chat/badges",
    tag = "admin",
    responses((status = 200, body = Vec<BadgeImage>))
)]
pub(crate) async fn badges(State(s): State<DisplayState>) -> Json<Vec<BadgeImage>> {
    let auth = s.twitch.read().await.clone();
    let Some(broadcaster) = auth.account().borrow().clone() else {
        return Json(Vec::new());
    };
    let mut cache = s.badges.lock().await;
    if let Some((owner, badges)) = cache.as_ref()
        && *owner == broadcaster
    {
        return Json(badges.clone());
    }
    let helix = Helix::new(&s.helix, auth, s.http.clone());
    let mut all = Vec::new();
    for path in [
        "/chat/badges/global".to_owned(),
        format!("/chat/badges?broadcaster_id={broadcaster}"),
    ] {
        match helix.get_json(&path).await {
            Ok(body) => all.extend(badge_images(&body)),
            Err(e) => {
                tracing::warn!(error = %e, "chat badges not read");
                return Json(Vec::new());
            }
        }
    }
    *cache = Some((broadcaster, all.clone()));
    Json(all)
}

/// The badge versions in a Helix Get Chat Badges answer whose image is on Twitch's CDN; a channel's version comes
/// after (and so wins over) the global one with the same set and ID.
fn badge_images(body: &serde_json::Value) -> Vec<BadgeImage> {
    let mut out = Vec::new();
    for set in body["data"].as_array().into_iter().flatten() {
        let Some(set_id) = set["set_id"].as_str() else {
            continue;
        };
        for v in set["versions"].as_array().into_iter().flatten() {
            let (Some(id), Some(url)) = (v["id"].as_str(), v["image_url_2x"].as_str()) else {
                continue;
            };
            if !is_twitch_image(url) {
                continue;
            }
            out.push(BadgeImage {
                set_id: set_id.to_owned(),
                id: id.to_owned(),
                title: v["title"].as_str().unwrap_or_default().to_owned(),
                url: url.to_owned(),
            });
        }
    }
    out
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AvatarQuery {
    /// Twitch user IDs, comma-separated (at most 200).
    ids: String,
}

/// The stored avatars of these viewers, by Twitch user ID; viewers without one (yet) are left out.
#[utoipa::path(
    get,
    path = "/api/admin/viewers/avatars",
    tag = "admin",
    params(AvatarQuery),
    responses(
        (status = 200, body = std::collections::HashMap<String, String>),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn avatars(
    State(s): State<DisplayState>,
    Query(q): Query<AvatarQuery>,
) -> Response {
    let ids: Vec<String> = q
        .ids
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    if ids.len() > AVATARS
        || !ids
            .iter()
            .all(|id| id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()))
    {
        return error(StatusCode::BAD_REQUEST, "viewers.invalid_ids");
    }
    match viewers::avatars(&s.pool, &ids).await {
        Ok(found) => Json(found).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "avatars not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "viewers.storage_failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_images_keep_only_twitch_cdn_urls() {
        let body = serde_json::json!({"data": [
            {"set_id": "moderator", "versions": [
                {"id": "1", "title": "Moderator", "image_url_2x": "https://static-cdn.jtvnw.net/badges/v1/mod/2"}]},
            {"set_id": "evil", "versions": [
                {"id": "1", "title": "Evil", "image_url_2x": "https://evil.example/x.png"},
                {"id": "2", "title": "No URL"}]}
        ]});
        assert_eq!(
            badge_images(&body),
            [BadgeImage {
                set_id: "moderator".into(),
                id: "1".into(),
                title: "Moderator".into(),
                url: "https://static-cdn.jtvnw.net/badges/v1/mod/2".into(),
            }]
        );
    }
}
