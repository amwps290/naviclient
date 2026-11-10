//! Latest reachability test per saved proxy.

use std::collections::HashMap;
use std::time::Duration;

pub(in crate::app) use crate::source::route_probe::RouteProbeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ProbeState {
    Testing,
    Reachable(Duration),
    Failed(RouteProbeError),
}

/// Each proxy keeps only its newest test; editing it invalidates the result.
#[derive(Default)]
pub(in crate::app) struct ProxyProbes {
    states: HashMap<String, (u64, ProbeState)>,
    next: u64,
}

impl ProxyProbes {
    pub(in crate::app) fn begin(&mut self, proxy: &str) -> u64 {
        self.next = self.next.wrapping_add(1);
        self.states
            .insert(proxy.to_string(), (self.next, ProbeState::Testing));
        self.next
    }

    pub(in crate::app) fn complete(
        &mut self,
        proxy: &str,
        request: u64,
        result: Result<Duration, RouteProbeError>,
    ) -> bool {
        let Some((current, state)) = self.states.get_mut(proxy) else {
            return false;
        };
        if *current != request || *state != ProbeState::Testing {
            return false;
        }
        *state = match result {
            Ok(latency) => ProbeState::Reachable(latency),
            Err(error) => ProbeState::Failed(error),
        };
        true
    }

    pub(in crate::app) fn get(&self, proxy: &str) -> Option<&ProbeState> {
        self.states.get(proxy).map(|(_, state)| state)
    }

    pub(in crate::app) fn forget(&mut self, proxy: &str) {
        self.states.remove(proxy);
    }
}

#[cfg(test)]
mod tests;
