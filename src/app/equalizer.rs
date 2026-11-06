//! Equalizer audio updates, slider synchronization, and deferred config saves.

use super::equalizer_state::{EqualizerSaveRequest, EQUALIZER_SAVE_DELAY};
use super::*;

impl NavidromeApp {
    pub(super) fn set_eq_band(&mut self, band: usize, gain_db: f32, cx: &mut Context<Self>) {
        if let Some(request) = self
            .equalizer
            .set_band(&mut self.config.equalizer, band, gain_db)
        {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(super) fn set_eq_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Some(request) = self
            .equalizer
            .set_enabled(&mut self.config.equalizer, enabled)
        {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(super) fn set_eq_preset(
        &mut self,
        gains: [f32; 10],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = self.equalizer.set_preset(&mut self.config.equalizer, gains);
        for (slider, gain) in self.eq_sliders.iter().zip(self.config.equalizer.gains_db) {
            slider.update(cx, |slider, cx| slider.set_value(gain, window, cx));
        }
        if let Some(request) = request {
            self.apply_equalizer_change(request, cx);
        }
    }

    fn apply_equalizer_change(&mut self, request: EqualizerSaveRequest, cx: &mut Context<Self>) {
        self.audio.set_equalizer(self.config.equalizer.clone());
        cx.spawn(async move |this, cx| {
            Timer::after(EQUALIZER_SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                if !this.equalizer.complete_save(request) {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(t!(
                        "Equalizer save failed: %{error}",
                        error = format!("{error:#}")
                    ));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
        cx.notify();
    }
}
