//! Streaming, audio output, normalization and lyrics preferences.

use crate::app::prelude::*;
use gpui_kit::component::menu::PopupMenuItem;

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
        let cache_dir = config::audio_cache_dir(&self.config);
        v_flex()
            .gap_3()
            .child(section_title(t!("Streaming & cache"), cx))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("Audio cache folder")),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_sm()
                                            .child(cache_dir.display().to_string()),
                                    )
                                    .child(
                                        Button::new("choose-cache-directory")
                                            .label(t!("Choose folder"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.choose_cache_directory(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("reset-cache-directory")
                                            .label(t!("Use default"))
                                            .disabled(self.config.cache_dir.is_none())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.apply_cache_directory(None);
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(div().text_sm().child(t!("Streaming quality")))
                            .child({
                                let app = cx.entity().downgrade();
                                Button::new("streaming-quality-select")
                                    .w(px(220.0))
                                    .justify_between()
                                    .label(self.config.transcoding_quality.label())
                                    .dropdown_caret(true)
                                    .outline()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |this, cx| {
                                            let mut menu = menu;
                                            for quality in TranscodingQuality::ALL {
                                                menu = menu.item(
                                                    PopupMenuItem::new(quality.label())
                                                        .checked(
                                                            this.config.transcoding_quality
                                                                == quality,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_transcoding_quality(
                                                                    quality, cx,
                                                                );
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns streaming quality menu")
                                    })
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_output_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(section_title(t!("Audio output"), cx))
            .child(
                v_flex()
                    .gap_4()
                    .child({
                        let playback = self.audio.state();
                        let active = match playback.output_name.as_deref() {
                            Some(name) => name.to_string(),
                            None => t!("System default").to_string(),
                        };
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "Active: %{active} · %{rate} Hz · %{channels} ch",
                                active = active,
                                rate = playback.output_sample_rate,
                                channels = playback.output_channels
                            ))
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .child({
                                let app = cx.entity().downgrade();
                                let selected_label = self
                                    .config
                                    .output_device
                                    .clone()
                                    .unwrap_or_else(|| t!("System default").to_string());
                                Button::new("output-device-select")
                                    .w(px(300.0))
                                    .max_w_full()
                                    .justify_between()
                                    .label(selected_label.clone())
                                    .tooltip(selected_label)
                                    .dropdown_caret(true)
                                    .outline()
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
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_output_device(
                                                                    Some(name_for_click.clone()),
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns output device menu")
                                    })
                            })
                            .child(
                                Button::new("refresh-output-devices")
                                    .icon(AppIcon::Refresh)
                                    .tooltip(t!("Refresh devices"))
                                    .ghost()
                                    .small()
                                    .loading(self.state.output_devices.loading())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_output_devices(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_normalization_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(section_title(t!("Volume normalization"), cx))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        t!("Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level."
                                        ),
                                    ),
                            ),
                    )
                    .child({
                        let app = cx.entity().downgrade();
                        Button::new("volume-normalization-select")
                            .w(px(220.0))
                            .justify_between()
                            .label(self.config.volume_normalization.label())
                            .tooltip(self.config.volume_normalization.tooltip())
                            .dropdown_caret(true)
                            .outline()
                            .dropdown_menu(move |menu, _, cx| {
                                app.update(cx, |this, cx| {
                                    let mut menu = menu;
                                    for mode in VolumeNormalization::ALL {
                                        menu = menu.item(
                                            PopupMenuItem::new(mode.label())
                                                .checked(
                                                    this.config.volume_normalization
                                                        == mode,
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        this.set_volume_normalization(
                                                            mode, cx,
                                                        );
                                                    },
                                                )),
                                        );
                                    }
                                    menu
                                })
                                .expect("app owns volume normalization menu")
                            })
                    }),
            )
            .into_any_element()
    }
}
