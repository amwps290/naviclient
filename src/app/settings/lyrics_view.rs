//! Lyrics preferences are persisted before changing live request behavior.

use super::components::{settings_group, settings_row};
use crate::app::prelude::*;
use crate::lyrics::provider::ProviderId;
use crate::lyrics::{LyricsPolicy, LyricsSettings};
use gpui_kit::component::menu::PopupMenuItem;

impl LyricsPolicy {
    fn label(self) -> SharedString {
        match self {
            Self::SourceFirst => t!("Music source first"),
            Self::OnlineFirst => t!("Online lyrics first"),
            Self::Manual => t!("Manual search only"),
        }
        .into()
    }
}

impl NavidromeApp {
    pub(in crate::app) fn set_word_lyrics(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let mut next = self.config.clone();
        next.lyrics.word_timing = enabled;
        match config::save(&next) {
            Ok(()) => {
                self.config = next;
            }
            Err(error) => self.notify_error(t!("Could not save lyrics: %{error}", error = error)),
        }
        cx.notify();
    }

    pub(in crate::app) fn change_lyrics_settings(
        &mut self,
        change: impl FnOnce(&mut LyricsSettings),
        cx: &mut Context<Self>,
    ) {
        if self.save_lyrics_settings(change, cx) {
            self.lyrics_ui.cancel();
        }
    }

    /// The search dialog can enable the service without losing its search fields.
    pub(in crate::app) fn save_lyrics_settings(
        &mut self,
        change: impl FnOnce(&mut LyricsSettings),
        cx: &mut Context<Self>,
    ) -> bool {
        let mut next = self.config.clone();
        change(&mut next.lyrics);
        if next.resolve_connection(&next.lyrics.connection).is_err() {
            self.notify_error(t!("Proxy unavailable"));
            return false;
        }
        if let Err(error) = config::save(&next) {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            return false;
        }
        self.config = next;
        self.lyrics_client = None;
        self.refresh_lyrics();
        self.notify_success(t!("Lyrics settings saved."));
        cx.notify();
        true
    }

    pub(in crate::app) fn render_lyrics_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let policy = self.config.lyrics.policy;
        let (count, bytes) = self.lyrics_cache.stats();

        let display_group = settings_group(
            None,
            None,
            vec![
                settings_row(
                    t!("Word-by-word lyrics"),
                    Some(
                        t!("Highlight words when timing is available; otherwise follow whole lines.")
                            .into(),
                    ),
                    Switch::new("word-lyrics-enabled")
                        .checked(self.config.lyrics.word_timing)
                        .accessibility_label(t!("Word-by-word lyrics"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.set_word_lyrics(*checked, cx)
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Show translation"),
                    Some(t!("Only translations provided by the lyrics source are displayed.").into()),
                    Switch::new("lyrics-translation-switch")
                        .checked(self.config.show_lyrics_translation)
                        .accessibility_label(t!("Show translation"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.set_show_lyrics_translation(*checked, cx)
                        }))
                        .into_any_element(),
                    cx,
                ),
            ],
            cx,
        );

        let online_group = settings_group(
            Some(t!("Online lyrics").into()),
            None,
            vec![
                settings_row(
                    t!("Online lyrics"),
                    Some(
                        t!("When enabled, song metadata is sent to the selected lyrics services.")
                            .into(),
                    ),
                    Switch::new("online-lyrics-enabled")
                        .checked(self.config.lyrics.enabled)
                        .accessibility_label(t!("Online lyrics"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.change_lyrics_settings(|s| s.enabled = *checked, cx)
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Loading policy"),
                    None,
                    surface_button("lyrics-policy")
                        .w(px(220.0))
                        .justify_between()
                        .label(policy.label())
                        .dropdown_caret(true)
                        .compact_action()
                        .dropdown_menu(move |menu, _, cx| {
                            app.update(cx, |_, cx| {
                                [
                                    LyricsPolicy::SourceFirst,
                                    LyricsPolicy::OnlineFirst,
                                    LyricsPolicy::Manual,
                                ]
                                .into_iter()
                                .fold(menu, |menu, choice| {
                                    menu.item(
                                        PopupMenuItem::new(choice.label())
                                            .checked(choice == policy)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.change_lyrics_settings(
                                                    |s| s.policy = choice,
                                                    cx,
                                                )
                                            })),
                                    )
                                })
                            })
                            .expect("app owns lyrics policy menu")
                        })
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Connection"),
                    None,
                    self.connection_picker(
                        "lyrics-connection",
                        &self.config.lyrics.connection,
                        false,
                        |this, connection, cx| {
                            this.change_lyrics_settings(|s| s.connection = connection, cx)
                        },
                        cx,
                    ),
                    cx,
                ),
            ],
            cx,
        );

        let mut source_rows = vec![div()
            .px_4()
            .py_2p5()
            .text_xs()
            .text_color(palette(cx).text_secondary)
            .child(t!(
                "Automatic lookup tries enabled sources from top to bottom. No account is required."
            ))
            .into_any_element()];
        source_rows.extend(ProviderId::ALL.into_iter().map(|id| {
            settings_row(
                id.label(),
                None,
                Switch::new(SharedString::from(format!("lyrics-source-{}", id.key())))
                    .checked(self.config.lyrics.sources.contains(&id))
                    .accessibility_label(id.label())
                    .on_change(cx.listener(move |this, checked, _, cx| {
                        this.change_lyrics_settings(
                            |settings| {
                                settings.sources.retain(|source| *source != id);
                                if *checked {
                                    settings.sources.push(id);
                                }
                            },
                            cx,
                        );
                    }))
                    .into_any_element(),
                cx,
            )
        }));
        let sources_group =
            settings_group(Some(t!("Lyrics sources").into()), None, source_rows, cx);

        let cache_group = settings_group(
            None,
            None,
            vec![settings_row(
                t!(
                    "Saved lyrics: %{count} · %{size} MB",
                    count = count,
                    size = format!("{:.2}", bytes as f64 / 1048576.0)
                ),
                Some(
                    t!("Clearing online cache keeps remembered choices and timing adjustments.")
                        .into(),
                ),
                surface_button("clear-online-lyrics-cache")
                    .label(t!("Clear online cache"))
                    .compact_action()
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.lyrics_cache.clear_automatic() {
                            Ok(()) => this.notify_success(t!("Online lyrics cache cleared.")),
                            Err(error) => this
                                .notify_error(t!("Could not save lyrics: %{error}", error = error)),
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                cx,
            )],
            cx,
        );

        v_flex()
            .w_full()
            .gap_5()
            .child(display_group)
            .child(online_group)
            .child(sources_group)
            .child(cache_group)
            .into_any_element()
    }
}
