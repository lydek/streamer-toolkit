//! Follow, subscription, cheer and raid alerts (thin module): plays alerts on overlays one at a time while the
//! module is enabled.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{AlertsService, Timing};
pub use infrastructure::{AlertEvent, AlertKindName};
