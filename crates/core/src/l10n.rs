//! English-first localization. Keys are the English source text; translations live in
//! `Localization/*.json` (`{"English key": "Italian text"}`) and `Localization/plurals.json`.
//!
//! Add new interface strings to the JSON file owned by your area (for example
//! `Localization/library.json`) with an Italian translation. `cargo test -p discophile-core`
//! verifies the files parse and do not define a key twice with different values.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::error::MusicError;

const SOURCES: &[(&str, &str)] = &[
    ("strings.json", include_str!("../../../Localization/strings.json")),
    ("desktop.json", include_str!("../../../Localization/desktop.json")),
    ("core.json", include_str!("../../../Localization/core.json")),
    ("library.json", include_str!("../../../Localization/library.json")),
    ("player.json", include_str!("../../../Localization/player.json")),
    ("settings.json", include_str!("../../../Localization/settings.json")),
    ("assistant.json", include_str!("../../../Localization/assistant.json")),
    ("audiobooks.json", include_str!("../../../Localization/audiobooks.json")),
    ("discovery.json", include_str!("../../../Localization/discovery.json")),
];
const PLURALS: &str = include_str!("../../../Localization/plurals.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[default]
    English,
    Italian,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Italian];
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Italian => "it",
        }
    }
    /// The language's own name, shown in the language picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Italian => "Italiano",
        }
    }
    /// Picks a supported language from a BCP 47 tag such as `it-IT`; English is the fallback.
    pub fn from_tag(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("it") { Self::Italian } else { Self::English }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    LANGUAGE.store(language as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::Italian,
        _ => Language::English,
    }
}

type Plurals = HashMap<String, HashMap<String, HashMap<String, String>>>;

fn italian() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = HashMap::new();
        for (name, source) in SOURCES {
            match serde_json::from_str::<HashMap<String, String>>(source) {
                Ok(entries) => table.extend(entries),
                Err(error) => log::error!("Localization/{name} is invalid: {error}"),
            }
        }
        table
    })
}

fn plurals() -> &'static Plurals {
    static TABLE: OnceLock<Plurals> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(PLURALS).unwrap_or_default())
}

/// The localized text for an English key. Missing translations fall back to English.
pub fn t(key: &str) -> String {
    match language() {
        Language::English => key.to_string(),
        Language::Italian => italian().get(key).cloned().unwrap_or_else(|| key.to_string()),
    }
}

/// Whether an Italian translation exists for `key` (used by verification tests).
pub fn has_translation(key: &str) -> bool {
    italian().contains_key(key)
}

/// Localizes `key` and substitutes Apple-style placeholders: `%@`, `%lld`, `%1$@`, `%2$lld`, `%%`.
pub fn tf(key: &str, args: &[&dyn Display]) -> String {
    format_template(&t(key), args)
}

/// Localizes a count-based key, choosing the plural form when one is defined.
pub fn count(key: &str, n: i64) -> String {
    let lang = language().code();
    if let Some(form) = plurals().get(key).and_then(|l| l.get(lang)) {
        let category = if n == 1 { "one" } else { "other" };
        if let Some(template) = form.get(category).or_else(|| form.get("other")) {
            return format_template(template, &[&n]);
        }
    }
    tf(key, &[&n])
}

pub fn format_template(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.char_indices().peekable();
    let mut next_arg = 0usize;
    while let Some((i, c)) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let rest = &template[i + 1..];
        if rest.starts_with('%') {
            out.push('%');
            chars.next();
            continue;
        }
        // Optional positional index: digits followed by '$'.
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let (position, after) = if !digits.is_empty() && rest[digits.len()..].starts_with('$') {
            (Some(digits.parse::<usize>().unwrap_or(1)), &rest[digits.len() + 1..])
        } else {
            (None, rest)
        };
        let spec_len = if after.starts_with("lld") {
            3
        } else if after.starts_with("ld") {
            2
        } else if after.starts_with('@') || after.starts_with('d') || after.starts_with('s') {
            1
        } else {
            out.push('%');
            continue;
        };
        let index = position.map(|p| p.saturating_sub(1)).unwrap_or_else(|| {
            let current = next_arg;
            next_arg += 1;
            current
        });
        if let Some(arg) = args.get(index) {
            out.push_str(&arg.to_string());
        }
        let consumed = (after.as_ptr() as usize - rest.as_ptr() as usize) + spec_len;
        for _ in 0..consumed {
            chars.next();
        }
    }
    out
}

/// `m:ss` or `h:mm:ss` playback time.
pub fn time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".into();
    }
    let value = seconds as i64;
    if value >= 3600 {
        format!("{}:{:02}:{:02}", value / 3600, (value % 3600) / 60, value % 60)
    } else {
        format!("{}:{:02}", value / 60, value % 60)
    }
}

/// A compact duration such as `2h 58m` or `24m`.
pub fn short_duration(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as i64;
    if minutes >= 60 { format!("{}h {}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
}

/// The localized user-facing message for a music error.
pub fn music_error(error: MusicError) -> String {
    t(match error {
        MusicError::InvalidServer => "Enter a complete server URL, such as https://music.example.com.",
        MusicError::InsecureServer => "Use HTTPS, or explicitly allow HTTP for your trusted local server.",
        MusicError::MissingCredentials => "Enter your username and password.",
        MusicError::Authentication => "Sign-in failed. Check your username, password, and server authentication settings.",
        MusicError::TokenAuthenticationUnsupported => "Your server does not support this authentication method.",
        MusicError::Forbidden => "Your account does not have permission to access this content.",
        MusicError::Unsupported => "This feature is not supported by your server.",
        MusicError::NotFound => "This content is no longer available on the server.",
        MusicError::PlaylistChanged => "This playlist changed on the server. Reload it before trying again.",
        MusicError::InvalidResponse => "The server returned an unexpected response. Check that this is a Subsonic server URL.",
        MusicError::EmptyQueue => "Choose something from your library to start listening.",
        MusicError::UnavailableInPreview => "Connect your server to play or download your own music.",
        MusicError::ServerUnavailable => "Unable to connect. Check your connection and server address, then try again.",
    })
}

/// The localized message for an Audiobookshelf error.
pub fn audiobook_error(error: MusicError) -> String {
    match error {
        MusicError::InvalidResponse => t("Audiobookshelf returned an unexpected response. Check the server address and version."),
        MusicError::Authentication => t("Audiobookshelf sign-in expired or failed. Check your credentials and reconnect."),
        MusicError::Unsupported => t("This audiobook format or server feature is not supported."),
        other => music_error(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_parse_without_conflicts() {
        let mut seen: HashMap<String, (String, &str)> = HashMap::new();
        for (name, source) in SOURCES {
            let entries: HashMap<String, String> =
                serde_json::from_str(source).unwrap_or_else(|e| panic!("Localization/{name}: {e}"));
            for (key, value) in entries {
                assert!(!value.trim().is_empty(), "Localization/{name}: empty translation for {key:?}");
                if let Some((previous, file)) = seen.get(&key) {
                    assert_eq!(previous, &value, "{key:?} differs between {file} and {name}");
                }
                seen.insert(key, (value, name));
            }
        }
        let _: Plurals = serde_json::from_str(PLURALS).expect("Localization/plurals.json");
    }

    #[test]
    fn placeholders() {
        assert_eq!(format_template("Chapter %1$lld of %2$lld", &[&3, &9]), "Chapter 3 of 9");
        assert_eq!(format_template("%2$@ by %1$@", &[&"A", &"B"]), "B by A");
        assert_eq!(format_template("%@ · %lld%%", &[&"x", &5]), "x · 5%");
    }

    #[test]
    fn plurals_and_fallback() {
        set_language(Language::English);
        assert_eq!(count("%lld songs", 1), "1 song");
        assert_eq!(count("%lld songs", 3), "3 songs");
        set_language(Language::Italian);
        assert_eq!(count("%lld songs", 1), "1 brano");
        assert_eq!(t("Library"), "Libreria");
        assert_eq!(t("Untranslated key for test"), "Untranslated key for test");
        set_language(Language::English);
    }

    #[test]
    fn times() {
        assert_eq!(time(65.0), "1:05");
        assert_eq!(time(3725.0), "1:02:05");
        assert_eq!(short_duration(10680.0), "2h 58m");
    }
}
