//! Home listening feature and artwork-derived feature colors.

use super::artwork_helpers::{accent_foreground, adjust_lightness, readable_accent};
use super::view_helpers::compact_window_layout;
use super::*;

impl NavidromeApp {
    pub(super) fn home_feature_colors(
        &self,
        cover_id: Option<&str>,
        cx: &Context<Self>,
    ) -> (Hsla, Hsla, Hsla) {
        let (base, raw_accent) = cover_id
            .and_then(|id| self.state.covers.palette(id))
            .unwrap_or((cx.theme().info, cx.theme().chart_2));
        // Grayscale artwork otherwise produces another gray action button.
        // Blend in the theme's semantic info color while retaining cover identity.
        let raw_accent = if raw_accent.s < 0.14 {
            cx.theme().info.blend(raw_accent.opacity(0.35))
        } else {
            raw_accent
        };
        let light = cx.theme().background.l >= 0.5;
        let start = cx
            .theme()
            .secondary
            .blend(base.opacity(if light { 0.24 } else { 0.18 }));
        let end = cx
            .theme()
            .background
            .blend(raw_accent.opacity(if light { 0.18 } else { 0.14 }));
        (
            start,
            end,
            readable_accent(raw_accent, cx.theme().background),
        )
    }

    /// Home 的唯一主焦点：优先继续当前播放；无播放会话时展示最近专辑。
    /// 使用既有封面调色板与播放器按钮语言，不引入新的品牌色或装饰体系。
    pub(super) fn render_home_feature(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let compact = compact_window_layout(f32::from(window.viewport_size().width));
        let art_size = if compact { 96.0 } else { 136.0 };
        let feature_height = if compact { 144.0 } else { 176.0 };

        if let Some(song) = &self.state.now_playing {
            let playback = self.audio.state();
            let is_playing = playback.active && !playback.paused;
            let playback_label = if is_playing { t!("Pause") } else { t!("Play") };
            let (surface_start, surface_end, accent) =
                self.home_feature_colors(song.cover_art.as_deref(), cx);
            let play_style = ButtonCustomVariant::new(cx)
                .color(accent)
                .foreground(accent_foreground(accent))
                .hover(adjust_lightness(accent, 0.05))
                .active(adjust_lightness(accent, -0.05));
            return Some(
                h_flex()
                    .id("home-continue-listening")
                    .w_full()
                    .h(px(feature_height))
                    .when(compact, |this| this.p_4().gap_4())
                    .when(!compact, |this| this.p_5().gap_5())
                    .items_center()
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.58))
                    .shadow_sm()
                    .bg(linear_gradient(
                        118.0,
                        linear_color_stop(surface_start, 0.0),
                        linear_color_stop(surface_end, 1.0),
                    ))
                    .child(
                        div()
                            .id("home-feature-cover")
                            .flex_none()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_now_playing();
                                cx.notify();
                            }))
                            .child(self.render_cover(song.cover_art.as_deref(), art_size, cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .when(compact, |this| this.gap_1())
                            .when(!compact, |this| this.gap_2())
                            .child(
                                div()
                                    .when(compact, |this| this.text_sm())
                                    .when(!compact, |this| this.text_base())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent)
                                    .child(t!("Continue listening")),
                            )
                            .child(
                                div()
                                    .when(compact, |this| this.text_xl())
                                    .when(!compact, |this| this.text_2xl())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground.opacity(0.72))
                                    .truncate()
                                    .child(if song.album.trim().is_empty() {
                                        song.artist.clone()
                                    } else {
                                        format!("{} · {}", song.artist, song.album)
                                    }),
                            )
                            .child(
                                h_flex()
                                    .when(compact, |this| this.pt_1().gap_1())
                                    .when(!compact, |this| this.pt_2().gap_2())
                                    .items_center()
                                    .child(
                                        Button::new("home-feature-playback")
                                            .icon(if is_playing {
                                                PlayerIcon::Pause
                                            } else {
                                                PlayerIcon::Play
                                            })
                                            .accessibility_label(playback_label.clone())
                                            .tooltip(playback_label)
                                            .custom(play_style)
                                            .rounded(px(999.0))
                                            .w(px(if compact { 40.0 } else { 44.0 }))
                                            .h(px(if compact { 40.0 } else { 44.0 }))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("home-feature-now-playing")
                                            .label(t!("Open player"))
                                            .icon(AppIcon::ChevronRight)
                                            .ghost()
                                            .small()
                                            .rounded(px(8.0))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_now_playing();
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }

        let album = self
            .state
            .home_albums
            .albums(HomeAlbumSection::Recent)
            .first()?
            .clone();
        let (surface_start, surface_end, accent) =
            self.home_feature_colors(album.cover_art.as_deref(), cx);
        let play_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground(accent))
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05));
        let album_for_cover = album.clone();
        let album_for_play = album.clone();
        let album_for_open = album.clone();
        Some(
            h_flex()
                .id("home-recent-feature")
                .w_full()
                .h(px(feature_height))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_5().gap_5())
                .items_center()
                .rounded(px(12.0))
                .border_1()
                .border_color(cx.theme().border.opacity(0.58))
                .shadow_sm()
                .bg(linear_gradient(
                    118.0,
                    linear_color_stop(surface_start, 0.0),
                    linear_color_stop(surface_end, 1.0),
                ))
                .child(
                    div()
                        .id("home-feature-album-cover")
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_album(album_for_cover.clone());
                            cx.notify();
                        }))
                        .child(self.render_cover(album.cover_art.as_deref(), art_size, cx)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .when(compact, |this| this.gap_1())
                        .when(!compact, |this| this.gap_2())
                        .child(
                            div()
                                .when(compact, |this| this.text_sm())
                                .when(!compact, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(accent)
                                .child(t!("Recently played")),
                        )
                        .child(
                            div()
                                .when(compact, |this| this.text_xl())
                                .when(!compact, |this| this.text_2xl())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(album.name.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground.opacity(0.72))
                                .truncate()
                                .child(album.artist.clone()),
                        )
                        .child(
                            h_flex()
                                .when(compact, |this| this.pt_1().gap_1())
                                .when(!compact, |this| this.pt_2().gap_2())
                                .items_center()
                                .child(
                                    Button::new("home-feature-play-album")
                                        .icon(PlayerIcon::Play)
                                        .accessibility_label(t!("Play album"))
                                        .tooltip(t!("Play album"))
                                        .custom(play_style)
                                        .rounded(px(999.0))
                                        .w(px(if compact { 40.0 } else { 44.0 }))
                                        .h(px(if compact { 40.0 } else { 44.0 }))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.play_album(album_for_play.clone());
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("home-feature-open-album")
                                        .label(t!("Go to album"))
                                        .icon(AppIcon::ChevronRight)
                                        .ghost()
                                        .small()
                                        .rounded(px(8.0))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_album(album_for_open.clone());
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
