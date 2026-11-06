//! Primary style seeds and admission rules for exploratory candidates.

use std::collections::{HashMap, HashSet};

use super::super::metadata::{
    song_artist_key as normalized_artist, song_genre_key as normalized_genre,
};
use super::super::{CandidateSource, ListeningProfile, RecommendationCandidate};
use crate::models::Song;

pub(super) const MAX_SIMILARITY_SEEDS: usize = 4;

#[derive(Default)]
pub(super) struct StyleSignature {
    genres: HashSet<String>,
    artists: HashSet<String>,
}

impl StyleSignature {
    pub(super) fn from_seeds(seeds: &[Song]) -> Self {
        let mut signature = Self::default();
        for song in seeds {
            if let Some(genre) = normalized_genre(song) {
                signature.genres.insert(genre);
            }
            if let Some(artist) = normalized_artist(song) {
                signature.artists.insert(artist);
            }
        }
        signature
    }

    pub(super) fn matches(&self, song: &Song, profile: &ListeningProfile) -> bool {
        if self.genres.is_empty() && self.artists.is_empty() {
            return true;
        }
        let genre_match = normalized_genre(song).is_some_and(|genre| self.genres.contains(&genre));
        let artist_match =
            normalized_artist(song).is_some_and(|artist| self.artists.contains(&artist));
        genre_match || artist_match || profile.score(song) >= 4.0
    }
}

pub(super) fn select_style_seeds(
    favorites: &[Song],
    candidates: &[RecommendationCandidate],
    profile: &ListeningProfile,
) -> Vec<Song> {
    let mut pool: HashMap<String, (Song, f32)> = HashMap::new();
    for (song, base) in favorites.iter().map(|song| (song, 20.0)).chain(
        candidates
            .iter()
            .filter(|candidate| candidate.source == CandidateSource::Frequent)
            .map(|candidate| (&candidate.song, 10.0)),
    ) {
        if song.id.trim().is_empty() {
            continue;
        }
        let score = base + profile.score(song);
        pool.entry(song.id.clone())
            .and_modify(|(_, known_score)| *known_score = known_score.max(score))
            .or_insert_with(|| (song.clone(), score));
    }
    let mut ranked = pool.into_values().collect::<Vec<_>>();
    ranked.sort_by(|(left_song, left_score), (right_song, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left_song.id.cmp(&right_song.id))
    });
    let mut ranked = ranked.into_iter();
    let Some((primary, _)) = ranked.next() else {
        return Vec::new();
    };
    let primary_genre = normalized_genre(&primary);
    let primary_artist = normalized_artist(&primary);
    let mut seeds = vec![primary];
    for (song, _) in ranked {
        let same_genre = primary_genre
            .as_ref()
            .is_some_and(|genre| normalized_genre(&song).as_ref() == Some(genre));
        let same_artist = primary_artist
            .as_ref()
            .is_some_and(|artist| normalized_artist(&song).as_ref() == Some(artist));
        if same_genre || same_artist {
            seeds.push(song);
            if seeds.len() == MAX_SIMILARITY_SEEDS {
                break;
            }
        }
    }
    seeds
}
