//! Adapters: the SQLite repository, the admin API, the outbox subscription and the `checkin.changed` screen event.

pub(crate) mod card_style;
pub(crate) mod db;
pub(crate) mod http;

use std::{path::PathBuf, sync::Arc};

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
    settings::CardStyle,
};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    application::{
        self, COMMAND_KEY, Card, CheckedIn, CheckinService, Config, HideCard, OnCheckedIn,
        ProfileImages, Redeemed, Screen, ShowCard, Timing,
    },
    domain::{TwitchUserId, Viewer, style::fill},
};
use toolkit_core::time::{self, Timestamp};

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
    /// The style's title with its variables filled in (raw text: render it as text).
    pub title: String,
    /// The style's description with its variables filled in (raw text: render it as text).
    pub description: String,
    pub style: CardLook,
}

/// How the card looks; image URLs are `/checkin/{name}`, readable without a token.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CardLook {
    /// `simple` or `journal`.
    pub preset: String,
    pub background_color: String,
    #[schema(required)]
    pub background_url: Option<String>,
    pub stamp_color: String,
    #[schema(required)]
    pub stamp_url: Option<String>,
    pub per_row: u32,
    /// `fade`, `slide`, `pop` or `none`.
    pub animation: String,
}

impl From<&CardStyle> for CardLook {
    fn from(s: &CardStyle) -> Self {
        let url = |name: &Option<String>| name.as_ref().map(|n| format!("/checkin/{n}"));
        Self {
            preset: s.preset.clone(),
            background_color: s.background_color.clone(),
            background_url: url(&s.background_image),
            stamp_color: s.stamp_color.clone(),
            stamp_url: url(&s.stamp_image),
            per_row: s.per_row,
            animation: s.animation.clone(),
        }
    }
}

fn publish_card(
    bus: &EventBus,
    sounds: &SoundService,
    style: &CardStyle,
    card: &Card,
    show: std::time::Duration,
) {
    let text = |t: &str| fill(t, &card.name, card.stamps, card.total, card.slots);
    let payload = CheckinStampedPayload {
        title: text(&style.title),
        description: text(&style.description),
        style: style.into(),
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
        occurred_at: time::millis(Timestamp::now()),
        source: card.source,
        payload: serde_json::to_value(payload).expect("serializable"),
    });
}

const HIDDEN: &str = "checkin.hidden";
const HIDDEN_VERSION: u32 = 1;

/// A `checkin.hidden` event exactly as `/ws/overlay` sends it: 隱藏畫面卡片 takes the card on screen away at once.
/// Not kept in snapshots.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinHiddenEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CheckinHiddenType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CheckinHiddenPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CheckinHiddenType {
    #[serde(rename = "checkin.hidden")]
    CheckinHidden,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CheckinHiddenPayload {}

fn publish_hidden(bus: &EventBus) {
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: HIDDEN,
        version: HIDDEN_VERSION,
        occurred_at: time::millis(Timestamp::now()),
        source: Source::System,
        payload: serde_json::to_value(CheckinHiddenPayload {}).expect("serializable"),
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
        occurred_at: time::millis(Timestamp::now()),
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
    let (changed_bus, hidden_bus) = (bus.clone(), bus.clone());
    let changed: OnCheckedIn = Arc::new(move |c| publish_changed(&changed_bus, c));
    let hide_card: HideCard = Arc::new(move || publish_hidden(&hidden_bus));
    let config = Arc::new(Config::load(data_dir, commands));
    let styled = config.clone();
    let show_card: ShowCard =
        Arc::new(move |card, show| publish_card(&bus, &sounds, &styled.style(), card, show));
    CheckinService::start(
        db::SqliteCheckins::new(pool),
        modules,
        config,
        Screen {
            changed,
            show_card,
            hide_card,
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
            repeat: service.repeat_card(),
        }),
    }]
}

struct RedemptionCheckins {
    repo: db::SqliteCheckins,
    config: Arc<Config>,
    notify: OnCheckedIn,
    repeat: OnCheckedIn,
}

impl Subscriber for RedemptionCheckins {
    fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a> {
        Box::pin(async move {
            let r: RedemptionRecorded =
                serde_json::from_value(event.payload.clone()).map_err(|e| e.to_string())?;
            let redemption = r.twitch_redemption_id.clone();
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
            let outcome = application::redeemed(&self.repo, &rules, &redeemed, Timestamp::now())
                .await
                .map_err(|e| e.to_string())?;
            if outcome == Some(application::Outcome::Repeat) && self.config.style().show_repeat {
                if let Ok(Some(c)) =
                    application::repeated(&self.repo, &rules, &redeemed.viewer).await
                {
                    (self.repeat)(&c);
                }
            } else if let Some(mut c) =
                outcome.and_then(|o| application::checked_in(&redeemed.viewer, &o, &rules))
            {
                c.redemption = Some(redemption);
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
        http::deduct_stamps,
        http::list_deductions,
        http::push_card,
        http::hide_card,
        card_style::card_style,
        card_style::set_card_style,
        card_style::import_card_image,
        card_style::test_card_style,
        http::preview_import,
        http::apply_import
    ),
    components(schemas(CheckinChangedEvent, CheckinStampedEvent, CheckinHiddenEvent))
)]
struct CheckinApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    CheckinApi::openapi()
}

#[cfg(test)]
pub(crate) mod tests;
