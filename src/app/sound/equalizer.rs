//! Equalizer audio updates, slider synchronization, and deferred config saves.

use super::equalizer_state::{EqualizerSaveRequest, EQUALIZER_SAVE_DELAY};
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn set_eq_band(
        &mut self,
        band: usize,
        gain_db: f32,
        cx: &mut Context<Self>,
    ) {
        if let Some(request) = self
            .equalizer
            .set_band(&mut self.config.equalizer, band, gain_db)
        {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(in crate::app) fn set_eq_preamp(&mut self, preamp_db: f32, cx: &mut Context<Self>) {
        if let Some(request) = self
            .equalizer
            .set_preamp(&mut self.config.equalizer, preamp_db)
        {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(in crate::app) fn set_eq_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Some(request) = self
            .equalizer
            .set_enabled(&mut self.config.equalizer, enabled)
        {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(in crate::app) fn set_eq_preset(
        &mut self,
        gains: [f32; 10],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_eq_preset_with_preamp(0.0, gains, window, cx);
    }

    pub(in crate::app) fn set_eq_preset_with_preamp(
        &mut self,
        preamp_db: f32,
        gains: [f32; 10],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request =
            self.equalizer
                .set_preset_with_preamp(&mut self.config.equalizer, preamp_db, gains);
        let preamp = self.config.equalizer.preamp_db;
        self.eq_preamp_slider
            .update(cx, |slider, cx| slider.set_value(preamp, window, cx));
        for (slider, gain) in self.eq_sliders.iter().zip(self.config.equalizer.gains_db) {
            slider.update(cx, |slider, cx| slider.set_value(gain, window, cx));
        }
        if let Some(request) = request {
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(in crate::app) fn save_eq_custom_preset(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.eq_preset_name_input.read(cx).value().to_string();
        if let Some(request) = self
            .equalizer
            .save_custom_preset(&mut self.config.equalizer, &name)
        {
            self.eq_preset_name_input
                .update(cx, |input, cx| input.set_value(String::new(), window, cx));
            self.apply_equalizer_change(request, cx);
        }
    }

    pub(in crate::app) fn delete_eq_custom_preset(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(request) = self
            .equalizer
            .delete_custom_preset(&mut self.config.equalizer, name)
        {
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
