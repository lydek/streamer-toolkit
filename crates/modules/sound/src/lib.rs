//! Sound management (thin module): a library of imported sound files with a label and volume each, the slots that
//! choose what alerts, check-ins and channel point rewards play, and the reward sounds for `/overlay/sound/`.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{SoundError, SoundService};
pub use domain::{Cue, MAX_BYTES};
pub use infrastructure::{AlertSound, PlaySound, SoundPlayEvent, file_routes, openapi, routes};
