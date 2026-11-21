//! Adapters: the SQLite repository, the admin API, the outbox subscription and the `checkin.changed` screen event.

pub(crate) mod db;
pub(crate) mod http;

use std::{path::PathBuf, sync::Arc};

use chrono::{SecondsFormat, Utc};
use redemption::{REDEMPTION_RECORDED, RedemptionRecorded};
use serde::{Deserialize, Serialize};
use sound::{PlaySound, SoundService};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    commands::{CommandTable, Declared},
    events::Source,
    modules::{self, Modules},
    outbox::{Handled, OutboxEvent, Subscriber, Subscription},
    presentation::Presenter,
};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    application::{
        self, COMMAND_KEY, Card, CheckedIn, CheckinService, Config, OnCheckedIn, ProfileImages,
        Redeemed, Screen, ShowCard, Timing,
    },
    domain::{TwitchUserId, Viewer},
};

const STAMPED: &str = "checkin.stamped";
const STAMPED_VERSION: u32 = 1;

/// A `checkin.stamped` event exactly as `/ws/overlay` sends it: a stamp card to show on `/overlay/checkin/`. The
/// Host sends them one at a time. Not kept in snapshots, so a reloaded overlay never replays one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStampedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CheckinStampedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CheckinStampedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CheckinStampedType {
    #[serde(rename = "checkin.stamped")]
    CheckinStamped,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStampedPayload {
    /// Raw viewer text; overlays escape it when rendering.
    pub user_name: String,
    /// Twitch profile image (`https://static-cdn.jtvnw.net/` only); `null` when unknown.
    #[schema(required)]
    pub avatar_url: Option<String>,
    /// Stamps on the current card, out of `slots`.
    pub stamps: u32,
    pub slots: u32,
    pub total: u64,
    /// This check-in filled the card.
    pub full: bool,
    /// How long to show the card.
    pub duration_ms: u64,
    /// Played once as the card shows; `null`: silent (the default).
    #[schema(required)]
    pub sound: Option<PlaySound>,
}

fn publish_card(bus: &EventBus, sounds: &SoundService, card: &Card, show: std::time::Duration) {
    let payload = CheckinStampedPayload {
        user_name: card.name.clone(),
        avatar_url: card.avatar.clone(),
        stamps: card.stamps,
        slots: card.slots,
        total: card.total,
        full: card.full(),
        duration_ms: u64::try_from(show.as_millis()).unwrap_or(u64::MAX),
        sound: PlaySound::from_cue(sounds.checkin(card.full())),
    };
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: STAMPED,
        version: STAMPED_VERSION,
        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        source: card.source,
        payload: serde_json::to_value(payload).expect("serializable"),
    });
}

const CHANGED: &str = "checkin.changed";
const CHANGED_VERSION: u32 = 1;

/// A `checkin.changed` event exactly as `/ws/overlay` sends it: a viewer checked in. It carries only the viewer's
/// ID, so viewer text stays off this event; the admin UI reloads its lists. Not kept in snapshots.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinChangedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CheckinChangedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CheckinChangedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CheckinChangedType {
    #[serde(rename = "checkin.changed")]
    CheckinChanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CheckinChangedPayload {
    /// The viewer's ID, as in `CheckinViewer.id`.
    viewer_id: String,
}

fn publish_changed(bus: &EventBus, c: &CheckedIn) {
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: CHANGED,
        version: CHANGED_VERSION,
        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        // Simulated check-ins are never recorded, so every recorded one is real.
        source: Source::Twitch,
        payload: serde_json::to_value(CheckinChangedPayload {
            viewer_id: c.viewer_id.clone(),
        })
        .expect("serializable"),
    });
}

/// Starts check-ins into `pool`, which must already have [`db::MIGRATOR`] applied, with the settings in
/// `data_dir`. Each recorded check-in is announced on `bus` and gets a stamp card on the overlay when `presenter`
/// reaches it, with the viewer's profile image from `profiles` and the sound chosen in `sounds`; needs a Tokio
/// runtime.
#[allow(clippy::too_many_arguments)]
pub fn start(
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    data_dir: PathBuf,
    profiles: Arc<dyn ProfileImages>,
    sounds: Arc<SoundService>,
    timing: Timing,
    presenter: Arc<Presenter>,
    commands: Arc<CommandTable>,
) -> CheckinService {
    let changed_bus = bus.clone();
    let changed: OnCheckedIn = Arc::new(move |c| publish_changed(&changed_bus, c));
    let show_card: ShowCard = Arc::new(move |card, show| publish_card(&bus, &sounds, card, show));
    CheckinService::start(
        db::SqliteCheckins::new(pool),
        modules,
        Arc::new(Config::load(data_dir, commands)),
        Screen {
            changed,
            show_card,
            profiles,
            timing,
            presenter,
        },
    )
}

/// The chat commands this module answers, for the host's command table.
pub const COMMANDS: &[Declared] = &[Declared {
    key: COMMAND_KEY,
    module: modules::CHECKIN,
    default_name: "打卡",
}];

/// What this module takes from the outbox: redemptions, to check in with the configured reward.
pub fn subscriptions(pool: SqlitePool, service: &CheckinService) -> Vec<Subscription> {
    vec![Subscription {
        module: modules::CHECKIN,
        event_type: REDEMPTION_RECORDED,
        handler: Arc::new(RedemptionCheckins {
            repo: db::SqliteCheckins::new(pool),
            config: service.config().clone(),
            notify: service.on_checked_in().clone(),
        }),
    }]
}

struct RedemptionCheckins {
    repo: db::SqliteCheckins,
    config: Arc<Config>,
    notify: OnCheckedIn,
}

impl Subscriber for RedemptionCheckins {
    fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a> {
        Box::pin(async move {
            let r: RedemptionRecorded =
                serde_json::from_value(event.payload.clone()).map_err(|e| e.to_string())?;
            let redeemed = Redeemed {
                event_id: event.id.clone(),
                viewer: Viewer {
                    twitch_id: TwitchUserId(r.user_id),
                    login: r.user_login,
                    name: r.user_name,
                },
                reward_id: r.reward_id,
                occurred_at: event.occurred_at,
            };
            let rules = self.config.rules();
            let outcome = application::redeemed(&self.repo, &rules, &redeemed, Utc::now())
                .await
                .map_err(|e| e.to_string())?;
            if let Some(c) =
                outcome.and_then(|o| application::checked_in(&redeemed.viewer, &o, &rules))
            {
                (self.notify)(&c);
            }
            Ok(())
        })
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        http::checkin_settings,
        http::set_checkin_settings,
        http::list_viewers,
        http::today,
        http::manual_check_in,
        http::preview_import,
        http::apply_import
    ),
    components(schemas(CheckinChangedEvent, CheckinStampedEvent))
)]
struct CheckinApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    CheckinApi::openapi()
}

#[cfg(test)]
pub(crate) mod tests;
