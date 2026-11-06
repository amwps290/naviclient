//! Shared normalized artist and genre identity for affinity and style matching.

use crate::models::Song;

pub(super) fn song_artist_key(song: &Song) -> Option<String> {
    song.artist_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| format!("id:{id}"))
        .or_else(|| {
            let artist = song.artist.trim();
            (!artist.is_empty()).then(|| format!("name:{}", artist.to_lowercase()))
        })
}

pub(super) fn song_genre_key(song: &Song) -> Option<String> {
    song.genre
        .as_deref()
        .map(str::trim)
        .filter(|genre| !genre.is_empty())
        .map(str::to_lowercase)
}
