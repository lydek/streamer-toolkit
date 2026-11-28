//! The chat bot's ports (docs/modules/chat-bot.md): posting as the streamer through Helix, and counter values for
//! its `{計數:名稱}` variable.

use std::sync::Arc;

use bot::{BotLookups, BoxFuture, ChatSender};
use integrations::{
    tls::HttpsClient,
    twitch::{auth::TwitchAuth, helix::Helix},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
};
use utoipa::ToSchema;

use crate::admin::chat_send::{Posted, post_chat};
use toolkit_core::time::{self, Timestamp};

const SCOPE: &str = "user:write:chat";

pub(crate) struct HelixChatSender {
    pub(crate) twitch: Arc<RwLock<TwitchAuth>>,
    pub(crate) helix: String,
    pub(crate) http: HttpsClient,
}

impl ChatSender for HelixChatSender {
    fn can_send(&self) -> bool {
        // Only while a Client ID change holds the lock is it busy; then the message is not queued.
        self.twitch
            .try_read()
            .ok()
            .and_then(|t| t.token())
            .is_some_and(|t| t.scopes.iter().any(|s| s == SCOPE))
    }

    fn send(&self, text: String) -> BoxFuture<Result<String, String>> {
        let (twitch, base, http) = (self.twitch.clone(), self.helix.clone(), self.http.clone());
        Box::pin(async move {
            let twitch = twitch.read().await.clone();
            let id = twitch
                .token()
                .map(|t| t.user_id)
                .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| "twitch.sign_in_required".to_owned())?;
            let helix = Helix::new(&base, twitch, http);
            match post_chat(&helix, &id, &text).await {
                Ok(Posted::Sent(message_id)) => Ok(message_id),
                Ok(Posted::Dropped) => Err("chat.send_dropped".into()),
                Err(e) => Err(e.to_string()),
            }
        })
    }
}

pub(crate) const RETURNING: &str = "bot.returning";

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotReturningPayload {
    /// Twitch's ID of the chat message to tag.
    message_id: String,
    /// Days since the viewer was last seen before it.
    days: u64,
}

/// A `bot.returning` event exactly as `/ws/overlay` sends it (only 只提示我): the chat log and the docked chat tag
/// that message 回訪 · N 天沒來. Not kept in the snapshot: the tag is for now.
// The bus serializes its own envelope.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotReturningEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: BotReturningType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: BotReturningPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum BotReturningType {
    #[serde(rename = "bot.returning")]
    BotReturning,
}

/// Announces a 回訪 tag on the bus.
pub(crate) fn announce_returning(bus: Arc<EventBus>) -> bot::application::OnReturning {
    Arc::new(move |message_id, days| {
        let payload = BotReturningPayload { message_id, days };
        bus.publish(Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: RETURNING,
            version: 1,
            occurred_at: time::millis(Timestamp::now()),
            source: Source::Twitch,
            payload: serde_json::to_value(&payload).expect("tag serializes"),
        });
    })
}

pub(crate) struct CounterLookups(pub(crate) Arc<counter::Counters>);

impl BotLookups for CounterLookups {
    fn counter_value(&self, name: String) -> BoxFuture<Option<i64>> {
        let counters = self.0.clone();
        Box::pin(async move {
            let list = counters.list().await.ok()?;
            list.into_iter().find(|c| c.name == name).map(|c| c.value)
        })
    }
}
