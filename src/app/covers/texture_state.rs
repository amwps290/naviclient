//! Decoded cover retention and image handles waiting for paired texture/asset release.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use gpui_kit::Image;

pub(in crate::app) struct CoverTextures {
    default_image_id: u64,
    capacity: usize,
    used: VecDeque<Arc<Image>>,
    evicted: HashMap<u64, Arc<Image>>,
    retired: HashMap<u64, Arc<Image>>,
}

impl CoverTextures {
    pub(in crate::app) fn new(default_image_id: u64, capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            default_image_id,
            capacity,
            used: VecDeque::new(),
            evicted: HashMap::new(),
            retired: HashMap::new(),
        }
    }

    pub(in crate::app) fn stats(&self) -> (usize, usize) {
        (self.used.len(), self.capacity)
    }

    pub(in crate::app) fn evict_all_except(
        &mut self,
        protected_ids: &std::collections::HashSet<u64>,
    ) {
        let mut kept = VecDeque::with_capacity(self.used.len());
        while let Some(image) = self.used.pop_front() {
            let id = image.id();
            if protected_ids.contains(&id) {
                kept.push_back(image);
            } else {
                self.evicted.insert(id, image);
            }
        }
        self.used = kept;
    }

    pub(in crate::app) fn mark_used(&mut self, image: &Arc<Image>) {
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
    pub(in crate::app) fn retire(&mut self, image: Arc<Image>) {
        let id = image.id();
        if id == self.default_image_id {
            return;
        }
        self.used.retain(|image| image.id() != id);
        self.evicted.remove(&id);
        self.retired.insert(id, image);
    }

    #[cfg(test)]
    pub(in crate::app) fn reset(&mut self, source_images: impl IntoIterator<Item = Arc<Image>>) {
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
    pub(in crate::app) fn take_releases(&mut self) -> Vec<Arc<Image>> {
        let mut releases = std::mem::take(&mut self.retired);
        releases.extend(std::mem::take(&mut self.evicted));
        releases.into_values().collect()
    }
}

#[cfg(test)]
mod tests;
