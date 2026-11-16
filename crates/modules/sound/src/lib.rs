//! Sound management (thin module): a library of imported sound files with a label and volume each.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{SoundError, SoundService};
pub use domain::{Cue, MAX_BYTES};
pub use infrastructure::{AlertSound, PlaySound, file_routes, openapi, routes};
