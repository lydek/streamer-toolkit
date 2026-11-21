//! The Host's OpenAPI document (code-first contract). `pnpm api:gen` writes it and the TypeScript types
//! generated from it to `apps/web/src/api/`; CI fails when they are stale.

use utoipa::OpenApi;

use crate::{
    admin, backup, chat_display, chat_dock, chat_log, home_tiles, obs, overlay, queue,
    stream_summary, tab_bar, templates, text_size, update,
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
        admin::twitch_account::twitch_overview,
        admin::twitch_account::set_client_id,
        admin::twitch_account::login,
        admin::twitch_account::logout,
        admin::general_settings::general,
        admin::general_settings::set_general,
        admin::general_settings::list_modules,
        admin::general_settings::set_module,
        admin::checkin_admin::checkin_rewards,
        admin::checkin_admin::checkin_outbox,
        admin::checkin_admin::retry_dead_letter,
        admin::checkin_admin::skip_dead_letter,
        admin::simulate::simulate_chat,
        admin::simulate::simulate_alert,
        admin::simulate::simulate_checkin,
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
        chat_display::chat_display,
        chat_display::set_chat_display,
        chat_display::badges,
        chat_display::avatars,
        chat_log::periods,
        chat_log::log,
        queue::skip_queue,
        queue::clear_queue,
        queue::stop_queue,
        stream_summary::stream_summary,
        text_size::text_size,
        text_size::set_text_size,
        obs::obs_overview,
        obs::update_obs,
        obs::switch_scene,
        obs::update_item,
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
        alerts::AlertReceivedEvent,
        obs::ObsStatusEvent,
        obs::ObsStateEvent,
        queue::PresentationQueueEvent,
        queue::PresentationStoppedEvent,
        queue::PresentationSkippedEvent,
        stream_summary::StreamSummary,
        chat_display::BadgeImage,
        toolkit_core::settings::ChatDisplay,
        toolkit_core::settings::ChatStyle,
        chat_log::ChatPeriod,
        chat_log::ChatLogBody,
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
    doc.merge(stream::openapi());
    doc.merge(activity::openapi());
    doc.merge(alerts::openapi());
    doc.merge(chat::openapi());
    doc
}
