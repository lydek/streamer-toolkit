//! Maps the internal alert to the external `alert` payload.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound::{AlertSound, Cue, SoundService};
use toolkit_core::{
    bus::{Event, EventBus},
    events::{Alert, AlertKind, Source},
    modules::Modules,
    presentation::{Presenter, Shown},
};
use utoipa::ToSchema;

use crate::application::{AlertsService, Timing};

const ALERT: &str = "alert";
const ALERT_VERSION: u32 = 1;
const RECEIVED: &str = "alert.received";

impl AlertsService {
    /// Announces each alert on `bus` as `alert.received` when it arrives, and publishes it as `alert` as `presenter`
    /// paces it, with the sound each kind plays.
    pub fn new(
        bus: Arc<EventBus>,
        modules: Arc<Modules>,
        timing: Timing,
        presenter: Arc<Presenter>,
        sounds: Arc<SoundService>,
    ) -> Self {
        let (announce_bus, announce_sounds) = (bus.clone(), sounds.clone());
        Self::with_publisher(
            modules,
            timing,
            presenter,
            move |alert, source, show| {
                announce_bus.publish(Event {
                    id: alert.id.clone(),
                    event_type: RECEIVED,
                    version: ALERT_VERSION,
                    occurred_at: alert.occurred_at.clone(),
                    source,
                    payload: payload(alert.clone(), show, &announce_sounds),
                });
            },
            move |alert, source, show| {
                let missing = matches!(
                    sounds.alert(alert.kind.name()),
                    Cue::File { id, .. } if sounds.file_missing(&id)
                );
                bus.publish(Event {
                    id: alert.id.clone(),
                    event_type: ALERT,
                    version: ALERT_VERSION,
                    occurred_at: alert.occurred_at.clone(),
                    source,
                    payload: payload(alert, show, &sounds),
                });
                if missing {
                    Shown::WithoutSound
                } else {
                    Shown::Yes
                }
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlertKindName {
    Follow,
    Subscribe,
    Resubscribe,
    /// One gift of any number of subs; the subs it hands out get no alert of their own.
    Gift,
    Cheer,
    Raid,
}

impl AlertKindName {
    /// As serialized, e.g. `follow`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Subscribe => "subscribe",
            Self::Resubscribe => "resubscribe",
            Self::Gift => "gift",
            Self::Cheer => "cheer",
            Self::Raid => "raid",
        }
    }
}

/// The `payload` of an `alert` event. Fields the kind does not have are left out.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertPayload {
    pub kind: AlertKindName,
    /// The viewer, or for a raid the raiding channel; `null` for an anonymous gift or cheer.
    #[schema(required)]
    pub user_id: Option<String>,
    /// `null` for an anonymous gift or cheer.
    #[schema(required)]
    pub user_login: Option<String>,
    /// Raw viewer text; `null` for an anonymous gift or cheer. Overlays escape it when rendering.
    #[schema(required)]
    pub user_name: Option<String>,
    /// Subscription tier, 1 to 3: `subscribe`, `resubscribe`, `gift`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<u8>,
    /// Months subscribed in total: `resubscribe`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub months: Option<u32>,
    /// Subs gifted: `gift`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<u32>,
    /// Raiders: `raid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewers: Option<u32>,
    /// Raw viewer text of a `resubscribe` or `cheer`, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How long the overlay shows it; the next alert comes after this and a short pause.
    pub duration_ms: u64,
    /// Played once as the alert starts; chosen per kind in the sound settings.
    pub sound: AlertSound,
}

/// An `alert` event exactly as `/ws/overlay` sends it: the event envelope around its payload. Not kept in
/// snapshots, so a reloaded overlay never replays one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: AlertType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: AlertPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum AlertType {
    #[serde(rename = "alert")]
    Alert,
}

/// An `alert.received` event exactly as `/ws/overlay` sends it: an alert arrived, sent at once whether or not an
/// overlay will play it (the admin window lists it). Not kept in snapshots.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertReceivedEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: AlertReceivedType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: AlertPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum AlertReceivedType {
    #[serde(rename = "alert.received")]
    AlertReceived,
}

fn payload(alert: Alert, show: Duration, sounds: &SoundService) -> Value {
    let base = AlertPayload {
        duration_ms: show.as_millis() as u64,
        sound: AlertSound::BuiltIn,
        kind: AlertKindName::Follow,
        user_id: alert.user_id,
        user_login: alert.user_login,
        user_name: alert.user_name,
        tier: None,
        months: None,
        count: None,
        bits: None,
        viewers: None,
        message: None,
    };
    let text = |m: String| (!m.is_empty()).then_some(m);
    let mut payload = match alert.kind {
        AlertKind::Follow => base,
        AlertKind::Subscribe { tier, .. } => AlertPayload {
            kind: AlertKindName::Subscribe,
            tier: Some(tier),
            ..base
        },
        AlertKind::Resubscribe {
            tier,
            months,
            message,
        } => AlertPayload {
            kind: AlertKindName::Resubscribe,
            tier: Some(tier),
            months: Some(months),
            message: text(message),
            ..base
        },
        AlertKind::Gift { tier, count } => AlertPayload {
            kind: AlertKindName::Gift,
            tier: Some(tier),
            count: Some(count),
            ..base
        },
        AlertKind::Cheer { bits, message } => AlertPayload {
            kind: AlertKindName::Cheer,
            bits: Some(bits),
            message: text(message),
            ..base
        },
        AlertKind::Raid { viewers } => AlertPayload {
            kind: AlertKindName::Raid,
            viewers: Some(viewers),
            ..base
        },
    };
    payload.sound = sounds.alert(payload.kind.name()).into();
    serde_json::to_value(payload).expect("alert payload serializes")
}
