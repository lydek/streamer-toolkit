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
        crate::application::test_commands(&dir),
    );
    full_card_sound(&dir);
    service
        .config()
        .update(
            &CheckinSettings {
                card_slots: 2,
                ..CheckinSettings::default()
            },
            toolkit_core::commands::Allowed::Everyone,
        )
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
        serde_json::json!({
            "userName": "<b>Viewer 1</b>", "avatarUrl": AVATAR, "stamps": 1, "slots": 2, "total": 1, "full": false,
            "durationMs": 80, "sound": null,
            // The default style: the name and the total filled in as raw text, for the overlay to show as text.
            "title": "<b>Viewer 1</b>", "description": "累計 1 次",
            "style": {
                "preset": "simple", "backgroundColor": "#181624", "backgroundUrl": null, "stampColor": "#d8b7ff",
                "stampUrl": null, "perRow": 10, "animation": "slide"
            }
        })
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
        crate::application::test_commands(&dir),
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

/// Waits until the recorded outcomes, oldest first, pass `done`.
async fn until(
    service: &CheckinService,
    done: impl Fn(&[Option<toolkit_core::presentation::Outcome>]) -> bool,
) {
    for _ in 0..300 {
        let outcomes: Vec<_> = service
            .recent()
            .into_iter()
            .rev()
            .map(|r| r.outcome)
            .collect();
        if done(&outcomes) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("never got there: {:?}", service.recent());
}

#[tokio::test]
async fn stamp_cards_record_what_became_of_each() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use toolkit_core::presentation::{Outcome, Subject};
    let pool = pool().await;
    let dir = data_dir("recent-cards");
    let open = Arc::new(AtomicBool::new(true));
    let on_air = open.clone();
    let presenter = Presenter::start(GAP, move |_| on_air.load(Ordering::Relaxed));
    let service = start(
        pool.clone(),
        Arc::new(EventBus::default()),
        Arc::new(Modules::load(dir.clone())),
        dir.clone(),
        Arc::new(NoProfiles),
        sounds(&dir),
        Timing {
            show: std::time::Duration::from_millis(400),
        },
        presenter.clone(),
        crate::application::test_commands(&dir),
    );
    let outcomes = || {
        service
            .recent()
            .into_iter()
            .rev()
            .map(|r| r.outcome)
            .collect::<Vec<_>>()
    };
    let check_in = |user: &str| {
        service.receive_chat(&chat(user, "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch)
    };

    check_in("1");
    until(&service, |o| o == [Some(Outcome::Played)]).await;
    open.store(false, Ordering::Relaxed);
    check_in("2");
    until(&service, |o| o.len() == 2 && o[1] == Some(Outcome::OffAir)).await;
    open.store(true, Ordering::Relaxed);
    for user in ["3", "4", "5"] {
        check_in(user);
    }
    until(&service, |o| o.len() == 5).await;
    assert_eq!(&outcomes()[2..], [None, None, None], "queued");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    presenter.skip(|_| {});
    presenter.clear();
    check_in("6");
    until(&service, |o| o.len() == 6).await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    presenter.stop(|_, _| {});
    until(&service, |o| o.iter().all(Option::is_some)).await;
    assert_eq!(
        outcomes(),
        [
            Some(Outcome::Played),
            Some(Outcome::OffAir),
            Some(Outcome::Skipped),
            Some(Outcome::Cleared),
            Some(Outcome::Cleared),
            Some(Outcome::Stopped),
        ]
    );
    assert_eq!(
        service.recent().last().unwrap().subject,
        Subject::Card {
            viewer: "<b>Viewer 1</b>".into(),
            stamps: 1,
            slots: 10
        }
    );

    // A simulated card is listed too, marked as such.
    assert!(service.simulate_card("Kai", 3));
    assert_eq!(service.recent()[0].source, Source::Simulation);
}

#[tokio::test]
async fn a_pushed_card_shows_without_a_record_and_hide_clears_it() {
    use crate::{
        application::CheckinRepository,
        domain::{BusinessDay, CheckIn, Trigger},
    };
    let pool = pool().await;
    let dir = data_dir("cards-push");
    let bus = Arc::new(EventBus::default());
    let (_, mut rx) = bus.subscribe();
    let service = Arc::new(start_quiet(
        pool.clone(),
        bus,
        Arc::new(Modules::load(dir.clone())),
        dir.clone(),
    ));
    let at = DateTime::parse_from_rfc3339("2026-11-20T01:00:00Z")
        .unwrap()
        .to_utc();
    let rule = BusinessDay::new("Asia/Taipei", 0).unwrap();
    let viewer = Viewer {
        twitch_id: TwitchUserId("1".into()),
        login: "ann".into(),
        name: "Ann".into(),
    };
    db::SqliteCheckins::new(pool.clone())
        .check_in(&viewer, &CheckIn::at(&rule, at, Trigger::Chat, at))
        .await
        .unwrap();
    let id: String = sqlx::query_scalar("SELECT id FROM checkin_viewers")
        .fetch_one(&pool)
        .await
        .unwrap();
    let app = http::routes(pool.clone(), service.clone());
    let post = |uri: String| {
        let app = app.clone();
        async move {
            let req = Request::post(uri).body(Body::empty()).unwrap();
            app.oneshot(req).await.unwrap().status().as_u16()
        }
    };

    assert_eq!(
        post(format!("/api/admin/checkin/viewers/{id}/card")).await,
        204
    );
    let (card, _) = next_card(&mut rx).await;
    assert_eq!(card["payload"]["userName"], "Ann");
    assert_eq!(
        (&card["payload"]["stamps"], &card["payload"]["total"]),
        (&1.into(), &1.into())
    );
    assert_eq!(count(&pool).await, 1, "a push records no check-in");
    assert!(
        service.recent().is_empty(),
        "nor does the event feed list it"
    );
    assert_eq!(
        post("/api/admin/checkin/viewers/nobody/card".into()).await,
        404
    );

    assert_eq!(post("/api/admin/checkin/card/hide".into()).await, 204);
    let hidden = loop {
        let text = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let wire: serde_json::Value = serde_json::from_str(&text).unwrap();
        if wire["type"] == "checkin.hidden" {
            break wire;
        }
    };
    let typed: CheckinHiddenEvent = serde_json::from_value(hidden.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), hidden);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_repeat_check_in_shows_its_card_only_when_the_style_asks_and_records_nothing() {
    let pool = pool().await;
    let dir = data_dir("cards-repeat");
    let bus = Arc::new(EventBus::default());
    let (_, mut rx) = bus.subscribe();
    let service = Arc::new(start_quiet(
        pool.clone(),
        bus,
        Arc::new(Modules::load(dir.clone())),
        dir.clone(),
    ));
    service.receive_chat(&chat("1", "!打卡", "2026-11-16T01:00:00Z"), Source::Twitch);
    let (first, _) = next_card(&mut rx).await;
    assert_eq!(first["payload"]["total"], 1);

    // Off by default: a repeat shows nothing.
    service.receive_chat(&chat("1", "!打卡", "2026-11-16T02:00:00Z"), Source::Twitch);
    let quiet =
        tokio::time::timeout(std::time::Duration::from_millis(300), next_card(&mut rx)).await;
    assert!(quiet.is_err());

    let style = toolkit_core::settings::CardStyle {
        show_repeat: true,
        ..Default::default()
    };
    service.config().set_style(style).unwrap();
    service.receive_chat(&chat("1", "!打卡", "2026-11-16T03:00:00Z"), Source::Twitch);
    let (again, _) = next_card(&mut rx).await;
    assert_eq!(again["payload"]["total"], 1, "the card as it stands");
    assert_eq!(count(&pool).await, 1, "a repeat records nothing");
    let _ = std::fs::remove_dir_all(&dir);
}
