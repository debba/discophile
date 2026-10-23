//! Listening positions: saved on this computer first, then uploaded in order. Failed uploads stay
//! pending and retry after a minute (or on reconnecting); a newer server position always wins.

use std::collections::VecDeque;

use discophile_core::audiobookshelf::{Audiobook, AudiobookPlaybackSession, AudiobookProgress, SyncBackoff};
use discophile_core::l10n::t;
use discophile_core::offline_audiobook::AudiobookProgressArchive;
use discophile_core::{MusicResult, now_millis};
use uuid::Uuid;

use crate::app::App;
use crate::platform::storage;
use crate::tasks::update;

const ARCHIVE_KEY: &str = "audiobooks/progress";
/// How often pending positions are retried in the background when the backoff allows it.
const RETRY_INTERVAL_MS: i64 = 60_000;

/// A request to stream a book, run in the sync queue so earlier position uploads finish first.
#[derive(Clone)]
pub struct StartRequest {
    pub account: String,
    pub connection: Uuid,
    pub book: Audiobook,
    pub position: Option<f64>,
    pub generation: u64,
}

enum Job {
    Record {
        account: String,
        connection: Uuid,
        item: String,
        session: String,
        progress: AudiobookProgress,
        listened: f64,
        close: bool,
        offline: bool,
    },
    Retry,
    Prepare(Box<StartRequest>),
}

#[derive(Default)]
pub struct Sync {
    pub archive: AudiobookProgressArchive,
    /// Shown under the player and on the Audiobooks page while positions wait for the server.
    pub error: Option<String>,
    backoff: SyncBackoff,
    queue: VecDeque<Job>,
    busy: bool,
    last_retry_ms: i64,
}

impl Sync {
    pub fn load() -> Self {
        let mut sync = Self::default();
        if let Some(text) = storage::read(ARCHIVE_KEY) {
            match AudiobookProgressArchive::decode(text.as_bytes()) {
                Ok(archive) => sync.archive = archive,
                Err(_) => sync.error = Some(t("Your saved audiobook positions could not be read on this computer.")),
            }
        }
        sync
    }

    fn persist(&mut self) {
        match self.archive.encode() {
            Ok(bytes) => storage::write(ARCHIVE_KEY, &String::from_utf8_lossy(&bytes)),
            Err(_) => self.error = Some(t("The audiobook position could not be saved on this computer. Keep the app open and try again.")),
        }
    }

    pub fn saved(&self, account: &str, item: &str) -> Option<AudiobookProgress> {
        self.archive.saved(account, item).copied()
    }
}

fn unsent_message() -> String {
    t("Your audiobook position is saved on this computer. Server sync will retry when you reconnect.")
}

/// Records a position from the player and queues its upload.
#[allow(clippy::too_many_arguments)]
pub fn record(
    app: &mut App,
    account: &str,
    connection: Uuid,
    item: &str,
    session: &str,
    progress: AudiobookProgress,
    listened: f64,
    close: bool,
    offline: bool,
) {
    let sync = &mut app.audiobooks.sync;
    sync.archive.record(account, item, progress);
    sync.persist();
    sync.queue.push_back(Job::Record {
        account: account.to_string(),
        connection,
        item: item.to_string(),
        session: session.to_string(),
        progress,
        listened,
        close,
        offline,
    });
    pump(app);
}

/// A server position seen while browsing or downloading.
pub fn remember(app: &mut App, account: &str, item: &str, remote: AudiobookProgress) {
    let sync = &mut app.audiobooks.sync;
    sync.archive.remember(account, item, remote);
    sync.persist();
}

/// Uploads pending positions of the connected account. `force` ignores the retry delay.
pub fn retry_pending(app: &mut App, force: bool) {
    let sync = &mut app.audiobooks.sync;
    if force {
        sync.backoff.reset();
    }
    sync.last_retry_ms = now_millis();
    if !sync.queue.iter().any(|j| matches!(j, Job::Retry)) {
        sync.queue.push_back(Job::Retry);
    }
    pump(app);
}

/// Every frame: retries pending positions once a minute.
pub fn tick(app: &mut App) {
    let Some(account) = app.audiobooks.account.as_ref().map(|a| a.id()) else { return };
    let sync = &app.audiobooks.sync;
    let now = now_millis();
    if now - sync.last_retry_ms >= RETRY_INTERVAL_MS && sync.backoff.can_attempt(now) && !sync.archive.pending_items(&account).is_empty() {
        retry_pending(app, false);
    }
}

/// Starts streaming a book after earlier uploads have finished.
pub fn prepare(app: &mut App, request: StartRequest) {
    app.audiobooks.sync.queue.push_back(Job::Prepare(Box::new(request)));
    pump(app);
}

fn finish(app: &mut App) {
    app.audiobooks.sync.busy = false;
    pump(app);
}

fn pump(app: &mut App) {
    loop {
        if app.audiobooks.sync.busy {
            return;
        }
        let Some(job) = app.audiobooks.sync.queue.pop_front() else { return };
        let Some(client) = app.audiobooks.client.clone() else {
            if let Job::Prepare(request) = job {
                super::playback::prepared(app, *request, None, Err(discophile_core::MusicError::Authentication));
            }
            continue;
        };
        let current = client.account();
        let now = now_millis();
        match job {
            Job::Record { account, connection, item, session, progress, listened, close, offline } => {
                if connection != current.connection_id || !app.audiobooks.sync.backoff.can_attempt(now) {
                    continue;
                }
                app.audiobooks.sync.busy = true;
                app.tasks.spawn(async move {
                    let result = client.push_progress(&item, &session, &progress, listened, close, offline).await;
                    update(move |app| {
                        let sync = &mut app.audiobooks.sync;
                        let connected = app.audiobooks.account.as_ref().is_some_and(|a| a.id() == account);
                        match result {
                            Ok(()) => {
                                sync.archive.sent(&account, &item, &progress);
                                sync.persist();
                                if connected {
                                    sync.error = None;
                                }
                            }
                            Err(_) if connected => {
                                sync.backoff.failed(now_millis());
                                sync.error = Some(unsent_message());
                            }
                            Err(_) => {}
                        }
                        finish(app);
                    })
                });
            }
            Job::Retry => {
                let account = current.id();
                let items = app.audiobooks.sync.archive.pending_items(&account);
                if items.is_empty() || !app.audiobooks.sync.backoff.can_attempt(now) {
                    continue;
                }
                app.audiobooks.sync.busy = true;
                app.tasks.spawn(async move {
                    let mut results = Vec::new();
                    let mut failed = false;
                    for (item, sent) in items {
                        match client.reconcile_progress(&item, Some(&sent)).await {
                            Ok(latest) => results.push((item, sent, latest.unwrap_or(sent))),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        }
                    }
                    update(move |app| {
                        let connected = app.audiobooks.account.as_ref().is_some_and(|a| a.id() == account);
                        let sync = &mut app.audiobooks.sync;
                        for (item, sent, latest) in &results {
                            sync.archive.reconciled(&account, item, sent, *latest);
                        }
                        sync.persist();
                        if connected {
                            if failed {
                                sync.backoff.failed(now_millis());
                                sync.error = Some(unsent_message());
                            } else {
                                sync.error = None;
                            }
                        }
                        finish(app);
                    })
                });
            }
            Job::Prepare(request) => {
                let item = request.book.id.clone();
                let pending = app.audiobooks.sync.archive.pending(&request.account, &item).copied();
                app.audiobooks.sync.busy = true;
                app.tasks.spawn(async move {
                    let result: MusicResult<(Option<AudiobookProgress>, AudiobookPlaybackSession)> = async {
                        let latest = client.reconcile_progress(&item, pending.as_ref()).await?;
                        let session = client.play(&item).await?;
                        Ok((latest, session))
                    }
                    .await;
                    update(move |app| {
                        let request = *request;
                        if let Ok((latest, _)) = &result {
                            let sync = &mut app.audiobooks.sync;
                            sync.archive.prepared(&request.account, &request.book.id, pending.as_ref(), *latest);
                            sync.persist();
                            sync.backoff.reset();
                        }
                        app.audiobooks.sync.busy = false;
                        super::playback::prepared(app, request, Some(client), result);
                        pump(app);
                    })
                });
            }
        }
    }
}
