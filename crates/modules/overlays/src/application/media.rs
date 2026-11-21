//! Media library use cases: imported files are copied into the data folder's `media/` under a UUID v7 name; their
//! titles and lengths live in the database.

use std::{fs, io, path::PathBuf};

use uuid::Uuid;

use super::{OverlayRepository, OverlayService, RepositoryError};
use crate::domain::media::{DURATION_MAX_MS, GIF_MS, MAX_BYTES, MediaEntry, MediaFormat};

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// Not a GIF, WebM or MP4 file, by extension and content.
    #[error("media.unsupported_type")]
    UnsupportedType,
    #[error("media.too_large")]
    TooLarge,
    #[error("media.not_found")]
    NotFound,
    /// An empty title, or a length out of range.
    #[error("media.invalid")]
    Invalid,
    #[error("media.save_failed: {0}")]
    SaveFailed(String),
}

impl From<RepositoryError> for MediaError {
    fn from(e: RepositoryError) -> Self {
        Self::SaveFailed(e.0)
    }
}

impl From<io::Error> for MediaError {
    fn from(e: io::Error) -> Self {
        Self::SaveFailed(e.to_string())
    }
}

impl<R: OverlayRepository> OverlayService<R> {
    /// In import order.
    pub async fn media(&self) -> Result<Vec<MediaEntry>, RepositoryError> {
        self.repo.media().await
    }

    /// Where an entry's file is, if both exist.
    pub async fn media_file(&self, id: &str) -> Option<(PathBuf, MediaFormat)> {
        let entry = self.repo.media_entry(id).await.ok()??;
        let path = self.media_dir.join(entry.file_name());
        path.is_file().then_some((path, entry.format))
    }

    /// Whether an entry's file is still in the folder.
    pub fn media_missing(&self, entry: &MediaEntry) -> bool {
        !self.media_dir.join(entry.file_name()).is_file()
    }

    /// Copies `bytes` in as a new entry; `file_name` only gives the format and the starting title. A GIF starts at
    /// 5 seconds; a video waits for its length.
    pub async fn import_media(
        &self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<MediaEntry, MediaError> {
        if bytes.len() > MAX_BYTES {
            return Err(MediaError::TooLarge);
        }
        let format = MediaFormat::detect(file_name, bytes).ok_or(MediaError::UnsupportedType)?;
        let entry = MediaEntry {
            id: Uuid::now_v7().to_string(),
            title: toolkit_core::audio::label_from_file_name(file_name, "media"),
            format,
            duration_ms: (format == MediaFormat::Gif).then_some(GIF_MS),
        };
        fs::create_dir_all(&self.media_dir)?;
        let path = self.media_dir.join(entry.file_name());
        fs::write(&path, bytes)?;
        if let Err(e) = self.repo.insert_media(&entry).await {
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        self.reload_library().await?;
        Ok(entry)
    }

    /// Renames an entry and sets how long it shows (1 ms to 10 minutes).
    pub async fn update_media(
        &self,
        id: &str,
        title: &str,
        duration_ms: Option<u32>,
    ) -> Result<MediaEntry, MediaError> {
        let title = toolkit_core::audio::clean_label(title).ok_or(MediaError::Invalid)?;
        if duration_ms.is_some_and(|ms| !(1..=DURATION_MAX_MS).contains(&ms)) {
            return Err(MediaError::Invalid);
        }
        let mut entry = self
            .repo
            .media_entry(id)
            .await?
            .ok_or(MediaError::NotFound)?;
        entry.title = title;
        entry.duration_ms = duration_ms.or(entry.duration_ms);
        self.repo.update_media(&entry).await?;
        self.reload_library().await?;
        Ok(entry)
    }

    /// Removes the entry and the rules that show it, then its file; a file that cannot be removed is left behind,
    /// never the entry.
    pub async fn delete_media(&self, id: &str) -> Result<(), MediaError> {
        let entry = self
            .repo
            .media_entry(id)
            .await?
            .ok_or(MediaError::NotFound)?;
        self.repo.delete_media(id).await?;
        self.reload_library().await?;
        if let Err(e) = fs::remove_file(self.media_dir.join(entry.file_name()))
            && e.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(error = %e, "media file not removed");
        }
        Ok(())
    }
}
