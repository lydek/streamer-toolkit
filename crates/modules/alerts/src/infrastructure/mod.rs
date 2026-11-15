//! Maps the internal alert to the external `alert` payload.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toolkit_core::events::{Alert, AlertKind, Source};
use utoipa::ToSchema;

pub(crate) const ALERT: &str = "alert";
pub(crate) const ALERT_VERSION: u32 = 1;

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

/// The `payload` of an `alert` event. Fields the kind does not have are left out.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertPayload {
    pub kind: AlertKindName,
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

pub(crate) fn payload(alert: Alert, show: Duration) -> Value {
    let base = AlertPayload {
        duration_ms: show.as_millis() as u64,
        kind: AlertKindName::Follow,
        user_name: alert.user_name,
        tier: None,
        months: None,
        count: None,
        bits: None,
        viewers: None,
        message: None,
    };
    let text = |m: String| (!m.is_empty()).then_some(m);
    let payload = match alert.kind {
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
    serde_json::to_value(payload).expect("alert payload serializes")
}
