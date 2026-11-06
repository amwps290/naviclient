//! Server settings section and active connection status card.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_server_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Connections")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .when(self.api.is_some(), |this| {
                                this.child(
                                    Button::new("settings-refresh")
                                        .icon(AppIcon::Refresh)
                                        .tooltip(t!("Refresh library"))
                                        .loading(self.state.connection.loading())
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.refresh_library();
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                Button::new("settings-add-server")
                                    .icon(KitIcon::Plus)
                                    .label(t!("Add server"))
                                    .tooltip(t!("Add another server"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_server(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(self.render_server_status_card(cx))
            .into_any_element()
    }

    /// 服务器设置卡：上半部分是服务器列表（点击切换，行尾铅笔/垃圾桶
    /// 分别编辑与删除那一行），下半部分是编辑表单——仅在铅笔点开某行
    /// 或该条目尚未配置（首次运行/刚新增）时存在，平时不铺任何表单。
    /// 编辑不切换服务器：改另一台的地址账号不会动当前正在播的库；
    /// 断线重试在状态行上，不需要先进表单绕一圈。
    fn render_server_status_card(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let connected = self.state.connection.server().is_some();
        let active_configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        let status = if self.state.connection.loading() {
            t!("Connecting…").to_string()
        } else if connected {
            match self.state.connection.server() {
                Some(server) => t!(
                    "Connected · %{app} %{version}",
                    app = server.app_name.as_deref().unwrap_or("Navidrome"),
                    version = server.display_version()
                )
                .to_string(),
                None => t!("Connected").to_string(),
            }
        } else if !active_configured {
            // 未配置的条目表单直接展开，提示语指向表单。
            t!("Not connected — fill in the address and account").to_string()
        } else if self.config.servers.len() > 1 {
            // 列表就在状态行上方，措辞指向列表而不是方位。
            t!("Not connected — pick a server in the list").to_string()
        } else {
            t!("Not connected").to_string()
        };
        // accent 是背景 token（亮色 neutral-100、深色 neutral-800），做前景
        // 会与卡片底色同值；连接态的强调色用 primary。
        let status_color = if connected {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        let (protocol_note, extension_note) = self
            .state
            .connection
            .server()
            .map(|server| {
                let protocol = if server.extensions_discovered {
                    t!(
                        "OpenSubsonic %{version} · %{count} extensions",
                        version = server.protocol_version,
                        count = server.extensions.len()
                    )
                    .to_string()
                } else {
                    t!(
                        "Subsonic %{version} · compatibility mode",
                        version = server.protocol_version
                    )
                    .to_string()
                };
                let discovery_ready = server.capabilities.smart_discovery().is_supported();
                let extensions = server.extension_summary(6).map(|extensions| {
                    if discovery_ready {
                        t!(
                            "Smart discovery available · %{extensions}",
                            extensions = extensions
                        )
                        .to_string()
                    } else {
                        t!("Capabilities: %{extensions}", extensions = extensions).to_string()
                    }
                });
                (Some(protocol), extensions)
            })
            .unwrap_or((None, None));
        // 表单目标：行内铅笔打开的条目；没有显式目标时，尚未配置的
        // 激活条目（首次运行、刚新增）也直接展示表单——那时表单就是
        // 主任务。已配置的服务器默认不铺任何表单，编辑入口在每行里。
        let form_target = self.state.settings.server_form_target(&self.config);
        // 密码存储位置提示（里程碑 #17）：已迁入系统凭据库的条目不再
        // 往配置文件里写明文，用户需要在设置页能确认当前状态。
        let credential_note = if self
            .config
            .active_profile()
            .is_some_and(|profile| profile.password_in_keyring)
        {
            t!("Password is kept in the system credential store")
        } else {
            t!("Password is kept in the config file")
        };

        let mut card = v_flex()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .bg(cx.theme().secondary.opacity(0.18))
            // ---- 上：服务器列表 ----
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_4()
                    .child(self.render_server_list(cx))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().text_xs().text_color(status_color).child(status))
                                    // 断线时给一个就地重试：重连不必先改地址账号。
                                    .when(
                                        !connected
                                            && active_configured
                                            && !self.state.connection.loading(),
                                        |row| {
                                            row.child(
                                                Button::new("retry-server-connection")
                                                    .label(t!("Retry"))
                                                    .tooltip(t!("Reconnect to the selected server"))
                                                    .ghost()
                                                    .small()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.reconnect_active_server();
                                                        cx.notify();
                                                    })),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(credential_note),
                            )
                            .when_some(protocol_note, |this, note| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(note),
                                )
                            })
                            .when_some(extension_note, |this, note| {
                                this.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(note),
                                )
                            }),
                    ),
            );
        // ---- 下：编辑表单（仅在行内铅笔点开或条目未配置时存在） ----
        if let Some(target) = form_target {
            card = card
                .child(div().h(px(1.0)).bg(cx.theme().border))
                .child(self.render_server_edit_form(target, cx));
        }
        card.into_any_element()
    }
}
