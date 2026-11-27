//! Song requests (complex module, docs/modules/songs.md): the song list with its categories and import; requests
//! and their queue.

pub mod application;
mod domain;
mod infrastructure;

pub use infrastructure::{Songs, db::MIGRATOR, openapi, routes, start};
