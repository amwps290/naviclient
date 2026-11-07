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
            Self::Servers => (
                t!("Servers"),
                t!("Manage server connections and library access."),
                "settings-nav-servers",
                AppIcon::Server,
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
            Self::Appearance => (
                t!("Appearance"),
                t!("Choose the theme and interface language."),
                "settings-nav-appearance",
                AppIcon::Settings,
            ),
            Self::Shortcuts => (
                t!("Shortcuts"),
                t!("Customize keyboard shortcuts for playback and navigation."),
                "settings-nav-shortcuts",
                AppIcon::Keyboard,
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
                    .gap_2()
                    .when(!compact, |this| this.justify_start())
                    .child(
                        div()
                            .w(px(20.0))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(Icon::new(presentation.icon).with_size(px(16.0))),
                    )
                    .child(div().min_w_0().text_sm().truncate().child(label)),
            )
            .custom(soft_toggle_style(selected, cx))
            .selected(selected)
            .rounded(px(tokens::radius::CONTROL))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cancel_shortcut_recording();
                this.state.settings.section = section;
                this.settings_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }));

        if compact {
            div().flex_1().min_w_0().child(button).into_any_element()
        } else {
            button.into_any_element()
        }
    }

    pub(in crate::app) fn render_settings_navigation(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let buttons = SettingsSection::ALL
            .into_iter()
            .map(|section| self.settings_navigation_button(section, compact, cx))
            .collect::<Vec<_>>();

        if compact {
            h_flex()
                .w_full()
                .flex_none()
                .gap_1()
                .p_2()
                .children(buttons)
                .into_any_element()
        } else {
            v_flex()
                .w(px(184.0))
                .flex_none()
                .gap_1()
                .p_3()
                .children(buttons)
                .into_any_element()
        }
    }
}
