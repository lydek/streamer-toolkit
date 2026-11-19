//! A sighting of a viewer: who, and their roles when the event shows them.

use toolkit_core::events::{Alert, ChannelPointsRedemption, ChatBadge, ChatMessage};

/// The roles a chat message's badges show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Roles {
    pub(crate) broadcaster: bool,
    pub(crate) moderator: bool,
    pub(crate) vip: bool,
    pub(crate) subscriber: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sighting {
    pub(crate) user_id: String,
    pub(crate) login: String,
    pub(crate) name: String,
    /// Only chat shows roles; other sightings keep the stored ones.
    pub(crate) roles: Option<Roles>,
}

impl Sighting {
    pub(crate) fn from_chat(m: &ChatMessage) -> Self {
        let has = |b| m.badges.contains(&b);
        Self {
            user_id: m.user_id.clone(),
            login: m.user_login.clone(),
            name: m.user_name.clone(),
            roles: Some(Roles {
                broadcaster: has(ChatBadge::Broadcaster),
                moderator: has(ChatBadge::Moderator),
                vip: has(ChatBadge::Vip),
                subscriber: has(ChatBadge::Subscriber),
            }),
        }
    }

    /// `None` for an anonymous gift or cheer.
    pub(crate) fn from_alert(a: &Alert) -> Option<Self> {
        Some(Self {
            user_id: a.user_id.clone()?,
            login: a.user_login.clone()?,
            name: a.user_name.clone()?,
            roles: None,
        })
    }

    pub(crate) fn from_redemption(r: &ChannelPointsRedemption) -> Self {
        Self {
            user_id: r.user_id.clone(),
            login: r.user_login.clone(),
            name: r.user_name.clone(),
            roles: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolkit_core::events::AlertKind;

    #[test]
    fn only_chat_carries_roles_and_anonymous_alerts_are_nobody() {
        let chat = ChatMessage {
            id: "c".into(),
            user_id: "7".into(),
            user_login: "fan".into(),
            user_name: "Fan".into(),
            color: None,
            text: "hi".into(),
            fragments: vec![],
            badges: vec![ChatBadge::Vip, ChatBadge::Subscriber],
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: "2026-11-19T10:00:00Z".into(),
        };
        assert_eq!(
            Sighting::from_chat(&chat).roles,
            Some(Roles {
                vip: true,
                subscriber: true,
                ..Roles::default()
            })
        );
        let anonymous = Alert {
            id: "a".into(),
            user_id: None,
            user_login: None,
            user_name: None,
            kind: AlertKind::Gift { tier: 1, count: 5 },
            occurred_at: "2026-11-19T10:00:00Z".into(),
        };
        assert_eq!(Sighting::from_alert(&anonymous), None);
    }
}
