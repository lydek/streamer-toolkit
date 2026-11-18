//! What a clip reference may be (thin module: simple checks, no aggregates).

/// Longest clip ID accepted, in characters.
const ID_MAX: usize = 100;
/// Most clips waiting in the queue.
pub(crate) const QUEUE_MAX: usize = 50;

/// A clip as Helix Get Clips describes it. `title` and `broadcaster_name` are third-party text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub id: String,
    pub title: String,
    pub broadcaster_name: String,
    pub duration_ms: u64,
}

/// A Twitch clip ID: letters, digits, `_` and `-` only, so it is safe in a URL query as is. Only parsing makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipId(String);

impl ClipId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn new(id: &str) -> Option<Self> {
        let valid = (1..=ID_MAX).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        valid.then(|| Self(id.to_owned()))
    }
}

/// The clip ID in a Twitch clip URL (`clips.twitch.tv/<id>`, `clips.twitch.tv/embed?clip=<id>` or
/// `twitch.tv/<channel>/clip/<id>`, with or without `https://`), or a bare clip ID. `None` for anything else.
pub(crate) fn parse_clip_id(input: &str) -> Option<ClipId> {
    let input = input.trim();
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let Some((host, path)) = rest.split_once('/') else {
        return ClipId::new(rest);
    };
    let path = path.split('#').next().unwrap_or_default();
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let id = match (host.to_ascii_lowercase().as_str(), segments.as_slice()) {
        ("clips.twitch.tv", ["embed"]) => query.split('&').find_map(|p| p.strip_prefix("clip="))?,
        ("clips.twitch.tv", [id]) => *id,
        ("twitch.tv" | "www.twitch.tv" | "m.twitch.tv", [_channel, "clip", id]) => *id,
        _ => return None,
    };
    ClipId::new(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "AwkwardHelplessSalamanderSwiftRage";
    const NEW: &str = "CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4";

    #[test]
    fn clip_urls_and_bare_ids_parse() {
        for input in [
            OLD.to_owned(),
            format!("  {NEW}\n"),
            format!("https://clips.twitch.tv/{OLD}"),
            format!("clips.twitch.tv/{NEW}/"),
            format!("https://www.twitch.tv/twitch/clip/{NEW}?filter=clips&range=all"),
            format!("http://twitch.tv/twitch/clip/{OLD}#t"),
            format!("https://m.twitch.tv/twitch/clip/{OLD}"),
            format!("https://WWW.Twitch.TV/twitch/clip/{OLD}"),
            format!("https://clips.twitch.tv/embed?clip={NEW}&parent=localhost&autoplay=true"),
            format!("https://clips.twitch.tv/embed?parent=localhost&clip={OLD}"),
        ] {
            let want = if input.contains(NEW) { NEW } else { OLD };
            let id = parse_clip_id(&input);
            assert_eq!(id.as_ref().map(ClipId::as_str), Some(want), "{input}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for input in [
            String::new(),
            "   ".into(),
            "not a clip".into(),
            "clip<script>".into(),
            "a".repeat(ID_MAX + 1),
            "clips.twitch.tv".into(),
            format!("https://example.com/{OLD}"),
            format!("https://clips.twitch.tv.example.com/{OLD}"),
            format!("https://evil@clips.twitch.tv/{OLD}"),
            format!("https://clips.twitch.tv:8080/{OLD}"),
            format!("https://clips.twitch.tv/embed/{OLD}"),
            "https://clips.twitch.tv/embed".into(),
            "https://clips.twitch.tv/embed?parent=localhost".into(),
            "https://clips.twitch.tv/embed?clip=bad%20id".into(),
            format!("https://www.twitch.tv/twitch/videos/{OLD}"),
            format!("https://www.twitch.tv/{OLD}"),
            "https://clips.twitch.tv/bad%20id".into(),
            format!("ftp://clips.twitch.tv/{OLD}"),
        ] {
            assert_eq!(parse_clip_id(&input), None, "{input}");
        }
    }
}
