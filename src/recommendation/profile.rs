//! Listening affinity derived from bounded, decaying local history.

use std::collections::HashMap;

use super::metadata::{song_artist_key, song_genre_key};
use crate::models::{ListeningEvent, ListeningHistory, Song};

#[derive(Clone, Debug, Default)]
pub struct ListeningProfile {
    song_scores: HashMap<String, f32>,
    artist_scores: HashMap<String, f32>,
    genre_scores: HashMap<String, f32>,
}

impl ListeningProfile {
    pub fn from_history(history: &ListeningHistory) -> Self {
        let mut profile = Self::default();
        for (age, event) in history.events.iter().rev().take(500).enumerate() {
            let decay = 0.985_f32.powi(i32::try_from(age).unwrap_or(i32::MAX));
            let weight = listening_event_weight(event) * decay;
            *profile
                .song_scores
                .entry(event.song.id.clone())
                .or_default() += weight;
            if let Some(artist) = song_artist_key(&event.song) {
                *profile.artist_scores.entry(artist).or_default() += weight;
            }
            if let Some(genre) = song_genre_key(&event.song) {
                *profile.genre_scores.entry(genre).or_default() += weight;
            }
        }
        profile
    }

    pub fn score(&self, song: &Song) -> f32 {
        let song_score = self
            .song_scores
            .get(&song.id)
            .copied()
            .unwrap_or_default()
            .clamp(-3.0, 8.0)
            * 2.0;
        let artist_score = song_artist_key(song)
            .and_then(|key| self.artist_scores.get(&key).copied())
            .unwrap_or_default()
            .clamp(-2.0, 8.0)
            * 1.5;
        let genre_score = song_genre_key(song)
            .and_then(|key| self.genre_scores.get(&key).copied())
            .unwrap_or_default()
            .clamp(-2.0, 8.0)
            * 0.75;
        song_score + artist_score + genre_score
    }
}

fn listening_event_weight(event: &ListeningEvent) -> f32 {
    if event.completed {
        return 3.0;
    }
    if event.listened_seconds == 0 {
        return 0.0;
    }
    let duration = event.song.duration.unwrap_or_default().max(0) as f32;
    let listened = event.listened_seconds as f32;
    if event.listened_seconds >= 4 * 60 || duration > 0.0 && listened >= duration * 0.5 {
        1.5
    } else if event.listened_seconds >= 30 {
        0.5
    } else {
        -0.75
    }
}
