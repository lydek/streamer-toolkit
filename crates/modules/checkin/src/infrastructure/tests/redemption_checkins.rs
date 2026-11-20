use super::*;

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
