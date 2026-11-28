//! The live event sockets on a served Host: `/ws/admin` opens only after the first message carries the admin token;
//! `/ws/overlay` needs none and sends each overlay kind only what its page reads.

use std::time::Duration;

use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as WsMessage, client::IntoClientRequest},
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Serves the whole Host router on a free port; requests still name the configured port, as the guard expects.
async fn serve() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app()).await });
    port
}

async fn connect(port: u16, path: &str) -> Socket {
    let mut req = format!("ws://127.0.0.1:{port}{path}")
        .into_client_request()
        .unwrap();
    let headers = req.headers_mut();
    headers.insert(header::HOST, "127.0.0.1:47810".parse().unwrap());
    headers.insert(header::ORIGIN, "http://localhost:5173".parse().unwrap());
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

/// The next frame, or `None` once the socket is closed.
async fn next(socket: &mut Socket) -> Option<WsMessage> {
    tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("a frame or the close in time")
        .and_then(Result::ok)
}

async fn overlay_counts(port: u16) -> serde_json::Value {
    let res = http(port, "GET", "/api/admin/overlays", "").await;
    serde_json::from_str(&res).unwrap()
}

/// A plain HTTP/1.1 request with the admin token, read to the end (no HTTP client among the Host's dependencies).
async fn http(port: u16, method: &str, path: &str, json: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:47810\r\nOrigin: http://localhost:5173\r\n\
         Authorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{json}",
        json.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut res = String::new();
    stream.read_to_string(&mut res).await.unwrap();
    res.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn the_right_token_first_opens_the_stream() {
    let port = serve().await;
    let mut socket = connect(port, "/ws/admin").await;
    socket.send(WsMessage::text(TOKEN)).await.unwrap();
    let Some(WsMessage::Text(first)) = next(&mut socket).await else {
        panic!("a snapshot first");
    };
    let snapshot: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(snapshot["type"], "snapshot");
    let counts = overlay_counts(port).await;
    assert_eq!(
        counts["other"], 0,
        "the admin window is no overlay: {counts}"
    );
}

#[tokio::test]
async fn a_wrong_token_gets_nothing_and_a_close() {
    let port = serve().await;
    let mut socket = connect(port, "/ws/admin").await;
    socket.send(WsMessage::text("not-the-token")).await.unwrap();
    assert!(
        matches!(next(&mut socket).await, Some(WsMessage::Close(_)) | None),
        "no snapshot before the token checks out"
    );
}

#[tokio::test]
async fn no_token_in_time_gets_nothing_and_a_close() {
    let port = serve().await;
    let mut socket = connect(port, "/ws/admin").await;
    assert!(matches!(
        next(&mut socket).await,
        Some(WsMessage::Close(_)) | None
    ));
}

#[tokio::test]
async fn the_overlay_socket_still_needs_no_token() {
    let port = serve().await;
    let mut socket = connect(port, "/ws/overlay").await;
    let Some(WsMessage::Text(first)) = next(&mut socket).await else {
        panic!("a snapshot first");
    };
    assert!(first.contains("\"snapshot\""));
}

/// The snapshot a socket gets first, as JSON.
async fn snapshot(socket: &mut Socket) -> serde_json::Value {
    let Some(WsMessage::Text(first)) = next(socket).await else {
        panic!("a snapshot first");
    };
    serde_json::from_str(&first).unwrap()
}

/// The next event's type, or `None` when nothing comes within a moment.
async fn next_type(socket: &mut Socket) -> Option<String> {
    let frame = tokio::time::timeout(Duration::from_millis(500), socket.next())
        .await
        .ok()??;
    let WsMessage::Text(text) = frame.ok()? else {
        return None;
    };
    let event: serde_json::Value = serde_json::from_str(&text).unwrap();
    event["type"].as_str().map(str::to_owned)
}

#[tokio::test]
async fn only_the_chat_overlay_and_the_admin_window_get_chat() {
    let port = serve().await;
    let chat = r#"{"text":"hi","userName":"Tester"}"#;
    http(port, "POST", "/api/admin/simulate/chat", chat).await;

    let mut admin = connect(port, "/ws/admin").await;
    admin.send(WsMessage::text(TOKEN)).await.unwrap();
    let mut sockets = vec![("admin", admin)];
    for (name, path) in [
        ("chat", "/ws/overlay?kind=chat"),
        ("alerts", "/ws/overlay?kind=alerts"),
        ("none", "/ws/overlay"),
        ("unknown", "/ws/overlay?kind=nope"),
    ] {
        sockets.push((name, connect(port, path).await));
    }
    for (name, socket) in &mut sockets {
        let snapshot = snapshot(socket).await;
        let messages = snapshot["payload"]["chat.message"]
            .as_array()
            .unwrap()
            .len();
        let gets_chat = matches!(*name, "admin" | "chat");
        assert_eq!(
            messages,
            usize::from(gets_chat),
            "{name}: the key stays, empty when not sent"
        );
    }

    http(port, "POST", "/api/admin/simulate/chat", chat).await;
    for (name, socket) in &mut sockets {
        // Other events (the presentation queue, say) may come first.
        let mut got = Vec::new();
        while let Some(event_type) = next_type(socket).await {
            got.push(event_type);
        }
        let gets_chat = matches!(*name, "admin" | "chat");
        assert_eq!(
            got.iter().any(|t| t == "chat.message"),
            gets_chat,
            "{name}: {got:?}"
        );
    }
}
