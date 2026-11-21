//! SQLite repository for check-ins. Rows map to the domain here; nothing else sees them.

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqliteConnection, SqlitePool, migrate::Migrator};
use uuid::Uuid;

use crate::{
    application::{CheckinRepository, Outcome, RepositoryError},
    domain::{CheckIn, ImportedViewer, Trigger, TwitchUserId, Viewer},
};

/// This module's migrations; the host merges them with the other modules'.
pub static MIGRATOR: Migrator = sqlx::migrate!();

/// How many check-in records there are; the host's backup lists it.
pub async fn record_count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM checkin_records")
        .fetch_one(pool)
        .await
}

pub(crate) struct SqliteCheckins {
    pool: SqlitePool,
}

impl SqliteCheckins {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// A viewer with their totals, for the list.
pub(crate) struct ViewerTotals {
    pub(crate) id: String,
    pub(crate) user_login: String,
    pub(crate) user_name: String,
    pub(crate) total: u64,
    pub(crate) last_day: Option<String>,
}

/// One page of viewers, latest check-in first, and how many match in total. `search` matches part of the login
/// or display name; `%`, `_` and `!` in it are literal.
pub(crate) async fn list_viewers(
    pool: &SqlitePool,
    search: Option<&str>,
    offset: u32,
    limit: u32,
) -> Result<(Vec<ViewerTotals>, u64), RepositoryError> {
    let pattern = search.map(|t| {
        let escaped = t.replace('!', "!!").replace('%', "!%").replace('_', "!_");
        format!("%{escaped}%")
    });
    // Every value is bound; `?1` is the pattern, NULL for no search.
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM checkin_viewers v \
         WHERE (?1 IS NULL OR v.user_login LIKE ?1 ESCAPE '!' OR v.user_name LIKE ?1 ESCAPE '!')",
    )
    .bind(&pattern)
    .fetch_one(pool)
    .await
    .map_err(storage)?;
    let rows: Vec<(String, String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT v.id, v.user_login, v.user_name, v.baseline_total + COUNT(r.id) AS total, \
         MAX(COALESCE(MAX(r.business_day), v.baseline_last_day), COALESCE(v.baseline_last_day, MAX(r.business_day))) AS last_day \
         FROM checkin_viewers v LEFT JOIN checkin_records r ON r.viewer_id = v.id \
         WHERE (?1 IS NULL OR v.user_login LIKE ?1 ESCAPE '!' OR v.user_name LIKE ?1 ESCAPE '!') \
         GROUP BY v.id ORDER BY last_day DESC NULLS LAST, v.user_login, v.user_name, v.id LIMIT ?2 OFFSET ?3",
    )
    .bind(&pattern)
    .bind(i64::from(limit))
    .bind(i64::from(offset))
    .fetch_all(pool)
    .await
    .map_err(storage)?;
    let items = rows
        .into_iter()
        .map(
            |(id, user_login, user_name, total, last_day)| ViewerTotals {
                id,
                user_login,
                user_name,
                total: u64::try_from(total).unwrap_or(0),
                last_day,
            },
        )
        .collect();
    Ok((items, u64::try_from(total).unwrap_or(0)))
}

/// How many viewers checked in on `day`, and the latest `recent` of them (viewer ID, name, time).
pub(crate) async fn day_summary(
    pool: &SqlitePool,
    day: chrono::NaiveDate,
    recent: u32,
) -> Result<(u64, Vec<(String, String, String)>), RepositoryError> {
    let day = day.to_string();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM checkin_records WHERE business_day = ?")
            .bind(&day)
            .fetch_one(pool)
            .await
            .map_err(storage)?;
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT v.id, v.user_name, r.occurred_at FROM checkin_records r JOIN checkin_viewers v ON v.id = r.viewer_id \
         WHERE r.business_day = ? ORDER BY r.occurred_at DESC, r.id DESC LIMIT ?",
    )
    .bind(&day)
    .bind(i64::from(recent))
    .fetch_all(pool)
    .await
    .map_err(storage)?;
    let rows = rows
        .into_iter()
        .map(|(id, name, at)| {
            let at = DateTime::parse_from_rfc3339(&at)
                .map(|t| t.to_utc().to_rfc3339_opts(SecondsFormat::Millis, true))
                .unwrap_or(at);
            (id, name, at)
        })
        .collect();
    Ok((u64::try_from(count).unwrap_or(0), rows))
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// Fixed width, so text order is time order.
pub(crate) fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn trigger_text(t: Trigger) -> &'static str {
    match t {
        Trigger::Chat => "chat",
        Trigger::Redemption => "redemption",
        Trigger::Manual => "manual",
    }
}

/// [`CheckinRepository::check_in`] inside the caller's transaction.
pub(crate) async fn check_in_tx(
    conn: &mut SqliteConnection,
    viewer: &Viewer,
    c: &CheckIn,
) -> Result<Outcome, RepositoryError> {
    let viewer_id: String = sqlx::query_scalar(
        "INSERT INTO checkin_viewers (id, twitch_user_id, user_login, user_name, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (twitch_user_id) DO UPDATE SET \
         user_login = excluded.user_login, user_name = excluded.user_name, updated_at = excluded.updated_at \
         RETURNING id",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&viewer.twitch_id.0)
    .bind(&viewer.login)
    .bind(&viewer.name)
    .bind(time_text(c.recorded_at))
    .bind(time_text(c.recorded_at))
    .fetch_one(&mut *conn)
    .await
    .map_err(storage)?;

    let repeat: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM checkin_records WHERE viewer_id = ? \
         AND (business_day = ? OR (occurred_at >= ? AND occurred_at < ?)))",
    )
    .bind(&viewer_id)
    .bind(c.day.to_string())
    .bind(time_text(c.window.0))
    .bind(time_text(c.window.1))
    .fetch_one(&mut *conn)
    .await
    .map_err(storage)?;
    if repeat {
        return Ok(Outcome::Repeat);
    }

    let inserted = sqlx::query(
        "INSERT INTO checkin_records (id, viewer_id, business_day, occurred_at, trigger_kind, recorded_at) \
         VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (viewer_id, business_day) DO NOTHING",
    )
    .bind(c.id.to_string())
    .bind(&viewer_id)
    .bind(c.day.to_string())
    .bind(time_text(c.occurred_at))
    .bind(trigger_text(c.trigger))
    .bind(time_text(c.recorded_at))
    .execute(&mut *conn)
    .await
    .map_err(storage)?
    .rows_affected();
    if inserted == 0 {
        return Ok(Outcome::Repeat);
    }

    let total: i64 = sqlx::query_scalar(
        "SELECT baseline_total + (SELECT COUNT(*) FROM checkin_records WHERE viewer_id = ?1) \
         FROM checkin_viewers WHERE id = ?1",
    )
    .bind(&viewer_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage)?;
    Ok(Outcome::CheckedIn {
        viewer_id,
        total: u64::try_from(total).unwrap_or(0),
    })
}

impl CheckinRepository for SqliteCheckins {
    async fn known_viewers(&self) -> Result<std::collections::HashSet<String>, RepositoryError> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT twitch_user_id FROM checkin_viewers")
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?;
        Ok(ids.into_iter().collect())
    }

    async fn import(
        &self,
        viewers: &[ImportedViewer],
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let now = time_text(now);
        for v in viewers {
            // A new viewer's login is unknown until they check in; the display name stands in.
            let name = v.name.clone().unwrap_or_else(|| v.twitch_id.0.clone());
            let image = v
                .profile_image
                .as_deref()
                .filter(|u| crate::application::is_twitch_image(u));
            sqlx::query(
                "INSERT INTO checkin_viewers (id, twitch_user_id, user_login, user_name, created_at, updated_at, \
                 profile_image_url, baseline_total, baseline_last_day) VALUES (?, ?, '', ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT (twitch_user_id) DO UPDATE SET baseline_total = excluded.baseline_total, \
                 baseline_last_day = excluded.baseline_last_day, updated_at = excluded.updated_at, \
                 profile_image_url = COALESCE(checkin_viewers.profile_image_url, excluded.profile_image_url)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(&v.twitch_id.0)
            .bind(name)
            .bind(&now)
            .bind(&now)
            .bind(image)
            .bind(i64::try_from(v.total).unwrap_or(i64::MAX))
            .bind(v.last_day.map(|d| d.to_string()))
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)
    }

    async fn profile_image(&self, viewer_id: &str) -> Result<Option<String>, RepositoryError> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT profile_image_url FROM checkin_viewers WHERE id = ?",
        )
        .bind(viewer_id)
        .fetch_optional(&self.pool)
        .await
        .map(Option::flatten)
        .map_err(storage)
    }

    async fn set_profile_image(&self, viewer_id: &str, url: &str) -> Result<(), RepositoryError> {
        sqlx::query("UPDATE checkin_viewers SET profile_image_url = ? WHERE id = ?")
            .bind(url)
            .bind(viewer_id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(storage)
    }

    async fn find_viewer(&self, id: &str) -> Result<Option<Viewer>, RepositoryError> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT twitch_user_id, user_login, user_name FROM checkin_viewers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        Ok(row.map(|(twitch_id, login, name)| Viewer {
            twitch_id: TwitchUserId(twitch_id),
            login,
            name,
        }))
    }

    async fn check_in(&self, viewer: &Viewer, c: &CheckIn) -> Result<Outcome, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let outcome = check_in_tx(&mut tx, viewer, c).await?;
        tx.commit().await.map_err(storage)?;
        Ok(outcome)
    }

    async fn check_in_once(
        &self,
        event_id: &str,
        viewer: &Viewer,
        c: &CheckIn,
    ) -> Result<Outcome, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let new = sqlx::query(
            "INSERT INTO checkin_inbox (event_id, processed_at) VALUES (?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(event_id)
        .bind(time_text(c.recorded_at))
        .execute(&mut *tx)
        .await
        .map_err(storage)?
        .rows_affected()
            == 1;
        let outcome = if new {
            check_in_tx(&mut tx, viewer, c).await?
        } else {
            Outcome::Repeat
        };
        tx.commit().await.map_err(storage)?;
        Ok(outcome)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        application::{CheckinService, Config},
        domain::BusinessDay,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use std::{path::PathBuf, sync::Arc, time::Duration};
    use toolkit_core::{
        events::{ChatMessage, Source},
        modules::{self, Modules},
    };

    /// A temporary data folder of its own.
    pub(crate) fn data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-checkin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    pub(crate) async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let mut all: Vec<_> = MIGRATOR
            .iter()
            .chain(toolkit_core::outbox::MIGRATOR.iter())
            .cloned()
            .collect();
        all.sort_by_key(|m| m.version);
        Migrator::with_migrations(all).run(&pool).await.unwrap();
        pool
    }

    pub(crate) fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().to_utc()
    }

    pub(crate) fn viewer(id: &str) -> Viewer {
        Viewer {
            twitch_id: TwitchUserId(id.into()),
            login: format!("login{id}"),
            name: format!("Viewer {id}"),
        }
    }

    pub(crate) fn chat(user_id: &str, text: &str, sent_at: &str) -> ChatMessage {
        ChatMessage {
            id: Uuid::now_v7().to_string(),
            broadcaster_id: String::new(),
            user_id: user_id.into(),
            user_login: format!("login{user_id}"),
            user_name: format!("<b>Viewer {user_id}</b>"),
            color: None,
            text: text.into(),
            fragments: Vec::new(),
            badges: Vec::new(),
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: sent_at.into(),
        }
    }

    pub(crate) async fn count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM checkin_records")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The viewer's new total, or `None` for a repeat.
    async fn check(repo: &SqliteCheckins, rule: &BusinessDay, id: &str, at: &str) -> Option<u64> {
        let c = CheckIn::at(rule, utc(at), Trigger::Chat, utc(at));
        match repo.check_in(&viewer(id), &c).await.unwrap() {
            Outcome::CheckedIn { total, .. } => Some(total),
            Outcome::Repeat => None,
        }
    }

    #[tokio::test]
    async fn one_check_in_per_viewer_per_business_day() {
        let pool = pool().await;
        let repo = SqliteCheckins::new(pool.clone());
        let taipei = BusinessDay::new("Asia/Taipei", 0).unwrap();
        // 08:00 and 23:59 in Taipei on 11-16, then 00:01 on 11-17.
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T00:00:00Z").await,
            Some(1)
        );
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T15:59:00Z").await,
            None
        );
        assert_eq!(
            check(&repo, &taipei, "2", "2026-11-16T15:59:00Z").await,
            Some(1)
        );
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T16:01:00Z").await,
            Some(2)
        );
        assert_eq!(count(&pool).await, 3);

        // The database refuses a second row for the same day on its own too.
        let raw = sqlx::query(
            "INSERT INTO checkin_records SELECT 'other', viewer_id, business_day, occurred_at, trigger_kind, \
             recorded_at FROM checkin_records LIMIT 1",
        )
        .execute(&pool)
        .await;
        assert!(raw.is_err(), "unique viewer + business day");
    }

    #[tokio::test]
    async fn changing_zone_or_reset_hour_never_counts_today_twice() {
        let pool = pool().await;
        let repo = SqliteCheckins::new(pool.clone());
        // Checked in at 01:00 on 11-17 in Taipei (a 11-17 check-in with a midnight reset).
        let midnight = BusinessDay::new("Asia/Taipei", 0).unwrap();
        assert!(
            check(&repo, &midnight, "1", "2026-11-16T17:00:00Z")
                .await
                .is_some()
        );
        // The day now starts at 06:00, so 05:00 is still 11-16 – the day the 01:00 check-in falls in.
        let six = BusinessDay::new("Asia/Taipei", 6).unwrap();
        assert_eq!(check(&repo, &six, "1", "2026-11-16T21:00:00Z").await, None);
        // Moving to UTC makes 01:00 Taipei 17:00 on 11-16 UTC: the same day by its window.
        let utc_zone = BusinessDay::new("UTC", 0).unwrap();
        assert_eq!(
            check(&repo, &utc_zone, "1", "2026-11-16T20:00:00Z").await,
            None
        );
        assert_eq!(count(&pool).await, 1);
        // UTC's 11-17 carries the same label as the first check-in, so it stays refused: never one too many.
        assert_eq!(
            check(&repo, &utc_zone, "1", "2026-11-17T01:00:00Z").await,
            None
        );
        assert_eq!(
            check(&repo, &utc_zone, "1", "2026-11-18T01:00:00Z").await,
            Some(2)
        );
    }

    #[tokio::test]
    async fn viewer_keeps_the_latest_names() {
        let pool = pool().await;
        let repo = SqliteCheckins::new(pool.clone());
        let rule = BusinessDay::new("UTC", 0).unwrap();
        check(&repo, &rule, "1", "2026-11-16T00:00:00Z").await;
        let mut renamed = viewer("1");
        renamed.name = "New name".into();
        let c = CheckIn::at(
            &rule,
            utc("2026-11-16T01:00:00Z"),
            Trigger::Chat,
            utc("2026-11-16T01:00:00Z"),
        );
        assert_eq!(repo.check_in(&renamed, &c).await.unwrap(), Outcome::Repeat);
        let (name, rows): (String, i64) = sqlx::query_as(
            "SELECT user_name, (SELECT COUNT(*) FROM checkin_viewers) FROM checkin_viewers",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((name.as_str(), rows), ("New name", 1));
    }

    pub(crate) fn service(pool: &SqlitePool, name: &str) -> (CheckinService, Arc<Modules>) {
        let dir = data_dir(name);
        let modules = Arc::new(Modules::load(dir.clone()));
        let config = Arc::new(Config::load(
            dir.clone(),
            crate::application::test_commands(&dir),
        ));
        let service = CheckinService::start(
            SqliteCheckins::new(pool.clone()),
            modules.clone(),
            config,
            crate::infrastructure::tests::quiet_screen(),
        );
        (service, modules)
    }

    pub(crate) async fn wait_for_count(pool: &SqlitePool, n: i64) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while count(pool).await != n {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn only_those_allowed_check_in_and_the_broadcaster_always_may() {
        use toolkit_core::{commands::Allowed, events::ChatBadge};
        let pool = pool().await;
        let (service, _) = service(&pool, "allowed");
        service
            .config()
            .update(
                &toolkit_core::settings::CheckinSettings::default(),
                Allowed::Vip,
            )
            .unwrap();
        let with = |id: &str, badges: &[ChatBadge]| ChatMessage {
            broadcaster_id: "100".into(),
            badges: badges.to_vec(),
            ..chat(id, "!打卡", "2026-11-16T01:00:00Z")
        };
        for m in [
            with("1", &[]),
            with("2", &[ChatBadge::Subscriber]),
            with("3", &[ChatBadge::Vip]),
            with("4", &[ChatBadge::Moderator]),
            with("100", &[]),
        ] {
            service.receive_chat(&m, Source::Twitch);
        }
        wait_for_count(&pool, 3).await;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM checkin_viewers ORDER BY twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(ids, ["100", "3", "4"]);
    }

    #[tokio::test]
    async fn chat_command_checks_in_and_simulations_never_do() {
        let pool = pool().await;
        let (service, _) = service(&pool, "chat");
        service.receive_chat(
            &chat("1", "!打卡", "2026-11-16T01:00:00Z"),
            Source::Simulation,
        );
        service.receive_chat(&chat("2", "打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
        service.receive_chat(
            &chat("3", "hi !打卡", "2026-11-16T01:00:00Z"),
            Source::Twitch,
        );
        service.receive_chat(
            &chat("4", "!打卡 早安", "2026-11-16T01:00:00Z"),
            Source::Twitch,
        );
        service.receive_chat(&chat("4", "!打卡", "2026-11-16T02:00:00Z"), Source::Twitch);
        service.receive_chat(&chat("5", "!打卡", "2026-11-16T02:00:00Z"), Source::Twitch);
        wait_for_count(&pool, 2).await;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT v.twitch_user_id FROM checkin_viewers v ORDER BY v.twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            ids,
            ["4", "5"],
            "only real messages starting with the command"
        );
        let (day, at, kind): (String, String, String) = sqlx::query_as(
            "SELECT business_day, occurred_at, trigger_kind FROM checkin_records ORDER BY occurred_at LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(day, "2026-11-16");
        assert_eq!(
            at, "2026-11-16T01:00:00.000000Z",
            "the time the message was sent"
        );
        assert_eq!(kind, "chat");
    }

    #[tokio::test]
    async fn disabled_module_drops_chat_commands() {
        let pool = pool().await;
        let (service, switch) = service(&pool, "disabled");
        switch.set_enabled(modules::CHECKIN, false).unwrap();
        service.receive_chat(&chat("1", "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
        switch.set_enabled(modules::CHECKIN, true).unwrap();
        service.receive_chat(&chat("2", "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
        wait_for_count(&pool, 1).await;
        let id: String = sqlx::query_scalar("SELECT twitch_user_id FROM checkin_viewers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(id, "2");
    }
}
