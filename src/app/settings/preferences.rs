//! Playback, cache, language, and theme preferences.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn apply_cache_directory(&mut self, cache_dir: Option<PathBuf>) {
        let mut next = self.config.clone();
        next.cache_dir = cache_dir;
        let effective_dir = match config::prepare_audio_cache_dir(&next) {
            Ok(directory) => directory,
            Err(error) => {
                self.notify_error(t!(
                    "Unable to use cache directory %{dir}: %{error}",
                    dir = config::audio_cache_dir(&next).display(),
                    error = error
                ));
                return;
            }
        };

        if let Err(error) = config::save(&next) {
            self.notify_error(t!(
                "Cache directory save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.config = next;
        self.cancel_next_preload();
        self.audio.set_cache_directory(effective_dir.clone());
        self.sync_offline_store_dir();
        self.state.status = t!("Audio cache: %{dir}", dir = effective_dir.display()).to_string();
    }

    pub(in crate::app) fn refresh_cache_usage(&mut self, cx: &mut Context<Self>) {
        if self.state.settings.cache_busy {
            return;
        }
        self.state.settings.cache_busy = true;
        let cfg = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.runtime.spawn_blocking(move || {
            let _ = tx.send(config::cache_usage(&cfg));
        });
        cx.spawn(async move |this, cx| {
            let usage = loop {
                if let Ok(usage) = rx.try_recv() {
                    break Some(usage);
                }
                Timer::after(Duration::from_millis(25)).await;
            };
            let _ = this.update(cx, |this, cx| {
                this.state.settings.cache_busy = false;
                if let Some(usage) = usage {
                    this.state.settings.cache_usage = Some(usage);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn clear_audio_cache_action(&mut self, cx: &mut Context<Self>) {
        if self.state.settings.cache_busy {
            return;
        }
        self.cancel_next_preload();
        self.state.settings.cache_busy = true;
        let cfg = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.runtime.spawn_blocking(move || {
            let result = config::clear_audio_cache(&cfg).map_err(|e| e.to_string());
            let usage = config::cache_usage(&cfg);
            let _ = tx.send((result, usage));
        });
        cx.spawn(async move |this, cx| {
            let outcome = loop {
                if let Ok(outcome) = rx.try_recv() {
                    break outcome;
                }
                Timer::after(Duration::from_millis(25)).await;
            };
            let _ = this.update(cx, |this, cx| {
                this.state.settings.cache_busy = false;
                this.state.settings.cache_usage = Some(outcome.1);
                match outcome.0 {
                    Ok(freed) => this.notify_success(t!(
                        "Cleared audio cache (%{size})",
                        size = crate::app::playback::display::format_file_size(freed)
                    )),
                    Err(error) => this
                        .notify_error(t!("Could not clear audio cache: %{error}", error = error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn clear_cover_cache_action(&mut self, cx: &mut Context<Self>) {
        if self.state.settings.cache_busy {
            return;
        }
        self.state.settings.cache_busy = true;
        let cfg = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.runtime.spawn_blocking(move || {
            let result = config::clear_cover_cache(&cfg).map_err(|e| e.to_string());
            let usage = config::cache_usage(&cfg);
            let _ = tx.send((result, usage));
        });
        cx.spawn(async move |this, cx| {
            let outcome = loop {
                if let Ok(outcome) = rx.try_recv() {
                    break outcome;
                }
                Timer::after(Duration::from_millis(25)).await;
            };
            let _ = this.update(cx, |this, cx| {
                this.state.settings.cache_busy = false;
                this.state.settings.cache_usage = Some(outcome.1);
                match outcome.0 {
                    Ok(freed) => this.notify_success(t!(
                        "Cleared cover cache (%{size})",
                        size = crate::app::playback::display::format_file_size(freed)
                    )),
                    Err(error) => this
                        .notify_error(t!("Could not clear cover cache: %{error}", error = error)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn set_audio_cache_limit_mb(
        &mut self,
        limit_mb: u64,
        cx: &mut Context<Self>,
    ) {
        if self.config.audio_cache_limit_mb == limit_mb {
            return;
        }
        self.config.audio_cache_limit_mb = limit_mb;
        self.audio.set_cache_limit_mb(limit_mb);
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.refresh_cache_usage(cx);
    }

    pub(in crate::app) fn set_pause_on_output_disconnect(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.config.pause_on_output_disconnect = enabled;
        self.audio.set_pause_on_output_disconnect(enabled);
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    pub(in crate::app) fn choose_cache_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("Select audio cache folder").into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|paths| paths.into_iter().next());
            if let Some(path) = selected {
                this.update(cx, |this, cx| {
                    this.apply_cache_directory(Some(path));
                    cx.notify();
                })?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(in crate::app) fn set_transcoding_quality(
        &mut self,
        quality: TranscodingQuality,
        cx: &mut Context<Self>,
    ) {
        self.cancel_next_preload();
        self.config.transcoding_quality = quality;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Playback quality save failed: %{error}",
                error = format!("{error:#}")
            ));
        } else {
            log::info!(
                "transcoding quality changed; quality={}",
                quality.cache_profile()
            );
        }
        cx.notify();
    }

    pub(in crate::app) fn set_volume_normalization(
        &mut self,
        mode: VolumeNormalization,
        cx: &mut Context<Self>,
    ) {
        self.cancel_next_preload();
        self.config.volume_normalization = mode;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Volume normalization save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 歌词翻译行显示开关；服务器无多语言变体时该开关无效果。
    pub(in crate::app) fn set_show_lyrics_translation(
        &mut self,
        show: bool,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.config.clone();
        next.show_lyrics_translation = show;
        if let Err(error) = config::save(&next) {
            self.notify_error(t!(
                "Lyrics translation save failed: %{error}",
                error = format!("{error:#}")
            ));
        } else {
            self.config = next;
            self.notify_success(t!("Lyrics settings saved."));
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_remaining_time(&mut self, cx: &mut Context<Self>) {
        self.config.show_remaining_time = !self.config.show_remaining_time;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    pub(in crate::app) fn set_mini_show_lyrics(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.mini_show_lyrics = enabled;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 切换界面语言：写配置 + set_locale，下一次渲染整份 UI 文案生效。
    pub(in crate::app) fn set_language(
        &mut self,
        preference: LanguagePreference,
        cx: &mut Context<Self>,
    ) {
        if self.config.language == preference {
            return;
        }
        self.config.language = preference;
        let locale = preference.resolve();
        rust_i18n::set_locale(locale);
        crate::typography::configure_ui_font(cx, locale);
        crate::typography::configure_serif_font(cx, locale);
        // 托盘菜单是原生控件：重建一次让它也换成新语言。
        tray::refresh_labels();
        if let Err(error) = config::save(&self.config) {
            self.state.status =
                t!("Config save failed: %{error}", error = format!("{error:#}")).to_string();
        }
        cx.notify();
    }

    pub(in crate::app) fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = preference;
        match preference {
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
        }
        if let Err(error) = config::save(&self.config) {
            self.state.status =
                t!("Theme save failed: %{error}", error = format!("{error:#}")).to_string();
        }
        cx.notify();
    }
}
