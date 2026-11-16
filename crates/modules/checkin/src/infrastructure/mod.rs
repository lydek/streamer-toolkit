//! Adapters: the SQLite repository and the admin API.

pub(crate) mod db;
pub(crate) mod http;

use std::{path::PathBuf, sync::Arc};

use sqlx::SqlitePool;
use toolkit_core::modules::Modules;
use utoipa::OpenApi;

use crate::application::{CheckinService, Config};

/// Starts check-ins into `pool`, which must already have [`db::MIGRATOR`] applied, with the settings in
/// `data_dir`; needs a Tokio runtime.
pub fn start(pool: SqlitePool, modules: Arc<Modules>, data_dir: PathBuf) -> CheckinService {
    CheckinService::start(
        db::SqliteCheckins::new(pool),
        modules,
        Arc::new(Config::load(data_dir)),
    )
}

#[derive(OpenApi)]
#[openapi(paths(http::checkin_settings, http::set_checkin_settings))]
struct CheckinApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    CheckinApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::{data_dir, pool};
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    async fn send(
        app: &axum::Router,
        method: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri("/api/admin/checkin/settings")
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn settings_are_validated_saved_and_applied() {
        let dir = data_dir("http-settings");
        let modules = Arc::new(Modules::load(dir.clone()));
        let app = http::routes(Arc::new(start(pool().await, modules, dir.clone())));

        let (status, body) = send(&app, "GET", None).await;
        assert_eq!(status, 200);
        assert_eq!(
            body,
            serde_json::json!({ "command": "打卡", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": 10 })
        );

        let next = serde_json::json!({ "command": " !Sign ", "time_zone": "Europe/Berlin", "reset_hour": 6, "card_slots": 8 });
        let (status, body) = send(&app, "PUT", Some(next)).await;
        assert_eq!(status, 200);
        assert_eq!(body["command"], "Sign", "stored without its !");
        let saved = toolkit_core::settings::load(&dir).checkin;
        assert_eq!(
            (
                saved.command.as_str(),
                saved.time_zone.as_str(),
                saved.reset_hour,
                saved.card_slots
            ),
            ("Sign", "Europe/Berlin", 6, 8)
        );

        for bad in [
            serde_json::json!({ "command": "two words", "time_zone": "UTC", "reset_hour": 0, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "Nowhere/City", "reset_hour": 0, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "UTC", "reset_hour": 24, "card_slots": 10 }),
            serde_json::json!({ "command": "x", "time_zone": "UTC", "reset_hour": 0, "card_slots": 0 }),
        ] {
            let (status, body) = send(&app, "PUT", Some(bad.clone())).await;
            assert_eq!(
                (status, &body["code"]),
                (400, &serde_json::json!("checkin.invalid_settings")),
                "{bad}"
            );
        }
        let (_, body) = send(&app, "GET", None).await;
        assert_eq!(body["command"], "Sign", "a refused update changes nothing");
        let _ = std::fs::remove_dir_all(dir);
    }
}
