//! Background music (thin module): a music library of imported files, kept apart from the sound library, and
//! playlists that group its tracks.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{BgmError, BgmService};
pub use domain::MAX_BYTES;
pub use infrastructure::{file_routes, openapi, routes};
