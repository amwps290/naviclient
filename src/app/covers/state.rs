//! Cover data, in-flight request ownership, and bounded failure bookkeeping.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use gpui_kit::{Hsla, Image as GpuiImage};

use super::artwork::cover_request_needed;
use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::msg::DecodedCover;

const MAX_FAILED_COVERS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoverRequest {
    request: LibraryRequest,
    id: String,
    want_palette: bool,
}

impl CoverRequest {
    pub(in crate::app) fn id(&self) -> &str {
        &self.id
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
    entries: HashMap<String, CoverEntry>,
    pending: HashMap<String, PendingCover>,
    failed: HashSet<String>,
    failed_order: VecDeque<String>,
}

#[derive(Default)]
pub(in crate::app) struct CoverUpdate {
    pub(in crate::app) replaced: Option<Arc<GpuiImage>>,
    pub(in crate::app) follow_up_palette: bool,
}

impl CoverCache {
    pub(in crate::app) fn image(&self, id: &str) -> Option<&Arc<GpuiImage>> {
        self.entries.get(id)?.image.as_ref()
    }

    pub(in crate::app) fn palette(&self, id: &str) -> Option<(Hsla, Hsla)> {
        self.entries.get(id)?.palette
    }

    pub(in crate::app) fn images(&self) -> impl Iterator<Item = &Arc<GpuiImage>> {
        self.entries
            .values()
            .filter_map(|entry| entry.image.as_ref())
    }

    pub(in crate::app) fn request_needed(&self, id: &str, want_palette: bool) -> bool {
        if self.failed.contains(id) {
            return false;
        }
        let entry = self.entries.get(id);
        cover_request_needed(
            entry.is_some_and(|entry| entry.image.is_some()),
            entry.is_some_and(|entry| entry.palette.is_some()),
            want_palette && !entry.is_some_and(|entry| entry.palette_attempted),
        )
    }

    pub(in crate::app) fn begin(
        &mut self,
        id: &str,
        want_palette: bool,
        requests: &mut LibraryRequests,
    ) -> Option<CoverRequest> {
        if !self.request_needed(id, want_palette) {
            return None;
        }
        if let Some(pending) = self.pending.get_mut(id) {
            // A feature/playing view can request color while a grid image is
            // still loading. Preserve that demand for the completion handler.
            pending.want_palette |= want_palette;
            return None;
        }
        let request = CoverRequest {
            request: requests.next(),
            id: id.to_string(),
            want_palette,
        };
        self.pending.insert(
            id.to_string(),
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
            .get(request.id())
            .map(|pending| &pending.request)
            != Some(request)
        {
            return None;
        }
        let pending = self
            .pending
            .remove(request.id())
            .expect("matching cover request exists");
        let decoded = match result {
            Ok(decoded) if decoded.image.is_some() || decoded.palette.is_some() => decoded,
            _ => {
                self.remember_failed(request.id.clone());
                return Some(CoverUpdate::default());
            }
        };
        let entry = self.entries.entry(request.id.clone()).or_default();
        entry.palette_attempted |= request.want_palette || decoded.palette.is_some();
        if let Some(palette) = decoded.palette {
            entry.palette = Some(palette);
        }
        let replaced = decoded.image.and_then(|image| {
            let image_id = image.id();
            entry
                .image
                .replace(image)
                .filter(|previous| previous.id() != image_id)
        });
        Some(CoverUpdate {
            replaced,
            follow_up_palette: pending.want_palette
                && entry.image.is_some()
                && entry.palette.is_none()
                && !entry.palette_attempted,
        })
    }

    fn remember_failed(&mut self, id: String) {
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
