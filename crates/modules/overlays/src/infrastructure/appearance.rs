//! The built-in overlays' look: its admin API and the `screen.appearance` event.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use toolkit_core::events::Source;
use utoipa::ToSchema;

use super::{ErrorBody, Overlays, error, storage_error};
use crate::{
    application::OverlayError,
    domain::appearance::{Appearance, Theme},
};

pub(crate) const APPEARANCE: &str = "screen.appearance";
pub(crate) const APPEARANCE_VERSION: u32 = 1;

/// A theme for the built-in overlays; custom templates never use it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OverlayTheme {
    Dark,
    Cream,
    Contrast,
}

impl From<Theme> for OverlayTheme {
    fn from(t: Theme) -> Self {
        match t {
            Theme::Dark => Self::Dark,
            Theme::Cream => Self::Cream,
            Theme::Contrast => Self::Contrast,
        }
    }
}

impl From<OverlayTheme> for Theme {
    fn from(t: OverlayTheme) -> Self {
        match t {
            OverlayTheme::Dark => Self::Dark,
            OverlayTheme::Cream => Self::Cream,
            OverlayTheme::Contrast => Self::Contrast,
        }
    }
}

/// The built-in overlays' theme and text size; also the body of `PUT /api/admin/screen-tools/appearance`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppearancePayload {
    pub theme: OverlayTheme,
    /// Percent of each overlay's own text size: 50–200 in steps of 10.
    pub text_size: u16,
}

impl From<Appearance> for AppearancePayload {
    fn from(a: Appearance) -> Self {
        Self {
            theme: a.theme.into(),
            text_size: a.text_size,
        }
    }
}

/// A `screen.appearance` event exactly as `/ws/overlay` sends it after any change and at start; the snapshot keeps
/// only the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: AppearanceType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: AppearancePayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum AppearanceType {
    #[serde(rename = "screen.appearance")]
    ScreenAppearance,
}

pub(crate) fn routes() -> Router<Arc<Overlays>> {
    Router::new().route(
        "/api/admin/screen-tools/appearance",
        get(get_appearance).put(set_appearance),
    )
}

fn body(result: Result<Appearance, OverlayError>) -> Response {
    match result {
        Ok(a) => Json(AppearancePayload::from(a)).into_response(),
        Err(OverlayError::Invalid) => error(StatusCode::BAD_REQUEST, "overlays.invalid"),
        Err(OverlayError::Storage(e)) => storage_error(&e),
    }
}

/// The built-in overlays' look now (also kept current by `screen.appearance`).
#[utoipa::path(
    get,
    path = "/api/admin/screen-tools/appearance",
    tag = "admin",
    responses(
        (status = 200, body = AppearancePayload),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn get_appearance(State(s): State<Arc<Overlays>>) -> Response {
    body(s.appearance().await.map_err(Into::into))
}

/// Saves the look; open built-in overlays change at once.
#[utoipa::path(
    put,
    path = "/api/admin/screen-tools/appearance",
    tag = "admin",
    request_body = AppearancePayload,
    responses(
        (status = 200, body = AppearancePayload),
        (status = 400, description = "`overlays.invalid`: text size out of range", body = ErrorBody),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_appearance(
    State(s): State<Arc<Overlays>>,
    payload: Result<Json<AppearancePayload>, JsonRejection>,
) -> Response {
    let Ok(Json(p)) = payload else {
        return error(StatusCode::BAD_REQUEST, "overlays.invalid");
    };
    body(
        s.set_appearance(Appearance {
            theme: p.theme.into(),
            text_size: p.text_size,
        })
        .await,
    )
}
