//! Server selection rows and inline edit/removal actions.

use crate::app::prelude::*;

impl NavidromeApp {
    /// 已保存服务器列表：点击切换，右侧按钮删除条目。
    /// 这是多服务器的主要入口，排在编辑表单之前。
    /// 服务器列表：一行一台，整行可点即切换（比在行尾找小按钮快），
    /// 只有删除按钮留在行尾。选中项用左侧实心点 + 高亮底色表示。
    pub(in crate::app) fn render_server_list(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let active = self.config.active_server.clone();
        let can_remove = self.config.servers.len() > 1;
        v_flex()
            .w_full()
            .gap_1()
            .children(self.config.servers.iter().map(|profile| {
                let switch_id = profile.id.clone();
                let remove_id = profile.id.clone();
                let edit_id = profile.id.clone();
                let is_active = active.as_deref() == Some(profile.id.as_str());
                let label = profile.label();
                let detail = if profile.is_configured() {
                    t!(
                        "%{url} · %{user}",
                        url = profile.url,
                        user = profile.username
                    )
                    .to_string()
                } else {
                    t!("Not configured yet").to_string()
                };
                let switchable = !is_active && profile.is_configured();
                let tooltip = if is_active {
                    t!("Currently connected").to_string()
                } else if switchable {
                    t!(
                        "Switch to %{label}\n%{detail}",
                        label = label,
                        detail = detail
                    )
                    .to_string()
                } else {
                    t!("%{label} is not configured yet", label = label).to_string()
                };
                h_flex()
                    .id(SharedString::from(format!("server-row-{switch_id}")))
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(if is_active {
                        // 选中态的边框用框架的 list_active_border（亮色 blue-400、
                        // 深色 blue-700），与行底同一个蓝色族，不再是灰上加灰。
                        cx.theme().list_active_border.opacity(0.55)
                    } else {
                        cx.theme().border.opacity(0.7)
                    })
                    // 选中行底色沿用框架的选中语义色 list_active——accent 是
                    // 背景 token，0.07 叠在卡片底上等于没叠（与歌曲行同一教训）。
                    .bg(if is_active {
                        cx.theme().list_active
                    } else {
                        cx.theme().background
                    })
                    .when(switchable, |this| {
                        // hover 预演选中态：从白色走向同一片蓝色，
                        // 而不是把鼠标经过变成一次无意义的灰色闪光。
                        this.cursor_pointer()
                            .hover(|style| style.bg(cx.theme().list_active))
                    })
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    // 当前连接由行底、图标与行尾胶囊三重表达，
                    // 原来那个 6px 圆点在这里只是噪声，已移除。
                    .child(
                        Icon::new(if is_active {
                            AppIcon::Server
                        } else {
                            AppIcon::ServerOff
                        })
                        .with_size(px(14.0))
                        // 图标绝不能再用 accent：它是背景 token，亮色主题下
                        // ≈#f5f5f5，与选中行底只差 1/255，看起来就是“图标不显示”。
                        .text_color(if is_active {
                            cx.theme().primary
                        } else {
                            cx.theme().muted_foreground
                        }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(if is_active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .truncate()
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .items_center()
                            .when(is_active, |this| {
                                this.child(
                                    // 胶囊：实心主色 + 主色前景，而不是半透明灰底
                                    // 靠 4px 小圆角硬拗的“标签”。用 primary /
                                    // primary_foreground 一对 token，亮暗主题都
                                    // 自动拿到足够对比度（≈17:1）。
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .h(px(20.0))
                                        .px(px(8.0))
                                        .rounded(px(999.0))
                                        .bg(cx.theme().primary)
                                        .text_color(cx.theme().primary_foreground)
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(t!("Active")),
                                )
                            })
                            .child(
                                Button::new(SharedString::from(format!("edit-server-{edit_id}")))
                                    .icon(AppIcon::Pencil)
                                    .tooltip(t!("Edit connection"))
                                    .ghost()
                                    .small()
                                    // 行本身可点击切换：行内按钮必须拦住鼠标按下，
                                    // 否则会“打开编辑表单”与“切换服务器”同时发生。
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.begin_edit_server(&edit_id, window, cx);
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "remove-server-{remove_id}"
                                )))
                                .icon(AppIcon::Trash)
                                .tooltip(if can_remove {
                                    t!("Remove this server")
                                } else {
                                    t!("Keep at least one server")
                                })
                                .disabled(!can_remove)
                                .ghost()
                                .small()
                                // 同上：可切换行里的删除按钮此前会连带触发切换
                                // （先弹删除确认、随后又切到那台），拦住冒泡。
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.confirm_remove_server(&remove_id, window, cx);
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(switchable, |this| {
                        this.on_click(cx.listener(move |this, _, window, cx| {
                            this.switch_server(&switch_id, window, cx);
                            cx.notify();
                        }))
                    })
            }))
            .into_any_element()
    }
}
