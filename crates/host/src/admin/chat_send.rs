//! Sending a chat message as the streamer (Helix Send Chat Message, `user:write:chat`).

use super::*;

/// Twitch's limit for one chat message.
const MESSAGE_MAX_CHARS: usize = 500;

#[derive(Deserialize, ToSchema)]
pub(crate) struct ChatSendBody {
    text: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ChatSent {
    /// Twitch's ID of the message, as chat events carry it.
    message_id: String,
}

#[derive(Deserialize)]
struct SendList {
    data: Vec<SendResult>,
}

#[derive(Deserialize)]
struct SendResult {
    message_id: String,
    is_sent: bool,
}

#[utoipa::path(
    post,
    path = "/api/admin/chat/send",
    tag = "admin",
    request_body = ChatSendBody,
    responses(
        (status = 200, body = ChatSent),
        (status = 400, description = "`chat.send_invalid` (empty or over 500 characters), `chat.send_refused` (Twitch refused the message), or not signed in", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`: the sign-in lacks `user:write:chat`", body = ErrorBody),
        (status = 422, description = "`chat.send_dropped`: Twitch took the message but did not send it (e.g. a duplicate or AutoMod)", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn chat_send(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<ChatSendBody>,
) -> Response {
    let text = body.text.trim();
    if text.is_empty() || text.chars().count() > MESSAGE_MAX_CHARS {
        return error(StatusCode::BAD_REQUEST, "chat.send_invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let Some(id) = twitch
        .token()
        .map(|t| t.user_id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let request = serde_json::json!({ "broadcaster_id": id, "sender_id": id, "message": text });
    match helix.post_json_read("/chat/messages", &request).await {
        Ok(body) => match serde_json::from_value::<SendList>(body)
            .ok()
            .and_then(|l| l.data.into_iter().next())
        {
            Some(r) if r.is_sent => Json(ChatSent {
                message_id: r.message_id,
            })
            .into_response(),
            Some(_) => error(StatusCode::UNPROCESSABLE_ENTITY, "chat.send_dropped"),
            None => auth_error(AuthError::InvalidResponse),
        },
        Err(HelixError::Auth(e)) => auth_error(e),
        Err(HelixError::Refused(401 | 403)) => {
            error(StatusCode::FORBIDDEN, "twitch.scope_required")
        }
        Err(HelixError::Refused(400)) => error(StatusCode::BAD_REQUEST, "chat.send_refused"),
        Err(e @ (HelixError::RateLimited(_) | HelixError::Refused(_))) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}
