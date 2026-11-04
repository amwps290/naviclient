//! 设置覆盖层、分区导航及常规设置的渲染。

use super::*;
use gpui_kit::component::menu::PopupMenuItem;

impl NavidromeApp {
    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(super) fn render_settings_overlay(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let settings_content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .vertical_scrollbar(&self.settings_scroll)
            .when(compact, |this| this.px_5().py_4())
            .when(!compact, |this| this.px_6().py_5())
            .child(self.render_settings(cx));
        let settings_body = if compact {
            v_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(true, cx))
                .child(settings_content)
                .into_any_element()
        } else {
            h_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(false, cx))
                .child(settings_content)
                .into_any_element()
        };

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude() // 遮罩挡住背后所有命中框：后方页面不再响应滚轮/悬停
            .flex()
            .justify_center()
            .items_start()
            // 固定黑色半透明背景：亮/暗主题下都是压暗效果。
            .bg(hsla(0.0, 0.0, 0.0, 0.35))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.settings_open = false;
                    this.cancel_shortcut_recording();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(960.0))
                    .h(px(650.0))
                    .max_w(relative(if compact { 0.96 } else { 0.94 }))
                    .max_h(relative(if compact { 0.94 } else { 0.88 }))
                    .mt(px(if compact { 16.0 } else { 48.0 }))
                    .bg(cx.theme().background)
                    .rounded_xl()
                    .shadow_xl()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        // 点击卡片内部不触发背景关闭。
                        cx.stop_propagation()
                    })
                    .child(
                        h_flex()
                            .flex_none()
                            .when(compact, |this| this.px_4().py_2())
                            .when(!compact, |this| this.px_5().py_3())
                            .justify_between()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Settings")),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip(t!("Close settings"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = false;
                                        this.cancel_shortcut_recording();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(settings_body),
            )
            .into_any_element()
    }

    fn settings_navigation_button(
        &self,
        id: &'static str,
        section: SettingsSection,
        label: SharedString,
        icon: AppIcon,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let selected = self.state.settings_section == section;
        let button = Button::new(id)
            .w_full()
            .h(px(36.0))
            .when(!compact, |this| this.justify_start())
            .accessibility_label(label.clone())
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .when(!compact, |this| this.justify_start())
                    .child(
                        div()
                            .w(px(20.0))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(Icon::new(icon).with_size(px(16.0))),
                    )
                    .child(div().min_w_0().text_sm().truncate().child(label)),
            )
            .ghost()
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cancel_shortcut_recording();
                this.state.settings_section = section;
                this.settings_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }));

        if compact {
            div().flex_1().min_w_0().child(button).into_any_element()
        } else {
            button.into_any_element()
        }
    }

    fn render_settings_navigation(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let buttons = vec![
            self.settings_navigation_button(
                "settings-nav-servers",
                SettingsSection::Servers,
                t!("Servers").into(),
                AppIcon::Server,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-playback",
                SettingsSection::Playback,
                t!("Playback").into(),
                AppIcon::VolumeHigh,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-desktop",
                SettingsSection::Desktop,
                t!("Desktop").into(),
                AppIcon::MiniPlayer,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-appearance",
                SettingsSection::Appearance,
                t!("Appearance").into(),
                AppIcon::Settings,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-shortcuts",
                SettingsSection::Shortcuts,
                t!("Shortcuts").into(),
                AppIcon::Keyboard,
                compact,
                cx,
            ),
        ];

        if compact {
            h_flex()
                .w_full()
                .flex_none()
                .gap_1()
                .p_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        } else {
            v_flex()
                .w(px(184.0))
                .flex_none()
                .gap_1()
                .p_3()
                .border_r_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        }
    }

    pub(super) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cache_dir = config::audio_cache_dir(&self.config);
        let (title, description) = match self.state.settings_section {
            SettingsSection::Servers => (
                t!("Servers"),
                t!("Manage server connections and library access."),
            ),
            SettingsSection::Playback => (
                t!("Playback"),
                t!("Configure streaming, audio output, and listening preferences."),
            ),
            SettingsSection::Desktop => (
                t!("Desktop"),
                t!("Control notifications and window behavior."),
            ),
            SettingsSection::Appearance => (
                t!("Appearance"),
                t!("Choose the theme and interface language."),
            ),
            SettingsSection::Shortcuts => (
                t!("Shortcuts"),
                t!("Customize keyboard shortcuts for playback and navigation."),
            ),
        };
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .when(self.state.settings_section == SettingsSection::Servers, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Connections")),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .when(self.api.is_some(), |this| {
                                        this.child(
                                            Button::new("settings-refresh")
                                                .icon(AppIcon::Refresh)
                                                .tooltip(t!("Refresh library"))
                                                .loading(self.state.loading)
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.refresh_library();
                                                    cx.notify();
                                                })),
                                        )
                                    })
                                    .child(
                                        Button::new("settings-add-server")
                                            .icon(KitIcon::Plus)
                                            .label(t!("Add server"))
                                            .tooltip(t!("Add another server"))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_server(window, cx);
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(self.render_server_status_card(cx)),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Streaming & cache")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
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
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Streaming quality")),
                                    )
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
                                                                    this.config
                                                                        .transcoding_quality
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
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Audio output")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child({
                                let playback = self.audio.state();
                                let active = match playback.output_name.as_deref() {
                                    Some(name) => name.to_string(),
                                    None => t!("System default").to_string(),
                                };
                                div().text_xs().text_color(cx.theme().muted_foreground).child(
                                    t!("Active: %{active} · %{rate} Hz · %{channels} ch",
                                        active = active,
                                        rate = playback.output_sample_rate,
                                        channels = playback.output_channels
                                    ),
                                )
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
                                                            .checked(
                                                                this.config.output_device.is_none(),
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.set_output_device(None, cx);
                                                                },
                                                            )),
                                                    );
                                                    for (name, is_default) in
                                                        &this.state.output_devices
                                                    {
                                                        let name_for_click = name.clone();
                                                        let label = if *is_default {
                                                            t!(
                                                                "%{name} (default)",
                                                                name = name
                                                            )
                                                        } else {
                                                            t!("%{name}", name = name)
                                                        };
                                                        menu = menu.item(
                                                            PopupMenuItem::new(label)
                                                                .checked(
                                                                    this.config
                                                                        .output_device
                                                                        .as_deref()
                                                                        == Some(name.as_str()),
                                                                )
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.set_output_device(
                                                                            Some(
                                                                                name_for_click
                                                                                    .clone(),
                                                                            ),
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
                                            .loading(self.state.output_devices_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_output_devices(cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Volume normalization")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
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
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Lyrics")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Show translation")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Show translated lyrics under the original line when the server provides another language variant."
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                Switch::new("lyrics-translation-switch")
                                    .checked(self.config.show_lyrics_translation)
                                    .accessibility_label(t!("Show translation"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.set_show_lyrics_translation(*checked, cx);
                                    })),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Desktop, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("System integration")),
                    )
                    .child(
                        v_flex()
                            .gap_5()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                h_flex()
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
                                                    .child(t!("Now-playing notifications")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("Send a desktop notification with the cover, title, artist and album when a track starts. Rapid switching replaces the previous notification."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("track-notifications-switch")
                                            .checked(self.config.show_track_notifications)
                                            .accessibility_label(t!("Now-playing notifications"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_show_track_notifications(*checked, cx);
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
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
                                                    .child(t!("Close button minimizes")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("The window close button minimizes to the taskbar instead of quitting; tray Exit still quits. Some Wayland compositors ignore minimize requests."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("close-minimizes-switch")
                                            .checked(self.config.close_minimizes)
                                            .accessibility_label(t!("Close button minimizes"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_close_minimizes(*checked, cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Theme")),
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                h_flex().gap_2().children(
                                    [
                                        ThemePreference::Light,
                                        ThemePreference::Dark,
                                        ThemePreference::System,
                                    ]
                                    .into_iter()
                                    .map(|preference| {
                                        // 同上：ID 用 Debug 名（Light/Dark/System），
                                        // 不随界面语言变化。
                                        Button::new(SharedString::from(format!(
                                            "theme-{preference:?}"
                                        )))
                                        .label(preference.label())
                                        .selected(self.config.theme == preference)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_theme(preference, window, cx);
                                            }),
                                        )
                                    }),
                                ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Language")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!(
                                                "Interface language. Changes apply instantly."
                                            )),
                                    ),
                            )
                            .child({
                                let app = cx.entity().downgrade();
                                Button::new("language-select")
                                    .w(px(220.0))
                                    .justify_between()
                                    .label(self.config.language.native_label())
                                    .dropdown_caret(true)
                                    .outline()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |this, cx| {
                                            let mut menu = menu;
                                            for preference in LanguagePreference::ALL {
                                                menu = menu.item(
                                                    PopupMenuItem::new(preference.native_label())
                                                        .checked(
                                                            this.config.language == preference,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_language(preference, cx);
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns language menu")
                                    })
                            }),
                    ),
            ))
            .when(
                self.state.settings_section == SettingsSection::Shortcuts,
                |settings| settings.child(self.render_shortcut_settings(cx)),
            )
            .into_any_element()
    }
}
