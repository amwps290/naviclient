//! Multi-pass selection with shared diversity, uniqueness and bucket counts.

use std::collections::{HashMap, HashSet};

use super::ranking::{MixBucket, RankedCandidate};
use super::{RecommendationContext, RecommendationKind, RecommendationRequest};

#[derive(Clone, Copy)]
struct SelectionPass {
    allow_recent: bool,
    artist_limit: Option<usize>,
    album_limit: Option<usize>,
    bucket: Option<(MixBucket, usize)>,
}

impl SelectionPass {
    fn strict(context: &RecommendationContext) -> Self {
        Self {
            allow_recent: false,
            artist_limit: nonzero_limit(context.max_tracks_per_artist),
            album_limit: nonzero_limit(context.max_tracks_per_album),
            bucket: None,
        }
    }

    fn relaxed(self) -> Self {
        Self {
            artist_limit: None,
            album_limit: None,
            ..self
        }
    }
}

/// Every accepted track updates all selection constraints together. Keys borrow
/// candidate metadata, and bucket counts avoid rescanning the selected tracks.
struct Selection<'a> {
    ranked: &'a [RankedCandidate],
    target_count: usize,
    tracks: Vec<&'a RankedCandidate>,
    song_ids: HashSet<&'a str>,
    artist_counts: HashMap<&'a str, usize>,
    album_counts: HashMap<&'a str, usize>,
    bucket_counts: [usize; 3],
}

impl<'a> Selection<'a> {
    fn new(ranked: &'a [RankedCandidate], target_count: usize) -> Self {
        Self {
            ranked,
            target_count,
            tracks: Vec::with_capacity(target_count.min(ranked.len())),
            song_ids: HashSet::new(),
            artist_counts: HashMap::new(),
            album_counts: HashMap::new(),
            bucket_counts: [0; 3],
        }
    }

    fn full(&self) -> bool {
        self.tracks.len() == self.target_count
    }

    fn fill(&mut self, pass: SelectionPass) {
        for candidate in self.ranked {
            if self.full()
                || pass.bucket.is_some_and(|(bucket, target)| {
                    self.bucket_counts[bucket_index(bucket)] >= target
                })
            {
                return;
            }
            if pass
                .bucket
                .is_some_and(|(bucket, _)| candidate.bucket != bucket)
                || self.song_ids.contains(candidate.song.id.as_str())
                || candidate.recent && !pass.allow_recent
            {
                continue;
            }
            let artist = candidate
                .song
                .artist_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .or_else(|| {
                    (!candidate.song.artist.trim().is_empty())
                        .then_some(candidate.song.artist.as_str())
                });
            let album = candidate
                .song
                .album_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .or_else(|| {
                    (!candidate.song.album.trim().is_empty())
                        .then_some(candidate.song.album.as_str())
                });
            if limit_reached(pass.artist_limit, artist, &self.artist_counts)
                || limit_reached(pass.album_limit, album, &self.album_counts)
            {
                continue;
            }

            self.song_ids.insert(candidate.song.id.as_str());
            if let Some(key) = artist {
                *self.artist_counts.entry(key).or_default() += 1;
            }
            if let Some(key) = album {
                *self.album_counts.entry(key).or_default() += 1;
            }
            self.bucket_counts[bucket_index(candidate.bucket)] += 1;
            self.tracks.push(candidate);
        }
    }
}

pub(super) fn select<'a>(
    request: &RecommendationRequest,
    ranked: &'a [RankedCandidate],
    context: &RecommendationContext,
) -> Vec<&'a RankedCandidate> {
    let strict = SelectionPass::strict(context);
    let mut selection = Selection::new(ranked, request.target_count);

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
        // Every bucket gets a strict-diversity opportunity before relaxing.
        // Quotas admit only fresh tracks; recent tracks remain a fallback.
        for pass in [strict, strict.relaxed()] {
            for bucket in quotas {
                selection.fill(SelectionPass {
                    bucket: Some(bucket),
                    ..pass
                });
            }
        }
    }

    for pass in [
        strict,
        strict.relaxed(),
        SelectionPass {
            allow_recent: true,
            ..strict
        },
        SelectionPass {
            allow_recent: true,
            ..strict.relaxed()
        },
    ] {
        selection.fill(pass);
        if selection.full() {
            break;
        }
    }
    selection.tracks
}

fn nonzero_limit(limit: usize) -> Option<usize> {
    (limit > 0).then_some(limit)
}

fn bucket_index(bucket: MixBucket) -> usize {
    match bucket {
        MixBucket::Familiar => 0,
        MixBucket::Related => 1,
        MixBucket::Explore => 2,
    }
}

fn limit_reached(limit: Option<usize>, key: Option<&str>, counts: &HashMap<&str, usize>) -> bool {
    limit.is_some_and(|limit| {
        key.is_some_and(|key| counts.get(key).copied().unwrap_or_default() >= limit)
    })
}
