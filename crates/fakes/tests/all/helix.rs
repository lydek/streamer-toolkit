//! The Helix calls the Host's admin pages make, through the real client (`integrations::twitch::helix`), against the
//! fake Twitch: the shapes the Host parses, and the state they change.

use std::sync::Arc;

use fakes::twitch::{BROADCASTER_ID, FakeTwitch};
use integrations::{
    secrets::InMemory,
    tls,
    twitch::{
        auth::{TwitchAuth, fake_stored_token},
        helix::{Helix, HelixError},
    },
};
use serde_json::json;
use tokio::net::TcpListener;

async fn helix(fake: &FakeTwitch) -> Helix {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(fake.clone().serve(listener));
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
