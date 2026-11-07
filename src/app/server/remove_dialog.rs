//! Server removal confirmation, rendering and local-data cleanup choices.

use crate::app::prelude::*;

impl NavidromeApp {
    /// 删除服务器前的确认弹窗（可选连同清理这台服务器的本地数据）。
    pub(in crate::app) fn confirm_remove_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.server_removal.begin(&self.config, id) {
            return;
        }
        self.show_remove_server_dialog(window, cx);
    }

    /// 弹窗确认后的实际删除；clean_data 的语义与 remove_server 一致。
    /// 弹窗的关闭由调用方（按钮/框架）负责，这里只执行删除。
    fn submit_remove_server(
        &mut self,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.server_removal.take_target() else {
            return;
        };
        self.remove_server(&id, clean_data, window, cx);
        cx.notify();
    }

    /// 关闭弹窗（取消 / Esc / 点击遮罩）：只清状态，不删除。
    fn cancel_remove_server(&mut self, cx: &mut Context<Self>) {
        self.server_removal.cancel();
        cx.notify();
    }

    fn show_remove_server_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // 与歌单弹窗同因：先等菜单层收尾，再安装弹窗的焦点域。
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    app.update(cx, |this, cx| {
                        let dialog = this.render_remove_server_dialog(dialog, cx);
                        crate::app::style::surface::raised_dialog(dialog, cx)
                    })
                    .expect("app owns dialog")
                });
            });
        }
        cx.notify();
    }

    /// 删除服务器的确认弹窗：两个递进选项——只移除条目，或连同清理
    /// 这台服务器的本地数据（播放会话/响度/流派缓存与已存凭据）。
    fn render_remove_server_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some(label) = self.server_removal.label() else {
            return dialog;
        };
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-remove-server")
                    .label(t!("Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_remove_server(cx);
                        window.close_dialog(cx);
                    })),
            )
            .child(
                Button::new("confirm-remove-server")
                    .label(t!("Remove"))
                    .danger()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_remove_server(false, window, cx);
                        window.close_dialog(cx);
                    })),
            )
            .child(
                Button::new("confirm-remove-server-data")
                    .label(t!("Remove and clear data"))
                    .danger()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_remove_server(true, window, cx);
                        window.close_dialog(cx);
                    })),
            );
        let ok_app = cx.entity().downgrade();
        dialog
            .title(t!("Remove server?"))
            .w(px(480.0))
            .overlay_closable(false)
            .keyboard(true)
            // Enter 芓当于“只移除”，保守默认不清理本地数据。
            .on_ok(move |_, window, cx| {
                let _ = ok_app.update(cx, |this, cx| {
                    this.submit_remove_server(false, window, cx)
                });
                true
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.cancel_remove_server(cx);
            }))
            .child(
                v_flex()
                    .gap_2()
                    .p_1()
                    .child(div().text_sm().child(t!("Remove “%{label}” from this app? The server itself and its music stay untouched.",
                        label = label
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("Remove keeps local data: the saved session, loudness and genre caches, and the stored credential are kept, so re-adding the same server continues where you left off. Remove and clear data deletes them for good — the password will have to be re-entered."))
                                ),
                    ),
            )
            .footer(footer)
    }
}
