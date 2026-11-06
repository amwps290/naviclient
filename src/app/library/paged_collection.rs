//! Sequential paging with request identity and mutually exclusive load states.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageRequest {
    id: u64,
    page: usize,
}

impl PageRequest {
    pub(in crate::app) fn page(self) -> usize {
        self.page
    }
}

/// Keep this source outside resettable view state so old server responses cannot
/// match a request issued after reconnecting.
#[derive(Default)]
pub(in crate::app) struct PageRequests {
    last_id: u64,
}

impl PageRequests {
    fn next(&mut self, page: usize) -> PageRequest {
        self.last_id = self
            .last_id
            .checked_add(1)
            .expect("page request ID exhausted");
        PageRequest {
            id: self.last_id,
            page,
        }
    }
}

#[derive(Default)]
enum PagePhase {
    #[default]
    Idle,
    Loading(PageRequest),
    Failed,
    Exhausted,
}

pub(in crate::app) struct PagedCollection<T> {
    items: Vec<T>,
    next_page: usize,
    phase: PagePhase,
}

impl<T> Default for PagedCollection<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_page: 0,
            phase: PagePhase::Idle,
        }
    }
}

impl<T> PagedCollection<T> {
    pub(in crate::app) fn items(&self) -> &[T] {
        &self.items
    }

    pub(in crate::app) fn len(&self) -> usize {
        self.items.len()
    }

    pub(in crate::app) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        matches!(self.phase, PagePhase::Loading(_))
    }

    pub(in crate::app) fn failed(&self) -> bool {
        matches!(self.phase, PagePhase::Failed)
    }

    pub(in crate::app) fn exhausted(&self) -> bool {
        matches!(self.phase, PagePhase::Exhausted)
    }

    pub(in crate::app) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Failures retry the same page. Starting a retry also clears its error state.
    pub(in crate::app) fn begin(&mut self, requests: &mut PageRequests) -> Option<PageRequest> {
        if self.loading() || self.exhausted() {
            return None;
        }
        let request = requests.next(self.next_page);
        self.phase = PagePhase::Loading(request);
        Some(request)
    }

    pub(in crate::app) fn accepts(&self, request: PageRequest) -> bool {
        matches!(self.phase, PagePhase::Loading(current) if current == request)
    }

    /// Ignore stale or duplicate responses without changing any state. Return
    /// only newly appended items so callers can prefetch that page's artwork.
    pub(in crate::app) fn complete<E>(
        &mut self,
        request: PageRequest,
        result: Result<Vec<T>, E>,
    ) -> Option<Result<&[T], E>> {
        if !self.accepts(request) {
            return None;
        }
        Some(match result {
            Ok(items) => {
                let start = self.items.len();
                // A short page can still have a successor; only an empty page
                // proves exhaustion, matching the existing server behavior.
                self.phase = if items.is_empty() {
                    PagePhase::Exhausted
                } else {
                    self.next_page += 1;
                    PagePhase::Idle
                };
                self.items.extend(items);
                Ok(&self.items[start..])
            }
            Err(error) => {
                self.phase = PagePhase::Failed;
                Err(error)
            }
        })
    }
}

#[cfg(test)]
mod tests;
