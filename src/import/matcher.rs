//! 匹配引擎：把导入的曲目元数据匹配到 Navidrome 曲库。
//!
//! 逐首调用 Subsonic `search3`（复用 `Api::search`），对返回候选打分后
//! 取最优；只有“标题几乎一致”且总分过线才自动接受，其余留给导入审查
//! 列表（P2 再加人工换选）。
//!
//! 归一化目标：全角/半角、大小写、标点与空白差异，以及结尾的
//! `(Live)`、`（伴奏）` 之类的括号后缀——这些都是网易云/QQ 与本地
//! 音乐库 tag 之间最常见的漂移。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::api::Api;
use crate::models::Song;

use super::{ImportedTrack, MatchedSongs};

/// 自动接受阈值：总分 ≥ 0.75，标题分 ≥ 0.8，且艺人分 ≥ 0.8。
/// 艺人是硬门槛，避免“同名 + 同时长”的翻唱或重录被自动导入。
const ACCEPT_TOTAL: f32 = 0.75;
const ACCEPT_TITLE: f32 = 0.8;
/// 每首查询的候选数量。
const CANDIDATES: u32 = 20;
/// 匹配并发度：不打爆本地 Navidrome，也要对公网服务器客气。
const WORKERS: usize = 4;
const MATCH_ATTEMPTS: usize = 2;

/// 归一化：全角转半角、小写、去空白与标点，只留字母数字
/// （CJK 字符属于字母数字，会保留）。
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        let character = match character as u32 {
            0x3000 => ' ',
            code @ 0xFF01..=0xFF5E => char::from_u32(code - 0xFEE0).unwrap_or(character),
            _ => character,
        };
        for folded in character.to_lowercase() {
            if folded.is_alphanumeric() {
                out.push(folded);
            }
        }
    }
    out
}

const BRACKET_PAIRS: [(char, char); 7] = [
    ('(', ')'),
    ('（', '）'),
    ('【', '】'),
    ('[', ']'),
    ('「', '」'),
    ('『', '』'),
    ('〈', '〉'),
];

/// 去掉标题结尾的常见版本括号后缀（`(Live)`、`【伴奏】`…）。只剥最后
/// 一组，且不会删除普通标题中有语义的括号内容；括号不平衡时原样返回。
pub fn strip_parenthetical(text: &str) -> String {
    let trimmed = text.trim();
    let Some((open, close)) = BRACKET_PAIRS
        .iter()
        .copied()
        .find(|(_, close)| trimmed.ends_with(*close))
    else {
        return trimmed.to_string();
    };
    let mut depth = 0usize;
    let mut cut = None;
    for (index, character) in trimmed.char_indices().rev() {
        if character == close {
            depth += 1;
        } else if character == open {
            match depth.checked_sub(1) {
                Some(remaining) => {
                    depth = remaining;
                    if depth == 0 {
                        cut = Some(index);
                        break;
                    }
                }
                None => break,
            }
        }
    }
    match cut {
        Some(index)
            if is_version_suffix(
                &trimmed[index + open.len_utf8()..trimmed.len() - close.len_utf8()],
            ) =>
        {
            trimmed[..index].trim_end().to_string()
        }
        _ => trimmed.to_string(),
    }
}

fn is_version_suffix(suffix: &str) -> bool {
    let suffix = normalize(suffix);
    [
        "live",
        "现场",
        "伴奏",
        "伴奏版",
        "instrumental",
        "karaoke",
        "demo",
        "remix",
        "remaster",
        "remastered",
        "acoustic",
        "radioedit",
    ]
    .iter()
    .any(|marker| suffix == *marker || suffix.starts_with(marker))
}

/// 字符级编辑距离（标题归一化后一般很短，朴素 DP 足够）。
fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    for (offset, a_char) in a.chars().enumerate() {
        let mut current = Vec::with_capacity(b_chars.len() + 1);
        current.push(offset + 1);
        for (index, b_char) in b_chars.iter().enumerate() {
            let cost = usize::from(a_char != *b_char);
            let best = (previous[index] + cost)
                .min(previous[index + 1] + 1)
                .min(current[index] + 1);
            current.push(best);
        }
        previous = current;
    }
    previous[b_chars.len()]
}

/// 标题相似度：归一化相等 1.0，互含 0.9，编辑距离最高 0.7。
pub fn title_score(imported: &str, candidate: &str) -> f32 {
    let a = normalize(&strip_parenthetical(imported));
    let b = normalize(&strip_parenthetical(candidate));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.contains(&b) || b.contains(&a) {
        return 0.9;
    }
    let len = a.chars().count().max(b.chars().count());
    let distance = levenshtein(&a, &b);
    ((1.0 - distance as f32 / len as f32) * 0.7).max(0.0)
}

/// 艺人相似度：任一导入艺人（含 feat 嘉宾）在候选艺人串里命中即计分。
/// 候选串先按分隔符拆开再逐一比对。
fn candidate_artist_names(candidate: &str) -> Vec<String> {
    candidate
        .split(['/', '、', ',', '&', ';', '×', '·', '(', ')', '（', '）'])
        .map(normalize)
        .filter(|name| !name.is_empty())
        .collect()
}

fn name_score(imported: &str, candidates: &[String]) -> f32 {
    let imported = normalize(imported);
    if imported.is_empty() {
        return 0.0;
    }
    if candidates.contains(&imported) {
        return 1.0;
    }
    let imported_len = imported.chars().count();
    if imported_len >= 3
        && candidates.iter().any(|name| {
            name.chars().count() >= 3
                && (name.contains(imported.as_str()) || imported.contains(name.as_str()))
        })
    {
        0.8
    } else {
        0.0
    }
}

pub fn artist_score(imported: &[String], candidate: &str) -> f32 {
    let names = candidate_artist_names(candidate);
    imported
        .iter()
        .map(|imported| name_score(imported, &names))
        .fold(0.0, f32::max)
}

fn primary_artist_score(imported: &[String], candidate: &str) -> f32 {
    let names: Vec<String> = candidate
        .split(['/', '、', ',', '&', ';', '×', '·', '(', ')', '（', '）'])
        .map(normalize)
        .filter(|name: &String| !name.is_empty())
        .collect();
    imported
        .first()
        .map_or(0.0, |primary| name_score(primary, &names))
}

/// 时长相似度：±2.5s 满分（库内时长常有取整），差异大反而降分，
/// 用来压制“同名翻唱/串烧”误匹配。
pub fn duration_score(imported_ms: u32, candidate_secs: Option<i32>) -> f32 {
    if imported_ms == 0 {
        return 0.5;
    }
    let Some(secs) = candidate_secs.filter(|secs| *secs > 0) else {
        return 0.5;
    };
    let diff = (imported_ms as i64)
        .saturating_sub((secs as i64) * 1000)
        .abs();
    if diff <= 2_500 {
        1.0
    } else if diff <= 6_000 {
        0.8
    } else if diff <= 15_000 {
        0.3
    } else {
        0.05
    }
}

/// 单首综合分：标题为主，艺人次之，时长校验兜底。
pub fn track_score(track: &ImportedTrack, song: &Song) -> f32 {
    let title = title_score(&track.title, &song.title);
    let artist = artist_score(&track.artists, &song.artist);
    let duration = duration_score(track.duration_ms, song.duration);
    0.6 * title + 0.2 * artist + 0.2 * duration
}

/// 候选里挑最优且过线的；否则 None（P1 直接标记为未匹配）。
pub fn best_match(track: &ImportedTrack, candidates: &[Song]) -> Option<Song> {
    let scored = |song: &Song| {
        let title = title_score(&track.title, &song.title);
        let artist = artist_score(&track.artists, &song.artist);
        let primary_artist = primary_artist_score(&track.artists, &song.artist);
        let total = track_score(track, song);
        (title, artist, primary_artist, total)
    };
    candidates
        .iter()
        .map(|song| (scored(song), song))
        .filter(|((title, _, primary_artist, total), _)| {
            *title >= ACCEPT_TITLE && *primary_artist >= 0.8 && *total >= ACCEPT_TOTAL
        })
        .max_by(|a, b| a.0 .3.total_cmp(&b.0 .3))
        .map(|(_, song)| song.clone())
}

/// 用曲库搜索匹配单首：先“标题 + 主艺人”，没结果或全不过线再退化
/// 到仅标题（艺人 tag 常有繁简/别名差异）。
pub async fn match_track(api: &Api, track: &ImportedTrack) -> Result<Option<Song>> {
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

async fn match_track_with_retry(api: &Api, track: &ImportedTrack) -> Result<Option<Song>> {
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
    api: &Api,
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
                let (song, failed) = match match_track_with_retry(&api, &track).await {
                    Ok(song) => (song, false),
                    Err(error) => {
                        log::warn!("failed to match track ‘{}’: {error:#}", track.title);
                        (None, true)
                    }
                };
                matched.push((index, song, failed));
                let count = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(count);
            }
            matched
        });
    }
    let mut matched = vec![None::<Song>; tracks.len()];
    let mut failures = vec![false; tracks.len()];
    while let Some(result) = workers.join_next().await {
        let worker = result.map_err(|error| anyhow!("matching worker failed: {error}"))?;
        for (index, song, failed) in worker {
            matched[index] = song;
            failures[index] = failed;
        }
    }
    Ok(MatchedSongs {
        songs: matched,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::super::ImportedTrack;
    use super::*;

    fn song(title: &str, artist: &str, secs: i32) -> Song {
        Song {
            title: title.to_string(),
            artist: artist.to_string(),
            duration: Some(secs),
            ..Song::default()
        }
    }

    fn track(title: &str, artists: &[&str], ms: u32) -> ImportedTrack {
        ImportedTrack {
            title: title.to_string(),
            artists: artists.iter().map(|artist| artist.to_string()).collect(),
            album: String::new(),
            duration_ms: ms,
        }
    }

    #[test]
    fn normalize_folds_width_case_and_punctuation() {
        assert_eq!(normalize(" Ｏｎｅ Ｄａｙ "), "oneday");
        assert_eq!(normalize("晴天（Live）"), "晴天live");
        assert_eq!(normalize("A.B.C"), "abc");
    }

    #[test]
    fn strip_parenthetical_removes_trailing_group_only() {
        assert_eq!(strip_parenthetical("晴天 (Live)"), "晴天");
        assert_eq!(strip_parenthetical("夜曲【伴奏】"), "夜曲");
        assert_eq!(strip_parenthetical("Song (A) (B)"), "Song (A) (B)");
        assert_eq!(strip_parenthetical("Intro (The End)"), "Intro (The End)");
        assert_eq!(strip_parenthetical("无括号"), "无括号");
        // 不平衡：结尾闭括号没有对应开括号，保持原样。
        assert_eq!(strip_parenthetical("奇怪）"), "奇怪）");
    }

    #[test]
    fn title_score_covers_equality_containment_and_fuzzy() {
        assert_eq!(title_score("晴天", "晴天"), 1.0);
        // 结尾括号后缀剥掉后全等。
        assert_eq!(title_score("晴天(Live)", "晴天"), 1.0);
        // 互含但主体不等。
        assert_eq!(title_score("晴天", "晴天的回忆"), 0.9);
        // 纯编辑距离封顶 0.7，不越过接受线。
        let fuzzy = title_score("晴天", "睛天");
        assert!(fuzzy > 0.0 && fuzzy <= 0.7, "fuzzy score was {fuzzy}");
        assert_eq!(title_score("晴天", "晚安"), 0.0);
    }

    #[test]
    fn artist_score_splits_candidates_and_matches_any_imported() {
        let imported = vec!["周杰伦".to_string(), "杨瑞代".to_string()];
        assert_eq!(artist_score(&imported, "周杰伦"), 1.0);
        assert_eq!(artist_score(&imported, "周杰伦 / 杨瑞代"), 1.0);
        assert_eq!(artist_score(&imported, "Someone"), 0.0);
        assert_eq!(artist_score(&[], "周杰伦"), 0.0);
        assert_eq!(artist_score(&["A".to_string()], "ABBA"), 0.0);
    }

    #[test]
    fn duration_score_rewards_close_lengths() {
        assert_eq!(duration_score(200_000, Some(200)), 1.0);
        assert_eq!(duration_score(200_000, Some(202)), 1.0);
        assert!(duration_score(200_000, Some(230)) < 0.5);
        assert_eq!(duration_score(200_000, None), 0.5);
        assert_eq!(duration_score(0, Some(200)), 0.5);
    }

    #[test]
    fn best_match_accepts_strong_hits_and_rejects_impostors() {
        let imported = track("晴天", &["周杰伦"], 269_000);
        let library = vec![
            song("晴天", "周杰伦", 269),
            song("晴天", "张三", 269), // 同名同时长，但艺人不符，不能过线
            song("晴天线", "周杰伦", 269), // 标题不过线
            song("晚安", "周杰伦", 269), // 完全不同歌
        ];
        let matched = best_match(&imported, &library).expect("should match");
        assert_eq!(matched.title, "晴天");
        assert_eq!(matched.artist, "周杰伦");

        // 同名翻唱即使时长完全相同，艺人硬门槛也会拒绝。
        let wrong_artist = vec![song("晴天", "路人甲", 269)];
        assert!(best_match(&imported, &wrong_artist).is_none());

        // 客串艺人命中不能代替主艺人，否则会自动选中错误版本。
        let collaboration = track("晴天", &["周杰伦", "杨瑞代"], 269_000);
        let guest_only = vec![song("晴天", "杨瑞代", 269)];
        assert!(best_match(&collaboration, &guest_only).is_none());

        // 同名翻唱 + 时长相差 40 秒同样拒绝。
        let impostors = vec![song("晴天", "路人甲", 305)];
        assert!(best_match(&imported, &impostors).is_none());
    }

    #[test]
    fn empty_candidates_match_nothing() {
        let imported = track("晴天", &["周杰伦"], 269_000);
        assert!(best_match(&imported, &[]).is_none());
    }
}
