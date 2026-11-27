//! Streaming, audio output, normalization and lyrics preferences.

use super::components::{settings_group, settings_row};
use crate::app::playback::display::format_file_size;
use crate::app::prelude::*;
use gpui_kit::component::menu::PopupMenuItem;

const AUDIO_CACHE_LIMIT_OPTIONS_MB: [u64; 5] = [512, 1024, 2048, 4096, 8192];

fn format_cache_limit_label(mb: u64) -> String {
    if mb >= 1024 && mb.is_multiple_of(1024) {
        format!("{} GB", mb / 1024)
    } else {
        format!("{mb} MB")
    }
}

impl NavidromeApp {
    pub(in crate::app) fn render_playback_settings(
        &self,
        cx: &Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        vec![
            self.render_streaming_settings(cx),
            self.render_output_settings(cx),
            self.render_normalization_settings(cx),
        ]
    }

    fn render_streaming_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let cache_dir = config::audio_cache_dir(&self.config);

        let cache_row = v_flex()
            .w_full()
            .px_4()
            .py_3p5()
            .gap_2p5()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text)
                    .child(t!("Audio cache folder")),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_1p5()
                            .rounded(px(tokens::radius::CONTROL))
                            .bg(p.sunken)
                            .border_1()
                            .border_color(if dark {
                                hsla(0.0, 0.0, 1.0, 0.06)
                            } else {
                                p.border.opacity(0.75)
                            })
                            .truncate()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(p.text_secondary)
                            .child(cache_dir.display().to_string()),
                    )
                    .child(
                        surface_button("open-cache-directory")
                            .label(t!("Open folder"))
                            .compact_action()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let dir = config::audio_cache_dir(&this.config);
                                if let Err(error) = config::prepare_audio_cache_dir(&this.config) {
                                    this.notify_error(t!(
                                        "Unable to use cache directory %{dir}: %{error}",
                                        dir = dir.display(),
                                        error = error
                                    ));
                                    return;
                                }
                                cx.open_with_system(&dir);
                            })),
                    )
                    .child(
                        surface_button("choose-cache-directory")
                            .label(t!("Choose folder"))
                            .compact_action()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choose_cache_directory(cx);
                            })),
                    )
                    .child(
                        ghost_button("reset-cache-directory")
                            .label(t!("Use default"))
                            .compact_action()
                            .disabled(self.config.cache_dir.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_cache_directory(None);
                                this.refresh_cache_usage(cx);
                            })),
                    ),
            )
            .child({
                let usage_text = match self.state.settings.cache_usage {
                    Some(usage) => t!(
                        "Audio %{audio} (%{audio_count}) · Covers %{covers} (%{cover_count})",
                        audio = format_file_size(usage.audio_bytes),
                        audio_count = usage.audio_files,
                        covers = format_file_size(usage.cover_bytes),
                        cover_count = usage.cover_files
                    )
                    .to_string(),
                    None => t!("Cache usage not scanned yet").to_string(),
                };
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(usage_text),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                ghost_button("refresh-cache-usage")
                                    .label(t!("Scan"))
                                    .compact_action()
                                    .disabled(self.state.settings.cache_busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_cache_usage(cx);
                                    })),
                            )
                            .child(
                                surface_button("clear-audio-cache")
                                    .label(t!("Clear audio cache"))
                                    .compact_action()
                                    .disabled(self.state.settings.cache_busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_audio_cache_action(cx);
                                    })),
                            )
                            .child(
                                surface_button("clear-cover-cache")
                                    .label(t!("Clear cover cache"))
                                    .compact_action()
                                    .disabled(self.state.settings.cache_busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_cover_cache_action(cx);
                                    })),
                            ),
                    )
            })
            .into_any_element();

        let cache_limit_row = {
            let app = cx.entity().downgrade();
            let current_mb = self.config.audio_cache_limit_mb;
            let picker = surface_button("audio-cache-limit-select")
                .w(px(140.0))
                .justify_between()
                .label(format_cache_limit_label(current_mb))
                .dropdown_caret(true)
                .compact_action()
                .dropdown_menu(move |menu, _, cx| {
                    app.update(cx, |this, cx| {
                        let mut menu = menu;
                        for mb in AUDIO_CACHE_LIMIT_OPTIONS_MB {
                            menu = menu.item(
                                PopupMenuItem::new(format_cache_limit_label(mb))
                                    .checked(this.config.audio_cache_limit_mb == mb)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_audio_cache_limit_mb(mb, cx);
                                    })),
                            );
                        }
                        menu
                    })
                    .expect("app owns cache limit menu")
                })
                .into_any_element();
            settings_row(t!("Audio cache size limit"), None, picker, cx)
        };

        let quality_row = {
            let app = cx.entity().downgrade();
            let picker = surface_button("streaming-quality-select")
                .w(px(220.0))
                .justify_between()
                .label(self.config.transcoding_quality.label())
                .dropdown_caret(true)
                .compact_action()
                .dropdown_menu(move |menu, _, cx| {
                    app.update(cx, |this, cx| {
                        let mut menu = menu;
                        for quality in TranscodingQuality::ALL {
                            menu = menu.item(
                                PopupMenuItem::new(quality.label())
                                    .checked(this.config.transcoding_quality == quality)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_transcoding_quality(quality, cx);
                                    })),
                            );
                        }
                        menu
                    })
                    .expect("app owns streaming quality menu")
                })
                .into_any_element();
            settings_row(t!("Streaming quality"), None, picker, cx)
        };

        settings_group(
            Some(t!("Streaming & cache").into()),
            None,
            vec![quality_row, cache_limit_row, cache_row],
            cx,
        )
    }

    fn render_output_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let playback = self.audio.state();
        let active = match playback.output_name.as_deref() {
            Some(name) => name.to_string(),
            None => t!("System default").to_string(),
        };
        let active_summary: SharedString = t!(
            "Active: %{active} · %{rate} Hz · %{channels} ch",
            active = active,
            rate = playback.output_sample_rate,
            channels = playback.output_channels
        )
        .into();

        let app = cx.entity().downgrade();
        let selected_label = self
            .config
            .output_device
            .clone()
            .unwrap_or_else(|| t!("System default").to_string());

        let device_controls = h_flex()
            .gap_1p5()
            .items_center()
            .child(
                surface_button("output-device-select")
                    .w(px(240.0))
                    .max_w_full()
                    .justify_between()
                    .label(selected_label.clone())
                    .tooltip(selected_label)
                    .dropdown_caret(true)
                    .compact_action()
                    .dropdown_menu(move |menu, _, cx| {
                        app.update(cx, |this, cx| {
                            let mut menu = menu.item(
                                PopupMenuItem::new(t!("System default"))
                                    .checked(this.config.output_device.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_output_device(None, cx);
                                    })),
                            );
                            for device in this.state.output_devices.items() {
                                let name = &device.name;
                                let name_for_click = name.clone();
                                let label = if device.is_default {
                                    t!("%{name} (default)", name = name)
                                } else {
                                    t!("%{name}", name = name)
                                };
                                menu = menu.item(
                                    PopupMenuItem::new(label)
                                        .checked(
                                            this.config.output_device.as_deref()
                                                == Some(name.as_str()),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_output_device(
                                                Some(name_for_click.clone()),
                                                cx,
                                            );
                                        })),
                                );
                            }
                            menu
                        })
                        .expect("app owns output device menu")
                    }),
            )
            .child(
                icon_button(
                    "refresh-output-devices",
                    AppIcon::Refresh,
                    t!("Refresh devices"),
                    tokens::ICON_BUTTON_SM,
                )
                .loading(self.state.output_devices.loading())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.refresh_output_devices(cx);
                })),
            )
            .into_any_element();

        let status_dot = if playback.output_connected {
            p.online
        } else {
            p.text_muted
        };
        let output_row = h_flex()
            .w_full()
            .px_4()
            .py_3p5()
            .justify_between()
            .items_center()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .child(t!("Audio output")),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .size(px(7.0))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(status_dot),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(p.text_secondary)
                                    .child(active_summary),
                            ),
                    ),
            )
            .child(device_controls)
            .into_any_element();

        let disconnect_row = settings_row(
            t!("Pause when output device disconnects"),
            Some(
                t!("Keep playback paused after switching back from a disconnected headset or DAC.")
                    .into(),
            ),
            Switch::new("pause-on-output-disconnect")
                .checked(self.config.pause_on_output_disconnect)
                .accessibility_label(t!("Pause when output device disconnects"))
                .on_change(cx.listener(|this, checked, _, cx| {
                    this.set_pause_on_output_disconnect(*checked, cx);
                }))
                .into_any_element(),
            cx,
        );

        settings_group(
            Some(t!("Audio output").into()),
            None,
            vec![output_row, disconnect_row],
            cx,
        )
    }

    fn render_normalization_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let picker = surface_button("volume-normalization-select")
            .w(px(220.0))
            .justify_between()
            .label(self.config.volume_normalization.label())
            .tooltip(self.config.volume_normalization.tooltip())
            .dropdown_caret(true)
            .compact_action()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for mode in VolumeNormalization::ALL {
                        menu = menu.item(
                            PopupMenuItem::new(mode.label())
                                .checked(this.config.volume_normalization == mode)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_volume_normalization(mode, cx);
                                })),
                        );
                    }
                    menu
                })
                .expect("app owns volume normalization menu")
            })
            .into_any_element();

        let row = settings_row(
            t!("Volume normalization"),
            Some(
                t!("Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level.")
                    .into(),
            ),
            picker,
            cx,
        );

        settings_group(None, None, vec![row], cx)
    }
}
