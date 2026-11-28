//! Cover data, in-flight request ownership, and bounded failure bookkeeping.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use gpui_kit::{Hsla, Image as GpuiImage};

use super::artwork::cover_request_needed;
use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::msg::DecodedCover;
use crate::source::{MediaKey, SourceId};

const MAX_FAILED_COVERS: usize = 256;
pub(in crate::app) const MAX_COVER_ENTRIES: usize = 256;
pub(in crate::app) const MAX_COVER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoverRequest {
    request: LibraryRequest,
    key: MediaKey,
    want_palette: bool,
}

impl CoverRequest {
    pub(in crate::app) fn source(&self) -> &SourceId {
        &self.key.source
    }

    pub(in crate::app) fn id(&self) -> &str {
        &self.key.remote_id
    }
}

#[derive(Default)]
struct CoverEntry {
    image: Option<Arc<GpuiImage>>,
    palette: Option<(Hsla, Hsla)>,
    /// A recognized image can fail palette extraction. Do not automatically
    /// retry that extraction forever from the same cached bytes.
    palette_attempted: bool,
}

struct PendingCover {
    request: CoverRequest,
    want_palette: bool,
}

#[derive(Default)]
pub(in crate::app) struct CoverCache {
    entries: HashMap<MediaKey, CoverEntry>,
    order: VecDeque<MediaKey>,
    image_bytes: usize,
    pending: HashMap<MediaKey, PendingCover>,
    failed: HashSet<MediaKey>,
    failed_order: VecDeque<MediaKey>,
}

#[derive(Default)]
pub(in crate::app) struct CoverUpdate {
    pub(in crate::app) replaced: Option<Arc<GpuiImage>>,
    pub(in crate::app) evicted: Vec<Arc<GpuiImage>>,
    pub(in crate::app) follow_up_palette: bool,
}

impl CoverCache {
    pub(in crate::app) fn stats(&self) -> (usize, usize) {
        let images = self
            .entries
            .values()
            .filter(|entry| entry.image.is_some())
            .count();
        (images, self.image_bytes)
    }

    pub(in crate::app) fn touch(&mut self, source: &SourceId, id: &str) {
        let key = MediaKey::new(source.clone(), id);
        if self.entries.contains_key(&key) {
            self.order.retain(|existing| existing != &key);
            self.order.push_front(key);
        }
    }

    pub(in crate::app) fn retain_sources(
        &mut self,
        sources: &HashSet<SourceId>,
    ) -> Vec<Arc<GpuiImage>> {
        let retained: HashSet<_> = self
            .entries
            .iter()
            .filter(|(key, _)| sources.contains(&key.source))
            .filter_map(|(_, entry)| entry.image.as_ref().map(|image| image.id()))
            .collect();
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| !sources.contains(&key.source))
            .filter_map(|(_, entry)| entry.image.clone())
            .filter(|image| !retained.contains(&image.id()))
            .collect();
        self.entries.retain(|key, _| sources.contains(&key.source));
        self.order.retain(|key| sources.contains(&key.source));
        self.recompute_bytes();
        self.pending.retain(|key, _| sources.contains(&key.source));
        self.failed.retain(|key| sources.contains(&key.source));
        self.failed_order
            .retain(|key| sources.contains(&key.source));
        removed
    }

    /// Evict cached cover entries not in `protected`, returning detached images
    /// whose GPUI textures/assets should be released.
    pub(in crate::app) fn clear_except(
        &mut self,
        protected: &HashSet<MediaKey>,
    ) -> Vec<Arc<GpuiImage>> {
        let retained_ids: HashSet<_> = self
            .entries
            .iter()
            .filter(|(key, _)| protected.contains(*key))
            .filter_map(|(_, entry)| entry.image.as_ref().map(|image| image.id()))
            .collect();
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| !protected.contains(*key))
            .filter_map(|(_, entry)| entry.image.clone())
            .filter(|image| !retained_ids.contains(&image.id()))
            .collect();
        self.entries.retain(|key, _| protected.contains(key));
        self.order.retain(|key| protected.contains(key));
        self.recompute_bytes();
        removed
    }

    fn recompute_bytes(&mut self) {
        self.image_bytes = self
            .entries
            .values()
            .filter_map(|entry| entry.image.as_ref())
            .map(|image| image.bytes.len())
            .sum();
    }

    fn evict_over_budget(&mut self, protected: &MediaKey) -> Vec<Arc<GpuiImage>> {
        self.evict_to(MAX_COVER_ENTRIES, MAX_COVER_BYTES, protected)
    }

    fn evict_to(
        &mut self,
        max_entries: usize,
        max_bytes: usize,
        protected: &MediaKey,
    ) -> Vec<Arc<GpuiImage>> {
        let mut evicted = Vec::new();
        while self.entries.len() > max_entries || self.image_bytes > max_bytes {
            let Some(victim_pos) = self
                .order
                .iter()
                .rposition(|key| key != protected && self.entries.contains_key(key))
            else {
                break;
            };
            let victim = self.order.remove(victim_pos).expect("valid victim index");
            if let Some(entry) = self.entries.remove(&victim) {
                if let Some(image) = entry.image {
                    self.image_bytes = self.image_bytes.saturating_sub(image.bytes.len());
                    let id = image.id();
                    let still_shared = self
                        .entries
                        .values()
                        .any(|other| other.image.as_ref().is_some_and(|img| img.id() == id));
                    if !still_shared {
                        evicted.push(image);
                    }
                }
            }
        }
        evicted
    }

    pub(in crate::app) fn image(&self, source: &SourceId, id: &str) -> Option<&Arc<GpuiImage>> {
        self.entries
            .get(&MediaKey::new(source.clone(), id))?
            .image
            .as_ref()
    }

    pub(in crate::app) fn palette(&self, source: &SourceId, id: &str) -> Option<(Hsla, Hsla)> {
        self.entries
            .get(&MediaKey::new(source.clone(), id))?
            .palette
    }

    #[cfg(test)]
    pub(in crate::app) fn images(&self) -> impl Iterator<Item = &Arc<GpuiImage>> {
        self.entries
            .values()
            .filter_map(|entry| entry.image.as_ref())
    }

    pub(in crate::app) fn request_needed(
        &self,
        source: &SourceId,
        id: &str,
        want_palette: bool,
    ) -> bool {
        let key = MediaKey::new(source.clone(), id);
        if self.failed.contains(&key) {
            return false;
        }
        let entry = self.entries.get(&key);
        cover_request_needed(
            entry.is_some_and(|entry| entry.image.is_some()),
            entry.is_some_and(|entry| entry.palette.is_some()),
            want_palette && !entry.is_some_and(|entry| entry.palette_attempted),
        )
    }

    pub(in crate::app) fn begin(
        &mut self,
        source: &SourceId,
        id: &str,
        want_palette: bool,
        requests: &mut LibraryRequests,
    ) -> Option<CoverRequest> {
        if !self.request_needed(source, id, want_palette) {
            return None;
        }
        let key = MediaKey::new(source.clone(), id);
        if let Some(pending) = self.pending.get_mut(&key) {
            // A feature/playing view can request color while a grid image is
            // still loading. Preserve that demand for the completion handler.
            pending.want_palette |= want_palette;
            return None;
        }
        let request = CoverRequest {
            request: requests.next(),
            key: key.clone(),
            want_palette,
        };
        self.pending.insert(
            key,
            PendingCover {
                request: request.clone(),
                want_palette,
            },
        );
        Some(request)
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: &CoverRequest,
        result: Result<DecodedCover, String>,
    ) -> Option<CoverUpdate> {
        if self
            .pending
            .get(&request.key)
            .map(|pending| &pending.request)
            != Some(request)
        {
            return None;
        }
        let pending = self
            .pending
            .remove(&request.key)
            .expect("matching cover request exists");
        let decoded = match result {
            Ok(decoded) if decoded.image.is_some() || decoded.palette.is_some() => decoded,
            _ => {
                self.remember_failed(request.key.clone());
                return Some(CoverUpdate::default());
            }
        };
        let entry = self.entries.entry(request.key.clone()).or_default();
        entry.palette_attempted |= request.want_palette || decoded.palette.is_some();
        if let Some(palette) = decoded.palette {
            entry.palette = Some(palette);
        }
        let mut previous_bytes = 0usize;
        let mut new_bytes = 0usize;
        let replaced = decoded.image.and_then(|image| {
            let image_id = image.id();
            new_bytes = image.bytes.len();
            let old = entry.image.replace(image);
            if let Some(prev) = &old {
                previous_bytes = prev.bytes.len();
            }
            old.filter(|previous| previous.id() != image_id)
        });
        if new_bytes > 0 || previous_bytes > 0 {
            self.image_bytes = self
                .image_bytes
                .saturating_sub(previous_bytes)
                .saturating_add(new_bytes);
        }
        let follow_up_palette = pending.want_palette
            && entry.image.is_some()
            && entry.palette.is_none()
            && !entry.palette_attempted;
        self.order.retain(|existing| existing != &request.key);
        self.order.push_front(request.key.clone());
        let evicted = self.evict_over_budget(&request.key);
        Some(CoverUpdate {
            replaced,
            evicted,
            follow_up_palette,
        })
    }

    fn remember_failed(&mut self, id: MediaKey) {
        if self.failed.insert(id.clone()) {
            self.failed_order.push_back(id);
        }
        while self.failed.len() > MAX_FAILED_COVERS {
            if let Some(oldest) = self.failed_order.pop_front() {
                self.failed.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests;
