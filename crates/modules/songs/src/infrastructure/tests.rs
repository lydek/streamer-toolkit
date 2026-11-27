use super::*;
use axum::{
    body::Body,
    http::{Method, Request, header},
};
use tower::ServiceExt;

fn req(method: Method, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri);
    match body {
        Some(json) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string())),
        None => b.body(Body::empty()),
    }
    .unwrap()
}

fn file(uri: &str, bytes: &'static [u8]) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .unwrap()
}

async fn json(res: Response) -> serde_json::Value {
    let body = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap_or_default()
}

async fn app() -> Router {
    routes(start(db::tests::pool().await))
}

#[tokio::test]
async fn songs_are_added_once_and_a_deleted_category_leaves_its_songs() {
    let app = app().await;
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/song-list/categories",
            Some(serde_json::json!({ "name": "華語", "requestable": true })),
        ))
        .await
        .unwrap();
    let category = json(res).await["id"].as_str().unwrap().to_owned();
    let song = |title: &str, artist: &str| serde_json::json!({ "title": title, "artist": artist, "category_id": category, "youtube_url": "https://youtu.be/dQw4w9WgXcQ" });
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/song-list/songs",
            Some(song(" 晴天 ", "周杰倫")),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let made = json(res).await;
    assert_eq!(
        (&made["title"], &made["request_count"]),
        (&"晴天".into(), &0.into())
    );

    // The same song, whatever the width and case; a category name already taken; a bad link.
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/song-list/songs",
            Some(song("晴天", "周杰倫")),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/song-list/categories",
            Some(serde_json::json!({ "name": " 華語 ", "requestable": false })),
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["code"], "songs.duplicate");
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/song-list/songs",
            Some(serde_json::json!({ "title": "x", "youtube_url": "https://example.com" })),
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["code"], "songs.invalid");

    let res = app
        .clone()
        .oneshot(req(
            Method::DELETE,
            &format!("/api/admin/song-list/categories/{category}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let list = json(
        app.oneshot(req(Method::GET, "/api/admin/song-list", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(list["categories"], serde_json::json!([]));
    assert_eq!(list["songs"][0]["title"], "晴天");
    assert_eq!(list["songs"][0]["category_id"], serde_json::Value::Null);
}

#[tokio::test]
async fn an_import_is_previewed_then_written_and_twice_gives_the_same() {
    let app = app().await;
    let csv: &'static [u8] =
        "\u{FEFF}歌名,歌手,分類,連結\n晴天,周杰倫,華語,https://youtu.be/dQw4w9WgXcQ\nLemon,米津玄師,日語,\n,沒有歌名,,\n".as_bytes();
    let res = app
        .clone()
        .oneshot(file("/api/admin/song-list/import/preview?format=csv", csv))
        .await
        .unwrap();
    let preview = json(res).await;
    assert_eq!(
        (&preview["added"], &preview["new_categories"]),
        (&2.into(), &serde_json::json!(["華語", "日語"]))
    );
    assert_eq!(preview["skipped"][0]["reason"], "no_title");
    // A preview writes nothing.
    let list = json(
        app.clone()
            .oneshot(req(Method::GET, "/api/admin/song-list", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(list["songs"], serde_json::json!([]));

    let res = app
        .clone()
        .oneshot(file("/api/admin/song-list/import?format=csv", csv))
        .await
        .unwrap();
    assert_eq!(json(res).await["added"], 2);
    let again = json(
        app.clone()
            .oneshot(file("/api/admin/song-list/import?format=csv", csv))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        (&again["added"], &again["updated"], &again["unchanged"]),
        (&0.into(), &0.into(), &2.into())
    );
    let list = json(
        app.clone()
            .oneshot(req(Method::GET, "/api/admin/song-list", None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(list["songs"].as_array().unwrap().len(), 2);
    assert_eq!(list["categories"][0]["songs"], 1);

    let res = app
        .oneshot(file(
            "/api/admin/song-list/import/preview?format=xlsx",
            b"not a workbook",
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["code"], "songs.unreadable_file");
}
