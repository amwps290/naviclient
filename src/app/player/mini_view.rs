//! 迷你播放器窗口的封面、歌曲信息和播放控制渲染。

use crate::app::prelude::*;
use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};

impl NavidromeApp {
    pub(in crate::app) fn render_mini(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let song = self.state.now_playing.clone();
        let has_track = song.is_some();
        let playback = self.playback_for_display();
        let is_playing = playback.active && !playback.paused;
        let duration = playback.duration.unwrap_or_default();
        let progress_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };

        let cover = song
            .as_ref()
            .and_then(|song| self.song_cover_image(song))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);

        let accent = palette(cx).brand;
        let background = tint_background(
            cover_tint(
                TintSurface::MiniPlayer,
                self.now_playing_cover_palette(),
                theme.background,
                theme.is_dark(),
            ),
            theme.background,
            120.0,
        );

        let title = song
            .as_ref()
            .map(|song| song.title.clone())
            .unwrap_or_else(|| t!("No track playing").to_string());
        let artist = song
            .as_ref()
            .map(|song| {
                song.bilibili_part
                    .as_ref()
                    .map(|part| format!("P{}/{} · {}", part.index, part.count, part.title))
                    .unwrap_or_else(|| song.artist.clone())
            })
            .unwrap_or_else(|| t!("Choose a song").to_string());

        let cover_element = img(cover)
            // `on_click` only fires for elements with an id: it needs element state
            // to track the press/release pair.
            .id("mini-cover")
            .size(px(34.0))
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            // 播放中封面外发光随封面调色板同步，静止时保留一层柔和阴影。
            .when(is_playing, |this| {
                this.shadow(vec![
                    box_shadow(px(0.0), px(0.0), px(8.0), px(0.0), accent.opacity(0.4)),
                    box_shadow(px(0.0), px(0.0), px(16.0), px(0.0), accent.opacity(0.2)),
                ])
            })
            .when(!is_playing, |this| this.shadow(cover_shadow(false, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude();

        // Fixed widths in the mini row that leave room for the scrolling info text:
        // px_1p5 padding (12) + cover (34) + gap_1p5 between the 4 row children (18)
        // + controls (18+4+26+4+18 = 70) + expand button (18) = 152. On Linux the
        // window shadow shrinks the content, so use the content (inner) width.
        let paddings = window_paddings(window);
        let content_width = window.viewport_size().width - paddings.left - paddings.right;
        let info_width = f32::from(content_width - px(152.0)).max(1.0);
        let track_id = song
            .as_ref()
            .map(|song| song.id.clone())
            .unwrap_or_default();
        let title_id = SharedString::from(format!("mini-title-{track_id}"));
        let artist_id = SharedString::from(format!("mini-artist-{track_id}"));

        let info = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .overflow_hidden()
            .id("mini-now-playing-info")
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude()
            .child(self.render_scrolling_text(
                title.clone().into(),
                info_width,
                rems(0.75).to_pixels(window.rem_size()),
                FontWeight::SEMIBOLD,
                title_id,
                window,
            ))
            .child(
                // 迷你窗口没有播放条，用灰字换掉艺人行来区分「在等网络」和
                // 「真的出错」，否则用户看到的只是界面静止。
                div()
                    .text_color(theme.muted_foreground)
                    .when(playback.stall.is_some(), |this| {
                        this.child(t!(
                            "Buffering… %{seconds}s",
                            seconds = playback.stall.unwrap_or_default().as_secs()
                        ))
                    })
                    .when(
                        playback.stall.is_none() && playback.error.is_some(),
                        |this| this.child(t!("Playback failed — open for details")),
                    )
                    .when(
                        playback.stall.is_none() && playback.error.is_none(),
                        |this| {
                            this.child(self.render_scrolling_text(
                                artist.clone().into(),
                                info_width,
                                px(10.0),
                                FontWeight::NORMAL,
                                artist_id,
                                window,
                            ))
                        },
                    ),
            );

        let controls = h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .child(
                Button::new("mini-previous")
                    .icon(PlayerIcon::Previous)
                    .tooltip(t!("Previous track"))
                    .ghost()
                    .with_size(px(18.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-play-pause")
                    .icon(if is_playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    })
                    .tooltip(if is_playing { t!("Pause") } else { t!("Play") })
                    .primary()
                    .with_size(px(26.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-next")
                    .icon(PlayerIcon::Next)
                    .tooltip(t!("Next track"))
                    .ghost()
                    .with_size(px(18.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            );

        // 滚轮调音量后 1 秒内，进度线位置改为显示当前音量，给无面板的迷你窗口
        // 一个即时反馈；平时仍显示播放进度。
        let volume_flash = self.volume.mini_flash_visible(std::time::Instant::now());
        let (bar_fraction, bar_color) = if volume_flash {
            (
                self.volume.output_volume(self.config.volume),
                accent.opacity(0.95),
            )
        } else {
            (progress_percent / 100.0, accent.opacity(0.9))
        };
        let progress_line = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(2.5))
            .bg(accent.opacity(0.2))
            .child(
                div()
                    .h_full()
                    .rounded_r_full()
                    .w(relative(bar_fraction.clamp(0.0, 1.0)))
                    .bg(bar_color),
            );

        let expand_button = Button::new("mini-expand")
            .icon(AppIcon::Maximize)
            .tooltip(t!("Exit mini player"))
            .ghost()
            .with_size(px(18.0))
            .rounded_full()
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
            }));

        // The mini player root is a window drag area, so the expand button needs to
        // block the drag behind it (like the `controls` row) or its clicks would be
        // turned into window moves.
        let expand_button = h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(expand_button);

        v_flex()
            .relative()
            .size_full()
            .bg(background)
            .border_1()
            .border_color(theme.border.opacity(0.5))
            .text_color(theme.foreground)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if this.adjust_volume_from_scroll(event, window, cx) {
                    this.show_mini_volume_flash(cx);
                }
            }))
            .when(cfg!(target_os = "windows"), |this| {
                this.window_control_area(WindowControlArea::Drag)
            })
            .when(cfg!(not(target_os = "windows")), |this| {
                this.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .gap_1p5()
                    .items_center()
                    .child(cover_element)
                    .child(info)
                    .child(controls)
                    .child(expand_button),
            )
            .child(progress_line)
            .into_any_element()
    }
}
