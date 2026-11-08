//! Switching, adding and removing server profiles and their scoped data.

use crate::app::prelude::*;

impl NavidromeApp {
    /// 切换到另一台已保存的服务器。切换前先把当前服务器的作用域数据落盘。
    pub(in crate::app) fn switch_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.config.active_source.as_ref()
            == Some(&crate::source::SourceId::new(
                crate::source::ProviderKind::OpenSubsonic,
                id,
            ))
        {
            return;
        }
        if !self.config.servers.iter().any(|profile| profile.id == id) {
            return;
        }
        self.prepare_server_change();
        if !self.config.switch_to(id) {
            return;
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!(
            "switched server; id={id} url={} user={}",
            self.config.server_url,
            self.config.username
        );
        self.state.status =
            t!("Switched to %{server}", server = self.active_server_label()).to_string();
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
    }

    /// 新增一台空白服务器并立即切过去，方便直接填地址账号。
    pub(in crate::app) fn add_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_server_change();
        let id = self.config.add_server();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!("added server entry; id={id}");
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
        // 新条目直接展开表单：新增的意图就是填写连接信息。
        self.state.settings.editing_server = Some(id);
        self.state.status = t!("Enter the address and account for the new server").to_string();
    }

    /// 删除一台服务器；正在使用的条目被删除时自动切到相邻条目。
    /// clean_data = true 时一并清理这台服务器的全部作用域缓存与系统
    /// 凭据库条目；凭据删除不可恢复，重新添加同一台
    /// 服务器需要重输密码，因此默认保留。
    pub(in crate::app) fn remove_server(
        &mut self,
        id: &str,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let removing_active = self.config.active_source.as_ref()
            == Some(&crate::source::SourceId::new(
                crate::source::ProviderKind::OpenSubsonic,
                id,
            ));
        let Some(profile) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        if removing_active && self.config.servers.len() > 1 {
            self.prepare_server_change();
        }
        if !self.config.remove_server(id) {
            self.notify_error(t!("Keep at least one server configured"));
            return;
        }
        self.retire_playback_source(&crate::source::SourceId::new(
            crate::source::ProviderKind::OpenSubsonic,
            id,
        ));
        if clean_data {
            self.server_cache_writer
                .discard_source(&crate::source::SourceId::new(
                    crate::source::ProviderKind::OpenSubsonic,
                    id,
                ));
            if profile.password_in_keyring {
                secrets::delete_password(id);
            }
            let removed = config::remove_source_cache(&crate::source::SourceId::new(
                crate::source::ProviderKind::OpenSubsonic,
                id,
            ));
            log::info!("removed {removed} scoped cache file(s) of server {id}");
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!("removed server entry; id={id}");
        // 正在编辑的条目被删掉：表单不能指向不存在的 id。
        if self.state.settings.editing_server.as_deref() == Some(id) {
            self.state.settings.editing_server = None;
        }
        if removing_active {
            self.reconnect_active_server();
            self.sync_server_inputs(window, cx);
        }
        self.prune_source_covers();
        self.state.status = t!("Removed %{server}", server = profile.label()).to_string();
    }

    /// 当前服务器的显示名（侧边栏与设置页顶部共用）。
    pub(in crate::app) fn active_server_label(&self) -> String {
        if self.browsing_bilibili() {
            return self
                .config
                .sources
                .iter()
                .find(|profile| Some(profile.id()) == self.config.active_source)
                .and_then(|profile| match profile {
                    crate::source::SourceProfile::Bilibili { name, .. } => {
                        Some(format!("Bilibili · {name}"))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| "Bilibili".into());
        }
        self.config
            .active_profile()
            .map(|profile| profile.label())
            .unwrap_or_else(|| t!("No server").to_string())
    }
}
