//! Queue drawer chrome and controls. Song-list virtualization stays in
//! `views.rs`; this module owns the drawer-specific header, tabs, empty states
//! and queue actions so the main view router does not absorb another large UI.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_queue_drawer(
        &self,
        width: f32,
        player_clearance: f32,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let compact = width <= 340.0;
        let tab = self.state.queue_drawer_tab;
        let visible_count = match tab {
            QueueDrawerTab::UpNext => self.state.queue.len(),
            QueueDrawerTab::History => self.state.listening.recent_songs().len(),
        };
        let dim = if cx.theme().is_dark() { 0.22 } else { 0.08 };

        let drawer = translucent_surface(
            v_flex()
                .id("queue-drawer")
                .w(px(width))
                .h_full()
                .min_w_0()
                .flex_none()
                .occlude()
                .overflow_hidden(),
            tokens::radius::LARGE,
            self.now_playing_cover_palette().map(|(base, _)| base),
            cx,
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(self.render_queue_drawer_header(tab, visible_count, cx))
        .child(self.render_queue_drawer_tabs(tab, cx))
        .when(tab == QueueDrawerTab::UpNext, |this| {
            this.child(self.render_auto_continue_control(cx))
        })
        .when(
            tab == QueueDrawerTab::UpNext && self.state.queue.is_empty(),
            |this| this.child(self.render_queue_empty_state(false, cx)),
        )
        .when(
            tab == QueueDrawerTab::UpNext && !self.state.queue.is_empty(),
            |this| {
                this.child(self.render_queue_action_bar(compact, cx)).child(
                    v_flex()
                        .flex_1()
                        .min_h(px(200.0))
                        .child(self.render_song_list(SongScope::Queue, cx)),
                )
            },
        )
        .when(
            tab == QueueDrawerTab::History && self.state.listening.recent_songs().is_empty(),
            |this| this.child(self.render_queue_empty_state(true, cx)),
        )
        .when(
            tab == QueueDrawerTab::History && !self.state.listening.recent_songs().is_empty(),
            |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .min_h(px(200.0))
                        .child(self.render_song_list(SongScope::History, cx)),
                )
            },
        );

        div()
            .id("queue-backdrop")
            .absolute()
            .inset_0()
            .pt(px(tokens::QUEUE_DRAWER_MARGIN))
            .pr(px(tokens::QUEUE_DRAWER_MARGIN))
            .pb(px(player_clearance))
            .occlude()
            .flex()
            .justify_end()
            .bg(hsla(0.0, 0.0, 0.0, dim))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.queue_drawer_open = false;
                    this.clear_song_multi();
                    cx.notify();
                }),
            )
            .child(drawer)
            .into_any_element()
    }

    pub(in crate::app) fn render_queue_drawer_header(
        &self,
        tab: QueueDrawerTab,
        visible_count: usize,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .h(px(56.0))
            .flex_none()
            .px_4()
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(section_title(tab.label(), cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {}",
                                self.playback_source_label(),
                                t!("%{count} tracks", count = visible_count)
                            )),
                    ),
            )
            .child(
                Button::new("close-queue-drawer")
                    .icon(AppIcon::Close)
                    .tooltip(t!("Close"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.queue_drawer_open = false;
                        this.clear_song_multi();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_queue_drawer_tabs(
        &self,
        selected: QueueDrawerTab,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        div()
            .w_full()
            .flex_none()
            .px_4()
            .pb_3()
            .border_b_1()
            .border_color(p.border.opacity(0.55))
            .child(
                h_flex()
                    .w_full()
                    .p_1()
                    .gap_1()
                    .rounded_full()
                    .bg(p.sunken)
                    .children(QueueDrawerTab::ALL.map(|tab| {
                        let active = selected == tab;
                        Button::new(match tab {
                            QueueDrawerTab::UpNext => "queue-tab-up-next",
                            QueueDrawerTab::History => "queue-tab-history",
                        })
                        .label(tab.label())
                        .custom(soft_toggle_style(active, cx))
                        .small()
                        .flex_1()
                        .rounded_full()
                        .selected(active)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.state.queue_drawer_tab = tab;
                            this.clear_song_multi();
                            cx.notify();
                        }))
                    })),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_auto_continue_control(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .min_h(px(52.0))
            .flex_none()
            .px_4()
            .py_2()
            .gap_3()
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Automatic continuation")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.state.auto_continue.loading() {
                                t!("Finding more music…")
                            } else {
                                t!("Keep playing recommendations when the queue ends")
                            }),
                    ),
            )
            .child(
                Switch::new("queue-auto-continue-switch")
                    .checked(self.config.auto_continue)
                    .accessibility_label(t!("Automatic continuation"))
                    .on_change(cx.listener(|this, checked, _, cx| {
                        this.set_auto_continue(*checked, cx);
                    })),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_queue_empty_state(
        &self,
        history: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let (title, description) = if history {
            (
                t!("No listening history yet"),
                t!("Tracks appear here when playback starts."),
            )
        } else {
            (
                t!("Queue is empty"),
                t!("Start playing an album or playlist to build a queue."),
            )
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .px_6()
            .gap_2()
            .text_center()
            .child(
                Icon::new(AppIcon::Queue)
                    .with_size(px(28.0))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_queue_action_bar(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .flex_none()
            .when(compact, |this| this.px_2())
            .when(!compact, |this| this.px_3())
            .py_2()
            .gap_0p5()
            .child(
                Button::new("drawer-queue-play-all")
                    .icon(PlayerIcon::Play)
                    .label(t!("Play all"))
                    .tooltip(t!("Play all"))
                    .primary()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.state.queue.is_empty() {
                            this.play_song_at_scope(SongScope::Queue, 0);
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("drawer-queue-clear-after")
                    .label(if compact {
                        t!("Clear after")
                    } else {
                        t!("Clear after current")
                    })
                    .tooltip(t!("Clear after current"))
                    .ghost()
                    .small()
                    .disabled(self.state.queue_index.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clear_queue_after_current();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("drawer-save-queue-playlist")
                    .icon(AppIcon::Playlists)
                    .tooltip(t!("Save as playlist"))
                    .ghost()
                    .small()
                    .disabled(
                        self.state.playlists.editing()
                            || self.api.as_ref().is_none_or(|api| {
                                !api.capabilities().playlist_write
                                    || self
                                        .state
                                        .queue
                                        .songs()
                                        .any(|song| !song.belongs_to(api.id()))
                            }),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.api.as_ref().is_some_and(|api| {
                            api.capabilities().playlist_write
                                && this
                                    .state
                                    .queue
                                    .songs()
                                    .all(|song| song.belongs_to(api.id()))
                        }) {
                            this.name_playlist(None, this.state.queue.song_ids(), window, cx);
                        }
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("drawer-queue-clear-all")
                    .icon(AppIcon::Trash)
                    .tooltip(t!("Clear queue"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clear_queue();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
