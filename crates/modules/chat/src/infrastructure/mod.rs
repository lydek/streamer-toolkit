//! Maps the internal chat message to the external `chat.message` payload.

use serde_json::{Value, json};
use toolkit_core::events::{ChatMessage, Fragment};

pub(crate) const CHAT_MESSAGE: &str = "chat.message";
pub(crate) const CHAT_MESSAGE_VERSION: u32 = 1;

pub(crate) fn payload(m: ChatMessage) -> Value {
    let fragments: Vec<Value> = m
        .fragments
        .into_iter()
        .map(|f| match f {
            Fragment::Text(text) => json!({ "type": "text", "text": text }),
            Fragment::Emote { id, text } => json!({ "type": "emote", "text": text, "emoteId": id }),
        })
        .collect();
    json!({
        "userId": m.user_id,
        "userLogin": m.user_login,
        "userName": m.user_name,
        "color": m.color,
        "text": m.text,
        "fragments": fragments,
    })
}
