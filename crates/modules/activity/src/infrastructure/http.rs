//! Admin API: the record retention, and how many records a shorter one would delete. The caller adds the token
//! check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::settings::RecordRetention;
use utoipa::{IntoParams, ToSchema};

use super::db::SqliteActivity;
use crate::application::{self, SetError};

#[derive(Clone)]
pub(crate) struct Http {
    repo: std::sync::Arc<SqliteActivity>,
    data_dir: PathBuf,
}

/// `others` are the other modules' record tables the retention also covers.
pub fn routes(pool: SqlitePool, data_dir: PathBuf, others: super::db::OtherRecords) -> Router {
    Router::new()
        .route(
            "/api/admin/records/retention",
            get(retention).put(set_retention),
        )
        .route("/api/admin/records/expired", get(expired))
        .with_state(Http {
            repo: std::sync::Arc::new(SqliteActivity::new(pool, others)),
            data_dir,
        })
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

/// How long chat and activity records are kept: `{"days": n}` or `"forever"`.
#[utoipa::path(
    get,
    path = "/api/admin/records/retention",
    tag = "admin",
    responses(
        (status = 200, body = RecordRetention),
        (status = 500, description = "`activity.retention_unreadable`: settings.json cannot be read", body = ErrorBody),
    )
)]
pub(crate) async fn retention(State(s): State<Http>) -> Response {
    match application::retention(&s.data_dir) {
        Some(r) => Json(r).into_response(),
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "activity.retention_unreadable",
        ),
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct Purged {
    /// Records deleted because the new retention no longer keeps them.
    deleted: u64,
}

/// Saves the retention (1-3650 days, or forever) and deletes the records it no longer keeps, together: a failed
/// save deletes nothing. The admin UI asks before shortening it and reads the retention again after an error.
#[utoipa::path(
    put,
    path = "/api/admin/records/retention",
    tag = "admin",
    request_body = RecordRetention,
    responses(
        (status = 200, body = Purged),
        (status = 400, description = "`activity.invalid_retention`", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`: nothing changed; `activity.storage_failed`: the delete did not commit, the new retention may be saved", body = ErrorBody),
    )
)]
pub(crate) async fn set_retention(
    State(s): State<Http>,
    body: Result<Json<RecordRetention>, JsonRejection>,
) -> Response {
    let Ok(Json(next)) = body else {
        return error(StatusCode::BAD_REQUEST, "activity.invalid_retention");
    };
    match application::set_retention(&*s.repo, &s.data_dir, next).await {
        Ok(deleted) => Json(Purged { deleted }).into_response(),
        Err(SetError::Invalid) => error(StatusCode::BAD_REQUEST, "activity.invalid_retention"),
        Err(SetError::SaveFailed) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed")
        }
        Err(SetError::Storage(e)) => {
            tracing::error!(error = %e, "old activity records not deleted");
            error(StatusCode::INTERNAL_SERVER_ERROR, "activity.storage_failed")
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ExpiredQuery {
    /// The retention to check, 1-3650 days.
    days: u16,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct ExpiredCount {
    /// Records older than that, which a change to it would delete now.
    count: u64,
}

/// How many records keeping only `days` days would delete now; asked before shortening the retention.
#[utoipa::path(
    get,
    path = "/api/admin/records/expired",
    tag = "admin",
    params(ExpiredQuery),
    responses(
        (status = 200, body = ExpiredCount),
        (status = 400, description = "`activity.invalid_retention`", body = ErrorBody),
        (status = 500, description = "`activity.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn expired(
    State(s): State<Http>,
    query: Result<Query<ExpiredQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let next = query.ok().map(|Query(q)| RecordRetention::Days(q.days));
    let Some(next) = next.filter(|r| r.valid()) else {
        return error(StatusCode::BAD_REQUEST, "activity.invalid_retention");
    };
    match application::expired(&*s.repo, next).await {
        Ok(count) => Json(ExpiredCount { count }).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "activity records not counted");
            error(StatusCode::INTERNAL_SERVER_ERROR, "activity.storage_failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{db, start};
    use axum::{body::Body, http::Request};
    use std::time::Duration;
    use toolkit_core::events::{Alert, AlertKind, Source};
    use toolkit_core::time::{self, Timestamp};
    use tower::ServiceExt;

    fn data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-activity-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn follow(id: &str, occurred_at: &str) -> Alert {
        Alert {
            id: id.into(),
            user_id: Some("7".into()),
            user_login: Some("fan".into()),
            user_name: Some("Fan".into()),
            kind: AlertKind::Follow,
            occurred_at: occurred_at.into(),
        }
    }

    async fn rows(pool: &SqlitePool) -> Vec<(String, Option<String>)> {
        sqlx::query_as(
            "SELECT twitch_message_id, session_id FROM activity_events ORDER BY occurred_at",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn until(pool: &SqlitePool, n: usize) {
        for _ in 0..100 {
            if rows(pool).await.len() == n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {n} rows, have {:?}", rows(pool).await);
    }

    #[tokio::test]
    async fn twitch_activity_is_stored_once_with_its_session_and_simulations_never() {
        let pool = db::tests::pool().await;
        let dir = data_dir("store");
        let service = start(
            pool.clone(),
            dir.clone(),
            || async { Some("session-1".into()) },
            Vec::new(),
        );
        let now = Timestamp::now().to_string();
        service.receive(follow("m1", &now), Source::Twitch);
        service.receive(follow("m1", &now), Source::Twitch);
        service.receive(follow("m2", &now), Source::Simulation);
        service.receive(follow("m3", &now), Source::Twitch);
        until(&pool, 2).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            rows(&pool).await,
            [
                ("m1".into(), Some("session-1".into())),
                ("m3".into(), Some("session-1".into()))
            ],
            "a redelivered notification stores once; a simulated one never"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn shortening_the_retention_deletes_older_records_and_forever_keeps_them() {
        let pool = db::tests::pool().await;
        let dir = data_dir("retention");
        // Forever first, so the start-up purge keeps the old records this test stores.
        toolkit_core::settings::update(&dir, |s| {
            s.record_retention = Some(RecordRetention::Forever)
        })
        .unwrap();
        let service = start(pool.clone(), dir.clone(), || async { None }, Vec::new());
        let days_ago = |d: i64| (Timestamp::now() - time::days(d)).to_string();
        service.receive(follow("old", &days_ago(40)), Source::Twitch);
        service.receive(follow("week", &days_ago(8)), Source::Twitch);
        service.receive(follow("new", &days_ago(0)), Source::Twitch);
        until(&pool, 3).await;
        let app = routes(pool.clone(), dir.clone(), Vec::new());
        let send = |req: Request<Body>| {
            let app = app.clone();
            async move {
                let res = app.oneshot(req).await.unwrap();
                let status = res.status();
                let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
                (
                    status,
                    serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default(),
                )
            }
        };
        let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
        let put = |body: &str| {
            Request::put("/api/admin/records/retention")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };

        assert_eq!(
            send(get("/api/admin/records/retention")).await.1,
            serde_json::json!("forever")
        );
        assert_eq!(
            send(get("/api/admin/records/expired?days=7")).await.1,
            serde_json::json!({"count": 2})
        );
        assert_eq!(
            send(get("/api/admin/records/expired?days=30")).await.1,
            serde_json::json!({"count": 1})
        );
        for bad in [
            "/api/admin/records/expired?days=0",
            "/api/admin/records/expired?days=3651",
            "/api/admin/records/expired",
        ] {
            assert_eq!(send(get(bad)).await.0, StatusCode::BAD_REQUEST, "{bad}");
        }

        let (status, body) = send(put(r#"{"days": 30}"#)).await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, serde_json::json!({"deleted": 1}))
        );
        assert_eq!(rows(&pool).await.len(), 2);
        assert_eq!(
            send(get("/api/admin/records/retention")).await.1,
            serde_json::json!({"days": 30})
        );
        let (status, body) = send(put(r#""forever""#)).await;
        assert_eq!(
            (status, body),
            (StatusCode::OK, serde_json::json!({"deleted": 0}))
        );
        assert_eq!(rows(&pool).await.len(), 2, "forever deletes nothing");
        // A start deletes what the saved retention no longer keeps (here the 8-day-old record).
        toolkit_core::settings::update(&dir, |s| {
            s.record_retention = Some(RecordRetention::Days(7))
        })
        .unwrap();
        let _restarted = start(pool.clone(), dir.clone(), || async { None }, Vec::new());
        until(&pool, 1).await;
        for bad in [r#"{"days": 0}"#, r#"{"days": 3651}"#, r#""never""#] {
            assert_eq!(send(put(bad)).await.0, StatusCode::BAD_REQUEST, "{bad}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn never_changed_means_30_days_and_an_unreadable_file_means_nothing() {
        let dir = data_dir("default");
        assert_eq!(
            application::retention(&dir),
            Some(RecordRetention::Days(30))
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        assert_eq!(application::retention(&dir), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_unreadable_settings_file_deletes_nothing_at_start() {
        let pool = db::tests::pool().await;
        let dir = data_dir("unreadable");
        toolkit_core::settings::update(&dir, |s| {
            s.record_retention = Some(RecordRetention::Forever)
        })
        .unwrap();
        let service = start(pool.clone(), dir.clone(), || async { None }, Vec::new());
        let old = (Timestamp::now() - time::days(400)).to_string();
        service.receive(follow("old", &old), Source::Twitch);
        until(&pool, 1).await;
        // Broken now: a start must not fall back to 30 days and delete the year-old record.
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        let _restarted = start(pool.clone(), dir.clone(), || async { None }, Vec::new());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(rows(&pool).await.len(), 1);
        let res = routes(pool.clone(), dir.clone(), Vec::new())
            .oneshot(
                Request::get("/api/admin/records/retention")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_save_deletes_nothing() {
        let pool = db::tests::pool().await;
        let dir = data_dir("save-fails");
        toolkit_core::settings::update(&dir, |s| {
            s.record_retention = Some(RecordRetention::Forever)
        })
        .unwrap();
        let service = start(pool.clone(), dir.clone(), || async { None }, Vec::new());
        let old = (Timestamp::now() - time::days(40)).to_string();
        service.receive(follow("old", &old), Source::Twitch);
        until(&pool, 1).await;
        // settings.json can no longer be written (a folder in its place).
        std::fs::remove_file(dir.join("settings.json")).unwrap();
        std::fs::create_dir(dir.join("settings.json")).unwrap();
        let res = routes(pool.clone(), dir.clone(), Vec::new())
            .oneshot(
                Request::put("/api/admin/records/retention")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"days": 30}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            rows(&pool).await.len(),
            1,
            "the delete rolled back with the failed save"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_delete_saves_nothing() {
        // A read-only database: the delete fails, so the retention must stay forever.
        let dir = data_dir("readonly");
        toolkit_core::settings::update(&dir, |s| {
            s.record_retention = Some(RecordRetention::Forever)
        })
        .unwrap();
        let file = dir.join("ro.db");
        let writable = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&file)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        db::MIGRATOR.run(&writable).await.unwrap();
        writable.close().await;
        let readonly = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&file)
                    .read_only(true),
            )
            .await
            .unwrap();
        let res = routes(readonly.clone(), dir.clone(), Vec::new())
            .oneshot(
                Request::put("/api/admin/records/retention")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"days": 1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(application::retention(&dir), Some(RecordRetention::Forever));
        readonly.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
