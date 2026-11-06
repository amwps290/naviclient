//! Ordered, bounded album requests with all observed candidate sources.

use std::collections::{BTreeSet, HashMap};

use super::super::CandidateSource;
use crate::models::Album;

pub(super) const MAX_ALBUM_REQUESTS: usize = 10;

#[derive(Debug)]
pub(super) struct AlbumRequest {
    pub(super) id: String,
    pub(super) sources: BTreeSet<CandidateSource>,
}

pub(super) fn merge_album_plan<const N: usize>(
    groups: [(Vec<Album>, CandidateSource); N],
) -> Vec<AlbumRequest> {
    let mut plan = Vec::<AlbumRequest>::new();
    let mut indices = HashMap::<String, usize>::new();
    for (albums, source) in groups {
        for album in albums {
            if album.id.trim().is_empty() {
                continue;
            }
            if let Some(&index) = indices.get(&album.id) {
                plan[index].sources.insert(source);
            } else if plan.len() < MAX_ALBUM_REQUESTS {
                indices.insert(album.id.clone(), plan.len());
                plan.push(AlbumRequest {
                    id: album.id,
                    sources: BTreeSet::from([source]),
                });
            }
        }
    }
    plan
}
