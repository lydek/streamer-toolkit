//! The stamp card's look and its images: imported into the data folder's `checkin/` under a UUID name, and
//! removed once no saved style uses them.

use std::{fs, io, path::PathBuf};

use toolkit_core::settings::{self, CardStyle};
use uuid::Uuid;

use super::Config;
use crate::domain::style::{self, ImageFormat};

/// The largest card image accepted.
pub(crate) const IMAGE_MAX_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StyleError {
    /// Out of range, or an image the folder does not have.
    #[error("checkin.invalid_style")]
    Invalid,
    /// Not a PNG, JPG, GIF or WebP file, by extension and content.
    #[error("checkin.unsupported_image")]
    UnsupportedImage,
    #[error("checkin.image_too_large")]
    TooLarge,
    #[error("settings.save_failed")]
    SaveFailed(#[from] io::Error),
}

impl Config {
    pub(crate) fn style(&self) -> CardStyle {
        self.style.read().unwrap().clone()
    }

    fn images(&self) -> PathBuf {
        self.data_dir.join("checkin")
    }

    /// Where a stored image is, for a name the toolkit gave that is still in the folder.
    pub(crate) fn image(&self, name: &str) -> Option<(PathBuf, ImageFormat)> {
        let format = ImageFormat::of_file(name)?;
        let path = self.images().join(name);
        path.is_file().then_some((path, format))
    }

    /// Validates, saves, then applies; images no saved style uses any more are removed.
    pub(crate) fn set_style(&self, next: CardStyle) -> Result<CardStyle, StyleError> {
        let next = style::checked(next).ok_or(StyleError::Invalid)?;
        if [&next.background_image, &next.stamp_image]
            .into_iter()
            .flatten()
            .any(|name| self.image(name).is_none())
        {
            return Err(StyleError::Invalid);
        }
        // An animation pack lives in the template directory (the host installs it).
        if let Some(pack) = style::pack_of(&next.preset)
            && !self
                .data_dir
                .join("templates")
                .join("checkin")
                .join(pack)
                .join("index.html")
                .is_file()
        {
            return Err(StyleError::Invalid);
        }
        let mut current = self.style.write().unwrap();
        settings::update(&self.data_dir, |s| s.checkin.card = next.clone())?;
        *current = next.clone();
        drop(current);
        self.remove_unused_images(&next);
        Ok(next)
    }

    /// Copies an image in; returns its stored name. `file_name` only gives the format.
    pub(crate) fn import_image(&self, file_name: &str, bytes: &[u8]) -> Result<String, StyleError> {
        if bytes.len() > IMAGE_MAX_BYTES {
            return Err(StyleError::TooLarge);
        }
        let format = ImageFormat::detect(file_name, bytes).ok_or(StyleError::UnsupportedImage)?;
        let name = format!("{}.{}", Uuid::now_v7(), format.extension());
        fs::create_dir_all(self.images())?;
        fs::write(self.images().join(&name), bytes)?;
        Ok(name)
    }

    /// Removes the images `keep` does not use; one that cannot be removed waits for the next save.
    fn remove_unused_images(&self, keep: &CardStyle) {
        let Ok(entries) = fs::read_dir(self.images()) else {
            return;
        };
        let used = [&keep.background_image, &keep.stamp_image];
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ImageFormat::of_file(&name).is_some()
                && !used.iter().any(|u| u.as_deref() == Some(name.as_str()))
                && let Err(e) = fs::remove_file(entry.path())
            {
                tracing::warn!(error = %e, "unused card image not removed");
            }
        }
    }
}
