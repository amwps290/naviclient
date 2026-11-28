//! Settings UI and actions for one-click personal data backup & migration.

use std::path::PathBuf;

use super::components::{settings_group, settings_row};
use crate::app::prelude::*;
use crate::backup::{
    apply_config_backup, apply_equalizer_backup, build_bundle, default_backups_dir, read_bundle,
    write_bundle, write_safety_snapshot, BackupBundle, BackupSelection, RestoreMode,
    RestoreSummary,
};

impl NavidromeApp {
    pub(in crate::app) fn render_backup_settings_group(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let sel = self.state.settings.backup_selection;
        let local_playlists_count = self.local_playlists.list().len();
        let listening_count = self.listening_log.export_all().len();
        let lyrics_count = self.lyrics_cache.export_preferences().songs.len();

        settings_group(
            Some(t!("Backup & migration").into()),
            None,
            vec![
                settings_row(
                    t!("Export or restore your settings, shortcuts, EQ presets, lyrics preferences, local playlists, and listening history. Passwords and login sessions are never exported."),
                    None,
                    div().into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("App settings & shortcuts"),
                    Some(
                        t!("Includes preferences, shortcuts, proxies, and source list (credentials stripped).")
                            .into(),
                    ),
                    Switch::new("backup-include-config")
                        .checked(sel.config)
                        .accessibility_label(t!("App settings & shortcuts"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.state.settings.backup_selection.config = *checked;
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Equalizer presets"),
                    Some(
                        t!(
                            "%{count} custom presets",
                            count = self.config.equalizer.custom_presets.len()
                        )
                        .into(),
                    ),
                    Switch::new("backup-include-eq")
                        .checked(sel.equalizer)
                        .accessibility_label(t!("Equalizer presets"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.state.settings.backup_selection.equalizer = *checked;
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Lyrics selections & timing offsets"),
                    Some(t!("%{count} saved track preferences", count = lyrics_count).into()),
                    Switch::new("backup-include-lyrics")
                        .checked(sel.lyrics_preferences)
                        .accessibility_label(t!("Lyrics selections & timing offsets"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.state.settings.backup_selection.lyrics_preferences = *checked;
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Cross-source local playlists"),
                    Some(t!("%{count} local playlists", count = local_playlists_count).into()),
                    Switch::new("backup-include-playlists")
                        .checked(sel.local_playlists)
                        .accessibility_label(t!("Cross-source local playlists"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.state.settings.backup_selection.local_playlists = *checked;
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Local listening log"),
                    Some(t!("%{count} logged plays", count = listening_count).into()),
                    Switch::new("backup-include-listening")
                        .checked(sel.listening_log)
                        .accessibility_label(t!("Local listening log"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.state.settings.backup_selection.listening_log = *checked;
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Backup archive"),
                    Some(
                        t!("Before restoring, an automatic safety snapshot is saved to your local backups folder.")
                            .into(),
                    ),
                    h_flex()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            primary_button("export-backup-btn")
                                .icon(AppIcon::Download)
                                .label(t!("Export backup…"))
                                .compact_action()
                                .disabled(!sel.any())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.prompt_export_backup(cx);
                                })),
                        )
                        .child(
                            surface_button("restore-backup-merge-btn")
                                .label(t!("Restore (merge)…"))
                                .compact_action()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.prompt_restore_backup(RestoreMode::Merge, cx);
                                })),
                        )
                        .child(
                            ghost_button("restore-backup-overwrite-btn")
                                .label(t!("Restore (overwrite)…"))
                                .compact_action()
                                .text_color(cx.theme().danger)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.prompt_restore_backup(RestoreMode::Overwrite, cx);
                                })),
                        )
                        .into_any_element(),
                    cx,
                ),
            ],
            cx,
        )
    }

    pub(in crate::app) fn prompt_export_backup(&mut self, cx: &mut Context<Self>) {
        let selection: BackupSelection = self.state.settings.backup_selection;
        if !selection.any() {
            return;
        }
        let bundle = build_bundle(
            selection,
            &self.config,
            &self.lyrics_cache,
            &self.local_playlists,
            &self.listening_log,
        );
        let date = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let suggested = format!("naviclient-backup-{date}.navibackup.json");
        let default_dir = directories::UserDirs::new()
            .map(|dirs| {
                dirs.document_dir()
                    .unwrap_or_else(|| dirs.home_dir())
                    .to_path_buf()
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&default_dir, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let selected = receiver.await.ok().and_then(Result::ok).flatten();
            if let Some(mut path) = selected {
                if path.extension().is_none() {
                    path.set_extension("json");
                }
                let result = write_bundle(&path, &bundle);
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(()) => {
                            this.notify_success(t!(
                                "Backup exported to %{path}",
                                path = path.display()
                            ));
                        }
                        Err(error) => {
                            this.notify_error(t!(
                                "Backup export failed: %{error}",
                                error = format!("{error:#}")
                            ));
                        }
                    }
                    cx.notify();
                });
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(in crate::app) fn prompt_restore_backup(
        &mut self,
        mode: RestoreMode,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("Select backup file to restore").into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|mut paths| paths.drain(..).next());
            if let Some(path) = selected {
                let parsed = read_bundle(&path);
                let _ = this.update(cx, |this, cx| {
                    match parsed {
                        Ok(bundle) => match this.apply_backup_bundle(bundle, mode) {
                            Ok(summary) => {
                                this.notify_success(t!(
                                    "Backup restored: %{playlists} playlists, %{lyrics} lyrics, %{plays} listening logs",
                                    playlists = summary.playlists_restored,
                                    lyrics = summary.lyrics_restored,
                                    plays = summary.listening_entries_restored
                                ));
                            }
                            Err(error) => {
                                this.notify_error(t!(
                                    "Backup restore failed: %{error}",
                                    error = format!("{error:#}")
                                ));
                            }
                        },
                        Err(error) => {
                            this.notify_error(t!(
                                "Invalid backup file: %{error}",
                                error = format!("{error:#}")
                            ));
                        }
                    }
                    cx.notify();
                });
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    fn apply_backup_bundle(
        &mut self,
        bundle: BackupBundle,
        mode: RestoreMode,
    ) -> anyhow::Result<RestoreSummary> {
        let snapshot_path = write_safety_snapshot(
            &default_backups_dir(),
            &self.config,
            &self.lyrics_cache,
            &self.local_playlists,
            &self.listening_log,
        )?;
        let overwrite = mode == RestoreMode::Overwrite;
        let mut summary = RestoreSummary {
            safety_snapshot: Some(snapshot_path),
            ..Default::default()
        };

        let mut config_changed = false;
        let mut next_config = self.config.clone();
        if let Some(incoming_cfg) = bundle.config.as_ref() {
            next_config = apply_config_backup(&next_config, incoming_cfg, mode);
            summary.config_restored = true;
            config_changed = true;
        }
        if let Some(incoming_eq) = bundle.equalizer.as_ref() {
            next_config.equalizer =
                apply_equalizer_backup(&next_config.equalizer, incoming_eq, mode);
            summary.equalizer_restored = true;
            config_changed = true;
        }
        if config_changed {
            config::save(&next_config)?;
            self.config = next_config;
            self.audio.set_equalizer(self.config.equalizer.clone());
        }

        if let Some(lyrics_backup) = bundle.lyrics_preferences {
            summary.lyrics_restored = self
                .lyrics_cache
                .import_preferences(lyrics_backup, overwrite)?;
        }
        if let Some(playlists) = bundle.local_playlists {
            summary.playlists_restored = self
                .local_playlists
                .import_playlists(playlists, overwrite)?;
            if let Some(active_id) = self
                .state
                .active_local_playlist
                .as_ref()
                .map(|p| p.id.clone())
            {
                self.state.active_local_playlist = self.local_playlists.get(&active_id).cloned();
            }
        }
        if let Some(entries) = bundle.listening_log {
            summary.listening_entries_restored = self
                .listening_log
                .import_entries(entries, overwrite)?;
            self.state.insights.mark_dirty();
        }
        Ok(summary)
    }
}
