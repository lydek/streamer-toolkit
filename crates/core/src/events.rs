//! Internal events produced by integrations (or the simulator) and consumed by modules.

use serde::{Deserialize, Serialize};

/// Where an event came from. Simulated events never go into real business records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Twitch,
    Obs,
    Simulation,
    /// The toolkit itself, e.g. a template file changed.
    System,
}

/// A chat message; Twitch's field names and fragment types stop at the integration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    /// `#RRGGBB`, or `None` when the user never picked a color (or the source sent something else).
    pub color: Option<String>,
    pub text: String,
    pub fragments: Vec<Fragment>,
    /// RFC 3339 time the message was sent.
    pub sent_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fragment {
    Text(String),
    Emote { id: String, text: String },
}

/// A follow, subscription, cheer or raid; Twitch's field names stop at the integration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub id: String,
    /// Display name; `None` for an anonymous gift or cheer.
    pub user_name: Option<String>,
    pub kind: AlertKind,
    /// RFC 3339 time Twitch sent it.
    pub occurred_at: String,
}

/// `tier` is 1, 2 or 3 (Prime counts as 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertKind {
    Follow,
    /// Twitch also sends one per gifted sub (`gifted`); the gift itself is a separate [`AlertKind::Gift`].
    Subscribe {
        tier: u8,
        gifted: bool,
    },
    Resubscribe {
        tier: u8,
        months: u32,
        message: String,
    },
    Gift {
        tier: u8,
        count: u32,
    },
    Cheer {
        bits: u32,
        message: String,
    },
    Raid {
        viewers: u32,
    },
}
