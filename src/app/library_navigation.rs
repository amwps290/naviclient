//! Metadata projections used by song links, menus and library navigation.

use rust_i18n::t;

use crate::models::{Album, Artist, Song};

pub(super) fn artist_for_song(song: &Song) -> Option<Artist> {
    Some(Artist {
        id: song
            .artist_id
            .as_ref()
            .filter(|id| !id.trim().is_empty())?
            .clone(),
        name: song.artist.clone(),
        album_count: None,
        cover_art: song.cover_art.clone(),
    })
}

pub(super) fn album_for_song(song: &Song) -> Option<Album> {
    Some(Album {
        id: song
            .album_id
            .as_ref()
            .filter(|id| !id.trim().is_empty())?
            .clone(),
        name: if song.album.trim().is_empty() {
            t!("Unknown album").to_string()
        } else {
            song.album.clone()
        },
        artist: song.artist.clone(),
        artist_id: song.artist_id.clone().filter(|id| !id.trim().is_empty()),
        cover_art: song.cover_art.clone(),
        ..Album::default()
    })
}

#[cfg(test)]
mod tests;
