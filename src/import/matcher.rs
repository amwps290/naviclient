//! Bounded Subsonic searches, per-track retries, and ordered playlist matching.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::api::OpenSubsonicProvider;
use crate::models::Song;

use super::scoring::{best_match, strip_parenthetical};
use super::{ImportedTrack, MatchedSongs, TrackMatch};

/// 每首查询的候选数量。
const CANDIDATES: u32 = 20;
/// 匹配并发度：不打爆本地 Navidrome，也要对公网服务器客气。
const WORKERS: usize = 4;
const MATCH_ATTEMPTS: usize = 2;

/// 用曲库搜索匹配单首：先“标题 + 主艺人”，没结果或全不过线再退化
/// 到仅标题（艺人 tag 常有繁简/别名差异）。
pub async fn match_track(
    api: &OpenSubsonicProvider,
    track: &ImportedTrack,
) -> Result<Option<Song>> {
    let title = strip_parenthetical(&track.title);
    let primary = track
        .artists
        .first()
        .map(|artist| strip_parenthetical(artist));
    let mut candidates = match primary.as_ref().filter(|artist| !artist.is_empty()) {
        Some(artist) => {
            api.search(&format!("{title} {artist}"), CANDIDATES, 0, 0)
                .await?
                .songs
        }
        None => Vec::new(),
    };
    let mut best = best_match(track, &candidates);
    if best.is_none() && !title.is_empty() {
        candidates = api.search(&title, CANDIDATES, 0, 0).await?.songs;
        best = best_match(track, &candidates);
    }
    Ok(best)
}

async fn match_track_with_retry(
    api: &OpenSubsonicProvider,
    track: &ImportedTrack,
) -> Result<Option<Song>> {
    let mut last_error = None;
    for attempt in 0..MATCH_ATTEMPTS {
        match match_track(api, track).await {
            Ok(song) => return Ok(song),
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < MATCH_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    Err(last_error.expect("at least one matching attempt"))
}

/// 匹配整张歌单。按索引取模分给 WORKERS 个任务并发执行，结果写回与
/// `tracks` 对齐的槽位；每完成一首回调一次当前进度数。
pub async fn match_playlist<F>(
    api: &OpenSubsonicProvider,
    tracks: &[ImportedTrack],
    progress: F,
) -> Result<MatchedSongs>
where
    F: Fn(usize) + Send + Sync + 'static,
{
    let progress = Arc::new(progress);
    let done = Arc::new(AtomicUsize::new(0));
    let mut workers = tokio::task::JoinSet::new();
    for worker in 0..WORKERS {
        let items: Vec<(usize, ImportedTrack)> = tracks
            .iter()
            .enumerate()
            .filter(|(index, _)| index % WORKERS == worker)
            .map(|(index, track)| (index, track.clone()))
            .collect();
        if items.is_empty() {
            continue;
        }
        let api = api.clone();
        let progress = progress.clone();
        let done = done.clone();
        workers.spawn(async move {
            let mut matched = Vec::with_capacity(items.len());
            for (index, track) in items {
                let outcome = match match_track_with_retry(&api, &track).await {
                    Ok(Some(song)) => TrackMatch::Matched(Box::new(song)),
                    Ok(None) => TrackMatch::Unmatched,
                    Err(error) => {
                        log::warn!("failed to match track ‘{}’: {error:#}", track.title);
                        TrackMatch::Failed
                    }
                };
                matched.push((index, outcome));
                let count = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(count);
            }
            matched
        });
    }
    let mut matched = vec![TrackMatch::Unmatched; tracks.len()];
    while let Some(result) = workers.join_next().await {
        let worker = result.map_err(|error| anyhow!("matching worker failed: {error}"))?;
        for (index, outcome) in worker {
            matched[index] = outcome;
        }
    }
    Ok(MatchedSongs { entries: matched })
}
