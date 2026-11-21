//! Counters (thin module): global counts with a chat command each, shown on `/overlay/counter/`; a per-stream
//! counter goes back to 0 once when the next stream starts.

pub mod application;
mod domain;
mod infrastructure;

pub use infrastructure::{CounterValuesEvent, Counters, db::MIGRATOR, openapi, routes, start};
