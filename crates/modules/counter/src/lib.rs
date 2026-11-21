//! Counters (thin module): global counts with a chat command each, shown on `/overlay/counter/`; a per-stream
//! counter goes back to 0 once when the next stream starts. Counts kept per viewer (keyed by Twitch user ID, named
//! by the viewers module) rank who has the most.

pub mod application;
mod domain;
mod infrastructure;

pub use infrastructure::{CounterValuesEvent, Counters, db::MIGRATOR, openapi, routes, start};
