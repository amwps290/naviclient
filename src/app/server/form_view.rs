//! Server edit form and field layout.

use crate::app::prelude::*;

impl NavidromeApp {
    /// 连接表单：行内铅笔点开某一行、或该条目尚未配置（首次运行 /
    /// 刚新增）时展示，标题写明正在编辑哪一台。保存按钮文案随目标
    /// 与改动内容变化：目标是当前连接的服务器且动了地址/账号/密码
    /// 才是“保存并连接”；编辑别的服务器或只改显示名就是普通“保存”。
    pub(in crate::app) fn render_server_edit_form(
        &self,
        target: &ServerProfile,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let target_configured = target.is_configured();
        let is_active = self.config.active_server.as_deref() == Some(target.id.as_str());
        let reconnect = self.server_form(cx).requires_reconnect(
            &self.config,
            target,
            &target.resolved_password(),
        );
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(
                        if target_configured {
                            t!("Editing “%{editing}”", editing = target.label())
                        } else {
                            t!("Configure your Navidrome server")
                        },
                    ))
                    .when(self.config.servers.len() > 1, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("switch servers in the list above")),
                        )
                    }),
            )
            .child(self.render_server_field(t!("Display name"), &self.name_input, cx))
            .child(self.render_server_field(t!("Server URL"), &self.server_input, cx))
            .child(self.render_server_field(t!("Username"), &self.username_input, cx))
            .child(self.render_server_field(t!("Password"), &self.password_input, cx))
            // 连接状态只与当前连接的服务器有关：编辑另一台时不显示，
            // 避把 A 的连接错误误读成 B 的问题。
            .when(is_active, |form| {
                form.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.state.status.clone()),
                )
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    // 未配置的条目没有可取消回去的东西，不给退路按钮。
                    .when(target_configured, |row| {
                        row.child(
                            Button::new("cancel-server-edit")
                                .label(t!("Cancel"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_server_edit(window, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new("save-settings")
                            .label(if reconnect {
                                // 当前连接的服务器且改了连接身份：保存后
                                // 断开重连；未配置条目的保存意图也是
                                // 建立连接。
                                t!("Save and connect")
                            } else {
                                t!("Save")
                            })
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_settings(cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 一行「标签 + 输入框」的紧凑表单行：标签定宽左对齐，
    /// 保证四个字段的输入框起始位置完全一致。
    fn render_server_field(
        &self,
        label: impl Into<SharedString>,
        input: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let label: SharedString = label.into();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).w_full().bg(palette(cx).sunken)),
            )
            .into_any_element()
    }
}
