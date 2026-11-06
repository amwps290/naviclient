//! Pure metadata decoding and complete-playlist validation for Netease responses.

use anyhow::{anyhow, bail, Result};
use rust_i18n::t;
use serde_json::Value;

use super::super::{ImportedPlaylist, ImportedTrack};

pub(super) fn ensure_success_code(body: &Value) -> Result<()> {
    let code = body.get("code").and_then(value_as_u64).unwrap_or_default();
    if code == 200 {
        Ok(())
    } else {
        bail!(t!(
            "Playlist request failed: %{error}",
            error = format!("API {code}")
        ))
    }
}

#[cfg(test)]
fn parse_response(body: &Value) -> Result<ImportedPlaylist> {
    parse_response_with_unavailable(body, 0)
}

pub(super) fn parse_response_with_unavailable(
    body: &Value,
    unavailable_tracks: usize,
) -> Result<ImportedPlaylist> {
    let playlist = body
        .get("result")
        .or_else(|| body.get("playlist"))
        .ok_or_else(|| anyhow!(t!("Invalid playlist response.")))?;
    let name = playlist
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tracks = playlist
        .get("tracks")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!(t!("Playlist is empty or not public.")))?;
    if tracks.is_empty() {
        bail!(t!("Playlist is empty or not public."));
    }
    let declared_count = playlist
        .get("trackCount")
        .and_then(value_as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(tracks.len());
    let expected_count = declared_count.max(tracks.len());
    let imported: Vec<ImportedTrack> = tracks.iter().filter_map(parse_track).collect();
    if imported.is_empty() {
        bail!(t!("Playlist is empty or not public."));
    }
    if imported.len().saturating_add(unavailable_tracks) != expected_count {
        bail!(t!(
            "Playlist response is incomplete (%{received}/%{total} tracks).",
            received = imported.len(),
            total = expected_count
        ));
    }
    Ok(ImportedPlaylist {
        name,
        tracks: imported,
        unavailable_tracks,
    })
}

/// 单曲 → `ImportedTrack`；缺名字/艺人的行整行跳过（无法用于匹配）。
fn parse_track(track: &Value) -> Option<ImportedTrack> {
    let title = track.get("name")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let artists: Vec<String> = track
        .get("artists")
        .or_else(|| track.get("ar"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| {
            let name = artist.get("name")?.as_str()?.trim().to_string();
            (!name.is_empty()).then_some(name)
        })
        .collect();
    if artists.is_empty() {
        return None;
    }
    let album = track
        .get("album")
        .or_else(|| track.get("al"))
        .and_then(|album| album.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    // `duration` 是毫秒；接口偶发返回字符串，防御处理。
    let duration_ms = track
        .get("duration")
        .or_else(|| track.get("dt"))
        .and_then(value_as_u64)
        .and_then(|ms| u32::try_from(ms).ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(0);
    Some(ImportedTrack {
        title,
        artists,
        album,
        duration_ms,
    })
}

pub(super) fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
