use super::source::{MediaKey, SourceId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FavoriteKind {
    Artist,
    Album,
    Song,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FavoriteKey {
    pub kind: FavoriteKind,
    pub id: String,
}

impl FavoriteKey {
    pub fn new(kind: FavoriteKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album_count: Option<i32>,
    #[serde(default)]
    pub cover_art: Option<String>,
}

/// Subsonic `getGenres` 返回的流派条目。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub value: String,
    #[serde(default)]
    pub song_count: Option<i64>,
    #[serde(default)]
    pub album_count: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    /// Filled by the source adapter; remote IDs retain their protocol meaning.
    /// Legacy payloads are bound to their owner when restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceId>,
    pub id: String,
    pub title: String,
    /// Bilibili video/collection context; multi-part tracks use the part title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bilibili_part: Option<BilibiliPart>,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_id: Option<String>,
    /// Subsonic song genre; used as an explainable fallback when a server
    /// cannot return similarity-based radio candidates.
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub track: Option<i32>,
    /// 碟号（Subsonic `discNumber`）；多碟专辑按（碟号，曲目号）排序。
    #[serde(default)]
    pub disc: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub bit_rate: Option<i32>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
    /// ReplayGain 曲目增益（dB）；Navidrome/Subsonic 可选字段，缺失时安全回退。
    #[serde(
        default,
        alias = "replayGainTrack",
        deserialize_with = "deserialize_optional_f32"
    )]
    pub replay_gain_track_gain: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_optional_f32")]
    pub replay_gain_track_peak: Option<f32>,
    #[serde(
        default,
        alias = "replayGainAlbum",
        deserialize_with = "deserialize_optional_f32"
    )]
    pub replay_gain_album_gain: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_optional_f32")]
    pub replay_gain_album_peak: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliPart {
    pub bvid: String,
    pub cid: u64,
    pub index: u32,
    pub count: u32,
    pub title: String,
    pub uploader: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub video_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection_title: String,
}

impl Song {
    /// Also upgrades saved tracks whose primary title was the video title.
    pub(crate) fn normalize_bilibili_title(&mut self) {
        let Some(part) = &mut self.bilibili_part else {
            return;
        };
        if part.video_title.is_empty() {
            part.video_title.clone_from(&self.title);
        }
        if part.count > 1 && !part.title.trim().is_empty() {
            self.title.clone_from(&part.title);
        }
    }

    /// Video or collection context, with numbering only within a multi-part BV.
    pub(crate) fn bilibili_subtitle(&self) -> Option<String> {
        let part = self.bilibili_part.as_ref()?;
        let context = if part.count > 1
            && !part.video_title.trim().is_empty()
            && part.video_title != self.title
        {
            Some(part.video_title.as_str())
        } else {
            Some(part.collection_title.as_str())
                .filter(|title| !title.trim().is_empty() && *title != self.title)
        };
        if part.count > 1 {
            let position = format!("P{}/{}", part.index, part.count);
            Some(match context {
                Some(title) => format!("{title} · {position}"),
                None => position,
            })
        } else {
            context.map(str::to_string)
        }
    }

    pub fn media_key(&self) -> Option<MediaKey> {
        self.source
            .clone()
            .map(|source| MediaKey::new(source, &self.id))
    }

    pub fn belongs_to(&self, source: &SourceId) -> bool {
        self.source.as_ref().is_none_or(|owner| owner == source)
    }
}

#[cfg(test)]
mod tests;

/// 容错解析可选的 f32 字段：接受数字或字符串，其他情况返回 None。
fn deserialize_optional_f32<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64().map(|n| n as f32),
        serde_json::Value::String(text) => text.parse::<f32>().ok(),
        _ => None,
    }))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub song_count: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub public: Option<bool>,
    #[serde(default)]
    pub cover_art: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, Default)]
pub struct Favorites {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub display_artist: Option<String>,
    pub display_title: Option<String>,
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    pub fn is_synced(&self) -> bool {
        self.lines.iter().any(|line| line.start_ms.is_some())
    }

    pub fn active_line_index(&self, position: std::time::Duration) -> Option<usize> {
        let position_ms = u64::try_from(position.as_millis()).unwrap_or(u64::MAX);
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.start_ms.is_some_and(|start| start <= position_ms))
            .map(|(index, _)| index)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: Option<u64>,
    pub text: String,
    /// 其他语言变体中按时间戳就近匹配到的翻译行；无可用变体时为 None。
    pub translation: Option<String>,
}
