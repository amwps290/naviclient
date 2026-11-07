//! Loading placeholders for album, artist and song lists.

use crate::app::prelude::*;
use crate::app::widgets::shelf::cover_grid;

/// Skeleton element with subtle shimmer animation.
fn skeleton_bar(
    id: impl Into<SharedString>,
    width: impl Into<gpui_kit::Length>,
    height: impl Into<gpui_kit::Pixels>,
    rounded: impl Into<gpui_kit::Pixels>,
    sunken: gpui_kit::Hsla,
) -> gpui_kit::AnyElement {
    let id: SharedString = id.into();
    let width = width.into();
    let height = height.into();
    let rounded = rounded.into();
    let shimmer = Animation::new(Duration::from_secs_f32(1.5))
        .repeat()
        .with_easing(|t| {
            // Ease in-out for smooth breathing effect
            let x = (t * std::f32::consts::PI * 2.0).sin();
            0.5 + x * 0.15
        });
    div()
        .w(width)
        .h(height)
        .rounded(rounded)
        .bg(sunken)
        .with_animation(id, shimmer, move |this, opacity| {
            this.opacity(opacity)
        })
        .into_any_element()
}

impl NavidromeApp {
    /// 专辑网格骨架屏：数据加载期间占位，避免空页面跳动。
    pub(in crate::app) fn render_album_skeleton_grid(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let sunken = palette(cx).sunken;
        cover_grid(
            columns,
            (0..columns.count * 2).map(|i| {
                v_flex()
                    .gap_2()
                    .child(skeleton_bar(
                        format!("skel-album-cover-{i}"),
                        px(columns.width),
                        px(columns.width),
                        px(tokens::radius::COVER),
                        sunken,
                    ))
                    .child(skeleton_bar(
                        format!("skel-album-title-{i}"),
                        relative(0.7),
                        px(12.0),
                        px(6.0),
                        sunken,
                    ))
                    .child(skeleton_bar(
                        format!("skel-album-meta-{i}"),
                        relative(0.5),
                        px(10.0),
                        px(5.0),
                        sunken,
                    ))
                    .into_any_element()
            }),
        )
        .into_any_element()
    }

    /// 列表行骨架屏：播放列表 / 歌曲列表加载期间占位。
    pub(in crate::app) fn render_list_skeleton(
        &self,
        rows: usize,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let sunken = palette(cx).sunken;
        v_flex()
            .gap_2()
            .children((0..rows).map(|i| {
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(skeleton_bar(
                        format!("skel-list-cover-{i}"),
                        px(40.0),
                        px(40.0),
                        px(tokens::radius::CONTROL),
                        sunken,
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(skeleton_bar(
                                format!("skel-list-title-{i}"),
                                px(240.0),
                                px(12.0),
                                px(6.0),
                                sunken,
                            ))
                            .child(skeleton_bar(
                                format!("skel-list-meta-{i}"),
                                px(120.0),
                                px(10.0),
                                px(5.0),
                                sunken,
                            )),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 艺术家网格骨架屏：与艺术家页的圆形图片一致。
    pub(in crate::app) fn render_artist_skeleton_grid(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let sunken = palette(cx).sunken;
        cover_grid(
            columns,
            (0..columns.count * 2).map(|i| {
                v_flex()
                    .gap_2()
                    .child(skeleton_bar(
                        format!("skel-artist-cover-{i}"),
                        px(columns.width),
                        px(columns.width),
                        px(9999.0),
                        sunken,
                    ))
                    .child(skeleton_bar(
                        format!("skel-artist-name-{i}"),
                        relative(0.6),
                        px(12.0),
                        px(6.0),
                        sunken,
                    ))
                    .into_any_element()
            }),
        )
        .into_any_element()
    }
}
