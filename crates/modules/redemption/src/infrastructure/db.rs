//! SQLite repository for redemption records. Rows map to the domain here; nothing else sees them.

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{QueryBuilder, Sqlite, SqlitePool, migrate::Migrator};
use uuid::Uuid;

use crate::{
    application::{ListFilter, RedemptionRepository, RepositoryError},
    domain::{Redemption, RedemptionId, Reward, Status, TwitchRedemptionId, Viewer},
};

/// This module's migrations; the host merges them with the other modules'.
pub static MIGRATOR: Migrator = sqlx::migrate!();

pub(crate) struct SqliteRedemptions {
    pool: SqlitePool,
}

impl SqliteRedemptions {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// One page of records, newest first, and how many match in total.
    pub(crate) async fn list(
        &self,
        filter: &ListFilter,
    ) -> Result<(Vec<Redemption>, u64), RepositoryError> {
        let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM redemption_records");
        push_filter(&mut count, filter);
        let total: i64 = count
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(storage)?;

        let mut page = QueryBuilder::<Sqlite>::new("SELECT * FROM redemption_records");
        push_filter(&mut page, filter);
        page.push(" ORDER BY redeemed_at DESC, id DESC LIMIT ")
            .push_bind(i64::from(filter.limit))
            .push(" OFFSET ")
            .push_bind(i64::from(filter.offset));
        let rows: Vec<RedemptionRow> = page
            .build_query_as()
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?;
        let items = rows
            .into_iter()
            .map(Redemption::try_from)
            .collect::<Result<_, _>>()?;
        Ok((items, u64::try_from(total).unwrap_or(0)))
    }
}

/// `WHERE` for [`ListFilter`]; every value is bound, none is spliced into the SQL.
fn push_filter(q: &mut QueryBuilder<Sqlite>, f: &ListFilter) {
    q.push(" WHERE 1 = 1");
    if let Some(from) = f.from {
        q.push(" AND redeemed_at >= ").push_bind(time_text(from));
    }
    if let Some(to) = f.to {
        q.push(" AND redeemed_at < ").push_bind(time_text(to));
    }
    if let Some(viewer) = &f.viewer {
        let pattern = like_pattern(viewer);
        q.push(" AND (user_login LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '!' OR user_name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '!')");
    }
    if let Some(reward) = &f.reward {
        q.push(" AND reward_title LIKE ")
            .push_bind(like_pattern(reward))
            .push(" ESCAPE '!'");
    }
    if let Some(status) = f.status {
        q.push(" AND status = ").push_bind(status_text(status));
    }
}

/// Matches `text` anywhere; `%`, `_` and `!` (the escape character) in it are literal. SQLite's LIKE ignores ASCII case only.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_");
    format!("%{escaped}%")
}

#[derive(sqlx::FromRow)]
struct RedemptionRow {
    id: String,
    twitch_redemption_id: String,
    user_id: String,
    user_login: String,
    user_name: String,
    reward_id: String,
    reward_title: String,
    reward_cost: i64,
    user_input: String,
    status: String,
    redeemed_at: String,
    updated_at: String,
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// Fixed width, so text order is time order.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, RepositoryError> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.to_utc())
        .map_err(storage)
}

fn status_text(s: Status) -> &'static str {
    match s {
        Status::Unfulfilled => "unfulfilled",
        Status::Fulfilled => "fulfilled",
        Status::Canceled => "canceled",
        Status::Unknown => "unknown",
    }
}

fn parse_status(s: &str) -> Result<Status, RepositoryError> {
    Ok(match s {
        "unfulfilled" => Status::Unfulfilled,
        "fulfilled" => Status::Fulfilled,
        "canceled" => Status::Canceled,
        "unknown" => Status::Unknown,
        other => return Err(storage(format!("unknown status {other:?}"))),
    })
}

impl TryFrom<RedemptionRow> for Redemption {
    type Error = RepositoryError;

    fn try_from(row: RedemptionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: RedemptionId(Uuid::parse_str(&row.id).map_err(storage)?),
            twitch_id: TwitchRedemptionId(row.twitch_redemption_id),
            viewer: Viewer {
                id: row.user_id,
                login: row.user_login,
                name: row.user_name,
            },
            reward: Reward {
                id: row.reward_id,
                title: row.reward_title,
                cost: u32::try_from(row.reward_cost).map_err(storage)?,
            },
            user_input: row.user_input,
            status: parse_status(&row.status)?,
            redeemed_at: parse_time(&row.redeemed_at)?,
            updated_at: parse_time(&row.updated_at)?,
        })
    }
}

impl RedemptionRepository for SqliteRedemptions {
    async fn insert(&self, r: &Redemption) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, reward_id, \
             reward_title, reward_cost, user_input, status, redeemed_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (twitch_redemption_id) DO NOTHING",
        )
        .bind(r.id.0.to_string())
        .bind(&r.twitch_id.0)
        .bind(&r.viewer.id)
        .bind(&r.viewer.login)
        .bind(&r.viewer.name)
        .bind(&r.reward.id)
        .bind(&r.reward.title)
        .bind(i64::from(r.reward.cost))
        .bind(&r.user_input)
        .bind(status_text(r.status))
        .bind(time_text(r.redeemed_at))
        .bind(time_text(r.updated_at))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(result.rows_affected() == 1)
    }

    async fn find_by_twitch_id(
        &self,
        id: &TwitchRedemptionId,
    ) -> Result<Option<Redemption>, RepositoryError> {
        sqlx::query_as::<_, RedemptionRow>(
            "SELECT * FROM redemption_records WHERE twitch_redemption_id = ?",
        )
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?
        .map(Redemption::try_from)
        .transpose()
    }

    async fn save_status(&self, r: &Redemption) -> Result<(), RepositoryError> {
        sqlx::query("UPDATE redemption_records SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status_text(r.status))
            .bind(time_text(r.updated_at))
            .bind(r.id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::application::{RedemptionService, record};
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;
    use toolkit_core::{
        events::{ChannelPointsRedemption, RedemptionChange, RedemptionStatus, Source},
        modules::{self, Modules},
    };

    /// A module registry in its own temporary folder (every module enabled).
    pub(crate) fn modules(name: &str) -> std::sync::Arc<Modules> {
        let dir = std::env::temp_dir().join(format!("st-redemption-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::sync::Arc::new(Modules::load(dir))
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

    pub(crate) fn event(
        id: &str,
        change: RedemptionChange,
        status: RedemptionStatus,
    ) -> ChannelPointsRedemption {
        ChannelPointsRedemption {
            id: id.into(),
            change,
            user_id: "7".into(),
            user_login: "viewer".into(),
            user_name: "Viewer".into(),
            reward_id: "w-1".into(),
            reward_title: "Hydrate".into(),
            reward_cost: 100,
            user_input: "<b>drink</b>".into(),
            status,
            redeemed_at: "2026-11-16T10:00:00.1234567Z".into(),
        }
    }

    async fn count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM redemption_records")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn stored(repo: &SqliteRedemptions, id: &str) -> Option<Redemption> {
        repo.find_by_twitch_id(&TwitchRedemptionId(id.into()))
            .await
            .unwrap()
    }

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[tokio::test]
    async fn stores_every_field_and_reads_it_back() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool.clone());
        record(
            &repo,
            event(
                "r-1",
                RedemptionChange::Added,
                RedemptionStatus::Unfulfilled,
            ),
            at(100),
        )
        .await
        .unwrap();
        let r = stored(&repo, "r-1").await.unwrap();
        assert_eq!(r.id.0.get_version_num(), 7);
        assert_eq!(
            (
                r.viewer.id.as_str(),
                r.viewer.login.as_str(),
                r.viewer.name.as_str()
            ),
            ("7", "viewer", "Viewer")
        );
        assert_eq!(
            (r.reward.id.as_str(), r.reward.title.as_str(), r.reward.cost),
            ("w-1", "Hydrate", 100)
        );
        assert_eq!(
            r.user_input, "<b>drink</b>",
            "stored raw; escaped when shown"
        );
        assert_eq!(r.status, Status::Unfulfilled);
        assert_eq!(time_text(r.redeemed_at), "2026-11-16T10:00:00.123456Z");
        assert_eq!(r.updated_at, at(100));
    }

    #[tokio::test]
    async fn same_redemption_id_is_stored_once() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool.clone());
        let add = || {
            event(
                "r-1",
                RedemptionChange::Added,
                RedemptionStatus::Unfulfilled,
            )
        };
        record(&repo, add(), at(1)).await.unwrap();
        record(&repo, add(), at(2)).await.unwrap();
        assert_eq!(count(&pool).await, 1);
        assert_eq!(stored(&repo, "r-1").await.unwrap().updated_at, at(1));

        // The database refuses a second row on its own too.
        let mut twin = stored(&repo, "r-1").await.unwrap();
        twin.id = RedemptionId::new();
        assert!(!repo.insert(&twin).await.unwrap());
        let raw = sqlx::query(
            "INSERT INTO redemption_records SELECT 'other-id', twitch_redemption_id, user_id, user_login, user_name, \
             reward_id, reward_title, reward_cost, user_input, status, redeemed_at, updated_at FROM redemption_records",
        )
        .execute(&pool)
        .await;
        assert!(raw.is_err(), "unique constraint on the Twitch ID");
        assert_eq!(count(&pool).await, 1);
    }

    #[tokio::test]
    async fn status_follows_twitch_and_rejects_invalid_changes() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool.clone());
        use RedemptionChange::*;
        use RedemptionStatus::*;
        record(&repo, event("r-1", Added, Unfulfilled), at(1))
            .await
            .unwrap();
        record(&repo, event("r-1", Updated, Canceled), at(2))
            .await
            .unwrap();
        let r = stored(&repo, "r-1").await.unwrap();
        assert_eq!((r.status, r.updated_at), (Status::Canceled, at(2)));

        // Canceled (refunded) never becomes valid again; the rejection is logged, the record kept.
        record(&repo, event("r-1", Updated, Unfulfilled), at(3))
            .await
            .unwrap();
        record(&repo, event("r-1", Updated, Fulfilled), at(4))
            .await
            .unwrap();
        let r = stored(&repo, "r-1").await.unwrap();
        assert_eq!((r.status, r.updated_at), (Status::Canceled, at(2)));

        record(&repo, event("r-2", Added, Unfulfilled), at(5))
            .await
            .unwrap();
        record(&repo, event("r-2", Updated, Fulfilled), at(6))
            .await
            .unwrap();
        assert_eq!(
            stored(&repo, "r-2").await.unwrap().status,
            Status::Fulfilled
        );
        assert_eq!(count(&pool).await, 2);
    }

    #[tokio::test]
    async fn unknown_status_is_stored_and_later_resolved() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool.clone());
        use RedemptionChange::*;
        use RedemptionStatus::*;
        record(&repo, event("r-1", Added, Unknown), at(1))
            .await
            .unwrap();
        assert_eq!(stored(&repo, "r-1").await.unwrap().status, Status::Unknown);
        record(&repo, event("r-1", Updated, Fulfilled), at(2))
            .await
            .unwrap();
        assert_eq!(
            stored(&repo, "r-1").await.unwrap().status,
            Status::Fulfilled
        );
        // An unknown status never overwrites a known one.
        record(&repo, event("r-1", Updated, Unknown), at(3))
            .await
            .unwrap();
        assert_eq!(
            stored(&repo, "r-1").await.unwrap().status,
            Status::Fulfilled
        );
    }

    #[tokio::test]
    async fn update_before_the_redemption_keeps_the_data() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool.clone());
        use RedemptionChange::*;
        use RedemptionStatus::*;
        record(&repo, event("r-1", Updated, Fulfilled), at(1))
            .await
            .unwrap();
        record(&repo, event("r-1", Added, Unfulfilled), at(2))
            .await
            .unwrap();
        let r = stored(&repo, "r-1").await.unwrap();
        assert_eq!(r.status, Status::Fulfilled, "the late add does not undo it");
        assert_eq!(r.user_input, "<b>drink</b>");
        assert_eq!(count(&pool).await, 1);
    }

    #[tokio::test]
    async fn unreadable_time_falls_back_to_the_time_received() {
        let pool = pool().await;
        let repo = SqliteRedemptions::new(pool);
        let mut e = event(
            "r-1",
            RedemptionChange::Added,
            RedemptionStatus::Unfulfilled,
        );
        e.redeemed_at = "yesterday".into();
        record(&repo, e, at(9)).await.unwrap();
        assert_eq!(stored(&repo, "r-1").await.unwrap().redeemed_at, at(9));
    }

    async fn wait_for(repo: &SqliteRedemptions, id: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while stored(repo, id).await.is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn simulated_redemptions_are_never_stored() {
        let pool = pool().await;
        let service =
            RedemptionService::start(SqliteRedemptions::new(pool.clone()), modules("sim"), |_| {});
        use RedemptionChange::*;
        use RedemptionStatus::*;
        service.receive(event("sim", Added, Unfulfilled), Source::Simulation);
        service.receive(event("real", Added, Unfulfilled), Source::Twitch);
        let repo = SqliteRedemptions::new(pool.clone());
        // The simulated one never enters the queue; the real one shows the worker ran.
        wait_for(&repo, "real").await;
        assert!(stored(&repo, "sim").await.is_none());
        assert_eq!(count(&pool).await, 1);
    }

    #[tokio::test]
    async fn disabled_module_drops_redemptions_and_keeps_the_records() {
        let pool = pool().await;
        let switch = modules("disabled");
        let service =
            RedemptionService::start(SqliteRedemptions::new(pool.clone()), switch.clone(), |_| {});
        let repo = SqliteRedemptions::new(pool.clone());
        use RedemptionChange::*;
        use RedemptionStatus::*;
        service.receive(event("before", Added, Unfulfilled), Source::Twitch);
        wait_for(&repo, "before").await;

        switch.set_enabled(modules::REDEMPTION, false).unwrap();
        service.receive(event("while-off", Added, Unfulfilled), Source::Twitch);
        switch.set_enabled(modules::REDEMPTION, true).unwrap();
        service.receive(event("after", Added, Unfulfilled), Source::Twitch);
        wait_for(&repo, "after").await;
        assert!(stored(&repo, "while-off").await.is_none());
        assert!(
            stored(&repo, "before").await.is_some(),
            "disabling keeps the data"
        );
    }
}
