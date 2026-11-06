//! Theme and interface language preferences.

use super::*;
use gpui_kit::component::menu::PopupMenuItem;

impl NavidromeApp {
    pub(super) fn render_appearance_settings(
        &self,
        cx: &Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        vec![
            self.render_theme_settings(cx),
            self.render_language_settings(cx),
        ]
    }

    fn render_theme_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t!("Theme")),
            )
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .border_t_1()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .child(
                        h_flex().gap_2().children(
                            [
                                ThemePreference::Light,
                                ThemePreference::Dark,
                                ThemePreference::System,
                            ]
                            .into_iter()
                            .map(|preference| {
                                // ID 用 Debug 名（Light/Dark/System），
                                // 不随界面语言变化。
                                Button::new(SharedString::from(format!("theme-{preference:?}")))
                                    .label(preference.label())
                                    .selected(self.config.theme == preference)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_theme(preference, window, cx);
                                    }))
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_language_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t!("Language")),
            )
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .p_4()
                    .border_t_1()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .child(
                        v_flex().flex_1().min_w_0().child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("Interface language. Changes apply instantly.")),
                        ),
                    )
                    .child({
                        let app = cx.entity().downgrade();
                        Button::new("language-select")
                            .w(px(220.0))
                            .justify_between()
                            .label(self.config.language.native_label())
                            .dropdown_caret(true)
                            .outline()
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
                    }),
            )
            .into_any_element()
    }
}
