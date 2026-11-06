//! Volume controls and scheduling of persistence, hover panels, and mini feedback.

use super::volume_state::{
    normalize_volume, VolumePanelCloseRequest, MINI_VOLUME_FLASH_CLEAR_DELAY,
    VOLUME_PANEL_CLOSE_DELAY, VOLUME_SAVE_DELAY,
};
use super::*;

impl NavidromeApp {
    /// Returns whether a vertical scroll was consumed, allowing the mini player
    /// to flash its volume only for the events that adjust it.
    pub(super) fn adjust_volume_from_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
        if delta_y.abs() < f32::EPSILON {
            return false;
        }
        let delta = if delta_y > 0.0 { 0.05 } else { -0.05 };
        self.adjust_volume(delta, window, cx);
        cx.stop_propagation();
        true
    }

    pub(super) fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = self.volume.set_volume(&mut self.config.volume, volume);
        self.audio.set_volume(volume);
        self.schedule_volume_save(cx);
        cx.notify();
    }

    pub(super) fn adjust_volume(
        &mut self,
        delta: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let volume = normalize_volume(self.config.volume + delta);
        self.volume_slider.update(cx, |slider, cx| {
            slider.set_value(volume * 100.0, window, cx);
        });
        self.set_volume(volume, cx);
    }

    pub(super) fn toggle_mute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let volume = self.volume.toggle_mute(&mut self.config.volume);
        self.audio.set_volume(volume);
        if !self.volume.muted() {
            self.volume_slider.update(cx, |slider, cx| {
                slider.set_value(volume * 100.0, window, cx);
            });
            self.schedule_volume_save(cx);
        }
        cx.notify();
    }

    fn schedule_volume_save(&mut self, cx: &mut Context<Self>) {
        let request = self.volume.begin_save();
        cx.spawn(async move |this, cx| {
            Timer::after(VOLUME_SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                if !this.volume.complete_save(request) {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(t!(
                        "Volume save failed: %{error}",
                        error = format!("{error:#}")
                    ));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// Repaint after the flash expires even when paused and no periodic redraw runs.
    pub(super) fn show_mini_volume_flash(&mut self, cx: &mut Context<Self>) {
        let expiry = self.volume.show_mini_flash(std::time::Instant::now());
        cx.spawn(async move |this, cx| {
            Timer::after(MINI_VOLUME_FLASH_CLEAR_DELAY).await;
            this.update(cx, |this, cx| {
                if this.volume.expire_mini_flash(expiry) {
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(super) fn show_volume_panel(&mut self, cx: &mut Context<Self>) {
        self.volume.show_panel();
        cx.notify();
    }

    pub(super) fn schedule_volume_panel_close(&mut self, cx: &mut Context<Self>) {
        let request = self.volume.request_panel_close();
        self.close_volume_panel_after(request, cx);
    }

    fn close_volume_panel_after(
        &mut self,
        request: VolumePanelCloseRequest,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            Timer::after(VOLUME_PANEL_CLOSE_DELAY).await;
            this.update(cx, |this, cx| {
                if this.volume.complete_panel_close(request) {
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(super) fn begin_volume_panel_drag(&mut self, cx: &mut Context<Self>) {
        self.volume.begin_panel_drag();
        cx.notify();
    }

    pub(super) fn end_volume_panel_drag(&mut self, keep_open: bool, cx: &mut Context<Self>) {
        if let Some(request) = self.volume.end_panel_drag(keep_open) {
            self.close_volume_panel_after(request, cx);
        } else {
            cx.notify();
        }
    }
}
