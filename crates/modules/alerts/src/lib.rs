//! Follow, subscription, cheer and raid alerts (thin module): forwards alerts to overlays while the module is
//! enabled.

pub mod application;
mod domain;
mod infrastructure;

pub use application::AlertsService;
pub use infrastructure::{AlertEvent, AlertKindName};
