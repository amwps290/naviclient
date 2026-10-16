//! 输入框按键路径的回归测试：聚焦的输入框必须能接收字母与空格。
//!
//! 回归背景：输入框外层包一个 `on_key_down(stop_propagation)` 的 wrapper
//! 会阻断事件传播，而 Linux X11/Wayland 平台对“未被消费按键”的 key_char
//! 文本插入回退恰恰要求 propagate==true——英文/空格（单字节）因此全部
//! 丢失；只有多字节 IME 提交（中文）走 handle_ime 路径不受影响，于是
//! 表现为“中文能打、英文和空格打不进”。全局快捷键本就有
//! `has_focused_input` 守卫，wrapper 是冗余防御，已从应用中移除。

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Root;
use gpui_kit::{
    div, AppContext as _, Entity, Focusable, IntoElement, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window,
};

struct SearchProbe {
    input: Entity<InputState>,
}

impl Render for SearchProbe {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        div().flex_1().child(Input::new(&self.input).w_full())
    }
}

fn harness(cx: &mut TestAppContext) -> (Entity<InputState>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut input = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let state = cx.new(|cx| InputState::new(window, cx));
            input = Some(state.clone());
            let probe = cx.new(|_| SearchProbe {
                input: state.clone(),
            });
            cx.new(|cx| Root::new(probe, window, cx))
        })
        .unwrap()
    });
    let input = input.unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.update(|window, cx| input.read(cx).focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    (input, cx)
}

#[gpui_pre::test]
fn focused_input_accepts_letters_and_space(cx: &mut TestAppContext) {
    let (input, mut cx) = harness(cx);
    // 注意：simulate_keystrokes 按空格分词，空格本身要写成 "space"。
    cx.simulate_keystrokes("a space b space c");
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let value = cx.update(|_, cx| input.read(cx).value().to_string());
    assert_eq!(value, "a b c", "聚焦的输入框应能输入字母与空格");
}

#[gpui_pre::test]
fn focused_input_is_registered_for_shortcut_guards(cx: &mut TestAppContext) {
    // 全局快捷键的守卫依赖输入框聚焦时注册到窗口状态：
    // 该注册随渲染刷新，聚焦后必须立即可见。
    let (input, mut cx) = harness(cx);
    cx.update(|window, cx| {
        use gpui_kit::component::WindowExt as _;
        assert!(input.read(cx).focus_handle(cx).is_focused(window));
        assert!(
            window.has_focused_input(cx),
            "聚焦的输入框应注册到窗口状态，供快捷键守卫识别"
        );
    });
}
