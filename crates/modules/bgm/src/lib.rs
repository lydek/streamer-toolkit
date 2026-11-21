//! Background music (thin module): a music library of imported files, kept apart from the sound library, and
//! playlists that group its tracks, and the player that plays a playlist on `/overlay/bgm/`.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{BgmError, BgmService, player::BgmPlayer};
pub use domain::MAX_BYTES;
pub use infrastructure::{BgmPlayerEvent, file_routes, openapi, routes, start};
