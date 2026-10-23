//! Song lyrics: structured OpenSubsonic lyrics and LRC/plain text from legacy servers.

use serde::{Deserialize, Serialize};

/// One `structuredLyrics` entry from OpenSubsonic's `getLyricsBySongId`. Times are milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredLyrics {
    pub synced: bool,
    #[serde(default)]
    pub offset: Option<f64>,
    pub line: Vec<StructuredLyricsLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredLyricsLine {
    #[serde(default)]
    pub start: Option<f64>,
    pub value: String,
}

impl StructuredLyrics {
    pub fn lyrics(&self) -> SongLyrics {
        let timed = self.synced && self.line.iter().all(|l| l.start.is_some_and(f64::is_finite));
        let shift = self.offset.unwrap_or(0.0) / 1000.0;
        SongLyrics::new(
            self.line
                .iter()
                .map(|l| LyricLine {
                    text: l.value.clone(),
                    start: if timed { l.start.map(|s| s / 1000.0 - shift) } else { None },
                })
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub text: String,
    #[serde(default)]
    pub start: Option<f64>,
}

impl LyricLine {
    pub fn new(text: impl Into<String>, start: Option<f64>) -> Self {
        Self { text: text.into(), start }
    }
}

/// Playback times are in seconds, with server or LRC offsets already applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongLyrics {
    pub lines: Vec<LyricLine>,
}

impl SongLyrics {
    /// Fully timed lines are sorted by start (stable); otherwise every timestamp is dropped.
    pub fn new(lines: Vec<LyricLine>) -> Self {
        let synced = !lines.is_empty() && lines.iter().all(|l| l.start.is_some_and(f64::is_finite));
        let lines = if synced {
            let mut lines = lines;
            lines.sort_by(|a, b| a.start.unwrap_or(0.0).total_cmp(&b.start.unwrap_or(0.0)));
            lines
        } else {
            lines.into_iter().map(|l| LyricLine { text: l.text, start: None }).collect()
        };
        Self { lines }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.text.trim().is_empty())
    }

    pub fn is_synced(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.start.is_some_and(f64::is_finite))
    }

    pub fn active_line(&self, time: f64) -> Option<usize> {
        if !self.is_synced() || !time.is_finite() {
            return None;
        }
        self.lines.iter().rposition(|l| l.start.is_some_and(|s| s <= time))
    }

    /// Legacy servers may return either plain text or standard LRC timestamps.
    pub fn from_text(text: &str) -> Self {
        let mut timed: Vec<LyricLine> = Vec::new();
        let mut plain: Vec<LyricLine> = Vec::new();
        let mut offset = 0.0;
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for raw in normalized.split('\n') {
            let row = raw.trim_matches(|c: char| c.is_whitespace() && c != '\n');
            if let Some(value) = offset_tag(row) {
                offset = value / 1000.0;
                continue;
            }
            if is_metadata(row) {
                continue;
            }
            let stamps = timestamps(row);
            let Some(&(_, end)) = stamps.last() else {
                plain.push(LyricLine::new(row, None));
                continue;
            };
            let value = row[end..].trim();
            for (start, _) in stamps {
                if start.is_finite() {
                    timed.push(LyricLine::new(value, Some(start)));
                }
            }
        }
        if timed.is_empty() {
            Self::new(plain)
        } else {
            Self::new(
                timed.into_iter().map(|l| LyricLine { text: l.text, start: l.start.map(|s| s - offset) }).collect(),
            )
        }
    }
}

/// `[offset:+200]` in milliseconds.
fn offset_tag(row: &str) -> Option<f64> {
    let inner = row.strip_prefix('[')?.strip_suffix(']')?;
    let (key, value) = inner.split_once(':')?;
    if !key.eq_ignore_ascii_case("offset") {
        return None;
    }
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.trim_start_matches('+').parse().ok().or(Some(0.0))
}

fn is_metadata(row: &str) -> bool {
    const KEYS: [&str; 8] = ["ar", "al", "ti", "au", "by", "re", "ve", "length"];
    let Some(inner) = row.strip_prefix('[').filter(|_| row.ends_with(']')) else { return false };
    inner.split_once(':').is_some_and(|(key, _)| KEYS.iter().any(|k| k.eq_ignore_ascii_case(key)))
}

/// Every `[mm:ss]`, `[mm:ss.x]` or `[mm:ss:xxx]` timestamp in the row: (seconds, byte end).
fn timestamps(row: &str) -> Vec<(f64, usize)> {
    let bytes = row.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match timestamp_at(bytes, i) {
            Some((seconds, end)) => {
                result.push((seconds, end));
                i = end;
            }
            None => i += 1,
        }
    }
    result
}

fn timestamp_at(b: &[u8], start: usize) -> Option<(f64, usize)> {
    let digits = |from: usize, max: usize| b[from..].iter().take(max).take_while(|c| c.is_ascii_digit()).count();
    if b.get(start) != Some(&b'[') {
        return None;
    }
    let mut i = start + 1;
    let minutes_len = digits(i, usize::MAX);
    if minutes_len == 0 || b.get(i + minutes_len) != Some(&b':') {
        return None;
    }
    let minutes = &b[i..i + minutes_len];
    i += minutes_len + 1;
    if !(matches!(b.get(i), Some(b'0'..=b'5')) && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
        return None;
    }
    let seconds = &b[i..i + 2];
    i += 2;
    let mut fraction = 0.0;
    if matches!(b.get(i), Some(b'.' | b':')) {
        let len = digits(i + 1, 3);
        if len > 0 && b.get(i + 1 + len) == Some(&b']') {
            let text = std::str::from_utf8(&b[i + 1..i + 1 + len]).ok()?;
            fraction = format!("0.{text}").parse().unwrap_or(0.0);
            i += 1 + len;
        }
    }
    if b.get(i) != Some(&b']') {
        return None;
    }
    let number = |s: &[u8]| std::str::from_utf8(s).ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    Some((number(minutes) * 60.0 + number(seconds) + fraction, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structured(json: &str) -> SongLyrics {
        serde_json::from_str::<StructuredLyrics>(json).unwrap().lyrics()
    }

    fn texts(lyrics: &SongLyrics) -> Vec<&str> {
        lyrics.lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn structured_timing_converts_milliseconds_and_applies_offset_in_both_directions() {
        for (offset, expected) in [(500, 1.5), (-500, 2.5)] {
            let lyrics =
                structured(&format!(r#"{{"synced":true,"offset":{offset},"line":[{{"start":2000,"value":"Synthetic line"}}]}}"#));
            assert!(lyrics.is_synced());
            assert_eq!(lyrics.lines[0].start, Some(expected));
            assert_eq!(lyrics.active_line(expected - 0.01), None);
            assert_eq!(lyrics.active_line(expected), Some(0));
        }
    }

    #[test]
    fn active_line_follows_playback_and_backward_seeks() {
        let lyrics = SongLyrics::new(vec![
            LyricLine::new("Third", Some(8.0)),
            LyricLine::new("First", Some(2.0)),
            LyricLine::new("Second", Some(5.0)),
        ]);
        assert_eq!(lyrics.active_line(0.0), None);
        assert_eq!(lyrics.active_line(2.0), Some(0));
        assert_eq!(lyrics.active_line(5.0), Some(1));
        assert_eq!(lyrics.active_line(90.0), Some(2));
        assert_eq!(lyrics.active_line(3.0), Some(0));
        assert_eq!(lyrics.active_line(f64::NAN), None);
    }

    #[test]
    fn instrumental_marker_ends_previous_line_without_discarding_repeated_text() {
        let lyrics = SongLyrics::from_text("[00:01]Synthetic refrain\n[00:05]\n[00:08]Synthetic refrain");
        assert_eq!(lyrics.lines.len(), 3);
        assert_eq!(lyrics.active_line(6.0), Some(1));
        assert_eq!(lyrics.lines[1].text, "");
        assert_eq!(lyrics.active_line(8.0), Some(2));
    }

    #[test]
    fn lrc_repeated_timestamps_fractions_metadata_and_late_offset() {
        let lyrics =
            SongLyrics::from_text("[ar:Fixture]\n[ti:Sample]\n[00:03.125][00:10.50]Repeated line\n[00:01.2]Opening line\n[offset:+200]");
        assert!(lyrics.is_synced());
        assert_eq!(texts(&lyrics), ["Opening line", "Repeated line", "Repeated line"]);
        for (line, expected) in lyrics.lines.iter().zip([1.0, 2.925, 10.3]) {
            assert!((line.start.unwrap() - expected).abs() < 0.001, "{line:?}");
        }
    }

    #[test]
    fn plain_lyrics_keep_paragraphs_and_do_not_invent_timestamps() {
        let lyrics = SongLyrics::from_text("First line\r\n\r\nSecond line");
        assert_eq!(texts(&lyrics), ["First line", "", "Second line"]);
        assert!(!lyrics.is_synced());
        assert_eq!(lyrics.active_line(20.0), None);
        assert!(SongLyrics::from_text(" \n\t").is_empty());
    }

    #[test]
    fn missing_timestamps_disable_synchronization_for_whole_text() {
        let lyrics = structured(r#"{"synced":true,"line":[{"start":1000,"value":"First"},{"value":"Untimed"}]}"#);
        assert!(!lyrics.is_synced());
        assert_eq!(texts(&lyrics), ["First", "Untimed"]);
        assert_eq!(lyrics.lines[0].start, None);
    }

    #[test]
    fn unsynced_structured_lyrics_ignore_accidental_timestamps() {
        let lyrics = structured(r#"{"synced":false,"line":[{"start":3000,"value":"First"},{"start":1000,"value":"Second"}]}"#);
        assert!(!lyrics.is_synced());
        assert_eq!(texts(&lyrics), ["First", "Second"]);
    }
}
