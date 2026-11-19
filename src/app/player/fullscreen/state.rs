//! Fullscreen intent, asynchronous native transitions, and presentation controls.

use std::time::{Duration, Instant};

const IDLE_DELAY: Duration = Duration::from_millis(3200);
const FADE_DURATION: Duration = Duration::from_millis(280);
const NATIVE_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct NativeFullscreen {
    desired: Option<bool>,
    pending: Option<(bool, Instant)>,
}

impl NativeFullscreen {
    fn baseline(&self, actual: bool) -> bool {
        self.desired.unwrap_or(actual)
    }

    fn request(&mut self, target: bool, actual: bool, now: Instant) -> bool {
        self.desired = Some(target);
        self.poll(actual, now)
    }

    fn poll(&mut self, actual: bool, now: Instant) -> bool {
        if self.pending.is_some_and(|(target, sent)| {
            actual == target || now.saturating_duration_since(sent) >= NATIVE_RETRY_DELAY
        }) {
            self.pending = None;
        }
        if self.pending.is_some() {
            return false;
        }
        if let Some(target) = self.desired.filter(|target| *target != actual) {
            self.pending = Some((target, now));
            true
        } else {
            self.desired = None;
            false
        }
    }
}

struct Session {
    restore_native: bool,
    native_seen: bool,
    last_activity: Instant,
    last_fade: Instant,
    manually_hidden: bool,
    opacity: f32,
}

pub(super) struct FullscreenState {
    native: NativeFullscreen,
    session: Option<Session>,
    pub(super) visualizer_enabled: bool,
}

impl Default for FullscreenState {
    fn default() -> Self {
        Self {
            native: NativeFullscreen::default(),
            session: None,
            visualizer_enabled: true,
        }
    }
}

pub(super) struct FullscreenUpdate {
    pub(super) toggle_native: bool,
    pub(super) exited: bool,
    pub(super) changed: bool,
}

impl FullscreenState {
    pub(super) fn active(&self) -> bool {
        self.session.is_some()
    }

    pub(super) fn enter(&mut self, native: bool, now: Instant) -> bool {
        if self.active() {
            return false;
        }
        self.session = Some(Session {
            restore_native: self.native.baseline(native),
            native_seen: native,
            last_activity: now,
            last_fade: now,
            manually_hidden: false,
            opacity: 1.0,
        });
        self.native.request(true, native, now)
    }

    pub(super) fn exit(&mut self, native: bool, now: Instant) -> bool {
        let Some(session) = self.session.take() else {
            return false;
        };
        self.native.request(session.restore_native, native, now)
    }

    pub(super) fn wake(&mut self, now: Instant) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        session.last_activity = now;
        session.last_fade = now;
        let changed = !session.manually_hidden && session.opacity != 1.0;
        if !session.manually_hidden {
            session.opacity = 1.0;
        }
        changed
    }

    pub(super) fn toggle_controls(&mut self, now: Instant) {
        if let Some(session) = &mut self.session {
            session.manually_hidden = !session.manually_hidden;
        }
        self.wake(now);
    }

    pub(super) fn opacity(&self) -> f32 {
        self.session.as_ref().map_or(0.0, |session| session.opacity)
    }

    pub(super) fn poll(
        &mut self,
        native: bool,
        now: Instant,
        keep_controls: bool,
    ) -> FullscreenUpdate {
        // Native shortcuts and compositor controls can exit fullscreen without
        // going through our buttons. Respect that request once entry is observed.
        let exited = self
            .session
            .as_ref()
            .is_some_and(|session| session.native_seen && !native && self.native.desired.is_none());
        if exited {
            self.session = None;
        }
        let toggle_native = self.native.poll(native, now);
        let mut changed = exited;
        if let Some(session) = &mut self.session {
            session.native_seen |= native;
            let hide = session.manually_hidden
                || !keep_controls
                    && now.saturating_duration_since(session.last_activity) >= IDLE_DELAY;
            let step = (now
                .saturating_duration_since(session.last_fade)
                .as_secs_f32()
                / FADE_DURATION.as_secs_f32())
            .clamp(0.0, 1.0);
            session.last_fade = now;
            let next = if hide {
                (session.opacity - step).max(0.0)
            } else {
                (session.opacity + step).min(1.0)
            };
            changed |= (next - session.opacity).abs() > 0.001;
            session.opacity = next;
        }
        FullscreenUpdate {
            toggle_native,
            exited,
            changed,
        }
    }
}

#[cfg(test)]
mod tests;
