//! Update checks against the latest GitHub release (desktop). One request at launch when enabled, and
//! on request from Settings > About. Downloading opens the matching release file in the browser.

use discophile_core::updates::{self, Release};

use crate::app::App;
use crate::tasks::update;

/// Leaves launch to the libraries and artwork first.
const LAUNCH_DELAY_MS: u64 = 8_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed,
}

#[derive(Default)]
pub struct UpdatesState {
    pub status: UpdateStatus,
}

pub fn start(app: &mut App) {
    if app.settings.check_for_updates {
        check(app, LAUNCH_DELAY_MS);
    }
}

pub fn check_now(app: &mut App) {
    check(app, 0);
}

fn check(app: &mut App, delay_ms: u64) {
    if app.software_update.status == UpdateStatus::Checking {
        return;
    }
    app.software_update.status = UpdateStatus::Checking;
    app.tasks.spawn(async move {
        crate::platform::sleep_ms(delay_ms).await;
        let release = updates::fetch_latest().await;
        update(move |app: &mut App| {
            app.software_update.status = match release {
                Some(release) if updates::is_newer(&release.version, env!("CARGO_PKG_VERSION")) => UpdateStatus::Available(release),
                Some(_) => UpdateStatus::UpToDate,
                None => UpdateStatus::Failed,
            };
        })
    });
}

/// The newer release, unless its notice was dismissed.
pub fn notice(app: &App) -> Option<&Release> {
    match &app.software_update.status {
        UpdateStatus::Available(release) if app.settings.dismissed_update.as_deref() != Some(release.version.as_str()) => Some(release),
        _ => None,
    }
}

/// The file for this computer, or the release page when the release has none.
pub fn download_url(release: &Release) -> &str {
    release
        .asset_for(std::env::consts::OS, std::env::consts::ARCH)
        .map(|asset| asset.url.as_str())
        .unwrap_or(release.page_url.as_str())
}

pub fn download(app: &mut App, release: &Release) {
    app.ctx().open_url(egui::OpenUrl::new_tab(download_url(release)));
}
