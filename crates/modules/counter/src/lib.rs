//! Counters (thin module): global counts with a chat command each, shown on `/overlay/counter/`; a per-stream
//! counter goes back to 0 once when the next stream starts. Counts kept per viewer (keyed by Twitch user ID, named
//! by the viewers module) rank who has the most. The sub marathon timer counts down while live, on
//! `/overlay/marathon/`, and subs and bits add time.

pub mod application;
mod domain;
mod infrastructure;

pub use application::StreamState;
pub use infrastructure::{
    CounterValuesEvent, Counters, MarathonEvent, db::MIGRATOR, openapi, routes, start,
};
