//! Lyrics preferences are persisted before changing live request behavior.

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
        v_flex().gap_5()
            .child(h_flex().justify_between().gap_4().child(v_flex().flex_1().min_w_0().gap_1()
                .child(div().text_sm().child(t!("Online lyrics")))
                .child(div().text_xs().text_color(palette(cx).text_secondary).child(t!("When enabled, song metadata is sent to the selected lyrics services."))))
                .child(Switch::new("online-lyrics-enabled").checked(self.config.lyrics.enabled).accessibility_label(t!("Online lyrics"))
                    .on_change(cx.listener(|this,checked,_,cx| this.change_lyrics_settings(|s| s.enabled = *checked,cx)))))
            .child(v_flex().gap_3()
                .child(div().text_sm().child(t!("Lyrics sources")))
                .child(div().text_xs().text_color(palette(cx).text_secondary).child(t!("Automatic lookup tries enabled sources from top to bottom. No account is required.")))
                .children(ProviderId::ALL.into_iter().map(|id| {
                    h_flex().justify_between().items_center().gap_3()
                        .child(div().text_sm().child(id.label()))
                        .child(Switch::new(SharedString::from(format!("lyrics-source-{}", id.key())))
                            .checked(self.config.lyrics.sources.contains(&id))
                            .accessibility_label(id.label())
                            .on_change(cx.listener(move |this, checked, _, cx| {
                                this.change_lyrics_settings(|settings| {
                                    settings.sources.retain(|source| *source != id);
                                    if *checked { settings.sources.push(id); }
                                }, cx);
                            })))
                })))
            .child(h_flex().justify_between().gap_3().child(div().text_sm().child(t!("Loading policy")))
                .child(Button::new("lyrics-policy").label(policy.label()).dropdown_caret(true).outline().small()
                    .dropdown_menu(move |menu,_,cx| app.update(cx,|_,cx| {
                        [LyricsPolicy::SourceFirst,LyricsPolicy::OnlineFirst,LyricsPolicy::Manual].into_iter().fold(menu,|menu,choice| menu.item(
                            PopupMenuItem::new(choice.label()).checked(choice==policy).on_click(cx.listener(move |this,_,_,cx| this.change_lyrics_settings(|s| s.policy=choice,cx)))))
                    }).expect("app owns lyrics policy menu"))))
            .child(h_flex().justify_between().gap_3().child(div().text_sm().child(t!("Connection")))
                .child(self.connection_picker("lyrics-connection",&self.config.lyrics.connection,false,|this,connection,cx| this.change_lyrics_settings(|s| s.connection=connection,cx),cx)))
            .child(h_flex().justify_between().gap_4().child(v_flex().flex_1().min_w_0().gap_1()
                .child(div().text_sm().child(t!("Show translation")))
                .child(div().text_xs().text_color(palette(cx).text_secondary).child(t!("Only translations provided by the lyrics source are displayed."))))
                .child(Switch::new("lyrics-translation-switch").checked(self.config.show_lyrics_translation).accessibility_label(t!("Show translation"))
                    .on_change(cx.listener(|this,checked,_,cx| this.set_show_lyrics_translation(*checked,cx)))))
            .child(v_flex().gap_2().child(div().text_sm().child(t!("Saved lyrics: %{count} · %{size} MB",count=count,size=format!("{:.2}",bytes as f64/1048576.0))))
                .child(div().text_xs().text_color(palette(cx).text_secondary).child(t!("Clearing online cache keeps remembered choices and timing adjustments.")))
                .child(Button::new("clear-online-lyrics-cache").label(t!("Clear online cache")).outline().small().on_click(cx.listener(|this,_,_,cx| {
                    match this.lyrics_cache.clear_automatic() { Ok(()) => this.notify_success(t!("Online lyrics cache cleared.")), Err(error) => this.notify_error(t!("Could not save lyrics: %{error}",error=error)) } cx.notify();
                }))))
            .into_any_element()
    }
}
