//! Clickable artist and album metadata with small navigation targets.

use crate::app::library::navigation::{album_for_song, artist_for_song};
use crate::app::prelude::*;

impl NavidromeApp {
    /// 可点击的元数据文本（艺术家/专辑名）：hover 变强调色，点击执行
    /// 导航并阻止冒泡（行/卡片的整体点击是播放或打开详情）。
    /// 注意这里只阻断鼠标冒泡，与键盘输入路径无关。
    pub(in crate::app) fn clickable_meta(
        &self,
        id: impl Into<SharedString>,
        text: String,
        cx: &Context<Self>,
        open: impl Fn(&mut Self) + 'static,
    ) -> gpui_kit::AnyElement {
        div()
            .id(id.into())
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .hover(|style| style.text_color(cx.theme().primary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                open(this);
                cx.notify();
            }))
            .child(text)
            .into_any_element()
    }

    /// 艺术家名文本：有 artist_id 时可点导航到艺术家页，否则静态显示。
    pub(in crate::app) fn artist_name_link(
        &self,
        id: impl Into<SharedString>,
        name: String,
        artist_id: Option<String>,
        cover_art: Option<String>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match artist_id.filter(|id| !id.trim().is_empty()) {
            Some(artist_id) => self.clickable_meta(id, name.clone(), cx, move |this| {
                this.open_artist(Artist {
                    id: artist_id.clone(),
                    name: name.clone(),
                    album_count: None,
                    cover_art: cover_art.clone(),
                });
            }),
            None => div().min_w_0().truncate().child(name).into_any_element(),
        }
    }

    /// 歌曲行的专辑名文本：可点导航到专辑页。
    pub(in crate::app) fn song_album_link(
        &self,
        id: impl Into<SharedString>,
        display_name: String,
        song: &Song,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if let Some(part) = &song.bilibili_part {
            return div()
                .min_w_0()
                .truncate()
                .child(format!("P{}/{} · {}", part.index, part.count, part.title))
                .into_any_element();
        }
        match album_for_song(song).filter(|_| self.browsing_owns_song(song)) {
            Some(_) => {
                let song = song.clone();
                self.clickable_meta(id, display_name, cx, move |this| {
                    this.open_song_album(&song);
                })
            }
            None => div()
                .min_w_0()
                .truncate()
                .child(display_name)
                .into_any_element(),
        }
    }

    /// 歌曲行的艺术家名文本：可点导航到艺术家页。
    pub(in crate::app) fn song_artist_link(
        &self,
        id: impl Into<SharedString>,
        song: &Song,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if let Some(part) = &song.bilibili_part {
            return div()
                .min_w_0()
                .truncate()
                .child(t!("Uploader: %{name}", name = part.uploader))
                .into_any_element();
        }
        match artist_for_song(song).filter(|_| self.browsing_owns_song(song)) {
            Some(_) => {
                let song = song.clone();
                self.clickable_meta(id, song.artist.clone(), cx, move |this| {
                    this.open_song_artist(&song);
                })
            }
            None => div()
                .min_w_0()
                .truncate()
                .child(song.artist.clone())
                .into_any_element(),
        }
    }
}
