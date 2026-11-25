use super::*;
use gpui_kit::component::Root;
use gpui_kit::{TestAppContext, VisualTestContext};

fn layout(collapsed: bool, automatic: bool) -> ShellLayout {
    ShellLayout {
        compact: automatic,
        sidebar_collapsed: collapsed,
        auto_collapsed: automatic,
        sidebar_width: if collapsed {
            tokens::SIDEBAR_COLLAPSED_WIDTH
        } else {
            tokens::SIDEBAR_WIDTH
        },
        queue_drawer_width: tokens::QUEUE_DRAWER_WIDTH,
        player_margin: tokens::PLAYER_MARGIN,
        page_padding: tokens::PAGE_PADDING,
        content_width: 800.0,
    }
}

struct SidebarProbe {
    layout: ShellLayout,
}

impl Render for SidebarProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(tokens::SIDEBAR_WIDTH))
            .h(px(300.0))
            .child(bilibili_sidebar_content(
                &self.layout,
                format!("Bilibili · {}", "很长的账号名称LongAccountName".repeat(10)).into(),
                div()
                    .id("sidebar-probe-navigation")
                    .debug_selector(|| "sidebar-probe-navigation".into())
                    .w_full()
                    .h(px(34.0))
                    .into_any_element(),
                div()
                    .id("sidebar-probe-footer")
                    .debug_selector(|| "sidebar-probe-footer".into())
                    .mt_auto()
                    .w_full()
                    .h(px(34.0))
                    .into_any_element(),
            ))
    }
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn harness(
    cx: &mut TestAppContext,
    initial: ShellLayout,
) -> (Entity<SidebarProbe>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| SidebarProbe { layout: initial });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    (probe.unwrap(), visual)
}

fn assert_collapsed(visual: &mut VisualTestContext) {
    assert!(
        visual.debug_bounds("bilibili-source-label").is_none(),
        "account text must never wrap down the icon rail"
    );
    let sidebar = visual.debug_bounds("bilibili-sidebar").unwrap();
    let navigation = visual.debug_bounds("sidebar-probe-navigation").unwrap();
    let footer = visual.debug_bounds("sidebar-probe-footer").unwrap();
    assert_eq!(sidebar.size.width, px(tokens::SIDEBAR_COLLAPSED_WIDTH));
    assert_eq!(
        navigation.size.width,
        px(40.0),
        "56px rail uses 8px padding on each side"
    );
    assert_eq!(footer.size.width, navigation.size.width);
    assert_eq!(navigation.origin.x, sidebar.origin.x + px(8.0));
    assert_eq!(footer.origin.x, navigation.origin.x);
    assert!(footer.origin.y + footer.size.height <= sidebar.origin.y + sidebar.size.height);
}

#[gpui_pre::test]
fn collapsed_bilibili_sidebar_keeps_account_text_out_of_the_icon_rail(cx: &mut TestAppContext) {
    let (_, mut visual) = harness(cx, layout(true, false));
    assert_collapsed(&mut visual);
}

#[gpui_pre::test]
fn expanded_bilibili_sidebar_keeps_long_account_name_on_one_line(cx: &mut TestAppContext) {
    let (_, mut visual) = harness(cx, layout(false, false));
    let label = visual.debug_bounds("bilibili-source-label").unwrap();
    let navigation = visual.debug_bounds("sidebar-probe-navigation").unwrap();
    assert_eq!(label.size.width, px(176.0));
    assert!(
        label.size.height <= px(24.0),
        "long Chinese/Latin names must be truncated, not wrap"
    );
    assert!(navigation.origin.y >= label.origin.y + label.size.height);
}

#[gpui_pre::test]
fn manual_and_automatic_collapse_share_layout_and_restore_the_header(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness(cx, layout(false, false));
    for automatic in [false, true] {
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.layout = layout(true, automatic);
                cx.notify();
            })
        });
        draw(&mut visual);
        assert_collapsed(&mut visual);
    }
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            probe.layout = layout(false, false);
            cx.notify();
        })
    });
    draw(&mut visual);
    assert!(visual.debug_bounds("bilibili-source-label").is_some());
    assert_eq!(
        visual.debug_bounds("bilibili-sidebar").unwrap().size.width,
        px(tokens::SIDEBAR_WIDTH)
    );
}
