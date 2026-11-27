//! The song list's rules (docs/modules/songs.md): when two songs are the same, what a song and a category may hold,
//! and how an imported file becomes additions and updates.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

pub mod requests;

pub(crate) const TITLE_MAX_CHARS: usize = 100;
pub(crate) const ARTIST_MAX_CHARS: usize = 100;
pub(crate) const CATEGORY_MAX_CHARS: usize = 30;
pub(crate) const SONGS_MAX: usize = 5_000;
pub(crate) const CATEGORIES_MAX: usize = 100;

/// A song on the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    /// UUID v7.
    pub(crate) id: String,
    pub(crate) title: String,
    /// Empty: no artist.
    pub(crate) artist: String,
    pub(crate) category_id: Option<String>,
    pub(crate) youtube_url: Option<String>,
    pub(crate) last_sung_at: Option<DateTime<Utc>>,
    pub(crate) request_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) requestable: bool,
}

/// Text as compared: full-width letters, digits and signs as half-width, lower case, blanks trimmed and runs of
/// blanks as one.
pub(crate) fn normalize(text: &str) -> String {
    let half: String = text
        .chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect();
    half.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// What makes two songs the same: their title and artist, normalised.
pub(crate) fn song_key(title: &str, artist: &str) -> String {
    format!("{}\u{1F}{}", normalize(title), normalize(artist))
}

/// A YouTube video address (`youtube.com/watch?v=…` or `youtu.be/…`, with an 11-character ID).
pub(crate) fn youtube_url(text: &str) -> Option<String> {
    let url = text.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let id = if let Some(r) = rest.strip_prefix("youtu.be/") {
        r
    } else {
        let r = ["www.youtube.com/", "m.youtube.com/", "youtube.com/"]
            .iter()
            .find_map(|host| rest.strip_prefix(host))?;
        let query = r.strip_prefix("watch?")?;
        query.split('&').find_map(|kv| kv.strip_prefix("v="))?
    };
    let id: String = id
        .chars()
        .take_while(|c| !matches!(c, '?' | '&' | '#' | '/'))
        .collect();
    let ok = id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| url.to_owned())
}

/// A title, artist and link within the limits, trimmed; the reason when not.
pub(crate) fn checked_song(
    title: &str,
    artist: &str,
    youtube: &str,
) -> Result<(String, String, Option<String>), Skip> {
    let (title, artist, youtube) = (title.trim(), artist.trim(), youtube.trim());
    if title.is_empty() {
        return Err(Skip::NoTitle);
    }
    if title.chars().count() > TITLE_MAX_CHARS || artist.chars().count() > ARTIST_MAX_CHARS {
        return Err(Skip::TooLong);
    }
    let youtube = if youtube.is_empty() {
        None
    } else {
        Some(youtube_url(youtube).ok_or(Skip::BadLink)?)
    };
    Ok((title.to_owned(), artist.to_owned(), youtube))
}

/// A category name within the limit, trimmed.
pub(crate) fn category_name(text: &str) -> Option<String> {
    let name = text.trim();
    let chars = name.chars().count();
    (1..=CATEGORY_MAX_CHARS)
        .contains(&chars)
        .then(|| name.to_owned())
}

/// Why a row of an imported file was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The sheet has no title column; the whole sheet is left out.
    NoTitleColumn,
    NoTitle,
    TooLong,
    BadLink,
    /// The same song as an earlier row of the file.
    DuplicateInFile,
    /// Past the song list's limit.
    TooManySongs,
    /// A new category past the limit.
    TooManyCategories,
}

/// One sheet of an imported file (CSV: one sheet without a name), the header first.
pub struct Sheet {
    pub(crate) name: Option<String>,
    pub(crate) rows: Vec<Vec<String>>,
}

pub struct NewSong {
    pub(crate) title: String,
    pub(crate) artist: String,
    /// The category by name (an existing one, or one created by the import).
    pub(crate) category: Option<String>,
    pub(crate) youtube_url: Option<String>,
}

pub struct SongUpdate {
    pub(crate) id: String,
    /// The category by name, when the file names one different from the stored one.
    pub(crate) category: Option<String>,
    /// The link, when the file has one different from the stored one.
    pub(crate) youtube_url: Option<String>,
}

pub struct SkippedRow {
    /// The sheet's name; `None` for CSV.
    pub(crate) sheet: Option<String>,
    /// 1-based, as a spreadsheet shows it.
    pub(crate) row: usize,
    pub(crate) reason: Skip,
}

/// What an import would do.
#[derive(Default)]
pub struct ImportPlan {
    pub(crate) new_categories: Vec<String>,
    pub(crate) add: Vec<NewSong>,
    pub(crate) update: Vec<SongUpdate>,
    /// Rows naming a song already there, with nothing to change.
    pub(crate) unchanged: usize,
    pub(crate) skipped: Vec<SkippedRow>,
}

/// The columns found by the header (ignoring case): title is required.
struct Columns {
    title: usize,
    artist: Option<usize>,
    category: Option<usize>,
    link: Option<usize>,
}

fn columns(header: &[String]) -> Option<Columns> {
    let find = |names: &[&str]| {
        header
            .iter()
            .position(|h| names.contains(&normalize(h).as_str()))
    };
    Some(Columns {
        title: find(&["歌名", "title", "song"])?,
        artist: find(&["歌手", "artist", "singer"]),
        category: find(&["分類", "category"]),
        link: find(&["youtube", "連結", "link", "url"]),
    })
}

/// How `sheets` would change the list: blank cells never overwrite stored values; the same file twice gives the
/// same result.
pub(crate) fn plan_import(songs: &[Song], categories: &[Category], sheets: &[Sheet]) -> ImportPlan {
    let mut plan = ImportPlan::default();
    let by_key: HashMap<String, &Song> = songs
        .iter()
        .map(|s| (song_key(&s.title, &s.artist), s))
        .collect();
    let category_names: HashMap<&str, String> = categories
        .iter()
        .map(|c| (c.id.as_str(), normalize(&c.name)))
        .collect();
    let mut known_categories: HashSet<String> =
        categories.iter().map(|c| normalize(&c.name)).collect();
    let mut seen = HashSet::new();
    for sheet in sheets {
        let Some((header, rows)) = sheet.rows.split_first() else {
            continue;
        };
        let Some(cols) = columns(header) else {
            plan.skipped.push(SkippedRow {
                sheet: sheet.name.clone(),
                row: 1,
                reason: Skip::NoTitleColumn,
            });
            continue;
        };
        let cell = |row: &Vec<String>, i: Option<usize>| {
            i.and_then(|i| row.get(i))
                .map_or("", |v| v.trim())
                .to_owned()
        };
        for (i, row) in rows.iter().enumerate() {
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            let skip = |plan: &mut ImportPlan, reason| {
                plan.skipped.push(SkippedRow {
                    sheet: sheet.name.clone(),
                    row: i + 2,
                    reason,
                })
            };
            let checked = checked_song(
                &cell(row, Some(cols.title)),
                &cell(row, cols.artist),
                &cell(row, cols.link),
            );
            let (title, artist, youtube) = match checked {
                Ok(s) => s,
                Err(reason) => {
                    skip(&mut plan, reason);
                    continue;
                }
            };
            let named = cell(row, cols.category);
            let category = if cols.category.is_some() {
                category_name(&named)
            } else {
                sheet.name.as_deref().and_then(category_name)
            };
            let key = song_key(&title, &artist);
            if !seen.insert(key.clone()) {
                skip(&mut plan, Skip::DuplicateInFile);
                continue;
            }
            let stored = by_key.get(&key);
            // Before a new category is planned, so a skipped row leaves no empty one.
            if stored.is_none() && songs.len() + plan.add.len() >= SONGS_MAX {
                skip(&mut plan, Skip::TooManySongs);
                continue;
            }
            if let Some(c) = &category
                && !known_categories.contains(&normalize(c))
            {
                if categories.len() + plan.new_categories.len() >= CATEGORIES_MAX {
                    skip(&mut plan, Skip::TooManyCategories);
                    continue;
                }
                known_categories.insert(normalize(c));
                plan.new_categories.push(c.clone());
            }
            match stored {
                Some(song) => {
                    let stored = song
                        .category_id
                        .as_deref()
                        .and_then(|id| category_names.get(id));
                    let category = category.filter(|c| stored != Some(&normalize(c)));
                    let youtube =
                        youtube.filter(|y| song.youtube_url.as_deref() != Some(y.as_str()));
                    if category.is_none() && youtube.is_none() {
                        plan.unchanged += 1;
                    } else {
                        plan.update.push(SongUpdate {
                            id: song.id.clone(),
                            category,
                            youtube_url: youtube,
                        });
                    }
                }
                None => plan.add.push(NewSong {
                    title,
                    artist,
                    category,
                    youtube_url: youtube,
                }),
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect()
    }

    #[test]
    fn songs_are_the_same_whatever_the_width_case_and_blanks() {
        assert_eq!(
            song_key(" Ｌｅｍｏｎ ", "米津玄師"),
            song_key("lemon", "米津玄師")
        );
        assert_eq!(song_key("Shape  of You", ""), song_key("shape of you", " "));
        assert_ne!(song_key("晴天", "周杰倫"), song_key("晴天", ""));
    }

    #[test]
    fn only_youtube_video_addresses_are_links() {
        for ok in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?list=x&v=dQw4w9WgXcQ&t=10",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "http://m.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert!(youtube_url(ok).is_some(), "{ok}");
        }
        for bad in [
            "https://www.youtube.com/channel/UC123",
            "https://youtu.be/short",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "javascript:alert(1)",
            "youtu.be/dQw4w9WgXcQ",
        ] {
            assert!(youtube_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn an_import_adds_updates_and_skips_without_blanking_stored_values() {
        let categories = vec![Category {
            id: "k1".into(),
            name: "華語".into(),
            requestable: true,
        }];
        let songs = vec![
            Song {
                id: "s1".into(),
                title: "晴天".into(),
                artist: "周杰倫".into(),
                category_id: Some("k1".into()),
                youtube_url: Some("https://youtu.be/aaaaaaaaaaa".into()),
                last_sung_at: None,
                request_count: 3,
            },
            Song {
                id: "s2".into(),
                title: "小幸運".into(),
                artist: "田馥甄".into(),
                category_id: None,
                youtube_url: None,
                last_sung_at: None,
                request_count: 0,
            },
        ];
        let sheets = vec![
            Sheet {
                name: Some("日語".into()),
                rows: rows(&[
                    &["Title", "Artist", "YouTube"],
                    &["Lemon", "米津玄師", "https://youtu.be/bbbbbbbbbbb"],
                    &["", "", ""],
                    &["", "沒有歌名", ""],
                    &["lemon", "米津玄師", ""],
                    &["Bad", "", "https://example.com"],
                ]),
            },
            Sheet {
                name: Some("說明".into()),
                rows: rows(&[&["備註"], &["不是歌單"]]),
            },
            Sheet {
                name: None,
                rows: rows(&[
                    &["歌名", "歌手", "分類", "連結"],
                    // Blank link and category: nothing changes.
                    &["晴天", "周杰倫", "", ""],
                    // A category for a song without one.
                    &["小幸運", "田馥甄", "華語", ""],
                ]),
            },
        ];
        let plan = plan_import(&songs, &categories, &sheets);
        assert_eq!(plan.new_categories, ["日語"]);
        assert_eq!(plan.add.len(), 1);
        assert_eq!(plan.add[0].category.as_deref(), Some("日語"));
        assert_eq!(plan.update.len(), 1);
        assert_eq!(
            (
                plan.update[0].id.as_str(),
                plan.update[0].category.as_deref()
            ),
            ("s2", Some("華語"))
        );
        assert_eq!(plan.unchanged, 1);
        let reasons: Vec<_> = plan.skipped.iter().map(|s| (s.row, s.reason)).collect();
        assert_eq!(
            reasons,
            [
                (4, Skip::NoTitle),
                (5, Skip::DuplicateInFile),
                (6, Skip::BadLink),
                (1, Skip::NoTitleColumn)
            ]
        );
    }

    #[test]
    fn a_row_skipped_for_a_full_list_plans_no_category() {
        let songs: Vec<_> = (0..SONGS_MAX)
            .map(|i| Song {
                id: format!("s{i}"),
                title: format!("歌 {i}"),
                artist: String::new(),
                category_id: None,
                youtube_url: None,
                last_sung_at: None,
                request_count: 0,
            })
            .collect();
        let sheets = vec![Sheet {
            name: None,
            rows: rows(&[
                &["歌名", "分類"],
                &["新歌", "日語"],
                // A stored song still gets its new category.
                &["歌 1", "華語"],
            ]),
        }];
        let plan = plan_import(&songs, &[], &sheets);
        assert_eq!(plan.new_categories, ["華語"]);
        assert!(plan.add.is_empty());
        assert_eq!(plan.update.len(), 1);
        assert_eq!(
            plan.skipped
                .iter()
                .map(|s| (s.row, s.reason))
                .collect::<Vec<_>>(),
            [(2, Skip::TooManySongs)]
        );
    }
}
