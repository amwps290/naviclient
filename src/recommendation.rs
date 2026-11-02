//! Pure recommendation ranking shared by daily mixes, seed radios and future
//! auto-continue. Providers collect factual candidates; this module owns
//! deterministic scoring, de-duplication and diversity constraints.

use std::collections::{BTreeSet, HashMap, HashSet};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::models::Song;

mod daily;
pub(crate) use daily::{current_day_key, generate_daily_mix, DailyMixSnapshot};

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
pub struct RecommendationContext {
    /// Current queue entries are always excluded; automatic results may never
    /// duplicate or overwrite the user's explicitly arranged Up Next.
    pub excluded_song_ids: HashSet<String>,
    /// Recent tracks are deferred until non-recent candidates are exhausted.
    pub recent_song_ids: HashSet<String>,
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
            let score =
                strongest_score + supporting_score + stable_jitter(request.seed, &song.id) * 0.99;
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
        let familiar = request.target_count * 2 / 5;
        let related = request.target_count * 2 / 5;
        let explore = request.target_count.saturating_sub(familiar + related);
        let quotas = [
            (MixBucket::Familiar, familiar),
            (MixBucket::Related, related),
            (MixBucket::Explore, explore),
        ];
        // Give every bucket a strict-diversity opportunity before any bucket
        // may relax. Recent tracks remain behind all fresh strict candidates.
        for (allow_recent, relax_diversity) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            for (bucket, quota) in quotas {
                fill_selection(
                    &ranked,
                    request.target_count,
                    allow_recent,
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
    fn daily_mix_reserves_familiar_related_and_explore_capacity() {
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
        assert_eq!((familiar, related, explore), (4, 4, 2));
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
