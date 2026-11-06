//! Pure imported-track normalization, candidate scores, and automatic acceptance.

use crate::models::Song;

use super::ImportedTrack;

/// 自动接受阈值：总分 ≥ 0.75，标题分 ≥ 0.8，且艺人分 ≥ 0.8。
/// 艺人是硬门槛，避免“同名 + 同时长”的翻唱或重录被自动导入。
const ACCEPT_TOTAL: f32 = 0.75;
const ACCEPT_TITLE: f32 = 0.8;

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
    let names = candidate_artist_names(candidate);
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

#[cfg(test)]
mod tests;
