use rust_i18n::t;
use serde::{Deserialize, Serialize};

/// 专辑列表的排序方式。除 `NewestAdded` 外均直接映射到
/// Subsonic `getAlbumList2` 的服务端排序，保持增量分页可用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumSort {
    #[default]
    NewestAdded,
    Name,
    Artist,
    Year,
}

impl AlbumSort {
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::NewestAdded => t!("Recently added"),
            Self::Name => t!("Name"),
            Self::Artist => t!("Artist"),
            Self::Year => t!("Year"),
        }
    }

    pub const ALL: [Self; 4] = [Self::NewestAdded, Self::Name, Self::Artist, Self::Year];
}

/// 艺术家列表的排序方式。服务端不支持该排序，客户端对完整列表排序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistSort {
    #[default]
    Name,
    NameDesc,
    AlbumCount,
}

impl ArtistSort {
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Name => t!("Name (A–Z)"),
            Self::NameDesc => t!("Name (Z–A)"),
            Self::AlbumCount => t!("Album count"),
        }
    }

    pub const ALL: [Self; 3] = [Self::Name, Self::NameDesc, Self::AlbumCount];
}
