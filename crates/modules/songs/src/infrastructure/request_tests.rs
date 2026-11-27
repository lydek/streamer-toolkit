//! Song requests through the service and the routes, on a real (in-memory) SQLite database.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::Body,
    http::{Method, Request as HttpRequest, StatusCode, header},
};
use toolkit_core::{
    commands::CommandTable,
    events::{ChatMessage, Source},
    modules::{self, Modules},
};
use tower::ServiceExt;

use super::{Songs, db, requests::*, start};
use crate::application::{
    SongInput,
    requests::{COMMANDS, RequestError},
};

struct Fixture {
    songs: Arc<Songs>,
    requests: Arc<Requests>,
    said: Arc<Mutex<Vec<String>>>,
    changes: Arc<AtomicUsize>,
    modules: Arc<Modules>,
    pool: sqlx::SqlitePool,
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("st-songs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pool = db::tests::pool().await;
    let modules = Arc::new(Modules::load(dir.clone()));
    let said: Arc<Mutex<Vec<String>>> = Arc::default();
    let changes: Arc<AtomicUsize> = Arc::default();
    let (to, count) = (said.clone(), changes.clone());
    let requests = start_requests(
        pool.clone(),
        modules.clone(),
        Arc::new(CommandTable::load(dir.clone(), COMMANDS.to_vec())),
        dir.clone(),
        Arc::new(move |text| to.lock().unwrap().push(text)),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    Fixture {
        songs: start(pool.clone()),
        requests,
        said,
        changes,
        modules,
        pool,
        dir,
    }
}

impl Fixture {
    async fn category(&self, name: &str, requestable: bool) -> String {
        self.songs
            .save_category(None, name, requestable)
            .await
            .unwrap()
            .id
    }

    async fn song(&self, title: &str, artist: &str, category: Option<&str>) -> String {
        let input = SongInput {
            title: title.into(),
            artist: artist.into(),
            category_id: category.map(Into::into),
            youtube_url: String::new(),
        };
        self.songs.save_song(None, input).await.unwrap().id
    }

    fn chat(&self, text: &str, source: Source) {
        let m = ChatMessage {
            id: uuid::Uuid::now_v7().to_string(),
            broadcaster_id: "42".into(),
            user_id: "7".into(),
            user_login: "fish".into(),
            user_name: "小魚".into(),
            color: None,
            text: text.into(),
            fragments: vec![],
            badges: vec![],
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: vec![],
            shared_from: None,
            sent_at: "2026-11-28T10:00:00Z".into(),
        };
        self.requests.receive_chat(&m, source);
    }

    /// Waits for the chat requests sent so far to be answered.
    async fn replies(&self, expected: usize) -> Vec<String> {
        for _ in 0..200 {
            if self.said.lock().unwrap().len() >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // Long enough for one more to show up if it should not.
        tokio::time::sleep(Duration::from_millis(20)).await;
        self.said.lock().unwrap().clone()
    }

    async fn board(&self) -> (Option<String>, Vec<String>, Vec<String>) {
        let b = self.requests.board().await.unwrap();
        (
            b.singing.map(|r| r.title),
            b.queued.into_iter().map(|r| r.title).collect(),
            b.sung.into_iter().map(|r| r.title).collect(),
        )
    }

    async fn queued_ids(&self) -> Vec<String> {
        let b = self.requests.board().await.unwrap();
        b.queued.into_iter().map(|r| r.id).collect()
    }
}

#[tokio::test]
async fn chat_requests_queue_what_matches_once_and_the_bot_says_why_not() {
    let f = fixture("chat").await;
    let open = f.category("華語", true).await;
    let closed = f.category("待練勿點", false).await;
    f.song("晴天", "周杰倫", Some(&open)).await;
    f.song("Lemon", "米津玄師", None).await;
    f.song("Lemon", "別人", None).await;
    f.song("新歌", "", Some(&closed)).await;

    f.chat("!點歌 周杰倫 晴天", Source::Twitch);
    assert_eq!(f.replies(1).await.len(), 1);
    for text in [
        "!點歌 lemon",
        "!點歌 不存在 {觀眾}",
        "!點歌 新歌",
        "!點歌 晴天",
    ] {
        f.chat(text, Source::Twitch);
        let n = f.said.lock().unwrap().len();
        f.replies(n + 1).await;
    }
    // Neither simulated chat, an empty request nor the command's name inside a sentence queue anything.
    f.chat("!點歌 Lemon 米津玄師", Source::Simulation);
    f.chat("!點歌", Source::Twitch);
    f.chat("我想 !點歌 Lemon 米津玄師", Source::Twitch);
    assert_eq!(
        f.replies(6).await,
        [
            "小魚 點了「晴天」，排在第 1 首",
            "小魚 有好幾首「lemon」，請加上歌手",
            "小魚 歌單裡找不到「不存在 {觀眾}」",
            "小魚「新歌」現在不開放點",
            "小魚「晴天」已經在待唱了",
        ]
    );
    assert_eq!(f.board().await, (None, vec!["晴天".into()], vec![]));
    let (songs, _) = f.songs.list().await.unwrap();
    let counts: Vec<_> = songs
        .iter()
        .map(|s| (s.title.as_str(), s.request_count))
        .collect();
    assert!(counts.contains(&("晴天", 1)));
    assert_eq!(f.changes.load(Ordering::SeqCst), 1);

    // While the module is off, chat is ignored and what is queued stays.
    f.modules.set_enabled(modules::SONGS, false).unwrap();
    f.chat("!點歌 Lemon 米津玄師", Source::Twitch);
    assert_eq!(f.replies(6).await.len(), 5);
    assert_eq!(f.board().await.1, ["晴天"]);
}

#[tokio::test]
async fn a_full_queue_takes_no_more() {
    let f = fixture("full").await;
    for i in 0..100 {
        let id = f.song(&format!("歌 {i}"), "", None).await;
        f.requests.request_by_hand(&id, None).await.unwrap();
    }
    let last = f.song("最後", "", None).await;
    assert!(matches!(
        f.requests.request_by_hand(&last, None).await,
        Err(RequestError::QueueFull)
    ));
    f.chat("!點歌 最後", Source::Twitch);
    assert_eq!(f.replies(1).await, ["小魚 待唱已經滿了，晚點再點"]);
}

#[tokio::test]
async fn requests_move_only_as_the_state_table_allows_and_survive_a_restart() {
    let f = fixture("states").await;
    let closed = f.category("待練勿點", false).await;
    let a = f.song("A", "", Some(&closed)).await;
    let b = f.song("B", "", None).await;
    let c = f.song("C", "", None).await;
    // By hand, closed categories do not matter; nothing is posted.
    f.requests
        .request_by_hand(&a, Some(" 橘子 ".into()))
        .await
        .unwrap();
    f.requests.request_by_hand(&b, None).await.unwrap();
    f.requests.request_by_hand(&c, None).await.unwrap();
    assert!(matches!(
        f.requests.request_by_hand(&b, None).await,
        Err(RequestError::AlreadyQueued)
    ));
    assert!(matches!(
        f.requests.request_by_hand(&b, Some("名".repeat(26))).await,
        Err(RequestError::Invalid)
    ));
    assert!(f.said.lock().unwrap().is_empty());
    let board = f.requests.board().await.unwrap();
    assert_eq!(board.queued[0].viewer_name.as_deref(), Some("橘子"));

    let ids = f.queued_ids().await;
    // No song being sung: 唱完了 has nothing to finish; any queued one may start.
    assert!(matches!(
        f.requests.finish().await,
        Err(RequestError::NotAllowed)
    ));
    f.requests.start(&ids[1]).await.unwrap();
    assert!(matches!(
        f.requests.start(&ids[0]).await,
        Err(RequestError::NotAllowed)
    ));
    assert_eq!(
        f.board().await,
        (Some("B".into()), vec!["A".into(), "C".into()], vec![])
    );

    // 唱完了: B is sung (and its last sung time set), the first queued one starts.
    f.requests.finish().await.unwrap();
    assert_eq!(
        f.board().await,
        (Some("A".into()), vec!["C".into()], vec!["B".into()])
    );
    let (songs, _) = f.songs.list().await.unwrap();
    assert!(
        songs
            .iter()
            .find(|s| s.id == b)
            .unwrap()
            .last_sung_at
            .is_some()
    );
    assert!(
        songs
            .iter()
            .find(|s| s.id == a)
            .unwrap()
            .last_sung_at
            .is_none()
    );

    // Removed is final; sung ones are no longer active.
    f.requests.remove(&ids[2]).await.unwrap();
    assert!(matches!(
        f.requests.remove(&ids[2]).await,
        Err(RequestError::NotFound)
    ));
    assert!(matches!(
        f.requests.start(&ids[1]).await,
        Err(RequestError::NotFound)
    ));
    f.requests.finish().await.unwrap();
    assert_eq!(
        f.board().await,
        (None, vec![], vec!["A".into(), "B".into()])
    );

    // A restart reads the same queue.
    let again = start_requests(
        f.pool.clone(),
        f.modules.clone(),
        Arc::new(CommandTable::load(f.dir.clone(), COMMANDS.to_vec())),
        f.dir.clone(),
        Arc::new(|_| {}),
        Arc::new(|| {}),
    );
    let titles: Vec<_> = again
        .board()
        .await
        .unwrap()
        .sung
        .into_iter()
        .map(|r| r.title)
        .collect();
    assert_eq!(titles, ["A", "B"]);
}

#[tokio::test]
async fn the_queue_is_reordered_only_with_every_queued_request_once() {
    let f = fixture("order").await;
    for title in ["A", "B", "C"] {
        let id = f.song(title, "", None).await;
        f.requests.request_by_hand(&id, None).await.unwrap();
    }
    let ids = f.queued_ids().await;
    f.requests
        .reorder(&[ids[2].clone(), ids[0].clone(), ids[1].clone()])
        .await
        .unwrap();
    assert_eq!(f.board().await.1, ["C", "A", "B"]);
    for wrong in [
        vec![ids[0].clone(), ids[1].clone()],
        vec![ids[0].clone(), ids[0].clone(), ids[1].clone()],
        vec![ids[0].clone(), ids[1].clone(), "x".into()],
    ] {
        assert!(matches!(
            f.requests.reorder(&wrong).await,
            Err(RequestError::Invalid)
        ));
    }
    // Removing one keeps the others' order; a new one goes last.
    f.requests.remove(&ids[0]).await.unwrap();
    let d = f.song("D", "", None).await;
    f.requests.request_by_hand(&d, None).await.unwrap();
    assert_eq!(f.board().await.1, ["C", "B", "D"]);
}

#[tokio::test]
async fn the_page_reads_and_changes_the_queue_through_the_routes() {
    let f = fixture("routes").await;
    let song = f.song("晴天", "周杰倫", None).await;
    let app = request_routes(f.requests.clone());
    let call = |method: Method, uri: &str, body: Option<serde_json::Value>| {
        let b = HttpRequest::builder().method(method).uri(uri);
        match body {
            Some(json) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => b.body(Body::empty()),
        }
        .unwrap()
    };
    let res = app
        .clone()
        .oneshot(call(
            Method::POST,
            "/api/admin/song-requests",
            Some(serde_json::json!({ "song_id": song, "viewer": "橘子" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .clone()
        .oneshot(call(
            Method::POST,
            "/api/admin/song-requests",
            Some(serde_json::json!({ "song_id": song })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = app
        .clone()
        .oneshot(call(Method::GET, "/api/admin/song-requests", None))
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    let board: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(board["queued"][0]["title"], "晴天");
    assert_eq!(board["queued"][0]["viewer"], "橘子");
    assert_eq!(board["singing"], serde_json::Value::Null);
    let id = board["queued"][0]["id"].as_str().unwrap().to_owned();
    for (method, uri, status) in [
        (
            Method::POST,
            "/api/admin/song-requests/finish".to_owned(),
            StatusCode::CONFLICT,
        ),
        (
            Method::POST,
            format!("/api/admin/song-requests/{id}/start"),
            StatusCode::NO_CONTENT,
        ),
        (
            Method::POST,
            "/api/admin/song-requests/finish".to_owned(),
            StatusCode::NO_CONTENT,
        ),
        (
            Method::DELETE,
            format!("/api/admin/song-requests/{id}"),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let res = app.clone().oneshot(call(method, &uri, None)).await.unwrap();
        assert_eq!(res.status(), status, "{uri}");
    }
    let res = app
        .clone()
        .oneshot(call(
            Method::PUT,
            "/api/admin/song-requests/order",
            Some(serde_json::json!({ "ids": ["x"] })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
