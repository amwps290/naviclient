//! 桌面通知、关闭行为与托盘状态。

use super::*;
use desktop_state::TrackNotificationUpdate;

impl NavidromeApp {
    /// 切歌系统通知开关。
    pub(super) fn set_show_track_notifications(&mut self, show: bool, cx: &mut Context<Self>) {
        self.config.show_track_notifications = show;
        if !show {
            self.track_notifier.cancel_pending();
        }
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
        match self.desktop.track_notification_update(
            self.config.show_track_notifications,
            self.audio.progress().active,
            self.config.active_server_id(),
            self.state.now_playing.as_ref().map(|song| song.id.as_str()),
        ) {
            TrackNotificationUpdate::Unchanged => return,
            TrackNotificationUpdate::CancelPending => {
                self.track_notifier.cancel_pending();
                return;
            }
            TrackNotificationUpdate::Send => {}
        }
        let Some(song) = self.state.now_playing.as_ref() else {
            return;
        };
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
        if let Some(paused) = self.desktop.tray_paused_update(&self.audio.state()) {
            tray::set_tray_paused(paused);
        }
    }
}
