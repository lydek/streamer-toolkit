//! The Host's OpenAPI document (code-first contract). `pnpm api:gen` writes it and the TypeScript types
//! generated from it to `apps/web/src/api/`; CI fails when they are stale.

use utoipa::OpenApi;

use crate::{
    admin, backup, chat_dock, home_tiles, obs, overlay, tab_bar, templates, text_size, update,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Streamer Toolkit Host",
        description = "Local Host API on 127.0.0.1. `/ws/overlay` is a read-only WebSocket: the first message \
                       is a `Snapshot`, then events such as `ChatMessageEvent`, `AlertEvent`, `ObsStatusEvent`, \
                       `TemplateChangedEvent`, `RedemptionChangedEvent`, `CheckinChangedEvent`, `CheckinStampedEvent` and `SoundPlayEvent` follow in `seq` order."
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
        admin::checkin_rewards,
        admin::checkin_outbox,
        admin::retry_dead_letter,
        admin::skip_dead_letter,
        admin::simulate_chat,
        admin::simulate_alert,
        admin::simulate_checkin,
        backup::export_backup,
        backup::preview_restore,
        backup::confirm_restore,
        home_tiles::home_tiles,
        home_tiles::set_home_tiles,
        chat_dock::chat_dock,
        chat_dock::set_chat_dock,
        tab_bar::tab_bar,
        tab_bar::set_tab_bar,
        overlay::overlay_counts,
        text_size::text_size,
        text_size::set_text_size,
        obs::obs_overview,
        obs::update_obs,
        templates::chat_template,
        templates::select_chat_template,
        templates::alert_template,
        templates::select_alert_template,
        update::update,
        update::set_check,
        update::dismiss,
    ),
    components(schemas(
        overlay::Snapshot,
        chat::ChatMessageEvent,
        alerts::AlertEvent,
        obs::ObsStatusEvent,
        obs::ObsStateEvent,
        templates::TemplateChangedEvent,
        checkin::CheckinChangedEvent,
        checkin::CheckinStampedEvent
    )),
    tags((name = "admin", description = "Requires `Authorization: Bearer <per-launch admin token>`; \
                                         otherwise 401."))
)]
struct ApiDoc;

pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();
    doc.merge(redemption::openapi());
    doc.merge(checkin::openapi());
    doc.merge(sound::openapi());
    doc
}
