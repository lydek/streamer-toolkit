//! What a music file may be (thin module: simple checks, no aggregates).

pub(crate) use toolkit_core::audio::{Format, clean_label};

/// Largest music file accepted.
pub const MAX_BYTES: usize = 50 * 1024 * 1024;

/// The longest length the admin window may report: 24 hours, more than any file within the import limit can hold
/// even at a low bitrate, so every importable track can have its length.
pub(crate) const DURATION_MAX_MS: u32 = 24 * 60 * 60 * 1000;
