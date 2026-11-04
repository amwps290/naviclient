//! 设置覆盖层、服务器管理以及快捷键设置的渲染。

use super::*;
use gpui_kit::component::menu::PopupMenuItem;

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
    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(super) fn render_settings_overlay(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let settings_content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .vertical_scrollbar(&self.settings_scroll)
            .when(compact, |this| this.px_5().py_4())
            .when(!compact, |this| this.px_6().py_5())
            .child(self.render_settings(cx));
        let settings_body = if compact {
            v_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(true, cx))
                .child(settings_content)
                .into_any_element()
        } else {
            h_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(false, cx))
                .child(settings_content)
                .into_any_element()
        };

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude() // 遮罩挡住背后所有命中框：后方页面不再响应滚轮/悬停
            .flex()
            .justify_center()
            .items_start()
            // 固定黑色半透明背景：亮/暗主题下都是压暗效果。
            .bg(hsla(0.0, 0.0, 0.0, 0.35))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.settings_open = false;
                    this.cancel_shortcut_recording();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(960.0))
                    .h(px(650.0))
                    .max_w(relative(if compact { 0.96 } else { 0.94 }))
                    .max_h(relative(if compact { 0.94 } else { 0.88 }))
                    .mt(px(if compact { 16.0 } else { 48.0 }))
                    .bg(cx.theme().background)
                    .rounded_xl()
                    .shadow_xl()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        // 点击卡片内部不触发背景关闭。
                        cx.stop_propagation()
                    })
                    .child(
                        h_flex()
                            .flex_none()
                            .when(compact, |this| this.px_4().py_2())
                            .when(!compact, |this| this.px_5().py_3())
                            .justify_between()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Settings")),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip(t!("Close settings"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = false;
                                        this.cancel_shortcut_recording();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(settings_body),
            )
            .into_any_element()
    }

    fn settings_navigation_button(
        &self,
        id: &'static str,
        section: SettingsSection,
        label: SharedString,
        icon: AppIcon,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let selected = self.state.settings_section == section;
        let button = Button::new(id)
            .w_full()
            .h(px(36.0))
            .when(!compact, |this| this.justify_start())
            .accessibility_label(label.clone())
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .when(!compact, |this| this.justify_start())
                    .child(
                        div()
                            .w(px(20.0))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(Icon::new(icon).with_size(px(16.0))),
                    )
                    .child(div().min_w_0().text_sm().truncate().child(label)),
            )
            .ghost()
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cancel_shortcut_recording();
                this.state.settings_section = section;
                this.settings_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }));

        if compact {
            div().flex_1().min_w_0().child(button).into_any_element()
        } else {
            button.into_any_element()
        }
    }

    fn render_settings_navigation(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let buttons = vec![
            self.settings_navigation_button(
                "settings-nav-servers",
                SettingsSection::Servers,
                t!("Servers").into(),
                AppIcon::Server,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-playback",
                SettingsSection::Playback,
                t!("Playback").into(),
                AppIcon::VolumeHigh,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-desktop",
                SettingsSection::Desktop,
                t!("Desktop").into(),
                AppIcon::MiniPlayer,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-appearance",
                SettingsSection::Appearance,
                t!("Appearance").into(),
                AppIcon::Settings,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-shortcuts",
                SettingsSection::Shortcuts,
                t!("Shortcuts").into(),
                AppIcon::Keyboard,
                compact,
                cx,
            ),
        ];

        if compact {
            h_flex()
                .w_full()
                .flex_none()
                .gap_1()
                .p_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        } else {
            v_flex()
                .w(px(184.0))
                .flex_none()
                .gap_1()
                .p_3()
                .border_r_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        }
    }

    pub(super) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cache_dir = config::audio_cache_dir(&self.config);
        let (title, description) = match self.state.settings_section {
            SettingsSection::Servers => (
                t!("Servers"),
                t!("Manage server connections and library access."),
            ),
            SettingsSection::Playback => (
                t!("Playback"),
                t!("Configure streaming, audio output, and listening preferences."),
            ),
            SettingsSection::Desktop => (
                t!("Desktop"),
                t!("Control notifications and window behavior."),
            ),
            SettingsSection::Appearance => (
                t!("Appearance"),
                t!("Choose the theme and interface language."),
            ),
            SettingsSection::Shortcuts => (
                t!("Shortcuts"),
                t!("Customize keyboard shortcuts for playback and navigation."),
            ),
        };
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .when(self.state.settings_section == SettingsSection::Servers, |settings| settings.child(
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
                                                .loading(self.state.loading)
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
                    .child(self.render_server_status_card(cx)),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Streaming & cache")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("Audio cache folder")),
                                    )
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .child(cache_dir.display().to_string()),
                                            )
                                            .child(
                                                Button::new("choose-cache-directory")
                                                    .label(t!("Choose folder"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.choose_cache_directory(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("reset-cache-directory")
                                                    .label(t!("Use default"))
                                                    .disabled(self.config.cache_dir.is_none())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.apply_cache_directory(None);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Streaming quality")),
                                    )
                                    .child({
                                        let app = cx.entity().downgrade();
                                        Button::new("streaming-quality-select")
                                            .w(px(220.0))
                                            .justify_between()
                                            .label(self.config.transcoding_quality.label())
                                            .dropdown_caret(true)
                                            .outline()
                                            .dropdown_menu(move |menu, _, cx| {
                                                app.update(cx, |this, cx| {
                                                    let mut menu = menu;
                                                    for quality in TranscodingQuality::ALL {
                                                        menu = menu.item(
                                                            PopupMenuItem::new(quality.label())
                                                                .checked(
                                                                    this.config
                                                                        .transcoding_quality
                                                                        == quality,
                                                                )
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.set_transcoding_quality(
                                                                            quality, cx,
                                                                        );
                                                                    },
                                                                )),
                                                        );
                                                    }
                                                    menu
                                                })
                                                .expect("app owns streaming quality menu")
                                            })
                                    }),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Audio output")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child({
                                let playback = self.audio.state();
                                let active = match playback.output_name.as_deref() {
                                    Some(name) => name.to_string(),
                                    None => t!("System default").to_string(),
                                };
                                div().text_xs().text_color(cx.theme().muted_foreground).child(
                                    t!("Active: %{active} · %{rate} Hz · %{channels} ch",
                                        active = active,
                                        rate = playback.output_sample_rate,
                                        channels = playback.output_channels
                                    ),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_wrap()
                                    .child({
                                        let app = cx.entity().downgrade();
                                        let selected_label = self
                                            .config
                                            .output_device
                                            .clone()
                                            .unwrap_or_else(|| t!("System default").to_string());
                                        Button::new("output-device-select")
                                            .w(px(300.0))
                                            .max_w_full()
                                            .justify_between()
                                            .label(selected_label.clone())
                                            .tooltip(selected_label)
                                            .dropdown_caret(true)
                                            .outline()
                                            .dropdown_menu(move |menu, _, cx| {
                                                app.update(cx, |this, cx| {
                                                    let mut menu = menu.item(
                                                        PopupMenuItem::new(t!("System default"))
                                                            .checked(
                                                                this.config.output_device.is_none(),
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.set_output_device(None, cx);
                                                                },
                                                            )),
                                                    );
                                                    for (name, is_default) in
                                                        &this.state.output_devices
                                                    {
                                                        let name_for_click = name.clone();
                                                        let label = if *is_default {
                                                            t!(
                                                                "%{name} (default)",
                                                                name = name
                                                            )
                                                        } else {
                                                            t!("%{name}", name = name)
                                                        };
                                                        menu = menu.item(
                                                            PopupMenuItem::new(label)
                                                                .checked(
                                                                    this.config
                                                                        .output_device
                                                                        .as_deref()
                                                                        == Some(name.as_str()),
                                                                )
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.set_output_device(
                                                                            Some(
                                                                                name_for_click
                                                                                    .clone(),
                                                                            ),
                                                                            cx,
                                                                        );
                                                                    },
                                                                )),
                                                        );
                                                    }
                                                    menu
                                                })
                                                .expect("app owns output device menu")
                                            })
                                    })
                                    .child(
                                        Button::new("refresh-output-devices")
                                            .icon(AppIcon::Refresh)
                                            .tooltip(t!("Refresh devices"))
                                            .ghost()
                                            .small()
                                            .loading(self.state.output_devices_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_output_devices(cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Volume normalization")),
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
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level."
                                                ),
                                            ),
                                    ),
                            )
                            .child({
                                let app = cx.entity().downgrade();
                                Button::new("volume-normalization-select")
                                    .w(px(220.0))
                                    .justify_between()
                                    .label(self.config.volume_normalization.label())
                                    .tooltip(self.config.volume_normalization.tooltip())
                                    .dropdown_caret(true)
                                    .outline()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |this, cx| {
                                            let mut menu = menu;
                                            for mode in VolumeNormalization::ALL {
                                                menu = menu.item(
                                                    PopupMenuItem::new(mode.label())
                                                        .checked(
                                                            this.config.volume_normalization
                                                                == mode,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_volume_normalization(
                                                                    mode, cx,
                                                                );
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns volume normalization menu")
                                    })
                            }),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Lyrics")),
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
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Show translation")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Show translated lyrics under the original line when the server provides another language variant."
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                Switch::new("lyrics-translation-switch")
                                    .checked(self.config.show_lyrics_translation)
                                    .accessibility_label(t!("Show translation"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.set_show_lyrics_translation(*checked, cx);
                                    })),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Desktop, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("System integration")),
                    )
                    .child(
                        v_flex()
                            .gap_5()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .child(t!("Now-playing notifications")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("Send a desktop notification with the cover, title, artist and album when a track starts. Rapid switching replaces the previous notification."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("track-notifications-switch")
                                            .checked(self.config.show_track_notifications)
                                            .accessibility_label(t!("Now-playing notifications"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_show_track_notifications(*checked, cx);
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .child(t!("Close button minimizes")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("The window close button minimizes to the taskbar instead of quitting; tray Exit still quits. Some Wayland compositors ignore minimize requests."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("close-minimizes-switch")
                                            .checked(self.config.close_minimizes)
                                            .accessibility_label(t!("Close button minimizes"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_close_minimizes(*checked, cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
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
                                        // 同上：ID 用 Debug 名（Light/Dark/System），
                                        // 不随界面语言变化。
                                        Button::new(SharedString::from(format!(
                                            "theme-{preference:?}"
                                        )))
                                        .label(preference.label())
                                        .selected(self.config.theme == preference)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_theme(preference, window, cx);
                                            }),
                                        )
                                    }),
                                ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
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
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!(
                                                "Interface language. Changes apply instantly."
                                            )),
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
                                                        .checked(
                                                            this.config.language == preference,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_language(preference, cx);
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns language menu")
                                    })
                            }),
                    ),
            ))
            .when(
                self.state.settings_section == SettingsSection::Shortcuts,
                |settings| settings.child(self.render_shortcut_settings(cx)),
            )
            .into_any_element()
    }

    fn render_shortcut_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        const PLAYBACK: &[ShortcutAction] = &[
            ShortcutAction::TogglePlayback,
            ShortcutAction::Previous,
            ShortcutAction::Next,
            ShortcutAction::SeekBack,
            ShortcutAction::SeekForward,
            ShortcutAction::VolumeUp,
            ShortcutAction::VolumeDown,
            ShortcutAction::ToggleMute,
        ];
        const NAVIGATION: &[ShortcutAction] = &[
            ShortcutAction::ToggleNowPlaying,
            ShortcutAction::ToggleQueue,
            ShortcutAction::FocusSearch,
            ShortcutAction::CloseOverlays,
        ];
        const SONG_LIST: &[ShortcutAction] = &[
            ShortcutAction::SelectPrevious,
            ShortcutAction::SelectNext,
            ShortcutAction::SelectAll,
            ShortcutAction::PlaySelected,
        ];
        let all_default = ShortcutAction::ALL
            .into_iter()
            .all(|action| self.config.shortcuts.binding(action) == action.default_binding());
        let all_global_default = ShortcutAction::GLOBAL_PLAYBACK.into_iter().all(|action| {
            self.config.global_shortcuts.binding(action)
                == action
                    .default_global_binding()
                    .expect("global shortcut actions have defaults")
        });

        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("App shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-shortcuts")
                                    .label(t!("Restore app defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(all_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "These shortcuts work while the app window is active. Click a shortcut, then press a new combination; press Esc to cancel."
                            )),
                    )
                    .child(self.render_shortcut_group(t!("Playback controls"), PLAYBACK, cx))
                    .child(self.render_shortcut_group(t!("Navigation"), NAVIGATION, cx))
                    .child(self.render_shortcut_group(t!("Song list"), SONG_LIST, cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .pt_5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Global shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-global-shortcuts")
                                    .label(t!("Restore global defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(!cfg!(target_os = "windows") || all_global_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_global_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if cfg!(target_os = "windows") {
                                t!("Control playback while the app is hidden or another window is active. Use the Windows key or at least two modifier keys.")
                            } else {
                                t!("Custom global shortcuts are currently available on Windows.")
                            }),
                    )
                    .child(self.render_global_shortcut_group(cx)),
            )
            .into_any_element()
    }

    fn render_shortcut_group(
        &self,
        title: impl Into<SharedString>,
        actions: &'static [ShortcutAction],
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.into()),
            )
            .child(
                v_flex()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .overflow_hidden()
                    .children(
                        actions.iter().enumerate().map(|(index, action)| {
                            self.render_shortcut_row(*action, index > 0, cx)
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_shortcut == Some(action);
        let binding = self.config.shortcuts.binding(action);
        let is_default = binding == action.default_binding();
        let conflict = self
            .state
            .shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);

        v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .when(separated, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(shortcut_label(action)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, false);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_shortcut(action, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(conflict, |row, existing| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Already assigned to %{action}",
                    action = shortcut_label(existing)
                )))
            })
            .into_any_element()
    }

    fn render_global_shortcut_group(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .overflow_hidden()
            .children(
                ShortcutAction::GLOBAL_PLAYBACK
                    .iter()
                    .enumerate()
                    .map(|(index, action)| self.render_global_shortcut_row(*action, index > 0, cx)),
            )
            .into_any_element()
    }

    fn render_global_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_global_shortcut == Some(action);
        let binding = self.config.global_shortcuts.binding(action);
        let is_default = binding
            == action
                .default_global_binding()
                .expect("global shortcut actions have defaults");
        let conflict = self
            .state
            .global_shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);
        let validation = self
            .state
            .global_shortcut_validation
            .filter(|(target, _)| *target == action)
            .map(|(_, error)| error);
        let registration_failed = self.state.global_shortcut_failures.contains(&action);

        v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .when(separated, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(shortcut_label(action)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "global-shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .disabled(!cfg!(target_os = "windows"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_global_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, true);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "global-shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(!cfg!(target_os = "windows") || is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_global_shortcut(action, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(conflict, |row, existing| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Already assigned to %{action}",
                    action = shortcut_label(existing)
                )))
            })
            .when_some(validation, |row, error| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(match error {
                            GlobalShortcutValidationError::UnsafeModifiers => {
                                t!("Use the Windows key or at least two modifier keys.")
                            }
                            GlobalShortcutValidationError::UnsupportedKey => {
                                t!("That key cannot be used as a global shortcut.")
                            }
                        }),
                )
            })
            .when(registration_failed, |row| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Could not register this shortcut. It may already be used by another app."
                )))
            })
            .into_any_element()
    }
}
