//! Background polling orchestration, independent of application construction.

use super::redraw::{PlaybackPresentation, PlaybackRedrawState, RedrawRequest, POLL_INTERVAL};
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn start_polling(window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let mut redraw = PlaybackRedrawState::new(std::time::Instant::now());
            loop {
                Timer::after(POLL_INTERVAL).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        this.poll_background(window, cx, &mut redraw);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_background(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        redraw: &mut PlaybackRedrawState,
    ) {
        self.sync_gapless_transition();
        let mut changed = self.poll_tray_commands(window);
        changed |= self.poll_global_shortcuts(window, cx);
        changed |= self.poll_media_commands(window, cx);
        // 单实例激活信号：显示并置前窗口。
        if single_instance::poll_activation() {
            if let Some(hwnd) = self.main_hwnd {
                show_main_window(hwnd);
            }
        }
        self.maybe_save_session();
        changed |= self.poll_messages(window, cx);
        self.release_unused_cover_textures(window, cx);
        // 冲刷排队的 Toast：gpui-component 的通知系统由窗口 Root
        // 托管（自动消失、可关闭、主题化），需要 Window 句柄。
        for (kind, message) in self.pending_notifications.drain(..) {
            window.push_notification(
                match kind {
                    NotificationType::Error => Notification::error(message),
                    NotificationType::Info => Notification::info(message),
                    NotificationType::Success => Notification::success(message),
                    NotificationType::Warning => Notification::warning(message),
                },
                cx,
            );
        }
        self.handle_playback_end();
        self.poll_playback_reconnect();
        self.maybe_preload_next();
        self.maybe_scrobble_current();
        self.poll_listening_history();
        self.maybe_notify_track_started();
        self.sync_tray_paused();
        let scrolling = self.state.lyrics.scroll_target.is_some();
        self.update_active_lyric();
        changed |= self.vinyl_cover.poll_ready();
        let playback = self.audio.state();
        self.sync_media_session(&playback);
        // 可见但未聚焦的窗口仍应更新进度、歌词和动画。只有真正
        // 隐藏/最小化时才暂停重绘，避免切到别的应用后界面“冻住”。
        let visible = window.is_visible();
        let presentation = self.playback_presentation();
        changed |= self.vinyl_cover.tick(
            std::time::Instant::now(),
            presentation.animate_cover(&playback, visible),
        );
        if !visible {
            self.vinyl_cover.cancel_pending();
        }
        if redraw.should_redraw(
            std::time::Instant::now(),
            RedrawRequest {
                playback,
                presentation,
                visible,
                changed,
                scrolling,
            },
        ) {
            cx.notify();
        }
    }

    pub(in crate::app) fn playback_presentation(&self) -> PlaybackPresentation {
        PlaybackPresentation::for_view(
            self.state.view,
            self.state.settings.open,
            self.mini_window.is_mini(),
        )
    }
}
