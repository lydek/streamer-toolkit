//! Adapters: the SQLite repository, the admin API, the outbox subscription and the `checkin.changed` screen event.

pub(crate) mod db;
pub(crate) mod http;

use std::{path::PathBuf, sync::Arc};

use chrono::{SecondsFormat, Utc};
use redemption::{REDEMPTION_RECORDED, RedemptionRecorded};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::{self, Modules},
    outbox::{Handled, OutboxEvent, Subscriber, Subscription},
};
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    application::{
        self, Card, CheckedIn, CheckinService, Config, OnCheckedIn, ProfileImages, Redeemed,
        Screen, ShowCard, Timing,
    },
    domain::{TwitchUserId, Viewer},
};

const STAMPED: &str = "checkin.stamped";
const STAMPED_VERSION: u32 = 1;

/// A `checkin.stamped` event exactly as `/ws/overlay` sends it: a stamp card to show on `/overlay/checkin/`. The
/// Host sends them one at a time. Not kept in snapshots, so a reloaded overlay never replays one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStampedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CheckinStampedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CheckinStampedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CheckinStampedType {
    #[serde(rename = "checkin.stamped")]
    CheckinStamped,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStampedPayload {
    /// Raw viewer text; overlays escape it when rendering.
    pub user_name: String,
    /// Twitch profile image (`https://static-cdn.jtvnw.net/` only); `null` when unknown.
    #[schema(required)]
    pub avatar_url: Option<String>,
    /// Stamps on the current card, out of `slots`.
    pub stamps: u32,
    pub slots: u32,
    pub total: u64,
    /// This check-in filled the card.
    pub full: bool,
    /// How long to show the card.
    pub duration_ms: u64,
}

fn publish_card(bus: &EventBus, card: &Card, show: std::time::Duration) {
    let payload = CheckinStampedPayload {
        user_name: card.name.clone(),
        avatar_url: card.avatar.clone(),
        stamps: card.stamps,
        slots: card.slots,
        total: card.total,
        full: card.full(),
        duration_ms: u64::try_from(show.as_millis()).unwrap_or(u64::MAX),
    };
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: STAMPED,
        version: STAMPED_VERSION,
        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        source: card.source,
        payload: serde_json::to_value(payload).expect("serializable"),
    });
}

const CHANGED: &str = "checkin.changed";
const CHANGED_VERSION: u32 = 1;

/// A `checkin.changed` event exactly as `/ws/overlay` sends it: a viewer checked in. It carries only the viewer's
/// ID, so viewer text stays off this event; the admin UI reloads its lists. Not kept in snapshots.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckinChangedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CheckinChangedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CheckinChangedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CheckinChangedType {
    #[serde(rename = "checkin.changed")]
    CheckinChanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CheckinChangedPayload {
    /// The viewer's ID, as in `CheckinViewer.id`.
    viewer_id: String,
}

fn publish_changed(bus: &EventBus, c: &CheckedIn) {
    bus.publish(Event {
        id: Uuid::now_v7().to_string(),
        event_type: CHANGED,
        version: CHANGED_VERSION,
        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        // Simulated check-ins are never recorded, so every recorded one is real.
        source: Source::Twitch,
        payload: serde_json::to_value(CheckinChangedPayload {
            viewer_id: c.viewer_id.clone(),
        })
        .expect("serializable"),
    });
}

/// Starts check-ins into `pool`, which must already have [`db::MIGRATOR`] applied, with the settings in
/// `data_dir`. Each recorded check-in is announced on `bus` and gets a stamp card on the overlay, with the viewer's
/// profile image from `profiles`; needs a Tokio runtime.
pub fn start(
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    data_dir: PathBuf,
    profiles: Arc<dyn ProfileImages>,
    timing: Timing,
) -> CheckinService {
    let changed_bus = bus.clone();
    let changed: OnCheckedIn = Arc::new(move |c| publish_changed(&changed_bus, c));
    let show_card: ShowCard = Arc::new(move |card, show| publish_card(&bus, card, show));
    CheckinService::start(
        db::SqliteCheckins::new(pool),
        modules,
        Arc::new(Config::load(data_dir)),
        Screen {
            changed,
            show_card,
            profiles,
            timing,
        },
    )
}

/// What this module takes from the outbox: redemptions, to check in with the configured reward.
pub fn subscriptions(pool: SqlitePool, service: &CheckinService) -> Vec<Subscription> {
    vec![Subscription {
        module: modules::CHECKIN,
        event_type: REDEMPTION_RECORDED,
        handler: Arc::new(RedemptionCheckins {
            repo: db::SqliteCheckins::new(pool),
            config: service.config().clone(),
            notify: service.on_checked_in().clone(),
        }),
    }]
}

struct RedemptionCheckins {
    repo: db::SqliteCheckins,
    config: Arc<Config>,
    notify: OnCheckedIn,
}

impl Subscriber for RedemptionCheckins {
    fn handle<'a>(&'a self, event: &'a OutboxEvent) -> Handled<'a> {
        Box::pin(async move {
            let r: RedemptionRecorded =
                serde_json::from_value(event.payload.clone()).map_err(|e| e.to_string())?;
            let redeemed = Redeemed {
                event_id: event.id.clone(),
                viewer: Viewer {
                    twitch_id: TwitchUserId(r.user_id),
                    login: r.user_login,
                    name: r.user_name,
                },
                reward_id: r.reward_id,
                occurred_at: event.occurred_at,
            };
            let rules = self.config.rules();
            let outcome = application::redeemed(&self.repo, &rules, &redeemed, Utc::now())
                .await
                .map_err(|e| e.to_string())?;
            if let Some(c) =
                outcome.and_then(|o| application::checked_in(&redeemed.viewer, &o, &rules))
            {
                (self.notify)(&c);
            }
            Ok(())
        })
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        http::checkin_settings,
        http::set_checkin_settings,
        http::list_viewers,
        http::today,
        http::manual_check_in
    ),
    components(schemas(CheckinChangedEvent, CheckinStampedEvent))
)]
struct CheckinApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    CheckinApi::openapi()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Knows no profile images.
    pub(crate) struct NoProfiles;
    impl ProfileImages for NoProfiles {
        fn profile_image<'a>(
            &'a self,
            _: &'a str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>>
        {
            Box::pin(async { None })
        }
    }

    /// A screen that shows nothing.
    pub(crate) fn quiet_screen() -> Screen {
        Screen {
            changed: Arc::new(|_| {}),
            show_card: Arc::new(|_, _| {}),
            profiles: Arc::new(NoProfiles),
            timing: Timing::default(),
        }
    }

    /// `start` with a bus of its own, no profile images and default timing.
    fn start_quiet(
        pool: SqlitePool,
        bus: Arc<EventBus>,
        modules: Arc<Modules>,
        dir: PathBuf,
    ) -> CheckinService {
        start(
            pool,
            bus,
            modules,
            dir,
            Arc::new(NoProfiles),
            Timing::default(),
        )
    }
    use crate::infrastructure::db::tests::{chat, count, data_dir, pool, wait_for_count};
    use axum::{body::Body, http::Request};
    use chrono::DateTime;
    use toolkit_core::{events::Source, outbox::Outbox, settings::CheckinSettings};
    use tower::ServiceExt;

    struct Wired {
        modules: Arc<Modules>,
        pool: SqlitePool,
        service: CheckinService,
        outbox: Arc<Outbox>,
        dir: PathBuf,
    }

    /// The check-in module subscribed to a real outbox, with reward `w-1` checking in.
    async fn wired(name: &str) -> Wired {
        let pool = pool().await;
        let dir = data_dir(name);
        let modules = Arc::new(Modules::load(dir.clone()));
        let service = start_quiet(
            pool.clone(),
            Arc::new(EventBus::default()),
            modules.clone(),
            dir.clone(),
        );
        service
            .config()
            .update(&CheckinSettings {
                reward_id: Some("w-1".into()),
                ..CheckinSettings::default()
            })
            .unwrap();
        let outbox = Outbox::new(
            pool.clone(),
            modules.clone(),
            subscriptions(pool.clone(), &service),
        );
        Wired {
            modules,
            pool,
            service,
            outbox,
            dir,
        }
    }

    /// Publishes a redemption of `reward` by viewer `user`, redeemed at `at`, as the redemption module does.
    async fn redeem(w: &Wired, event_id: &str, user: &str, reward: &str, at: &str) {
        let payload = RedemptionRecorded {
            redemption_id: format!("rec-{event_id}"),
            twitch_redemption_id: format!("tw-{event_id}"),
            user_id: user.into(),
            user_login: format!("login{user}"),
            user_name: format!("Viewer {user}"),
            reward_id: reward.into(),
            reward_title: "Check in".into(),
        };
        let event = OutboxEvent {
            id: event_id.into(),
            event_type: REDEMPTION_RECORDED.into(),
            payload: serde_json::to_value(payload).unwrap(),
            occurred_at: DateTime::parse_from_rfc3339(at).unwrap().to_utc(),
        };
        let mut tx = w.pool.begin().await.unwrap();
        w.outbox.publish(&mut tx, &event).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn rows(pool: &SqlitePool) -> Vec<(String, String, String)> {
        sqlx::query_as(
            "SELECT v.twitch_user_id, r.business_day, r.trigger_kind FROM checkin_records r              JOIN checkin_viewers v ON v.id = r.viewer_id ORDER BY r.occurred_at",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn redeeming_the_reward_checks_in_for_the_day_it_happened() {
        let w = wired("redeem-day").await;
        // Redeemed at 23:50 in Taipei on 11-16, delivered (say, after a restart) the next day.
        redeem(&w, "e1", "7", "w-1", "2026-11-16T15:50:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(
            rows(&w.pool).await,
            [("7".into(), "2026-11-16".into(), "redemption".into())]
        );
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    #[tokio::test]
    async fn one_check_in_per_redemption_and_per_day() {
        let w = wired("redeem-once").await;
        redeem(&w, "e1", "7", "w-1", "2026-11-16T01:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        // Delivered again, e.g. after a crash before the delivery was marked.
        sqlx::query("UPDATE outbox_deliveries SET state = 'pending'")
            .execute(&w.pool)
            .await
            .unwrap();
        w.outbox.dispatch(Utc::now()).await.unwrap();
        // A second redemption the same business day.
        redeem(&w, "e2", "7", "w-1", "2026-11-16T05:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(count(&w.pool).await, 1);
        let delivered: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM outbox_deliveries WHERE state = 'delivered'")
                .fetch_one(&w.pool)
                .await
                .unwrap();
        assert_eq!(delivered, 2, "a repeat is handled, not retried");
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    #[tokio::test]
    async fn chat_then_redemption_on_the_same_day_counts_once() {
        let w = wired("redeem-after-chat").await;
        w.service
            .receive_chat(&chat("7", "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
        wait_for_count(&w.pool, 1).await;
        redeem(&w, "e1", "7", "w-1", "2026-11-16T02:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(
            rows(&w.pool).await,
            [("7".into(), "2026-11-16".into(), "chat".into())]
        );
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    async fn delivery_states(pool: &SqlitePool) -> Vec<(String, String, i64)> {
        sqlx::query_as("SELECT event_id, state, attempts FROM outbox_deliveries ORDER BY event_id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn paused_keeps_redemptions_and_applies_them_in_order_after_enabling() {
        let w = wired("paused").await;
        w.modules.set_enabled(modules::CHECKIN, false).unwrap();
        // 23:00 on 11-16 and 00:30 on 11-17 in Taipei, by two viewers and one viewer twice.
        redeem(&w, "e1", "7", "w-1", "2026-11-16T15:00:00Z").await;
        redeem(&w, "e2", "8", "w-1", "2026-11-16T15:10:00Z").await;
        redeem(&w, "e3", "7", "w-1", "2026-11-16T16:30:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(count(&w.pool).await, 0);
        let backlog = w.outbox.backlog(modules::CHECKIN).await.unwrap();
        assert_eq!(backlog.pending, 3);
        assert_eq!(
            backlog.oldest,
            Some(
                DateTime::parse_from_rfc3339("2026-11-16T15:00:00Z")
                    .unwrap()
                    .to_utc()
            )
        );
        assert!(
            delivery_states(&w.pool).await.iter().all(|d| d.2 == 0),
            "no retries counted while paused"
        );

        w.modules.set_enabled(modules::CHECKIN, true).unwrap();
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(
            rows(&w.pool).await,
            [
                ("7".into(), "2026-11-16".into(), "redemption".into()),
                ("8".into(), "2026-11-16".into(), "redemption".into()),
                ("7".into(), "2026-11-17".into(), "redemption".into()),
            ],
            "in order, each on the day it was redeemed"
        );
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    #[tokio::test]
    async fn closed_skips_redemptions_from_the_closed_period() {
        let w = wired("closed").await;
        // Paused first, then switched to closed: what was kept is skipped too.
        w.modules.set_enabled(modules::CHECKIN, false).unwrap();
        redeem(&w, "e1", "7", "w-1", "2026-11-16T01:00:00Z").await;
        w.modules.close(modules::CHECKIN).unwrap();
        redeem(&w, "e2", "8", "w-1", "2026-11-16T02:00:00Z").await;
        // Enabled again before the dispatcher ran: the host skips what is left first.
        w.outbox.skip_pending(modules::CHECKIN).await.unwrap();
        w.modules.set_enabled(modules::CHECKIN, true).unwrap();
        redeem(&w, "e3", "9", "w-1", "2026-11-16T03:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(
            rows(&w.pool).await,
            [("9".into(), "2026-11-16".into(), "redemption".into())]
        );
        let states: Vec<String> = delivery_states(&w.pool)
            .await
            .into_iter()
            .map(|d| d.1)
            .collect();
        assert_eq!(states, ["skipped", "skipped", "delivered"]);
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    #[tokio::test]
    async fn the_dispatcher_skips_while_closed() {
        let w = wired("closed-dispatch").await;
        w.modules.close(modules::CHECKIN).unwrap();
        redeem(&w, "e1", "7", "w-1", "2026-11-16T01:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(delivery_states(&w.pool).await[0].1, "skipped");
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    #[tokio::test]
    async fn other_rewards_or_no_reward_never_check_in() {
        let w = wired("redeem-other").await;
        redeem(&w, "e1", "7", "w-other", "2026-11-16T01:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        w.service
            .config()
            .update(&CheckinSettings::default())
            .unwrap();
        redeem(&w, "e2", "7", "w-1", "2026-11-16T01:00:00Z").await;
        w.outbox.dispatch(Utc::now()).await.unwrap();
        assert_eq!(count(&w.pool).await, 0);
        let _ = std::fs::remove_dir_all(&w.dir);
    }

    async fn send(
        app: &axum::Router,
        method: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri("/api/admin/checkin/settings")
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn settings_are_validated_saved_and_applied() {
        let dir = data_dir("http-settings");
        let modules = Arc::new(Modules::load(dir.clone()));
        let pool = pool().await;
        let service = start_quiet(
            pool.clone(),
            Arc::new(EventBus::default()),
            modules,
            dir.clone(),
        );
        let app = http::routes(pool, Arc::new(service));

        let (status, body) = send(&app, "GET", None).await;
        assert_eq!(status, 200);
        assert_eq!(
            body,
            serde_json::json!({ "command": "打卡", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": 10, "reward_id": null })
        );

        let next = serde_json::json!({ "command": " !Sign ", "time_zone": "Europe/Berlin", "reset_hour": 6, "card_slots": 8 });
        let (status, body) = send(&app, "PUT", Some(next)).await;
        assert_eq!(status, 200);
        assert_eq!(body["command"], "Sign", "stored without its !");
        let saved = toolkit_core::settings::load(&dir).checkin;
        assert_eq!(
            (
                saved.command.as_str(),
                saved.time_zone.as_str(),
                saved.reset_hour,
                saved.card_slots
            ),
            ("Sign", "Europe/Berlin", 6, 8)
        );

        for bad in [
            serde_json::json!({ "command": "two words", "time_zone": "UTC", "reset_hour": 0, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "Nowhere/City", "reset_hour": 0, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "UTC", "reset_hour": 24, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "UTC", "reset_hour": 0, "card_slots": 0 }),
        ] {
            let (status, body) = send(&app, "PUT", Some(bad.clone())).await;
            assert_eq!(
                (status, &body["code"]),
                (400, &serde_json::json!("checkin.invalid_settings")),
                "{bad}"
            );
        }
        let (_, body) = send(&app, "GET", None).await;
        assert_eq!(body["command"], "Sign", "a refused update changes nothing");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A Host-like setup: the module's routes and its bus, with check-ins recorded straight into the database.
    struct Api {
        pool: SqlitePool,
        app: axum::Router,
        bus: Arc<EventBus>,
        service: Arc<CheckinService>,
        dir: PathBuf,
    }

    async fn api(name: &str) -> Api {
        let pool = pool().await;
        let dir = data_dir(name);
        let bus = Arc::new(EventBus::default());
        let service = Arc::new(start_quiet(
            pool.clone(),
            bus.clone(),
            Arc::new(Modules::load(dir.clone())),
            dir.clone(),
        ));
        let app = http::routes(pool.clone(), service.clone());
        Api {
            pool,
            app,
            bus,
            service,
            dir,
        }
    }

    async fn seed(pool: &SqlitePool, user: &str, name: &str, at: &str) {
        use crate::{
            application::CheckinRepository,
            domain::{BusinessDay, CheckIn, Trigger},
        };
        let rule = BusinessDay::new("Asia/Taipei", 0).unwrap();
        let at = DateTime::parse_from_rfc3339(at).unwrap().to_utc();
        let viewer = Viewer {
            twitch_id: TwitchUserId(user.into()),
            login: format!("login{user}"),
            name: name.into(),
        };
        db::SqliteCheckins::new(pool.clone())
            .check_in(&viewer, &CheckIn::at(&rule, at, Trigger::Chat, at))
            .await
            .unwrap();
    }

    async fn call(
        app: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    fn names(page: &serde_json::Value) -> Vec<&str> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["user_name"].as_str().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn viewers_are_listed_with_totals_cards_and_search() {
        let a = api("viewers").await;
        // Ann checks in on three days, Bob once (latest), <b>Cat</b> once (earliest).
        for at in [
            "2026-11-10T01:00:00Z",
            "2026-11-11T01:00:00Z",
            "2026-11-12T01:00:00Z",
        ] {
            seed(&a.pool, "1", "Ann", at).await;
        }
        seed(&a.pool, "2", "Bob", "2026-11-14T01:00:00Z").await;
        seed(&a.pool, "3", "<b>Cat</b>", "2026-11-01T01:00:00Z").await;

        let (status, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
        assert_eq!(status, 200);
        assert_eq!(page["total"], 3);
        assert_eq!(
            names(&page),
            ["Bob", "Ann", "<b>Cat</b>"],
            "latest check-in first; text raw"
        );
        let ann = &page["items"][1];
        assert_eq!(
            (&ann["total"], &ann["stamps"], &ann["card_slots"]),
            (&3.into(), &3.into(), &10.into())
        );
        assert_eq!(ann["last_check_in"], "2026-11-12");
        assert_eq!(ann["user_login"], "login1");

        let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=AN", None).await;
        assert_eq!(names(&page), ["Ann"]);
        let (_, page) = call(
            &a.app,
            "GET",
            "/api/admin/checkin/viewers?search=login3",
            None,
        )
        .await;
        assert_eq!(names(&page), ["<b>Cat</b>"]);
        let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=%25", None).await;
        assert_eq!(page["total"], 0, "% is literal");
        let (_, page) = call(
            &a.app,
            "GET",
            "/api/admin/checkin/viewers?limit=1&offset=1",
            None,
        )
        .await;
        assert_eq!((names(&page), &page["total"]), (vec!["Ann"], &3.into()));
        for bad in ["?limit=0", "?limit=101", "?offset=-1"] {
            let (status, body) = call(
                &a.app,
                "GET",
                &format!("/api/admin/checkin/viewers{bad}"),
                None,
            )
            .await;
            assert_eq!(
                (status, &body["code"]),
                (400, &"checkin.invalid_query".into()),
                "{bad}"
            );
        }

        // A new card size recomputes every card at once.
        let settings = serde_json::json!({ "command": "打卡", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": 2, "reward_id": null });
        assert_eq!(
            call(&a.app, "PUT", "/api/admin/checkin/settings", Some(settings))
                .await
                .0,
            200
        );
        let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=Ann", None).await;
        assert_eq!(
            (&page["items"][0]["stamps"], &page["items"][0]["card_slots"]),
            (&1.into(), &2.into())
        );
        let _ = std::fs::remove_dir_all(&a.dir);
    }

    #[tokio::test]
    async fn manual_check_ins_follow_the_one_per_day_rule() {
        let a = api("manual").await;
        let (_, mut rx) = a.bus.subscribe();
        seed(&a.pool, "1", "Ann", "2026-11-01T01:00:00Z").await;
        let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
        let id = page["items"][0]["id"].as_str().unwrap().to_owned();
        let uri = format!("/api/admin/checkin/viewers/{id}/check-ins");

        let today = serde_json::json!({});
        assert_eq!(
            call(&a.app, "POST", &uri, Some(today.clone())).await,
            (200, serde_json::json!({ "result": "checked_in" }))
        );
        assert_eq!(
            call(&a.app, "POST", &uri, Some(today)).await.1["result"],
            "already_checked_in"
        );
        // Back-fill a past day, then the same day again.
        let past = serde_json::json!({ "date": "2026-11-05" });
        assert_eq!(
            call(&a.app, "POST", &uri, Some(past.clone())).await.1["result"],
            "checked_in"
        );
        assert_eq!(
            call(&a.app, "POST", &uri, Some(past)).await.1["result"],
            "already_checked_in"
        );
        assert_eq!(count(&a.pool).await, 3);
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT trigger_kind FROM checkin_records ORDER BY business_day")
                .fetch_all(&a.pool)
                .await
                .unwrap();
        assert_eq!(kinds, ["chat", "manual", "manual"]);

        for (date, code) in [
            ("2026-13-01", "checkin.invalid_date"),
            ("2999-01-01", "checkin.invalid_date"),
        ] {
            let (status, body) = call(
                &a.app,
                "POST",
                &uri,
                Some(serde_json::json!({ "date": date })),
            )
            .await;
            assert_eq!((status, &body["code"]), (400, &code.into()), "{date}");
        }
        let (status, body) = call(
            &a.app,
            "POST",
            "/api/admin/checkin/viewers/nobody/check-ins",
            Some(serde_json::json!({})),
        )
        .await;
        assert_eq!(
            (status, &body["code"]),
            (404, &"checkin.viewer_not_found".into())
        );

        // Each recorded one is announced with the viewer ID only; repeats are not.
        let mut changes = Vec::new();
        while let Ok(text) = rx.try_recv() {
            let wire: serde_json::Value = serde_json::from_str(&text).unwrap();
            if wire["type"] == CHANGED {
                changes.push(wire);
            }
        }
        assert_eq!(changes.len(), 2, "repeats are not announced");
        for wire in changes {
            assert_eq!(wire["payload"], serde_json::json!({ "viewer_id": id }));
            let typed: CheckinChangedEvent = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(
                serde_json::to_value(typed).unwrap(),
                wire,
                "the schema type matches the bus"
            );
        }
        let _ = std::fs::remove_dir_all(&a.dir);
    }

    #[tokio::test]
    async fn today_counts_the_current_business_day() {
        let a = api("today").await;
        let now = Utc::now();
        let today = a.service.config().rules().day.day_of(now);
        seed(&a.pool, "1", "Ann", &now.to_rfc3339()).await;
        seed(
            &a.pool,
            "2",
            "Bob",
            &(now + chrono::Duration::milliseconds(1)).to_rfc3339(),
        )
        .await;
        seed(&a.pool, "3", "Old", "2026-01-01T01:00:00Z").await;
        let (status, body) = call(&a.app, "GET", "/api/admin/checkin/today", None).await;
        assert_eq!(status, 200);
        assert_eq!(body["business_day"], today.to_string());
        assert_eq!(body["count"], 2);
        let recent: Vec<&str> = body["recent"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["user_name"].as_str().unwrap())
            .collect();
        assert_eq!(recent, ["Bob", "Ann"]);
        let _ = std::fs::remove_dir_all(&a.dir);
    }

    /// Answers every lookup with `url`, counting the lookups.
    struct FixedProfile {
        url: &'static str,
        lookups: std::sync::atomic::AtomicUsize,
    }
    impl ProfileImages for FixedProfile {
        fn profile_image<'a>(
            &'a self,
            _: &'a str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>>
        {
            self.lookups
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(async { Some(self.url.to_owned()) })
        }
    }

    async fn next_card(
        rx: &mut tokio::sync::broadcast::Receiver<Arc<str>>,
    ) -> (serde_json::Value, std::time::Instant) {
        loop {
            let text = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            let wire: serde_json::Value = serde_json::from_str(&text).unwrap();
            if wire["type"] == STAMPED {
                return (wire, std::time::Instant::now());
            }
        }
    }

    const AVATAR: &str =
        "https://static-cdn.jtvnw.net/jtv_user_pictures/x-profile_image-300x300.png";

    fn quick() -> Timing {
        Timing {
            show: std::time::Duration::from_millis(80),
            gap: std::time::Duration::from_millis(20),
        }
    }

    #[tokio::test]
    async fn each_check_in_shows_its_card_once_in_order() {
        let pool = pool().await;
        let dir = data_dir("cards");
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let profiles = Arc::new(FixedProfile {
            url: AVATAR,
            lookups: Default::default(),
        });
        let service = start(
            pool.clone(),
            bus.clone(),
            Arc::new(Modules::load(dir.clone())),
            dir.clone(),
            profiles.clone(),
            quick(),
        );
        service
            .config()
            .update(&CheckinSettings {
                card_slots: 2,
                ..CheckinSettings::default()
            })
            .unwrap();

        // Viewer 1 twice on one day (shown once), then on the next day (fills the 2-slot card); viewer 2 once.
        for (user, at) in [
            ("1", "2026-11-16T01:00:00Z"),
            ("1", "2026-11-16T02:00:00Z"),
            ("2", "2026-11-16T02:30:00Z"),
            ("1", "2026-11-17T01:00:00Z"),
        ] {
            service.receive_chat(&chat(user, "!打卡", at), Source::Twitch);
        }
        let (first, t1) = next_card(&mut rx).await;
        let (second, t2) = next_card(&mut rx).await;
        let (third, t3) = next_card(&mut rx).await;
        assert_eq!(first["source"], "twitch");
        assert_eq!(
            first["payload"],
            serde_json::json!({ "userName": "<b>Viewer 1</b>", "avatarUrl": AVATAR, "stamps": 1, "slots": 2, "total": 1, "full": false, "durationMs": 80 })
        );
        assert_eq!(second["payload"]["userName"], "<b>Viewer 2</b>");
        assert_eq!(
            (&third["payload"]["stamps"], &third["payload"]["full"]),
            (&2.into(), &true.into())
        );
        assert!(
            t2 - t1 >= quick().show + quick().gap && t3 - t2 >= quick().show + quick().gap,
            "one at a time"
        );
        assert_eq!(
            profiles.lookups.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "stored after the first lookup"
        );
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT profile_image_url FROM checkin_viewers WHERE twitch_user_id = '1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored.as_deref(), Some(AVATAR));

        let typed: CheckinStampedEvent = serde_json::from_value(first.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            first,
            "the schema type matches the bus"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn only_twitch_images_reach_the_overlay() {
        let pool = pool().await;
        let dir = data_dir("cards-cdn");
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let profiles = Arc::new(FixedProfile {
            url: "https://evil.example/x.png",
            lookups: Default::default(),
        });
        let service = start(
            pool.clone(),
            bus,
            Arc::new(Modules::load(dir.clone())),
            dir.clone(),
            profiles,
            quick(),
        );
        service.receive_chat(&chat("1", "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
        let (card, _) = next_card(&mut rx).await;
        assert_eq!(card["payload"]["avatarUrl"], serde_json::Value::Null);
        assert!(application::is_twitch_image(AVATAR));
        for bad in [
            "http://static-cdn.jtvnw.net/x.png",
            "https://static-cdn.jtvnw.net.evil/x",
            "https://static-cdn.jtvnw.net/a b",
        ] {
            assert!(!application::is_twitch_image(bad), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn simulated_cards_show_and_record_nothing() {
        let pool = pool().await;
        let dir = data_dir("cards-sim");
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let service = start_quiet(
            pool.clone(),
            bus,
            Arc::new(Modules::load(dir.clone())),
            dir.clone(),
        );
        assert!(!service.simulate_card("Sim", 0));
        assert!(!service.simulate_card("Sim", 11), "more than the 10 slots");
        assert!(service.simulate_card("<i>Sim</i>", 10));
        let (card, _) = next_card(&mut rx).await;
        assert_eq!(card["source"], "simulation");
        assert_eq!(card["payload"]["userName"], "<i>Sim</i>");
        assert_eq!(card["payload"]["full"], true);
        assert_eq!(count(&pool).await, 0);
        let viewers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkin_viewers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(viewers, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
