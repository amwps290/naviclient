//! Settings page header and selected-section composition.

use super::*;

impl NavidromeApp {
    pub(super) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let section = self.state.settings.section;
        let presentation = section.presentation();
        let content = match section {
            SettingsSection::Servers => vec![self.render_server_settings(cx)],
            SettingsSection::Playback => self.render_playback_settings(cx),
            SettingsSection::Desktop => vec![self.render_desktop_settings(cx)],
            SettingsSection::Appearance => self.render_appearance_settings(cx),
            SettingsSection::Shortcuts => vec![self.render_shortcut_settings(cx)],
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
                            .child(presentation.title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(presentation.description),
                    ),
            )
            .children(content)
            .into_any_element()
    }
}
