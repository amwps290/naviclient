//! Shared proxies: a list with reachability tests, an add/edit dialog, and
//! persistence that is saved before any live client is replaced.

use super::bilibili_state::AccountStatus;
use super::proxy_probe::ProbeState;
use super::state::{ProxyDialog, SettingsDialog};
use crate::app::prelude::*;
use crate::models::network::{NetworkError, NetworkRoute};
use crate::source::route_probe::RouteProbeError;
use crate::source::{ProviderKind, SourceId};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::menu::PopupMenuItem;

impl NetworkError {
    pub(in crate::app) fn message(self) -> SharedString {
        match self {
            Self::InvalidProxy => t!("Enter a valid proxy address."),
            Self::MissingProxy => t!("The selected proxy is missing. Choose a proxy or direct connection."),
            Self::MissingSource => t!("This music source no longer exists."),
            Self::ProxyInUse => t!("This proxy is in use. Change its music sources and lyrics service to another connection before removing it."),
        }.into()
    }
}

#[derive(Debug)]
enum SaveError {
    Network(NetworkError),
    Persistence,
}

fn save_network_change(
    config: &Config,
    change: impl FnOnce(&mut Config) -> Result<(), NetworkError>,
    save: impl FnOnce(&Config) -> anyhow::Result<()>,
) -> Result<Config, SaveError> {
    let mut next = config.clone();
    change(&mut next).map_err(SaveError::Network)?;
    save(&next).map_err(|_| SaveError::Persistence)?;
    Ok(next)
}

impl NavidromeApp {
    /// Apply a network change, then rebuild the clients of every source whose
    /// route changed. Settings save silently; only failures are reported.
    pub(in crate::app) fn change_network(
        &mut self,
        change: impl FnOnce(&mut Config) -> Result<(), NetworkError>,
    ) -> Result<(), NetworkError> {
        let next = match save_network_change(&self.config, change, config::save) {
            Ok(next) => next,
            Err(SaveError::Network(error)) => return Err(error),
            Err(SaveError::Persistence) => {
                self.notify_error(t!("Could not save network settings. Retry."));
                return Ok(());
            }
        };
        let changed: Vec<_> = next
            .sources
            .iter()
            .map(|profile| profile.id())
            .filter(|source| self.config.route_for(source) != next.route_for(source))
            .collect();
        let lyrics_changed = self
            .config
            .resolve_connection(&self.config.lyrics.connection)
            != next.resolve_connection(&next.lyrics.connection);
        self.config = next;
        for source in changed {
            self.apply_source_route(&source);
        }
        if lyrics_changed {
            self.lyrics_client = None;
            self.lyrics_ui.cancel();
            self.refresh_lyrics();
        }
        Ok(())
    }

    fn apply_source_route(&mut self, source: &SourceId) {
        self.cancel_source_playback_work(source);
        if source.provider == ProviderKind::Bilibili {
            if matches!(
                self.bilibili_accounts.get(source),
                Some(AccountStatus::Ready(_))
            ) {
                self.activate_bilibili_account(source);
            } else {
                self.check_bilibili_account(source);
            }
            return;
        }
        if self
            .playback_source
            .as_ref()
            .is_some_and(|api| api.id() == source)
        {
            self.playback_source = self.available_source(source);
        }
        if self.config.active_source.as_ref() == Some(source) {
            self.reconnect_active_server();
        }
    }

    pub(in crate::app) fn open_proxy_dialog(
        &mut self,
        id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let proxy = id.and_then(|id| {
            self.config
                .network
                .proxies
                .iter()
                .find(|proxy| proxy.id == id)
                .cloned()
        });
        if id.is_some() && proxy.is_none() {
            return;
        }
        let (name, url) = proxy
            .map(|proxy| (proxy.name, proxy.url))
            .unwrap_or_default();
        self.proxy_name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.proxy_url_input
            .update(cx, |input, cx| input.set_value(url, window, cx));
        self.state
            .settings
            .open_dialog(SettingsDialog::Proxy(ProxyDialog {
                target: id.map(str::to_owned),
                error: None,
            }));
        self.show_settings_dialog(window, cx);
        self.proxy_url_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    fn save_proxy(&mut self, cx: &mut Context<Self>) {
        let Some(SettingsDialog::Proxy(dialog)) = self.state.settings.dialog.clone() else {
            return;
        };
        let name = self.proxy_name_input.read(cx).value().to_string();
        let url = self.proxy_url_input.read(cx).value().to_string();
        let result = self.change_network(|config| {
            config
                .store_proxy(dialog.target.as_deref(), &name, &url)
                .map(|_| ())
        });
        match result {
            Ok(()) => {
                if let Some(id) = &dialog.target {
                    self.state.settings.proxy_probes.forget(id);
                }
                self.state.settings.dismiss_dialog();
            }
            Err(error) => {
                if let Some(SettingsDialog::Proxy(dialog)) = self.state.settings.dialog.as_mut() {
                    dialog.error = Some(error);
                }
            }
        }
    }

    fn remove_proxy(&mut self, id: &str) {
        match self.change_network(|config| config.remove_proxy(id)) {
            Ok(()) => self.state.settings.proxy_probes.forget(id),
            Err(error) => self.notify_error(error.message()),
        }
    }

    fn test_proxy(&mut self, id: &str) {
        let Some(proxy) = self
            .config
            .network
            .proxies
            .iter()
            .find(|proxy| proxy.id == id)
        else {
            return;
        };
        let route = NetworkRoute::Proxy(proxy.url.clone());
        let request = self.state.settings.proxy_probes.begin(id);
        let tx = self.tx.clone();
        let proxy = id.to_string();
        self.spawn_future(async move {
            let result = crate::source::route_probe::probe_route(&route).await;
            let _ = tx.send(Msg::ProxyProbed {
                proxy,
                request,
                result,
            });
        });
    }

    pub(in crate::app) fn finish_proxy_probe(
        &mut self,
        proxy: String,
        request: u64,
        result: Result<Duration, RouteProbeError>,
    ) {
        self.state
            .settings
            .proxy_probes
            .complete(&proxy, request, result);
    }

    pub(in crate::app) fn render_proxy_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let proxies = &self.config.network.proxies;
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("New music sources use the system proxy. Choose a saved proxy or a direct connection for each source in its menu.")),
                    )
                    .child(
                        Button::new("add-proxy")
                            .label(t!("Add proxy"))
                            .icon(KitIcon::Plus)
                            .primary()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_proxy_dialog(None, window, cx);
                            })),
                    ),
            )
            .when(proxies.is_empty(), |list| {
                list.child(
                    v_flex()
                        .py_8()
                        .gap_2()
                        .items_center()
                        .child(
                            Icon::new(AppIcon::Globe)
                                .with_size(px(28.0))
                                .text_color(palette(cx).text_muted),
                        )
                        .child(div().text_sm().child(t!("No saved proxies")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(palette(cx).text_secondary)
                                .child(t!("Add an HTTP, HTTPS or SOCKS5 proxy, such as 127.0.0.1:7890.")),
                        ),
                )
            })
            .children(proxies.iter().map(|proxy| self.render_proxy_row(proxy, cx)))
            .into_any_element()
    }

    fn render_proxy_row(
        &self,
        proxy: &crate::models::network::ProxyProfile,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let users = self.config.proxy_users(&proxy.id);
        let probe = self.state.settings.proxy_probes.get(&proxy.id).copied();
        let (dot, result) = match probe {
            None => (None, None),
            Some(ProbeState::Testing) => {
                (Some(cx.theme().warning), Some(t!("Testing…").to_string()))
            }
            Some(ProbeState::Reachable(latency)) => (
                Some(p.online),
                Some(t!("Reachable · %{ms} ms", ms = latency.as_millis()).to_string()),
            ),
            Some(ProbeState::Failed(error)) => (
                Some(cx.theme().danger),
                Some(
                    match error {
                        RouteProbeError::Timeout => t!("Timed out"),
                        RouteProbeError::Unreachable => t!("Unreachable"),
                        RouteProbeError::InvalidProxy => t!("Enter a valid proxy address."),
                    }
                    .to_string(),
                ),
            ),
        };
        let usage = if users == 0 {
            t!("Not used").to_string()
        } else {
            t!("Used by %{count} connections", count = users).to_string()
        };
        let test = proxy.id.clone();
        let menu_id = proxy.id.clone();
        let app = cx.entity().downgrade();
        h_flex()
            .w_full()
            .gap_3()
            .px_3()
            .py_3()
            .items_center()
            .rounded(px(tokens::radius::ROW))
            .hover(|row| row.bg(p.sunken))
            .child(
                Icon::new(AppIcon::Globe)
                    .with_size(px(18.0))
                    .text_color(p.text_secondary),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(proxy.label().to_string()),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .when_some(dot, |line, dot| {
                                line.child(div().size(px(7.0)).flex_none().rounded_full().bg(dot))
                            })
                            .child(
                                div().min_w_0().truncate().child(
                                    [Some(proxy.url.clone()), Some(usage), result]
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>()
                                        .join(" · "),
                                ),
                            ),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("test-proxy-{}", proxy.id)))
                    .label(t!("Test"))
                    .outline()
                    .small()
                    .loading(probe == Some(ProbeState::Testing))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.test_proxy(&test);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("proxy-menu-{}", proxy.id)))
                    .icon(AppIcon::More)
                    .tooltip(t!("More actions"))
                    .ghost()
                    .small()
                    .dropdown_menu(move |menu, _, cx| {
                        let edit = menu_id.clone();
                        let remove = menu_id.clone();
                        app.update(cx, move |this, cx| {
                            let used = this.config.proxy_users(&remove) > 0;
                            menu.item(
                                PopupMenuItem::new(t!("Edit proxy"))
                                    .icon(AppIcon::Pencil)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_proxy_dialog(Some(&edit), window, cx);
                                    })),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new(if used {
                                    t!("In use — change its music sources and lyrics connection first")
                                } else {
                                    t!("Remove proxy")
                                })
                                .icon(AppIcon::Trash)
                                .disabled(used)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.remove_proxy(&remove);
                                        cx.notify();
                                    },
                                )),
                            )
                        })
                        .expect("app owns proxy menu")
                    }),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_proxy_dialog(
        &self,
        dialog: Dialog,
        state: &ProxyDialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let form = v_flex()
            .w_full()
            .gap_3()
            .child(self.render_form_row(
                t!("Name"),
                Input::new(&self.proxy_name_input)
                    .w_full()
                    .bg(palette(cx).sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("Address"),
                Input::new(&self.proxy_url_input)
                    .w_full()
                    .bg(palette(cx).sunken)
                    .into_any_element(),
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .map(|text| match state.error {
                        Some(error) => text.text_color(cx.theme().danger).child(error.message()),
                        None => text
                            .text_color(palette(cx).text_secondary)
                            .child(t!("HTTP, HTTPS or SOCKS5 (socks5:// or socks5h://). Without a scheme, HTTP is used. Proxies that need a username and password are not supported.")),
                    }),
            );
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-proxy-edit")
                    .label(t!("Cancel"))
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                Button::new("save-proxy")
                    .label(t!("Save proxy"))
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_proxy(cx);
                        this.sync_settings_dialog(window, cx);
                        cx.notify();
                    })),
            );
        dialog
            .title(if state.target.is_some() {
                t!("Edit proxy")
            } else {
                t!("Add proxy")
            })
            .w(px(480.0))
            .child(form)
            .footer(footer)
    }
}

#[cfg(test)]
mod tests;
