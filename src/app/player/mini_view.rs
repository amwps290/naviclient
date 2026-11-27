//! Compact capsule cover, metadata, inline transport and seek controls for the mini window.

use gpui_kit::base::{Slider as BaseSlider, SliderIndicator, SliderTrack};

use crate::app::prelude::*;
use crate::app::style::cover_tint::{cover_tint, TintSurface};

const COVER_SIZE: f32 = 44.0;
const COVER_INSET: f32 = 1.5;
const CONTENT_LEFT: f32 = 6.0;
const CONTENT_RIGHT: f32 = 10.0;
const COVER_GAP: f32 = 9.0;
const CONTROLS_GAP: f32 = 6.0;
const META_GAP: f32 = 5.0;
const SKIP_BUTTON_SIZE: f32 = 22.0;
const PLAY_BUTTON_SIZE: f32 = 28.0;
const CONTROLS_INNER_GAP: f32 = 2.0;
const CONTROLS_WIDTH: f32 = SKIP_BUTTON_SIZE * 2.0 + PLAY_BUTTON_SIZE + CONTROLS_INNER_GAP * 2.0;

impl NavidromeApp {
    pub(in crate::app) fn render_mini(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let colors = palette(cx);
        let song = self.state.now_playing.clone();
        let has_track = song.is_some();
        let playback = self.playback_for_display();
        let is_playing = playback.active && !playback.paused;
        let duration = playback.duration.unwrap_or_default();
        let progress = if duration.is_zero() {
            0.0
        } else {
            (playback.position.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
        };

        let cover = song
            .as_ref()
            .and_then(|song| self.song_cover_image(song))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        let tint = cover_tint(
            TintSurface::MiniPlayer,
            self.now_playing_cover_palette(),
            theme.background,
            theme.is_dark(),
        )
        .unwrap_or((colors.brand_soft, colors.sunken));
        let border = if theme.is_dark() {
            colors.border
        } else {
            hsla(0.0, 0.0, 1.0, 0.42)
        };
        let surface_cache = self.mini_window.surface.clone();
        let surface = gpui_kit::canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let scale = window.scale_factor();
                let width = (f32::from(bounds.size.width) * scale).round().max(1.0) as u32;
                let height = (f32::from(bounds.size.height) * scale).round().max(1.0) as u32;
                let image = surface_cache
                    .borrow_mut()
                    .image(width, height, scale, tint, border, window);
                if let Err(error) = window.paint_image(
                    bounds,
                    bounds,
                    gpui_kit::Corners::all(px(0.0)),
                    image,
                    0,
                    false,
                ) {
                    log::warn!("failed to paint mini surface: {error}");
                }
            },
        )
        .absolute()
        .inset_0();

        // The shared cache retains the angle when paused; the shell advances it.
        let artwork_size = COVER_SIZE - COVER_INSET * 2.0;
        let artwork = self
            .vinyl_cover
            .render(artwork_size, self.vinyl_cover.phase())
            .unwrap_or_else(|| {
                self.render_cover_image(cover, artwork_size, artwork_size * 0.5, Vec::new())
            });
        let cover_button = div()
            .size(px(COVER_SIZE))
            .flex_none()
            .rounded_full()
            .shadow(cover_shadow(is_playing, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .child(
                Button::new("mini-cover")
                    .accessibility_label(t!("Exit mini player"))
                    .tooltip(t!("Exit mini player"))
                    .ghost()
                    .rounded_full()
                    .cursor_pointer()
                    .size(px(COVER_SIZE))
                    .p_0()
                    .child(mini_cover_frame(artwork, cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.exit_mini_mode(window, cx);
                        this.open_now_playing();
                    })),
            );

        let title = song
            .as_ref()
            .map(|song| song.title.clone())
            .unwrap_or_else(|| t!("No track playing").to_string());
        let artist = if let Some(stall) = playback.stall {
            t!("Buffering… %{seconds}s", seconds = stall.as_secs()).to_string()
        } else if playback.error.is_some() {
            t!("Playback failed — open for details").to_string()
        } else {
            song.as_ref()
                .map(|song| {
                    if let Some(part) = &song.bilibili_part {
                        song.bilibili_subtitle().unwrap_or_else(|| {
                            t!("Uploader: %{name}", name = part.uploader).to_string()
                        })
                    } else {
                        song.artist.clone()
                    }
                })
                .unwrap_or_else(|| t!("Choose a song").to_string())
        };

        // Wheel changes briefly replace both time and progress with volume.
        let volume_flash = self.volume.mini_flash_visible(std::time::Instant::now());
        let volume = self.volume.output_volume(self.config.volume);
        let time = if volume_flash {
            format!("{}%", (volume * 100.0).round() as u32)
        } else {
            format_playback(playback.position)
        };
        let time_width = (time.len() as f32 * 5.6).max(26.0);
        let paddings = window_paddings(window);
        let content_width = window.viewport_size().width - paddings.left - paddings.right;
        let info_width = f32::from(
            content_width
                - px(2.0
                    + CONTENT_LEFT
                    + COVER_SIZE
                    + COVER_GAP
                    + CONTROLS_GAP
                    + CONTROLS_WIDTH
                    + CONTENT_RIGHT),
        )
        .max(1.0);
        let artist_width = (info_width - time_width - META_GAP).max(1.0);
        let track_id = song
            .as_ref()
            .map(|song| song.id.as_str())
            .unwrap_or_default();
        let title_id = SharedString::from(format!("mini-title-{track_id}"));
        let artist_id = SharedString::from(format!("mini-artist-{track_id}"));
        let title_tooltip = title.clone();
        let artist_tooltip = artist.clone();

        let controls = h_flex()
            .flex_none()
            .pb(px(3.0))
            .gap(px(CONTROLS_INNER_GAP))
            .items_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .child(
                Button::new("mini-previous")
                    .child(Icon::new(PlayerIcon::Previous).with_size(px(12.0)))
                    .accessibility_label(t!("Previous track"))
                    .tooltip(t!("Previous track"))
                    .ghost()
                    .w(px(SKIP_BUTTON_SIZE))
                    .h(px(SKIP_BUTTON_SIZE))
                    .p_0()
                    .rounded_full()
                    .cursor_pointer()
                    .text_color(colors.text_secondary)
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                mini_play_button(is_playing, has_track, cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    },
                )),
            )
            .child(
                Button::new("mini-next")
                    .child(Icon::new(PlayerIcon::Next).with_size(px(12.0)))
                    .accessibility_label(t!("Next track"))
                    .tooltip(t!("Next track"))
                    .ghost()
                    .w(px(SKIP_BUTTON_SIZE))
                    .h(px(SKIP_BUTTON_SIZE))
                    .p_0()
                    .rounded_full()
                    .cursor_pointer()
                    .text_color(colors.text_secondary)
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            );

        let info = v_flex()
            .flex_1()
            .min_w_0()
            .pb(px(3.0))
            .child(
                div()
                    .id("mini-title")
                    .h(px(16.0))
                    .line_height(px(16.0))
                    .tooltip(move |window, cx| {
                        Tooltip::new(title_tooltip.clone()).build(window, cx)
                    })
                    .child(self.render_scrolling_text(
                        title.into(),
                        info_width,
                        px(12.5),
                        FontWeight::SEMIBOLD,
                        title_id,
                        window,
                    )),
            )
            .child(
                h_flex()
                    .h(px(14.0))
                    .mt(px(1.0))
                    .gap(px(META_GAP))
                    .items_center()
                    .text_color(colors.text_secondary)
                    .child(
                        div()
                            .id("mini-artist")
                            .flex_1()
                            .min_w_0()
                            .line_height(px(14.0))
                            .tooltip(move |window, cx| {
                                Tooltip::new(artist_tooltip.clone()).build(window, cx)
                            })
                            .child(self.render_scrolling_text(
                                artist.into(),
                                artist_width,
                                px(10.0),
                                FontWeight::NORMAL,
                                artist_id,
                                window,
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(time_width))
                            .text_size(px(9.0))
                            .line_height(px(14.0))
                            .text_right()
                            .child(time),
                    ),
            );

        // The shared slider's transport subscription handles seeking. A 14px
        // hit track tucked along the capsule's flat bottom owns clicks/drags
        // without overlapping the disc or inline transport buttons.
        let seek_disabled = !has_track || duration.is_zero();
        let bar_fraction = if volume_flash { volume } else { progress };
        let progress_line = mini_seek_control(
            &self.playback_slider,
            seek_disabled,
            bar_fraction,
            colors.brand,
            if is_playing || volume_flash {
                0.68
            } else {
                0.35
            },
        );

        v_flex()
            .relative()
            .size_full()
            .rounded(px(super::mini_surface::RADIUS))
            .overflow_hidden()
            // Retain the former border's layout inset. Padding lets the
            // absolute surface reach the outer bounds without border clipping.
            .p(px(1.0))
            .text_color(colors.text)
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
            .child(surface)
            .child(
                h_flex()
                    .size_full()
                    .pl(px(CONTENT_LEFT))
                    .pr(px(CONTENT_RIGHT))
                    .items_center()
                    .child(cover_button)
                    .child(div().w(px(COVER_GAP)).flex_none())
                    .child(info)
                    .child(div().w(px(CONTROLS_GAP)).flex_none())
                    .child(controls),
            )
            .child(progress_line)
            .into_any_element()
    }
}

fn mini_play_button(playing: bool, has_track: bool, cx: &App) -> Button {
    let colors = palette(cx);
    let label = if playing { t!("Pause") } else { t!("Play") };
    let background = if cx.theme().is_dark() {
        colors.text
    } else {
        gpui_kit::rgb(tokens::MINI_PLAY_BACKGROUND).into()
    };
    let foreground = colors.page;
    let hover = background.blend(colors.brand.opacity(0.16));
    // Button owns its hover/active registration. Supply colors through its
    // variant instead of adding another InteractiveElement::hover style.
    let style = ButtonCustomVariant::new(cx)
        .color(background)
        .foreground(foreground)
        .hover(hover)
        .active(background.blend(colors.brand.opacity(0.24)))
        .shadow(false);
    Button::new("mini-play-pause")
        .child(
            Icon::new(if playing {
                PlayerIcon::Pause
            } else {
                PlayerIcon::Play
            })
            .with_size(px(11.5)),
        )
        .accessibility_label(label.clone())
        .tooltip(label)
        .custom(style)
        .w(px(PLAY_BUTTON_SIZE))
        .h(px(PLAY_BUTTON_SIZE))
        .p_0()
        .rounded_full()
        .cursor_pointer()
        .bg(background)
        .text_color(foreground)
        .disabled(!has_track)
        .when(!has_track, |this| this.opacity(0.45))
}

#[cfg(test)]
fn mini_control_slot(button: Button) -> gpui_kit::Div {
    h_flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .justify_center()
        .child(button)
}

fn mini_cover_frame(artwork: gpui_kit::AnyElement, cx: &App) -> gpui_kit::Div {
    let colors = palette(cx);
    div()
        .relative()
        .size(px(COVER_SIZE))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(
            135.0,
            linear_color_stop(colors.text_muted.opacity(0.35), 0.0),
            linear_color_stop(colors.raised.opacity(0.55), 1.0),
        ))
        .child(
            div()
                .absolute()
                .inset(px(COVER_INSET))
                .rounded_full()
                .child(artwork),
        )
        .child(
            div()
                .absolute()
                .left(px((COVER_SIZE - 4.0) / 2.0))
                .top(px((COVER_SIZE - 4.0) / 2.0))
                .size(px(4.0))
                .rounded_full()
                .border_1()
                .border_color(hsla(0.0, 0.0, 0.85, 1.0))
                .bg(hsla(0.0, 0.0, 0.25, 1.0)),
        )
}

fn mini_seek_control(
    slider: &Entity<SliderState>,
    disabled: bool,
    fraction: f32,
    accent: Hsla,
    opacity: f32,
) -> gpui_kit::Div {
    div()
        .absolute()
        .left(px(58.0))
        .right(px(28.0))
        .bottom(px(0.0))
        .h(px(14.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .occlude()
        .child(
            BaseSlider::new(slider)
                .disabled(disabled)
                .size_full()
                .child(
                    SliderTrack::new(slider)
                        .disabled(disabled)
                        .relative()
                        .size_full()
                        .flex()
                        .items_end()
                        .pb(px(3.5))
                        .when(!disabled, |this| this.cursor_pointer())
                        .child(
                            SliderIndicator::new(slider)
                                .relative()
                                .w_full()
                                .h(px(2.0))
                                .rounded_full()
                                .bg(accent.opacity(0.14))
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(fraction.clamp(0.0, 1.0)))
                                        .rounded_full()
                                        .bg(accent.opacity(opacity)),
                                ),
                        ),
                ),
        )
}

#[cfg(test)]
mod tests;
