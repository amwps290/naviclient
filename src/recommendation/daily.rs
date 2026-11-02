use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{anyhow, Result};
use chrono::Local;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use super::{
    recommend, CandidateSource, ListeningProfile, RecommendationCandidate, RecommendationContext,
    RecommendationKind, RecommendationRequest, RecommendedTrack,
};
use crate::api::OpenSubsonicProvider;
use crate::models::{Album, CapabilitySupport, Song};

pub const DAILY_MIX_VERSION: u32 = 2;
const DAILY_TARGET: usize = 30;
const MAX_ALBUM_REQUESTS: usize = 10;
const MAX_ALBUM_TRACKS: usize = 12;
const MAX_SIMILARITY_SEEDS: usize = 4;
const MAX_GENRE_SEEDS: usize = 2;
const MAX_CANDIDATES: usize = 240;
const REQUEST_CONCURRENCY: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyMixSnapshot {
    pub server_id: String,
    pub day: String,
    pub algorithm_version: u32,
    pub variant: u32,
    pub generated_at: u64,
    pub tracks: Vec<RecommendedTrack>,
}

impl DailyMixSnapshot {
    pub fn is_current(&self, server_id: &str, day: &str) -> bool {
        self.server_id == server_id
            && self.day == day
            && self.algorithm_version == DAILY_MIX_VERSION
    }

    pub fn songs(&self) -> Vec<Song> {
        self.tracks.iter().map(|track| track.song.clone()).collect()
    }
}

pub fn current_day_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

#[derive(Clone, Debug)]
pub struct DailyMixGenerationContext {
    pub server_id: String,
    pub day: String,
    pub variant: u32,
    pub sonic_similarity: CapabilitySupport,
    pub recent_song_ids: HashSet<String>,
    pub listening_profile: ListeningProfile,
    pub excluded_song_ids: HashSet<String>,
}

pub async fn generate_daily_mix(
    api: OpenSubsonicProvider,
    context: DailyMixGenerationContext,
) -> Result<DailyMixSnapshot> {
    let DailyMixGenerationContext {
        server_id,
        day,
        variant,
        sonic_similarity,
        recent_song_ids,
        listening_profile,
        excluded_song_ids,
    } = context;
    // Independent factual sources fail softly. A temporary failure in one
    // shelf must not discard usable candidates from the others.
    let (favorites, frequent, recent, newest, random) = tokio::join!(
        api.favorites(),
        api.albums_frequent(4),
        api.albums_recent(2),
        api.albums_newest(3),
        api.albums_random(3),
    );
    let mut source_errors = Vec::new();
    let favorites = favorites.unwrap_or_else(|error| {
        source_errors.push(format!("favorites: {error:#}"));
        Default::default()
    });
    let frequent = frequent.unwrap_or_else(|error| {
        source_errors.push(format!("frequent albums: {error:#}"));
        Vec::new()
    });
    let newest = newest.unwrap_or_else(|error| {
        source_errors.push(format!("newest albums: {error:#}"));
        Vec::new()
    });
    let recent = recent.unwrap_or_else(|error| {
        source_errors.push(format!("recent albums: {error:#}"));
        Vec::new()
    });
    let random = random.unwrap_or_else(|error| {
        source_errors.push(format!("random albums: {error:#}"));
        Vec::new()
    });

    let mut candidates = favorites
        .songs
        .iter()
        .take(60)
        .cloned()
        .map(|song| RecommendationCandidate {
            song,
            source: CandidateSource::Favorite,
        })
        .collect::<Vec<_>>();

    let album_plan = merge_album_plan([
        (frequent, CandidateSource::Frequent),
        (recent, CandidateSource::Frequent),
        (newest, CandidateSource::Newest),
        (random, CandidateSource::Random),
    ]);
    let album_candidates = collect_album_candidates(api.clone(), album_plan).await;
    candidates.extend(album_candidates);

    let seeds = select_style_seeds(&favorites.songs, &candidates, &listening_profile);
    let style = StyleSignature::from_seeds(&seeds);
    candidates.retain(|candidate| {
        !matches!(
            candidate.source,
            CandidateSource::Newest | CandidateSource::Random
        ) || style.matches(&candidate.song, &listening_profile)
    });

    if sonic_similarity.allows_optimistic_request() {
        candidates.extend(collect_similar_candidates(api.clone(), &seeds).await);
    }
    candidates.extend(collect_genre_candidates(api, &seeds).await);

    if candidates.is_empty() {
        return Err(if source_errors.is_empty() {
            anyhow!("the server returned no daily mix candidates")
        } else {
            anyhow!(source_errors.join("; "))
        });
    }
    if !source_errors.is_empty() {
        log::debug!(
            "daily mix generated with partial source failures: {}",
            source_errors.join("; ")
        );
    }
    candidates.truncate(MAX_CANDIDATES);

    let seed = daily_seed(&server_id, &day, DAILY_MIX_VERSION, variant);
    let tracks = recommend(
        &RecommendationRequest {
            kind: RecommendationKind::DailyMix,
            target_count: DAILY_TARGET,
            seed,
        },
        candidates,
        &RecommendationContext {
            excluded_song_ids,
            recent_song_ids,
            listening_profile,
            ..RecommendationContext::with_daily_mix_defaults()
        },
    );
    if tracks.is_empty() {
        return Err(anyhow!("all daily mix candidates were filtered out"));
    }

    Ok(DailyMixSnapshot {
        server_id,
        day,
        algorithm_version: DAILY_MIX_VERSION,
        variant,
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        tracks,
    })
}

#[derive(Default)]
struct StyleSignature {
    genres: HashSet<String>,
    artists: HashSet<String>,
}

impl StyleSignature {
    fn from_seeds(seeds: &[Song]) -> Self {
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

    fn matches(&self, song: &Song, profile: &ListeningProfile) -> bool {
        if self.genres.is_empty() && self.artists.is_empty() {
            return true;
        }
        let genre_match = normalized_genre(song).is_some_and(|genre| self.genres.contains(&genre));
        let artist_match =
            normalized_artist(song).is_some_and(|artist| self.artists.contains(&artist));
        genre_match || artist_match || profile.score(song) >= 4.0
    }
}

fn select_style_seeds(
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
    let Some((primary, _)) = ranked.first().cloned() else {
        return Vec::new();
    };
    let primary_genre = normalized_genre(&primary);
    let primary_artist = normalized_artist(&primary);
    let mut seeds = vec![primary];
    for (song, _) in ranked.into_iter().skip(1) {
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

fn normalized_genre(song: &Song) -> Option<String> {
    song.genre
        .as_deref()
        .map(str::trim)
        .filter(|genre| !genre.is_empty())
        .map(str::to_lowercase)
}

fn normalized_artist(song: &Song) -> Option<String> {
    song.artist_id
        .as_deref()
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
        .map(|artist| format!("id:{artist}"))
        .or_else(|| {
            let artist = song.artist.trim();
            (!artist.is_empty()).then(|| format!("name:{}", artist.to_lowercase()))
        })
}

fn merge_album_plan<const N: usize>(
    groups: [(Vec<Album>, CandidateSource); N],
) -> Vec<(Album, Vec<CandidateSource>)> {
    let mut order = Vec::new();
    let mut merged: HashMap<String, (Album, BTreeSet<CandidateSource>)> = HashMap::new();
    for (albums, source) in groups {
        for album in albums {
            if album.id.trim().is_empty() {
                continue;
            }
            if !merged.contains_key(&album.id) {
                if order.len() == MAX_ALBUM_REQUESTS {
                    continue;
                }
                order.push(album.id.clone());
                merged.insert(album.id.clone(), (album.clone(), BTreeSet::new()));
            }
            if let Some((_, sources)) = merged.get_mut(&album.id) {
                sources.insert(source);
            }
        }
    }
    order
        .into_iter()
        .filter_map(|id| {
            merged
                .remove(&id)
                .map(|(album, sources)| (album, sources.into_iter().collect()))
        })
        .collect()
}

async fn collect_album_candidates(
    api: OpenSubsonicProvider,
    plan: Vec<(Album, Vec<CandidateSource>)>,
) -> Vec<RecommendationCandidate> {
    let mut candidates = Vec::new();
    for chunk in plan.chunks(REQUEST_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for (album, sources) in chunk.iter().cloned() {
            let api = api.clone();
            tasks.spawn(
                async move { (album.id.clone(), sources, api.album_songs(&album.id).await) },
            );
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_album_id, sources, Ok(songs))) => {
                    for song in songs.into_iter().take(MAX_ALBUM_TRACKS) {
                        for source in &sources {
                            candidates.push(RecommendationCandidate {
                                song: song.clone(),
                                source: *source,
                            });
                        }
                    }
                }
                Ok((album_id, _, Err(error))) => {
                    log::debug!("daily mix album {album_id} unavailable: {error:#}")
                }
                Err(error) => log::debug!("daily mix album task failed: {error}"),
            }
        }
    }
    candidates
}

async fn collect_similar_candidates(
    api: OpenSubsonicProvider,
    seeds: &[Song],
) -> Vec<RecommendationCandidate> {
    let mut candidates = Vec::new();
    for chunk in seeds.chunks(REQUEST_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for song in chunk {
            let api = api.clone();
            let song_id = song.id.clone();
            tasks.spawn(async move { (song_id.clone(), api.similar_songs(&song_id, 20).await) });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_seed, Ok(songs))) => {
                    candidates.extend(songs.into_iter().map(|song| RecommendationCandidate {
                        song,
                        source: CandidateSource::Similar,
                    }))
                }
                Ok((seed, Err(error))) => {
                    log::debug!("daily mix similarity seed {seed} unavailable: {error:#}")
                }
                Err(error) => log::debug!("daily mix similarity task failed: {error}"),
            }
        }
    }
    candidates
}

async fn collect_genre_candidates(
    api: OpenSubsonicProvider,
    seeds: &[Song],
) -> Vec<RecommendationCandidate> {
    let genres = seeds
        .iter()
        .filter_map(|song| song.genre.as_deref())
        .map(str::trim)
        .filter(|genre| !genre.is_empty())
        .fold(Vec::<String>::new(), |mut genres, genre| {
            if !genres.iter().any(|known| known.eq_ignore_ascii_case(genre))
                && genres.len() < MAX_GENRE_SEEDS
            {
                genres.push(genre.to_string());
            }
            genres
        });
    let mut tasks = JoinSet::new();
    for genre in genres {
        let api = api.clone();
        tasks.spawn(async move {
            let result = api.songs_by_genre(&genre, 20, 0).await;
            (genre, result)
        });
    }
    let mut candidates = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((_genre, Ok(songs))) => {
                candidates.extend(songs.into_iter().map(|song| RecommendationCandidate {
                    song,
                    source: CandidateSource::SameGenre,
                }))
            }
            Ok((genre, Err(error))) => {
                log::debug!("daily mix genre {genre:?} unavailable: {error:#}")
            }
            Err(error) => log::debug!("daily mix genre task failed: {error}"),
        }
    }
    candidates
}

fn daily_seed(server_id: &str, day: &str, version: u32, variant: u32) -> u64 {
    let mut hasher = Md5::new();
    hasher.update(server_id.as_bytes());
    hasher.update(day.as_bytes());
    hasher.update(version.to_le_bytes());
    hasher.update(variant.to_le_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: &str) -> Album {
        Album {
            id: id.into(),
            name: id.into(),
            ..Album::default()
        }
    }

    fn styled_song(id: &str, artist: &str, genre: &str) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            artist: artist.into(),
            artist_id: Some(format!("artist-{artist}")),
            genre: Some(genre.into()),
            ..Song::default()
        }
    }

    #[test]
    fn style_seeds_stay_inside_the_primary_genre_cluster() {
        let favorites = vec![
            styled_song("a-pop", "one", "Pop"),
            styled_song("b-pop", "two", "pop"),
            styled_song("c-rock", "three", "Rock"),
        ];
        let seeds = select_style_seeds(&favorites, &[], &ListeningProfile::default());
        assert_eq!(
            seeds
                .iter()
                .map(|song| song.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a-pop", "b-pop"]
        );
    }

    #[test]
    fn exploration_requires_the_seed_style_when_metadata_is_available() {
        let seeds = vec![styled_song("seed", "one", "Ambient")];
        let style = StyleSignature::from_seeds(&seeds);
        let profile = ListeningProfile::default();
        assert!(style.matches(&styled_song("same", "two", "ambient"), &profile));
        assert!(style.matches(&styled_song("artist", "one", "Rock"), &profile));
        assert!(!style.matches(&styled_song("off", "three", "Metal"), &profile));
    }

    #[test]
    fn album_plan_is_bounded_deduplicated_and_keeps_all_sources() {
        let plan = merge_album_plan([
            (
                vec![album("shared"), album("frequent")],
                CandidateSource::Frequent,
            ),
            (vec![album("shared"), album("new")], CandidateSource::Newest),
            (
                (0..20).map(|index| album(&format!("r{index}"))).collect(),
                CandidateSource::Random,
            ),
        ]);
        assert_eq!(plan.len(), MAX_ALBUM_REQUESTS);
        assert_eq!(plan[0].0.id, "shared");
        assert_eq!(
            plan[0].1,
            vec![CandidateSource::Frequent, CandidateSource::Newest]
        );
    }

    #[test]
    fn daily_seed_is_stable_and_variant_sensitive() {
        assert_eq!(
            daily_seed("server", "2026-11-02", 1, 0),
            daily_seed("server", "2026-11-02", 1, 0)
        );
        assert_ne!(
            daily_seed("server", "2026-11-02", 1, 0),
            daily_seed("server", "2026-11-02", 1, 1)
        );
        assert_ne!(
            daily_seed("server-a", "2026-11-02", 1, 0),
            daily_seed("server-b", "2026-11-02", 1, 0)
        );
    }

    #[test]
    fn snapshot_round_trips_with_recommendation_reasons() {
        let snapshot = DailyMixSnapshot {
            server_id: "server".into(),
            day: "2026-11-02".into(),
            algorithm_version: DAILY_MIX_VERSION,
            variant: 2,
            generated_at: 123,
            tracks: vec![RecommendedTrack {
                song: Song {
                    id: "song".into(),
                    title: "Song".into(),
                    ..Song::default()
                },
                reason: super::super::RecommendationReason::Explore,
                score: 15.5,
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: DailyMixSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.variant, 2);
        assert_eq!(restored.tracks[0].song.id, "song");
        assert_eq!(
            restored.tracks[0].reason,
            super::super::RecommendationReason::Explore
        );
    }

    #[test]
    fn snapshot_validity_requires_server_day_and_algorithm_version() {
        let snapshot = DailyMixSnapshot {
            server_id: "server".into(),
            day: "2026-11-02".into(),
            algorithm_version: DAILY_MIX_VERSION,
            variant: 0,
            generated_at: 0,
            tracks: Vec::new(),
        };
        assert!(snapshot.is_current("server", "2026-11-02"));
        assert!(!snapshot.is_current("other", "2026-11-02"));
        assert!(!snapshot.is_current("server", "2026-11-03"));
    }
}
