use super::*;

#[tokio::test]
async fn backup_routes_require_the_token() {
    for (method, uri) in [
        (Method::GET, "/api/admin/backup"),
        (Method::POST, "/api/admin/backup/restore/upload"),
        (Method::PUT, "/api/admin/backup/restore/upload/x?offset=0"),
        (Method::DELETE, "/api/admin/backup/restore/upload/x"),
        (Method::POST, "/api/admin/backup/restore/upload/x/check"),
        (Method::POST, "/api/admin/backup/restore"),
    ] {
        let req = get_req(uri, "127.0.0.1:47810")
            .method(method)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(req).await, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
async fn backup_download_can_be_cancelled_and_retried() {
    use axum::body::HttpBody;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, Write};

    let dir = tempfile::tempdir().unwrap();
    let sounds = dir.path().join("sounds");
    std::fs::create_dir(&sounds).unwrap();
    let mut sound = std::fs::File::create(sounds.join("sample.mp3")).unwrap();
    let mut expected = Sha256::new();
    // Use deterministic, poorly compressible bytes and produce them a chunk at a time.
    let mut seed = 0x1234_abcd_u32;
    let mut chunk = [0; 64 * 1024];
    for _ in 0..512 {
        for word in chunk.chunks_exact_mut(4) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            word.copy_from_slice(&seed.to_le_bytes());
        }
        sound.write_all(&chunk).unwrap();
        expected.update(chunk);
    }
    drop(sound);

    let mut config = test_config();
    config.data_dir = dir.path().to_owned();
    config.db = db::open(dir.path()).await.unwrap();
    let app = router(config).unwrap();
    let download = || async {
        let request = get_req("/api/admin/backup", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.into_body()
    };
    let cache_empty = || {
        std::fs::read_dir(dir.path().join("cache"))
            .unwrap()
            .next()
            .is_none()
    };

    let mut cancelled = download().await;
    let frame = std::future::poll_fn(|cx| std::pin::Pin::new(&mut cancelled).poll_frame(cx))
        .await
        .unwrap()
        .unwrap();
    assert!(frame.into_data().unwrap().starts_with(b"PK"));
    drop(cancelled);
    assert!(cache_empty());

    let mut body = download().await;
    let mut saved = tempfile::tempfile_in(dir.path()).unwrap();
    let mut frames = 0;
    while let Some(frame) =
        std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)).await
    {
        let data = frame.unwrap().into_data().unwrap();
        assert!(data.len() <= 64 * 1024);
        saved.write_all(&data).unwrap();
        frames += 1;
    }
    drop(body);
    assert!(frames > 1);
    assert!(cache_empty());
    saved.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(saved).unwrap();
    let mut sample = archive.by_name("sounds/sample.mp3").unwrap();
    let mut actual = Sha256::new();
    loop {
        let n = sample.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        actual.update(&chunk[..n]);
    }
    assert_eq!(actual.finalize(), expected.finalize());
}

#[tokio::test]
async fn update_routes_require_the_token() {
    for (method, uri) in [
        (Method::GET, "/api/admin/update"),
        (Method::PUT, "/api/admin/update/check"),
        (Method::PUT, "/api/admin/update/dismiss"),
    ] {
        let req = get_req(uri, "127.0.0.1:47810")
            .method(method)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(req).await, StatusCode::UNAUTHORIZED, "{uri}");
    }
}
