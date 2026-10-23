//! Library names mentioned in assistant text, so replies can link to the artists, albums and songs the
//! library has. Names are compared word by word, ignoring case and punctuation, and the longest match
//! wins. To avoid linking ordinary words, a match must start with a capital letter or a digit (unless the
//! name itself is lowercase), names shorter than four letters must match exactly, and one-word song
//! titles must be in quotes.

use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MentionKind {
    Artist,
    Album,
    Song,
}

/// A name found in a text: its byte range and what it refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub range: Range<usize>,
    pub kind: MentionKind,
    /// The library id of the artist, album or song.
    pub id: String,
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    kind: MentionKind,
    id: String,
}

/// The names of a library, ready to be found in texts.
#[derive(Clone, Debug, Default)]
pub struct MentionIndex {
    entries: HashMap<String, Entry>,
    max_words: usize,
}

/// Longest name considered, in words.
const MAX_WORDS: usize = 8;

impl MentionIndex {
    /// Adds a name. The first entry for a name is kept, so add artists, then albums, then songs.
    pub fn add(&mut self, name: &str, kind: MentionKind, id: &str) {
        let name = name.trim();
        let words: Vec<String> = tokens(name).into_iter().map(|(_, word)| word).collect();
        if id.is_empty() || words.is_empty() || words.len() > MAX_WORDS || name.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
            return;
        }
        self.max_words = self.max_words.max(words.len());
        self.entries.entry(words.join(" ")).or_insert_with(|| Entry { name: name.to_string(), kind, id: id.to_string() });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The names in `text`, in order and without overlaps.
    pub fn find(&self, text: &str) -> Vec<Mention> {
        let tokens = tokens(text);
        let mut found = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let mut matched = None;
            for count in (1..=self.max_words.min(tokens.len() - index)).rev() {
                let span = &tokens[index..index + count];
                // A name never spans a line break or a gap wider than a short punctuation run.
                if span.windows(2).any(|pair| {
                    let gap = &text[pair[0].0.end..pair[1].0.start];
                    gap.contains('\n') || gap.chars().count() > 3
                }) {
                    continue;
                }
                let key = span.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" ");
                let Some(entry) = self.entries.get(&key) else { continue };
                let range = span[0].0.start..span[count - 1].0.end;
                if accepts(text, &range, entry, count) {
                    matched = Some((count, Mention { range, kind: entry.kind, id: entry.id.clone() }));
                    break;
                }
            }
            match matched {
                Some((count, mention)) => {
                    found.push(mention);
                    index += count;
                }
                None => index += 1,
            }
        }
        found
    }
}

fn accepts(text: &str, range: &Range<usize>, entry: &Entry, words: usize) -> bool {
    let found = &text[range.clone()];
    let first = found.chars().next().unwrap_or(' ');
    let name_first = entry.name.chars().find(|c| c.is_alphanumeric()).unwrap_or(' ');
    if !(first.is_uppercase() || first.is_numeric() || name_first.is_lowercase()) {
        return false;
    }
    if entry.name.chars().filter(|c| c.is_alphanumeric()).count() < 4 && found != entry.name {
        return false;
    }
    if entry.kind == MentionKind::Song && words == 1 {
        let before = text[..range.start].chars().next_back();
        return before.is_some_and(|c| matches!(c, '"' | '“' | '«' | '\'' | '‘'));
    }
    true
}

/// Words with their byte ranges, lowercased. Apostrophes inside a word are kept.
fn tokens(text: &str) -> Vec<(Range<usize>, String)> {
    let mut words = Vec::new();
    let mut start = None;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let inner_apostrophe = matches!(c, '\'' | '’') && start.is_some() && chars.peek().is_some_and(|(_, next)| next.is_alphanumeric());
        if c.is_alphanumeric() || inner_apostrophe {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            words.push(begin..index);
        }
    }
    if let Some(begin) = start {
        words.push(begin..text.len());
    }
    words.into_iter().map(|range| (range.clone(), text[range].to_lowercase().replace('’', "'"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> MentionIndex {
        let mut index = MentionIndex::default();
        index.add("The Beatles", MentionKind::Artist, "ar-1");
        index.add("Air", MentionKind::Artist, "ar-2");
        index.add("Sgt. Pepper's Lonely Hearts Club Band", MentionKind::Album, "al-1");
        index.add("Revolver", MentionKind::Album, "al-2");
        index.add("Help", MentionKind::Song, "so-1");
        index.add("Here Comes the Sun", MentionKind::Song, "so-2");
        index.add("Revolver", MentionKind::Song, "so-3");
        index
    }

    fn names(text: &str) -> Vec<(&str, MentionKind)> {
        index().find(text).into_iter().map(|m| (&text[m.range], m.kind)).collect()
    }

    #[test]
    fn finds_names_ignoring_case_and_punctuation() {
        let text = "The Beatles released Sgt Pepper’s Lonely Hearts Club Band after Revolver; HERE COMES THE SUN came later.";
        assert_eq!(
            names(text),
            vec![
                ("The Beatles", MentionKind::Artist),
                ("Sgt Pepper’s Lonely Hearts Club Band", MentionKind::Album),
                ("Revolver", MentionKind::Album),
                ("HERE COMES THE SUN", MentionKind::Song),
            ]
        );
    }

    #[test]
    fn skips_ordinary_words() {
        assert!(names("They needed help and some fresh air, like the beatles of old.").is_empty());
        assert!(names("A song called Help").is_empty());
        assert_eq!(names("A song called \"Help\" and an album by Air."), vec![("Help", MentionKind::Song), ("Air", MentionKind::Artist)]);
    }

    #[test]
    fn names_do_not_span_lines() {
        assert!(names("The\nBeatles").is_empty());
        assert_eq!(names("Here Comes\n\nthe Sun, by The Beatles"), vec![("The Beatles", MentionKind::Artist)]);
    }

    #[test]
    fn ignores_unusable_entries() {
        let mut index = MentionIndex::default();
        index.add("", MentionKind::Artist, "a");
        index.add("X", MentionKind::Artist, "b");
        index.add("Valid", MentionKind::Artist, "");
        assert!(index.is_empty());
    }
}
