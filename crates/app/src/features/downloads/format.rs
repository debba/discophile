//! Pure helpers for downloads: byte counts, file extensions and option lists. Shared by desktop and web.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use discophile_core::l10n::{self, Language};

/// Listening time choices for automatic downloads, in minutes.
pub const MINUTE_OPTIONS: [i64; 4] = [30, 60, 120, 240];
/// Storage limit choices for automatic downloads, in MiB.
pub const LIMIT_OPTIONS: [i64; 6] = [128, 256, 512, 1_024, 2_048, 4_096];

/// A file-size string in decimal units, like iOS `ByteCountFormatter` with the `.file` style.
pub fn bytes(value: i64) -> String {
    bytes_in(value, l10n::language())
}

pub fn bytes_in(value: i64, language: Language) -> String {
    let value = value.max(0) as f64;
    let text = if value == 0.0 {
        "0 KB".to_string()
    } else if value < 1_000_000.0 {
        format!("{} KB", (value / 1_000.0).round().max(1.0))
    } else if value < 1_000_000_000.0 {
        format!("{:.1} MB", value / 1_000_000.0)
    } else {
        format!("{:.2} GB", value / 1_000_000_000.0)
    };
    if language == Language::Italian { text.replace('.', ",") } else { text }
}

/// The label of a storage limit choice given in MiB.
pub fn limit_label(megabytes: i64) -> String {
    bytes(megabytes * 1_024 * 1_024)
}

/// The audio file extension for a response's content type, or `fallback` when the type is generic.
pub fn extension_for_content_type(content_type: Option<&str>, fallback: &str) -> String {
    let mime = content_type.unwrap_or_default().split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let extension = match mime.as_str() {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" | "audio/x-mpeg" | "audio/x-mp3" => "mp3",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "audio/x-mp4" => "m4a",
        "audio/aac" | "audio/aacp" | "audio/x-aac" => "aac",
        "audio/ogg" | "application/ogg" | "audio/vorbis" | "audio/x-ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => "wav",
        "audio/aiff" | "audio/x-aiff" => "aiff",
        _ => fallback,
    };
    extension.to_ascii_lowercase()
}

/// Progress as a fraction from 0 to 1; unknown totals report 0.
pub fn fraction(written: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (written as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

/// A whole percentage for accessibility values.
pub fn percent(fraction: f32) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_use_decimal_units_and_localized_separators() {
        assert_eq!(bytes_in(0, Language::English), "0 KB");
        assert_eq!(bytes_in(10, Language::English), "1 KB");
        assert_eq!(bytes_in(532_000, Language::English), "532 KB");
        assert_eq!(bytes_in(12_345_678, Language::English), "12.3 MB");
        assert_eq!(bytes_in(12_345_678, Language::Italian), "12,3 MB");
        assert_eq!(bytes_in(4_294_967_296, Language::English), "4.29 GB");
        assert_eq!(bytes_in(-5, Language::English), "0 KB");
    }

    #[test]
    fn content_types_choose_the_extension() {
        assert_eq!(extension_for_content_type(Some("audio/mpeg"), "flac"), "mp3");
        assert_eq!(extension_for_content_type(Some("Audio/FLAC; charset=binary"), "mp3"), "flac");
        assert_eq!(extension_for_content_type(Some("audio/x-m4a"), "mp3"), "m4a");
        assert_eq!(extension_for_content_type(Some("application/octet-stream"), "opus"), "opus");
        assert_eq!(extension_for_content_type(None, "MP3"), "mp3");
    }

    #[test]
    fn progress_is_bounded() {
        assert_eq!(fraction(5, 0), 0.0);
        assert_eq!(fraction(50, 100), 0.5);
        assert_eq!(fraction(150, 100), 1.0);
        assert_eq!(percent(0.426), "43%");
    }
}
