//! One pending removal target, independent of active-server and settings resets.

use crate::models::Config;

struct RemovalTarget {
    id: String,
    label: String,
}

#[derive(Default)]
pub(in crate::app) struct ServerRemovalState {
    target: Option<RemovalTarget>,
}

impl ServerRemovalState {
    /// Opening another row must not change the target of an existing dialog.
    pub(in crate::app) fn begin(&mut self, config: &Config, id: &str) -> bool {
        if self.target.is_some() {
            return false;
        }
        let Some(profile) = config.servers.iter().find(|profile| profile.id == id) else {
            return false;
        };
        self.target = Some(RemovalTarget {
            id: profile.id.clone(),
            label: profile.label(),
        });
        true
    }

    pub(in crate::app) fn label(&self) -> Option<&str> {
        self.target.as_ref().map(|target| target.label.as_str())
    }

    /// Submission consumes the original ID once. Profile removal revalidates
    /// that ID against the current config before any persistence or cleanup.
    pub(in crate::app) fn take_target(&mut self) -> Option<String> {
        self.target.take().map(|target| target.id)
    }

    pub(in crate::app) fn cancel(&mut self) {
        self.target = None;
    }
}

#[cfg(test)]
mod tests;
