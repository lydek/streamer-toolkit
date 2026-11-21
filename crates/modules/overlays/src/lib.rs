//! Screen tools (thin module): the break / starting-soon card on `/overlay/break/`, whose countdown end is stored so
//! a restart or an overlay reload shows the same time, and small on-stream warnings on `/overlay/warnings/` when the
//! chat overlay or Twitch is down or the mic stays muted.

pub mod application;
mod domain;
mod infrastructure;

pub use domain::warnings::Conditions;
pub use infrastructure::{
    BreakEvent, Overlays, WarningsEvent, db::MIGRATOR, openapi, routes, start,
};
