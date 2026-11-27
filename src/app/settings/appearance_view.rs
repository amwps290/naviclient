//! Theme and interface language preferences.

use super::components::{settings_card, settings_group, settings_group_header, settings_row};
use crate::app::prelude::*;
use gpui_kit::component::menu::PopupMenuItem;

impl NavidromeApp {
    pub(in crate::app) fn render_appearance_settings(
        &self,
        cx: &Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        vec![
            self.render_theme_settings(cx),
            self.render_language_settings(cx),
        ]
    }

    fn render_theme_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();

        let cards = [
            ThemePreference::Light,
            ThemePreference::Dark,
            ThemePreference::System,
        ]
        .into_iter()
        .map(|preference| {
            let selected = self.config.theme == preference;
            let swatch = self.render_theme_swatch(preference, cx);
            // ID 用 Debug 名（Light/Dark/System），不随界面语言变化。
            div()
                .id(SharedString::from(format!("theme-{preference:?}")))
                .flex_1()
                .min_w(px(130.0))
                .cursor_pointer()
                .rounded(px(tokens::radius::COVER))
                .p_2p5()
                .flex()
                .flex_col()
                .gap_2p5()
                .bg(if selected {
                    p.brand_soft.opacity(if dark { 0.80 } else { 0.85 })
                } else {
                    p.raised
                })
                .border_1()
                .border_color(if selected {
                    p.brand
                } else if dark {
                    hsla(0.0, 0.0, 1.0, 0.08)
                } else {
                    p.border.opacity(0.85)
                })
                .when(!selected, |card| {
                    card.hover(|card| card.bg(p.sunken.opacity(0.8)))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_theme(preference, window, cx);
                }))
                .child(swatch)
                .child(
                    h_flex()
                        .w_full()
                        .px_0p5()
                        .justify_between()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(if selected {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(if selected { p.brand } else { p.text })
                                .child(preference.label()),
                        )
                        .child(
                            div()
                                .size(px(14.0))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_1()
                                .border_color(if selected { p.brand } else { p.border })
                                .when(selected, |dot| {
                                    dot.bg(p.brand)
                                        .child(div().size(px(5.0)).rounded_full().bg(p.on_brand))
                                }),
                        ),
                )
        });

        v_flex()
            .w_full()
            .gap_2()
            .child(settings_group_header(t!("Theme"), None, cx))
            .child(
                settings_card(cx)
                    .p_3()
                    .child(h_flex().w_full().gap_3().children(cards)),
            )
            .into_any_element()
    }

    /// Miniature window preview for each theme card.
    fn render_theme_swatch(&self, preference: ThemePreference, cx: &App) -> gpui_kit::AnyElement {
        let light = tokens::LIGHT.resolve();
        let dark = tokens::DARK.resolve();
        let border = if cx.theme().is_dark() {
            hsla(0.0, 0.0, 1.0, 0.10)
        } else {
            palette(cx).border
        };

        let mini_pane = |pal: tokens::Palette| {
            h_flex()
                .size_full()
                .bg(pal.page)
                .p_1p5()
                .gap_1p5()
                .child(
                    v_flex()
                        .w(px(22.0))
                        .h_full()
                        .gap_1()
                        .p_1()
                        .rounded(px(4.0))
                        .bg(pal.sunken)
                        .child(div().w_full().h(px(4.0)).rounded_full().bg(pal.brand))
                        .child(
                            div()
                                .w(px(12.0))
                                .h(px(3.0))
                                .rounded_full()
                                .bg(pal.text_muted.opacity(0.45)),
                        )
                        .child(
                            div()
                                .w(px(10.0))
                                .h(px(3.0))
                                .rounded_full()
                                .bg(pal.text_muted.opacity(0.30)),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .w(px(36.0))
                                        .h(px(5.0))
                                        .rounded_full()
                                        .bg(pal.text.opacity(0.75)),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(div().size(px(16.0)).rounded(px(4.0)).bg(pal.raised))
                                        .child(
                                            div()
                                                .size(px(16.0))
                                                .rounded(px(4.0))
                                                .bg(pal.brand_soft),
                                        ),
                                ),
                        )
                        .child(div().w_full().h(px(8.0)).rounded(px(4.0)).bg(pal.raised)),
                )
        };

        let preview = div()
            .w_full()
            .h(px(62.0))
            .rounded(px(tokens::radius::CONTROL))
            .border_1()
            .border_color(border)
            .overflow_hidden();

        match preference {
            ThemePreference::Light => preview.child(mini_pane(light)).into_any_element(),
            ThemePreference::Dark => preview.child(mini_pane(dark)).into_any_element(),
            ThemePreference::System => preview
                .child(
                    h_flex()
                        .size_full()
                        .child(
                            div()
                                .w(relative(0.5))
                                .h_full()
                                .overflow_hidden()
                                .child(mini_pane(light)),
                        )
                        .child(
                            div()
                                .w(relative(0.5))
                                .h_full()
                                .overflow_hidden()
                                .child(mini_pane(dark)),
                        ),
                )
                .into_any_element(),
        }
    }

    fn render_language_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let picker = surface_button("language-select")
            .w(px(220.0))
            .justify_between()
            .label(self.config.language.native_label())
            .dropdown_caret(true)
            .compact_action()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for preference in LanguagePreference::ALL {
                        menu = menu.item(
                            PopupMenuItem::new(preference.native_label())
                                .checked(this.config.language == preference)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_language(preference, cx);
                                })),
                        );
                    }
                    menu
                })
                .expect("app owns language menu")
            })
            .into_any_element();

        settings_group(
            Some(t!("Language").into()),
            None,
            vec![settings_row(
                t!("Language"),
                Some(t!("Interface language. Changes apply instantly.").into()),
                picker,
                cx,
            )],
            cx,
        )
    }
}
