//! Adapters: the admin API (behind the admin token, added by the host), the `counter.values` event and storage.

pub mod db;

use std::{future::Future, sync::Arc};

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    commands::{Allowed, CommandError, CommandTable, CommandTakenBody},
    events::Source,
    modules::Modules,
};
use utoipa::{OpenApi, ToSchema};

use crate::{
    application::{CounterError, CounterService, CounterSettings, ViewerCounterSummary},
    domain::{Counter, ViewerCount},
};

/// The counter service the host runs.
pub type Counters = CounterService<db::SqliteCounters>;

const VALUES: &str = "counter.values";
const VALUES_VERSION: u32 = 1;

/// One counter on the overlay. `name` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CounterValue {
    pub id: String,
    pub name: String,
    pub value: i64,
}

/// The `payload` of a `counter.values` event: every counter, in creation order.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CounterValuesPayload {
    pub counters: Vec<CounterValue>,
}

/// A `counter.values` event exactly as `/ws/overlay` sends it after any change; the snapshot keeps only the
/// latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CounterValuesEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: CounterValuesType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: CounterValuesPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum CounterValuesType {
    #[serde(rename = "counter.values")]
    CounterValues,
}

/// Starts counters in `pool`, which must already have [`db::MIGRATOR`] applied, announcing every change as
/// `counter.values` on `bus`; `session` gives the stream session now. Needs a Tokio runtime.
pub fn start<F>(
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    commands: Arc<CommandTable>,
    session: impl Fn() -> F + Send + Sync + 'static,
) -> Arc<Counters>
where
    F: Future<Output = Option<String>> + Send + 'static,
{
    bus.retain(VALUES, 1);
    CounterService::start(
        db::SqliteCounters::new(pool),
        commands,
        modules,
        Box::new(move || Box::pin(session())),
        Box::new(move |all| {
            let payload = CounterValuesPayload {
                counters: all
                    .iter()
                    .map(|c| CounterValue {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        value: c.value,
                    })
                    .collect(),
            };
            bus.publish(Event {
                id: uuid::Uuid::now_v7().to_string(),
                event_type: VALUES,
                version: VALUES_VERSION,
                occurred_at: chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                source: Source::System,
                payload: serde_json::to_value(payload).expect("counter values serialize"),
            });
        }),
    )
}

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn counter_error(e: CounterError) -> Response {
    match e {
        CounterError::Invalid => error(StatusCode::BAD_REQUEST, "counter.invalid"),
        CounterError::NotFound => error(StatusCode::NOT_FOUND, "counter.not_found"),
        CounterError::ViewerNotFound => error(StatusCode::NOT_FOUND, "counter.viewer_not_found"),
        CounterError::Command(CommandError::Taken { module, label }) => (
            StatusCode::CONFLICT,
            Json(CommandTakenBody::new(module, label)),
        )
            .into_response(),
        CounterError::Command(e @ CommandError::SaveFailed(_)) => {
            tracing::error!(error = %e, "counter command not saved");
            error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed")
        }
        CounterError::Storage(e) => {
            tracing::error!(code = "counter.storage_failed", error = %e.0, "counter not stored");
            error(StatusCode::INTERNAL_SERVER_ERROR, "counter.storage_failed")
        }
    }
}

/// A counter with its command. `name` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CounterBody {
    pub id: String,
    pub name: String,
    pub value: i64,
    /// The chat command without its `!`; empty: none.
    pub command: String,
    /// Who may use the command; the broadcaster always may.
    pub allowed: Allowed,
    /// Back to 0 when the next stream starts.
    pub per_stream: bool,
}

fn body(s: &Counters, c: Counter) -> CounterBody {
    let command = s.command(&c.id);
    CounterBody {
        id: c.id,
        name: c.name,
        value: c.value,
        command: command.name,
        allowed: command.allowed,
        per_stream: c.per_stream,
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CounterInput {
    /// 1–100 characters after trimming.
    pub name: String,
    /// One word of at most 25 characters, with or without its `!`; empty: no chat command. No other command may
    /// have it (ignoring case).
    pub command: String,
    pub allowed: Allowed,
    pub per_stream: bool,
}

impl From<CounterInput> for CounterSettings {
    fn from(i: CounterInput) -> Self {
        Self {
            name: i.name,
            command: i.command,
            allowed: i.allowed,
            per_stream: i.per_stream,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CounterStep {
    /// `1` or `-1`; a counter never goes below 0.
    pub delta: i64,
}

/// One viewer's count. `login` and `display_name` are viewer text: render them as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ViewerCountBody {
    pub twitch_user_id: String,
    pub login: String,
    pub display_name: String,
    pub value: i64,
}

impl From<ViewerCount> for ViewerCountBody {
    fn from(v: ViewerCount) -> Self {
        Self {
            twitch_user_id: v.twitch_user_id,
            login: v.login,
            display_name: v.display_name,
            value: v.value,
        }
    }
}

/// A count kept per viewer. `name` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ViewerCounterBody {
    pub id: String,
    pub name: String,
    /// Every viewer's count added up.
    pub total: i64,
    /// The three viewers with the most, most first.
    pub top: Vec<ViewerCountBody>,
}

impl From<ViewerCounterSummary> for ViewerCounterBody {
    fn from(s: ViewerCounterSummary) -> Self {
        Self {
            id: s.counter.id,
            name: s.counter.name,
            total: s.total,
            top: s.top.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ViewerCounterName {
    /// 1–100 characters after trimming.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ViewerStep {
    /// A viewer from the viewer directory.
    pub twitch_user_id: String,
    /// `1` or `-1`; a count never goes below 0.
    pub delta: i64,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ViewerSearch {
    /// Part of a login or display name.
    pub q: String,
}

/// The admin API.
pub fn routes(service: Arc<Counters>) -> Router {
    Router::new()
        .route(
            "/api/admin/counters/viewers",
            get(list_viewer_counters).post(create_viewer_counter),
        )
        .route(
            "/api/admin/counters/viewers/{id}",
            put(rename_viewer_counter).delete(delete_viewer_counter),
        )
        .route(
            "/api/admin/counters/viewers/{id}/counts",
            get(viewer_counts),
        )
        .route(
            "/api/admin/counters/viewers/{id}/search",
            get(search_viewers),
        )
        .route("/api/admin/counters/viewers/{id}/add", post(add_for_viewer))
        .route(
            "/api/admin/counters",
            get(list_counters).post(create_counter),
        )
        .route(
            "/api/admin/counters/{id}",
            put(update_counter).delete(delete_counter),
        )
        .route("/api/admin/counters/{id}/add", post(add_to_counter))
        .route("/api/admin/counters/{id}/reset", post(reset_counter))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/counters",
    tag = "admin",
    responses(
        (status = 200, body = Vec<CounterBody>),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn list_counters(State(s): State<Arc<Counters>>) -> Response {
    match s.list().await {
        Ok(all) => Json(all.into_iter().map(|c| body(&s, c)).collect::<Vec<_>>()).into_response(),
        Err(e) => counter_error(e.into()),
    }
}

/// Adds a counter at 0. A per-stream one counts the stream on now as already reset.
#[utoipa::path(
    post,
    path = "/api/admin/counters",
    tag = "admin",
    request_body = CounterInput,
    responses(
        (status = 200, body = CounterBody),
        (status = 400, description = "`counter.invalid`: an empty name, a command that is not one word, or more than 100 counters", body = ErrorBody),
        (status = 409, description = "`commands.taken`: another command has the name", body = CommandTakenBody),
        (status = 500, description = "`counter.storage_failed` or `settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn create_counter(
    State(s): State<Arc<Counters>>,
    input: Result<Json<CounterInput>, JsonRejection>,
) -> Response {
    let Ok(Json(input)) = input else {
        return error(StatusCode::BAD_REQUEST, "counter.invalid");
    };
    match s.create(input.into()).await {
        Ok(c) => Json(body(&s, c)).into_response(),
        Err(e) => counter_error(e),
    }
}

/// Changes a counter's settings; its value stays. Becoming per-stream counts the stream on now as already reset.
#[utoipa::path(
    put,
    path = "/api/admin/counters/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = CounterInput,
    responses(
        (status = 200, body = CounterBody),
        (status = 400, description = "`counter.invalid`", body = ErrorBody),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 409, description = "`commands.taken`: another command has the name", body = CommandTakenBody),
        (status = 500, description = "`counter.storage_failed` or `settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn update_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
    input: Result<Json<CounterInput>, JsonRejection>,
) -> Response {
    let Ok(Json(input)) = input else {
        return error(StatusCode::BAD_REQUEST, "counter.invalid");
    };
    match s.update(&id, input.into()).await {
        Ok(c) => Json(body(&s, c)).into_response(),
        Err(e) => counter_error(e),
    }
}

/// Deletes a counter and its chat command.
#[utoipa::path(
    delete,
    path = "/api/admin/counters/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => counter_error(e),
    }
}

/// Adds or takes one.
#[utoipa::path(
    post,
    path = "/api/admin/counters/{id}/add",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = CounterStep,
    responses(
        (status = 204),
        (status = 400, description = "`counter.invalid`: not 1 or -1", body = ErrorBody),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn add_to_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
    step: Result<Json<CounterStep>, JsonRejection>,
) -> Response {
    match step {
        Ok(Json(CounterStep {
            delta: d @ (1 | -1),
        })) => match s.add(&id, d).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => counter_error(e),
        },
        _ => error(StatusCode::BAD_REQUEST, "counter.invalid"),
    }
}

/// Puts a counter back to 0.
#[utoipa::path(
    post,
    path = "/api/admin/counters/{id}/reset",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn reset_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
) -> Response {
    match s.reset(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => counter_error(e),
    }
}

/// The per-viewer counters with their totals and top three.
#[utoipa::path(
    get,
    path = "/api/admin/counters/viewers",
    tag = "admin",
    responses(
        (status = 200, body = Vec<ViewerCounterBody>),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn list_viewer_counters(State(s): State<Arc<Counters>>) -> Response {
    match s.viewer_counters().await {
        Ok(all) => Json(
            all.into_iter()
                .map(ViewerCounterBody::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => counter_error(e.into()),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/counters/viewers",
    tag = "admin",
    request_body = ViewerCounterName,
    responses(
        (status = 200, body = ViewerCounterBody),
        (status = 400, description = "`counter.invalid`: an empty name, or more than 100 per-viewer counters", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn create_viewer_counter(
    State(s): State<Arc<Counters>>,
    body: Result<Json<ViewerCounterName>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "counter.invalid");
    };
    match s.create_viewer_counter(&b.name).await {
        Ok(counter) => Json(ViewerCounterBody::from(ViewerCounterSummary {
            counter,
            total: 0,
            top: Vec::new(),
        }))
        .into_response(),
        Err(e) => counter_error(e),
    }
}

#[utoipa::path(
    put,
    path = "/api/admin/counters/viewers/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = ViewerCounterName,
    responses(
        (status = 204),
        (status = 400, description = "`counter.invalid`: an empty name", body = ErrorBody),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn rename_viewer_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
    body: Result<Json<ViewerCounterName>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "counter.invalid");
    };
    match s.rename_viewer_counter(&id, &b.name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => counter_error(e),
    }
}

/// Deletes a per-viewer counter and every viewer's count in it.
#[utoipa::path(
    delete,
    path = "/api/admin/counters/viewers/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_viewer_counter(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete_viewer_counter(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => counter_error(e),
    }
}

/// Every viewer above 0, most first.
#[utoipa::path(
    get,
    path = "/api/admin/counters/viewers/{id}/counts",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 200, body = Vec<ViewerCountBody>),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn viewer_counts(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
) -> Response {
    match s.viewer_counts(&id).await {
        Ok(all) => Json(
            all.into_iter()
                .map(ViewerCountBody::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => counter_error(e),
    }
}

/// Viewers in the viewer directory whose login or display name contains `q` (at most 20, most recently seen
/// first), with their count in this counter.
#[utoipa::path(
    get,
    path = "/api/admin/counters/viewers/{id}/search",
    tag = "admin",
    params(("id" = String, Path), ViewerSearch),
    responses(
        (status = 200, body = Vec<ViewerCountBody>),
        (status = 400, description = "`counter.invalid`: no `q`", body = ErrorBody),
        (status = 404, description = "`counter.not_found`", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn search_viewers(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
    query: Result<Query<ViewerSearch>, QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "counter.invalid");
    };
    match s.search_viewers(&id, &q.q).await {
        Ok(found) => Json(
            found
                .into_iter()
                .map(ViewerCountBody::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => counter_error(e),
    }
}

/// Adds or takes one for a viewer.
#[utoipa::path(
    post,
    path = "/api/admin/counters/viewers/{id}/add",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = ViewerStep,
    responses(
        (status = 204),
        (status = 400, description = "`counter.invalid`: not 1 or -1", body = ErrorBody),
        (status = 404, description = "`counter.not_found`, or `counter.viewer_not_found`: not in the viewer directory", body = ErrorBody),
        (status = 500, description = "`counter.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn add_for_viewer(
    State(s): State<Arc<Counters>>,
    Path(id): Path<String>,
    step: Result<Json<ViewerStep>, JsonRejection>,
) -> Response {
    match step {
        Ok(Json(ViewerStep {
            twitch_user_id,
            delta: d @ (1 | -1),
        })) => match s.add_for_viewer(&id, &twitch_user_id, d).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => counter_error(e),
        },
        _ => error(StatusCode::BAD_REQUEST, "counter.invalid"),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_viewer_counters,
        create_viewer_counter,
        rename_viewer_counter,
        delete_viewer_counter,
        viewer_counts,
        search_viewers,
        add_for_viewer,
        list_counters,
        create_counter,
        update_counter,
        delete_counter,
        add_to_counter,
        reset_counter
    ),
    components(schemas(CounterValuesEvent))
)]
struct CounterApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    CounterApi::openapi()
}

#[cfg(test)]
mod tests;
