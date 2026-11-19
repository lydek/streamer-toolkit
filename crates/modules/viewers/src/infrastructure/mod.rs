//! Wires the viewer directory to SQLite.

use std::sync::Arc;

use sqlx::SqlitePool;
use toolkit_core::twitch::ProfileImages;

use crate::application::ViewerService;

pub mod db;

/// Starts the viewer directory in `pool`, which must already have [`db::MIGRATOR`] applied, looking up avatars
/// with `profiles`; needs a Tokio runtime.
pub fn start(pool: SqlitePool, profiles: Arc<dyn ProfileImages>) -> ViewerService {
    ViewerService::start(db::SqliteViewers::new(pool), profiles)
}
