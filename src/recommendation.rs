//! Pure recommendation ranking shared by daily mixes, seed radios and future
//! auto-continue. Providers collect factual candidates; this module owns
//! deterministic scoring, de-duplication and diversity constraints.

use std::collections::{BTreeSet, HashMap, HashSet};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::models::{ListeningEvent, ListeningHistory, QueueOrigin, Song};

mod daily;
mod radio;
pub(crate) use daily::{
    current_day_key, generate_daily_mix, DailyMixGenerationContext, DailyMixSnapshot,
};
pub(crate) use radio::{generate_radio, RadioGenerationContext, RadioSeed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendationKind {
    DailyMix,
    SongRadio,
    ArtistRadio,
    AutoContinue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateSource {
    Favorite,
    Frequent,
    Similar,
    ArtistTop,
    SameArtist,
    SameGenre,
    Newest,
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    Favorite,
    FrequentlyPlayed,
    SimilarTrack,
    PopularByArtist,
    SameArtist,
    SameGenre,
    NewRelease,
    Explore,
}

#[derive(Clone, Debug)]
pub struct RecommendationRequest {
    pub kind: RecommendationKind,
    pub target_count: usize,
    /// Stable seed, e.g. hash(server id + date + algorithm version + variant).
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct RecommendationCandidate {
    pub song: Song,
    pub source: CandidateSource,
}

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

#[derive(Clone, Debug, Default)]
pub struct RecommendationContext {
    /// Current queue entries are always excluded; automatic results may never
    /// duplicate or overwrite the user's explicitly arranged Up Next.
    pub excluded_song_ids: HashSet<String>,
    /// Recent tracks are deferred until non-recent candidates are exhausted.
    pub recent_song_ids: HashSet<String>,
    /// Affinity derived from actual local listening duration and completion.
    pub listening_profile: ListeningProfile,
    pub max_tracks_per_artist: usize,
    pub max_tracks_per_album: usize,
}

impl RecommendationContext {
    pub fn with_daily_mix_defaults() -> Self {
        Self {
            max_tracks_per_artist: 3,
            max_tracks_per_album: 2,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendedTrack {
    pub song: Song,
    pub reason: RecommendationReason,
    pub score: f32,
    /// Queue provenance is part of the recommendation result so every caller
    /// preserves the distinction between manual and generated tracks.
    #[serde(default)]
    pub origin: QueueOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MixBucket {
    Familiar,
    Related,
    Explore,
}

#[derive(Clone)]
struct RankedCandidate {
    song: Song,
    reason: RecommendationReason,
    bucket: MixBucket,
    score: f32,
    recent: bool,
}

/// Rank and select candidates. Selection has four deterministic passes:
/// non-recent with strict diversity, non-recent relaxed, recent with strict
/// diversity, then fully relaxed. Duplicates are never reintroduced.
pub fn recommend(
    request: &RecommendationRequest,
    candidates: Vec<RecommendationCandidate>,
    context: &RecommendationContext,
) -> Vec<RecommendedTrack> {
    if request.target_count == 0 {
        return Vec::new();
    }

    let mut merged: HashMap<String, (Song, BTreeSet<CandidateSource>)> = HashMap::new();
    for candidate in candidates {
        let id = candidate.song.id.trim();
        if id.is_empty() || context.excluded_song_ids.contains(id) {
            continue;
        }
        merged
            .entry(candidate.song.id.clone())
            .and_modify(|(_, sources)| {
                sources.insert(candidate.source);
            })
            .or_insert_with(|| {
                let mut sources = BTreeSet::new();
                sources.insert(candidate.source);
                (candidate.song, sources)
            });
    }

    let mut ranked = merged
        .into_values()
        .map(|(song, sources)| {
            let strongest = sources
                .iter()
                .copied()
                .max_by_key(|source| source_weight(request.kind, *source))
                .unwrap_or(CandidateSource::Random);
            let strongest_score = source_weight(request.kind, strongest) as f32;
            let supporting_score: f32 = sources
                .iter()
                .copied()
                .filter(|source| *source != strongest)
                .map(|source| source_weight(request.kind, source) as f32 * 0.2)
                .sum();
            let score = strongest_score
                + supporting_score
                + context.listening_profile.score(&song)
                + stable_jitter(request.seed, &song.id) * 0.99;
            let recent = context.recent_song_ids.contains(&song.id);
            RankedCandidate {
                song,
                reason: source_reason(strongest),
                bucket: source_bucket(strongest),
                score,
                recent,
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.song.id.cmp(&right.song.id))
    });

    let strict_artist = nonzero_limit(context.max_tracks_per_artist);
    let strict_album = nonzero_limit(context.max_tracks_per_album);
    let mut selected = Vec::with_capacity(request.target_count.min(ranked.len()));
    let mut selected_ids = HashSet::new();
    let mut artist_counts = HashMap::new();
    let mut album_counts = HashMap::new();

    if request.kind == RecommendationKind::DailyMix {
        let familiar = (request.target_count / 10).max(usize::from(request.target_count > 0));
        let explore = (request.target_count * 15 + 50) / 100;
        let related = request
            .target_count
            .saturating_sub(familiar.saturating_add(explore));
        let quotas = [
            (MixBucket::Familiar, familiar),
            (MixBucket::Related, related),
            (MixBucket::Explore, explore),
        ];
        // Give every bucket a strict-diversity opportunity before relaxing.
        // Recent tracks are never used to satisfy a quota; the generic fallback
        // below only admits them when all fresh candidates are exhausted.
        for relax_diversity in [false, true] {
            for (bucket, quota) in quotas {
                fill_selection(
                    &ranked,
                    request.target_count,
                    false,
                    if relax_diversity { None } else { strict_artist },
                    if relax_diversity { None } else { strict_album },
                    Some(bucket),
                    Some(quota),
                    &mut selected,
                    &mut selected_ids,
                    &mut artist_counts,
                    &mut album_counts,
                );
            }
        }
    }

    for (allow_recent, artist_limit, album_limit) in [
        (false, strict_artist, strict_album),
        (false, None, None),
        (true, strict_artist, strict_album),
        (true, None, None),
    ] {
        fill_selection(
            &ranked,
            request.target_count,
            allow_recent,
            artist_limit,
            album_limit,
            None,
            None,
            &mut selected,
            &mut selected_ids,
            &mut artist_counts,
            &mut album_counts,
        );
        if selected.len() == request.target_count {
            break;
        }
    }

    selected
        .into_iter()
        .map(|candidate| RecommendedTrack {
            song: candidate.song.clone(),
            reason: candidate.reason,
            score: candidate.score,
            origin: recommendation_origin(request.kind),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn fill_selection<'a>(
    ranked: &'a [RankedCandidate],
    target_count: usize,
    allow_recent: bool,
    artist_limit: Option<usize>,
    album_limit: Option<usize>,
    bucket_filter: Option<MixBucket>,
    bucket_target: Option<usize>,
    selected: &mut Vec<&'a RankedCandidate>,
    selected_ids: &mut HashSet<String>,
    artist_counts: &mut HashMap<String, usize>,
    album_counts: &mut HashMap<String, usize>,
) {
    for candidate in ranked {
        if selected.len() == target_count
            || bucket_target.is_some_and(|target| {
                selected
                    .iter()
                    .filter(|selected| Some(selected.bucket) == bucket_filter)
                    .count()
                    >= target
            })
        {
            return;
        }
        if bucket_filter.is_some_and(|bucket| candidate.bucket != bucket) {
            continue;
        }
        if selected_ids.contains(&candidate.song.id) || candidate.recent && !allow_recent {
            continue;
        }
        let artist = candidate
            .song
            .artist_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                (!candidate.song.artist.trim().is_empty()).then_some(candidate.song.artist.as_str())
            });
        let album = candidate
            .song
            .album_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                (!candidate.song.album.trim().is_empty()).then_some(candidate.song.album.as_str())
            });
        if artist_limit.is_some_and(|limit| {
            artist.is_some_and(|key| artist_counts.get(key).copied().unwrap_or_default() >= limit)
        }) || album_limit.is_some_and(|limit| {
            album.is_some_and(|key| album_counts.get(key).copied().unwrap_or_default() >= limit)
        }) {
            continue;
        }

        selected_ids.insert(candidate.song.id.clone());
        if let Some(key) = artist {
            *artist_counts.entry(key.to_string()).or_default() += 1;
        }
        if let Some(key) = album {
            *album_counts.entry(key.to_string()).or_default() += 1;
        }
        selected.push(candidate);
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

fn song_artist_key(song: &Song) -> Option<String> {
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

fn song_genre_key(song: &Song) -> Option<String> {
    song.genre
        .as_deref()
        .map(str::trim)
        .filter(|genre| !genre.is_empty())
        .map(str::to_lowercase)
}

fn nonzero_limit(limit: usize) -> Option<usize> {
    (limit > 0).then_some(limit)
}

fn source_weight(kind: RecommendationKind, source: CandidateSource) -> u16 {
    match (kind, source) {
        (_, CandidateSource::Favorite) => 90,
        (RecommendationKind::SongRadio, CandidateSource::Similar) => 100,
        (RecommendationKind::ArtistRadio, CandidateSource::ArtistTop) => 100,
        (_, CandidateSource::Similar) => 75,
        (_, CandidateSource::Frequent) => 65,
        (_, CandidateSource::ArtistTop) => 60,
        (RecommendationKind::ArtistRadio, CandidateSource::SameArtist) => 80,
        (_, CandidateSource::SameArtist) => 50,
        (_, CandidateSource::SameGenre) => 40,
        (_, CandidateSource::Newest) => 25,
        (_, CandidateSource::Random) => 15,
    }
}

fn recommendation_origin(kind: RecommendationKind) -> QueueOrigin {
    match kind {
        RecommendationKind::DailyMix => QueueOrigin::DailyMix,
        RecommendationKind::SongRadio => QueueOrigin::SongRadio,
        RecommendationKind::ArtistRadio => QueueOrigin::ArtistRadio,
        RecommendationKind::AutoContinue => QueueOrigin::AutoContinue,
    }
}

fn source_bucket(source: CandidateSource) -> MixBucket {
    match source {
        CandidateSource::Favorite | CandidateSource::Frequent => MixBucket::Familiar,
        CandidateSource::Similar
        | CandidateSource::ArtistTop
        | CandidateSource::SameArtist
        | CandidateSource::SameGenre => MixBucket::Related,
        CandidateSource::Newest | CandidateSource::Random => MixBucket::Explore,
    }
}

fn source_reason(source: CandidateSource) -> RecommendationReason {
    match source {
        CandidateSource::Favorite => RecommendationReason::Favorite,
        CandidateSource::Frequent => RecommendationReason::FrequentlyPlayed,
        CandidateSource::Similar => RecommendationReason::SimilarTrack,
        CandidateSource::ArtistTop => RecommendationReason::PopularByArtist,
        CandidateSource::SameArtist => RecommendationReason::SameArtist,
        CandidateSource::SameGenre => RecommendationReason::SameGenre,
        CandidateSource::Newest => RecommendationReason::NewRelease,
        CandidateSource::Random => RecommendationReason::Explore,
    }
}

fn stable_jitter(seed: u64, song_id: &str) -> f32 {
    let mut hasher = Md5::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(song_id.as_bytes());
    let digest = hasher.finalize();
    let value = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]);
    value as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, artist: &str, album: &str) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            artist: artist.into(),
            artist_id: Some(format!("artist-{artist}")),
            album: album.into(),
            album_id: Some(format!("album-{album}")),
            ..Song::default()
        }
    }

    fn candidate(song: Song, source: CandidateSource) -> RecommendationCandidate {
        RecommendationCandidate { song, source }
    }

    fn request(target_count: usize, seed: u64) -> RecommendationRequest {
        RecommendationRequest {
            kind: RecommendationKind::DailyMix,
            target_count,
            seed,
        }
    }

    #[test]
    fn excludes_queue_ids_and_merges_duplicate_candidate_sources() {
        let duplicate = song("a", "one", "first");
        let result = recommend(
            &request(10, 7),
            vec![
                candidate(duplicate.clone(), CandidateSource::Random),
                candidate(duplicate, CandidateSource::Favorite),
                candidate(song("blocked", "two", "second"), CandidateSource::Favorite),
                candidate(song("", "none", "none"), CandidateSource::Favorite),
            ],
            &RecommendationContext {
                excluded_song_ids: HashSet::from(["blocked".into()]),
                ..RecommendationContext::with_daily_mix_defaults()
            },
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].song.id, "a");
        assert_eq!(result[0].reason, RecommendationReason::Favorite);
        assert!(result[0].score > 90.0);
    }

    #[test]
    fn applies_artist_and_album_diversity_before_relaxing() {
        let candidates = vec![
            candidate(song("a1", "a", "x"), CandidateSource::Favorite),
            candidate(song("a2", "a", "x"), CandidateSource::Favorite),
            candidate(song("a3", "a", "x"), CandidateSource::Favorite),
            candidate(song("b1", "b", "y"), CandidateSource::Random),
            candidate(song("c1", "c", "z"), CandidateSource::Random),
        ];
        let result = recommend(
            &request(3, 3),
            candidates,
            &RecommendationContext {
                max_tracks_per_artist: 1,
                max_tracks_per_album: 1,
                ..RecommendationContext::default()
            },
        );
        assert_eq!(result.len(), 3);
        assert_eq!(
            result
                .iter()
                .map(|track| track.song.artist.as_str())
                .collect::<HashSet<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn fills_small_pools_by_relaxing_without_duplicates() {
        let result = recommend(
            &request(4, 11),
            (0..4)
                .map(|index| {
                    candidate(
                        song(&format!("s{index}"), "same", "same"),
                        CandidateSource::Random,
                    )
                })
                .collect(),
            &RecommendationContext {
                max_tracks_per_artist: 1,
                max_tracks_per_album: 1,
                ..RecommendationContext::default()
            },
        );
        assert_eq!(result.len(), 4);
        assert_eq!(
            result
                .iter()
                .map(|track| track.song.id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn recent_tracks_are_deferred_until_needed() {
        let context = RecommendationContext {
            recent_song_ids: HashSet::from(["recent".into()]),
            ..RecommendationContext::with_daily_mix_defaults()
        };
        let candidates = vec![
            candidate(song("recent", "a", "a"), CandidateSource::Favorite),
            candidate(song("fresh", "b", "b"), CandidateSource::Random),
        ];
        let one = recommend(&request(1, 9), candidates.clone(), &context);
        assert_eq!(one[0].song.id, "fresh");
        let two = recommend(&request(2, 9), candidates, &context);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].song.id, "fresh");
    }

    #[test]
    fn daily_mix_reserves_anchor_related_and_in_style_explore_capacity() {
        let mut candidates = Vec::new();
        for index in 0..10 {
            candidates.push(candidate(
                song(
                    &format!("f{index}"),
                    &format!("fa{index}"),
                    &format!("fx{index}"),
                ),
                CandidateSource::Favorite,
            ));
            candidates.push(candidate(
                song(
                    &format!("s{index}"),
                    &format!("sa{index}"),
                    &format!("sx{index}"),
                ),
                CandidateSource::Similar,
            ));
            candidates.push(candidate(
                song(
                    &format!("e{index}"),
                    &format!("ea{index}"),
                    &format!("ex{index}"),
                ),
                CandidateSource::Random,
            ));
        }
        let result = recommend(
            &request(10, 5),
            candidates,
            &RecommendationContext::with_daily_mix_defaults(),
        );
        let familiar = result
            .iter()
            .filter(|track| track.reason == RecommendationReason::Favorite)
            .count();
        let related = result
            .iter()
            .filter(|track| track.reason == RecommendationReason::SimilarTrack)
            .count();
        let explore = result
            .iter()
            .filter(|track| track.reason == RecommendationReason::Explore)
            .count();
        assert_eq!((familiar, related, explore), (1, 7, 2));
    }

    #[test]
    fn listening_profile_distinguishes_completed_engaged_and_skipped_tracks() {
        let mut history = ListeningHistory::default();
        let mut completed = song("completed", "favorite", "one");
        completed.duration = Some(200);
        completed.genre = Some("Pop".into());
        history.record_started(completed.clone(), 1);
        assert!(history.update_latest("completed", 180, true));

        let mut engaged = song("engaged", "engaged", "two");
        engaged.duration = Some(100);
        history.record_started(engaged.clone(), 2);
        assert!(history.update_latest("engaged", 50, false));

        let mut skipped = song("skipped", "skipped", "three");
        skipped.duration = Some(240);
        history.record_started(skipped.clone(), 3);
        assert!(history.update_latest("skipped", 8, false));

        let profile = ListeningProfile::from_history(&history);
        assert!(profile.score(&completed) > profile.score(&engaged));
        assert!(profile.score(&engaged) > profile.score(&skipped));
        assert!(profile.score(&skipped) < 0.0);

        let related_artist = song("new", "favorite", "new-album");
        let unrelated = song("other", "other", "other-album");
        assert!(profile.score(&related_artist) > profile.score(&unrelated));
    }

    #[test]
    fn listening_profile_changes_order_inside_the_same_candidate_source() {
        let preferred = song("preferred", "preferred", "a");
        let neutral = song("neutral", "neutral", "b");
        let mut history = ListeningHistory::default();
        history.record_started(preferred.clone(), 1);
        assert!(history.update_latest("preferred", 200, true));
        let result = recommend(
            &request(2, 10),
            vec![
                candidate(neutral, CandidateSource::Random),
                candidate(preferred, CandidateSource::Random),
            ],
            &RecommendationContext {
                listening_profile: ListeningProfile::from_history(&history),
                ..RecommendationContext::with_daily_mix_defaults()
            },
        );
        assert_eq!(result[0].song.id, "preferred");
    }

    #[test]
    fn ranking_is_stable_for_the_same_seed_and_varies_for_another_variant() {
        let candidates = (0..20)
            .map(|index| {
                candidate(
                    song(
                        &format!("s{index}"),
                        &format!("a{index}"),
                        &format!("x{index}"),
                    ),
                    CandidateSource::Random,
                )
            })
            .collect::<Vec<_>>();
        let ids = |seed| {
            recommend(
                &request(8, seed),
                candidates.clone(),
                &RecommendationContext::with_daily_mix_defaults(),
            )
            .into_iter()
            .map(|track| track.song.id)
            .collect::<Vec<_>>()
        };
        assert_eq!(ids(42), ids(42));
        assert_ne!(ids(42), ids(43));
    }
}
