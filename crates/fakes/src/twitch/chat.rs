//! `POST /helix/chat/messages`: the app sends chat as the broadcaster; Twitch posts it and sends it back through
//! EventSub like any other message.

use super::*;
use crate::events::{self, Params};

/// Twitch's limit on a chat message.
const MESSAGE_MAX_CHARS: usize = 500;

pub(super) async fn send(State(fake): State<FakeTwitch>, Json(body): Json<Value>) -> Response {
    if body["broadcaster_id"] != BROADCASTER_ID || body["sender_id"] != BROADCASTER_ID {
        return rejected(
            StatusCode::FORBIDDEN,
            "The sender must be the signed-in broadcaster.",
        );
    }
    let text = body["message"].as_str().unwrap_or_default();
    if text.trim().is_empty() || text.chars().count() > MESSAGE_MAX_CHARS {
        return rejected(
            StatusCode::BAD_REQUEST,
            "The message must be 1 to 500 characters.",
        );
    }
    let id = fake.id("fake-message");
    let said = Params {
        user: BROADCASTER_LOGIN.into(),
        message: text.into(),
        ..Params::default()
    };
    let mut event = events::event("channel.chat.message", &said, &id).expect("chat event");
    event["badges"] = json!([{"set_id": "broadcaster", "id": "1", "info": ""}]);
    fake.notify("channel.chat.message", event, Some(&id));
    Json(json!({"data": [{"message_id": id, "is_sent": true, "drop_reason": null}]}))
        .into_response()
}
