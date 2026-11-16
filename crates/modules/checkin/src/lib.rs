//! Check-ins (complex module): one per viewer per business day, judged in the broadcaster's time zone, with a
//! stamp card derived from the total (docs/modules/checkin.md).

pub mod application;
mod domain;
mod infrastructure;

pub use application::CheckinService;
pub use infrastructure::{
    CheckinChangedEvent, CheckinChangedPayload,
    db::MIGRATOR,
    http::{CheckinSettingsBody, routes},
    openapi, start, subscriptions,
};
