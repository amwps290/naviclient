//! 迷你窗口、音量交互、桌面通知、关闭行为和托盘状态。

use super::*;

impl NavidromeApp {
    pub(super) fn enter_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings_open = false;
        self.mini_mode = true;
        self.restore_size = Some(window.viewport_size());
        self.restore_maximized = window.is_maximized();
        // The window shadow/border (Linux client-side decorations) shrinks the
        // drawable area by its padding, so request a larger outer window to keep
        // the mini player content at the intended size on every platform.
        let paddings = window_paddings(window);
        let mini_size = size(
            MINI_WINDOW_WIDTH + paddings.left + paddings.right,
            MINI_WINDOW_HEIGHT + paddings.top + paddings.bottom,
        );
        // Lock the mini window to a fixed size: remember the target and snap back
        // to it in render() if the user drags a border to resize it.
        self.mini_target_size = Some(mini_size);
        window.resize(mini_size);
        set_always_on_top(window, true);
        cx.notify();
    }

    pub(super) fn exit_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mini_mode = false;
        self.mini_target_size = None;
        window.resize(
            self.restore_size
                .take()
                .unwrap_or_else(|| size(px(1280.0), px(820.0))),
        );
        if self.restore_maximized && !window.is_maximized() {
            window.zoom_window();
        }
        self.restore_maximized = false;
        set_always_on_top(window, false);
        cx.notify();
    }

    pub(super) fn toggle_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mini_mode {
            self.exit_mini_mode(window, cx);
        } else {
            self.enter_mini_mode(window, cx);
        }
    }

    pub(super) fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = normalize_volume(volume);
        self.muted = false;
        self.config.volume = volume;
        if volume > 0.001 {
            self.volume_before_mute = volume;
        }
        self.audio.set_volume(volume);
        self.schedule_volume_save(cx);
        cx.notify();
    }

    pub(super) fn adjust_volume(
        &mut self,
        delta: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let volume = normalize_volume(self.config.volume + delta);
        self.volume_slider.update(cx, |slider, cx| {
            slider.set_value(volume * 100.0, window, cx);
        });
        self.set_volume(volume, cx);
    }

    pub(super) fn toggle_mute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.muted || self.config.volume <= 0.001 {
            let volume = restored_volume(self.volume_before_mute);
            self.muted = false;
            self.config.volume = volume;
            self.audio.set_volume(volume);
            self.volume_slider.update(cx, |slider, cx| {
                slider.set_value(volume * 100.0, window, cx);
            });
            self.schedule_volume_save(cx);
        } else {
            self.volume_before_mute = self.config.volume;
            self.muted = true;
            self.audio.set_volume(0.0);
        }
        cx.notify();
    }

    pub(super) fn schedule_volume_save(&mut self, cx: &mut Context<Self>) {
        self.volume_save_generation = self.volume_save_generation.wrapping_add(1);
        let generation = self.volume_save_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(500)).await;
            this.update(cx, |this, cx| {
                if this.volume_save_generation != generation {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(t!(
                        "Volume save failed: %{error}",
                        error = format!("{error:#}")
                    ));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// 迷你窗口滚轮调音量时在进度线位置闪光显示音量；
    /// 计时结束后补一次重绘，避免暂停（无周期重绘）时闪光停留。
    pub(super) fn show_mini_volume_flash(&mut self, cx: &mut Context<Self>) {
        self.mini_volume_flash = Some(std::time::Instant::now());
        self.mini_volume_flash_generation = self.mini_volume_flash_generation.wrapping_add(1);
        let generation = self.mini_volume_flash_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(1100)).await;
            this.update(cx, |this, cx| {
                if this.mini_volume_flash_generation == generation {
                    this.mini_volume_flash = None;
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(super) fn show_volume_panel(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_generation = self.volume_panel_generation.wrapping_add(1);
        self.volume_panel_open = true;
        cx.notify();
    }

    pub(super) fn schedule_volume_panel_close(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_generation = self.volume_panel_generation.wrapping_add(1);
        let generation = self.volume_panel_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(220)).await;
            this.update(cx, |this, cx| {
                if this.volume_panel_generation == generation && !this.volume_panel_dragging {
                    this.volume_panel_open = false;
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(super) fn begin_volume_panel_drag(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_dragging = true;
        self.show_volume_panel(cx);
    }

    pub(super) fn end_volume_panel_drag(&mut self, keep_open: bool, cx: &mut Context<Self>) {
        self.volume_panel_dragging = false;
        if keep_open {
            self.show_volume_panel(cx);
        } else {
            self.schedule_volume_panel_close(cx);
        }
    }

    pub(super) fn open_now_playing(&mut self) {
        if self.state.view != View::NowPlaying {
            self.state.view_before_now_playing = self.state.view;
        }
        self.state.settings_open = false;
        self.state.view = View::NowPlaying;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    pub(super) fn leave_now_playing(&mut self) {
        self.state.view = self.state.view_before_now_playing;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    /// 切歌系统通知开关。
    pub(super) fn set_show_track_notifications(&mut self, show: bool, cx: &mut Context<Self>) {
        self.config.show_track_notifications = show;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Notification setting save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 关闭按钮最小化开关。
    pub(super) fn set_close_minimizes(&mut self, minimize: bool, cx: &mut Context<Self>) {
        self.config.close_minimizes = minimize;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Close behavior save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 标题栏 X 按钮（Linux CSD）的关闭处理：按设置最小化或退出。
    /// on_close_window 仅 Linux 生效，其他平台维持默认行为。
    pub(super) fn close_button_handler(
        cx: &Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        let app = cx.entity();
        move |_, window, cx| {
            app.update(cx, |this, _| {
                if this.config.close_minimizes {
                    window.minimize_window();
                } else {
                    window.remove_window();
                }
            });
        }
    }

    /// 切歌系统通知：正在播放曲目变化且音频已接通时发送一次。
    /// 音频未接通（切换瞬间）时先不记录，等下一次轮询再发。
    pub(super) fn maybe_notify_track_started(&mut self) {
        if !self.config.show_track_notifications {
            return;
        }
        let Some(song) = self.state.now_playing.clone() else {
            self.last_notified_track = None;
            return;
        };
        if self.last_notified_track.as_deref() == Some(song.id.as_str()) {
            return;
        }
        if !self.audio.state().active {
            return;
        }
        self.last_notified_track = Some(song.id.clone());
        let cover = song
            .cover_art
            .as_deref()
            .map(|id| config::cover_cache_path(&self.config, self.config.active_server_id(), id));
        self.track_notifier.track_changed(
            &song.title,
            &notify::notification_body(&song.artist, &song.album),
            cover.as_deref(),
        );
    }

    /// 播放/暂停状态变化时同步托盘菜单文案（Play/Pause）。
    pub(super) fn sync_tray_paused(&mut self) {
        let paused = self.audio.state().paused;
        if self.last_tray_paused != Some(paused) {
            self.last_tray_paused = Some(paused);
            tray::set_tray_paused(paused);
        }
    }
}
