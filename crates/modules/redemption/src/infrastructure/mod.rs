//! Adapters: the SQLite repository.

pub(crate) mod db;

use sqlx::SqlitePool;

use crate::application::RedemptionService;

/// Starts recording into `pool`, which must already have [`db::MIGRATOR`] applied; needs a Tokio runtime.
pub fn start(pool: SqlitePool) -> crate::RedemptionService {
    RedemptionService::start(db::SqliteRedemptions::new(pool))
}
