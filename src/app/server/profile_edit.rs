//! Applying server form edits and coordinating credentials, persistence, and reconnects.

use super::edit::{PasswordStorage, ServerForm};
use crate::app::prelude::*;
use crate::app::settings::state::{ServerDialog, SettingsDialog};
use crate::models::network::SourceConnection;

impl NavidromeApp {
    pub(in crate::app) fn server_form(&self, cx: &Context<Self>) -> ServerForm {
        ServerForm::new(
            &self.server_input.read(cx).value(),
            &self.username_input.read(cx).value(),
            &self.password_input.read(cx).value(),
            &self.name_input.read(cx).value(),
        )
    }

    /// Save the server dialog: a new server is created and connected, an
    /// existing one is updated. The dialog closes once the change is saved.
    pub(in crate::app) fn save_settings(&mut self, cx: &Context<Self>) {
        let Some(SettingsDialog::Server(dialog)) = self.state.settings.dialog.clone() else {
            return;
        };
        let form = self.server_form(cx);
        let edit = match dialog.target.as_deref() {
            None => form.prepare_new(&self.config),
            Some(_) => {
                let Some(target) = self.state.settings.server_form_target(&self.config) else {
                    return;
                };
                let password = target.resolved_password();
                let id = target.id.clone();
                form.prepare(&self.config, &id, &password)
            }
        };
        let Some(edit) = edit else {
            self.notify_error(t!("Enter the server address and account"));
            return;
        };
        let reconnect = edit.requires_reconnect();
        if reconnect {
            self.prepare_server_change();
        }

        let profile = edit.profile();
        let source =
            crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, &profile.id);
        let storage = if profile.password.is_empty() {
            if edit.delete_existing_password() {
                secrets::delete_password(&profile.id);
            }
            PasswordStorage::Empty
        } else {
            match secrets::store_password(&profile.id, &profile.username, &profile.password) {
                Ok(()) => {
                    log::info!(
                        "stored the password of server {} in the system credential store",
                        profile.id
                    );
                    PasswordStorage::Keyring
                }
                Err(error) => {
                    log::warn!("system credential store unavailable; keeping the password in the config file: {error:#}");
                    PasswordStorage::Plaintext
                }
            }
        };
        let mut next = edit.finish(storage);
        let route_changed = next.source_connection(&source) != dialog.connection;
        if let Err(error) = next.set_source_connection(&source, dialog.connection.clone()) {
            self.notify_error(error.message());
            return;
        }
        let reconnect =
            reconnect || (route_changed && next.active_source.as_ref() == Some(&source));
        if let Err(error) = config::save(&next) {
            let message =
                t!("Config save failed: %{error}", error = format!("{error:#}")).to_string();
            self.state.status = message.clone();
            self.notify_error(message);
            return;
        }
        self.config = next;
        self.state.settings.dismiss_dialog();
        if route_changed {
            self.cancel_source_playback_work(&source);
            if self
                .playback_source
                .as_ref()
                .is_some_and(|api| api.id() == &source)
            {
                self.playback_source = self.available_source(&source);
            }
        }
        if reconnect {
            self.reconnect_active_server();
        }
    }

    /// 把设置页的四个输入框同步为当前服务器（切换后必须调用，否则
    /// 用户看到并保存的是上一台服务器的账号）。密码同样解析后展示，
    /// 已迁入凭据库的条目回显真实密码，用户改密码时无需重新输入。
    pub(in crate::app) fn sync_server_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.config.active_profile().cloned() else {
            return;
        };
        self.fill_server_inputs(&profile, window, cx);
    }

    /// 从指定条目填充表单的四个输入框（打开编辑与取消恢复共用）。
    pub(in crate::app) fn fill_server_inputs(
        &self,
        profile: &ServerProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = profile.name.clone();
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        let url = profile.url.clone();
        self.server_input
            .update(cx, |input, cx| input.set_value(url, window, cx));
        let username = profile.username.clone();
        self.username_input
            .update(cx, |input, cx| input.set_value(username, window, cx));
        let password = profile.resolved_password();
        self.password_input
            .update(cx, |input, cx| input.set_value(password, window, cx));
    }

    /// Open the server dialog for a saved server, or for a new one when
    /// `id` is `None`. Editing never switches the browsing source.
    pub(in crate::app) fn open_server_dialog(
        &mut self,
        id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (profile, connection) = match id {
            Some(id) => {
                let Some(profile) = self
                    .config
                    .servers
                    .iter()
                    .find(|profile| profile.id == id)
                    .cloned()
                else {
                    return;
                };
                let source =
                    crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, id);
                (profile, self.config.source_connection(&source))
            }
            None => (ServerProfile::new("", "", ""), SourceConnection::System),
        };
        self.fill_server_inputs(&profile, window, cx);
        self.state
            .settings
            .open_dialog(SettingsDialog::Server(ServerDialog {
                target: id.map(str::to_owned),
                connection,
            }));
        self.show_settings_dialog(window, cx);
        self.server_input
            .update(cx, |input, cx| input.focus(window, cx));
    }
}
