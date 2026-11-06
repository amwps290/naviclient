//! Equalizer settings and revision publication behind one private shared lock.

use std::sync::{Arc, Mutex};

use crate::models::EqSettings;

struct EqShared {
    version: u64,
    settings: EqSettings,
}

#[derive(Clone)]
pub(crate) struct EqHandle {
    inner: Arc<Mutex<EqShared>>,
}

pub(crate) fn new_eq_handle(settings: EqSettings) -> EqHandle {
    EqHandle {
        inner: Arc::new(Mutex::new(EqShared {
            version: 0,
            settings,
        })),
    }
}

impl EqHandle {
    /// Publish the requested settings and their revision under the same lock.
    /// Retain the worker's existing behavior: poisoned state ignores edits.
    pub(crate) fn set(&self, settings: EqSettings) {
        if let Ok(mut shared) = self.inner.lock() {
            shared.settings = settings;
            shared.version = shared.version.wrapping_add(1);
        }
    }

    /// Initial filter construction retains its existing poisoned-lock failure.
    pub(crate) fn snapshot(&self) -> (u64, EqSettings) {
        let shared = self.inner.lock().unwrap();
        (shared.version, shared.settings.clone())
    }

    /// An unchanged or poisoned state keeps the filter's current coefficients.
    /// Clone settings only after observing a different revision; release the
    /// shared lock before any filter coefficients are recalculated.
    pub(super) fn changed_since(&self, version: u64) -> Option<(u64, EqSettings)> {
        let shared = self.inner.lock().ok()?;
        (shared.version != version).then(|| (shared.version, shared.settings.clone()))
    }

    #[cfg(test)]
    pub(crate) fn shares_state(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    #[cfg(test)]
    pub(crate) fn set_version_for_test(&self, version: u64) {
        self.inner.lock().unwrap().version = version;
    }
}

#[cfg(test)]
mod tests;
