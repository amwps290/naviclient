//! Add/edit/remove dialogs and actions for local music folder sources.

use std::path::PathBuf;

use gpui_kit::component::dialog::Dialog;

use super::state::{LocalFolderDialog, SettingsDialog};
use crate::app::prelude::*;
use crate::source::local::normalize_roots;
use crate::source::{ProviderKind, SourceId, SourceProfile};

impl NavidromeApp {
    pub(in crate::app) fn open_local_folder_dialog(
        &mut self,
        target_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = target_id.and_then(|id| {
            self.config.sources.iter().find_map(|profile| match profile {
                SourceProfile::LocalFolder {
                    id: profile_id,
                    name,
                    roots,
                } if profile_id == id => Some((name.clone(), roots.clone())),
                _ => None,
            })
        });
        if target_id.is_some() && existing.is_none() {
            return;
        }
        let (name, roots) = existing.unwrap_or_default();
        self.local_folder_name_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
        });
        self.local_folder_path_input.update(cx, |input, cx| {
            input.set_value(String::new(), window, cx);
        });
        self.state.settings.open_dialog(SettingsDialog::LocalFolder(
            LocalFolderDialog {
                target: target_id.map(str::to_string),
                roots,
                error: None,
            },
        ));
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn confirm_remove_local_folder(
        &mut self,
        source: SourceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state
            .settings
            .open_dialog(SettingsDialog::RemoveLocalFolder(source));
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn switch_local_folder_source(&mut self, source: &SourceId) {
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

    pub(in crate::app) fn rescan_local_folder_source(&mut self, source: &SourceId) {
        if self.config.active_source.as_ref() == Some(source) {
            self.refresh_connection();
            self.notify_success(t!("Rescanning local music folders…"));
            return;
        }
        let Some(MusicSource::LocalFolder(provider)) =
            crate::app::playback::source_context::configured_source(&self.config, source)
        else {
            return;
        };
        let tx = self.tx.clone();
        self.notify_success(t!("Rescanning local music folders…"));
        self.spawn_future(async move {
            let _ = tokio::task::spawn_blocking(move || provider.rescan()).await;
            let _ = tx;
        });
    }

    fn pick_local_music_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(t!("Select music folder").into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver.await.ok().and_then(Result::ok).flatten();
            if let Some(paths) = selected {
                this.update(cx, |this, cx| {
                    if let Some(SettingsDialog::LocalFolder(dialog)) =
                        this.state.settings.dialog.as_mut()
                    {
                        dialog.roots.extend(paths);
                        dialog.roots = normalize_roots(std::mem::take(&mut dialog.roots));
                        dialog.error = None;
                    }
                    cx.notify();
                })?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    fn add_typed_local_folder_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.local_folder_path_input.read(cx).value().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let added = normalize_roots(vec![PathBuf::from(trimmed)]);
        if let Some(SettingsDialog::LocalFolder(dialog)) = self.state.settings.dialog.as_mut() {
            dialog.roots.extend(added);
            dialog.roots = normalize_roots(std::mem::take(&mut dialog.roots));
            dialog.error = None;
        }
        self.local_folder_path_input.update(cx, |input, cx| {
            input.set_value(String::new(), window, cx);
        });
    }

    fn save_local_folder_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Also absorb any path currently typed into the path input field.
        self.add_typed_local_folder_path(window, cx);
        let Some(SettingsDialog::LocalFolder(state)) = self.state.settings.dialog.clone() else {
            return;
        };
        let roots = normalize_roots(state.roots);
        if roots.is_empty() {
            if let Some(SettingsDialog::LocalFolder(dialog)) = self.state.settings.dialog.as_mut() {
                dialog.error = Some(t!("Add at least one folder path.").to_string());
            }
            return;
        }
        let entered_name = self
            .local_folder_name_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let name = if entered_name.is_empty() {
            roots
                .first()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| t!("Local music").to_string())
        } else {
            entered_name
        };

        let switch_to_new = state.target.is_none();
        let mut next = self.config.clone();
        let target_id = if let Some(existing_id) = state.target {
            if let Some(profile) = next.sources.iter_mut().find(|profile| {
                matches!(profile, SourceProfile::LocalFolder { id, .. } if id == &existing_id)
            }) {
                *profile = SourceProfile::LocalFolder {
                    id: existing_id.clone(),
                    name,
                    roots,
                };
            }
            existing_id
        } else {
            let new_id = next_local_folder_id(&next);
            next.sources.push(SourceProfile::LocalFolder {
                id: new_id.clone(),
                name,
                roots,
            });
            new_id
        };
        let target_source = SourceId::new(ProviderKind::LocalFolder, &target_id);
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

    pub(in crate::app) fn remove_local_folder_source(&mut self, source: SourceId) {
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
        self.retire_playback_source(&source);
        self.server_cache_writer.discard_source(&source);
        let _ = config::remove_source_cache(&source);
        let data_dir = config::source_data_dir(&source);
        let _ = std::fs::remove_dir_all(&data_dir);
        if removing_active {
            self.reconnect_active_server();
        }
        self.prune_source_covers();
    }

    pub(in crate::app) fn render_local_folder_dialog(
        &self,
        dialog: Dialog,
        state: &LocalFolderDialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let p = palette(cx);
        let title = if state.target.is_some() {
            t!("Edit local music folders")
        } else {
            t!("Add local music folder")
        };

        let mut folder_list = v_flex().w_full().gap_1p5();
        if state.roots.is_empty() {
            folder_list = folder_list.child(
                div()
                    .w_full()
                    .px_3()
                    .py_3()
                    .rounded(px(tokens::radius::CONTROL))
                    .bg(p.sunken)
                    .text_xs()
                    .text_color(p.text_muted)
                    .child(t!(
                        "No folders added yet. Browse or paste a folder path below."
                    )),
            );
        } else {
            for (index, root) in state.roots.iter().enumerate() {
                let display = root.display().to_string();
                let exists = root.is_dir();
                folder_list = folder_list.child(
                    h_flex()
                        .w_full()
                        .px_2p5()
                        .py_1p5()
                        .gap_2()
                        .items_center()
                        .rounded(px(tokens::radius::CONTROL))
                        .bg(p.sunken)
                        .child(
                            Icon::new(AppIcon::Folder)
                                .with_size(px(14.0))
                                .text_color(if exists { p.brand } else { cx.theme().warning }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(if exists { p.text } else { cx.theme().warning })
                                .truncate()
                                .child(display),
                        )
                        .child(
                            icon_button(
                                SharedString::from(format!("remove-local-root-{index}")),
                                AppIcon::Trash,
                                t!("Remove folder"),
                                tokens::ICON_BUTTON_XS,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(SettingsDialog::LocalFolder(dialog)) =
                                    this.state.settings.dialog.as_mut()
                                {
                                    if index < dialog.roots.len() {
                                        dialog.roots.remove(index);
                                    }
                                }
                                cx.notify();
                            })),
                        ),
                );
            }
        }

        let body = v_flex()
            .w_full()
            .gap_3()
            .child(self.render_form_row(
                t!("Display name"),
                Input::new(&self.local_folder_name_input)
                    .w_full()
                    .bg(p.sunken)
                    .into_any_element(),
                cx,
            ))
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("Music folders")),
                            )
                            .child(
                                surface_button("browse-local-music-folder")
                                    .label(t!("Choose folder…"))
                                    .icon(AppIcon::Folder)
                                    .compact_action()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pick_local_music_folder(cx);
                                    })),
                            ),
                    )
                    .child(folder_list)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.local_folder_path_input)
                                        .w_full()
                                        .bg(p.sunken),
                                ),
                            )
                            .child(
                                surface_button("add-local-folder-path")
                                    .label(t!("Add"))
                                    .compact_action()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_typed_local_folder_path(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when_some(state.error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            });

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-local-folder")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                primary_button("save-local-folder")
                    .label(t!("Save and scan"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_local_folder_dialog(window, cx);
                        cx.notify();
                    })),
            );

        dialog.title(title).w(px(540.0)).child(body).footer(footer)
    }

    pub(in crate::app) fn render_remove_local_folder_dialog(
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
                SourceProfile::LocalFolder { name, .. } if !name.trim().is_empty() => {
                    Some(name.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| t!("Local music").to_string());
        let remove = source.clone();
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-remove-local-folder")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                danger_button("confirm-remove-local-folder")
                    .label(t!("Remove"))
                    .dialog_action()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_local_folder_source(remove.clone());
                        this.cancel_settings_dialog(window, cx);
                    })),
            );
        dialog
            .title(t!("Remove local music folder?"))
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
                                "Only the library index and local favorites for this source are removed. Your audio files on disk are not modified."
                            )),
                    ),
            )
            .footer(footer)
    }
}

fn next_local_folder_id(config: &Config) -> String {
    let mut index = 1usize;
    loop {
        let candidate = format!("local-{index}");
        let exists = config.sources.iter().any(|profile| {
            matches!(profile, SourceProfile::LocalFolder { id, .. } if id == &candidate)
        });
        if !exists {
            return candidate;
        }
        index += 1;
    }
}
