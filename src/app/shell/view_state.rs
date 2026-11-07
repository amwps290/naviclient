//! Stable navigation views and queue drawer tabs.

use rust_i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum View {
    Home,
    Favorites,
    Artists,
    Albums,
    Genres,
    GenreDetail,
    Playlists,
    Search,
    ArtistDetail,
    AlbumDetail,
    PlaylistDetail,
    NowPlaying,
}

impl View {
    pub(in crate::app) fn detail_parent(self) -> Option<Self> {
        match self {
            Self::ArtistDetail => Some(Self::Artists),
            Self::AlbumDetail => Some(Self::Albums),
            Self::GenreDetail => Some(Self::Genres),
            Self::PlaylistDetail => Some(Self::Playlists),
            _ => None,
        }
    }

    /// A detail page keeps its own library section selected in the sidebar.
    pub(in crate::app) fn is_selected_for(self, current: Self) -> bool {
        self == current || current.detail_parent() == Some(self)
    }

    /// These pages open with a full-bleed header and pad their own content.
    pub(in crate::app) fn has_detail_hero(self) -> bool {
        matches!(
            self,
            Self::ArtistDetail | Self::AlbumDetail | Self::PlaylistDetail
        )
    }

    /// 侧边栏入口的稳定元素 ID 前缀。与显示文案解耦，切换界面语言时
    /// 元素 ID 不变（否则 GPUI 会把整行当作新元素、丢掉悬停/焦点状态）。
    pub(in crate::app) fn nav_id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Favorites => "favorites",
            Self::Artists | Self::ArtistDetail => "artists",
            Self::Albums | Self::AlbumDetail => "albums",
            Self::Genres | Self::GenreDetail => "genres",
            Self::Playlists | Self::PlaylistDetail => "playlists",
            Self::Search => "search",
            Self::NowPlaying => "now-playing",
        }
    }
}

#[cfg(test)]
mod tests;

/// 队列抽屉的两个稳定视图；历史按实际播放事件排序，不由队列索引推导。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum QueueDrawerTab {
    #[default]
    UpNext,
    History,
}

impl QueueDrawerTab {
    pub(in crate::app) fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::UpNext => t!("Up next"),
            Self::History => t!("History"),
        }
    }

    pub(in crate::app) const ALL: [Self; 2] = [Self::UpNext, Self::History];
}
