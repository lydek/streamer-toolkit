use super::*;
use crate::infrastructure::db::tests::{chat, count, data_dir, pool, wait_for_count};
use axum::{body::Body, http::Request};
use chrono::DateTime;
use toolkit_core::{events::Source, outbox::Outbox, settings::CheckinSettings};
use tower::ServiceExt;

mod admin_routes;
mod card_overlay;
mod redemption_checkins;

/// Knows no profile images.
pub(crate) struct NoProfiles;
impl ProfileImages for NoProfiles {
    fn profile_image<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async { None })
    }
}

/// A screen that shows nothing.
pub(crate) fn quiet_screen() -> Screen {
    Screen {
        changed: Arc::new(|_| {}),
        show_card: Arc::new(|_, _| {}),
        hide_card: Arc::new(|| {}),
        profiles: Arc::new(NoProfiles),
        timing: Timing::default(),
        presenter: presenter(),
    }
}

/// The pause between items in tests.
const GAP: std::time::Duration = std::time::Duration::from_millis(20);

/// A presentation queue whose overlays are always open.
fn presenter() -> Arc<Presenter> {
    Presenter::start(GAP, |_| true)
}

/// `start` with a bus of its own, no profile images and default timing.
fn start_quiet(
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    dir: PathBuf,
) -> CheckinService {
    start(
        pool,
        bus,
        modules,
        dir.clone(),
        Arc::new(NoProfiles),
        sounds(&dir),
        Timing::default(),
        presenter(),
        crate::application::test_commands(&dir),
    )
}

/// The sound settings in `dir`.
fn sounds(dir: &std::path::Path) -> Arc<SoundService> {
    Arc::new(SoundService::new(
        dir.to_path_buf(),
        Arc::new(Modules::load(dir.to_path_buf())),
        Arc::new(EventBus::default()),
        presenter(),
    ))
}
