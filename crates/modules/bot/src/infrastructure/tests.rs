use super::*;
use crate::application::{BotLookups, BoxFuture, ChatSender};
use axum::body::Body;
use axum::http::{Method, Request, header};
use toolkit_core::{commands::CommandTable, modules::Modules};
use tower::ServiceExt;

struct NoSender;
impl ChatSender for NoSender {
    fn can_send(&self) -> bool {
        false
    }
    fn send(&self, _: String) -> BoxFuture<Result<String, String>> {
        Box::pin(async { Err("not used".into()) })
    }
}
struct NoLookups;
impl BotLookups for NoLookups {
    fn counter_value(&self, _: String) -> BoxFuture<Option<i64>> {
        Box::pin(async { None })
    }
}

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

async fn json(res: Response) -> serde_json::Value {
    let body = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
    serde_json::from_slice(&body).unwrap_or_default()
}

#[tokio::test]
async fn commands_are_added_edited_and_deleted_and_never_clash_with_another_feature() {
    let dir = std::env::temp_dir().join(format!("st-bot-routes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let table = Arc::new(CommandTable::load(dir.clone(), vec![]));
    // Another feature's command.
    table
        .set(
            "counter.k",
            CommandEntry {
                name: "死亡".into(),
                allowed: Allowed::Everyone,
                module: "counter".into(),
                label: Some("死亡次數".into()),
            },
            |_| {},
        )
        .unwrap();
    let service = BotService::start(
        dir.clone(),
        table,
        Arc::new(Modules::load(dir.clone())),
        Arc::new(NoSender),
        Arc::new(NoLookups),
        Arc::new(|_, _| {}),
    );
    let app = routes(service);
    let command = |name: &str, reply: &str| serde_json::json!({ "name": name, "allowed": "subscriber", "reply": reply, "cooldown_seconds": 30, "enabled": true });

    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/bot/commands",
            Some(command(" !discord ", " 歡迎加入 ")),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let made = json(res).await;
    assert_eq!(
        (&made["name"], &made["reply"], &made["allowed"]),
        (&"discord".into(), &"歡迎加入".into(), &"subscriber".into())
    );
    let id = made["id"].as_str().unwrap().to_owned();
    let saved = settings::load(&dir);
    assert_eq!(saved.bot.commands[0].reply, "歡迎加入");
    assert_eq!(saved.chat_commands[&format!("bot.{id}")].name, "discord");

    // Taken by the counter, or not a command at all.
    let res = app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/admin/bot/commands",
            Some(command("死亡", "x")),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json(res).await["label"], "死亡次數");
    for bad in [
        command("two words", "x"),
        command("ok", " "),
        command("ok", &"字".repeat(501)),
    ] {
        let res = app
            .clone()
            .oneshot(req(Method::POST, "/api/admin/bot/commands", Some(bad)))
            .await
            .unwrap();
        assert_eq!(json(res).await["code"], "bot.invalid");
    }

    let res = app
        .clone()
        .oneshot(req(
            Method::PUT,
            &format!("/api/admin/bot/commands/{id}"),
            Some(command("dc", "改了")),
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["name"], "dc");
    let res = app
        .clone()
        .oneshot(req(Method::GET, "/api/admin/bot/commands", None))
        .await
        .unwrap();
    assert_eq!(json(res).await.as_array().unwrap().len(), 1);

    let res = app
        .clone()
        .oneshot(req(
            Method::DELETE,
            &format!("/api/admin/bot/commands/{id}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let saved = settings::load(&dir);
    assert!(saved.bot.commands.is_empty());
    assert!(!saved.chat_commands.contains_key(&format!("bot.{id}")));
    let res = app
        .oneshot(req(
            Method::DELETE,
            &format!("/api/admin/bot/commands/{id}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}
