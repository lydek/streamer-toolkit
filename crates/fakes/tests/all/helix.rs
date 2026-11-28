//! The Helix calls the Host's admin pages make, through the real client (`integrations::twitch::helix`), against the
//! fake Twitch: the shapes the Host parses, and the state they change.

use std::{sync::Arc, time::Duration};

use fakes::twitch::{BROADCASTER_ID, FakeTwitch};
use integrations::{
    secrets::InMemory,
    tls,
    twitch::{
        auth::{TwitchAuth, fake_stored_token},
        eventsub::EventSub,
        helix::{Helix, HelixError},
    },
};
use serde_json::json;
use tokio::{net::TcpListener, sync::mpsc};
use toolkit_core::events::Contest;

async fn start(fake: &FakeTwitch) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(fake.clone().serve(listener));
    base
}

/// Signed in as the dev builds are, with every scope (投票與預測 needs the poll and prediction ones).
fn client(base: &str) -> Helix {
    let http = tls::https_client(tls::client_config().unwrap());
    let store = Arc::new(InMemory::with(fake_stored_token()));
    let auth = TwitchAuth::new(
        "fake-client".into(),
        &format!("http://{base}/oauth2"),
        http.clone(),
        store,
    );
    Helix::new(&format!("http://{base}/helix"), auth, http)
}

async fn helix(fake: &FakeTwitch) -> Helix {
    client(&start(fake).await)
}

/// The app's EventSub client on the fake, handing over every poll and prediction it reads; returns once connected.
async fn contests(fake: &FakeTwitch) -> (Helix, mpsc::UnboundedReceiver<Contest>) {
    let base = start(fake).await;
    let (tx, rx) = mpsc::unbounded_channel();
    let (up_tx, mut up) = mpsc::unbounded_channel();
    tokio::spawn(
        EventSub {
            url: format!("ws://{base}/eventsub"),
            helix: client(&base),
            tls: tls::client_config().unwrap(),
            deliver: Arc::new(|_| {}),
            deliver_alert: Arc::new(|_| {}),
            deliver_redemption: Arc::new(|_| {}),
            deliver_stream: Arc::new(|_| {}),
            deliver_contest: Arc::new(move |c| {
                let _ = tx.send(c);
            }),
            deliver_removal: Arc::new(|_, _| Box::pin(async {})),
            connected: Arc::new(move |c| {
                let _ = up_tx.send(c);
            }),
        }
        .run(),
    );
    let connected = tokio::time::timeout(Duration::from_secs(10), up.recv())
        .await
        .unwrap();
    assert_eq!(connected, Some(true));
    (client(&base), rx)
}

async fn next(rx: &mut mpsc::UnboundedReceiver<Contest>) -> Contest {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event")
        .unwrap()
}

fn refused(result: Result<impl std::fmt::Debug, HelixError>) -> u16 {
    match result {
        Err(HelixError::Refused(status)) => status,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn stream_info_is_read_changed_and_searched() {
    let fake = FakeTwitch::default();
    let helix = helix(&fake).await;
    let path = format!("/channels?broadcaster_id={BROADCASTER_ID}");

    let c = &helix.get_json(&path).await.unwrap()["data"][0];
    assert_eq!(
        (c["title"].as_str(), c["game_id"].as_str()),
        (Some("Fake stream"), Some("509658"))
    );
    assert_eq!(c["game_name"], "Just Chatting");
    assert!(c["tags"].is_array() && c["broadcaster_language"].is_string());

    let change = json!({"title": "新標題", "game_id": "26936", "tags": ["Music"], "broadcaster_language": "en"});
    helix.patch_json(&path, &change).await.unwrap();
    let c = &helix.get_json(&path).await.unwrap()["data"][0];
    assert_eq!(
        (
            c["title"].as_str(),
            c["game_name"].as_str(),
            c["broadcaster_language"].as_str()
        ),
        (Some("新標題"), Some("Music"), Some("en"))
    );
    assert_eq!(c["tags"], json!(["Music"]));
    assert_eq!(fake.snapshot().channel.title, "新標題");

    helix
        .patch_json(&path, &json!({"game_id": ""}))
        .await
        .unwrap();
    assert_eq!(
        helix.get_json(&path).await.unwrap()["data"][0]["game_name"],
        ""
    );
    assert_eq!(
        refused(helix.patch_json(&path, &json!({"game_id": "1"})).await),
        400
    );
    assert_eq!(
        refused(helix.patch_json(&path, &json!({"title": " "})).await),
        400
    );
    assert_eq!(
        refused(
            helix
                .patch_json(&path, &json!({"tags": ["two words"]}))
                .await
        ),
        400
    );
    assert_eq!(
        refused(
            helix
                .patch_json("/channels?broadcaster_id=7", &json!({"title": "x"}))
                .await
        ),
        401
    );

    let found = helix
        .get_json("/search/categories?first=20&query=just%20chat")
        .await
        .unwrap();
    assert_eq!(found["data"][0]["name"], "Just Chatting");
    assert!(found["data"][0]["id"].is_string() && found["data"][0]["box_art_url"].is_string());
    let none = helix
        .get_json("/search/categories?first=20&query=%E9%9F%B3")
        .await
        .unwrap();
    assert_eq!(none["data"], json!([]));

    let calls = fake.recorder().report();
    let keys: Vec<&str> = calls["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["key"].as_str())
        .collect();
    assert!(
        keys.contains(&"PATCH /helix/channels") && keys.contains(&"GET /helix/search/categories")
    );
}

#[tokio::test]
async fn rewards_are_managed_and_redemptions_marked_once() {
    let fake = FakeTwitch::default();
    let helix = helix(&fake).await;
    let path = format!("/channel_points/custom_rewards?broadcaster_id={BROADCASTER_ID}");
    let count = |v: &serde_json::Value| v["data"].as_array().unwrap().len();

    let all = helix.get_json(&path).await.unwrap();
    assert_eq!(count(&all), 3, "the preloaded rewards");
    for key in [
        "global_cooldown_setting",
        "max_per_stream_setting",
        "max_per_user_per_stream_setting",
    ] {
        assert!(all["data"][0][key]["is_enabled"].is_boolean(), "{key}");
    }
    let own = format!("{path}&only_manageable_rewards=true");
    assert_eq!(count(&helix.get_json(&own).await.unwrap()), 0);

    let made = helix
        .post_json_read(&path, &json!({"title": "Hug", "cost": 50, "is_global_cooldown_enabled": true, "global_cooldown_seconds": 30}))
        .await
        .unwrap();
    let id = made["data"][0]["id"].as_str().unwrap().to_owned();
    assert!(
        id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "{id}"
    );
    assert_eq!(
        made["data"][0]["global_cooldown_setting"]["global_cooldown_seconds"],
        30
    );
    assert_eq!(count(&helix.get_json(&own).await.unwrap()), 1);
    assert_eq!(count(&helix.get_json(&path).await.unwrap()), 4);
    assert_eq!(
        refused(
            helix
                .post_json_read(&path, &json!({"title": "No cost"}))
                .await
        ),
        400
    );

    let one = format!("{path}&id={id}");
    helix
        .patch_json(&one, &json!({"cost": 75, "is_paused": true}))
        .await
        .unwrap();
    let changed = &helix.get_json(&one).await.unwrap()["data"][0];
    assert_eq!(
        (changed["cost"].as_u64(), changed["is_paused"].as_bool()),
        (Some(75), Some(true))
    );
    let preloaded = format!("{path}&id=fake-reward-check-in");
    assert_eq!(
        refused(helix.patch_json(&preloaded, &json!({"cost": 1})).await),
        403
    );
    assert_eq!(refused(helix.delete(&preloaded).await), 403);
    assert_eq!(
        refused(
            helix
                .patch_json(&format!("{path}&id=fake-none"), &json!({"cost": 1}))
                .await
        ),
        404
    );

    // A viewer redeems the app's reward: the event carries its real ID, and the app marks it once.
    let sent = fakes::control::send_body(
        &fake,
        &json!({"kind": "redeem", "reward": "Hug", "user": "viewer"}),
    )
    .unwrap();
    let redemption = sent["id"].as_str().unwrap().to_owned();
    let mark = |reward: &str| {
        format!(
            "/channel_points/custom_rewards/redemptions?id={redemption}&broadcaster_id={BROADCASTER_ID}&reward_id={reward}"
        )
    };
    let done = helix
        .patch_json_read(&mark(&id), &json!({"status": "FULFILLED"}))
        .await
        .unwrap();
    assert_eq!(
        (
            done["data"][0]["status"].as_str(),
            done["data"][0]["reward"]["id"].as_str()
        ),
        (Some("FULFILLED"), Some(id.as_str()))
    );
    assert!(
        fake.recorder()
            .recent(10)
            .iter()
            .any(|e| e.key == "channel.channel_points_custom_reward_redemption.update")
    );
    assert_eq!(
        refused(
            helix
                .patch_json_read(&mark(&id), &json!({"status": "CANCELED"}))
                .await
        ),
        404,
        "already marked"
    );

    let other =
        fakes::control::send_body(&fake, &json!({"kind": "redeem", "reward": "Check in"})).unwrap();
    let other_mark = format!(
        "/channel_points/custom_rewards/redemptions?id={}&broadcaster_id={BROADCASTER_ID}&reward_id=fake-reward-check-in",
        other["id"].as_str().unwrap()
    );
    assert_eq!(
        refused(
            helix
                .patch_json_read(&other_mark, &json!({"status": "CANCELED"}))
                .await
        ),
        403
    );

    helix.delete(&one).await.unwrap();
    assert_eq!(count(&helix.get_json(&own).await.unwrap()), 0);
    assert_eq!(refused(helix.delete(&one).await), 404);
}

#[tokio::test]
async fn chat_sent_by_the_app_comes_back_as_the_broadcasters_message() {
    let fake = FakeTwitch::default();
    let helix = helix(&fake).await;
    let me = json!({"broadcaster_id": BROADCASTER_ID, "sender_id": BROADCASTER_ID});
    let sent = helix
        .post_json_read("/chat/messages", &json!({"broadcaster_id": BROADCASTER_ID, "sender_id": BROADCASTER_ID, "message": "大家好"}))
        .await
        .unwrap();
    let row = &sent["data"][0];
    assert_eq!(row["is_sent"], true);
    let id = row["message_id"].as_str().unwrap();
    let logged = fake.recorder().recent(5);
    let event = logged
        .iter()
        .find(|e| e.key == "channel.chat.message")
        .expect("the chat event");
    assert_eq!(event.id.as_deref(), Some(id));

    let mut empty = me.clone();
    empty["message"] = json!(" ");
    assert_eq!(
        refused(helix.post_json_read("/chat/messages", &empty).await),
        400
    );
    let mut long = me.clone();
    long["message"] = json!("a".repeat(501));
    assert_eq!(
        refused(helix.post_json_read("/chat/messages", &long).await),
        400
    );
    let other = json!({"broadcaster_id": BROADCASTER_ID, "sender_id": "7", "message": "hi"});
    assert_eq!(
        refused(helix.post_json_read("/chat/messages", &other).await),
        403
    );
}

#[tokio::test]
async fn polls_start_take_votes_and_end_with_their_events() {
    use toolkit_core::events::PollStatus;
    let fake = FakeTwitch::default();
    let (helix, mut events) = contests(&fake).await;
    let poll = |title: &str| {
        json!({"broadcaster_id": BROADCASTER_ID, "title": title, "duration": 60,
               "choices": [{"title": "能"}, {"title": "不能"}]})
    };

    let made = helix
        .post_json_read("/polls", &poll("吃雞？"))
        .await
        .unwrap();
    let id = made["data"][0]["id"].as_str().unwrap().to_owned();
    assert!(
        id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "{id}"
    );
    let read = integrations::twitch::contests::poll_from_json(made["data"][0].clone()).unwrap();
    assert_eq!((read.status, read.choices.len()), (PollStatus::Active, 2));
    assert!(!read.ends_at.is_empty(), "the start plus the duration");
    let Contest::Poll(begin) = next(&mut events).await else {
        panic!("a poll")
    };
    assert_eq!(
        (begin.id.as_str(), begin.status),
        (id.as_str(), PollStatus::Active)
    );
    assert_eq!(
        refused(helix.post_json_read("/polls", &poll("Another")).await),
        400,
        "one at a time"
    );

    fakes::control::poll_body(
        &fake,
        &json!({"action": "vote", "choice": "2", "count": "3"}),
    )
    .unwrap();
    let Contest::Poll(progress) = next(&mut events).await else {
        panic!("a poll")
    };
    assert_eq!(progress.choices[1].votes, 3);
    let listed = helix
        .get_json(&format!("/polls?broadcaster_id={BROADCASTER_ID}&first=1"))
        .await
        .unwrap();
    assert_eq!(listed["data"][0]["choices"][1]["votes"], 3);

    let end = json!({"broadcaster_id": BROADCASTER_ID, "id": id, "status": "TERMINATED"});
    let ended = helix.patch_json_read("/polls", &end).await.unwrap();
    assert_eq!(ended["data"][0]["status"], "TERMINATED");
    let Contest::Poll(over) = next(&mut events).await else {
        panic!("a poll")
    };
    assert_eq!(over.status, PollStatus::Terminated);
    assert_eq!(
        refused(helix.patch_json_read("/polls", &end).await),
        400,
        "already over"
    );
    assert!(fakes::control::poll_body(&fake, &json!({"action": "end"})).is_err());

    // A new poll may start once the last one is over.
    helix
        .post_json_read("/polls", &poll("再一次"))
        .await
        .unwrap();
    assert_eq!(fake.snapshot().poll.unwrap().title, "再一次");
}

#[tokio::test]
async fn predictions_take_bets_lock_and_settle_with_their_events() {
    use toolkit_core::events::PredictionStatus;
    let fake = FakeTwitch::default();
    let (helix, mut events) = contests(&fake).await;
    let prediction = |title: &str| {
        json!({"broadcaster_id": BROADCASTER_ID, "title": title, "prediction_window": 120,
               "outcomes": [{"title": "能"}, {"title": "不能"}]})
    };

    let made = helix
        .post_json_read("/predictions", &prediction("吃雞？"))
        .await
        .unwrap();
    let p = &made["data"][0];
    let id = p["id"].as_str().unwrap().to_owned();
    let winner = p["outcomes"][0]["id"].as_str().unwrap().to_owned();
    for id in [&id, &winner] {
        assert!(
            id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "{id}"
        );
    }
    let read = integrations::twitch::contests::prediction_from_json(p.clone(), false).unwrap();
    assert_eq!(
        (read.status, read.outcomes.len()),
        (PredictionStatus::Active, 2)
    );
    assert!(!read.locks_at.is_empty(), "the start plus the window");
    let Contest::Prediction(begin) = next(&mut events).await else {
        panic!("a prediction")
    };
    assert_eq!(
        (begin.id.as_str(), begin.status),
        (id.as_str(), PredictionStatus::Active)
    );
    assert_eq!(
        refused(
            helix
                .post_json_read("/predictions", &prediction("Another"))
                .await
        ),
        400,
        "one at a time"
    );

    fakes::control::prediction_body(
        &fake,
        &json!({"action": "bet", "outcome": "1", "points": "500", "users": "2"}),
    )
    .unwrap();
    let Contest::Prediction(progress) = next(&mut events).await else {
        panic!("a prediction")
    };
    assert_eq!(
        (progress.outcomes[0].users, progress.outcomes[0].points),
        (2, 500)
    );

    let change =
        |status: &str| json!({"broadcaster_id": BROADCASTER_ID, "id": id, "status": status});
    let locked = helix
        .patch_json_read("/predictions", &change("LOCKED"))
        .await
        .unwrap();
    assert_eq!(locked["data"][0]["status"], "LOCKED");
    let Contest::Prediction(lock) = next(&mut events).await else {
        panic!("a prediction")
    };
    assert_eq!(lock.status, PredictionStatus::Locked);
    assert_eq!(
        refused(
            helix
                .patch_json_read("/predictions", &change("LOCKED"))
                .await
        ),
        400,
        "already locked"
    );
    assert!(
        fakes::control::prediction_body(&fake, &json!({"action": "bet", "outcome": "1"})).is_err()
    );
    assert_eq!(
        refused(
            helix
                .post_json_read("/predictions", &prediction("Still locked"))
                .await
        ),
        400
    );

    let mut resolve = change("RESOLVED");
    resolve["winning_outcome_id"] = json!("fake-none");
    assert_eq!(
        refused(helix.patch_json_read("/predictions", &resolve).await),
        400,
        "not an outcome"
    );
    resolve["winning_outcome_id"] = json!(winner);
    let done = helix
        .patch_json_read("/predictions", &resolve)
        .await
        .unwrap();
    assert_eq!(done["data"][0]["winning_outcome_id"], winner.as_str());
    let Contest::Prediction(end) = next(&mut events).await else {
        panic!("a prediction")
    };
    assert_eq!(
        (end.status, end.winning_outcome_id.as_deref()),
        (PredictionStatus::Resolved, Some(winner.as_str()))
    );
    assert_eq!(
        refused(
            helix
                .patch_json_read("/predictions", &change("CANCELED"))
                .await
        ),
        400,
        "already settled"
    );

    // A new prediction may start once the last one is settled; canceling works while it is open.
    let again = helix
        .post_json_read("/predictions", &prediction("再一次"))
        .await
        .unwrap();
    let again_id = again["data"][0]["id"].as_str().unwrap();
    let cancel = json!({"broadcaster_id": BROADCASTER_ID, "id": again_id, "status": "CANCELED"});
    assert_eq!(
        helix
            .patch_json_read("/predictions", &cancel)
            .await
            .unwrap()["data"][0]["status"],
        "CANCELED"
    );
    let listed = helix
        .get_json(&format!(
            "/predictions?broadcaster_id={BROADCASTER_ID}&first=1"
        ))
        .await
        .unwrap();
    assert_eq!(listed["data"][0]["status"], "CANCELED");
}
