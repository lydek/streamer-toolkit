//! Tables of viewer records that share the broadcaster's record retention (`settings::RecordRetention`). The
//! activity module keeps the retention; each other module hands it its own table as a [`RecordTable`], so no module
//! touches another's table.

use std::{future::Future, pin::Pin};

use sqlx::SqliteConnection;

pub type RecordFuture<'c> = Pin<Box<dyn Future<Output = Result<u64, sqlx::Error>> + Send + 'c>>;

pub trait RecordTable: Send + Sync {
    /// Deletes the records from before `cutoff` (UTC, fixed-width RFC 3339 with microseconds) on `conn`, which may
    /// be inside the caller's transaction; returns how many.
    fn delete_before<'c>(
        &'c self,
        conn: &'c mut SqliteConnection,
        cutoff: &'c str,
    ) -> RecordFuture<'c>;
    /// How many records are from before `cutoff`.
    fn count_before<'c>(
        &'c self,
        conn: &'c mut SqliteConnection,
        cutoff: &'c str,
    ) -> RecordFuture<'c>;
}
