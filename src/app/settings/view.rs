//! Settings page header and selected-section composition.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let section = self.state.settings.section;
        let presentation = section.presentation();
        let content = match section {
            SettingsSection::Sources => vec![self.render_source_settings(cx)],
            SettingsSection::Proxies => vec![self.render_proxy_settings(cx)],
            SettingsSection::Playback => self.render_playback_settings(cx),
            SettingsSection::Lyrics => vec![self.render_lyrics_settings(cx)],
            SettingsSection::Desktop => vec![self.render_desktop_settings(cx)],
            SettingsSection::Appearance => self.render_appearance_settings(cx),
            SettingsSection::Shortcuts => vec![self.render_shortcut_settings(cx)],
        };
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_6()
            .child(
                v_flex()
                    .gap_1()
                    .child(section_title(presentation.title, cx))
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(palette(cx).text_secondary)
                            .child(presentation.description),
                    ),
            )
            .children(content)
            .into_any_element()
    }
}
