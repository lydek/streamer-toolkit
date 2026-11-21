//! Activity records (thin module): follows, subs, resubs, gifts, cheers and raids, stored with the stream session
//! they happened in and kept for the record retention the broadcaster chose.

mod application;
mod domain;
mod infrastructure;

pub use application::ActivityService;
pub use infrastructure::{
    db::{
        ActivityEntry, MIGRATOR, OtherRecords, SessionCounts, activity_between, session_counts,
        session_entries,
    },
    http::routes,
    openapi, start,
};
