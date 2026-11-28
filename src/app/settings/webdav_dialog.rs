//! Add/edit/remove dialogs and actions for WebDAV / Alist storage sources.

use gpui_kit::component::dialog::Dialog;

use super::state::{SettingsDialog, WebDavDialog};
use crate::app::prelude::*;
use crate::models::network::SourceConnection;
use crate::source::webdav::{
    normalize_root_path, normalize_webdav_url, resolve_webdav_password, webdav_credential_key,
};
use crate::source::{ProviderKind, SourceId, SourceProfile};

impl NavidromeApp {
    pub(in crate::app) fn open_webdav_dialog(
        &mut self,
        target_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = target_id.and_then(|id| {
            self.config.sources.iter().find_map(|profile| match profile {
                SourceProfile::WebDav {
                    id: profile_id,
                    name,
                    base_url,
                    root_path,
                    username,
                    password,
                    password_in_keyring,
                } if profile_id == id => Some((
                    name.clone(),
                    base_url.clone(),
                    root_path.clone(),
                    username.clone(),
                    resolve_webdav_password(profile_id, password, *password_in_keyring),
                )),
                _ => None,
            })
        });
        if target_id.is_some() && existing.is_none() {
            return;
        }
        let connection = target_id
            .map(|id| {
                self.config
                    .source_connection(&SourceId::new(ProviderKind::WebDav, id))
            })
            .unwrap_or(SourceConnection::System);
        let (name, base_url, root_path, username, password) =
            existing.unwrap_or_else(|| (String::new(), String::new(), "/".into(), String::new(), String::new()));

        self.webdav_name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.webdav_url_input
            .update(cx, |input, cx| input.set_value(base_url, window, cx));
        self.webdav_root_input
            .update(cx, |input, cx| input.set_value(root_path, window, cx));
        self.webdav_username_input
            .update(cx, |input, cx| input.set_value(username, window, cx));
        self.webdav_password_input
            .update(cx, |input, cx| input.set_value(password, window, cx));

        self.state.settings.open_dialog(SettingsDialog::WebDav(WebDavDialog {
            target: target_id.map(str::to_string),
            connection,
            error: None,
        }));
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn confirm_remove_webdav(
        &mut self,
        source: SourceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state
            .settings
            .open_dialog(SettingsDialog::RemoveWebDav(source));
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn switch_webdav_source(&mut self, source: &SourceId) {
        if self.config.active_source.as_ref() == Some(source) {
            self.state.settings.close();
            return;
        }
        if !self
            .config
            .sources
            .iter()
            .any(|profile| profile.id() == *source)
        {
            return;
        }
        self.prepare_server_change();
        let mut next = self.config.clone();
        if !next.switch_source(source) {
            return;
        }
        if let Err(error) = config::save(&next) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.config = next;
        self.state.status =
            t!("Switched to %{server}", server = self.active_server_label()).to_string();
        self.reconnect_active_server();
    }

    pub(in crate::app) fn rescan_webdav_source(&mut self, source: &SourceId) {
        if self.config.active_source.as_ref() == Some(source) {
            self.refresh_connection();
            self.notify_success(t!("Scanning WebDAV library…"));
            return;
        }
        let Some(MusicSource::WebDav(provider)) =
            crate::app::playback::source_context::configured_source(&self.config, source)
        else {
            return;
        };
        self.notify_success(t!("Scanning WebDAV library…"));
        self.spawn_future(async move {
            let _ = provider.rescan().await;
        });
    }

    fn save_webdav_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(SettingsDialog::WebDav(state)) = self.state.settings.dialog.clone() else {
            return;
        };
        let raw_url = self.webdav_url_input.read(cx).value().to_string();
        let normalized_url = match normalize_webdav_url(&raw_url) {
            Ok(url) => url,
            Err(error) => {
                if let Some(SettingsDialog::WebDav(dialog)) = self.state.settings.dialog.as_mut() {
                    dialog.error = Some(error.to_string());
                }
                return;
            }
        };
        let entered_name = self.webdav_name_input.read(cx).value().trim().to_string();
        let root_path = normalize_root_path(self.webdav_root_input.read(cx).value().as_ref());
        let username = self.webdav_username_input.read(cx).value().trim().to_string();
        let password = self.webdav_password_input.read(cx).value().to_string();

        let name = if entered_name.is_empty() {
            url::Url::parse(&normalized_url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "WebDAV".to_string())
        } else {
            entered_name
        };

        let switch_to_new = state.target.is_none();
        let mut next = self.config.clone();
        let target_id = state
            .target
            .clone()
            .unwrap_or_else(|| next_webdav_id(&next));

        // Store password in OS credential store when available; fall back cleanly to config if unavailable.
        let (stored_password, password_in_keyring) = if password.is_empty() {
            secrets::delete_password(&webdav_credential_key(&target_id));
            (String::new(), false)
        } else if secrets::store_password(&webdav_credential_key(&target_id), &username, &password)
            .is_ok()
        {
            (String::new(), true)
        } else {
            (password, false)
        };

        let profile = SourceProfile::WebDav {
            id: target_id.clone(),
            name,
            base_url: normalized_url,
            root_path,
            username,
            password: stored_password,
            password_in_keyring,
        };

        if let Some(existing) = next.sources.iter_mut().find(|item| {
            matches!(item, SourceProfile::WebDav { id, .. } if id == &target_id)
        }) {
            *existing = profile;
        } else {
            next.sources.push(profile);
        }

        let target_source = SourceId::new(ProviderKind::WebDav, &target_id);
        let _ = next.set_source_connection(&target_source, state.connection);
        if switch_to_new {
            self.prepare_server_change();
            next.active_source = Some(target_source.clone());
        }

        if let Err(error) = config::save(&next) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.config = next;
        let is_active = self.config.active_source.as_ref() == Some(&target_source);
        if is_active {
            self.reconnect_active_server();
        }
        self.state.settings.dismiss_dialog();
        self.sync_settings_dialog(window, cx);
    }

    pub(in crate::app) fn remove_webdav_source(&mut self, source: SourceId) {
        if self.config.sources.len() <= 1 {
            self.notify_error(t!("Keep at least one music source configured"));
            return;
        }
        let removing_active = self.config.active_source.as_ref() == Some(&source);
        if removing_active {
            self.prepare_server_change();
        }
        let mut next = self.config.clone();
        next.sources.retain(|profile| profile.id() != source);
        if next.active_source.as_ref() == Some(&source) {
            next.active_source = next.sources.first().map(SourceProfile::id);
        }
        if let Err(error) = config::save(&next) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.config = next;
        secrets::delete_password(&webdav_credential_key(&source.profile_id));
        self.retire_playback_source(&source);
        self.server_cache_writer.discard_source(&source);
        let _ = config::remove_source_cache(&source);
        let _ = std::fs::remove_dir_all(config::source_data_dir(&source));
        if removing_active {
            self.reconnect_active_server();
        }
        self.prune_source_covers();
    }

    pub(in crate::app) fn render_webdav_dialog(
        &self,
        dialog: Dialog,
        state: &WebDavDialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let p = palette(cx);
        let title = if state.target.is_some() {
            t!("Edit WebDAV storage")
        } else {
            t!("Add WebDAV / Alist storage")
        };

        let form = v_flex()
            .w_full()
            .gap_3()
            .child(self.render_form_row(
                t!("Display name"),
                Input::new(&self.webdav_name_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("WebDAV URL"),
                Input::new(&self.webdav_url_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("Root path"),
                Input::new(&self.webdav_root_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("Username"),
                Input::new(&self.webdav_username_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("Password"),
                Input::new(&self.webdav_password_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(self.render_form_row(
                t!("Connection"),
                self.connection_picker(
                    "webdav-dialog-connection",
                    &state.connection,
                    false,
                    |this, choice, _| {
                        if let Some(SettingsDialog::WebDav(webdav)) =
                            this.state.settings.dialog.as_mut()
                        {
                            webdav.connection = choice;
                        }
                    },
                    cx,
                ),
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(p.text_secondary)
                    .child(t!(
                        "Works with Synology/fnOS/TrueNAS WebDAV, Nextcloud, and Alist / OpenList."
                    )),
            )
            .when_some(state.error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            });

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-webdav-dialog")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                primary_button("save-webdav-dialog")
                    .label(t!("Save and scan"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_webdav_dialog(window, cx);
                        cx.notify();
                    })),
            );

        dialog.title(title).w(px(540.0)).child(form).footer(footer)
    }

    pub(in crate::app) fn render_remove_webdav_dialog(
        &self,
        dialog: Dialog,
        source: &SourceId,
        cx: &Context<Self>,
    ) -> Dialog {
        let label = self
            .config
            .sources
            .iter()
            .find(|profile| profile.id() == *source)
            .and_then(|profile| match profile {
                SourceProfile::WebDav { name, .. } if !name.trim().is_empty() => {
                    Some(name.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "WebDAV".to_string());
        let remove = source.clone();
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-remove-webdav")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                danger_button("confirm-remove-webdav")
                    .label(t!("Remove"))
                    .dialog_action()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_webdav_source(remove.clone());
                        this.cancel_settings_dialog(window, cx);
                    })),
            );
        dialog
            .title(t!("Remove WebDAV storage?"))
            .w(px(440.0))
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child(t!(
                        "Remove “%{label}” from this app?",
                        label = label
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(t!(
                                "Its saved credentials and local library index will be deleted. Remote files on your server are not modified."
                            )),
                    ),
            )
            .footer(footer)
    }
}

fn next_webdav_id(config: &Config) -> String {
    let mut index = 1usize;
    loop {
        let candidate = format!("webdav-{index}");
        let exists = config
            .sources
            .iter()
            .any(|profile| matches!(profile, SourceProfile::WebDav { id, .. } if id == &candidate));
        if !exists {
            return candidate;
        }
        index += 1;
    }
}
