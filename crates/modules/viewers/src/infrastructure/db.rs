//! Viewers in SQLite (`viewers`).

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqliteConnection, SqlitePool, migrate::Migrator};

use crate::{
    application::{Recorded, RepositoryError, ViewerRepository},
    domain::Sighting,
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub(crate) struct SqliteViewers {
    pool: SqlitePool,
}

impl SqliteViewers {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

impl ViewerRepository for SqliteViewers {
    async fn record(
        &self,
        sightings: &[(Sighting, DateTime<Utc>)],
    ) -> Result<Vec<Recorded>, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let mut results = Vec::with_capacity(sightings.len());
        for (sighting, at) in sightings {
            results.push(record_sighting(&mut tx, sighting, *at).await?);
        }
        tx.commit().await.map_err(storage)?;
        Ok(results)
    }

    async fn set_avatar(&self, user_id: &str, url: &str) -> Result<(), RepositoryError> {
        sqlx::query("UPDATE viewers SET avatar_url = ? WHERE twitch_user_id = ?")
            .bind(url)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }
}

async fn record_sighting(
    conn: &mut SqliteConnection,
    s: &Sighting,
    at: DateTime<Utc>,
) -> Result<Recorded, RepositoryError> {
    let at = at.to_rfc3339_opts(SecondsFormat::Micros, true);
    let role = |f: fn(&crate::domain::Roles) -> bool| s.roles.as_ref().map(f);
    // A NULL role (not shown by this event) keeps the stored one; a new viewer starts without roles.
    // A new row's first sighting is this one; an existing row keeps its earlier first sighting.
    // The caller holds a transaction. Two sightings can have the same timestamp.
    let existed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM viewers WHERE twitch_user_id = ?)")
            .bind(&s.user_id)
            .fetch_one(&mut *conn)
            .await
            .map_err(storage)?;
    let no_avatar: bool = sqlx::query_scalar(
            "INSERT INTO viewers (twitch_user_id, login, display_name, broadcaster, moderator, vip, subscriber, \
             first_seen_at, last_seen_at) \
             VALUES (?1, ?2, ?3, COALESCE(?4, 0), COALESCE(?5, 0), COALESCE(?6, 0), COALESCE(?7, 0), ?8, ?8) \
             ON CONFLICT (twitch_user_id) DO UPDATE SET login = excluded.login, \
             display_name = excluded.display_name, last_seen_at = excluded.last_seen_at, \
             broadcaster = COALESCE(?4, broadcaster), moderator = COALESCE(?5, moderator), \
             vip = COALESCE(?6, vip), subscriber = COALESCE(?7, subscriber) \
             RETURNING avatar_url IS NULL",
        )
        .bind(&s.user_id)
        .bind(&s.login)
        .bind(&s.name)
        .bind(role(|r| r.broadcaster))
        .bind(role(|r| r.moderator))
        .bind(role(|r| r.vip))
        .bind(role(|r| r.subscriber))
        .bind(at)
        .fetch_one(conn)
        .await
        .map_err(storage)?;
    Ok(Recorded {
        no_avatar,
        new: !existed,
    })
}

/// The stored avatars of `ids` (Twitch user IDs); viewers without one are left out.
pub async fn avatars(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<std::collections::HashMap<String, String>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let mut q = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT twitch_user_id, avatar_url FROM viewers WHERE avatar_url IS NOT NULL AND twitch_user_id IN (",
    );
    let mut list = q.separated(", ");
    for id in ids {
        list.push_bind(id);
    }
    q.push(")");
    let rows: Vec<(String, String)> = q.build_query_as().fetch_all(pool).await?;
    Ok(rows.into_iter().collect())
}

/// A viewer's names, for other modules to show next to their own records (they never read this table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerName {
    pub twitch_user_id: String,
    pub login: String,
    pub display_name: String,
}

/// At most `limit` viewers whose login or display name contains `query` (ASCII letters ignoring case), most
/// recently seen first; nothing for a blank query.
pub async fn search(
    pool: &SqlitePool,
    query: &str,
    limit: u32,
) -> Result<Vec<ViewerName>, sqlx::Error> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // The query is matched literally: `%`, `_` and the escape character `!` lose their LIKE meaning.
    let pattern = format!(
        "%{}%",
        query
            .replace('!', "!!")
            .replace('%', "!%")
            .replace('_', "!_")
    );
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT twitch_user_id, login, display_name FROM viewers \
         WHERE login LIKE ?1 ESCAPE '!' OR display_name LIKE ?1 ESCAPE '!' \
         ORDER BY last_seen_at DESC LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(name).collect())
}

/// The names of `ids` (Twitch user IDs) by ID; IDs not in the directory are left out.
pub async fn names(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<std::collections::HashMap<String, ViewerName>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let mut q = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT twitch_user_id, login, display_name FROM viewers WHERE twitch_user_id IN (",
    );
    let mut list = q.separated(", ");
    for id in ids {
        list.push_bind(id);
    }
    q.push(")");
    let rows: Vec<(String, String, String)> = q.build_query_as().fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| (r.0.clone(), name(r))).collect())
}

fn name((twitch_user_id, login, display_name): (String, String, String)) -> ViewerName {
    ViewerName {
        twitch_user_id,
        login,
        display_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::start;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use toolkit_core::{
        events::{Alert, AlertKind, ChatBadge, ChatMessage, Source},
        twitch::{ProfileBatch, ProfileImages},
    };

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        pool
    }

    /// Answers every lookup with `url` and counts them.
    struct Fixed {
        url: &'static str,
        asked: Mutex<Vec<String>>,
    }

    impl ProfileImages for Fixed {
        fn profile_image<'a>(
            &'a self,
            id: &'a str,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
            self.asked.lock().unwrap().push(id.to_owned());
            Box::pin(async move { Some(self.url.to_owned()) })
        }
    }

    #[tokio::test]
    async fn viewers_are_found_by_login_or_display_name_and_named_by_id() {
        let pool = pool().await;
        for (id, login, name, seen) in [
            ("1", "fish", "小魚", "2026-11-21T01:00:00.000000Z"),
            ("2", "berry", "莓莓", "2026-11-21T03:00:00.000000Z"),
            ("3", "fisher_kai", "阿凱", "2026-11-21T02:00:00.000000Z"),
            ("4", "a%b", "100%", "2026-11-21T00:00:00.000000Z"),
        ] {
            sqlx::query(
                "INSERT INTO viewers (twitch_user_id, login, display_name, first_seen_at, last_seen_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(login)
            .bind(name)
            .bind(seen)
            .bind(seen)
            .execute(&pool)
            .await
            .unwrap();
        }
        let ids = |found: Vec<ViewerName>| {
            found
                .into_iter()
                .map(|v| v.twitch_user_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(search(&pool, " FISH ", 20).await.unwrap()),
            ["3", "1"],
            "latest seen first"
        );
        assert_eq!(ids(search(&pool, "莓", 20).await.unwrap()), ["2"]);
        assert_eq!(ids(search(&pool, "fish", 1).await.unwrap()), ["3"]);
        assert_eq!(
            ids(search(&pool, "%", 20).await.unwrap()),
            ["4"],
            "matched literally"
        );
        assert_eq!(ids(search(&pool, "_", 20).await.unwrap()), ["3"]);
        assert!(search(&pool, "  ", 20).await.unwrap().is_empty());

        let found = names(&pool, &["2".into(), "9".into()]).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found["2"].login.as_str(), found["2"].display_name.as_str()),
            ("berry", "莓莓")
        );
    }

    type Row = (String, String, Option<String>, bool, bool, String, String);

    async fn rows(pool: &SqlitePool) -> Vec<Row> {
        sqlx::query_as(
            "SELECT login, display_name, avatar_url, moderator, subscriber, first_seen_at, last_seen_at \
             FROM viewers ORDER BY twitch_user_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn settle(pool: &SqlitePool, done: impl Fn(&[Row]) -> bool) -> Vec<Row> {
        for _ in 0..100 {
            let now = rows(pool).await;
            if done(&now) {
                return now;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("never settled: {:?}", rows(pool).await);
    }

    fn chat(login: &str, name: &str, badges: Vec<ChatBadge>) -> ChatMessage {
        ChatMessage {
            id: "c".into(),
            broadcaster_id: String::new(),
            user_id: "7".into(),
            user_login: login.into(),
            user_name: name.into(),
            color: None,
            text: "hi".into(),
            fragments: vec![],
            badges,
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: "2026-11-19T10:00:00Z".into(),
        }
    }

    const AVATAR: &str =
        "https://static-cdn.jtvnw.net/jtv_user_pictures/x-profile_image-300x300.png";

    #[derive(Default)]
    struct BatchProfiles(Mutex<Vec<Vec<String>>>);

    impl ProfileImages for BatchProfiles {
        fn profile_image<'a>(
            &'a self,
            _: &'a str,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
            panic!("viewer lookups must be batched")
        }

        fn profile_images<'a>(
            &'a self,
            ids: &'a [String],
        ) -> Pin<Box<dyn Future<Output = ProfileBatch> + Send + 'a>> {
            Box::pin(async move {
                let mut calls = self.0.lock().unwrap();
                calls.push(ids.to_vec());
                if calls.len() == 1 {
                    return Err(Duration::from_millis(10));
                }
                Ok(ids.iter().map(|id| (id.clone(), AVATAR.into())).collect())
            })
        }
    }

    #[tokio::test]
    async fn avatar_lookups_batch_distinct_viewers_and_retry_after_a_temporary_failure() {
        let pool = pool().await;
        let profiles = Arc::new(BatchProfiles::default());
        let viewers = start(pool.clone(), profiles.clone());
        for id in ["7", "7", "8"] {
            let mut message = chat("viewer", "Viewer", vec![]);
            message.user_id = id.into();
            viewers.receive_chat(&message, Source::Twitch);
        }
        settle(&pool, |rows| {
            rows.len() == 2 && rows.iter().all(|r| r.2.is_some())
        })
        .await;
        assert_eq!(*profiles.0.lock().unwrap(), [["7", "8"], ["7", "8"]]);
    }

    #[tokio::test]
    async fn identical_timestamps_only_mark_one_sighting_new() {
        let repo = SqliteViewers::new(pool().await);
        let at = Utc::now();
        let sighting = Sighting::from_chat(&chat("fan", "Fan", vec![]));
        let results = repo
            .record(&[(sighting.clone(), at), (sighting, at)])
            .await
            .unwrap();
        assert!(results[0].new);
        assert!(!results[1].new);
    }

    #[tokio::test]
    async fn only_a_viewers_first_sighting_counts_as_new() {
        let pool = pool().await;
        let profiles = Arc::new(Fixed {
            url: AVATAR,
            asked: Mutex::default(),
        });
        let viewers = start(pool.clone(), profiles);
        let m = chat("fan", "Fan", vec![]);
        assert!(viewers.receive_chat_first_time(&m, Source::Twitch).await);
        assert!(!viewers.receive_chat_first_time(&m, Source::Twitch).await);
        let other = chat("sim", "Sim", vec![]);
        assert!(
            !viewers
                .receive_chat_first_time(&other, Source::Simulation)
                .await,
            "simulations never count"
        );
    }

    #[tokio::test]
    async fn a_renamed_viewer_stays_one_row_with_roles_from_chat_and_one_avatar_lookup() {
        let pool = pool().await;
        let profiles = Arc::new(Fixed {
            url: AVATAR,
            asked: Mutex::default(),
        });
        let viewers = start(pool.clone(), profiles.clone());

        viewers.receive_chat(
            &chat("fan", "Fan", vec![ChatBadge::Moderator]),
            Source::Twitch,
        );
        let first = settle(&pool, |r| r.len() == 1 && r[0].2.is_some()).await;
        assert_eq!(
            (&first[0].0, &first[0].1, first[0].2.as_deref(), first[0].3),
            (&"fan".into(), &"Fan".into(), Some(AVATAR), true)
        );

        // Renamed; an alert shows no roles, so the moderator role stays.
        viewers.receive_alert(
            &Alert {
                id: "a".into(),
                user_id: Some("7".into()),
                user_login: Some("newfan".into()),
                user_name: Some("NewFan".into()),
                kind: AlertKind::Follow,
                occurred_at: "2026-11-19T10:00:00Z".into(),
            },
            Source::Twitch,
        );
        let renamed = settle(&pool, |r| r[0].0 == "newfan").await;
        assert_eq!(
            (renamed.len(), renamed[0].1.as_str(), renamed[0].3),
            (1, "NewFan", true)
        );
        assert_eq!(renamed[0].5, first[0].5, "first seen stays");

        // Chat again without the badge: the role follows the latest chat.
        viewers.receive_chat(
            &chat("newfan", "NewFan", vec![ChatBadge::Subscriber]),
            Source::Twitch,
        );
        let now = settle(&pool, |r| r[0].4).await;
        assert!(!now[0].3);
        assert_eq!(
            profiles.asked.lock().unwrap().len(),
            1,
            "the avatar is cached"
        );
    }

    /// Never answers, like a Helix that keeps timing out.
    struct Stuck;

    impl ProfileImages for Stuck {
        fn profile_image<'a>(
            &'a self,
            _: &'a str,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn slow_avatar_lookups_never_hold_up_the_records() {
        let pool = pool().await;
        let viewers = start(pool.clone(), Arc::new(Stuck));
        for n in 0..20 {
            let mut m = chat(&format!("v{n}"), &format!("V{n}"), vec![]);
            m.user_id = format!("{n}");
            viewers.receive_chat(&m, Source::Twitch);
        }
        // Twenty lookups that each take 3 s would need a minute if they ran in the worker.
        let started = std::time::Instant::now();
        settle(&pool, |r| r.len() == 20).await;
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn only_twitch_cdn_avatars_are_stored_and_simulations_change_nothing() {
        let pool = pool().await;
        let profiles = Arc::new(Fixed {
            url: "https://evil.example/x.png",
            asked: Mutex::default(),
        });
        let viewers = start(pool.clone(), profiles.clone());
        viewers.receive_chat(&chat("sim", "Sim", vec![]), Source::Simulation);
        viewers.receive_chat(&chat("fan", "Fan", vec![]), Source::Twitch);
        let now = settle(&pool, |r| r.len() == 1).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while profiles.asked.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            rows(&pool).await[0].2,
            None,
            "a URL off Twitch's CDN is never stored"
        );
        assert_eq!(now[0].0, "fan", "the simulated viewer was never recorded");

        // A redemption records its viewer too.
        viewers.receive_redemption(
            &toolkit_core::events::ChannelPointsRedemption {
                id: "r".into(),
                change: toolkit_core::events::RedemptionChange::Added,
                user_id: "8".into(),
                user_login: "redeemer".into(),
                user_name: "Redeemer".into(),
                reward_id: "w".into(),
                reward_title: "Hydrate".into(),
                reward_cost: 100,
                user_input: String::new(),
                status: toolkit_core::events::RedemptionStatus::Unfulfilled,
                redeemed_at: "2026-11-19T10:00:00Z".into(),
            },
            Source::Twitch,
        );
        let now = settle(&pool, |r| r.len() == 2).await;
        assert_eq!(now[1].0, "redeemer");
    }
}
