//! Read-only track metadata dialog and deferred menu focus handoff.

use crate::app::playback::display::format_file_size;
use crate::app::prelude::*;

impl NavidromeApp {
    /// 打开“曲目信息”对话框。对话框挂在窗口全局层，菜单入口先让
    /// 菜单完成关闭再挂载，避免其焦点恢复吞掉 Enter/Escape。
    pub(in crate::app) fn open_song_info(
        &mut self,
        song: Song,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.song_info = Some(song);
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            let render_app = app.clone();
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_song_info_dialog(dialog, cx))
                        .expect("app owns song info dialog")
                });
            });
        }
        cx.notify();
    }

    /// 曲目信息对话框：服务器元数据全览（标签、音轨号、时长、格式、
    /// 码率、文件大小、路径），只读。
    fn render_song_info_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some(song) = &self.state.song_info else {
            return dialog;
        };
        let info_row = |content: gpui_kit::Div, label: &str, value: String| {
            content.child(
                h_flex()
                    .gap_3()
                    .items_baseline()
                    .child(
                        div()
                            .w(px(96.0))
                            .flex_none()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().flex_1().min_w_0().text_sm().truncate().child(value)),
            )
        };
        let mut info = v_flex().gap_2();
        info = info_row(info, &t!("Title"), song.title.clone());
        if !song.artist.trim().is_empty() {
            info = info_row(info, &t!("Artist"), song.artist.clone());
        }
        if !song.album.trim().is_empty() {
            info = info_row(info, &t!("Album"), song.album.clone());
        }
        if song.track.is_some() || song.disc.is_some() {
            let value = match (song.disc, song.track) {
                (Some(disc), Some(track)) => t!(
                    "Disc %{disc} · Track %{track}",
                    disc = disc,
                    track = format!("{track:02}")
                ),
                (Some(disc), None) => t!("Disc %{disc}", disc = disc),
                (None, Some(track)) => t!("Track %{track}", track = format!("{track:02}")),
                (None, None) => std::borrow::Cow::Borrowed(""),
            };
            info = info_row(info, &t!("Position"), value.to_string());
        }
        info = info_row(info, &t!("Duration"), format_duration(song.duration));
        if song.suffix.as_deref().is_some_and(|s| !s.is_empty()) {
            let mut value = song.suffix.as_deref().unwrap_or_default().to_uppercase();
            if let Some(bit_rate) = song.bit_rate {
                value.push_str(&format!(" · {bit_rate} kbps"));
            }
            if let Some(size) = song.size {
                value.push_str(&format!(" · {}", format_file_size(size.max(0) as u64)));
            }
            info = info_row(info, &t!("Format"), value.to_string());
        }
        if let Some(path) = &song.path {
            if !path.is_empty() {
                info = info_row(info, &t!("File"), path.clone());
            }
        }
        info = info_row(info, &t!("ID"), song.id.clone());
        dialog
            .title(t!("Track info"))
            .w(px(440.0))
            .close_button(true)
            .keyboard(true)
            .on_close(cx.listener(|this, _, _, cx| {
                this.state.song_info = None;
                cx.notify();
            }))
            .child(
                v_flex().gap_2().p_1().child(info).child(
                    div()
                        .pt_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("Metadata from the Navidrome server.")),
                ),
            )
    }
}
