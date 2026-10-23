//! Audiobook reader preferences, local bookmarks and per-book repeat, persisted on this computer.

use discophile_core::audiobook_reader::{AudiobookBookmark, AudiobookReaderPreferences, RepeatingAudiobooks};
use discophile_core::l10n::t;
use uuid::Uuid;

use crate::platform::storage;

const PREFERENCES_KEY: &str = "audiobooks/reader";
const BOOKMARKS_KEY: &str = "audiobooks/bookmarks";
const REPEATING_KEY: &str = "audiobooks/repeating";

#[derive(Default)]
pub struct Reader {
    pub preferences: AudiobookReaderPreferences,
    pub bookmarks: Vec<AudiobookBookmark>,
    pub repeating: RepeatingAudiobooks,
    pub error: Option<String>,
}

impl Reader {
    pub fn load() -> Self {
        let preferences = storage::load::<AudiobookReaderPreferences>(PREFERENCES_KEY).map(|p| p.validated()).unwrap_or_default();
        let mut error = None;
        let bookmarks = match storage::read(BOOKMARKS_KEY) {
            Some(text) => AudiobookBookmark::decode(text.as_bytes()).unwrap_or_else(|_| {
                error = Some(t("Your audiobook bookmarks could not be read on this computer."));
                Vec::new()
            }),
            None => Vec::new(),
        };
        let repeating = storage::load(REPEATING_KEY).unwrap_or_default();
        Self { preferences, bookmarks, repeating, error }
    }

    pub fn save_preferences(&mut self) {
        self.preferences = self.preferences.validated();
        storage::save(PREFERENCES_KEY, &self.preferences);
    }

    fn store(&mut self, values: Vec<AudiobookBookmark>) -> bool {
        match AudiobookBookmark::encode(&values) {
            Ok(bytes) => {
                storage::write(BOOKMARKS_KEY, &String::from_utf8_lossy(&bytes));
                self.bookmarks = values;
                self.error = None;
                true
            }
            Err(_) => {
                self.error = Some(t("The bookmark could not be saved. Check available storage and try again."));
                false
            }
        }
    }

    /// A book's bookmarks in timeline order.
    pub fn for_book(&self, account: &str, book: &str) -> Vec<AudiobookBookmark> {
        AudiobookBookmark::for_book(&self.bookmarks, account, book).into_iter().cloned().collect()
    }

    pub fn add(&mut self, account: &str, book: &str, position: f64, title: &str) -> bool {
        match AudiobookBookmark::new(account, book, position, title) {
            Ok(bookmark) => {
                let mut values = self.bookmarks.clone();
                values.push(bookmark);
                self.store(values)
            }
            Err(_) => {
                self.error = Some(t("The bookmark could not be saved. Check available storage and try again."));
                false
            }
        }
    }

    pub fn rename(&mut self, id: Uuid, title: &str) {
        if let Some(values) = AudiobookBookmark::renamed(&self.bookmarks, id, title) {
            self.store(values);
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        let values = AudiobookBookmark::removed(&self.bookmarks, id);
        self.store(values);
    }

    pub fn is_repeating(&self, account: &str, book: &str) -> bool {
        self.repeating.contains(account, book)
    }

    pub fn set_repeating(&mut self, account: &str, book: &str, enabled: bool) {
        self.repeating.set(account, book, enabled);
        storage::save(REPEATING_KEY, &self.repeating);
    }
}
