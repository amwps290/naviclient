//! Pure decoding, variant selection and translation pairing for server lyrics.

use serde_json::Value;

use crate::models::{LyricLine, Lyrics};

/// 解析单个 structuredLyrics 变体为歌词行（不含翻译）。
fn parse_lyric_lines(variant: &Value) -> Option<Vec<LyricLine>> {
    let offset = variant
        .get("offset")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let raw_lines = variant.get("line")?.as_array()?;
    let mut lines = Vec::with_capacity(raw_lines.len());
    for line in raw_lines {
        let start_ms = line.get("start").and_then(Value::as_i64).map(|start| {
            let adjusted = i128::from(start) + i128::from(offset);
            adjusted.clamp(0, i128::from(u64::MAX)) as u64
        });
        let text = line
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        // 空白行在不同歌词源中可能代表排版、间奏或清屏，含义并不一致。
        // UI 不为它们创建列表项，保证任意歌曲的可见行间距一致。
        if text.is_empty() {
            continue;
        }

        lines.push(LyricLine {
            start_ms,
            text,
            translation: None,
        });
    }
    Some(lines)
}

/// 翻译行与主行配对的时间容差（毫秒）。
const TRANSLATION_MATCH_TOLERANCE_MS: u64 = 2_500;

/// 将翻译变体逐行配到主歌词上（原地写入 translation 字段）：
/// 双方都同步时按时间戳就近匹配（±2.5 秒），都无时间戳且行数
/// 相同则按行号对齐；空行/与原文相同的文本不写入。
fn pair_translations(primary: &mut [LyricLine], candidate: &[LyricLine]) {
    let primary_synced = primary.iter().any(|line| line.start_ms.is_some());
    let candidate_synced = candidate.iter().any(|line| line.start_ms.is_some());
    if primary_synced && candidate_synced {
        for line in primary.iter_mut() {
            if line.text.is_empty() {
                continue;
            }
            let Some(start) = line.start_ms else {
                continue;
            };
            let best = candidate
                .iter()
                .filter_map(|other| {
                    let other_start = other.start_ms?;
                    let distance = start.abs_diff(other_start);
                    (distance <= TRANSLATION_MATCH_TOLERANCE_MS).then_some((distance, &other.text))
                })
                .min_by_key(|(distance, _)| *distance);
            if let Some((_, text)) = best {
                if !text.is_empty() && text != &line.text {
                    line.translation = Some(text.clone());
                }
            }
        }
    } else if !primary_synced && !candidate_synced && primary.len() == candidate.len() {
        for (line, other) in primary.iter_mut().zip(candidate) {
            if !other.text.is_empty() && other.text != line.text {
                line.translation = Some(other.text.clone());
            }
        }
    }
}

/// 变体的语言标签；缺失时视为空字符串（不同变体仍可区分）。
fn variant_language(variant: &Value) -> &str {
    variant
        .get("lang")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn non_empty_line_count(variant: &Value) -> usize {
    variant
        .get("line")
        .and_then(Value::as_array)
        .map_or(0, |lines| {
            lines
                .iter()
                .filter(|line| {
                    line.get("value")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
                })
                .count()
        })
}

/// 变体排序键：同步优先、有效歌词行多者优先。空行不算内容，避免带有
/// 大量间奏标记的低质量版本仅靠原始数组更长而胜出。
fn variant_rank(variant: &Value) -> (bool, usize) {
    (
        variant
            .get("synced")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        non_empty_line_count(variant),
    )
}

/// 同分时靠前者胜出：服务器通常把原文排在翻译之前，平局让原文当主行。
fn pick_variant<'a>(candidates: &[&'a Value]) -> Option<&'a Value> {
    candidates
        .iter()
        .enumerate()
        .max_by(|(index_a, a), (index_b, b)| {
            variant_rank(a)
                .cmp(&variant_rank(b))
                // max_by 取“较大者”，传 b 的索引反向比较即可让小索引获胜。
                .then(index_b.cmp(index_a))
        })
        .map(|(_, variant)| *variant)
}

pub(super) fn parse_structured_lyrics(body: &Value) -> Option<Lyrics> {
    let variants = body
        .get("lyricsList")?
        .get("structuredLyrics")?
        .as_array()?;
    let usable = variants
        .iter()
        .filter(|lyrics| non_empty_line_count(lyrics) > 0)
        .collect::<Vec<_>>();
    let selected = pick_variant(&usable)?;
    // 翻译候选：不同语言且行非空；同分时同样优先靠前者。
    let translation_candidates: Vec<&Value> = usable
        .into_iter()
        .filter(|variant| variant_language(variant) != variant_language(selected))
        .collect();
    let translation_variant = pick_variant(&translation_candidates);
    let mut lines = parse_lyric_lines(selected)?;
    if let Some(candidate) = translation_variant {
        if let Some(candidate_lines) = parse_lyric_lines(candidate) {
            pair_translations(&mut lines, &candidate_lines);
        }
    }

    Some(Lyrics {
        display_artist: selected
            .get("displayArtist")
            .and_then(Value::as_str)
            .map(str::to_string),
        display_title: selected
            .get("displayTitle")
            .and_then(Value::as_str)
            .map(str::to_string),
        lines,
    })
}

pub(super) fn parse_plain_lyrics(body: &Value) -> Option<Lyrics> {
    let lyrics = body.get("lyrics")?;
    let value = lyrics.get("value").and_then(Value::as_str)?.trim();
    if value.is_empty() {
        return None;
    }

    Some(Lyrics {
        display_artist: lyrics
            .get("artist")
            .and_then(Value::as_str)
            .map(str::to_string),
        display_title: lyrics
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        lines: value
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| LyricLine {
                start_ms: None,
                text: line.to_string(),
                translation: None,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests;
