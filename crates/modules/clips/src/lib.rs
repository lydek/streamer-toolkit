//! SO clips (thin module): a queue of Twitch clips looked up on Helix, to play through the official clip embed.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{ClipError, ClipLookup, ClipsService, LookupError};
pub use domain::{Clip, ClipId};
pub use infrastructure::{openapi, routes};
