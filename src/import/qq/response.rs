//! QQ metadata decoding, layered status codes and complete-playlist validation.

use anyhow::{anyhow, bail, Result};
use rust_i18n::t;
use serde_json::Value;

use super::super::{ImportedPlaylist, ImportedTrack};

pub(super) fn parse_response(body: &Value) -> Result<ImportedPlaylist> {
    ensure_zero_code(body, "code")?;
    let request = body
        .get("req")
        .ok_or_else(|| anyhow!(t!("Invalid playlist response.")))?;
    ensure_zero_code(request, "code")?;
    let data = request
        .get("data")
        .ok_or_else(|| anyhow!(t!("Invalid playlist response.")))?;
    ensure_zero_code(data, "code")?;
    ensure_zero_code(data, "subcode")?;
    let dirinfo = data
        .get("dirinfo")
        .ok_or_else(|| anyhow!(t!("Playlist is empty or not public.")))?;
    let name = dirinfo
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let declared_count = dirinfo
        .get("songnum")
        .and_then(value_as_u64)
        .and_then(|count| usize::try_from(count).ok());
    let tracks = data
        .get("songlist")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!(t!("Playlist is empty or not public.")))?;
    if tracks.is_empty() {
        bail!(t!("Playlist is empty or not public."));
    }
    let declared_count = declared_count.unwrap_or(tracks.len());
    let expected_count = declared_count.max(tracks.len());
    // QQ 对已下架或区域不可用曲目可能只保留在声明数量中，不再返回元数据。
    let unavailable_tracks = expected_count.saturating_sub(tracks.len());
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

fn ensure_zero_code(body: &Value, field: &str) -> Result<()> {
    let Some(code) = body.get(field).and_then(value_as_u64) else {
        return Ok(());
    };
    if code == 0 {
        Ok(())
    } else {
        bail!(t!(
            "Playlist request failed: %{error}",
            error = format!("API {field} {code}")
        ))
    }
}

fn parse_track(track: &Value) -> Option<ImportedTrack> {
    let title = track
        .get("songname")
        .or_else(|| track.get("title"))
        .or_else(|| track.get("name"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    let artists: Vec<String> = track
        .get("singer")
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
        .get("albumname")
        .or_else(|| track.get("album").and_then(|album| album.get("name")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let duration_ms = track
        .get("interval")
        .and_then(value_as_u64)
        .and_then(|seconds| seconds.checked_mul(1000))
        .and_then(|milliseconds| u32::try_from(milliseconds).ok())
        .filter(|milliseconds| *milliseconds > 0)
        .unwrap_or(0);
    Some(ImportedTrack {
        title,
        artists,
        album,
        duration_ms,
    })
}

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
