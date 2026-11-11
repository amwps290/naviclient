//! Library request identities and mutually exclusive loading phases.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LibraryRequest(u64);

/// This counter survives resets of AppState, including server reconnections.
#[derive(Default)]
pub(in crate::app) struct LibraryRequests {
    last_id: u64,
}

impl LibraryRequests {
    pub(in crate::app) fn next(&mut self) -> LibraryRequest {
        self.last_id = self
            .last_id
            .checked_add(1)
            .expect("library request ID exhausted");
        LibraryRequest(self.last_id)
    }
}

#[derive(Default)]
enum LoadPhase {
    #[default]
    Idle,
    Loading(LibraryRequest),
    Loaded,
    Failed {
        error: String,
    },
}

/// Loading, failure, and request identity form one state. Data stays with its
/// owner, which decides whether an accepted failure preserves or clears it.
#[derive(Default)]
pub(in crate::app) struct LibraryLoad {
    phase: LoadPhase,
}

impl LibraryLoad {
    pub(in crate::app) fn pending(&self, request: LibraryRequest) -> bool {
        matches!(self.phase, LoadPhase::Loading(current) if current == request)
    }
    pub(in crate::app) fn loading(&self) -> bool {
        matches!(self.phase, LoadPhase::Loading(_))
    }

    pub(in crate::app) fn error(&self) -> Option<&str> {
        match &self.phase {
            LoadPhase::Failed { error } => Some(error),
            _ => None,
        }
    }

    pub(in crate::app) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        let request = requests.next();
        self.phase = LoadPhase::Loading(request);
        request
    }

    /// Stale and duplicate results do not alter the current load or its error.
    pub(in crate::app) fn complete<T>(
        &mut self,
        request: LibraryRequest,
        result: Result<T, String>,
    ) -> Option<Result<T, &str>> {
        if !matches!(self.phase, LoadPhase::Loading(current) if current == request) {
            return None;
        }
        Some(match result {
            Ok(value) => {
                self.phase = LoadPhase::Loaded;
                Ok(value)
            }
            Err(error) => {
                self.phase = LoadPhase::Failed { error };
                Err(self.error().expect("failed load stores its error"))
            }
        })
    }
}

#[cfg(test)]
mod tests;
