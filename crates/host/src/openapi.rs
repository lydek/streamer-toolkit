//! The Host's OpenAPI document (code-first contract). `pnpm api:gen` writes it and the TypeScript types
//! generated from it to `apps/web/src/api/`; CI fails when they are stale.

use utoipa::OpenApi;

use crate::{
    admin, after_numbers, backup, card_packs, chat_display, chat_dock, chat_log, diagnostics,
    event_feed, home_tiles, image_sizes, leaderboard, mixer, obs, overlay, overlay_test,
    performance, queue, rehearsal, services, source_settings, stream_summary, tab_bar, templates,
    text_size, update,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Streamer Toolkit Host",
        description = "Local Host API on 127.0.0.1. `/ws/overlay` is a read-only WebSocket: the first message \
                       is a `Snapshot`, then events such as `ChatMessageEvent`, `AlertEvent`, `ObsStatusEvent`, \
                       `TemplateChangedEvent`, `RedemptionChangedEvent`, `CheckinChangedEvent`, `CheckinStampedEvent`, `SoundPlayEvent` and `SoundStoppedEvent` follow in `seq` order."
    ),
    paths(
        admin::twitch_account::twitch_overview,
        admin::twitch_account::set_client_id,
        admin::twitch_account::login,
        admin::twitch_account::logout,
        admin::general_settings::general,
        admin::general_settings::set_general,
        admin::general_settings::first_run,
        admin::general_settings::set_first_run,
        admin::general_settings::list_modules,
        admin::general_settings::set_module,
        admin::checkin_admin::checkin_rewards,
        admin::stream_info::stream_info,
        admin::stream_info::set_stream_info,
        admin::stream_info::search_categories,
        admin::chat_send::chat_send,
        admin::rewards::list_rewards,
        admin::rewards::update_reward,
        admin::rewards::create_reward,
        admin::rewards::delete_reward,
        admin::rewards::take_over_reward,
        admin::rewards::set_redemption_status,
        admin::polls::current_polls,
        admin::polls::start_poll,
        admin::polls::end_poll,
        admin::polls::start_prediction,
        admin::polls::end_prediction,
        admin::polls::poll_templates,
        admin::polls::set_poll_templates,
        admin::checkin_admin::checkin_outbox,
        admin::checkin_admin::retry_dead_letter,
        admin::checkin_admin::skip_dead_letter,
        admin::simulate::simulate_chat,
        admin::simulate::simulate_alert,
        admin::simulate::simulate_checkin,
        overlay_test::test_overlay,
        backup::export_backup,
        backup::sizes,
        backup::upload::start,
        backup::upload::part,
        backup::upload::check,
        backup::upload::cancel,
        backup::confirm_restore,
        home_tiles::home_tiles,
        home_tiles::set_home_tiles,
        chat_dock::chat_dock,
        chat_dock::set_chat_dock,
        after_numbers::after_numbers,
        after_numbers::set_after_numbers,
        mixer::mic,
        mixer::set_mic,
        services::services,
        services::set_services,
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
        stream_summary::stream_record,
        event_feed::event_feed,
        text_size::text_size,
        text_size::set_text_size,
        diagnostics::connections,
        diagnostics::logs,
        diagnostics::report,
        image_sizes::make_sizes,
        performance::performance,
        leaderboard::leaderboard,
        leaderboard::bots,
        rehearsal::run_script,
        rehearsal::stop_script,
        card_packs::list_packs,
        card_packs::upload_pack,
        card_packs::delete_pack,
        obs::obs_overview,
        obs::update_obs,
        obs::switch_scene,
        obs::set_studio,
        obs::set_preview_scene,
        obs::transition_now,
        obs::update_item,
        obs::scene_placements,
        obs::item_transform,
        obs::update_input,
        obs::input_meters,
        source_settings::source,
        source_settings::set_source,
        source_settings::refresh_source,
        source_settings::source_options,
        obs::overlay_sources,
        obs::add_overlay,
        obs::remove_overlay,
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
        crate::bot_ports::BotReturningEvent,
        crate::song_events::SongsChangedEvent,
        chat::ChatMessageEvent,
        alerts::AlertEvent,
        alerts::AlertReceivedEvent,
        obs::ObsStatusEvent,
        obs::ObsStateEvent,
        queue::PresentationQueueEvent,
        queue::PresentationStoppedEvent,
        queue::PresentationSkippedEvent,
        overlay_test::CounterTestEvent,
        overlay_test::MarathonTestEvent,
        stream_summary::StreamSummary,
        chat_display::BadgeImage,
        toolkit_core::settings::ChatDisplay,
        toolkit_core::settings::ChatStyle,
        chat_log::ChatPeriod,
        chat_log::ChatLogBody,
        image_sizes::ImageKind,
        image_sizes::Fit,
        templates::TemplateChangedEvent,
        templates::ChatTextSizeEvent,
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
    doc.merge(counter::openapi());
    doc.merge(bot::openapi());
    doc.merge(songs::openapi());
    doc.merge(overlays::openapi());
    doc.merge(sound::openapi());
    doc.merge(bgm::openapi());
    doc.merge(stream::openapi());
    doc.merge(activity::openapi());
    doc.merge(alerts::openapi());
    doc.merge(chat::openapi());
    doc
}
