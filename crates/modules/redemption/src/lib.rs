//! Channel point redemption records (complex module): one record per Twitch redemption, with its status following
//! Twitch under the rules in docs/modules/redemption.md.

pub mod application;
mod domain;
mod infrastructure;

pub use application::RedemptionService;
pub use infrastructure::{
    REDEMPTION_RECORDED, RedemptionChangedEvent, RedemptionChangedPayload, RedemptionRecorded,
    db::{MIGRATOR, RedeemedEntry, count_between, record_count, redeemed_between},
    http::{RedemptionPage, RedemptionQuery, RedemptionRecord, RedemptionStatusName, routes},
    openapi, start,
};
