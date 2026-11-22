//! Outbox for domain events between modules (docs/architecture.md §4). A publisher writes the event in its own
//! transaction, together with one delivery row per module subscribed at that moment; a background dispatcher then
//! hands each event to each subscriber in publish order, with bounded retries. Subscribers must be idempotent:
//! they record the event ID in their own inbox table in the same transaction as their effect.

use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool, migrate::Migrator};
use tokio::sync::Notify;

use crate::modules::Modules;

/// The outbox tables belong to `core`; the host merges these migrations with the modules'.
pub static MIGRATOR: Migrator = sqlx::migrate!();

/// Failed attempts before an event moves to the dead letters, where it waits for retry or skip.
pub const MAX_ATTEMPTS: i64 = 5;
const POLL: Duration = Duration::from_secs(1);
const CLEANUP_EVERY: Duration = Duration::from_secs(600);

/// A domain event as stored. `occurred_at` is when it happened in the business sense (RFC 3339, UTC):
/// subscribers judge business time from it, never from when they process it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEvent {
    pub id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

pub type Handled<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// A module's handler. It may get the same event more than once and must apply it only once.
pub trait Subscriber: Send + Sync + 'static {
    fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a>;
}

/// `module` (its registry ID) receives every `event_type` event published from now on, enabled or not.
pub struct Subscription {
    pub module: &'static str,
    pub event_type: &'static str,
    pub handler: Arc<dyn Subscriber>,
}

pub struct Outbox {
    pool: SqlitePool,
    modules: Arc<Modules>,
    subscriptions: Vec<Subscription>,
    wake: Notify,
}

/// Events a subscriber has not received yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlog {
    pub pending: u64,
    /// `occurred_at` of the oldest one.
    pub oldest: Option<DateTime<Utc>>,
}

/// An event that failed [`MAX_ATTEMPTS`] times and waits for the broadcaster to retry or skip it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeadLetter {
    pub event: OutboxEvent,
    pub attempts: i64,
    pub last_error: String,
    pub failed_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DeadRow {
    id: String,
    event_type: String,
    payload: String,
    occurred_at: String,
    attempts: i64,
    last_error: Option<String>,
    updated_at: String,
}

/// Fixed width, so text order is time order.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

impl Outbox {
    /// `pool` must already have [`MIGRATOR`] applied. A disabled module gets nothing: while paused its events
    /// wait (retries are not counted), while closed they are skipped.
    pub fn new(
        pool: SqlitePool,
        modules: Arc<Modules>,
        subscriptions: Vec<Subscription>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            modules,
            subscriptions,
            wake: Notify::new(),
        })
    }

    fn modules(&self) -> BTreeSet<&'static str> {
        self.subscriptions.iter().map(|s| s.module).collect()
    }

    /// Writes `event` and its deliveries inside the caller's transaction; call [`Outbox::wake`] after committing.
    pub async fn publish(
        &self,
        conn: &mut SqliteConnection,
        event: &OutboxEvent,
    ) -> sqlx::Result<()> {
        let now = time_text(Utc::now());
        sqlx::query(
            "INSERT INTO outbox_events (id, event_type, payload, occurred_at, published_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&event.id)
        .bind(&event.event_type)
        .bind(event.payload.to_string())
        .bind(time_text(event.occurred_at))
        .bind(&now)
        .execute(&mut *conn)
        .await?;
        for s in self
            .subscriptions
            .iter()
            .filter(|s| s.event_type == event.event_type)
        {
            sqlx::query(
                "INSERT INTO outbox_deliveries (event_id, subscriber, state, next_attempt_at, updated_at) \
                 VALUES (?, ?, 'pending', ?, ?) ON CONFLICT DO NOTHING",
            )
            .bind(&event.id)
            .bind(s.module)
            .bind(&now)
            .bind(&now)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// Publishes `event` in a transaction of its own and wakes the dispatcher, for a publisher with no write of its
    /// own to share it with.
    pub async fn publish_now(&self, event: &OutboxEvent) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        self.publish(&mut tx, event).await?;
        tx.commit().await?;
        self.wake();
        Ok(())
    }

    /// Lets the dispatcher run now instead of at its next poll.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Spawns the dispatcher, which also delivers what was left over before a restart; needs a Tokio runtime.
    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut cleaned: Option<Instant> = None;
            loop {
                if let Err(e) = this.dispatch(Utc::now()).await {
                    tracing::error!(code = "outbox.dispatch_failed", error = %e, "outbox not dispatched");
                }
                if cleaned.is_none_or(|t| t.elapsed() >= CLEANUP_EVERY) {
                    if let Err(e) = this.clean_up().await {
                        tracing::error!(code = "outbox.cleanup_failed", error = %e, "outbox not cleaned up");
                    }
                    cleaned = Some(Instant::now());
                }
                tokio::select! {
                    () = this.wake.notified() => {}
                    () = tokio::time::sleep(POLL) => {}
                }
            }
        });
    }

    /// Delivers due events to every subscriber, each in publish order. A failing event holds back the ones after
    /// it for that subscriber only, until it succeeds or moves to the dead letters.
    pub async fn dispatch(&self, now: DateTime<Utc>) -> sqlx::Result<()> {
        for module in self.modules() {
            self.deliver_to(module, now).await?;
        }
        Ok(())
    }

    async fn deliver_to(&self, module: &str, now: DateTime<Utc>) -> sqlx::Result<()> {
        let now_text = time_text(now);
        loop {
            // Checked per event, so a module disabled mid-way stops at once.
            if !self.modules.is_enabled(module) {
                if self.modules.is_closed(module) {
                    self.skip_pending(module).await?;
                }
                return Ok(());
            }
            // No connection is held while the handler runs: it needs the (single) connection itself.
            let head: Option<(String, String, String, String, i64, String)> = sqlx::query_as(
                "SELECT e.id, e.event_type, e.payload, e.occurred_at, d.attempts, d.next_attempt_at \
                 FROM outbox_deliveries d JOIN outbox_events e ON e.id = d.event_id \
                 WHERE d.subscriber = ? AND d.state = 'pending' ORDER BY e.published_at, e.id LIMIT 1",
            )
            .bind(module)
            .fetch_optional(&self.pool)
            .await?;
            let Some((id, event_type, payload, occurred_at, attempts, due)) = head else {
                return Ok(());
            };
            if due > now_text {
                return Ok(());
            }
            let Some(handler) = self
                .subscriptions
                .iter()
                .find(|s| s.module == module && s.event_type == event_type)
                .map(|s| s.handler.clone())
            else {
                // The module no longer takes this event type.
                self.set_state(&id, module, "skipped", None, &now_text)
                    .await?;
                continue;
            };
            let result = match (
                serde_json::from_str(&payload),
                DateTime::parse_from_rfc3339(&occurred_at),
            ) {
                (Ok(payload), Ok(at)) => {
                    let event = OutboxEvent {
                        id: id.clone(),
                        event_type,
                        payload,
                        occurred_at: at.to_utc(),
                    };
                    handler.handle(&event).await
                }
                _ => Err("unreadable stored event".to_owned()),
            };
            match result {
                Ok(()) => {
                    self.set_state(&id, module, "delivered", None, &now_text)
                        .await?
                }
                Err(error) if attempts + 1 >= MAX_ATTEMPTS => {
                    tracing::error!(code = "outbox.dead_letter", event = %id, subscriber = module, %error, "event moved to the dead letters");
                    self.fail(&id, module, "dead", &error, &now_text, &now_text)
                        .await?;
                }
                Err(error) => {
                    tracing::warn!(code = "outbox.delivery_failed", event = %id, subscriber = module, %error, "event will be retried");
                    let backoff = Duration::from_secs(1 << attempts.clamp(0, 10));
                    let due = time_text(now + backoff);
                    self.fail(&id, module, "pending", &error, &due, &now_text)
                        .await?;
                    return Ok(());
                }
            }
        }
    }

    async fn set_state(
        &self,
        id: &str,
        module: &str,
        state: &str,
        error: Option<&str>,
        now: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE outbox_deliveries SET state = ?, last_error = ?, updated_at = ? WHERE event_id = ? AND subscriber = ?",
        )
        .bind(state)
        .bind(error)
        .bind(now)
        .bind(id)
        .bind(module)
        .execute(&self.pool)
        .await
        .map(|_| ())
    }

    async fn fail(
        &self,
        id: &str,
        module: &str,
        state: &str,
        error: &str,
        due: &str,
        now: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE outbox_deliveries SET state = ?, attempts = attempts + 1, last_error = ?, next_attempt_at = ?, \
             updated_at = ? WHERE event_id = ? AND subscriber = ?",
        )
        .bind(state)
        .bind(error)
        .bind(due)
        .bind(now)
        .bind(id)
        .bind(module)
        .execute(&self.pool)
        .await
        .map(|_| ())
    }

    /// Marks every event `module` has not received yet as skipped; returns how many. Called before a closed module
    /// is enabled again, so events from the closed period are never delivered.
    pub async fn skip_pending(&self, module: &str) -> sqlx::Result<u64> {
        sqlx::query(
            "UPDATE outbox_deliveries SET state = 'skipped', updated_at = ? WHERE subscriber = ? AND state = 'pending'",
        )
        .bind(time_text(Utc::now()))
        .bind(module)
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected())
    }

    pub async fn backlog(&self, module: &str) -> sqlx::Result<Backlog> {
        let (pending, oldest): (i64, Option<String>) = sqlx::query_as(
            "SELECT COUNT(*), MIN(e.occurred_at) FROM outbox_deliveries d JOIN outbox_events e ON e.id = d.event_id              WHERE d.subscriber = ? AND d.state = 'pending'",
        )
        .bind(module)
        .fetch_one(&self.pool)
        .await?;
        Ok(Backlog {
            pending: u64::try_from(pending).unwrap_or(0),
            oldest: oldest
                .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                .map(|t| t.to_utc()),
        })
    }

    /// Oldest first; unreadable rows are left out.
    pub async fn dead_letters(&self, module: &str) -> sqlx::Result<Vec<DeadLetter>> {
        let rows: Vec<DeadRow> = sqlx::query_as(
            "SELECT e.id, e.event_type, e.payload, e.occurred_at, d.attempts, d.last_error, d.updated_at              FROM outbox_deliveries d JOIN outbox_events e ON e.id = d.event_id              WHERE d.subscriber = ? AND d.state = 'dead' ORDER BY e.published_at, e.id",
        )
        .bind(module)
        .fetch_all(&self.pool)
        .await?;
        let time = |t: &str| DateTime::parse_from_rfc3339(t).ok().map(|t| t.to_utc());
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(DeadLetter {
                    event: OutboxEvent {
                        id: r.id,
                        event_type: r.event_type,
                        payload: serde_json::from_str(&r.payload).ok()?,
                        occurred_at: time(&r.occurred_at)?,
                    },
                    attempts: r.attempts,
                    last_error: r.last_error.unwrap_or_default(),
                    failed_at: time(&r.updated_at)?,
                })
            })
            .collect())
    }

    /// Puts a dead letter back in line with a fresh set of attempts; `false` when there is no such dead letter.
    pub async fn retry(&self, module: &str, event_id: &str) -> sqlx::Result<bool> {
        let now = time_text(Utc::now());
        let changed = sqlx::query(
            "UPDATE outbox_deliveries SET state = 'pending', attempts = 0, next_attempt_at = ?, updated_at = ?              WHERE subscriber = ? AND event_id = ? AND state = 'dead'",
        )
        .bind(&now)
        .bind(&now)
        .bind(module)
        .bind(event_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1;
        if changed {
            self.wake();
        }
        Ok(changed)
    }

    /// Gives up on a dead letter; `false` when there is no such dead letter.
    pub async fn skip(&self, module: &str, event_id: &str) -> sqlx::Result<bool> {
        sqlx::query(
            "UPDATE outbox_deliveries SET state = 'skipped', updated_at = ?              WHERE subscriber = ? AND event_id = ? AND state = 'dead'",
        )
        .bind(time_text(Utc::now()))
        .bind(module)
        .bind(event_id)
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected() == 1)
    }

    /// Drops the deliveries of modules that no longer subscribe to anything, then every event no subscriber still
    /// waits for (pending) or has to decide on (dead).
    pub async fn clean_up(&self) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        let mut removed = QueryBuilder::<Sqlite>::new("DELETE FROM outbox_deliveries");
        let modules = self.modules();
        if !modules.is_empty() {
            removed.push(" WHERE subscriber NOT IN (");
            let mut list = removed.separated(", ");
            for m in modules {
                list.push_bind(m);
            }
            removed.push(")");
        }
        removed.build().execute(&mut *tx).await?;
        sqlx::query(
            "DELETE FROM outbox_deliveries WHERE event_id IN (SELECT e.id FROM outbox_events e WHERE NOT EXISTS \
             (SELECT 1 FROM outbox_deliveries d WHERE d.event_id = e.id AND d.state IN ('pending', 'dead')))",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM outbox_events WHERE NOT EXISTS \
             (SELECT 1 FROM outbox_deliveries d WHERE d.event_id = outbox_events.id)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Mutex;

    /// A registry where every module is enabled.
    fn modules() -> Arc<Modules> {
        Arc::new(Modules::load(std::env::temp_dir().join(format!(
            "st-outbox-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))))
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

    /// Applies each event once through its own inbox rows, like a real subscriber; fails while `failures` lasts.
    #[derive(Default)]
    struct Recorder {
        inbox: String,
        pool: Mutex<Option<SqlitePool>>,
        applied: Mutex<Vec<String>>,
        calls: Mutex<u32>,
        failures: Mutex<u32>,
    }

    impl Subscriber for Recorder {
        fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a> {
            Box::pin(async move {
                *self.calls.lock().unwrap() += 1;
                {
                    let mut failures = self.failures.lock().unwrap();
                    if *failures > 0 {
                        *failures -= 1;
                        return Err("boom".into());
                    }
                }
                let pool = self.pool.lock().unwrap().clone().unwrap();
                let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
                let new = sqlx::query(
                    "INSERT INTO test_inbox (event_id) VALUES (?) ON CONFLICT DO NOTHING",
                )
                .bind(format!("{}:{}", self.inbox, event.id))
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .rows_affected()
                    == 1;
                tx.commit().await.map_err(|e| e.to_string())?;
                if new {
                    self.applied.lock().unwrap().push(event.id.clone());
                }
                Ok(())
            })
        }
    }

    fn recorder(pool: &SqlitePool) -> Arc<Recorder> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let r = Arc::new(Recorder {
            inbox: NEXT
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string(),
            ..Recorder::default()
        });
        *r.pool.lock().unwrap() = Some(pool.clone());
        r
    }

    fn sub(module: &'static str, handler: Arc<Recorder>) -> Subscription {
        Subscription {
            module,
            event_type: "thing.done",
            handler,
        }
    }

    fn event(id: &str) -> OutboxEvent {
        OutboxEvent {
            id: id.into(),
            event_type: "thing.done".into(),
            payload: serde_json::json!({ "n": id }),
            occurred_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    async fn publish(outbox: &Outbox, pool: &SqlitePool, id: &str) {
        let mut tx = pool.begin().await.unwrap();
        outbox.publish(&mut tx, &event(id)).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn states(pool: &SqlitePool) -> Vec<(String, String, String, i64)> {
        sqlx::query_as(
            "SELECT event_id, subscriber, state, attempts FROM outbox_deliveries ORDER BY event_id, subscriber",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn with_inbox() -> SqlitePool {
        let pool = pool().await;
        sqlx::query("CREATE TABLE test_inbox (event_id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    fn at(secs: i64) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(secs)
    }

    #[tokio::test]
    async fn event_is_written_in_the_callers_transaction() {
        let pool = with_inbox().await;
        let outbox = Outbox::new(pool.clone(), modules(), vec![sub("a", recorder(&pool))]);
        let mut tx = pool.begin().await.unwrap();
        outbox.publish(&mut tx, &event("e1")).await.unwrap();
        tx.rollback().await.unwrap();
        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(events, 0, "rolled back with the business data");
        assert!(states(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn undelivered_events_are_delivered_after_a_restart_in_order() {
        let pool = with_inbox().await;
        let before = Outbox::new(pool.clone(), modules(), vec![sub("a", recorder(&pool))]);
        publish(&before, &pool, "e1").await;
        publish(&before, &pool, "e2").await;
        drop(before); // the program stopped before dispatching

        let a = recorder(&pool);
        let after = Outbox::new(pool.clone(), modules(), vec![sub("a", a.clone())]);
        after.dispatch(at(0)).await.unwrap();
        assert_eq!(*a.applied.lock().unwrap(), ["e1", "e2"]);
        assert!(states(&pool).await.iter().all(|s| s.2 == "delivered"));
    }

    #[tokio::test]
    async fn an_event_delivered_twice_is_applied_once() {
        let pool = with_inbox().await;
        let a = recorder(&pool);
        let outbox = Outbox::new(pool.clone(), modules(), vec![sub("a", a.clone())]);
        publish(&outbox, &pool, "e1").await;
        outbox.dispatch(at(0)).await.unwrap();
        // A crash after the subscriber committed but before the delivery was marked sends it again.
        sqlx::query("UPDATE outbox_deliveries SET state = 'pending'")
            .execute(&pool)
            .await
            .unwrap();
        outbox.dispatch(at(0)).await.unwrap();
        assert_eq!(*a.calls.lock().unwrap(), 2);
        assert_eq!(*a.applied.lock().unwrap(), ["e1"]);
    }

    #[tokio::test]
    async fn a_failing_subscriber_holds_back_only_itself_until_its_event_is_dead() {
        let pool = with_inbox().await;
        let (a, b) = (recorder(&pool), recorder(&pool));
        *a.failures.lock().unwrap() = u32::MAX;
        let outbox = Outbox::new(
            pool.clone(),
            modules(),
            vec![sub("a", a.clone()), sub("b", b.clone())],
        );
        publish(&outbox, &pool, "e1").await;
        publish(&outbox, &pool, "e2").await;

        outbox.dispatch(at(0)).await.unwrap();
        assert_eq!(
            *b.applied.lock().unwrap(),
            ["e1", "e2"],
            "b is not held back"
        );
        assert_eq!(
            *a.calls.lock().unwrap(),
            1,
            "a keeps the order and waits to retry e1"
        );
        outbox.dispatch(at(0)).await.unwrap();
        assert_eq!(*a.calls.lock().unwrap(), 1, "not due yet");

        // Retries back off (1, 2, 4, 8 s); the fifth failure moves e1 to the dead letters.
        for n in 1..MAX_ATTEMPTS {
            outbox.dispatch(at(1 << n)).await.unwrap();
        }
        let s = states(&pool).await;
        assert_eq!(s[0], ("e1".into(), "a".into(), "dead".into(), MAX_ATTEMPTS));
        // e2 is no longer held back by it (and fails on its own here).
        assert_eq!(s[2].2, "pending");
        assert_eq!(s[2].3, 1);

        *a.failures.lock().unwrap() = 0;
        outbox.dispatch(at(60)).await.unwrap();
        assert_eq!(*a.applied.lock().unwrap(), ["e2"]);
        assert_eq!(
            states(&pool).await[0].2,
            "dead",
            "a dead event waits for the broadcaster"
        );
    }

    #[tokio::test]
    async fn dead_letters_can_be_retried_or_skipped() {
        let pool = with_inbox().await;
        let a = recorder(&pool);
        let outbox = Outbox::new(pool.clone(), modules(), vec![sub("a", a.clone())]);
        publish(&outbox, &pool, "e1").await;
        publish(&outbox, &pool, "e2").await;
        sqlx::query(
            "UPDATE outbox_deliveries SET state = 'dead', attempts = 5, last_error = 'boom'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let dead = outbox.dead_letters("a").await.unwrap();
        assert_eq!(dead.len(), 2);
        assert_eq!(
            (
                dead[0].event.id.as_str(),
                dead[0].attempts,
                dead[0].last_error.as_str()
            ),
            ("e1", 5, "boom")
        );
        assert_eq!(dead[0].event.payload, serde_json::json!({ "n": "e1" }));

        assert!(outbox.retry("a", "e1").await.unwrap());
        assert!(outbox.skip("a", "e2").await.unwrap());
        assert!(!outbox.retry("a", "e2").await.unwrap(), "only dead letters");
        assert!(!outbox.skip("a", "missing").await.unwrap());
        outbox.dispatch(at(0)).await.unwrap();
        assert_eq!(*a.applied.lock().unwrap(), ["e1"]);
        assert!(outbox.dead_letters("a").await.unwrap().is_empty());
        assert_eq!(
            outbox.backlog("a").await.unwrap(),
            Backlog {
                pending: 0,
                oldest: None
            }
        );
    }

    #[tokio::test]
    async fn subscribers_are_fixed_when_the_event_is_published() {
        let pool = with_inbox().await;
        let before = Outbox::new(pool.clone(), modules(), vec![sub("a", recorder(&pool))]);
        publish(&before, &pool, "old").await;
        let late = recorder(&pool);
        let after = Outbox::new(
            pool.clone(),
            modules(),
            vec![sub("a", recorder(&pool)), sub("late", late.clone())],
        );
        publish(&after, &pool, "new").await;
        after.dispatch(at(0)).await.unwrap();
        assert_eq!(
            *late.applied.lock().unwrap(),
            ["new"],
            "a module added later never gets past events"
        );
    }

    #[tokio::test]
    async fn clean_up_keeps_what_is_still_waiting_or_dead() {
        let pool = with_inbox().await;
        let outbox = Outbox::new(
            pool.clone(),
            modules(),
            vec![sub("a", recorder(&pool)), sub("gone", recorder(&pool))],
        );
        for id in ["done", "waiting", "dead"] {
            publish(&outbox, &pool, id).await;
        }
        // Without "gone" (a removed module); "a" has one of each state.
        let outbox = Outbox::new(pool.clone(), modules(), vec![sub("a", recorder(&pool))]);
        for (id, state) in [("done", "delivered"), ("dead", "dead")] {
            sqlx::query(
                "UPDATE outbox_deliveries SET state = ? WHERE event_id = ? AND subscriber = 'a'",
            )
            .bind(state)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }
        outbox.clean_up().await.unwrap();
        let left: Vec<String> = sqlx::query_scalar("SELECT id FROM outbox_events ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left, ["dead", "waiting"]);
        assert!(
            states(&pool).await.iter().all(|s| s.1 == "a"),
            "the removed module's deliveries are gone"
        );
    }
}
