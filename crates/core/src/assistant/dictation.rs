//! Rolling speech recognition transcripts.

#[derive(Debug, Clone, PartialEq)]
pub struct DictationWord {
    pub text: String,
    /// Seconds from the start of the recording.
    pub timestamp: f64,
}

impl DictationWord {
    pub fn new(text: impl Into<String>, timestamp: f64) -> Self {
        Self {
            text: text.into(),
            timestamp,
        }
    }
}

/// Keeps words that fall out of a recognizer's rolling hypothesis, while allowing corrections to its current window.
#[derive(Debug, Clone, Default)]
pub struct DictationTranscript {
    words: Vec<DictationWord>,
    text: String,
}

impl DictationTranscript {
    const MAX_LENGTH: usize = 1_000;
    const DRIFT: f64 = 0.35;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Merges a new hypothesis (`formatted` text plus its timed words) and returns the full transcript.
    pub fn update(&mut self, formatted: &str, incoming: &[DictationWord]) -> String {
        let formatted = formatted.trim();
        if formatted.is_empty() {
            return self.text.clone();
        }
        let incoming: Vec<DictationWord> = incoming
            .iter()
            .filter(|w| w.timestamp.is_finite() && w.timestamp >= 0.0 && !w.text.is_empty())
            .cloned()
            .collect();
        let mut preserved: Vec<DictationWord> = Vec::new();
        if let Some(first) = incoming.first() {
            let first_lower = first.text.to_lowercase();
            let matched = self.words.iter().position(|w| {
                (w.timestamp - first.timestamp).abs() < Self::DRIFT
                    && w.text.to_lowercase() == first_lower
            });
            preserved = match matched {
                Some(index) => self.words[..index].to_vec(),
                None => self
                    .words
                    .iter()
                    .filter(|w| w.timestamp < first.timestamp - Self::DRIFT)
                    .cloned()
                    .collect(),
            };
            self.words = preserved
                .iter()
                .cloned()
                .chain(incoming.iter().cloned())
                .take(Self::MAX_LENGTH)
                .collect();
        }
        let prefix = preserved
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let joined = [prefix.as_str(), formatted]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        self.text = joined.chars().take(Self::MAX_LENGTH).collect();
        self.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str, start: f64) -> Vec<DictationWord> {
        text.split(' ')
            .enumerate()
            .map(|(i, w)| DictationWord::new(w, start + i as f64))
            .collect()
    }

    #[test]
    fn rolling_recognition_retains_the_beginning_and_accepts_corrections() {
        let mut transcript = DictationTranscript::new();
        transcript.update(
            "aggiungi brani alla playlist",
            &words("aggiungi brani alla playlist", 0.0),
        );
        assert_eq!(
            transcript.update("alla playlist serale", &words("alla playlist serale", 2.0)),
            "aggiungi brani alla playlist serale"
        );
        assert_eq!(
            transcript.update(
                "playlist serale italiana",
                &words("playlist serale italiana", 3.0)
            ),
            "aggiungi brani alla playlist serale italiana"
        );
        assert_eq!(
            transcript.update("", &[]),
            "aggiungi brani alla playlist serale italiana"
        );
        assert_eq!(
            transcript.update(
                "aggiungi brani alla playlist italiana",
                &words("aggiungi brani alla playlist italiana", 0.0)
            ),
            "aggiungi brani alla playlist italiana"
        );
    }

    #[test]
    fn hypothesis_revisions_and_timestamp_drift_do_not_duplicate_words() {
        let mut transcript = DictationTranscript::new();
        transcript.update("una play list", &words("una play list", 0.0));
        assert_eq!(
            transcript.update("una playlist", &words("una playlist", 0.1)),
            "una playlist"
        );
        assert_eq!(
            transcript.update("playlist serale", &words("playlist serale", 1.1)),
            "una playlist serale"
        );
    }

    #[test]
    fn repeated_words_and_maximum_length() {
        let mut transcript = DictationTranscript::new();
        transcript.update("molto molto calma", &words("molto molto calma", 0.0));
        assert_eq!(
            transcript.update("molto calma oggi", &words("molto calma oggi", 1.0)),
            "molto molto calma oggi"
        );
        assert_eq!(
            transcript.update(&"a".repeat(2_000), &[]).chars().count(),
            1_000
        );
    }
}
