//! Settings page header and selected-section composition.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let section = self.state.settings.section;
        let presentation = section.presentation();
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let content = match section {
            SettingsSection::Sources => vec![self.render_source_settings(cx)],
            SettingsSection::Proxies => vec![self.render_proxy_settings(cx)],
            SettingsSection::Playback => self.render_playback_settings(cx),
            SettingsSection::Lyrics => vec![self.render_lyrics_settings(cx)],
            SettingsSection::Desktop => vec![
                self.render_desktop_settings(cx),
                self.render_backup_settings_group(cx),
            ],
            SettingsSection::Appearance => self.render_appearance_settings(cx),
            SettingsSection::Shortcuts => vec![self.render_shortcut_settings(cx)],
            SettingsSection::Logs => vec![self.render_log_settings(cx)],
        };
        v_flex()
            .w_full()
            .max_w(px(760.0))
            .gap_5()
            .pb_4()
            .child(
                v_flex()
                    .gap_1()
                    .pb_3()
                    .pr_10()
                    .border_b_1()
                    .border_color(if dark {
                        hsla(0.0, 0.0, 1.0, 0.07)
                    } else {
                        p.border.opacity(0.65)
                    })
                    .child(serif_heading(presentation.title, 22.0, cx))
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(p.text_secondary)
                            .child(presentation.description),
                    ),
            )
            .children(content)
            .into_any_element()
    }
}
