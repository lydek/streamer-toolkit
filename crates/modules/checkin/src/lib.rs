//! Check-ins (complex module): one per viewer per business day, judged in the broadcaster's time zone, with a
//! stamp card derived from the total (docs/modules/checkin.md).

pub mod application;
mod domain;
mod infrastructure;

pub use application::{CheckinService, ProfileImages, Timing};
pub use infrastructure::{
    CheckinChangedEvent, CheckinChangedPayload, CheckinStampedEvent, CheckinStampedPayload,
    db::MIGRATOR,
    http::{CheckinSettingsBody, routes},
    openapi, start, subscriptions,
};
