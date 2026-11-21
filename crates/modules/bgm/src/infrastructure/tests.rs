use super::*;
use axum::http::Request;
use serde_json::{Value, json};
use std::path::PathBuf;
use toolkit_core::modules::Modules;
use tower::ServiceExt;

const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";
const MP3: &[u8] = b"ID3\x04\0\0\0\0\0\0";
const OGG: &[u8] = b"OggS\0\x02\0\0";

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-bgm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn app(dir: &std::path::Path) -> Router {
    app_on(dir, Arc::new(EventBus::default()))
}

/// The routes with a player that publishes on `bus` and has no overlay open.
fn app_on(dir: &std::path::Path, bus: Arc<EventBus>) -> Router {
    let service = Arc::new(BgmService::new(dir.to_path_buf()));
    let modules = Arc::new(Modules::load(dir.to_path_buf()));
    let player = start(
        service.clone(),
        modules,
        bus,
        || false,
        watch::channel(()).1,
    );
    routes(service.clone(), player).merge(file_routes(service))
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Response) {
    let res = app.clone().oneshot(req).await.unwrap();
    (res.status(), res)
}

async fn json(res: Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn upload(name: &str, body: impl Into<Body>) -> Request<Body> {
    Request::post(format!("/api/admin/music?name={name}"))
        .body(body.into())
        .unwrap()
}

fn with_json(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn library(app: &Router) -> Value {
    json(send(app, get("/api/admin/music")).await.1).await
}

#[tokio::test]
async fn imports_mp3_wav_and_ogg_under_new_names() {
    let dir = data_dir("import");
    let app = app(&dir);
    for (name, bytes, content_type) in [
        ("..%2F..%2FOpening.MP3", MP3, "audio/mpeg"),
        ("Lo-fi.wav", WAV, "audio/wav"),
        ("Rain.ogg", OGG, "audio/ogg"),
    ] {
        let (status, res) = send(&app, upload(name, bytes)).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        let track = json(res).await;
        let id = track["id"].as_str().unwrap();
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
        assert_eq!(track["duration_ms"], Value::Null);
        assert_eq!(track["missing"], false);

        let (status, res) = send(&app, get(&format!("/music/{id}"))).await;
        assert_eq!(status, StatusCode::OK);
        let h = res.headers();
        assert_eq!(h[header::CONTENT_TYPE], content_type);
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert!(!h.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], bytes);
    }
    let titles: Vec<_> = library(&app).await["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(titles, ["Opening", "Lo-fi", "Rain"]);
    assert_eq!(
        std::fs::read_dir(dir.join("music")).unwrap().count(),
        3,
        "named by the ID, never the file name"
    );
    assert!(
        !dir.join("Opening.MP3").exists() && !dir.parent().unwrap().join("Opening.MP3").exists()
    );
    // Not the sound library.
    assert!(!dir.join("sounds").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn refuses_files_whose_content_does_not_match_or_that_are_too_large() {
    let dir = data_dir("refuse");
    let app = app(&dir);
    for (name, body) in [
        ("x.mp3", WAV.to_vec()),
        ("x.ogg", MP3.to_vec()),
        ("x.html", b"<script>".to_vec()),
        ("x.wav", Vec::new()),
    ] {
        let (status, res) = send(&app, upload(name, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(json(res).await["code"], "music.unsupported_type");
    }
    let mut big = WAV.to_vec();
    big.resize(MAX_BYTES + 1, 0);
    let (status, res) = send(&app, upload("x.wav", big)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(res).await["code"], "music.too_large");
    assert_eq!(library(&app).await["tracks"], json!([]));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_track_whose_file_is_gone_says_so() {
    let dir = data_dir("missing");
    let app = app(&dir);
    let id = json(send(&app, upload("a.ogg", OGG)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::remove_file(dir.join(format!("music/{id}.ogg"))).unwrap();
    assert_eq!(library(&app).await["tracks"][0]["missing"], true);
    let (status, _) = send(&app, get(&format!("/music/{id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&app, get("/music/0190a5b0-0000-7000-8000-000000000000")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn titles_and_lengths() {
    let dir = data_dir("edit");
    let app = app(&dir);
    let id = json(send(&app, upload("a.mp3", MP3)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, res) = send(
        &app,
        with_json(
            "PUT",
            &format!("/api/admin/music/{id}"),
            json!({ "title": " Theme " }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(res).await["title"], "Theme");
    let (status, res) = send(
        &app,
        with_json(
            "PUT",
            &format!("/api/admin/music/{id}"),
            json!({ "title": "  " }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(res).await["code"], "music.invalid");

    let duration = |ms: u64| {
        with_json(
            "PUT",
            &format!("/api/admin/music/{id}/duration"),
            json!({ "duration_ms": ms }),
        )
    };
    assert_eq!(send(&app, duration(0)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        send(&app, duration(24 * 60 * 60 * 1000 + 1)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&app, duration(188_000)).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(library(&app).await["tracks"][0]["duration_ms"], 188_000);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn tracks_go_into_playlists_and_leave_them_when_deleted() {
    let dir = data_dir("playlists");
    let app = app(&dir);
    let mut ids = Vec::new();
    for name in ["a.mp3", "b.mp3"] {
        let track = json(send(&app, upload(name, MP3)).await.1).await;
        ids.push(track["id"].as_str().unwrap().to_owned());
    }
    let mut lists = Vec::new();
    for name in ["Chat", "Game"] {
        let (status, res) = send(
            &app,
            with_json(
                "POST",
                "/api/admin/music/playlists",
                json!({ "name": name }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        lists.push(json(res).await["id"].as_str().unwrap().to_owned());
    }
    let (chat, game) = (&lists[0], &lists[1]);
    let set = |track: &str, playlists: Value| {
        with_json(
            "PUT",
            &format!("/api/admin/music/{track}/playlists"),
            json!({ "playlists": playlists }),
        )
    };
    assert_eq!(
        send(&app, set(&ids[1], json!([chat, game]))).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, set(&ids[0], json!([chat]))).await.0,
        StatusCode::NO_CONTENT
    );
    let lib = library(&app).await;
    assert_eq!(
        lib["playlists"][0]["tracks"],
        json!([ids[1], ids[0]]),
        "joins the end"
    );
    assert_eq!(lib["playlists"][1]["tracks"], json!([ids[1]]));

    // Moving a track keeps its place where it stays.
    assert_eq!(
        send(&app, set(&ids[1], json!([chat]))).await.0,
        StatusCode::NO_CONTENT
    );
    let lib = library(&app).await;
    assert_eq!(lib["playlists"][0]["tracks"], json!([ids[1], ids[0]]));
    assert_eq!(lib["playlists"][1]["tracks"], json!([]));

    // An unknown playlist changes nothing.
    let (status, res) = send(&app, set(&ids[0], json!(["nope"]))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(res).await["code"], "music.not_found");
    assert_eq!(
        library(&app).await["playlists"][0]["tracks"],
        json!([ids[1], ids[0]])
    );

    let (status, res) = send(
        &app,
        with_json(
            "PUT",
            &format!("/api/admin/music/playlists/{game}"),
            json!({ "name": "Game BGM" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(res).await["name"], "Game BGM");

    let delete = |uri: String| Request::delete(uri).body(Body::empty()).unwrap();
    assert_eq!(
        send(&app, delete(format!("/api/admin/music/{}", ids[1])))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(!dir.join(format!("music/{}.mp3", ids[1])).exists());
    assert_eq!(
        library(&app).await["playlists"][0]["tracks"],
        json!([ids[0]])
    );

    assert_eq!(
        send(&app, delete(format!("/api/admin/music/playlists/{chat}")))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let lib = library(&app).await;
    assert_eq!(lib["playlists"].as_array().unwrap().len(), 1);
    assert_eq!(lib["tracks"].as_array().unwrap().len(), 1, "tracks stay");
    assert_eq!(
        send(&app, delete(format!("/api/admin/music/playlists/{chat}")))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn music_files_answer_byte_ranges() {
    let dir = data_dir("range");
    let app = app(&dir);
    let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ranged = |range: &str| {
        Request::get(format!("/music/{id}"))
            .header(header::RANGE, range)
            .body(Body::empty())
            .unwrap()
    };
    let size = WAV.len();
    for (range, from, to) in [
        ("bytes=4-7", 4, 8),
        ("bytes=8-", 8, size),
        ("bytes=-4", size - 4, size),
        ("bytes=0-9999", 0, size),
    ] {
        let (status, res) = send(&app, ranged(range)).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT, "{range}");
        let h = res.headers();
        assert_eq!(
            h[header::CONTENT_RANGE],
            format!("bytes {from}-{}/{size}", to - 1).as_str(),
            "{range}"
        );
        assert_eq!(h[header::CONTENT_TYPE], "audio/wav");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], &WAV[from..to], "{range}");
    }
    for range in ["bytes=99-", "bytes=5-2", "items=0-1", "bytes=x-1"] {
        let (status, res) = send(&app, ranged(range)).await;
        assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(
            res.headers()[header::CONTENT_RANGE],
            format!("bytes */{size}").as_str()
        );
    }
    let (status, res) = send(&app, get(&format!("/music/{id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_player_is_controlled_over_the_api_and_reports_on_the_bus() {
    let dir = data_dir("player");
    let bus = Arc::new(EventBus::default());
    let app = app_on(&dir, bus.clone());
    let player = |body: Value| with_json("POST", "/api/admin/music/player", body);
    let (snapshot, mut events) = bus.subscribe();
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    let first = &snapshot["payload"]["bgm.player"][0];
    assert_eq!(
        first["payload"]["state"], "stopped",
        "kept for the snapshot"
    );
    assert_eq!(first["payload"]["volume"], 50);

    assert_eq!(
        send(&app, player(json!({"action": "play"}))).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&app, player(json!({"action": "dance"}))).await.0,
        StatusCode::BAD_REQUEST
    );
    let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let playlist = json(
        send(
            &app,
            with_json(
                "POST",
                "/api/admin/music/playlists",
                json!({"name": "Chat"}),
            ),
        )
        .await
        .1,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &app,
        with_json(
            "PUT",
            &format!("/api/admin/music/{id}/playlists"),
            json!({"playlists": [playlist]}),
        ),
    )
    .await;
    // Not measured yet: nothing can play.
    let (status, res) = send(
        &app,
        player(json!({"action": "play", "playlist": playlist})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json(res).await["code"], "music.nothing_to_play");
    send(
        &app,
        with_json(
            "PUT",
            &format!("/api/admin/music/{id}/duration"),
            json!({"duration_ms": 60_000}),
        ),
    )
    .await;
    let (status, res) = send(&app, player(json!({"action": "play", "playlist": "nope"}))).await;
    assert_eq!(
        (status, json(res).await["code"].clone()),
        (StatusCode::NOT_FOUND, json!("music.not_found"))
    );
    assert_eq!(
        send(
            &app,
            player(json!({"action": "play", "playlist": playlist}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let options = |body: Value| with_json("PUT", "/api/admin/music/player/options", body);
    assert_eq!(
        send(&app, options(json!({"volume": 101, "shuffle": false})))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&app, options(json!({"volume": 30, "shuffle": true})))
            .await
            .0,
        StatusCode::NO_CONTENT
    );

    let mut last = Value::Null;
    while let Ok(text) = events.try_recv() {
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["type"] == "bgm.player" {
            // Every event matches its documented type.
            let typed: BgmPlayerEvent = serde_json::from_value(event.clone()).unwrap();
            assert_eq!(serde_json::to_value(typed).unwrap(), event);
            last = event;
        }
    }
    let p = &last["payload"];
    assert_eq!(p["state"], "playing");
    assert_eq!(p["playlist"], playlist.as_str());
    assert_eq!(p["track"]["url"], format!("/music/{id}").as_str());
    assert_eq!(p["track"]["durationMs"], 60_000);
    assert_eq!(
        (p["onAir"].clone(), p["positionMs"].clone()),
        (json!(false), json!(0))
    );
    assert_eq!(
        (p["volume"].clone(), p["shuffle"].clone()),
        (json!(30), json!(true))
    );
    let _ = std::fs::remove_dir_all(dir);
}
