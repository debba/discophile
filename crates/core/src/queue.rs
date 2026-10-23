use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::models::Song;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackQueue {
    songs: Vec<Song>,
    index: usize,
    #[serde(default)]
    pub repeat_mode: RepeatMode,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn songs(&self) -> &[Song] {
        &self.songs
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn current(&self) -> Option<&Song> {
        self.songs.get(self.index)
    }
    pub fn upcoming(&self) -> &[Song] {
        self.songs.get(self.index + 1..).unwrap_or_default()
    }
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    pub fn replace(&mut self, songs: Vec<Song>, start_at: usize, shuffled: bool) {
        if start_at >= songs.len() {
            self.songs.clear();
            self.index = 0;
            return;
        }
        if shuffled {
            let mut rest = songs;
            let first = rest.remove(start_at);
            rest.shuffle(&mut rand::rng());
            self.songs = std::iter::once(first).chain(rest).collect();
            self.index = 0;
        } else {
            self.songs = songs;
            self.index = start_at;
        }
    }

    /// Moves to the next song. Repeat One keeps the current song only for automatic advances.
    pub fn advance(&mut self, automatically: bool) -> bool {
        if self.current().is_none() {
            return false;
        }
        if automatically && self.repeat_mode == RepeatMode::One {
            return true;
        }
        if self.index + 1 < self.songs.len() {
            self.index += 1;
            return true;
        }
        if self.repeat_mode == RepeatMode::All {
            self.index = 0;
            return true;
        }
        false
    }

    /// The song an automatic advance would play, without moving.
    pub fn peek_next(&self, automatically: bool) -> Option<&Song> {
        let mut copy = self.clone();
        if copy.advance(automatically) { self.songs.get(copy.index) } else { None }
    }

    pub fn previous(&mut self) -> bool {
        if self.index == 0 {
            return false;
        }
        self.index -= 1;
        true
    }
    pub fn play_next(&mut self, song: Song) {
        if self.songs.is_empty() {
            self.replace(vec![song], 0, false);
        } else {
            self.songs.insert(self.index + 1, song);
        }
    }
    pub fn append(&mut self, song: Song) {
        self.songs.push(song);
    }
    pub fn shuffle_upcoming(&mut self) {
        if self.current().is_none() {
            return;
        }
        let mut upcoming = self.songs.split_off(self.index + 1);
        upcoming.shuffle(&mut rand::rng());
        self.songs.extend(upcoming);
    }
    pub fn select(&mut self, index: usize) {
        if index < self.songs.len() {
            self.index = index;
        }
    }
    /// Removes an upcoming or previous entry. The current song cannot be removed.
    pub fn remove(&mut self, index: usize) {
        if index >= self.songs.len() || index == self.index {
            return;
        }
        self.songs.remove(index);
        if index < self.index {
            self.index -= 1;
        }
    }
    /// Moves an entry, keeping the current song selected.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.songs.len() || to >= self.songs.len() || from == to {
            return;
        }
        let current_id = self.index;
        let song = self.songs.remove(from);
        self.songs.insert(to, song);
        self.index = if from == current_id {
            to
        } else if from < current_id && to >= current_id {
            current_id - 1
        } else if from > current_id && to <= current_id {
            current_id + 1
        } else {
            current_id
        };
    }
    /// Appends songs without duplicating the current queue's entries (used by radio refills).
    pub fn extend(&mut self, songs: impl IntoIterator<Item = Song>) {
        self.songs.extend(songs);
    }
    /// Keeps at most `limit` songs around the current index (resume queues keep up to 200).
    pub fn truncated(&self, limit: usize) -> PlaybackQueue {
        if self.songs.len() <= limit {
            return self.clone();
        }
        let start = self.index.saturating_sub(limit / 4).min(self.songs.len() - limit);
        PlaybackQueue {
            songs: self.songs[start..start + limit].to_vec(),
            index: self.index - start,
            repeat_mode: self.repeat_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(n: usize) -> Vec<Song> {
        (0..n).map(|i| Song::new(i.to_string(), format!("Song {i}"))).collect()
    }

    #[test]
    fn advance_and_repeat() {
        let mut q = PlaybackQueue::new();
        q.replace(songs(3), 1, false);
        assert_eq!(q.current().unwrap().id, "1");
        assert!(q.advance(false));
        assert!(!q.advance(false));
        q.repeat_mode = RepeatMode::All;
        assert!(q.advance(false));
        assert_eq!(q.index(), 0);
        q.repeat_mode = RepeatMode::One;
        assert!(q.advance(true));
        assert_eq!(q.index(), 0);
        assert!(q.advance(false));
        assert_eq!(q.index(), 1);
    }

    #[test]
    fn shuffle_keeps_selected_first() {
        let mut q = PlaybackQueue::new();
        q.replace(songs(20), 7, true);
        assert_eq!(q.index(), 0);
        assert_eq!(q.current().unwrap().id, "7");
        assert_eq!(q.songs().len(), 20);
    }

    #[test]
    fn move_and_remove_keep_current() {
        let mut q = PlaybackQueue::new();
        q.replace(songs(5), 2, false);
        q.move_item(4, 0);
        assert_eq!(q.current().unwrap().id, "2");
        q.remove(0);
        assert_eq!(q.current().unwrap().id, "2");
        q.remove(q.index());
        assert_eq!(q.current().unwrap().id, "2");
    }

    #[test]
    fn truncation_keeps_current() {
        let mut q = PlaybackQueue::new();
        q.replace(songs(500), 450, false);
        let t = q.truncated(200);
        assert_eq!(t.songs().len(), 200);
        assert_eq!(t.current().unwrap().id, "450");
    }
}
