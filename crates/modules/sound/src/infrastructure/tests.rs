use super::*;
use axum::http::Request;
use std::path::PathBuf;
use tower::ServiceExt;

pub(crate) const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-sound-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn app(dir: &std::path::Path) -> Router {
    let modules = Arc::new(toolkit_core::modules::Modules::load(dir.to_path_buf()));
    let service = Arc::new(SoundService::new(
        dir.to_path_buf(),
        modules,
        Arc::new(EventBus::default()),
        toolkit_core::presentation::Presenter::start(
            toolkit_core::presentation::DEFAULT_GAP,
            |_| true,
        ),
    ));
    routes(service.clone()).merge(file_routes(service))
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Response) {
    let res = app.clone().oneshot(req).await.unwrap();
    (res.status(), res)
}

async fn json(res: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn upload(name: &str, body: impl Into<Body>) -> Request<Body> {
    Request::post(format!("/api/admin/sounds?name={name}"))
        .body(body.into())
        .unwrap()
}

#[tokio::test]
async fn import_copies_the_file_under_a_new_name() {
    let dir = data_dir("import");
    let app = app(&dir);
    let (status, res) = send(&app, upload("..%2F..%2Fbell.WAV", WAV)).await;
    assert_eq!(status, StatusCode::OK);
    let sound = json(res).await;
    assert_eq!(sound["label"], "bell");
    assert_eq!(sound["volume"], 100);
    let id = sound["id"].as_str().unwrap();
    assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
    let files: Vec<_> = std::fs::read_dir(dir.join("sounds"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        files,
        [format!("{id}.wav")],
        "named by the ID, never the file name"
    );
    assert!(!dir.join("bell.WAV").exists() && !dir.parent().unwrap().join("bell.WAV").exists());

    let (status, res) = send(
        &app,
        Request::get(format!("/sounds/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let h = res.headers();
    assert_eq!(h[header::CONTENT_TYPE], "audio/wav");
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], WAV);

    let (_, res) = send(
        &app,
        Request::get("/api/admin/sounds")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json(res).await["sounds"][0]["id"], id);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn only_mp3_wav_and_ogg_up_to_10_mb_are_taken() {
    let dir = data_dir("refuse");
    let app = app(&dir);
    for (name, body) in [
        ("x.mp3", WAV.to_vec()),
        ("x.html", b"<script>".to_vec()),
        ("x.wav", Vec::new()),
    ] {
        let (status, res) = send(&app, upload(name, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(json(res).await["code"], "sound.unsupported_type");
    }
    let mut big = WAV.to_vec();
    big.resize(MAX_BYTES + 1, 0);
    let (status, res) = send(&app, upload("x.wav", big.clone())).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json(res).await["code"], "sound.too_large");
    big.truncate(MAX_BYTES);
    assert_eq!(
        send(&app, upload("x.wav", big)).await.0,
        StatusCode::OK,
        "exactly 10 MB"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rename_change_volume_and_delete() {
    let dir = data_dir("edit");
    let app = app(&dir);
    let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let put = |body: serde_json::Value| {
        Request::put(format!("/api/admin/sounds/{id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let (status, res) = send(
        &app,
        put(serde_json::json!({ "label": " Ding ", "volume": 40 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json(res).await,
        serde_json::json!({ "id": id, "label": "Ding", "volume": 40, "used_by": [], "duration_ms": null })
    );
    for bad in [
        serde_json::json!({ "label": " ", "volume": 40 }),
        serde_json::json!({ "label": "a", "volume": 101 }),
    ] {
        assert_eq!(send(&app, put(bad)).await.0, StatusCode::BAD_REQUEST);
    }

    let delete = || {
        Request::delete(format!("/api/admin/sounds/{id}"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(send(&app, delete()).await.0, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, delete()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        std::fs::read_dir(dir.join("sounds")).unwrap().count(),
        0,
        "file removed"
    );
    let file = Request::get(format!("/sounds/{id}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, file).await.0, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_file_route_serves_library_sounds_only() {
    let dir = data_dir("route");
    let app = app(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), "{}").unwrap();
    for path in [
        "/sounds/settings.json",
        "/sounds/..%2Fsettings.json",
        "/sounds/x",
    ] {
        let req = Request::get(path).body(Body::empty()).unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    // An edited settings.json cannot point the route outside `sounds/`.
    toolkit_core::settings::update(&dir, |s| {
        s.sound.library.push(SoundEntry {
            id: "../settings".into(),
            label: "x".into(),
            volume: 100,
            format: "json".into(),
            duration_ms: None,
        })
    })
    .unwrap();
    let req = Request::get("/sounds/..%2Fsettings")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}

fn choose(slot: &str, sound: serde_json::Value) -> Request<Body> {
    Request::put(format!("/api/admin/sounds/slots/{slot}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "sound": sound }).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn slots_play_their_choice_and_fall_back_when_the_sound_is_deleted() {
    let dir = data_dir("slots");
    let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
    let service = Arc::new(SoundService::new(
        dir.clone(),
        modules.clone(),
        Arc::new(EventBus::default()),
        toolkit_core::presentation::Presenter::start(
            toolkit_core::presentation::DEFAULT_GAP,
            |_| true,
        ),
    ));
    let app = routes(service.clone());
    let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Nothing chosen: alerts keep the built-in sound, check-ins are silent.
    assert_eq!(
        AlertSound::from(service.alert("follow")),
        AlertSound::BuiltIn
    );
    assert_eq!(PlaySound::from_cue(service.checkin(false)), None);

    for (slot, choice) in [
        ("alert.follow", id.as_str()),
        ("checkin.full", id.as_str()),
        ("alert.raid", "silent"),
    ] {
        assert_eq!(
            send(&app, choose(slot, choice.into())).await.0,
            StatusCode::NO_CONTENT,
            "{slot}"
        );
    }
    let file = AlertSound::File {
        url: format!("/sounds/{id}"),
        volume: 100,
    };
    assert_eq!(AlertSound::from(service.alert("follow")), file);
    assert_eq!(AlertSound::from(service.alert("raid")), AlertSound::Silent);
    assert_eq!(
        AlertSound::from(service.alert("cheer")),
        AlertSound::BuiltIn
    );
    assert_eq!(
        PlaySound::from_cue(service.checkin(true)),
        Some(PlaySound {
            url: format!("/sounds/{id}"),
            volume: 100
        })
    );
    assert_eq!(PlaySound::from_cue(service.checkin(false)), None);

    let (_, res) = send(
        &app,
        Request::get("/api/admin/sounds")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let library = json(res).await;
    assert_eq!(
        library["sounds"][0]["used_by"],
        serde_json::json!(["alert.follow", "checkin.full"])
    );
    assert_eq!(library["slots"]["alert.raid"], "silent");

    for (slot, choice, status, code) in [
        (
            "alert.host",
            serde_json::json!(id),
            StatusCode::BAD_REQUEST,
            "sound.unknown_slot",
        ),
        (
            "checkin.half",
            serde_json::Value::Null,
            StatusCode::BAD_REQUEST,
            "sound.unknown_slot",
        ),
        (
            "checkin",
            serde_json::json!("nope"),
            StatusCode::NOT_FOUND,
            "sound.not_found",
        ),
    ] {
        let (got, res) = send(&app, choose(slot, choice)).await;
        assert_eq!(
            (got, json(res).await["code"].clone()),
            (status, code.into()),
            "{slot}"
        );
    }

    // While the module is off every slot plays its default.
    modules
        .set_enabled(toolkit_core::modules::SOUND, false)
        .unwrap();
    assert_eq!(
        AlertSound::from(service.alert("follow")),
        AlertSound::BuiltIn
    );
    assert_eq!(PlaySound::from_cue(service.checkin(true)), None);
    modules
        .set_enabled(toolkit_core::modules::SOUND, true)
        .unwrap();

    let delete = Request::delete(format!("/api/admin/sounds/{id}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, delete).await.0, StatusCode::NO_CONTENT);
    assert_eq!(
        AlertSound::from(service.alert("follow")),
        AlertSound::BuiltIn
    );
    assert_eq!(PlaySound::from_cue(service.checkin(true)), None);
    assert_eq!(
        service.slots(),
        BTreeMap::from([("alert.raid".to_owned(), "silent".to_owned())]),
        "only the slots that played it are reset"
    );
    // Back to the default with `null`.
    assert_eq!(
        send(&app, choose("alert.raid", serde_json::Value::Null))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(service.slots().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_admin_window_records_each_sounds_length() {
    let dir = data_dir("duration");
    let app = app(&dir);
    let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let set = |id: &str, body: serde_json::Value| {
        Request::put(format!("/api/admin/sounds/{id}/duration"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let list = || {
        Request::get("/api/admin/sounds")
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        json(send(&app, list()).await.1).await["sounds"][0]["duration_ms"],
        serde_json::Value::Null,
        "unmeasured after import"
    );
    let ok = send(&app, set(&id, serde_json::json!({"duration_ms": 1500}))).await;
    assert_eq!(ok.0, StatusCode::NO_CONTENT);
    assert_eq!(
        json(send(&app, list()).await.1).await["sounds"][0]["duration_ms"],
        1500
    );
    for bad in [
        serde_json::json!({"duration_ms": 0}),
        serde_json::json!({"duration_ms": 6 * 60 * 60 * 1000 + 1}),
        serde_json::json!({"duration_ms": "1500"}),
    ] {
        assert_eq!(send(&app, set(&id, bad)).await.0, StatusCode::BAD_REQUEST);
    }
    let missing = set("nope", serde_json::json!({"duration_ms": 1500}));
    assert_eq!(send(&app, missing).await.0, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_reward_sound_holds_the_queue_for_its_length() {
    use toolkit_core::events::RedemptionChange::Added;
    let dir = data_dir("reward-length");
    let bus = Arc::new(EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
    let gap = std::time::Duration::from_millis(10);
    let service = Arc::new(SoundService::new(
        dir.clone(),
        modules,
        bus.clone(),
        toolkit_core::presentation::Presenter::start(gap, |_| true),
    ));
    let app = routes(service.clone());
    let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
    send(&app, choose(&format!("reward.{reward}"), id.clone().into())).await;
    service.set_duration(&id, 300).unwrap();
    let (_, mut rx) = bus.subscribe();

    service.receive_redemption(&redemption(reward, Added));
    service.receive_redemption(&redemption(reward, Added));
    rx.recv().await.unwrap();
    let first = std::time::Instant::now();
    rx.recv().await.unwrap();
    assert!(
        first.elapsed() >= std::time::Duration::from_millis(300),
        "the second waits until the first has played"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_reward_sound_whose_file_is_gone_is_marked_and_plays_nothing() {
    use toolkit_core::events::RedemptionChange::Added;
    use toolkit_core::presentation::{Problem, Subject};
    let dir = data_dir("reward-missing");
    let bus = Arc::new(EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
    let presenter =
        toolkit_core::presentation::Presenter::start(std::time::Duration::ZERO, |_| true);
    let mut status = presenter.subscribe();
    let service = Arc::new(SoundService::new(
        dir.clone(),
        modules,
        bus.clone(),
        presenter,
    ));
    let app = routes(service.clone());
    let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
    send(&app, choose(&format!("reward.{reward}"), id.clone().into())).await;
    assert!(!service.file_missing(&id));
    std::fs::remove_file(service.file(&id).unwrap().0).unwrap();
    assert!(service.file_missing(&id));
    let (_, mut rx) = bus.subscribe();

    service.receive_redemption(&redemption(reward, Added));
    let playing = status
        .wait_for(|s| s.current.as_ref().is_some_and(|c| c.problem.is_some()))
        .await
        .unwrap()
        .current
        .clone()
        .unwrap();
    assert_eq!(playing.problem, Some(Problem::SoundMissing));
    assert_eq!(
        playing.subject,
        Subject::Sound {
            sound: "bell".into(),
            viewer: "Viewer".into()
        }
    );
    assert!(rx.try_recv().is_err(), "nothing sent to the sound overlay");
    let _ = std::fs::remove_dir_all(&dir);
}

fn redemption(
    reward: &str,
    change: toolkit_core::events::RedemptionChange,
) -> toolkit_core::events::ChannelPointsRedemption {
    toolkit_core::events::ChannelPointsRedemption {
        id: "r-1".into(),
        change,
        user_id: "1".into(),
        user_login: "viewer".into(),
        user_name: "Viewer".into(),
        reward_id: reward.into(),
        reward_title: "Bell".into(),
        reward_cost: 100,
        user_input: String::new(),
        status: toolkit_core::events::RedemptionStatus::Unfulfilled,
        redeemed_at: "2026-11-17T10:00:00Z".into(),
    }
}

#[tokio::test]
async fn new_redemptions_of_a_mapped_reward_play_its_sound_once_and_live_only() {
    use toolkit_core::events::RedemptionChange::{Added, Updated};
    let dir = data_dir("rewards");
    let bus = Arc::new(EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
    let service = Arc::new(SoundService::new(
        dir.clone(),
        modules.clone(),
        bus.clone(),
        toolkit_core::presentation::Presenter::start(
            toolkit_core::presentation::DEFAULT_GAP,
            |_| true,
        ),
    ));
    let app = routes(service.clone());
    let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
    assert_eq!(
        send(&app, choose(&format!("reward.{reward}"), id.clone().into()))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, mut rx) = bus.subscribe();

    service.receive_redemption(&redemption(reward, Updated));
    service.receive_redemption(&redemption("other-reward", Added));
    modules
        .set_enabled(toolkit_core::modules::SOUND, false)
        .unwrap();
    service.receive_redemption(&redemption(reward, Added));
    modules
        .set_enabled(toolkit_core::modules::SOUND, true)
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "status changes, other rewards and a disabled module play nothing"
    );

    service.receive_redemption(&redemption(reward, Added));
    // The presentation queue publishes it on its own task.
    let event: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
    assert_eq!(event["type"], "sound.play");
    assert_eq!(event["source"], "twitch");
    assert_eq!(
        event["payload"],
        serde_json::json!({ "url": format!("/sounds/{id}"), "volume": 100 })
    );
    let typed: SoundPlayEvent = serde_json::from_value(event.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(typed).unwrap(),
        event,
        "the schema type matches the bus"
    );
    let (snapshot, _) = bus.subscribe();
    assert!(
        !snapshot.contains("sound.play"),
        "never replayed to an overlay that connects later"
    );

    // The test button plays any library sound as a simulation.
    let play = |id: &str| {
        Request::post(format!("/api/admin/sounds/{id}/play"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(send(&app, play(&id)).await.0, StatusCode::NO_CONTENT);
    let event: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(
        (event["type"].as_str(), event["source"].as_str()),
        (Some("sound.play"), Some("simulation"))
    );
    assert_eq!(send(&app, play("nope")).await.0, StatusCode::NOT_FOUND);

    let library = json(
        send(
            &app,
            Request::get("/api/admin/sounds")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .1,
    )
    .await;
    assert_eq!(
        library["sounds"][0]["used_by"],
        serde_json::json!([format!("reward.{reward}")])
    );
    let _ = std::fs::remove_dir_all(dir);
}
