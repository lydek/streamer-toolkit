use super::*;

const SOUND_ID: &str = "0190a5b0-0000-7000-8000-000000000001";

/// Full cards play library sound `SOUND_ID` at 40%; normal check-ins stay silent.
fn full_card_sound(dir: &std::path::Path) {
    toolkit_core::settings::update(dir, |s| {
        s.sound.library.push(toolkit_core::settings::SoundEntry {
            id: SOUND_ID.into(),
            label: "Fanfare".into(),
            volume: 40,
            format: "ogg".into(),
            duration_ms: None,
        });
        s.sound.slots.insert("checkin.full".into(), SOUND_ID.into());
    })
    .unwrap();
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>> {
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

const AVATAR: &str = "https://static-cdn.jtvnw.net/jtv_user_pictures/x-profile_image-300x300.png";

fn quick() -> Timing {
    Timing {
        show: std::time::Duration::from_millis(80),
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
        sounds(&dir),
        quick(),
        presenter(),
    );
    full_card_sound(&dir);
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
        serde_json::json!({ "userName": "<b>Viewer 1</b>", "avatarUrl": AVATAR, "stamps": 1, "slots": 2, "total": 1, "full": false, "durationMs": 80, "sound": null })
    );
    assert_eq!(second["payload"]["userName"], "<b>Viewer 2</b>");
    assert_eq!(
        (&third["payload"]["stamps"], &third["payload"]["full"]),
        (&2.into(), &true.into())
    );
    assert_eq!(
        third["payload"]["sound"],
        serde_json::json!({ "url": format!("/sounds/{SOUND_ID}"), "volume": 40 })
    );
    assert!(
        t2 - t1 >= quick().show + GAP && t3 - t2 >= quick().show + GAP,
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
        sounds(&dir),
        quick(),
        presenter(),
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
    full_card_sound(&dir);
    assert!(!service.simulate_card("Sim", 0));
    assert!(!service.simulate_card("Sim", 11), "more than the 10 slots");
    assert!(service.simulate_card("<i>Sim</i>", 10));
    let (card, _) = next_card(&mut rx).await;
    assert_eq!(card["source"], "simulation");
    assert_eq!(card["payload"]["userName"], "<i>Sim</i>");
    assert_eq!(card["payload"]["full"], true);
    assert_eq!(
        card["payload"]["sound"]["volume"], 40,
        "simulated cards play the chosen sound too"
    );
    assert_eq!(count(&pool).await, 0);
    let viewers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkin_viewers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(viewers, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
