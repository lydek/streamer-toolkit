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
