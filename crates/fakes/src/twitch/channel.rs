//! Stream info: `GET`/`PATCH /helix/channels` and `GET /helix/search/categories`, as Twitch answers them.

use super::*;

/// The categories the search knows, as `(id, name)` (Twitch's real IDs).
pub const CATEGORIES: &[(&str, &str)] = &[
    ("509658", "Just Chatting"),
    ("26936", "Music"),
    ("509660", "Art"),
    ("1469308723", "Software and Game Development"),
    ("509670", "Science & Technology"),
    ("417752", "Talk Shows & Podcasts"),
    ("27471", "Minecraft"),
    ("21779", "League of Legends"),
    ("516575", "VALORANT"),
    ("33214", "Fortnite"),
    ("32982", "Grand Theft Auto V"),
    ("511224", "Apex Legends"),
    ("513181", "Genshin Impact"),
];

/// Twitch's limit on tags; it refuses more.
const TAGS_MAX: usize = 10;

/// What viewers see on the channel; the stream (`/helix/streams`) shows the same title and category.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub title: String,
    /// Empty: no category.
    pub category_id: String,
    pub tags: Vec<String>,
    pub language: String,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            title: "Fake stream".into(),
            category_id: "509658".into(),
            tags: vec!["Taiwan".into(), "中文".into()],
            language: "zh".into(),
        }
    }
}

impl Channel {
    pub fn category_name(&self) -> &'static str {
        CATEGORIES
            .iter()
            .find(|(id, _)| *id == self.category_id)
            .map_or("", |(_, name)| name)
    }
}

pub(super) async fn get_channel(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
) -> Response {
    if param(&query, "broadcaster_id") != Some(BROADCASTER_ID) {
        return Json(json!({ "data": [] })).into_response();
    }
    let c = fake.0.world.lock().unwrap().channel.clone();
    Json(json!({"data": [{
        "broadcaster_id": BROADCASTER_ID, "broadcaster_login": BROADCASTER_LOGIN,
        "broadcaster_name": BROADCASTER_NAME, "broadcaster_language": c.language,
        "game_id": c.category_id, "game_name": c.category_name(), "title": c.title, "delay": 0,
        "tags": c.tags, "content_classification_labels": [], "is_branded_content": false,
    }]}))
    .into_response()
}

/// Every field is optional; `game_id` `""` or `"0"` clears the category. Twitch answers 204.
pub(super) async fn patch_channel(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
    Json(body): Json<Value>,
) -> Response {
    if param(&query, "broadcaster_id") != Some(BROADCASTER_ID) {
        return rejected(
            StatusCode::UNAUTHORIZED,
            "The ID in broadcaster_id must match the user ID in the token.",
        );
    }
    let mut next = fake.0.world.lock().unwrap().channel.clone();
    if let Some(title) = body["title"].as_str() {
        if title.trim().is_empty() {
            return rejected(StatusCode::BAD_REQUEST, "The title may not be empty.");
        }
        next.title = title.to_owned();
    }
    if let Some(id) = body["game_id"].as_str() {
        next.category_id = match id {
            "" | "0" => String::new(),
            id if CATEGORIES.iter().any(|(c, _)| *c == id) => id.to_owned(),
            _ => return rejected(StatusCode::BAD_REQUEST, "The game_id is not valid."),
        };
    }
    if let Some(tags) = body["tags"].as_array() {
        let tags: Vec<String> = tags
            .iter()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect();
        if tags.len() > TAGS_MAX
            || tags
                .iter()
                .any(|t| t.is_empty() || t.contains(char::is_whitespace))
        {
            return rejected(StatusCode::BAD_REQUEST, "The tags are not valid.");
        }
        next.tags = tags;
    }
    if let Some(language) = body["broadcaster_language"].as_str() {
        next.language = language.to_owned();
    }
    fake.0.world.lock().unwrap().channel = next;
    StatusCode::NO_CONTENT.into_response()
}

/// Names containing the query (any case), at most `first` (default 20).
pub(super) async fn search_categories(RawQuery(query): RawQuery) -> Response {
    let needle = param(&query, "query")
        .map(decode)
        .unwrap_or_default()
        .to_lowercase();
    let first = param(&query, "first")
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    let data: Vec<Value> = CATEGORIES
        .iter()
        .filter(|(_, name)| !needle.is_empty() && name.to_lowercase().contains(&needle))
        .take(first)
        .map(|(id, name)| {
            json!({"id": id, "name": name,
                   "box_art_url": format!("https://static-cdn.jtvnw.net/ttv-boxart/{id}-52x72.jpg")})
        })
        .collect();
    Json(json!({"data": data, "pagination": {}})).into_response()
}

/// A percent-encoded query value (`+` as a space).
fn decode(text: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = bytes.get(i + 1).zip(bytes.get(i + 2));
        match (
            bytes[i],
            pair.and_then(|(h, l)| Some(hex(*h)? << 4 | hex(*l)?)),
        ) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 2;
            }
            (b'+', _) => out.push(b' '),
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(decode("Just%20Chatting"), "Just Chatting");
        assert_eq!(decode("%E9%9F%B3%E6%A8%82"), "音樂");
        assert_eq!(decode("a+b%2"), "a b%2");
    }

    #[test]
    fn the_default_channel_has_a_known_category() {
        assert_eq!(Channel::default().category_name(), "Just Chatting");
    }
}
