//! Song requests (docs/modules/songs.md 點歌, 狀態轉換, 回覆): matching a viewer's words, the states a request
//! goes through, and filling in a reply.

use chrono::{DateTime, Utc};

use super::{Category, Song, normalize};

/// Queued requests at most.
pub(crate) const QUEUE_MAX: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Queued,
    Singing,
    Sung,
    Removed,
}

impl RequestState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Singing => "singing",
            Self::Sung => "sung",
            Self::Removed => "removed",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Queued, Self::Singing, Self::Sung, Self::Removed]
            .into_iter()
            .find(|s| s.as_str() == text)
    }

    /// The transitions the table allows; sung and removed are final.
    pub(crate) fn can_become(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Singing | Self::Removed)
                | (Self::Singing, Self::Sung | Self::Removed)
        )
    }
}

/// One request, with the song's title and artist as they were, so the history survives the song being deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// UUID v7.
    pub(crate) id: String,
    /// `None` once the song was deleted from the list.
    pub(crate) song_id: Option<String>,
    pub(crate) title: String,
    pub(crate) artist: String,
    /// Twitch user ID and display name of who it is for; the name alone when the streamer typed it, neither for
    /// nobody.
    pub(crate) viewer_id: Option<String>,
    pub(crate) viewer_name: Option<String>,
    /// Requested by the streamer on the page.
    pub(crate) by_hand: bool,
    pub(crate) state: RequestState,
    /// Order among the queued ones, smallest first.
    pub(crate) position: i64,
    pub(crate) requested_at: DateTime<Utc>,
    /// When it was sung or removed.
    pub(crate) finished_at: Option<DateTime<Utc>>,
}

impl Request {
    /// Moves to `next` if the table allows it.
    pub(crate) fn move_to(&mut self, next: RequestState, now: DateTime<Utc>) -> bool {
        if !self.state.can_become(next) {
            return false;
        }
        self.state = next;
        if matches!(next, RequestState::Sung | RequestState::Removed) {
            self.finished_at = Some(now);
        }
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Found<'a> {
    One(&'a Song),
    /// Several songs have that title: the artist tells them apart.
    Several,
    None,
}

/// The song a viewer asked for: their words, normalised, are its title, or "title artist", or "artist title".
pub(crate) fn find<'a>(songs: &'a [Song], words: &str) -> Found<'a> {
    let words = normalize(words);
    if words.is_empty() {
        return Found::None;
    }
    let hits: Vec<&Song> = songs
        .iter()
        .filter(|s| {
            let (title, artist) = (normalize(&s.title), normalize(&s.artist));
            words == title
                || (!artist.is_empty()
                    && (words == format!("{title} {artist}")
                        || words == format!("{artist} {title}")))
        })
        .collect();
    match hits.as_slice() {
        [one] => Found::One(one),
        [] => Found::None,
        _ => Found::Several,
    }
}

/// Whether viewers may request the song: it is uncategorised or its category is open.
pub(crate) fn open(song: &Song, categories: &[Category]) -> bool {
    song.category_id
        .as_ref()
        .and_then(|id| categories.iter().find(|c| &c.id == id))
        .is_none_or(|c| c.requestable)
}

/// What a request from chat came to, which picks the reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Queued, at this place among the queued ones (1 is next).
    Queued(usize),
    NotFound,
    Several,
    NotOpen,
    AlreadyQueued,
    Full,
}

/// The fixed variables of a reply.
pub(crate) struct ReplyVars<'a> {
    pub(crate) viewer: &'a str,
    pub(crate) title: &'a str,
    pub(crate) artist: &'a str,
    pub(crate) position: Option<usize>,
    pub(crate) words: &'a str,
}

/// The reply with `{觀眾}` `{歌名}` `{歌手}` `{順位}` `{輸入}` filled in, in one pass: a viewer's words or a title
/// holding a variable stay as they are.
pub(crate) fn fill(template: &str, v: &ReplyVars) -> String {
    let position = v.position.map(|p| p.to_string()).unwrap_or_default();
    let vars = [
        ("{觀眾}", v.viewer),
        ("{歌名}", v.title),
        ("{歌手}", v.artist),
        ("{順位}", position.as_str()),
        ("{輸入}", v.words),
    ];
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let at = &rest[open..];
        match vars.iter().find(|(name, _)| at.starts_with(name)) {
            Some((name, value)) => {
                out.push_str(value);
                rest = &at[name.len()..];
            }
            None => {
                out.push('{');
                rest = &at[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str, category: Option<&str>) -> Song {
        Song {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            category_id: category.map(Into::into),
            youtube_url: None,
            last_sung_at: None,
            request_count: 0,
        }
    }

    #[test]
    fn a_request_matches_the_title_or_title_and_artist_either_way() {
        let songs = [
            song("1", "晴天", "周杰倫", None),
            song("2", "Lemon", "米津玄師", None),
            song("3", "Lemon", "Other", None),
        ];
        assert_eq!(find(&songs, " 晴天 "), Found::One(&songs[0]));
        assert_eq!(find(&songs, "周杰倫　晴天"), Found::One(&songs[0]));
        assert_eq!(find(&songs, "ＬＥＭＯＮ 米津玄師"), Found::One(&songs[1]));
        assert_eq!(find(&songs, "lemon"), Found::Several);
        assert_eq!(find(&songs, "晴"), Found::None);
        assert_eq!(find(&songs, ""), Found::None);
    }

    #[test]
    fn only_open_categories_and_uncategorised_songs_can_be_requested() {
        let categories = [
            Category {
                id: "c1".into(),
                name: "華語".into(),
                requestable: true,
            },
            Category {
                id: "c2".into(),
                name: "待練".into(),
                requestable: false,
            },
        ];
        assert!(open(&song("1", "a", "", None), &categories));
        assert!(open(&song("1", "a", "", Some("c1")), &categories));
        assert!(!open(&song("1", "a", "", Some("c2")), &categories));
    }

    #[test]
    fn states_change_only_as_the_table_allows() {
        use RequestState::*;
        let all = [Queued, Singing, Sung, Removed];
        let allowed: Vec<_> = all
            .iter()
            .flat_map(|a| all.iter().map(move |b| (*a, *b)))
            .filter(|(a, b)| a.can_become(*b))
            .collect();
        assert_eq!(
            allowed,
            [
                (Queued, Singing),
                (Queued, Removed),
                (Singing, Sung),
                (Singing, Removed)
            ]
        );
        for s in all {
            assert_eq!(RequestState::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn a_reply_fills_its_variables_once_and_leaves_the_viewers_words_alone() {
        let vars = ReplyVars {
            viewer: "小魚",
            title: "{觀眾}",
            artist: "",
            position: Some(3),
            words: "{歌名} {計數:x}",
        };
        assert_eq!(
            fill(
                "{觀眾} 點了「{歌名}」第 {順位} 首；你打了 {輸入} {未知}",
                &vars
            ),
            "小魚 點了「{觀眾}」第 3 首；你打了 {歌名} {計數:x} {未知}"
        );
    }
}
