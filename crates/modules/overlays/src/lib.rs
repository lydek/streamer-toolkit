//! Screen tools (thin module): the break / starting-soon card on `/overlay/break/`, whose countdown end is stored so
//! a restart or an overlay reload shows the same time, and small on-stream warnings on `/overlay/warnings/` when the
//! chat overlay or Twitch is down or the mic stays muted. The built-in overlays share a theme and text size. GIFs and
//! videos imported into `media/` can show on a redemption.

pub mod application;
mod domain;
mod infrastructure;

pub use domain::warnings::Conditions;
pub use infrastructure::{
    AppearanceEvent, BreakEvent, Deps, MediaPlayEvent, Overlays, WarningsEvent, db::MIGRATOR,
    media_file_routes, openapi, routes, start,
};
