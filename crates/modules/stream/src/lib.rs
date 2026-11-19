//! Stream sessions (thin module): whether the broadcaster is live, since when and for how many viewers, and one
//! session per Twitch stream, which stays the current one after the stream ends until the next one starts.

mod application;
mod domain;
mod infrastructure;

pub use application::StreamService;
pub use infrastructure::{
    StreamStateName, StreamStatusEvent, StreamStatusPayload,
    db::{LatestSession, MIGRATOR, latest_session, sessions},
    openapi, start,
};
