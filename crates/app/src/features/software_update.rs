//! Desktop update state, persisted release notes and the install/restart handoff.

use crate::app::App;
use crate::platform::{
    storage,
    updater::{self, PendingUpdate},
};
use crate::tasks::update;
use discophile_core::updates::{self, Release};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed,
    Downloading {
        received: u64,
        total: u64,
    },
    Installing,
    Restarting,
}

impl UpdateStatus {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Downloading { .. } | Self::Installing | Self::Restarting)
    }
}

#[derive(Default)]
pub struct UpdatesState {
    /// Used only by the native developer preview; absent from release builds.
    #[cfg(debug_assertions)]
    pub preview_install: Option<fn(&mut App)>,
    pub status: UpdateStatus,
    pub available: Option<Release>,
    pub notice_dismissed: bool,
    pub notice_anchor: Option<egui::Rect>,
    pub dialog: bool,
    pub error: Option<String>,
    pub changelog: Option<Release>,
    pub markdown: crate::ui::release_notes::Cache,
    pub(crate) active_dialog: Option<egui::Id>,
    pub(crate) return_focus: Option<egui::Id>,
    pub(crate) notes_scroll: f32,
}

pub fn start(app: &mut App) {
    let pending = storage::load::<PendingUpdate>(updater::PENDING_KEY);
    if let Some(pending) = pending {
        if pending.release.version == CURRENT {
            app.software_update.changelog = Some(pending.release);
        } else if updates::is_newer(&pending.release.version, CURRENT) {
            app.software_update.error = Some(
                updater::take_failure()
                    .unwrap_or_else(|| "The previous update did not complete. You can try again; the installed version is unchanged.".into()),
            );
            app.software_update.status = UpdateStatus::Available(pending.release.clone());
            app.software_update.available = Some(pending.release);
            app.software_update.dialog = true;
            return;
        } else {
            storage::remove(updater::PENDING_KEY);
        }
    }
    if app.software_update.changelog.is_none() {
        if let Some(previous) = app.settings.last_seen_version.clone() {
            if updates::is_newer(CURRENT, &previous) {
                app.tasks.spawn(async move {
                    let notes = updates::fetch_notes_between(&previous, CURRENT).await;
                    update(move |app: &mut App| {
                        if let Some(notes) = notes.filter(|notes| !notes.is_empty()) {
                            app.software_update.changelog =
                                Some(Release { version: CURRENT.into(), page_url: format!("{}/releases", crate::platform::GITHUB), notes, assets: vec![] });
                        }
                    })
                });
            }
        } else {
            app.settings.last_seen_version = Some(CURRENT.into());
            app.settings.save();
        }
    }
    if app.settings.check_for_updates {
        check(app, 2_000);
    }
}

pub fn check_now(app: &mut App) {
    app.software_update.notice_dismissed = false;
    check(app, 0);
}

fn check(app: &mut App, delay_ms: u64) {
    if app.software_update.status == UpdateStatus::Checking || app.software_update.status.busy() {
        return;
    }
    app.software_update.status = UpdateStatus::Checking;
    app.tasks.spawn(async move {
        crate::platform::sleep_ms(delay_ms).await;
        let release = updates::fetch_latest().await;
        update(move |app: &mut App| {
            if app.software_update.status.busy() {
                return;
            }
            match release {
                Some(release) if updates::is_newer(&release.version, CURRENT) => {
                    app.software_update.available = Some(release.clone());
                    app.software_update.status = UpdateStatus::Available(release);
                }
                Some(_) => {
                    app.software_update.available = None;
                    app.software_update.status = UpdateStatus::UpToDate;
                }
                None => app.software_update.status = UpdateStatus::Failed,
            }
        })
    });
}

pub fn notice(app: &App) -> Option<&Release> {
    if app.software_update.notice_dismissed || app.software_update.status.busy() {
        return None;
    }
    app.software_update.available.as_ref()
}

pub fn install(app: &mut App) {
    if app.software_update.status.busy() {
        app.software_update.dialog = true;
        return;
    }
    #[cfg(debug_assertions)]
    if let Some(preview) = app.software_update.preview_install {
        preview(app);
        return;
    }
    let Some(mut release) = app.software_update.available.clone() else { return };
    app.software_update.dialog = true;
    app.software_update.error = None;
    app.software_update.notice_dismissed = true;
    app.software_update.status = UpdateStatus::Downloading { received: 0, total: 0 };
    let tasks = app.tasks.clone();
    app.tasks.spawn(async move {
        if let Some(notes) = updates::fetch_notes_between(CURRENT, &release.version).await.filter(|notes| !notes.is_empty()) {
            release.notes = notes;
        }
        let pending = PendingUpdate { previous_version: CURRENT.into(), release: release.clone() };
        let result = updater::prepare(&release, |progress| {
            tasks.send(update(move |app: &mut App| {
                app.software_update.status = match progress {
                    updater::Progress::Download { received, total } => UpdateStatus::Downloading { received, total },
                    updater::Progress::Verifying => UpdateStatus::Installing,
                };
            }));
        })
        .await;
        update(move |app: &mut App| {
            let result = result.and_then(|prepared| {
                updater::save_pending(&pending)?;
                prepared.handoff()
            });
            match result {
                Ok(()) => {
                    app.software_update.status = UpdateStatus::Restarting;
                    app.settings.save();
                    app.request_quit();
                }
                Err(error) => {
                    storage::remove(updater::PENDING_KEY);
                    app.software_update.status = UpdateStatus::Available(release);
                    app.software_update.error = Some(error);
                    app.software_update.dialog = true;
                }
            }
        })
    });
}

pub fn dismiss_changelog(app: &mut App) {
    app.software_update.changelog = None;
    app.settings.last_seen_version = Some(CURRENT.into());
    app.settings.save();
    storage::remove(updater::PENDING_KEY);
}
