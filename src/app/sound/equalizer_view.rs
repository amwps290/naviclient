//! Equalizer popover controls, presets and band sliders.

use crate::app::prelude::*;
use gpui_kit::component::popover::Popover;

impl NavidromeApp {
    /// 底部播放条的均衡器弹出面板：锚定 EQ 按钮的 Popover，
    /// 点击外部自动关闭；面板内开关/预设/10 段滑杆与配置实时同步。
    pub(in crate::app) fn render_eq_popover(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let preamp_slider = self.eq_preamp_slider.clone();
        let sliders = self.eq_sliders.clone();
        let preset_input = self.eq_preset_name_input.clone();
        let enabled = self.config.equalizer.enabled;
        let preamp = self.config.equalizer.preamp_db;
        let gains = self.config.equalizer.gains_db;
        let custom_presets = self.config.equalizer.custom_presets.clone();

        let presets: Vec<(&'static str, [f32; 10])> = EqSettings::PRESETS.to_vec();

        Popover::new("eq-popover")
            .trigger(toggle_icon_button(
                "player-eq",
                AppIcon::Eq,
                t!("Equalizer"),
                enabled,
                tokens::ICON_BUTTON_MD,
                cx,
            ))
            .content(move |_, _, popover_cx| {
                let p = palette(popover_cx);
                let dark = popover_cx.theme().is_dark();
                let emphasis = p.brand;
                let muted = p.text_muted;
                let hairline = if dark {
                    hsla(0.0, 0.0, 1.0, 0.08)
                } else {
                    p.border.opacity(0.82)
                };
                let stage_bg = if dark {
                    hsla(0.0, 0.0, 1.0, 0.03)
                } else {
                    p.sunken.opacity(0.65)
                };
                let chip_bg = if dark {
                    hsla(0.0, 0.0, 1.0, 0.05)
                } else {
                    p.sunken.opacity(0.85)
                };
                let mono = popover_cx.theme().mono_font_family.clone();

                v_flex()
                    .w(px(468.0))
                    .gap_3()
                    .p_3p5()
                    // Header: title + status dot + Flat reset + segmented On/Off
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(7.0)).rounded_full().bg(if enabled {
                                        emphasis
                                    } else {
                                        muted.opacity(0.45)
                                    }))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text)
                                            .child(t!("Equalizer")),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child({
                                        let app = app.clone();
                                        ghost_button("eq-popover-reset")
                                            .label(t!("Flat"))
                                            .rounded_full()
                                            .h(px(28.0))
                                            .px_2p5()
                                            .text_xs()
                                            .on_click(move |_, window, cx| {
                                                let _ = app.update(cx, |this, cx| {
                                                    this.set_eq_preset_with_preamp(
                                                        0.0, [0.0; 10], window, cx,
                                                    );
                                                });
                                            })
                                    })
                                    .child(
                                        h_flex()
                                            .p_0p5()
                                            .gap_0p5()
                                            .rounded_full()
                                            .bg(stage_bg)
                                            .border_1()
                                            .border_color(hairline)
                                            .children([(true, t!("On")), (false, t!("Off"))].map(
                                                |(enable, label)| {
                                                    let app = app.clone();
                                                    let active = enabled == enable;
                                                    pill_filter_button(
                                                        SharedString::from(format!(
                                                            "eq-popover-{enable}"
                                                        )),
                                                        label,
                                                        active,
                                                        popover_cx,
                                                    )
                                                    .h(px(24.0))
                                                    .px_2p5()
                                                    .text_xs()
                                                    .on_click(move |_, _, cx| {
                                                        let _ = app.update(cx, |this, cx| {
                                                            this.set_eq_enabled(enable, cx);
                                                        });
                                                    })
                                                },
                                            )),
                                    ),
                            ),
                    )
                    // Built-in presets
                    .child(h_flex().gap_1p5().flex_wrap().children(presets.iter().map(
                        |(name, preset_gains)| {
                            let preset_gains = *preset_gains;
                            let app = app.clone();
                            let active = enabled && preamp == 0.0 && gains == preset_gains;
                            pill_filter_button(
                                SharedString::from(format!("eq-popover-preset-{name}")),
                                EqSettings::preset_label(name),
                                active,
                                popover_cx,
                            )
                            .h(px(26.0))
                            .px_2p5()
                            .text_xs()
                            .on_click(move |_, window, cx| {
                                let _ = app.update(cx, |this, cx| {
                                    this.set_eq_preset(preset_gains, window, cx);
                                });
                            })
                        },
                    )))
                    // Custom presets as unified composite chips (preset + inline delete)
                    .when(!custom_presets.is_empty(), |this| {
                        this.child(h_flex().gap_1p5().flex_wrap().children(
                            custom_presets.iter().enumerate().map(|(idx, preset)| {
                                let preset_name = preset.name.clone();
                                let delete_name = preset.name.clone();
                                let preset_preamp = preset.preamp_db;
                                let preset_gains = preset.gains_db;
                                let app_apply = app.clone();
                                let app_del = app.clone();
                                let active =
                                    enabled && preamp == preset_preamp && gains == preset_gains;
                                h_flex()
                                    .items_center()
                                    .pl_0p5()
                                    .pr_1()
                                    .rounded_full()
                                    .bg(if active { p.brand_soft } else { chip_bg })
                                    .border_1()
                                    .border_color(if active {
                                        emphasis.opacity(if dark { 0.34 } else { 0.24 })
                                    } else {
                                        hairline
                                    })
                                    .child(
                                        ghost_button(SharedString::from(format!(
                                            "eq-custom-preset-{idx}"
                                        )))
                                        .label(preset_name)
                                        .rounded_full()
                                        .h(px(24.0))
                                        .px_2()
                                        .text_xs()
                                        .text_color(if active { emphasis } else { p.text_secondary })
                                        .on_click(move |_, window, cx| {
                                            let _ = app_apply.update(cx, |this, cx| {
                                                this.set_eq_preset_with_preamp(
                                                    preset_preamp,
                                                    preset_gains,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }),
                                    )
                                    .child(
                                        icon_button(
                                            SharedString::from(format!(
                                                "eq-custom-preset-del-{idx}"
                                            )),
                                            AppIcon::Close,
                                            t!("Delete"),
                                            18.0,
                                        )
                                        .on_click(move |_, _, cx| {
                                            let _ = app_del.update(cx, |this, cx| {
                                                this.delete_eq_custom_preset(&delete_name, cx);
                                            });
                                        }),
                                    )
                            }),
                        ))
                    })
                    // Preamp + 10-band fader stage inside a recessed panel with 0 dB center guide
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .px_2p5()
                            .py_3()
                            .rounded(px(tokens::radius::COVER))
                            .bg(stage_bg)
                            .border_1()
                            .border_color(hairline)
                            .child(
                                div()
                                    .absolute()
                                    .left_3()
                                    .right_3()
                                    .top(px(78.0))
                                    .h(px(1.0))
                                    .bg(hairline.opacity(0.65)),
                            )
                            .child(
                                h_flex()
                                    .relative()
                                    .justify_between()
                                    .child(
                                        v_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .w(px(40.0))
                                            .pr_1p5()
                                            .mr_1()
                                            .border_r_1()
                                            .border_color(hairline)
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family(mono.clone())
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(if enabled {
                                                        emphasis
                                                    } else {
                                                        muted
                                                    })
                                                    .child(format!("{preamp:+.0}")),
                                            )
                                            .child(
                                                Slider::new(&preamp_slider)
                                                    .vertical()
                                                    .h(px(104.0))
                                                    .bg(if enabled { emphasis } else { muted })
                                                    .text_color(if enabled {
                                                        emphasis
                                                    } else {
                                                        muted
                                                    })
                                                    .rounded_full(),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(muted)
                                                    .child(t!("Pre")),
                                            ),
                                    )
                                    .children(sliders.iter().enumerate().map(|(band, slider)| {
                                        let center_hz = EqSettings::BANDS[band];
                                        let label = if center_hz >= 1000.0 {
                                            format!("{}k", (center_hz / 1000.0) as u32)
                                        } else {
                                            format!("{}", center_hz as u32)
                                        };
                                        let gain = gains[band];
                                        v_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .w(px(34.0))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family(mono.clone())
                                                    .font_weight(if enabled && gain.abs() >= 0.5 {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .text_color(if enabled && gain.abs() >= 0.5 {
                                                        emphasis
                                                    } else {
                                                        muted
                                                    })
                                                    .child(format!("{gain:+.0}")),
                                            )
                                            .child(
                                                Slider::new(slider)
                                                    .vertical()
                                                    .h(px(104.0))
                                                    .bg(if enabled { emphasis } else { muted })
                                                    .text_color(if enabled {
                                                        emphasis
                                                    } else {
                                                        muted
                                                    })
                                                    .rounded_full(),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_family(mono.clone())
                                                    .text_color(muted)
                                                    .child(label),
                                            )
                                    })),
                            ),
                    )
                    // Save current curve as custom preset
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .pt_1()
                            .child(Input::new(&preset_input).flex_1().small())
                            .child({
                                let app = app.clone();
                                surface_button("eq-save-custom-preset")
                                    .label(t!("Save preset"))
                                    .compact_action()
                                    .on_click(move |_, window, cx| {
                                        let _ = app.update(cx, |this, cx| {
                                            this.save_eq_custom_preset(window, cx);
                                        });
                                    })
                            }),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
}
