//! Wires the activity records to SQLite and the admin API.

use std::{future::Future, path::PathBuf, sync::Arc};

use sqlx::SqlitePool;
use utoipa::OpenApi;

use crate::application::ActivityService;

pub mod db;
pub mod http;

/// Starts activity records in `pool`, which must already have [`db::MIGRATOR`] applied, with the retention in
/// `data_dir`; `session` asks for the stream session an event arriving now belongs to. Needs a Tokio runtime.
pub fn start<F>(
    pool: SqlitePool,
    data_dir: PathBuf,
    session: impl Fn() -> F + Send + Sync + 'static,
) -> ActivityService
where
    F: Future<Output = Option<String>> + Send + 'static,
{
    ActivityService::start(
        db::SqliteActivity::new(pool),
        data_dir,
        Arc::new(move || Box::pin(session())),
    )
}

#[derive(OpenApi)]
#[openapi(paths(http::retention, http::set_retention, http::expired))]
struct ActivityApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ActivityApi::openapi()
}
