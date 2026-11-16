//! Sound management (thin module): a library of imported sound files with a label and volume each.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{SoundError, SoundService};
pub use domain::MAX_BYTES;
pub use infrastructure::{file_routes, openapi, routes};
