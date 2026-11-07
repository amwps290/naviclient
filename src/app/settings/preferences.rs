//! Playback, cache, language, and theme preferences.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn apply_cache_directory(&mut self, cache_dir: Option<PathBuf>) {
        let effective_dir = cache_dir
            .clone()
            .unwrap_or_else(config::default_audio_cache_dir);
        if let Err(error) = fs::create_dir_all(&effective_dir) {
            self.notify_error(t!(
                "Unable to use cache directory %{dir}: %{error}",
                dir = effective_dir.display(),
                error = error
            ));
            return;
        }

        self.config.cache_dir = cache_dir;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Cache directory save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.cancel_next_preload();
        self.audio.set_cache_directory(effective_dir.clone());
        self.state.status = t!("Audio cache: %{dir}", dir = effective_dir.display()).to_string();
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
        self.config.show_lyrics_translation = show;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Lyrics translation save failed: %{error}",
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
