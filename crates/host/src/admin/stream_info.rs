//! 直播資訊: the title, category, tags and language viewers see on Twitch, read and written through Helix with the
//! broadcaster's sign-in (`channel:manage:broadcast` to write).

use super::*;
use axum::extract::Query;
use utoipa::IntoParams;

/// Twitch's own limits; it checks the rest (e.g. what a tag may contain) and refuses the request.
const TITLE_MAX_CHARS: usize = 140;
const TAGS_MAX: usize = 10;
const TAG_MAX_CHARS: usize = 25;

#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct Category {
    id: String,
    name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct StreamInfo {
    title: String,
    /// `None`: no category set.
    category: Option<Category>,
    tags: Vec<String>,
    /// ISO 639-1, e.g. `zh`; `other` when Twitch has none for it.
    language: String,
}

#[derive(Deserialize)]
struct ChannelList {
    data: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
    title: String,
    game_id: String,
    game_name: String,
    #[serde(default)]
    tags: Vec<String>,
    broadcaster_language: String,
}

#[derive(Deserialize)]
struct CategoryList {
    data: Vec<Category>,
}

/// The signed-in broadcaster's numeric ID; `None` while signed out.
fn broadcaster(twitch: &TwitchAuth) -> Option<String> {
    twitch
        .token()
        .map(|t| t.user_id)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

fn helix_error(e: HelixError) -> Response {
    match e {
        HelixError::Auth(e) => auth_error(e),
        // 401/403 after a refresh: the sign-in lacks the scope (or the channel is not the broadcaster's).
        HelixError::Refused(401 | 403) => error(StatusCode::FORBIDDEN, "twitch.scope_required"),
        HelixError::Refused(400) => error(StatusCode::BAD_REQUEST, "stream_info.refused"),
        e @ (HelixError::RateLimited(_) | HelixError::Refused(_)) => {
            error(StatusCode::BAD_GATEWAY, &e.to_string())
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/stream-info",
    tag = "admin",
    responses(
        (status = 200, body = StreamInfo),
        (status = 400, description = "Not signed in", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn stream_info(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    let Some(id) = broadcaster(&twitch) else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let list = match helix
        .get_json(&format!("/channels?broadcaster_id={id}"))
        .await
    {
        Ok(body) => serde_json::from_value::<ChannelList>(body),
        Err(e) => return helix_error(e),
    };
    match list.ok().and_then(|l| l.data.into_iter().next()) {
        Some(c) => Json(StreamInfo {
            title: c.title,
            category: (!c.game_id.is_empty()).then_some(Category {
                id: c.game_id,
                name: c.game_name,
            }),
            tags: c.tags,
            language: c.broadcaster_language,
        })
        .into_response(),
        None => auth_error(AuthError::InvalidResponse),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct StreamInfoBody {
    title: String,
    /// Empty: no category.
    category_id: String,
    tags: Vec<String>,
    language: String,
}

#[utoipa::path(
    put,
    path = "/api/admin/stream-info",
    tag = "admin",
    request_body = StreamInfoBody,
    responses(
        (status = 204),
        (status = 400, description = "`stream_info.invalid` (over Twitch's limits, checked here) or `stream_info.refused` (Twitch refused the content)", body = ErrorBody),
        (status = 403, description = "`twitch.scope_required`: the sign-in lacks `channel:manage:broadcast`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn set_stream_info(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<StreamInfoBody>,
) -> Response {
    let tags_ok = body.tags.len() <= TAGS_MAX
        && body
            .tags
            .iter()
            .all(|t| !t.trim().is_empty() && t.chars().count() <= TAG_MAX_CHARS);
    let category_ok = body.category_id.bytes().all(|b| b.is_ascii_digit());
    let language_ok = !body.language.is_empty()
        && body.language.len() <= 8
        && body.language.bytes().all(|b| b.is_ascii_alphabetic());
    if body.title.trim().is_empty()
        || body.title.chars().count() > TITLE_MAX_CHARS
        || !tags_ok
        || !category_ok
        || !language_ok
    {
        return error(StatusCode::BAD_REQUEST, "stream_info.invalid");
    }
    let twitch = s.twitch.read().await.clone();
    let Some(id) = broadcaster(&twitch) else {
        return auth_error(AuthError::SignInRequired);
    };
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let patch = serde_json::json!({
        "title": body.title,
        "game_id": body.category_id,
        "tags": body.tags,
        "broadcaster_language": body.language,
    });
    match helix
        .patch_json(&format!("/channels?broadcaster_id={id}"), &patch)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => helix_error(e),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CategoryQuery {
    query: String,
}

#[utoipa::path(
    get,
    path = "/api/admin/stream-info/categories",
    tag = "admin",
    params(CategoryQuery),
    responses(
        (status = 200, description = "Up to 20 categories matching the query", body = Vec<Category>),
        (status = 400, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn search_categories(
    State(s): State<Arc<AdminState>>,
    Query(q): Query<CategoryQuery>,
) -> Response {
    let query = q.query.trim();
    if query.is_empty() || query.chars().count() > 100 {
        return Json(Vec::<Category>::new()).into_response();
    }
    let twitch = s.twitch.read().await.clone();
    if broadcaster(&twitch).is_none() {
        return auth_error(AuthError::SignInRequired);
    }
    let helix = Helix::new(&s.endpoints.helix, twitch, s.http.clone());
    let path = format!("/search/categories?first=20&query={}", urlencode(query));
    match helix.get_json(&path).await {
        Ok(body) => match serde_json::from_value::<CategoryList>(body) {
            Ok(list) => Json(list.data).into_response(),
            Err(_) => auth_error(AuthError::InvalidResponse),
        },
        Err(e) => helix_error(e),
    }
}

/// Percent-encodes everything but unreserved characters (RFC 3986), for one query value.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::urlencode;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(urlencode("Just Chatting"), "Just%20Chatting");
        assert_eq!(urlencode("音樂&a=b"), "%E9%9F%B3%E6%A8%82%26a%3Db");
    }
}
