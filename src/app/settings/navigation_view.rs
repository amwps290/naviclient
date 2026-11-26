//! Shared section presentation and settings navigation.

use crate::app::prelude::*;

pub(in crate::app) struct SettingsSectionPresentation {
    pub(in crate::app) title: SharedString,
    pub(in crate::app) description: SharedString,
    navigation_id: &'static str,
    icon: AppIcon,
}

impl SettingsSection {
    pub(in crate::app) fn presentation(self) -> SettingsSectionPresentation {
        let (title, description, navigation_id, icon) = match self {
            Self::Sources => (
                t!("Music sources"),
                t!("Manage music sources, accounts, and their network connections."),
                "settings-nav-sources",
                AppIcon::Server,
            ),
            Self::Proxies => (
                t!("Proxies"),
                t!("Save proxies here, then choose a connection for each music source."),
                "settings-nav-proxies",
                AppIcon::Globe,
            ),
            Self::Playback => (
                t!("Playback"),
                t!("Configure streaming, audio output, and listening preferences."),
                "settings-nav-playback",
                AppIcon::VolumeHigh,
            ),
            Self::Desktop => (
                t!("Desktop"),
                t!("Control notifications and window behavior."),
                "settings-nav-desktop",
                AppIcon::MiniPlayer,
            ),
            Self::Lyrics => (
                t!("Lyrics"),
                t!("Choose lyrics sources, matching and display preferences."),
                "settings-nav-lyrics",
                AppIcon::Translation,
            ),
            Self::Appearance => (
                t!("Appearance"),
                t!("Choose the theme and interface language."),
                "settings-nav-appearance",
                AppIcon::Eye,
            ),
            Self::Shortcuts => (
                t!("Shortcuts"),
                t!("Customize keyboard shortcuts for playback and navigation."),
                "settings-nav-shortcuts",
                AppIcon::Keyboard,
            ),
            Self::Logs => (
                t!("Logs"),
                t!("View recent logs and playback details to troubleshoot problems."),
                "settings-nav-logs",
                AppIcon::Queue,
            ),
        };
        SettingsSectionPresentation {
            title: title.into(),
            description: description.into(),
            navigation_id,
            icon,
        }
    }
}

impl NavidromeApp {
    fn settings_navigation_button(
        &self,
        section: SettingsSection,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let presentation = section.presentation();
        let label = presentation.title;
        let selected = self.state.settings.section == section;
        let p = palette(cx);

        let button = Button::new(presentation.navigation_id)
            .w_full()
            .h(px(36.0))
            .when(!compact, |this| this.justify_start())
            .accessibility_label(label.clone())
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap_2p5()
                    .when(!compact, |this| this.justify_start())
                    .child(
                        div().w(px(18.0)).flex_none().flex().justify_center().child(
                            Icon::new(presentation.icon)
                                .with_size(px(15.0))
                                .text_color(if selected { p.brand } else { p.text_muted }),
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .when(selected, |this| this.font_weight(FontWeight::MEDIUM))
                            .truncate()
                            .child(label),
                    ),
            )
            .custom(soft_toggle_style(selected, cx))
            .selected(selected)
            .rounded(px(tokens::radius::ROW))
            .on_click(cx.listener(move |this, _, _, cx| {
                if section != this.state.settings.section {
                    this.state.settings.bilibili_login.cancel();
                }
                this.cancel_shortcut_recording();
                this.state.settings.section = section;
                if section == SettingsSection::Logs {
                    this.logs.refresh();
                }
                this.settings_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }));

        if compact {
            div()
                .flex_none()
                .min_w(px(116.0))
                .child(button)
                .into_any_element()
        } else {
            div()
                .relative()
                .w_full()
                .child(button)
                .when(selected, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(px(3.0))
                            .top(px(9.0))
                            .bottom(px(9.0))
                            .w(px(3.0))
                            .rounded_full()
                            .bg(p.brand),
                    )
                })
                .into_any_element()
        }
    }

    pub(in crate::app) fn render_settings_navigation(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if compact {
            let buttons = SettingsSection::ALL
                .into_iter()
                .map(|section| self.settings_navigation_button(section, true, cx))
                .collect::<Vec<_>>();
            return h_flex()
                .id("settings-nav-scroll")
                .w_full()
                .overflow_x_scroll()
                .flex_none()
                .gap_1()
                .p_2()
                .children(buttons)
                .into_any_element();
        }

        let p = palette(cx);
        let divider = |cx: &App| {
            let dark = cx.theme().is_dark();
            div()
                .w_full()
                .my_1p5()
                .px_2()
                .child(div().w_full().h(px(1.0)).bg(if dark {
                    hsla(0.0, 0.0, 1.0, 0.06)
                } else {
                    p.border.opacity(0.65)
                }))
        };

        let groups: [&[SettingsSection]; 3] = [
            &[SettingsSection::Sources, SettingsSection::Proxies],
            &[
                SettingsSection::Playback,
                SettingsSection::Lyrics,
                SettingsSection::Desktop,
                SettingsSection::Appearance,
            ],
            &[SettingsSection::Shortcuts, SettingsSection::Logs],
        ];

        let mut rail = v_flex().w_full().flex_none().gap_0p5().px_3().pb_4();
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                rail = rail.child(divider(cx));
            }
            for &section in group {
                rail = rail.child(self.settings_navigation_button(section, false, cx));
            }
        }
        rail.into_any_element()
    }
}
