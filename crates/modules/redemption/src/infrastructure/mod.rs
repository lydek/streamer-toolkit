//! Adapters: the SQLite repository, the admin API and the `redemption.changed` screen event.

pub(crate) mod db;
pub(crate) mod http;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::{self, Modules},
    outbox::{Handled, Outbox, OutboxEvent, Subscriber, Subscription},
    presentation::{REDEMPTION_PLAYED, RedemptionPlayed},
};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{application::RedemptionService, domain::Redemption};
use toolkit_core::time::{self, Timestamp};

/// Outbox event type of [`RedemptionRecorded`].
pub const REDEMPTION_RECORDED: &str = "redemption.recorded";

/// `redemption.recorded` in the outbox: a redemption was stored for the first time (whichever Twitch event created
/// it). The outbox event's `occurred_at` is the Twitch redemption time. Fields are raw Twitch text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedemptionRecorded {
    /// The record's internal ID.
    pub redemption_id: String,
    pub twitch_redemption_id: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub reward_id: String,
    pub reward_title: String,
}

const CHANGED: &str = "redemption.changed";
const CHANGED_VERSION: u32 = 1;

/// A `redemption.changed` event exactly as `/ws/overlay` sends it: a record was created or changed status. It
/// carries only the record ID, so viewer text stays off the overlay channel; the admin UI reloads the list.
/// Not kept in snapshots.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedemptionChangedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: RedemptionChangedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: RedemptionChangedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum RedemptionChangedType {
    #[serde(rename = "redemption.changed")]
    RedemptionChanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RedemptionChangedPayload {
    /// The record's ID, as in `RedemptionRecord.id`.
    id: String,
}

fn publish_changed(bus: &EventBus, r: &Redemption) {
    publish_changed_id(bus, r.id.0.to_string());
}

fn publish_changed_id(bus: &EventBus, id: String) {
    let payload = RedemptionChangedPayload { id };
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: CHANGED,
        version: CHANGED_VERSION,
        occurred_at: time::millis(Timestamp::now()),
        // Simulated redemptions are never stored, so every change comes from Twitch.
        source: Source::Twitch,
        payload: serde_json::to_value(payload).expect("serializable"),
    });
}

/// Starts recording into `pool`, which must already have [`db::MIGRATOR`] and the outbox's migrations applied,
/// announces changes on `bus` and publishes new records to `outbox`; needs a Tokio runtime.
pub fn start(
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    outbox: Arc<Outbox>,
) -> RedemptionService {
    RedemptionService::start(
        db::SqliteRedemptions::new(pool, outbox),
        modules,
        move |r| publish_changed(&bus, r),
    )
}

/// What this module takes from the outbox: what each redemption played, kept with its record and announced on
/// `bus` so the redemption list shows it.
pub fn subscriptions(pool: SqlitePool, bus: Arc<EventBus>) -> Vec<Subscription> {
    vec![Subscription {
        module: modules::REDEMPTION,
        event_type: REDEMPTION_PLAYED,
        handler: Arc::new(PlaybackRecords { pool, bus }),
    }]
}

struct PlaybackRecords {
    pool: SqlitePool,
    bus: Arc<EventBus>,
}

impl Subscriber for PlaybackRecords {
    /// The event ID keeps a redelivered event from being kept twice.
    fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a> {
        Box::pin(async move {
            let played: RedemptionPlayed =
                serde_json::from_value(event.payload.clone()).map_err(|e| e.to_string())?;
            let kept = db::record_played(&self.pool, &event.id, &played)
                .await
                .map_err(|e| e.to_string())?;
            // Announced once, when first kept; a redelivered event changes nothing.
            if kept
                && let Some(id) = db::record_id(&self.pool, &played.redemption_id)
                    .await
                    .map_err(|e| e.to_string())?
            {
                publish_changed_id(&self.bus, id);
            }
            Ok(())
        })
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(http::list_redemptions),
    components(schemas(RedemptionChangedEvent))
)]
struct RedemptionApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    RedemptionApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::{event, modules, pool, repo};
    use axum::{body::Body, http::Request};
    use std::time::Duration;
    use toolkit_core::events::{RedemptionChange, RedemptionStatus};
    use tower::ServiceExt;

    async fn wire(
        rx: &mut tokio::sync::broadcast::Receiver<toolkit_core::bus::Published>,
    ) -> serde_json::Value {
        let text = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn changes_are_announced_with_the_record_id_only() {
        let pool = pool().await;
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let service = start(
            pool.clone(),
            bus.clone(),
            modules("announce"),
            Outbox::new(pool.clone(), modules("outbox-a"), Vec::new()),
        );
        use RedemptionChange::*;
        use RedemptionStatus::*;
        service.receive(event("r-1", Added, Unfulfilled), Source::Twitch);
        let created = wire(&mut rx).await;
        // A repeat changes nothing and announces nothing; the status change does.
        service.receive(event("r-1", Added, Unfulfilled), Source::Twitch);
        service.receive(event("r-1", Updated, Fulfilled), Source::Twitch);
        let changed = wire(&mut rx).await;

        let id: String = sqlx::query_scalar("SELECT id FROM redemption_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        for wire in [&created, &changed] {
            assert_eq!(wire["type"], CHANGED);
            assert_eq!(wire["source"], "twitch");
            assert_eq!(wire["payload"], serde_json::json!({ "id": id }));
        }
        assert!(changed["seq"].as_u64() > created["seq"].as_u64());
        assert!(rx.try_recv().is_err(), "nothing else was announced");
        drop(service);
    }

    #[tokio::test]
    async fn playback_kept_is_announced_once_for_its_record() {
        use toolkit_core::presentation::{PlayResult, PlayedKind};
        let pool = pool().await;
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let service = start(
            pool.clone(),
            bus.clone(),
            modules("played"),
            Outbox::new(pool.clone(), modules("outbox-p"), Vec::new()),
        );
        service.receive(
            event(
                "r-1",
                RedemptionChange::Added,
                RedemptionStatus::Unfulfilled,
            ),
            Source::Twitch,
        );
        wire(&mut rx).await;
        let handler = subscriptions(pool.clone(), bus).remove(0).handler;
        let played = |redemption: &str| OutboxEvent {
            id: format!("e-{redemption}"),
            event_type: REDEMPTION_PLAYED.into(),
            payload: serde_json::to_value(RedemptionPlayed {
                redemption_id: redemption.into(),
                what: PlayedKind::Sound,
                result: PlayResult::Played,
            })
            .unwrap(),
            occurred_at: Timestamp::now(),
        };
        let twitch_id: String =
            sqlx::query_scalar("SELECT twitch_redemption_id FROM redemption_records")
                .fetch_one(&pool)
                .await
                .unwrap();
        handler.handle(&played(&twitch_id)).await.unwrap();
        let id: String = sqlx::query_scalar("SELECT id FROM redemption_records")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            wire(&mut rx).await["payload"],
            serde_json::json!({ "id": id })
        );
        // A redelivery, and a redemption that was never recorded, announce nothing.
        handler.handle(&played(&twitch_id)).await.unwrap();
        handler.handle(&played("t-unknown")).await.unwrap();
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err());
        drop(service);
    }

    #[tokio::test]
    async fn changed_event_type_matches_the_bus() {
        let pool = pool().await;
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let service = start(
            pool.clone(),
            bus,
            modules("wire"),
            Outbox::new(pool, modules("outbox-w"), Vec::new()),
        );
        service.receive(
            event(
                "r-1",
                RedemptionChange::Added,
                RedemptionStatus::Unfulfilled,
            ),
            Source::Twitch,
        );
        let wire = wire(&mut rx).await;
        // No missing, extra or renamed fields.
        let typed: RedemptionChangedEvent = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), wire);
        let doc = serde_json::to_value(openapi()).unwrap();
        for (schema, value) in [
            ("RedemptionChangedEvent", &wire),
            ("RedemptionChangedPayload", &wire["payload"]),
        ] {
            for key in doc["components"]["schemas"][schema]["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{schema} has a required list"))
            {
                let key = key.as_str().unwrap();
                assert!(
                    value.get(key).is_some(),
                    "{schema}.{key} is required but not sent"
                );
            }
        }
    }

    async fn get(pool: &SqlitePool, query: &str) -> (u16, serde_json::Value) {
        let service =
            RedemptionService::start(db::tests::repo(pool), db::tests::modules("http"), |_| {});
        let res = http::routes(pool.clone(), Arc::new(service))
            .oneshot(
                Request::get(format!("/api/admin/redemptions{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status().as_u16();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    fn names(page: &serde_json::Value) -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["user_name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Five records an hour apart; the newest is listed first.
    async fn seeded() -> SqlitePool {
        let pool = pool().await;
        let repo = repo(&pool);
        let rows = [
            (
                "r-1",
                "Alice",
                "alice",
                "Hydrate",
                RedemptionStatus::Unfulfilled,
            ),
            ("r-2", "Bob", "bob", "Hydrate", RedemptionStatus::Fulfilled),
            (
                "r-3",
                "小明",
                "xiaoming",
                "Song request",
                RedemptionStatus::Canceled,
            ),
            (
                "r-4",
                "Al_ice",
                "al_ice",
                "100% Song",
                RedemptionStatus::Unfulfilled,
            ),
            (
                "r-5",
                "Carol",
                "carol",
                "Hydrate",
                RedemptionStatus::Unknown,
            ),
        ];
        for (n, (id, name, login, reward, status)) in rows.into_iter().enumerate() {
            let mut e = event(id, RedemptionChange::Added, status);
            e.user_name = name.into();
            e.user_login = login.into();
            e.reward_title = reward.into();
            e.redeemed_at = format!("2026-11-16T{:02}:00:00Z", 10 + n);
            crate::application::record(&repo, e, Timestamp::now())
                .await
                .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn lists_newest_first_with_filters_and_pages() {
        let pool = seeded().await;
        let (status, all) = get(&pool, "").await;
        assert_eq!(status, 200);
        assert_eq!(all["total"], 5);
        assert_eq!(names(&all), ["Carol", "Al_ice", "小明", "Bob", "Alice"]);
        let first = &all["items"][4];
        assert_eq!(first["user_login"], "alice");
        assert_eq!(first["reward_title"], "Hydrate");
        assert_eq!(first["reward_cost"], 100);
        assert_eq!(
            first["user_input"], "<b>drink</b>",
            "raw; the UI escapes it"
        );
        assert_eq!(first["status"], "unfulfilled");
        assert_eq!(first["redeemed_at"], "2026-11-16T10:00:00.000Z");

        let cases = [
            // `to` is exclusive.
            (
                "?from=2026-11-16T11:00:00Z&to=2026-11-16T13:00:00Z",
                vec!["小明", "Bob"],
            ),
            // Offsets are honoured: 21:00 in UTC+8 is 13:00 UTC.
            ("?from=2026-11-16T21:00:00%2B08:00", vec!["Carol", "Al_ice"]),
            ("?viewer=ALI", vec!["Alice"]),
            ("?viewer=al_", vec!["Al_ice"]),
            ("?viewer=xiao", vec!["小明"]),
            ("?viewer=明", vec!["小明"]),
            ("?reward=song", vec!["Al_ice", "小明"]),
            ("?reward=100%25", vec!["Al_ice"]),
            ("?status=canceled", vec!["小明"]),
            ("?status=unknown", vec!["Carol"]),
            ("?reward=hydrate&status=fulfilled", vec!["Bob"]),
            (
                "?viewer=%20%20&reward=",
                vec!["Carol", "Al_ice", "小明", "Bob", "Alice"],
            ),
            ("?viewer=nobody", vec![]),
        ];
        for (query, expected) in cases {
            let (status, page) = get(&pool, query).await;
            assert_eq!(status, 200, "{query}");
            assert_eq!(names(&page), expected, "{query}");
            assert_eq!(page["total"], expected.len(), "{query}");
        }

        let (_, page) = get(&pool, "?limit=2&offset=2").await;
        assert_eq!(names(&page), ["小明", "Bob"]);
        assert_eq!(page["total"], 5, "total counts every page");
        let (_, page) = get(&pool, "?limit=2&offset=4").await;
        assert_eq!(names(&page), ["Alice"]);
        let (_, page) = get(&pool, "?reward=hydrate&limit=1&offset=1").await;
        assert_eq!(
            (names(&page), page["total"].as_u64()),
            (vec!["Bob".to_owned()], Some(3))
        );
    }

    #[tokio::test]
    async fn empty_database_lists_nothing() {
        let (status, page) = get(&pool().await, "").await;
        assert_eq!(status, 200);
        assert_eq!(
            page,
            serde_json::json!({ "items": [], "total": 0, "failed_writes": 0 })
        );
    }

    #[tokio::test]
    async fn bad_filters_are_rejected_with_a_code() {
        let pool = pool().await;
        for query in [
            "?from=yesterday",
            "?to=2026-11-16",
            "?status=refunded",
            "?limit=0",
            "?limit=101",
            "?offset=-1",
        ] {
            let (status, body) = get(&pool, query).await;
            assert_eq!(status, 400, "{query}");
            assert_eq!(body["code"], "redemption.invalid_filter", "{query}");
        }
    }
}
