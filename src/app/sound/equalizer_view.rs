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
                // accent 是背景 token，做文字/进度色在亮色主题里几乎不可见。
                let emphasis = popover_cx.theme().primary;
                let muted = popover_cx.theme().muted_foreground;
                let border = popover_cx.theme().border;
                v_flex()
                    .w(px(444.0))
                    .gap_3()
                    .p_3()
                    // 开关 + Reset
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Equalizer")),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child({
                                        let app = app.clone();
                                        ghost_button("eq-popover-reset")
                                            .label(t!("Flat"))
                                            .compact_action()
                                            .on_click(move |_, window, cx| {
                                                let _ = app.update(cx, |this, cx| {
                                                    this.set_eq_preset_with_preamp(
                                                        0.0, [0.0; 10], window, cx,
                                                    );
                                                });
                                            })
                                    })
                                    .children([(true, t!("On")), (false, t!("Off"))].map(
                                        |(enable, label)| {
                                            let app = app.clone();
                                            let active = enabled == enable;
                                            pill_filter_button(
                                                SharedString::from(format!("eq-popover-{enable}")),
                                                label,
                                                active,
                                                popover_cx,
                                            )
                                            .on_click(
                                                move |_, _, cx| {
                                                    let _ = app.update(cx, |this, cx| {
                                                        this.set_eq_enabled(enable, cx);
                                                    });
                                                },
                                            )
                                        },
                                    )),
                            ),
                    )
                    // 内置预设
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
                            .on_click(move |_, window, cx| {
                                let _ = app.update(cx, |this, cx| {
                                    this.set_eq_preset(preset_gains, window, cx);
                                });
                            })
                        },
                    )))
                    // 自定义预设
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
                                    .gap_0p5()
                                    .child(
                                        pill_filter_button(
                                            SharedString::from(format!("eq-custom-preset-{idx}")),
                                            preset_name,
                                            active,
                                            popover_cx,
                                        )
                                        .on_click(
                                            move |_, window, cx| {
                                                let _ = app_apply.update(cx, |this, cx| {
                                                    this.set_eq_preset_with_preamp(
                                                        preset_preamp,
                                                        preset_gains,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            },
                                        ),
                                    )
                                    .child(
                                        icon_button(
                                            SharedString::from(format!(
                                                "eq-custom-preset-del-{idx}"
                                            )),
                                            AppIcon::Close,
                                            t!("Delete"),
                                            20.0,
                                        )
                                        .on_click(
                                            move |_, _, cx| {
                                                let _ = app_del.update(cx, |this, cx| {
                                                    this.delete_eq_custom_preset(&delete_name, cx);
                                                });
                                            },
                                        ),
                                    )
                            }),
                        ))
                    })
                    // Preamp + 10 段滑杆
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .w(px(36.0))
                                    .pr_1()
                                    .mr_1()
                                    .border_r_1()
                                    .border_color(border.opacity(0.6))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if enabled { emphasis } else { muted })
                                            .child(format!("{preamp:+.0}")),
                                    )
                                    .child(
                                        Slider::new(&preamp_slider)
                                            .vertical()
                                            .h(px(96.0))
                                            .bg(emphasis)
                                            .text_color(emphasis)
                                            .rounded_full(),
                                    )
                                    .child(div().text_xs().text_color(muted).child(t!("Pre"))),
                            )
                            .children(sliders.iter().enumerate().map(|(band, slider)| {
                                let center_hz = EqSettings::BANDS[band];
                                let label = if center_hz >= 1000.0 {
                                    format!("{}k", (center_hz / 1000.0) as u32)
                                } else {
                                    format!("{}", center_hz as u32)
                                };
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .w(px(34.0))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if enabled { emphasis } else { muted })
                                            .child(format!("{:+.0}", gains[band])),
                                    )
                                    .child(
                                        Slider::new(slider)
                                            .vertical()
                                            .h(px(96.0))
                                            .bg(emphasis)
                                            .text_color(emphasis)
                                            .rounded_full(),
                                    )
                                    .child(div().text_xs().text_color(muted).child(label))
                            })),
                    )
                    // 保存当前曲线为自定义预设
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .pt_1()
                            .border_t_1()
                            .border_color(border.opacity(0.5))
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
