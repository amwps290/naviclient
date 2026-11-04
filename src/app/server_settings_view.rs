//! 服务器列表、连接状态、编辑表单及删除确认的渲染。

use super::*;

impl NavidromeApp {
    /// 服务器设置卡：上半部分是服务器列表（点击切换，行尾铅笔/垃圾桶
    /// 分别编辑与删除那一行），下半部分是编辑表单——仅在铅笔点开某行
    /// 或该条目尚未配置（首次运行/刚新增）时存在，平时不铺任何表单。
    /// 编辑不切换服务器：改另一台的地址账号不会动当前正在播的库；
    /// 断线重试在状态行上，不需要先进表单绕一圈。
    pub(super) fn render_server_status_card(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let connected = self.state.server.is_some();
        let active_configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        let status = if self.state.loading {
            t!("Connecting…").to_string()
        } else if connected {
            match self.state.server.as_ref() {
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
            .server
            .as_ref()
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
        let form_target = self
            .state
            .editing_server
            .as_deref()
            .and_then(|id| self.config.servers.iter().find(|profile| profile.id == id))
            .or_else(|| {
                (!active_configured)
                    .then(|| self.config.active_profile())
                    .flatten()
            })
            .cloned();
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
                                        !connected && active_configured && !self.state.loading,
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
        if let Some(target) = &form_target {
            card = card
                .child(div().h(px(1.0)).bg(cx.theme().border))
                .child(self.render_server_edit_form(target, cx));
        }
        card.into_any_element()
    }

    /// 连接表单：行内铅笔点开某一行、或该条目尚未配置（首次运行 /
    /// 刚新增）时展示，标题写明正在编辑哪一台。保存按钮文案随目标
    /// 与改动内容变化：目标是当前连接的服务器且动了地址/账号/密码
    /// 才是“保存并连接”；编辑别的服务器或只改显示名就是普通“保存”。
    fn render_server_edit_form(
        &self,
        target: &ServerProfile,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let target_configured = target.is_configured();
        let is_active = self.config.active_server.as_deref() == Some(target.id.as_str());
        let url = self.server_input.read(cx).value().trim().to_string();
        let username = self.username_input.read(cx).value().trim().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let identity_dirty = target.url.trim() != url
            || target.username.trim() != username
            || target.resolved_password() != password;
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
                            .label(if is_active && (identity_dirty || !target_configured) {
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
            .child(div().flex_1().min_w_0().child(Input::new(input).w_full()))
            .into_any_element()
    }

    /// 已保存服务器列表：点击切换，右侧按钮删除条目。
    /// 这是多服务器的主要入口，排在编辑表单之前。
    /// 服务器列表：一行一台，整行可点即切换（比在行尾找小按钮快），
    /// 只有删除按钮留在行尾。选中项用左侧实心点 + 高亮底色表示。
    fn render_server_list(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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

    /// 删除服务器的确认弹窗：两个递进选项——只移除条目，或连同清理
    /// 这台服务器的本地数据（播放会话/响度/流派缓存与已存凭据）。
    pub(super) fn render_remove_server_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some((_, label)) = &self.server_remove_prompt else {
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
