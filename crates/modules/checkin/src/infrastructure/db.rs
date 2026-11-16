//! SQLite repository for check-ins. Rows map to the domain here; nothing else sees them.

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqliteConnection, SqlitePool, migrate::Migrator};
use uuid::Uuid;

use crate::{
    application::{CheckinRepository, Outcome, RepositoryError},
    domain::{CheckIn, Trigger, Viewer},
};

/// This module's migrations; the host merges them with the other modules'.
pub static MIGRATOR: Migrator = sqlx::migrate!();

pub(crate) struct SqliteCheckins {
    pool: SqlitePool,
}

impl SqliteCheckins {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
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

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkin_records WHERE viewer_id = ?")
        .bind(&viewer_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(storage)?;
    Ok(Outcome::CheckedIn {
        total: u64::try_from(total).unwrap_or(0),
    })
}

impl CheckinRepository for SqliteCheckins {
    async fn check_in(&self, viewer: &Viewer, c: &CheckIn) -> Result<Outcome, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let outcome = check_in_tx(&mut tx, viewer, c).await?;
        tx.commit().await.map_err(storage)?;
        Ok(outcome)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        application::{CheckinService, Config},
        domain::{BusinessDay, TwitchUserId},
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
        MIGRATOR.run(&pool).await.unwrap();
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
            user_id: user_id.into(),
            user_login: format!("login{user_id}"),
            user_name: format!("<b>Viewer {user_id}</b>"),
            color: None,
            text: text.into(),
            fragments: Vec::new(),
            sent_at: sent_at.into(),
        }
    }

    pub(crate) async fn count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM checkin_records")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn check(repo: &SqliteCheckins, rule: &BusinessDay, id: &str, at: &str) -> Outcome {
        let c = CheckIn::at(rule, utc(at), Trigger::Chat, utc(at));
        repo.check_in(&viewer(id), &c).await.unwrap()
    }

    #[tokio::test]
    async fn one_check_in_per_viewer_per_business_day() {
        let pool = pool().await;
        let repo = SqliteCheckins::new(pool.clone());
        let taipei = BusinessDay::new("Asia/Taipei", 0).unwrap();
        // 08:00 and 23:59 in Taipei on 11-16, then 00:01 on 11-17.
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T00:00:00Z").await,
            Outcome::CheckedIn { total: 1 }
        );
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T15:59:00Z").await,
            Outcome::Repeat
        );
        assert_eq!(
            check(&repo, &taipei, "2", "2026-11-16T15:59:00Z").await,
            Outcome::CheckedIn { total: 1 }
        );
        assert_eq!(
            check(&repo, &taipei, "1", "2026-11-16T16:01:00Z").await,
            Outcome::CheckedIn { total: 2 }
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
        assert!(matches!(
            check(&repo, &midnight, "1", "2026-11-16T17:00:00Z").await,
            Outcome::CheckedIn { .. }
        ));
        // The day now starts at 06:00, so 05:00 is still 11-16 – the day the 01:00 check-in falls in.
        let six = BusinessDay::new("Asia/Taipei", 6).unwrap();
        assert_eq!(
            check(&repo, &six, "1", "2026-11-16T21:00:00Z").await,
            Outcome::Repeat
        );
        // Moving to UTC makes 01:00 Taipei 17:00 on 11-16 UTC: the same day by its window.
        let utc_zone = BusinessDay::new("UTC", 0).unwrap();
        assert_eq!(
            check(&repo, &utc_zone, "1", "2026-11-16T20:00:00Z").await,
            Outcome::Repeat
        );
        assert_eq!(count(&pool).await, 1);
        // UTC's 11-17 carries the same label as the first check-in, so it stays refused: never one too many.
        assert_eq!(
            check(&repo, &utc_zone, "1", "2026-11-17T01:00:00Z").await,
            Outcome::Repeat
        );
        assert!(matches!(
            check(&repo, &utc_zone, "1", "2026-11-18T01:00:00Z").await,
            Outcome::CheckedIn { total: 2 }
        ));
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
        let config = Arc::new(Config::load(dir));
        let service =
            CheckinService::start(SqliteCheckins::new(pool.clone()), modules.clone(), config);
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
