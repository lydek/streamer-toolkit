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
    /// The roles the sender's badges show; other badges are left out.
    pub badges: Vec<ChatBadge>,
    /// Highlighted with channel points.
    pub highlighted: bool,
    /// Bits cheered with this message; 0 when none.
    pub bits: u32,
    /// The message this one replies to.
    pub reply: Option<ChatReply>,
    /// RFC 3339 time the message was sent.
    pub sent_at: String,
}

/// A role shown by a chat badge (a founder counts as a subscriber).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatBadge {
    Broadcaster,
    Moderator,
    Vip,
    Subscriber,
}

/// The message a reply answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReply {
    pub message_id: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
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
    /// The viewer, or for a raid the raiding channel; all three are `None` for an anonymous gift or cheer.
    pub user_id: Option<String>,
    pub user_login: Option<String>,
    /// Display name.
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

impl AlertKind {
    /// The kind's name as payloads and sound slots use it, e.g. `follow`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Subscribe { .. } => "subscribe",
            Self::Resubscribe { .. } => "resubscribe",
            Self::Gift { .. } => "gift",
            Self::Cheer { .. } => "cheer",
            Self::Raid { .. } => "raid",
        }
    }
}

/// A moderator (or bot) taking chat down; Twitch's field names stop at the integration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRemoval {
    /// One message.
    Message { id: String },
    /// Every message of one viewer (a timeout or ban clears them).
    User { user_id: String },
    /// The whole chat room.
    All,
}

/// The broadcaster's stream starting or ending; Twitch's field names stop at the integration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamChange {
    /// Twitch says a live stream started (`stream.online`).
    Online { id: String, started_at: String },
    /// Twitch says the stream ended (`stream.offline`).
    Offline,
    /// A check of what is live (Helix Get Streams): the stream with its viewers, or `None` when not live. Checks
    /// can lag behind the events by a minute or so.
    Checked(Option<LiveStream>),
    /// Signed out of Twitch: nothing says whether the stream is live any more, and nothing proves it ended.
    SignedOut,
}

/// A live stream as a check found it. `id` is Twitch's stream ID; `started_at` is RFC 3339.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveStream {
    pub id: String,
    pub started_at: String,
    pub viewers: u32,
}

/// A channel points redemption as Twitch reports it; Twitch's status strings stop at the integration layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPointsRedemption {
    /// Twitch's redemption ID.
    pub id: String,
    pub change: RedemptionChange,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub reward_id: String,
    pub reward_title: String,
    pub reward_cost: u32,
    /// What the viewer typed; empty when the reward asks for nothing.
    pub user_input: String,
    pub status: RedemptionStatus,
    /// RFC 3339 time of the redemption.
    pub redeemed_at: String,
}

/// A new redemption, or a status change of one; both carry the whole redemption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedemptionChange {
    Added,
    Updated,
}

/// `Canceled` means the points were refunded; `Unknown` is a status Twitch did not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedemptionStatus {
    Unfulfilled,
    Fulfilled,
    Canceled,
    Unknown,
}
