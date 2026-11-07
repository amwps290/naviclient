//! The header shared by album, artist and playlist detail pages. It bleeds to
//! the content edges, so these pages pad the content below it themselves.

use crate::app::prelude::*;
use crate::app::style::cover_tint::{tint_background, TintSurface};

pub(in crate::app) struct DetailHero {
    pub(in crate::app) kind: SharedString,
    pub(in crate::app) title: SharedString,
    pub(in crate::app) cover_id: Option<String>,
    pub(in crate::app) round_cover: bool,
    pub(in crate::app) meta: gpui_kit::AnyElement,
    pub(in crate::app) actions: gpui_kit::AnyElement,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HeroSizes {
    cover: f32,
    title: f32,
}

fn hero_sizes(viewport_width: f32, viewport_height: f32, round_cover: bool) -> HeroSizes {
    let compact = viewport_width < tokens::DETAIL_COMPACT_WIDTH
        || viewport_height < tokens::DETAIL_COMPACT_HEIGHT;
    HeroSizes {
        cover: match (round_cover, compact) {
            (false, false) => tokens::DETAIL_COVER,
            (false, true) => tokens::DETAIL_COVER_COMPACT,
            (true, false) => tokens::DETAIL_ARTIST_COVER,
            (true, true) => tokens::DETAIL_ARTIST_COVER_COMPACT,
        },
        title: if compact {
            tokens::DETAIL_TITLE_COMPACT
        } else {
            tokens::DETAIL_TITLE
        },
    }
}

impl NavidromeApp {
    pub(in crate::app) fn render_detail_hero(
        &self,
        hero: DetailHero,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let viewport = window.viewport_size();
        let sizes = hero_sizes(
            f32::from(viewport.width),
            f32::from(viewport.height),
            hero.round_cover,
        );
        let padding = self.shell_layout(window).page_padding;
        let tint = self
            .tint_for_cover(TintSurface::DetailHeader, hero.cover_id.as_deref(), cx)
            .map(|(start, _)| (start, cx.theme().background));
        h_flex()
            .w_full()
            .flex_none()
            .items_end()
            .gap_6()
            .p(px(padding))
            .bg(tint_background(tint, cx.theme().background, 180.0))
            .child(self.render_cover_styled(
                hero.cover_id.as_deref(),
                sizes.cover,
                if hero.round_cover {
                    tokens::radius::FULL
                } else {
                    tokens::radius::COVER
                },
                cover_shadow(false, cx),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(hero.kind),
                    )
                    .child(serif_heading(hero.title, sizes.title, cx).truncate())
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(tokens::BODY))
                            .text_color(p.text_secondary)
                            .child(hero.meta),
                    )
                    .child(
                        h_flex()
                            .pt_2()
                            .gap_2()
                            .flex_wrap()
                            .items_center()
                            .child(hero.actions),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
