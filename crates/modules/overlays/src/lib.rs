//! Screen tools (thin module): the break / starting-soon card on `/overlay/break/`, whose countdown end is stored so
//! a restart or an overlay reload shows the same time.

pub mod application;
mod domain;
mod infrastructure;

pub use infrastructure::{BreakEvent, Overlays, db::MIGRATOR, openapi, routes, start};
