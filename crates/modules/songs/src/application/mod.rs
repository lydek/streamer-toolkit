//! The song list (docs/modules/songs.md): songs and categories with their limits, and importing a file through a
//! preview. Storage is a port; the SQLite implementation is in `infrastructure::db`.

use std::future::Future;

use crate::domain::{
    self, CATEGORIES_MAX, Category, ImportPlan, SONGS_MAX, Sheet, Skip, Song, category_name,
    checked_song, normalize, song_key,
};

#[derive(Debug, thiserror::Error)]
#[error("songs.storage_failed: {0}")]
pub struct RepositoryError(pub(crate) String);

#[derive(Debug, thiserror::Error)]
pub(crate) enum SongsError {
    /// Outside the limits (a field, or the number of songs or categories).
    #[error("songs.invalid")]
    Invalid,
    /// Another song is the same song, or another category has the name.
    #[error("songs.duplicate")]
    Duplicate,
    #[error("songs.not_found")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

/// Storage port. Writes that would break a unique key answer `Ok(false)` and change nothing.
pub trait SongRepository: Send + Sync + 'static {
    fn songs(&self) -> impl Future<Output = Result<Vec<Song>, RepositoryError>> + Send;
    fn categories(&self) -> impl Future<Output = Result<Vec<Category>, RepositoryError>> + Send;
    /// Inserts or replaces by ID; `false` when another song has the same key.
    fn save_song(&self, song: &Song) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// `false` when there was none.
    fn delete_song(&self, id: &str) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Inserts or replaces by ID; `false` when another category has the same name.
    fn save_category(
        &self,
        c: &Category,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Its songs become uncategorised; `false` when there was none.
    fn delete_category(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Writes the whole plan in one transaction.
    fn apply_import(
        &self,
        plan: &ImportPlan,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// A song as the admin page sends it.
pub(crate) struct SongInput {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) category_id: Option<String>,
    pub(crate) youtube_url: String,
}

pub struct SongsService<R> {
    pub(crate) repo: R,
    /// One change at a time, so limits and keys are checked against what is stored.
    lock: tokio::sync::Mutex<()>,
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

impl<R: SongRepository> SongsService<R> {
    pub(crate) fn new(repo: R) -> Self {
        Self {
            repo,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) async fn list(&self) -> Result<(Vec<Song>, Vec<Category>), RepositoryError> {
        Ok((self.repo.songs().await?, self.repo.categories().await?))
    }

    /// Adds a song (`id` `None`) or changes one; what is stored.
    pub(crate) async fn save_song(
        &self,
        id: Option<&str>,
        input: SongInput,
    ) -> Result<Song, SongsError> {
        let (title, artist, youtube_url) =
            checked_song(&input.title, &input.artist, &input.youtube_url)
                .map_err(|_| SongsError::Invalid)?;
        let _one = self.lock.lock().await;
        let songs = self.repo.songs().await?;
        let categories = self.repo.categories().await?;
        if let Some(c) = &input.category_id
            && !categories.iter().any(|k| &k.id == c)
        {
            return Err(SongsError::Invalid);
        }
        let song = match id {
            Some(id) => {
                let old = songs
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or(SongsError::NotFound)?;
                Song {
                    title,
                    artist,
                    category_id: input.category_id,
                    youtube_url,
                    ..old.clone()
                }
            }
            None if songs.len() >= SONGS_MAX => return Err(SongsError::Invalid),
            None => Song {
                id: new_id(),
                title,
                artist,
                category_id: input.category_id,
                youtube_url,
                last_sung_at: None,
                request_count: 0,
            },
        };
        let key = song_key(&song.title, &song.artist);
        if songs
            .iter()
            .any(|s| s.id != song.id && song_key(&s.title, &s.artist) == key)
            || !self.repo.save_song(&song).await?
        {
            return Err(SongsError::Duplicate);
        }
        Ok(song)
    }

    pub(crate) async fn delete_song(&self, id: &str) -> Result<(), SongsError> {
        let _one = self.lock.lock().await;
        if self.repo.delete_song(id).await? {
            Ok(())
        } else {
            Err(SongsError::NotFound)
        }
    }

    /// Adds a category (`id` `None`) or renames one and sets whether it is open to requests.
    pub(crate) async fn save_category(
        &self,
        id: Option<&str>,
        name: &str,
        requestable: bool,
    ) -> Result<Category, SongsError> {
        let name = category_name(name).ok_or(SongsError::Invalid)?;
        let _one = self.lock.lock().await;
        let categories = self.repo.categories().await?;
        let category = match id {
            Some(id) if categories.iter().any(|c| c.id == id) => Category {
                id: id.to_owned(),
                name,
                requestable,
            },
            Some(_) => return Err(SongsError::NotFound),
            None if categories.len() >= CATEGORIES_MAX => return Err(SongsError::Invalid),
            None => Category {
                id: new_id(),
                name,
                requestable,
            },
        };
        let key = normalize(&category.name);
        if categories
            .iter()
            .any(|c| c.id != category.id && normalize(&c.name) == key)
            || !self.repo.save_category(&category).await?
        {
            return Err(SongsError::Duplicate);
        }
        Ok(category)
    }

    /// Deletes a category; its songs become uncategorised.
    pub(crate) async fn delete_category(&self, id: &str) -> Result<(), SongsError> {
        let _one = self.lock.lock().await;
        if self.repo.delete_category(id).await? {
            Ok(())
        } else {
            Err(SongsError::NotFound)
        }
    }

    /// What importing `sheets` would do.
    pub(crate) async fn preview_import(
        &self,
        sheets: &[Sheet],
    ) -> Result<ImportPlan, RepositoryError> {
        let (songs, categories) = self.list().await?;
        Ok(domain::plan_import(&songs, &categories, sheets))
    }

    /// Imports `sheets` in one transaction; what it did.
    pub(crate) async fn import(&self, sheets: &[Sheet]) -> Result<ImportPlan, RepositoryError> {
        let _one = self.lock.lock().await;
        let plan = self.preview_import(sheets).await?;
        self.repo.apply_import(&plan).await?;
        Ok(plan)
    }
}

/// How a skip reason is named in the API.
pub(crate) fn skip_code(s: Skip) -> &'static str {
    match s {
        Skip::NoTitleColumn => "no_title_column",
        Skip::NoTitle => "no_title",
        Skip::TooLong => "too_long",
        Skip::BadLink => "bad_link",
        Skip::DuplicateInFile => "duplicate_in_file",
        Skip::TooManySongs => "too_many_songs",
        Skip::TooManyCategories => "too_many_categories",
    }
}
