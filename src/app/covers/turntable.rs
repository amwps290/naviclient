//! Visual track identity and the outgoing artwork retained during a record exchange.

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::Image;

use crate::app::prelude::*;
use crate::models::source::MediaKey;
use crate::vinyl::motion::{tonearm_target, TurntableMotion};

#[derive(Default)]
pub(in crate::app) struct Turntable {
    track: Option<MediaKey>,
    pub cover: Option<Arc<Image>>,
    pub outgoing_cover: Option<Arc<Image>>,
    pub outgoing_phase: f32,
    pub motion: TurntableMotion,
    visible: bool,
}

impl NavidromeApp {
    pub(in crate::app) fn sync_turntable(
        &mut self,
        cover: Arc<Image>,
        playback: &PlaybackState,
        window: &mut Window,
    ) {
        let visible = self.turntable_visible(window);
        self.tick_turntable(Instant::now(), playback, visible, window);
        let track = self.state.now_playing.as_ref().and_then(Song::media_key);
        if track != self.turntable.track
            && track.is_some()
            && self.turntable.track.is_some()
            && visible
            && self.turntable.visible
            && self.turntable.motion.begin_exchange()
        {
            self.turntable.outgoing_cover = self.turntable.cover.clone();
            self.turntable.outgoing_phase = self.vinyl_cover.phase();
            self.vinyl_cover.freeze(window);
        }
        self.turntable.track = track;
        self.turntable.cover = Some(cover);
        self.turntable.visible = visible;
    }

    pub(in crate::app) fn turntable_visible(&self, window: &Window) -> bool {
        window.is_visible()
            && self.playback_presentation()
                == crate::app::shell::redraw::PlaybackPresentation::NowPlaying
            && self.state.now_playing.is_some()
    }

    pub(in crate::app) fn tick_turntable(
        &mut self,
        now: Instant,
        playback: &PlaybackState,
        visible: bool,
        window: &mut Window,
    ) -> bool {
        let target = tonearm_target(
            playback.active && !playback.paused,
            playback.position,
            playback.duration,
        );
        let changed = self.turntable.motion.tick(now, visible, target);
        if !self.turntable.motion.frame().outgoing {
            self.turntable.outgoing_cover = None;
            self.vinyl_cover.release_frozen(window);
        }
        // A hidden poll must also prevent the next render from treating a track
        // selected while hidden as an exchange that happened on screen.
        if !visible {
            self.turntable.visible = false;
        }
        changed && visible
    }
}
