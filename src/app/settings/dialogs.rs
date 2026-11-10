//! One window dialog for settings tasks: servers, proxies and Bilibili accounts.
//! `SettingsUi::dialog` decides the content on every frame; the window owns
//! the dialog layer and closes it on Cancel/Escape or when the task finishes.

use super::state::SettingsDialog;
use crate::app::prelude::*;
use crate::models::network::SourceConnection;
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::menu::PopupMenuItem;

impl NavidromeApp {
    pub(in crate::app) fn show_settings_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let app = cx.entity().downgrade();
        // Let an open menu finish closing before the dialog takes focus.
        window.defer(cx, move |window, cx| {
            let closing = app.clone();
            window.open_dialog(cx, move |dialog, _, cx| {
                let closing = closing.clone();
                app.update(cx, |this, cx| {
                    let dialog = this
                        .render_settings_dialog(dialog, cx)
                        .overlay_closable(false)
                        .keyboard(true)
                        .on_close(move |_, _, cx| {
                            let _ = closing.update(cx, |this, cx| {
                                this.settings_dialog_closed();
                                cx.notify();
                            });
                        });
                    crate::app::style::surface::raised_dialog(dialog, cx)
                })
                .expect("app owns settings dialog")
            });
        });
        cx.notify();
    }

    /// Close the window dialog after background work finished its task.
    pub(in crate::app) fn sync_settings_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.settings.take_close_request() {
            window.defer(cx, |window, cx| {
                if window.has_active_dialog(cx) {
                    window.close_dialog(cx);
                }
            });
        }
    }

    /// Cancel buttons close the window dialog themselves; the kit runs
    /// `on_close` only for Escape and its own close button, so they must
    /// release the dialog's state (and stop a QR login) here as well.
    pub(in crate::app) fn cancel_settings_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_dialog_closed();
        window.close_dialog(cx);
        cx.notify();
    }

    fn settings_dialog_closed(&mut self) {
        if matches!(
            self.state.settings.dialog,
            Some(SettingsDialog::BilibiliLogin)
        ) {
            self.state.settings.bilibili_login.cancel();
        }
        self.state.settings.dialog_closed();
    }

    fn render_settings_dialog(&self, dialog: Dialog, cx: &Context<Self>) -> Dialog {
        match &self.state.settings.dialog {
            Some(SettingsDialog::Server(server)) => self.render_server_dialog(dialog, server, cx),
            Some(SettingsDialog::Proxy(proxy)) => self.render_proxy_dialog(dialog, proxy, cx),
            Some(SettingsDialog::BilibiliLogin) => self.render_bilibili_login_dialog(dialog, cx),
            Some(SettingsDialog::RemoveBilibili(source)) => {
                self.render_remove_bilibili_dialog(dialog, source, cx)
            }
            None => dialog,
        }
    }

    /// A connection picker: system proxy, direct, or any saved proxy.
    pub(in crate::app) fn connection_picker(
        &self,
        id: impl Into<SharedString>,
        current: &SourceConnection,
        disabled: bool,
        pick: impl Fn(&mut Self, SourceConnection, &mut Context<Self>) + Clone + 'static,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let current = current.clone();
        Button::new(id.into())
            .label(self.connection_label(&current))
            .icon(AppIcon::Globe)
            .dropdown_caret(true)
            .outline()
            .small()
            .disabled(disabled)
            .dropdown_menu(move |menu, _, cx| {
                let pick = pick.clone();
                let current = current.clone();
                app.update(cx, move |this, cx| {
                    let mut menu = menu;
                    for choice in this.connection_choices() {
                        let pick = pick.clone();
                        let checked = choice == current;
                        menu = menu.item(
                            PopupMenuItem::new(this.connection_label(&choice))
                                .checked(checked)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    pick(this, choice.clone(), cx);
                                    cx.notify();
                                })),
                        );
                    }
                    menu
                })
                .expect("app owns connection menu")
            })
            .into_any_element()
    }

    pub(in crate::app) fn connection_choices(&self) -> Vec<SourceConnection> {
        let mut choices = vec![SourceConnection::System, SourceConnection::Direct];
        choices.extend(
            self.config
                .network
                .proxies
                .iter()
                .map(|proxy| SourceConnection::Proxy(proxy.id.clone())),
        );
        choices
    }

    pub(in crate::app) fn connection_label(&self, connection: &SourceConnection) -> String {
        match connection {
            SourceConnection::System => t!("System proxy").to_string(),
            SourceConnection::Direct => t!("Direct connection").to_string(),
            SourceConnection::Proxy(id) => self
                .config
                .network
                .proxies
                .iter()
                .find(|proxy| &proxy.id == id)
                .map(|proxy| proxy.label().to_string())
                .unwrap_or_else(|| t!("Proxy unavailable").to_string()),
        }
    }
}
