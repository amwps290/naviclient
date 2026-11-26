//! Vinyl Salon: cover-reactive ambience, spacious turntable stage, luminous lyrics, and glass dock.

use super::layout::FullscreenLayout;
use crate::app::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct SalonPalette {
    pub(super) page: Hsla,
    pub(super) dock_top: Hsla,
    pub(super) dock_bottom: Hsla,
    pub(super) glass: Hsla,
    pub(super) text: Hsla,
    pub(super) secondary: Hsla,
    pub(super) muted: Hsla,
    pub(super) accent: Hsla,
    pub(super) active_lyric: Hsla,
    pub(super) border: Hsla,
}

pub(super) fn salon_palette() -> SalonPalette {
    SalonPalette {
        page: gpui_kit::rgb(0x12100f).into(),
        dock_top: hsla(0.08, 0.08, 0.14, 0.80),
        dock_bottom: hsla(0.08, 0.08, 0.09, 0.90),
        glass: hsla(0.10, 0.20, 0.92, 0.05),
        text: gpui_kit::rgb(0xf2ebe0).into(),
        secondary: gpui_kit::rgb(0xc7baa6).into(),
        muted: gpui_kit::rgb(0x958977).into(),
        accent: gpui_kit::rgb(0xd7bd8c).into(),
        active_lyric: gpui_kit::rgb(0xf9efd9).into(),
        border: hsla(0.11, 0.30, 0.78, 0.14),
    }
}

pub(super) fn salon_button(
    id: impl Into<gpui_kit::ElementId>,
    icon: AppIcon,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .icon(icon)
        .custom(salon_toggle_style(false, cx))
        .accessibility_label(label.clone())
        .tooltip(label)
        .with_size(px(34.0))
        .rounded_full()
}

pub(super) fn salon_toggle_style(active: bool, cx: &App) -> ButtonCustomVariant {
    let p = salon_palette();
    ButtonCustomVariant::new(cx)
        .color(if active {
            p.accent.opacity(0.16)
        } else {
            p.page.opacity(0.0)
        })
        .foreground(if active { p.accent } else { p.secondary })
        .hover(p.accent.opacity(0.12))
        .active(p.accent.opacity(0.20))
}

fn ambient_glow(color: Hsla, strength: f32) -> Hsla {
    hsla(color.h, color.s.min(0.55), 0.36, strength)
}

impl NavidromeApp {
    pub(in crate::app) fn render_fullscreen_player(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let width = f32::from(window.viewport_size().width);
        let height = f32::from(window.viewport_size().height);
        let layout = FullscreenLayout::new(width, height);
        let p = salon_palette();
        let opacity = self.fullscreen.state.opacity();
        let cover_colors = self.now_playing_cover_palette();
        let (left_glow, right_glow) = match cover_colors {
            Some((base, accent)) => (ambient_glow(base, 0.34), ambient_glow(accent, 0.18)),
            None => (hsla(0.09, 0.30, 0.24, 0.32), hsla(0.11, 0.24, 0.20, 0.16)),
        };

        v_flex()
            .id("fullscreen-player")
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.fullscreen.focus)
            .bg(p.page)
            .text_color(p.text)
            .px(px(layout.padding_x))
            .pt(px(layout.padding_top))
            .pb(px(layout.padding_bottom))
            .gap(px(layout.gap))
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    this.move_fullscreen_pointer(event.position, cx)
                }),
            )
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                window.focus(&this.fullscreen.focus, cx);
                this.fullscreen.pointer_down = true;
                this.wake_fullscreen_controls(cx);
            }))
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                this.fullscreen.pointer_down = false;
                this.wake_fullscreen_controls(cx);
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.fullscreen.pointer_down = false;
                    this.wake_fullscreen_controls(cx);
                }),
            )
            // Cover-reactive layered diagonal salon ambience.
            .child(div().absolute().inset_0().bg(linear_gradient(
                128.0,
                linear_color_stop(p.page.blend(left_glow), 0.0),
                linear_color_stop(p.page, 0.68),
            )))
            .child(div().absolute().inset_0().bg(linear_gradient(
                302.0,
                linear_color_stop(right_glow, 0.0),
                linear_color_stop(p.page.opacity(0.0), 0.56),
            )))
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.0,
                linear_color_stop(hsla(0.0, 0.0, 0.0, 0.32), 0.0),
                linear_color_stop(p.page.opacity(0.0), 0.34),
            )))
            .child(div().h(px(layout.header_height)).w_full().flex_none().when(
                opacity > 0.0,
                |this| {
                    this.child(self.render_fullscreen_header(cx))
                        .opacity(opacity)
                },
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(layout.stage_width))
                    .mx_auto()
                    .gap(px(layout.column_gap))
                    .items_center()
                    .when(layout.stacked, |this| this.flex_col())
                    .child(self.render_fullscreen_art(&layout, cx))
                    .child(self.render_fullscreen_lyrics(&layout, cx)),
            )
            .child(div().h(px(layout.footer_height)).w_full().flex_none().when(
                opacity > 0.0,
                |this| {
                    this.child(self.render_fullscreen_footer(&layout, cx))
                        .opacity(opacity)
                },
            ))
            .into_any_element()
    }

    fn render_fullscreen_header(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let translated = self.fullscreen.translated;
        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .justify_between()
            .track_focus(&self.fullscreen.header_focus)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().size(px(7.0)).rounded_full().bg(p.accent).shadow(vec![
                        box_shadow(px(0.0), px(0.0), px(12.0), px(0.0), p.accent.opacity(0.55)),
                    ]))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("NAVI"),
                    )
                    .child(div().w(px(1.0)).h(px(13.0)).bg(p.border))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(p.muted)
                            .child(t!("Vinyl Salon")),
                    ),
            )
            .child(
                h_flex()
                    .px_1p5()
                    .py_1()
                    .gap_1()
                    .items_center()
                    .rounded_full()
                    .bg(p.glass)
                    .border_1()
                    .border_color(p.border)
                    .child(
                        salon_button(
                            "fullscreen-translation",
                            AppIcon::Translation,
                            t!("Toggle translation (T)"),
                            cx,
                        )
                        .custom(salon_toggle_style(translated, cx))
                        .selected(translated)
                        .toggled(translated)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_fullscreen_translation(cx)),
                        ),
                    )
                    .child(div().w(px(1.0)).h(px(14.0)).bg(p.border))
                    .child(
                        salon_button(
                            "fullscreen-hide-controls",
                            AppIcon::Eye,
                            t!("Hide controls (H)"),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_fullscreen_controls(window, cx)
                        })),
                    )
                    .child(div().w(px(1.0)).h(px(14.0)).bg(p.border))
                    .child(
                        salon_button(
                            "fullscreen-exit",
                            AppIcon::FullscreenExit,
                            t!("Exit fullscreen (Esc)"),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.exit_fullscreen_player(window, cx)
                        })),
                    ),
            )
            .into_any_element()
    }

    fn render_fullscreen_art(
        &self,
        layout: &FullscreenLayout,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let song = self.state.now_playing.as_ref();
        let album = song.map(|song| song.album.clone()).unwrap_or_default();
        let title = song
            .map(|song| song.title.clone())
            .unwrap_or_else(|| t!("No track playing").to_string());
        let artist = song
            .map(|song| song.artist.clone())
            .unwrap_or_else(|| t!("Choose a song from your library.").to_string());
        let serif = crate::typography::serif_font_family(cx);
        let can_favorite = song.is_some_and(|song| {
            self.api.as_ref().is_some_and(|api| {
                api.capabilities().favorite_write
                    && self
                        .source_for_song(song)
                        .is_some_and(|source| source.id() == api.id())
            })
        });
        let halo_color = self
            .now_playing_cover_palette()
            .map(|(_, accent)| hsla(accent.h, accent.s.min(0.45), 0.55, 0.16))
            .unwrap_or_else(|| p.accent.opacity(0.14));

        v_flex()
            .w(px(layout.art_width))
            .flex_none()
            .items_center()
            .child(
                v_flex()
                    .w(px(layout.metadata_width))
                    .min_w_0()
                    .items_center()
                    .child(
                        div()
                            .relative()
                            .size(px(layout.cover_size))
                            .flex_none()
                            .rounded_full()
                            .shadow(vec![
                                box_shadow(
                                    px(0.0),
                                    px(0.0),
                                    px(layout.cover_size * 0.24),
                                    px(layout.cover_size * 0.02),
                                    halo_color,
                                ),
                                box_shadow(
                                    px(0.0),
                                    px(24.0),
                                    px(64.0),
                                    px(-8.0),
                                    hsla(0.0, 0.0, 0.0, 0.58),
                                ),
                            ])
                            .child(self.render_vinyl_record_styled(
                                layout.cover_size,
                                0.40,
                                Some(p.accent),
                                cx,
                            )),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .mt(px(if layout.compact { 22.0 } else { 28.0 }))
                            .gap_1p5()
                            .when(!album.trim().is_empty(), |this| {
                                this.child(
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .w(px(18.0))
                                                .h(px(1.0))
                                                .flex_none()
                                                .bg(p.accent.opacity(0.70)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(12.0))
                                                .text_color(p.muted)
                                                .truncate()
                                                .child(album.clone()),
                                        ),
                                )
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("fullscreen-song-title")
                                            .flex_1()
                                            .min_w_0()
                                            .font_family(serif)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(if layout.compact { 28.0 } else { 36.0 }))
                                            .line_height(relative(1.25))
                                            .text_color(p.text)
                                            .truncate()
                                            .tooltip({
                                                let title = title.clone();
                                                move |window, cx| {
                                                    Tooltip::new(title.clone()).build(window, cx)
                                                }
                                            })
                                            .child(title),
                                    )
                                    .when(can_favorite, |this| {
                                        let song = song.unwrap();
                                        let starred = self.state.favorites.is_starred(
                                            &FavoriteKey::new(FavoriteKind::Song, &song.id),
                                        );
                                        this.child(div().size(px(34.0)).flex_none().when(
                                            self.fullscreen.state.opacity() > 0.0,
                                            |this| {
                                                this.child(
                                                    self.favorite_button(
                                                        FavoriteKind::Song,
                                                        &song.id,
                                                        cx,
                                                    )
                                                    .text_color(if starred {
                                                        p.accent
                                                    } else {
                                                        p.muted
                                                    })
                                                    .custom(salon_toggle_style(starred, cx))
                                                    .opacity(self.fullscreen.state.opacity()),
                                                )
                                            },
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(p.secondary)
                                    .truncate()
                                    .child(artist),
                            )
                            .when_some(song.and_then(Song::bilibili_subtitle), |this, subtitle| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(p.muted)
                                        .truncate()
                                        .child(subtitle),
                                )
                            }),
                    )
                    .when(!layout.stacked, |this| {
                        this.child(
                            div()
                                .w_full()
                                .h(px(38.0))
                                .flex_none()
                                .mt(px(if layout.compact { 14.0 } else { 20.0 }))
                                .when(self.fullscreen.state.visualizer_enabled, |this| {
                                    this.child(super::visualizer::waveform_view(
                                        self.fullscreen.waveform,
                                        p.accent,
                                    ))
                                }),
                        )
                    }),
            )
            .into_any_element()
    }
}
