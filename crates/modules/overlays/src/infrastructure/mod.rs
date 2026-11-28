//! Adapters: the admin API (behind the admin token, added by the host), the `screen.break`, `screen.warnings` and
//! `screen.appearance` events, the media library's files and storage.

mod appearance;
pub mod db;
mod media;
mod warnings;

pub use media::{MediaPlayEvent, file_routes as media_file_routes};

pub use appearance::AppearanceEvent;
pub use warnings::WarningsEvent;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::Modules,
    presentation::Presenter,
};
use utoipa::{OpenApi, ToSchema};

use crate::{
    application::{OverlayError, OverlayService, PlaybackDeps, Publishers, RepositoryError},
    domain::{BreakCard, BreakSettings, Countdown, CountdownMode},
};
use toolkit_core::time::Timestamp;

/// The screen tools service the host runs.
pub type Overlays = OverlayService<db::SqliteOverlays>;

const BREAK: &str = "screen.break";
const BREAK_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BreakMode {
    Minutes,
    Until,
}

impl From<CountdownMode> for BreakMode {
    fn from(m: CountdownMode) -> Self {
        match m {
            CountdownMode::Minutes => Self::Minutes,
            CountdownMode::Until => Self::Until,
        }
    }
}

impl From<BreakMode> for CountdownMode {
    fn from(m: BreakMode) -> Self {
        match m {
            BreakMode::Minutes => Self::Minutes,
            BreakMode::Until => Self::Until,
        }
    }
}

/// The break card. `text` and `endText` are broadcaster text: render them as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakPayload {
    pub shown: bool,
    pub text: String,
    /// Shown instead of the countdown once `endsAt` has passed; the card stays until hidden.
    pub end_text: String,
    /// UTC, RFC 3339; set while shown.
    pub ends_at: Option<String>,
    pub mode: BreakMode,
    pub minutes: u32,
    /// `HH:MM`, local time.
    pub until_time: String,
}

impl From<&BreakCard> for BreakPayload {
    fn from(c: &BreakCard) -> Self {
        let s = &c.settings;
        Self {
            shown: c.shown,
            text: s.text.clone(),
            end_text: s.end_text.clone(),
            ends_at: c.ends_at.map(db::rfc3339),
            mode: s.mode.into(),
            minutes: s.minutes,
            until_time: s.until_time.clone(),
        }
    }
}

/// A `screen.break` event exactly as `/ws/overlay` sends it after any change and at start; the snapshot keeps only
/// the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: BreakType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: BreakPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum BreakType {
    #[serde(rename = "screen.break")]
    ScreenBreak,
}

/// Starts the screen tools in `pool`, which must already have [`db::MIGRATOR`] applied, announcing the break card
/// as `screen.break` and the warnings as `screen.warnings` on `bus` now and after every change. The host feeds the
/// warnings what it sees ([`OverlayService::see`]). Needs a Tokio runtime.
/// What the screen tools need from the rest of the app.
pub struct Deps {
    pub data_dir: PathBuf,
    /// Queues reward videos and GIFs with the other overlay items.
    pub presenter: Arc<Presenter>,
    pub modules: Arc<Modules>,
    /// The sound module's master volume, in percent: a reward video plays at it unless its rule sets its own.
    pub master_volume: Box<dyn Fn() -> u8 + Send + Sync>,
}

/// Media files live in the data folder's `media/`.
pub fn start(pool: SqlitePool, bus: Arc<EventBus>, deps: Deps) -> Arc<Overlays> {
    bus.retain(BREAK, 1);
    bus.retain(warnings::WARNINGS, 1);
    bus.retain(appearance::APPEARANCE, 1);
    let (warnings_bus, appearance_bus, media_bus) = (bus.clone(), bus.clone(), bus.clone());
    let event = |event_type, version, payload| Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type,
        version,
        occurred_at: db::rfc3339(Timestamp::now()),
        source: Source::System,
        payload,
    };
    let service = Arc::new(OverlayService::new(
        db::SqliteOverlays::new(pool),
        deps.data_dir.join("media"),
        Publishers {
            break_card: Box::new(move |card| {
                let payload =
                    serde_json::to_value(BreakPayload::from(card)).expect("break card serializes");
                bus.publish(event(BREAK, BREAK_VERSION, payload));
            }),
            warnings: Box::new(move |settings, now| {
                let payload = serde_json::to_value(warnings::WarningsPayload::new(settings, now))
                    .expect("warnings serialize");
                warnings_bus.publish(event(
                    warnings::WARNINGS,
                    warnings::WARNINGS_VERSION,
                    payload,
                ));
            }),
            appearance: Box::new(move |a| {
                let payload = serde_json::to_value(appearance::AppearancePayload::from(a))
                    .expect("appearance serializes");
                appearance_bus.publish(event(
                    appearance::APPEARANCE,
                    appearance::APPEARANCE_VERSION,
                    payload,
                ));
            }),
            media: Arc::new(move |show, source| {
                let payload = serde_json::to_value(media::MediaPlayPayload::from(show))
                    .expect("media play serializes");
                media_bus.publish(Event {
                    source,
                    ..event(media::MEDIA_PLAY, media::MEDIA_PLAY_VERSION, payload)
                });
            }),
        },
        PlaybackDeps {
            presenter: deps.presenter,
            modules: deps.modules,
            master_volume: deps.master_volume,
        },
    ));
    let first = service.clone();
    tokio::spawn(async move {
        if let Err(e) = first.announce().await {
            tracing::error!(code = "overlays.storage_failed", error = %e.0, "screen tools not loaded");
        }
    });
    service
}

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn storage_error(e: &RepositoryError) -> Response {
    tracing::error!(code = "overlays.storage_failed", error = %e.0, "screen tools not stored");
    error(StatusCode::INTERNAL_SERVER_ERROR, "overlays.storage_failed")
}

fn break_body(result: Result<BreakCard, OverlayError>) -> Response {
    match result {
        Ok(card) => Json(BreakPayload::from(&card)).into_response(),
        Err(OverlayError::Invalid) => error(StatusCode::BAD_REQUEST, "overlays.invalid"),
        Err(OverlayError::Storage(e)) => storage_error(&e),
    }
}

/// The break card's settings.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BreakSettingsBody {
    /// At most 100 characters after trimming; may be empty.
    pub text: String,
    /// At most 100 characters after trimming; may be empty.
    pub end_text: String,
    pub mode: BreakMode,
    /// 1–1440.
    pub minutes: u32,
    /// `HH:MM`.
    pub until_time: String,
}

/// Shows the card: give `minutes` (1–1440) or `until` (UTC, RFC 3339, within the next 24 hours).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ShowBreakBody {
    pub minutes: Option<u32>,
    pub until: Option<String>,
}

pub fn routes(service: Arc<Overlays>) -> Router {
    Router::new()
        .route(
            "/api/admin/screen-tools/break",
            get(get_break).put(set_break_settings),
        )
        .route("/api/admin/screen-tools/break/show", post(show_break))
        .route("/api/admin/screen-tools/break/hide", post(hide_break))
        .merge(warnings::routes())
        .merge(appearance::routes())
        .merge(media::routes())
        .with_state(service)
}

/// The break card now (also kept current by `screen.break`).
#[utoipa::path(
    get,
    path = "/api/admin/screen-tools/break",
    tag = "admin",
    responses(
        (status = 200, body = BreakPayload),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn get_break(State(s): State<Arc<Overlays>>) -> Response {
    break_body(s.break_card().await.map_err(Into::into))
}

/// Saves the settings; a shown card keeps its countdown and shows the new texts.
#[utoipa::path(
    put,
    path = "/api/admin/screen-tools/break",
    tag = "admin",
    request_body = BreakSettingsBody,
    responses(
        (status = 200, body = BreakPayload),
        (status = 400, description = "`overlays.invalid`: out of range", body = ErrorBody),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_break_settings(
    State(s): State<Arc<Overlays>>,
    body: Result<Json<BreakSettingsBody>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "overlays.invalid");
    };
    break_body(
        s.set_break_settings(BreakSettings {
            text: b.text,
            end_text: b.end_text,
            mode: b.mode.into(),
            minutes: b.minutes,
            until_time: b.until_time,
        })
        .await,
    )
}

/// Shows the card and starts its countdown from now.
#[utoipa::path(
    post,
    path = "/api/admin/screen-tools/break/show",
    tag = "admin",
    request_body = ShowBreakBody,
    responses(
        (status = 200, body = BreakPayload),
        (status = 400, description = "`overlays.invalid`: neither or both given, or out of range", body = ErrorBody),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn show_break(
    State(s): State<Arc<Overlays>>,
    body: Result<Json<ShowBreakBody>, JsonRejection>,
) -> Response {
    let countdown = match body {
        Ok(Json(ShowBreakBody {
            minutes: Some(m),
            until: None,
        })) => Some(Countdown::Minutes(m)),
        Ok(Json(ShowBreakBody {
            minutes: None,
            until: Some(t),
        })) => t.parse::<Timestamp>().ok().map(Countdown::Until),
        _ => None,
    };
    let Some(countdown) = countdown else {
        return error(StatusCode::BAD_REQUEST, "overlays.invalid");
    };
    break_body(s.show_break(countdown, Timestamp::now()).await)
}

#[utoipa::path(
    post,
    path = "/api/admin/screen-tools/break/hide",
    tag = "admin",
    responses(
        (status = 200, body = BreakPayload),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn hide_break(State(s): State<Arc<Overlays>>) -> Response {
    break_body(s.hide_break().await)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_break,
        set_break_settings,
        show_break,
        hide_break,
        warnings::get_warnings,
        warnings::set_warnings,
        appearance::get_appearance,
        appearance::set_appearance,
        media::list_media,
        media::import_media,
        media::update_media,
        media::delete_media,
        media::list_rules,
        media::set_rule,
        media::delete_rule,
        media::test_rule
    ),
    components(schemas(BreakEvent, WarningsEvent, AppearanceEvent, MediaPlayEvent))
)]
struct OverlayApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    OverlayApi::openapi()
}

#[cfg(test)]
mod tests;
