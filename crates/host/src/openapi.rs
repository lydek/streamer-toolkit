//! The Host's OpenAPI document (code-first contract). `pnpm api:gen` writes it and the TypeScript types
//! generated from it to `apps/web/src/api/`; CI fails when they are stale.

use utoipa::OpenApi;

use crate::{admin, home_tiles, obs, overlay, templates};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Streamer Toolkit Host",
        description = "Local Host API on 127.0.0.1. `/ws/overlay` is a read-only WebSocket: the first message \
                       is a `Snapshot`, then events such as `ChatMessageEvent`, `AlertEvent`, `ObsStatusEvent` and \
                       `TemplateChangedEvent` follow in `seq` order."
    ),
    paths(
        admin::twitch_overview,
        admin::set_client_id,
        admin::login,
        admin::logout,
        admin::general,
        admin::set_general,
        admin::list_modules,
        admin::set_module,
        admin::simulate_chat,
        admin::simulate_alert,
        home_tiles::home_tiles,
        home_tiles::set_home_tiles,
        obs::obs_overview,
        obs::update_obs,
        templates::chat_template,
        templates::select_chat_template,
        templates::alert_template,
        templates::select_alert_template,
    ),
    components(schemas(
        overlay::Snapshot,
        chat::ChatMessageEvent,
        alerts::AlertEvent,
        obs::ObsStatusEvent,
        templates::TemplateChangedEvent
    )),
    tags((name = "admin", description = "Requires `Authorization: Bearer <per-launch admin token>`; \
                                         otherwise 401."))
)]
struct ApiDoc;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}
