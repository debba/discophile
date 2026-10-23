//! Library names in assistant replies (see `discophile_core::mentions`): the index of the cached library
//! and the queue, rebuilt when they change, and the mentions found in each text, kept per text.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use discophile_core::mentions::{Mention, MentionIndex, MentionKind};

use crate::app::App;
use crate::nav::Route;

/// A cheap fingerprint of what the index is built from.
fn fingerprint(app: &App) -> u64 {
    let s = &app.library.snapshot;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (s.artists.len(), s.newest.len(), s.songs.len(), s.album_details.len()).hash(&mut hasher);
    s.albums.values().map(Vec::len).sum::<usize>().hash(&mut hasher);
    s.artists.first().map(|a| &a.id).hash(&mut hasher);
    let favorites = &s.favorites;
    (favorites.artist.as_ref().map(Vec::len), favorites.album.as_ref().map(Vec::len), favorites.song.as_ref().map(Vec::len)).hash(&mut hasher);
    let queue = app.player.queue.songs();
    (queue.len(), queue.first().map(|s| &s.id)).hash(&mut hasher);
    hasher.finish()
}

fn build(app: &App) -> MentionIndex {
    let s = &app.library.snapshot;
    let mut index = MentionIndex::default();
    let favorites = &s.favorites;
    for artist in s.artists.iter().chain(favorites.artist.iter().flatten()) {
        index.add(&artist.name, MentionKind::Artist, &artist.id);
    }
    let albums = s.newest.iter().chain(s.albums.values().flatten()).chain(s.album_details.values()).chain(favorites.album.iter().flatten());
    for album in albums {
        index.add(&album.name, MentionKind::Album, &album.id);
    }
    // A song opens its album, so songs are indexed by album id.
    let tracks = s.album_details.values().flat_map(discophile_core::album_listening::tracks);
    let songs = s.songs.iter().cloned().chain(favorites.song.iter().flatten().cloned()).chain(app.player.queue.songs().iter().cloned()).chain(tracks);
    for song in songs {
        if let Some(album) = song.album_id.as_deref() {
            index.add(&song.title, MentionKind::Song, album);
        }
    }
    index
}

/// The mentions of library items in `text`.
pub fn find(app: &App, ctx: &egui::Context, text: &str) -> Arc<Vec<Mention>> {
    let key = fingerprint(app);
    let index_id = egui::Id::unique("mention-index");
    let index = match ctx.data(|d| d.get_temp::<(u64, Arc<MentionIndex>)>(index_id)) {
        Some((stored, index)) if stored == key => index,
        _ => {
            let index = Arc::new(build(app));
            ctx.data_mut(|d| d.insert_temp(index_id, (key, index.clone())));
            index
        }
    };
    let text_id = egui::Id::unique(("mentions", key, text));
    if let Some(found) = ctx.data(|d| d.get_temp::<Arc<Vec<Mention>>>(text_id)) {
        return found;
    }
    let found = Arc::new(index.find(text));
    ctx.data_mut(|d| d.insert_temp(text_id, found.clone()));
    found
}

/// Opens what a mention refers to, leaving Now Playing if it is shown.
pub fn open(app: &mut App, mention: &Mention) {
    app.nav.now_playing = false;
    app.nav.push(match mention.kind {
        MentionKind::Artist => Route::Artist(mention.id.clone()),
        MentionKind::Album | MentionKind::Song => Route::Album(mention.id.clone()),
    });
}
