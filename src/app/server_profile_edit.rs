//! Applying server form edits and coordinating credentials, persistence, and reconnects.

use super::server_edit::{PasswordStorage, ServerForm};
use super::*;

impl NavidromeApp {
    pub(super) fn server_form(&self, cx: &Context<Self>) -> ServerForm {
        ServerForm::new(
            &self.server_input.read(cx).value(),
            &self.username_input.read(cx).value(),
            &self.password_input.read(cx).value(),
            &self.name_input.read(cx).value(),
        )
    }

    pub(super) fn save_settings(&mut self, cx: &Context<Self>) {
        let Some(target) = self.state.settings.server_form_target(&self.config) else {
            return;
        };
        let Some(edit) =
            self.server_form(cx)
                .prepare(&self.config, &target.id, &target.resolved_password())
        else {
            return;
        };
        let reconnect = edit.requires_reconnect();
        if reconnect {
            self.prepare_server_change();
        }

        let profile = edit.profile();
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
        self.config = edit.finish(storage);
        let saved = match config::save(&self.config) {
            Ok(()) => true,
            Err(error) => {
                let message =
                    t!("Config save failed: %{error}", error = format!("{error:#}")).to_string();
                self.state.status = message.clone();
                self.notify_error(message);
                false
            }
        };
        if reconnect {
            self.reconnect_active_server();
        } else if saved {
            self.state.settings.editing_server = None;
            self.notify_success(t!("Saved"));
        }
    }

    /// 把设置页的四个输入框同步为当前服务器（切换后必须调用，否则
    /// 用户看到并保存的是上一台服务器的账号）。密码同样解析后展示，
    /// 已迁入凭据库的条目回显真实密码，用户改密码时无需重新输入。
    pub(super) fn sync_server_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.config.active_profile().cloned() else {
            return;
        };
        self.fill_server_inputs(&profile, window, cx);
    }

    /// 放弃编辑：输入框恢复为表单目标条目的已保存值并收起表单。
    /// 未配置的条目没有可恢复的内容，调用方负责不显示取消按钮。
    pub(super) fn cancel_server_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(profile) = self
            .state
            .settings
            .server_form_target(&self.config)
            .cloned()
        {
            self.fill_server_inputs(&profile, window, cx);
        }
        self.state.settings.editing_server = None;
        cx.notify();
    }

    /// 从指定条目填充表单的四个输入框（打开编辑与取消恢复共用）。
    pub(super) fn fill_server_inputs(
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

    /// 打开指定条目的编辑表单。不切换服务器：修另一台的地址账号
    /// 不该把当前正在浏览/播放的库换掉。
    pub(super) fn begin_edit_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        self.state.settings.editing_server = Some(id.to_string());
        self.fill_server_inputs(&profile, window, cx);
        cx.notify();
    }
}
