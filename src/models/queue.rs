use serde::{Deserialize, Serialize};

use super::{PlayModes, Song};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOrigin {
    #[default]
    Manual,
    Playlist,
    DailyMix,
    SongRadio,
    ArtistRadio,
    AutoContinue,
}

pub fn normalize_queue_origins(
    mut origins: Vec<QueueOrigin>,
    queue_len: usize,
) -> Vec<QueueOrigin> {
    origins.resize(queue_len, QueueOrigin::Manual);
    origins.truncate(queue_len);
    origins
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlaybackSession {
    /// 归属服务器（切换服务器时不恢复其他服务器的会话）。
    pub server_url: String,
    /// 播放队列快照。
    pub queue: Vec<Song>,
    /// 与 queue 按索引对齐的来源；旧会话缺失时迁移为 Manual。
    #[serde(default)]
    pub queue_origins: Vec<QueueOrigin>,
    /// 当前播放的队列索引。
    pub queue_index: Option<usize>,
    /// 播放位置（秒）。
    pub position_secs: f64,
    /// 播放模式（随机开关 + 循环模式）。
    pub playback_modes: PlayModes,
    /// 随机播放历史（播放过的索引）。
    pub shuffle_played: Vec<usize>,
    /// 随机播放回退暂存。
    pub shuffle_forward: Vec<usize>,
}
