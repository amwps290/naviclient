//! Application integration tests for navigation and global theme adjustments.

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::{px, Hsla};

use crate::app::style::tokens::{palette, DARK, LIGHT};

fn close(a: Hsla, b: Hsla) -> bool {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
}

#[gpui_pre::test]
fn theme_overrides_follow_runtime_mode_changes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);

        let light = LIGHT.resolve();
        assert!(close(cx.theme().background, light.page));
        assert!(close(cx.theme().primary, light.brand));
        assert!(close(cx.theme().muted_foreground, light.text_secondary));
        // Surface buttons use the kit's default variant; capsule buttons use secondary.
        assert!(close(
            cx.theme().button,
            gpui_kit::rgb(LIGHT.button_surface).into()
        ));
        assert!(close(
            cx.theme().button_secondary,
            gpui_kit::rgb(LIGHT.button_tonal).into()
        ));
        assert!(close(cx.theme().button_secondary_foreground, light.text));
        assert!(close(cx.theme().button_primary, light.brand));
        assert!(close(cx.theme().button_primary_foreground, light.on_brand));
        assert!(close(palette(cx).page, light.page));
        assert_eq!(cx.theme().radius, px(10.0));
        assert_eq!(cx.theme().radius_lg, px(20.0));

        Theme::change(ThemeMode::Dark, None, cx);
        let dark = DARK.resolve();
        assert!(close(cx.theme().background, dark.page));
        assert!(close(cx.theme().primary, dark.brand));
        assert!(close(palette(cx).page, dark.page));
        assert_eq!(cx.theme().radius, px(10.0));

        Theme::change(ThemeMode::Light, None, cx);
        assert!(close(cx.theme().background, light.page));
        assert!(close(palette(cx).brand, light.brand));
    });
}

#[gpui_pre::test]
fn applying_theme_twice_changes_nothing(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);
        let first = cx.theme().primary;
        crate::app::apply_app_theme(cx);
        assert_eq!(cx.theme().primary, first);
    });
}

#[gpui_pre::test]
fn button_recipes_render_without_duplicate_hover_styles(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::style::buttons::{
        capsule_button, capsule_button_sm, capsule_icon_button, danger_button,
        danger_surface_button, ghost_button, icon_button, pill_filter_button, primary_button,
        round_play_button, surface_button, toggle_icon_button, ButtonExt,
    };
    use crate::assets::{AppIcon, PlayerIcon};
    use gpui_kit::component::Disableable;
    use gpui_kit::prelude::ParentElement;
    use gpui_kit::{div, Context, IntoElement, Render, Window};

    struct ButtonShowcase;
    impl Render for ButtonShowcase {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(round_play_button("rp", PlayerIcon::Play, "Play", 40.0, cx))
                .child(icon_button("ic", PlayerIcon::Play, "Play", 28.0))
                .child(icon_button("ic-dis", PlayerIcon::Play, "Play", 28.0).disabled(true))
                .child(toggle_icon_button(
                    "tg-off",
                    AppIcon::Shuffle,
                    "Shuffle",
                    false,
                    32.0,
                    cx,
                ))
                .child(toggle_icon_button(
                    "tg-on",
                    AppIcon::Shuffle,
                    "Shuffle",
                    true,
                    32.0,
                    cx,
                ))
                .child(capsule_button("cap").label("Append"))
                .child(capsule_button_sm("cap-sm").label("Append"))
                .child(capsule_icon_button("cap-ic", AppIcon::Queue, "Queue", 32.0))
                .child(pill_filter_button("pill-off", "All", false, cx))
                .child(pill_filter_button("pill-on", "All", true, cx))
                .child(primary_button("pri").label("Save").compact_action())
                .child(surface_button("sur").label("Open").compact_action())
                .child(ghost_button("gho").label("Delete").compact_action())
                .child(danger_button("dan").label("Remove").compact_action())
                .child(
                    danger_surface_button("dsur")
                        .label("Clear")
                        .compact_action(),
                )
        }
    }

    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);
    });
    let (_, window) = cx.add_window_view(|_, _| ButtonShowcase);
    window.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

#[gpui_pre::test]
fn song_row_and_playlist_buttons_render_without_hover_panic(cx: &mut gpui_kit::TestAppContext) {
    use crate::app::style::buttons::{ghost_button, icon_button, ButtonExt};
    use crate::assets::PlayerIcon;
    use gpui_kit::component::Disableable;
    use gpui_kit::prelude::{ParentElement, Styled};
    use gpui_kit::{div, px, Context, IntoElement, Render, Window};

    struct RowButtonsProbe;
    impl Render for RowButtonsProbe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let p = palette(cx);
            div()
                .child(
                    icon_button("play-song-row", PlayerIcon::Play, "Play", 26.0)
                        .disabled(false)
                        .text_color(p.text_secondary),
                )
                .child(
                    ghost_button("delete-playlist")
                        .label("Delete")
                        .rounded(px(9999.0))
                        .compact_action()
                        .text_color(cx.theme().danger)
                        .disabled(false),
                )
        }
    }

    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);
    });
    let (_, window) = cx.add_window_view(|_, _| RowButtonsProbe);
    window.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}
