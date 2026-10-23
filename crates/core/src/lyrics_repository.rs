//! Lyrics from the music server, falling back to LRCLIB, with a bounded cache for offline use.
//!
//! Successful lyrics stay fresh for seven days and misses for one day, so newly added lyrics are
//! discovered without repeating requests on every opening. Expired lyrics are still shown when a
//! refresh fails. The app crate persists [`LyricsCache`] (see [`LyricsRepository::snapshot`]).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::MusicResult;
use crate::l10n::t;
use crate::lrclib::{LrcLibClient, LyricsError};
use crate::lyrics::SongLyrics;
use crate::md5_hex;
use crate::models::Song;
use crate::now_millis;

/// Songs kept in the cache; the least recently saved are evicted first.
pub const CACHE_LIMIT: usize = 512;
pub const FOUND_LIFETIME_MILLIS: i64 = 7 * 86_400_000;
pub const MISSING_LIFETIME_MILLIS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricsSource {
    Server,
    Lrclib,
}

impl LyricsSource {
    /// Attribution shown under lyrics from an online provider.
    pub fn attribution(self) -> Option<String> {
        (self == Self::Lrclib).then(|| t("Lyrics from LRCLIB"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricsResult {
    pub lyrics: SongLyrics,
    pub source: LyricsSource,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsCacheEntry {
    pub server: Option<SongLyrics>,
    /// Unix milliseconds of the last server check.
    pub server_checked: Option<i64>,
    pub online: Option<SongLyrics>,
    pub online_checked: Option<i64>,
    /// Unix milliseconds of the last change, for eviction.
    pub saved: i64,
}

/// Whether a check is recent enough: seven days for found lyrics, one day for misses.
pub fn is_fresh(checked: Option<i64>, lyrics: Option<&SongLyrics>, now: i64) -> bool {
    let lifetime = if lyrics.is_none() { MISSING_LIFETIME_MILLIS } else { FOUND_LIFETIME_MILLIS };
    checked.is_some_and(|checked| now - checked < lifetime)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricsCache {
    #[serde(default)]
    entries: HashMap<String, LyricsCacheEntry>,
}

impl LyricsCache {
    /// The cache key covers the account and every field LRCLIB matches on, so edited metadata is looked up again.
    pub fn key(song: &Song, account: &str) -> String {
        let identity = [
            account.to_string(),
            song.id.clone(),
            song.title.clone(),
            song.artist.clone().unwrap_or_default(),
            song.album.clone().unwrap_or_default(),
            song.duration.map(|d| format!("{d:?}")).unwrap_or_default(),
        ];
        let json = serde_json::to_string(&identity).unwrap_or_default();
        md5_hex(format!("{json}lyrics-v1").as_bytes())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, key: &str) -> Option<&LyricsCacheEntry> {
        self.entries.get(key)
    }

    pub fn entry_mut(&mut self, key: &str) -> Option<&mut LyricsCacheEntry> {
        self.entries.get_mut(key)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Fresh synced lyrics that need no request.
    pub fn fresh_synced(&self, key: &str, online_enabled: bool, now: i64) -> Option<LyricsResult> {
        let entry = self.entries.get(key)?;
        if let Some(lyrics) = entry.server.as_ref().filter(|l| l.is_synced())
            && is_fresh(entry.server_checked, Some(lyrics), now)
        {
            return Some(LyricsResult { lyrics: lyrics.clone(), source: LyricsSource::Server });
        }
        if online_enabled
            && let Some(lyrics) = entry.online.as_ref().filter(|l| l.is_synced())
            && is_fresh(entry.online_checked, Some(lyrics), now)
        {
            return Some(LyricsResult { lyrics: lyrics.clone(), source: LyricsSource::Lrclib });
        }
        None
    }

    pub fn needs_server(&self, key: &str, refresh: bool, now: i64) -> bool {
        refresh || self.entries.get(key).is_none_or(|e| !is_fresh(e.server_checked, e.server.as_ref(), now))
    }

    pub fn needs_online(&self, key: &str, refresh: bool, now: i64) -> bool {
        refresh || self.entries.get(key).is_none_or(|e| !is_fresh(e.online_checked, e.online.as_ref(), now))
    }

    pub fn store_server(&mut self, key: &str, lyrics: Option<SongLyrics>, now: i64) {
        let entry = self.entries.entry(key.to_string()).or_default();
        entry.server = lyrics;
        entry.server_checked = Some(now);
        entry.saved = now;
        self.evict();
    }

    pub fn store_online(&mut self, key: &str, lyrics: Option<SongLyrics>, now: i64) {
        let entry = self.entries.entry(key.to_string()).or_default();
        entry.online = lyrics;
        entry.online_checked = Some(now);
        entry.saved = now;
        self.evict();
    }

    /// Synced server lyrics, then synced online lyrics, then any server text, then any online text.
    /// Online lyrics are never returned while online lyrics are disabled.
    pub fn best(&self, key: &str, online_enabled: bool) -> Option<LyricsResult> {
        let entry = self.entries.get(key)?;
        let server = entry.server.as_ref().map(|l| LyricsResult { lyrics: l.clone(), source: LyricsSource::Server });
        let online = entry
            .online
            .as_ref()
            .filter(|_| online_enabled)
            .map(|l| LyricsResult { lyrics: l.clone(), source: LyricsSource::Lrclib });
        let synced = |r: &Option<LyricsResult>| r.as_ref().is_some_and(|r| r.lyrics.is_synced());
        if synced(&server) {
            server
        } else if synced(&online) {
            online
        } else {
            server.or(online)
        }
    }

    fn evict(&mut self) {
        if self.entries.len() <= CACHE_LIMIT {
            return;
        }
        let mut by_age: Vec<(i64, String)> = self.entries.iter().map(|(k, e)| (e.saved, k.clone())).collect();
        by_age.sort();
        for (_, key) in by_age.into_iter().take(self.entries.len() - CACHE_LIMIT) {
            self.entries.remove(&key);
        }
    }
}

pub struct LyricsRepository {
    cache: Mutex<LyricsCache>,
    online: LrcLibClient,
    revision: AtomicU64,
}

impl LyricsRepository {
    pub fn new(cache: LyricsCache, online: LrcLibClient) -> Self {
        Self { cache: Mutex::new(cache), online, revision: AtomicU64::new(0) }
    }

    /// A copy of the cache for saving.
    pub fn snapshot(&self) -> LyricsCache {
        self.cache.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Increases whenever the cache changes, so the app knows when to save it.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.with_cache(LyricsCache::clear);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn with_cache<R>(&self, f: impl FnOnce(&mut LyricsCache) -> R) -> R {
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut cache)
    }

    /// Lyrics for a song. `server` loads lyrics from the music server and runs only when needed;
    /// LRCLIB is contacted only when `online_enabled` and the server has no synced lyrics.
    pub async fn lyrics<S>(
        &self,
        song: &Song,
        account: &str,
        online_enabled: bool,
        refresh: bool,
        server: S,
    ) -> Result<Option<LyricsResult>, LyricsError>
    where
        S: Future<Output = MusicResult<Option<SongLyrics>>>,
    {
        self.lyrics_with(song, account, online_enabled, refresh, server, self.online.lyrics(song)).await
    }

    /// [`lyrics`](Self::lyrics) with any online provider.
    pub async fn lyrics_with<S, O>(
        &self,
        song: &Song,
        account: &str,
        online_enabled: bool,
        refresh: bool,
        server: S,
        online: O,
    ) -> Result<Option<LyricsResult>, LyricsError>
    where
        S: Future<Output = MusicResult<Option<SongLyrics>>>,
        O: Future<Output = Result<Option<SongLyrics>, LyricsError>>,
    {
        if song.is_podcast() {
            return Ok(None);
        }
        let key = LyricsCache::key(song, account);
        if !refresh && let Some(result) = self.with_cache(|c| c.fresh_synced(&key, online_enabled, now_millis())) {
            return Ok(Some(result));
        }
        let mut failure = None;
        if self.with_cache(|c| c.needs_server(&key, refresh, now_millis())) {
            match server.await {
                Ok(lyrics) => {
                    self.with_cache(|c| c.store_server(&key, lyrics, now_millis()));
                    self.revision.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => failure = Some(LyricsError::Music(error)),
            }
        }
        if let Some(result) = self.with_cache(|c| c.best(&key, false)).filter(|r| r.lyrics.is_synced()) {
            return Ok(Some(result));
        }
        if online_enabled && self.with_cache(|c| c.needs_online(&key, refresh, now_millis())) {
            match online.await {
                Ok(lyrics) => {
                    self.with_cache(|c| c.store_online(&key, lyrics, now_millis()));
                    self.revision.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => failure = Some(error),
            }
        }
        match self.with_cache(|c| c.best(&key, online_enabled)) {
            Some(result) => Ok(Some(result)),
            None => failure.map_or(Ok(None), Err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MusicError;
    use crate::lrclib::tests::{RECORD, plain, song, synced};
    use crate::lrclib::{interpret_reply, lookup};
    use futures::executor::block_on;
    use std::cell::Cell;

    fn repository() -> LyricsRepository {
        LyricsRepository::new(LyricsCache::default(), LrcLibClient::new())
    }

    /// A server stub that counts calls.
    struct Server {
        calls: Cell<usize>,
        result: MusicResult<Option<SongLyrics>>,
    }
    impl Server {
        fn new(result: MusicResult<Option<SongLyrics>>) -> Self {
            Self { calls: Cell::new(0), result }
        }
        async fn fetch(&self) -> MusicResult<Option<SongLyrics>> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    /// An online provider stub over LRCLIB fixtures that counts requests.
    struct Online {
        requests: Cell<usize>,
        status: u16,
        body: String,
    }
    impl Online {
        fn new(status: u16, body: &str) -> Self {
            Self { requests: Cell::new(0), status, body: body.into() }
        }
        async fn fetch(&self, song: &Song) -> Result<Option<SongLyrics>, LyricsError> {
            lookup(song, |_| {
                self.requests.set(self.requests.get() + 1);
                let (_, reply) = interpret_reply(self.status, None, self.body.clone().into_bytes(), 0);
                async move { reply }
            })
            .await
        }
        fn disabled() -> Self {
            Self::new(500, "")
        }
    }

    fn get(
        repo: &LyricsRepository,
        song: &Song,
        account: &str,
        enabled: bool,
        refresh: bool,
        server: &Server,
        online: &Online,
    ) -> Result<Option<LyricsResult>, LyricsError> {
        block_on(repo.lyrics_with(song, account, enabled, refresh, server.fetch(), online.fetch(song)))
    }

    #[test]
    fn disabled_online_lyrics_never_contact_provider() {
        let repo = repository();
        let online = Online::disabled();
        let result =
            get(&repo, &song(), "account-a", false, false, &Server::new(Ok(Some(plain()))), &online).unwrap().unwrap();
        assert_eq!((result.lyrics, result.source), (plain(), LyricsSource::Server));
        assert_eq!(online.requests.get(), 0);
    }

    #[test]
    fn server_synced_lyrics_take_priority_without_provider_request() {
        let repo = repository();
        let online = Online::disabled();
        let result =
            get(&repo, &song(), "account-a", true, false, &Server::new(Ok(Some(synced()))), &online).unwrap().unwrap();
        assert_eq!((result.lyrics, result.source), (synced(), LyricsSource::Server));
        assert_eq!(online.requests.get(), 0);
    }

    #[test]
    fn external_lyrics_replace_plain_server_text_and_persist_but_are_hidden_when_disabled() {
        let repo = repository();
        let online = Online::new(200, RECORD);
        let result = get(&repo, &song(), "account-a", true, false, &Server::new(Ok(Some(plain()))), &online).unwrap();
        assert_eq!(result, Some(LyricsResult { lyrics: synced(), source: LyricsSource::Lrclib }));
        let saved: LyricsCache = serde_json::from_str(&serde_json::to_string(&repo.snapshot()).unwrap()).unwrap();
        let restored = LyricsRepository::new(saved, LrcLibClient::new());
        let offline = Server::new(Err(MusicError::ServerUnavailable));
        assert_eq!(get(&restored, &song(), "account-a", true, false, &offline, &online).unwrap(), result);
        let disabled = get(&restored, &song(), "account-a", false, false, &offline, &online).unwrap().unwrap();
        assert_eq!((disabled.lyrics, disabled.source), (plain(), LyricsSource::Server));
        assert_eq!(offline.calls.get(), 0);
        assert_eq!(online.requests.get(), 1);
    }

    #[test]
    fn cached_external_lyrics_alone_are_unavailable_when_disabled() {
        let repo = repository();
        let online = Online::new(200, RECORD);
        let server = Server::new(Ok(None));
        get(&repo, &song(), "account-a", true, false, &server, &online).unwrap();
        assert_eq!(get(&repo, &song(), "account-a", false, false, &server, &online).unwrap(), None);
    }

    #[test]
    fn missing_lyrics_are_cached_but_explicit_retry_fetches_again() {
        let repo = repository();
        let online = Online::new(404, "");
        let server = Server::new(Ok(None));
        for _ in 0..2 {
            assert_eq!(get(&repo, &song(), "account-a", true, false, &server, &online).unwrap(), None);
        }
        // The exact request and the search, once.
        assert_eq!(online.requests.get(), 2);
        assert_eq!(get(&repo, &song(), "account-a", true, true, &server, &online).unwrap(), None);
        assert_eq!(online.requests.get(), 4);
        assert_eq!(server.calls.get(), 2);
    }

    #[test]
    fn cache_is_separated_by_account_and_changed_track_metadata() {
        let repo = repository();
        let online = Online::disabled();
        get(&repo, &song(), "account-a", false, false, &Server::new(Ok(Some(synced()))), &online).unwrap();
        let other_server = Server::new(Ok(Some(plain())));
        let other = get(&repo, &song(), "account-b", false, false, &other_server, &online).unwrap().unwrap();
        assert_eq!(other.lyrics, plain());
        let edited = Song { duration: Some(230.0), ..song() };
        let changed = get(&repo, &edited, "account-a", false, false, &other_server, &online).unwrap().unwrap();
        assert_eq!(changed.lyrics, plain());
        assert_eq!(other_server.calls.get(), 2);
        assert_eq!(repo.snapshot().len(), 3);
    }

    #[test]
    fn server_failure_can_use_online_lyrics_and_online_failure_keeps_server_text() {
        let repo = repository();
        let unavailable = Server::new(Err(MusicError::ServerUnavailable));
        let online =
            get(&repo, &song(), "account-a", true, false, &unavailable, &Online::new(200, RECORD)).unwrap().unwrap();
        assert_eq!(online.source, LyricsSource::Lrclib);
        let fallback =
            get(&repo, &song(), "account-b", true, false, &Server::new(Ok(Some(plain()))), &Online::new(503, ""))
                .unwrap()
                .unwrap();
        assert_eq!((fallback.lyrics, fallback.source), (plain(), LyricsSource::Server));
        // Without any text, the failure is reported.
        let error = get(&repo, &song(), "account-c", true, false, &unavailable, &Online::new(503, ""));
        assert_eq!(error, Err(LyricsError::Music(MusicError::ServerUnavailable)));
    }

    #[test]
    fn expired_positive_cache_still_works_offline_and_expired_misses_are_retried() {
        let repo = repository();
        get(&repo, &song(), "account-a", true, false, &Server::new(Ok(None)), &Online::new(200, RECORD)).unwrap();
        let mut cache = repo.snapshot();
        let key = LyricsCache::key(&song(), "account-a");
        let old = now_millis() - 8 * 86_400_000;
        let entry = cache.entry_mut(&key).unwrap();
        entry.server_checked = Some(old);
        entry.online_checked = Some(old);
        let offline_repo = LyricsRepository::new(cache, LrcLibClient::new());
        let unavailable = Server::new(Err(MusicError::ServerUnavailable));
        let stale = get(&offline_repo, &song(), "account-a", true, false, &unavailable, &Online::new(503, ""))
            .unwrap()
            .unwrap();
        assert_eq!(stale.lyrics, synced());
        assert_eq!(unavailable.calls.get(), 1);
        // The missing server lyrics were expired too, so a newly available text is fetched.
        let new_server = Server::new(Ok(Some(synced())));
        let refreshed =
            get(&offline_repo, &song(), "account-a", false, false, &new_server, &Online::disabled()).unwrap().unwrap();
        assert_eq!(refreshed.source, LyricsSource::Server);
        assert_eq!(new_server.calls.get(), 1);
    }

    #[test]
    fn dropped_lookup_does_not_write_cache_and_cache_is_bounded() {
        let repo = repository();
        let track = song();
        let pending =
            repo.lyrics_with(&track, "account-a", true, false, futures::future::pending(), async { Ok(None) });
        assert!(futures::FutureExt::now_or_never(pending).is_none());
        assert!(repo.snapshot().is_empty());
        assert_eq!(repo.revision(), 0);
        let mut cache = LyricsCache::default();
        for i in 0..(CACHE_LIMIT as i64 + 3) {
            cache.store_server(&i.to_string(), None, i);
        }
        assert_eq!(cache.len(), CACHE_LIMIT);
        assert!(cache.entry("0").is_none() && cache.entry("3").is_some());
    }

    #[test]
    fn repository_futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let repo = repository();
        let track = song();
        assert_send(repo.lyrics(&track, "a", true, false, async { Ok(None) }));
    }

    #[test]
    fn podcasts_have_no_lyrics_and_attribution() {
        let podcast = Song { is_podcast: Some(true), ..song() };
        let server = Server::new(Ok(Some(plain())));
        assert_eq!(get(&repository(), &podcast, "a", true, false, &server, &Online::disabled()).unwrap(), None);
        assert_eq!(server.calls.get(), 0);
        assert!(LyricsSource::Lrclib.attribution().is_some());
        assert!(LyricsSource::Server.attribution().is_none());
    }
}
