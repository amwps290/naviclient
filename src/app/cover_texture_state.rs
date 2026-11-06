//! Decoded cover retention and image handles waiting for paired texture/asset release.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use gpui_kit::Image;

pub(super) struct CoverTextures {
    default_image_id: u64,
    capacity: usize,
    used: VecDeque<Arc<Image>>,
    evicted: HashMap<u64, Arc<Image>>,
    retired: HashMap<u64, Arc<Image>>,
}

impl CoverTextures {
    pub(super) fn new(default_image_id: u64, capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            default_image_id,
            capacity,
            used: VecDeque::new(),
            evicted: HashMap::new(),
            retired: HashMap::new(),
        }
    }

    pub(super) fn mark_used(&mut self, image: &Arc<Image>) {
        let id = image.id();
        // Stale views cannot revive a retired source before its release is applied.
        if id == self.default_image_id || self.retired.contains_key(&id) {
            return;
        }
        // Reusing an evicted image before the UI flush cancels that eviction.
        self.evicted.remove(&id);
        self.used.retain(|image| image.id() != id);
        self.used.push_front(image.clone());
        if self.used.len() > self.capacity {
            let oldest = self.used.pop_back().expect("nonempty retained image queue");
            self.evicted.insert(oldest.id(), oldest);
        }
    }

    /// Replaced source images must leave the retention budget immediately, while
    /// their handles stay alive until the window can release the decoded texture.
    pub(super) fn retire(&mut self, image: Arc<Image>) {
        let id = image.id();
        if id == self.default_image_id {
            return;
        }
        self.used.retain(|image| image.id() != id);
        self.evicted.remove(&id);
        self.retired.insert(id, image);
    }

    pub(super) fn reset(&mut self, source_images: impl IntoIterator<Item = Arc<Image>>) {
        let used = std::mem::take(&mut self.used);
        let evicted = std::mem::take(&mut self.evicted);
        for image in source_images
            .into_iter()
            .chain(used)
            .chain(evicted.into_values())
        {
            self.retire(image);
        }
    }

    /// Each image appears once per flush. Owning the handles avoids looking up
    /// an evicted image in a source cache that may already have been replaced.
    pub(super) fn take_releases(&mut self) -> Vec<Arc<Image>> {
        let mut releases = std::mem::take(&mut self.retired);
        releases.extend(std::mem::take(&mut self.evicted));
        releases.into_values().collect()
    }
}

#[cfg(test)]
mod tests;
