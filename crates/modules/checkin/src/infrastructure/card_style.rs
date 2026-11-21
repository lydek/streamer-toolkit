//! The stamp card style's admin API and the read-only card image route.

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
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use toolkit_core::settings::CardStyle;
use utoipa::{IntoParams, ToSchema};

use super::http::{ErrorBody, error};
use crate::application::{CheckinService, IMAGE_MAX_BYTES, StyleError};

/// The stamp card's look. `title` and `description` may hold `#觀眾#`, `#章數#`, `#次數#` and `#格數#`; they are
/// broadcaster text, shown as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CardStyleBody {
    /// `simple` or `journal`.
    pub preset: String,
    /// `#RRGGBB`.
    pub background_color: String,
    /// A name `POST /api/admin/checkin/images` gave; `null`: the color only.
    pub background_image: Option<String>,
    /// `#RRGGBB`.
    pub stamp_color: String,
    /// A name `POST /api/admin/checkin/images` gave; `null`: a plain stamp in the color.
    pub stamp_image: Option<String>,
    /// 1–20.
    pub per_row: u32,
    /// At most 100 characters after trimming.
    pub title: String,
    /// At most 100 characters after trimming.
    pub description: String,
    /// `fade`, `slide`, `pop` or `none`.
    pub animation: String,
    /// A repeat check-in on the same business day shows the card too; nothing is recorded.
    pub show_repeat: bool,
}

impl From<CardStyle> for CardStyleBody {
    fn from(s: CardStyle) -> Self {
        Self {
            preset: s.preset,
            background_color: s.background_color,
            background_image: s.background_image,
            stamp_color: s.stamp_color,
            stamp_image: s.stamp_image,
            per_row: s.per_row,
            title: s.title,
            description: s.description,
            animation: s.animation,
            show_repeat: s.show_repeat,
        }
    }
}

impl From<CardStyleBody> for CardStyle {
    fn from(b: CardStyleBody) -> Self {
        Self {
            preset: b.preset,
            background_color: b.background_color,
            background_image: b.background_image,
            stamp_color: b.stamp_color,
            stamp_image: b.stamp_image,
            per_row: b.per_row,
            title: b.title,
            description: b.description,
            animation: b.animation,
            show_repeat: b.show_repeat,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CardImageQuery {
    /// The original file name: its extension must match the content.
    pub name: String,
}

/// A stored card image.
#[derive(Debug, Serialize, ToSchema)]
pub struct CardImage {
    /// The name to put in the style.
    pub name: String,
    /// `/checkin/{name}`, readable without a token.
    pub url: String,
}

fn style_error(e: StyleError) -> Response {
    match e {
        StyleError::Invalid => error(StatusCode::BAD_REQUEST, "checkin.invalid_style"),
        StyleError::UnsupportedImage => error(StatusCode::BAD_REQUEST, "checkin.unsupported_image"),
        StyleError::TooLarge => error(StatusCode::PAYLOAD_TOO_LARGE, "checkin.image_too_large"),
        StyleError::SaveFailed(e) => {
            tracing::error!(error = %e, "card style not saved");
            error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed")
        }
    }
}

pub(crate) fn routes() -> Router<Arc<CheckinService>> {
    Router::new()
        .route(
            "/api/admin/checkin/card-style",
            get(card_style).put(set_card_style),
        )
        .route("/api/admin/checkin/card-style/test", post(test_card_style))
        .route(
            "/api/admin/checkin/images",
            post(import_card_image).layer(DefaultBodyLimit::max(IMAGE_MAX_BYTES)),
        )
}

/// The read-only card image route for the stamp card overlay and the admin UI; it needs no token.
pub fn image_routes(service: Arc<CheckinService>) -> Router {
    Router::new()
        .route("/checkin/{name}", get(card_image))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/card-style",
    tag = "admin",
    responses((status = 200, body = CardStyleBody))
)]
pub(crate) async fn card_style(State(s): State<Arc<CheckinService>>) -> Json<CardStyleBody> {
    Json(s.config().style().into())
}

/// Saves the style; the next card on the overlay shows it.
#[utoipa::path(
    put,
    path = "/api/admin/checkin/card-style",
    tag = "admin",
    request_body = CardStyleBody,
    responses(
        (status = 200, description = "The style as saved", body = CardStyleBody),
        (status = 400, description = "`checkin.invalid_style`: out of range, or an image the folder does not have", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_card_style(
    State(s): State<Arc<CheckinService>>,
    body: Result<Json<CardStyleBody>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "checkin.invalid_style");
    };
    match s.config().set_style(b.into()) {
        Ok(style) => Json(CardStyleBody::from(style)).into_response(),
        Err(e) => style_error(e),
    }
}

/// Copies a PNG, JPG, GIF or WebP image (at most 5 MB, no SVG) in for the style. The body is the file itself.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/images",
    tag = "admin",
    params(CardImageQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The image file"),
    responses(
        (status = 200, body = CardImage),
        (status = 400, description = "`checkin.unsupported_image`: not a PNG, JPG, GIF or WebP file (by extension and content)", body = ErrorBody),
        (status = 413, description = "`checkin.image_too_large`: larger than 5 MB", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn import_card_image(
    State(s): State<Arc<CheckinService>>,
    query: Result<Query<CardImageQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "checkin.unsupported_image");
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return style_error(StyleError::TooLarge);
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "checkin.unsupported_image"),
    };
    match s.config().import_image(&q.name, &bytes) {
        Ok(name) => Json(CardImage {
            url: format!("/checkin/{name}"),
            name,
        })
        .into_response(),
        Err(e) => style_error(e),
    }
}

/// 在 OBS 試播: shows a sample card in the saved style through the presentation queue (`source=simulation`);
/// nothing is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/card-style/test",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn test_card_style(State(s): State<Arc<CheckinService>>) -> StatusCode {
    let slots = s.config().rules().slots.get();
    s.simulate_card("測試觀眾", slots.div_ceil(2));
    StatusCode::NO_CONTENT
}

/// A card image with the Content-Type of its format, never sniffed.
async fn card_image(State(s): State<Arc<CheckinService>>, Path(name): Path<String>) -> Response {
    let Some((path, format)) = s.config().image(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
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
