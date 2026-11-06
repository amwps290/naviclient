//! Repeated matched keystrokes share one clock; selection movement is exempt.

use std::time::{Duration, Instant};

use gpui_kit::Keystroke;

use crate::models::ShortcutAction;

const REPEAT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(in crate::app) struct ShortcutRepeatGuard {
    last: Option<(String, Instant)>,
}

impl ShortcutRepeatGuard {
    /// Call only after recording, focus guards, and binding matching. Every
    /// matched event updates the clock, even when suppressed or exempted.
    pub(in crate::app) fn allows(
        &mut self,
        keystroke: &Keystroke,
        action: ShortcutAction,
        now: Instant,
    ) -> bool {
        let pressed = keystroke.unparse();
        let repeated = self.last.as_ref().is_some_and(|(previous, at)| {
            *previous == pressed && now.duration_since(*at) < REPEAT_INTERVAL
        });
        self.last = Some((pressed, now));
        !repeated
            || matches!(
                action,
                ShortcutAction::SelectPrevious | ShortcutAction::SelectNext
            )
    }
}

#[cfg(test)]
mod tests;
