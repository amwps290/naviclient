//! Time-range aggregation for local listening insights.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Local, TimeZone, Timelike};

use super::log_store::ListeningLogEntry;
use crate::models::Song;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListeningRange {
    Last7Days,
    #[default]
    Last30Days,
    ThisYear,
    AllTime,
}

impl ListeningRange {
    pub const ALL: [Self; 4] = [
        Self::Last7Days,
        Self::Last30Days,
        Self::ThisYear,
        Self::AllTime,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::ThisYear => "year",
            Self::AllTime => "all",
        }
    }

    pub fn start_timestamp(self, now_ts: u64) -> Option<u64> {
        match self {
            Self::Last7Days => Some(now_ts.saturating_sub(7 * 86_400)),
            Self::Last30Days => Some(now_ts.saturating_sub(30 * 86_400)),
            Self::ThisYear => {
                let dt = Local.timestamp_opt(now_ts as i64, 0).single()?;
                let jan1 = Local
                    .with_ymd_and_hms(dt.year(), 1, 1, 0, 0, 0)
                    .single()?;
                u64::try_from(jan1.timestamp()).ok()
            }
            Self::AllTime => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TopSongStat {
    pub song: Song,
    pub play_count: usize,
    pub listened_secs: u64,
}

#[derive(Clone, Debug)]
pub struct RankedStat {
    pub name: String,
    pub subtitle: Option<String>,
    pub play_count: usize,
    pub listened_secs: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ListeningInsights {
    pub total_listened_secs: u64,
    pub play_count: usize,
    pub completed_count: usize,
    pub unique_songs: usize,
    pub unique_artists: usize,
    pub unique_albums: usize,
    pub active_days: usize,
    pub avg_daily_secs: u64,
    pub top_songs: Vec<TopSongStat>,
    pub top_artists: Vec<RankedStat>,
    pub top_albums: Vec<RankedStat>,
    pub top_genres: Vec<RankedStat>,
    pub hourly_secs: [u64; 24],
    pub peak_hour: Option<usize>,
}

pub fn aggregate_insights(
    entries: &[ListeningLogEntry],
    range: ListeningRange,
    now_ts: u64,
) -> ListeningInsights {
    let min_ts = range.start_timestamp(now_ts);
    let mut total_listened_secs = 0u64;
    let mut play_count = 0usize;
    let mut completed_count = 0usize;
    let mut unique_songs = HashSet::new();
    let mut unique_artists = HashSet::new();
    let mut unique_albums = HashSet::new();
    let mut active_days = HashSet::new();
    let mut hourly_secs = [0u64; 24];

    // Map key -> (latest Song, play_count, listened_secs)
    let mut songs_map: HashMap<(Option<String>, String), (Song, usize, u64)> = HashMap::new();
    let mut artists_map: HashMap<String, (String, usize, u64)> = HashMap::new();
    let mut albums_map: HashMap<(String, String), (String, String, usize, u64)> = HashMap::new();
    let mut genres_map: HashMap<String, (String, usize, u64)> = HashMap::new();
    let mut earliest_ts: Option<u64> = None;

    for entry in entries {
        if let Some(min) = min_ts {
            if entry.ts < min {
                continue;
            }
        }
        let secs = effective_listened_secs(entry);
        play_count += 1;
        total_listened_secs += secs;
        if entry.completed {
            completed_count += 1;
        }
        earliest_ts = Some(earliest_ts.map_or(entry.ts, |prev| prev.min(entry.ts)));

        let source_key = entry.source.as_ref().map(|s| s.cache_namespace());
        let song_key = (source_key, entry.song_id.clone());
        unique_songs.insert(song_key.clone());
        songs_map
            .entry(song_key)
            .and_modify(|(song, count, total)| {
                *count += 1;
                *total += secs;
                if song.cover_art.is_none() {
                    let candidate = entry.to_song();
                    if candidate.cover_art.is_some() {
                        *song = candidate;
                    }
                }
            })
            .or_insert_with(|| (entry.to_song(), 1, secs));

        let artist = entry.artist.trim();
        if !artist.is_empty() {
            let norm = artist.to_lowercase();
            unique_artists.insert(norm.clone());
            artists_map
                .entry(norm)
                .and_modify(|(_, count, total)| {
                    *count += 1;
                    *total += secs;
                })
                .or_insert_with(|| (artist.to_string(), 1, secs));
        }

        let album = entry.album.trim();
        if !album.is_empty() {
            let key = (album.to_lowercase(), artist.to_lowercase());
            unique_albums.insert(key.clone());
            albums_map
                .entry(key)
                .and_modify(|(_, _, count, total)| {
                    *count += 1;
                    *total += secs;
                })
                .or_insert_with(|| (album.to_string(), artist.to_string(), 1, secs));
        }

        if let Some(genre) = entry.genre.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            let norm = genre.to_lowercase();
            genres_map
                .entry(norm)
                .and_modify(|(_, count, total)| {
                    *count += 1;
                    *total += secs;
                })
                .or_insert_with(|| (genre.to_string(), 1, secs));
        }

        if let Some(dt) = Local.timestamp_opt(entry.ts as i64, 0).single() {
            let hour = (dt.hour() as usize).min(23);
            hourly_secs[hour] += secs;
            active_days.insert((dt.year(), dt.ordinal()));
        }
    }

    let mut top_songs: Vec<TopSongStat> = songs_map
        .into_values()
        .map(|(song, play_count, listened_secs)| TopSongStat {
            song,
            play_count,
            listened_secs,
        })
        .collect();
    top_songs.sort_by(|a, b| {
        b.play_count
            .cmp(&a.play_count)
            .then_with(|| b.listened_secs.cmp(&a.listened_secs))
            .then_with(|| a.song.title.to_lowercase().cmp(&b.song.title.to_lowercase()))
    });
    top_songs.truncate(20);

    let mut top_artists: Vec<RankedStat> = artists_map
        .into_values()
        .map(|(name, play_count, listened_secs)| RankedStat {
            name,
            subtitle: None,
            play_count,
            listened_secs,
        })
        .collect();
    top_artists.sort_by(|a, b| {
        b.listened_secs
            .cmp(&a.listened_secs)
            .then_with(|| b.play_count.cmp(&a.play_count))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    top_artists.truncate(10);

    let mut top_albums: Vec<RankedStat> = albums_map
        .into_values()
        .map(|(name, artist, play_count, listened_secs)| RankedStat {
            name,
            subtitle: (!artist.is_empty()).then_some(artist),
            play_count,
            listened_secs,
        })
        .collect();
    top_albums.sort_by(|a, b| {
        b.listened_secs
            .cmp(&a.listened_secs)
            .then_with(|| b.play_count.cmp(&a.play_count))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    top_albums.truncate(8);

    let mut top_genres: Vec<RankedStat> = genres_map
        .into_values()
        .map(|(name, play_count, listened_secs)| RankedStat {
            name,
            subtitle: None,
            play_count,
            listened_secs,
        })
        .collect();
    top_genres.sort_by(|a, b| {
        b.listened_secs
            .cmp(&a.listened_secs)
            .then_with(|| b.play_count.cmp(&a.play_count))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    top_genres.truncate(8);

    let peak_hour = hourly_secs
        .iter()
        .enumerate()
        .filter(|(_, &secs)| secs > 0)
        .max_by_key(|(_, &secs)| secs)
        .map(|(hour, _)| hour);

    let window_days = match range {
        ListeningRange::Last7Days => 7u64,
        ListeningRange::Last30Days => 30u64,
        ListeningRange::ThisYear => {
            let start = range.start_timestamp(now_ts).unwrap_or(now_ts);
            (now_ts.saturating_sub(start) / 86_400).max(1)
        }
        ListeningRange::AllTime => earliest_ts
            .map(|first| (now_ts.saturating_sub(first) / 86_400).max(1))
            .unwrap_or(1),
    };
    let avg_daily_secs = if play_count == 0 {
        0
    } else {
        total_listened_secs / window_days.max(1)
    };

    ListeningInsights {
        total_listened_secs,
        play_count,
        completed_count,
        unique_songs: unique_songs.len(),
        unique_artists: unique_artists.len(),
        unique_albums: unique_albums.len(),
        active_days: active_days.len(),
        avg_daily_secs,
        top_songs,
        top_artists,
        top_albums,
        top_genres,
        hourly_secs,
        peak_hour,
    }
}

fn effective_listened_secs(entry: &ListeningLogEntry) -> u64 {
    if entry.listened_secs > 0 {
        u64::from(entry.listened_secs)
    } else if entry.completed {
        entry
            .duration_secs
            .filter(|&d| d > 0)
            .map(|d| d as u64)
            .unwrap_or(0)
    } else {
        0
    }
}
