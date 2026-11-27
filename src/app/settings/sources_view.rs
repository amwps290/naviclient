//! Music sources: one list for every provider, each row with a status dot,
//! one inline action and a menu for editing, connection and removal.

use super::bilibili_state::AccountStatus;
use super::components::{settings_card, settings_divider};
use super::source_row::{Connection, SourceAction, SourceHealth, SourceRow};
use crate::app::prelude::*;
use crate::models::network::SourceConnection;
use crate::source::{ProviderKind, SourceId, SourceProfile};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

impl NavidromeApp {
    pub(in crate::app) fn render_source_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let rows: Vec<_> = self
            .config
            .sources
            .iter()
            .filter_map(|profile| self.render_source_row(profile, cx))
            .collect();
        let empty = rows.is_empty();
        let mut list_card = settings_card(cx);
        if empty {
            list_card = list_card.child(
                v_flex()
                    .py_10()
                    .px_6()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(AppIcon::Server)
                            .with_size(px(26.0))
                            .text_color(palette(cx).text_muted),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_center()
                            .text_color(palette(cx).text_secondary)
                            .child(t!(
                                "Add a Navidrome server or a Bilibili account to start listening."
                            )),
                    ),
            );
        } else {
            for (index, row) in rows.into_iter().enumerate() {
                if index > 0 {
                    list_card = list_card.child(settings_divider(cx));
                }
                list_card = list_card.child(row);
            }
        }

        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex().justify_end().child(
                    primary_button("add-music-source")
                        .label(t!("Add music source"))
                        .icon(KitIcon::Plus)
                        .dropdown_caret(true)
                        .compact_action()
                        .dropdown_menu(move |menu, _, cx| {
                            app.update(cx, |_, cx| {
                                menu.item(
                                    PopupMenuItem::new("Navidrome / OpenSubsonic")
                                        .icon(AppIcon::Server)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_server_dialog(None, window, cx);
                                        })),
                                )
                                .item(
                                    PopupMenuItem::new(t!("Bilibili favorites"))
                                        .icon(AppIcon::Video)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_bilibili_login(None, window, cx);
                                        })),
                                )
                            })
                            .expect("app owns source menu")
                        }),
                ),
            )
            .child(list_card)
            .into_any_element()
    }

    fn source_row_state(&self, profile: &SourceProfile) -> Option<SourceRow> {
        let source = profile.id();
        let current = self.config.active_source.as_ref() == Some(&source);
        let summary = current.then(|| self.connected_server_summary()).flatten();
        let connected = match profile {
            SourceProfile::OpenSubsonic { .. } => summary.map(|(name, _)| name),
            SourceProfile::Bilibili { .. } => self.api.is_some().then(|| "Bilibili".into()),
        }
        .filter(|_| current);
        SourceRow::derive(
            &self.config,
            profile,
            self.bilibili_accounts.get(&source),
            Connection {
                current,
                loading: current && self.state.connection.loading(),
                connected,
            },
        )
    }

    fn render_source_row(
        &self,
        profile: &SourceProfile,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let row = self.source_row_state(profile)?;
        let source = profile.id();
        let key = source.cache_namespace();
        let current = self.config.active_source.as_ref() == Some(&source);
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let dot = match row.health {
            SourceHealth::Connected => p.online,
            SourceHealth::Busy => cx.theme().warning,
            SourceHealth::Problem => cx.theme().danger,
            SourceHealth::Idle => p.text_muted,
        };
        let details = (current && row.provider == ProviderKind::OpenSubsonic)
            .then(|| self.connected_server_summary())
            .flatten()
            .map(|(_, details)| details);
        let connection = self.connection_label(&self.config.source_connection(&source));
        let action = row.action.map(|action| {
            let target = source.clone();
            let id = SharedString::from(format!("source-action-{key}"));
            let button = if action == SourceAction::Switch {
                surface_button(id)
            } else {
                primary_button(id)
            };
            button
                .label(match action {
                    SourceAction::SetUp => t!("Set up"),
                    SourceAction::SignIn => t!("Sign in"),
                    SourceAction::Retry => t!("Retry"),
                    SourceAction::Switch => t!("Use"),
                })
                .compact_action()
                .disabled(row.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_source_action(&target, action, window, cx);
                    cx.notify();
                }))
        });
        let element = h_flex()
            .id(SharedString::from(format!("source-row-{key}")))
            .w_full()
            .gap_3p5()
            .px_4()
            .py_3p5()
            .items_center()
            .when(current, |row| {
                row.bg(p.brand_soft.opacity(if dark { 0.65 } else { 0.72 }))
            })
            .when(!current, |row| {
                row.hover(|row| row.bg(p.sunken.opacity(0.65)))
            })
            .child(
                div()
                    .size(px(38.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(tokens::radius::ROW))
                    .bg(if current {
                        p.brand.opacity(0.14)
                    } else {
                        p.raised
                    })
                    .border_1()
                    .border_color(if current {
                        p.brand.opacity(0.28)
                    } else if dark {
                        hsla(0.0, 0.0, 1.0, 0.07)
                    } else {
                        p.border.opacity(0.75)
                    })
                    .child(
                        Icon::new(match row.provider {
                            ProviderKind::OpenSubsonic => AppIcon::Server,
                            ProviderKind::Bilibili => AppIcon::Video,
                        })
                        .with_size(px(17.0))
                        .text_color(if current {
                            p.brand
                        } else {
                            p.text_secondary
                        }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(row.label.clone()),
                            )
                            .when(current, |title| {
                                title.child(
                                    div()
                                        .flex_none()
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(tokens::radius::FULL))
                                        .bg(p.brand.opacity(if dark { 0.22 } else { 0.14 }))
                                        .text_color(p.brand)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_size(px(11.0))
                                        .child(t!("Current")),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(div().size(px(7.0)).flex_none().rounded_full().bg(dot))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("{} · {}", row.detail, connection)),
                            )
                            .when_some(details, |line, details| {
                                line.child(
                                    div()
                                        .id(SharedString::from(format!("source-details-{key}")))
                                        .flex_none()
                                        .child(
                                            Icon::new(KitIcon::Info)
                                                .with_size(px(13.0))
                                                .text_color(p.text_muted),
                                        )
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(details.clone()).build(window, cx)
                                        }),
                                )
                            }),
                    ),
            )
            .children(action)
            .child(self.render_source_menu(&source, row.busy, cx));
        Some(element.into_any_element())
    }

    fn run_source_action(
        &mut self,
        source: &SourceId,
        action: SourceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (source.provider, action) {
            (ProviderKind::OpenSubsonic, SourceAction::SetUp | SourceAction::SignIn) => {
                self.open_server_dialog(Some(&source.profile_id), window, cx)
            }
            (ProviderKind::OpenSubsonic, SourceAction::Retry) => self.reconnect_active_server(),
            (ProviderKind::OpenSubsonic, SourceAction::Switch) => {
                self.switch_server(&source.profile_id, window, cx)
            }
            (ProviderKind::Bilibili, SourceAction::SetUp | SourceAction::SignIn) => {
                self.open_bilibili_login(Some(source.clone()), window, cx)
            }
            (ProviderKind::Bilibili, SourceAction::Retry) => {
                if matches!(
                    self.bilibili_accounts.get(source),
                    Some(AccountStatus::Ready(_))
                ) {
                    self.reconnect_active_server();
                } else {
                    self.check_bilibili_account(source);
                }
            }
            (ProviderKind::Bilibili, SourceAction::Switch) => {
                self.switch_bilibili_source(source);
                // Switching from settings keeps settings open, as for servers.
                self.state.settings.open = true;
            }
        }
    }

    fn render_source_menu(
        &self,
        source: &SourceId,
        busy: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let source = source.clone();
        icon_button(
            SharedString::from(format!("source-menu-{}", source.cache_namespace())),
            AppIcon::More,
            t!("More actions"),
            tokens::ICON_BUTTON_SM,
        )
        .disabled(busy)
        .dropdown_menu(move |menu, window, cx| {
            let head = source.clone();
            let menu = app
                .update(cx, |this, cx| {
                    let edit = head.clone();
                    let can_remove = this.config.sources.len() > 1;
                    let menu = menu.item(
                        PopupMenuItem::new(match head.provider {
                            ProviderKind::OpenSubsonic => t!("Edit connection"),
                            ProviderKind::Bilibili => t!("Sign in again"),
                        })
                        .icon(AppIcon::Pencil)
                        .on_click(cx.listener(
                            move |this, _, window, cx| match edit.provider {
                                ProviderKind::OpenSubsonic => {
                                    this.open_server_dialog(Some(&edit.profile_id), window, cx)
                                }
                                ProviderKind::Bilibili => {
                                    this.open_bilibili_login(Some(edit.clone()), window, cx)
                                }
                            },
                        )),
                    );
                    (menu, can_remove)
                })
                .expect("app owns source menu");
            let (menu, can_remove) = menu;
            let sub_app = app.clone();
            let sub_source = source.clone();
            let menu = menu.submenu_with_icon(
                Some(Icon::new(AppIcon::Globe)),
                t!("Connection"),
                window,
                cx,
                move |submenu, _, cx| {
                    sub_app
                        .update(cx, |this, cx| {
                            this.connection_menu(submenu, &sub_source, cx)
                        })
                        .expect("app owns connection menu")
                },
            );
            let remove = source.clone();
            app.update(cx, move |_, cx| {
                menu.separator().item(
                    PopupMenuItem::new(t!("Remove music source"))
                        .icon(AppIcon::Trash)
                        .disabled(!can_remove)
                        .on_click(
                            cx.listener(move |this, _, window, cx| match remove.provider {
                                ProviderKind::OpenSubsonic => {
                                    this.confirm_remove_server(&remove.profile_id, window, cx)
                                }
                                ProviderKind::Bilibili => {
                                    this.confirm_remove_bilibili(remove.clone(), window, cx)
                                }
                            }),
                        ),
                )
            })
            .expect("app owns source menu")
        })
        .into_any_element()
    }

    fn connection_menu(&self, menu: PopupMenu, source: &SourceId, cx: &Context<Self>) -> PopupMenu {
        let current = self.config.source_connection(source);
        let mut menu = menu;
        for choice in self.connection_choices() {
            let target = source.clone();
            let checked = choice == current;
            menu = menu.item(
                PopupMenuItem::new(self.connection_label(&choice))
                    .checked(checked)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose_source_connection(&target, choice.clone());
                        cx.notify();
                    })),
            );
        }
        menu.separator().item(
            PopupMenuItem::new(t!("Manage proxies…")).on_click(cx.listener(|this, _, _, cx| {
                this.state.settings.section = SettingsSection::Proxies;
                cx.notify();
            })),
        )
    }

    /// Change one source's route from its menu; also used by the dialogs.
    pub(in crate::app) fn choose_source_connection(
        &mut self,
        source: &SourceId,
        connection: SourceConnection,
    ) {
        if self.config.source_connection(source) == connection {
            return;
        }
        if let Err(error) =
            self.change_network(|config| config.set_source_connection(source, connection))
        {
            self.notify_error(error.message());
        }
    }
}
