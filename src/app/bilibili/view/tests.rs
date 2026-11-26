use super::*;
use gpui_kit::component::Root;
use gpui_kit::{TestAppContext, VisualTestContext};

struct LayoutProbe {
    width: f32,
    played: usize,
}

impl Render for LayoutProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let thumbnail = div()
            .w(px(tokens::VIDEO_HERO_COVER_COMPACT))
            .h(px(tokens::VIDEO_HERO_COVER_COMPACT * 9.0 / 16.0))
            .into_any_element();
        let continue_card = compact_continue_card(
            "很长的歌曲名Long video title".repeat(20).into(),
            "UP 主名称Long uploader".repeat(10).into(),
            thumbnail,
            round_play_button("probe-continue", PlayerIcon::Play, t!("Play"), 36.0)
                .debug_selector(|| "probe-continue".into())
                .on_click(cx.listener(|probe, _, _, cx| {
                    probe.played += 1;
                    cx.notify();
                })),
            None,
            true,
            cx,
        );
        let folders_card = div()
            .id("probe-folders")
            .debug_selector(|| "probe-folders".into())
            .w_full()
            .min_h(px(tokens::HERO_CARD_COMPACT_HEIGHT))
            .into_any_element();
        let recent = recent_play_cover(
            "probe-recent-cover".into(),
            228.0,
            div()
                .w(px(228.0))
                .h(px(228.0 * 9.0 / 16.0))
                .into_any_element(),
            Some(
                round_play_button(
                    "probe-recent-play",
                    PlayerIcon::Play,
                    t!("Play"),
                    tokens::HOVER_PLAY_BUTTON,
                )
                .debug_selector(|| "probe-recent-play".into())
                .on_click(cx.listener(|probe, _, _, cx| {
                    probe.played += 1;
                    cx.notify();
                })),
            ),
            None,
        );
        v_flex()
            .w(px(self.width))
            .h(px(600.0))
            .gap_4()
            .child(bilibili_home_lead_row(
                self.width,
                continue_card,
                folders_card,
            ))
            .child(recent)
    }
}

fn harness(cx: &mut TestAppContext, width: f32) -> (Entity<LayoutProbe>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| LayoutProbe { width, played: 0 });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    (probe.unwrap(), visual)
}

fn assert_inline_action(visual: &mut VisualTestContext) {
    let card = visual.debug_bounds("bilibili-continue-card").unwrap();
    let row = visual.debug_bounds("bilibili-continue-main").unwrap();
    let title = visual.debug_bounds("bilibili-continue-title").unwrap();
    let action = visual.debug_bounds("probe-continue").unwrap();
    assert_eq!(action.size.width, px(36.0));
    assert!(action.size.width < card.size.width / 4.0);
    assert!(action.origin.x > title.origin.x);
    assert!(action.origin.y >= row.origin.y);
    assert!(action.origin.y + action.size.height <= row.origin.y + row.size.height);
    assert!(
        title.size.height <= px(28.0),
        "long metadata must truncate rather than force a new action row"
    );
}

#[gpui_pre::test]
fn wide_home_pairs_continue_with_folder_shortcuts_and_keeps_action_inline(cx: &mut TestAppContext) {
    let (_, mut visual) = harness(cx, 680.0);
    let card = visual.debug_bounds("bilibili-continue-card").unwrap();
    let folders = visual.debug_bounds("probe-folders").unwrap();
    assert!(card.size.width < px(680.0 / 2.0));
    assert_eq!(card.origin.y, folders.origin.y);
    assert!(folders.origin.x >= card.origin.x + card.size.width);
    assert_inline_action(&mut visual);
}

#[gpui_pre::test]
fn narrow_content_stacks_cards_without_stretching_the_play_button(cx: &mut TestAppContext) {
    let (_, mut visual) = harness(cx, 520.0);
    let card = visual.debug_bounds("bilibili-continue-card").unwrap();
    let folders = visual.debug_bounds("probe-folders").unwrap();
    assert_eq!(card.size.width, px(520.0));
    assert!(folders.origin.y >= card.origin.y + card.size.height);
    assert_inline_action(&mut visual);
}

#[gpui_pre::test]
fn recent_play_action_is_inside_the_cover_and_still_clickable(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness(cx, 680.0);
    let cover = visual.debug_bounds("probe-recent-cover").unwrap();
    let action = visual.debug_bounds("probe-recent-play").unwrap();
    assert!(action.origin.x >= cover.origin.x && action.origin.y >= cover.origin.y);
    assert!(action.origin.x + action.size.width <= cover.origin.x + cover.size.width);
    assert!(action.origin.y + action.size.height <= cover.origin.y + cover.size.height);
    assert_eq!(
        action.origin.y + action.size.height,
        cover.origin.y + cover.size.height - px(8.0)
    );
    visual.simulate_click(action.center(), gpui_kit::Modifiers::default());
    visual.update(|_, cx| assert_eq!(probe.read(cx).played, 1));
}
