//! 网易云音乐歌单拉取（P1 数据源）。
//!
//! 走公开 v6 详情接口取得可信的 `trackCount` / `trackIds`，再分批调用
//! `api/song/detail` 补齐歌曲元数据。无需登录或 weapi 加密，请求带浏览器
//! UA 与 Referer。已下架或对未登录请求不可见的歌曲会计数后跳过；除此之外
//! 仍用 `trackCount` 校验真正的截断或解析损坏。

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use rust_i18n::t;
use serde_json::Value;

use super::{ImportedPlaylist, ImportedTrack};

const PLAYLIST_API: &str = "https://music.163.com/api/v6/playlist/detail";
const SONG_DETAIL_API: &str = "https://music.163.com/api/song/detail";
const REFERER: &str = "https://music.163.com/";
const SONG_BATCH_SIZE: usize = 500;

pub(super) async fn fetch(http: &reqwest::Client, id: &str) -> Result<ImportedPlaylist> {
    // v6 的 trackCount / trackIds 才是权威数量。旧接口对部分大型歌单会
    // 静默截成 10 首并伪造 trackCount，因此不能作为可信兜底。
    let mut body = request_json(http, PLAYLIST_API, id).await?;
    ensure_success_code(&body)?;
    let unavailable_tracks = hydrate_tracks(http, &mut body).await?;
    parse_response_with_unavailable(&body, unavailable_tracks)
}

async fn request_json(http: &reqwest::Client, endpoint: &str, id: &str) -> Result<Value> {
    let response = http
        .get(endpoint)
        .query(&[("id", id), ("n", "100000"), ("s", "0")])
        .header(reqwest::header::REFERER, REFERER)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
    let status = response.status();
    if !status.is_success() {
        // 风控（460 cheating 等）与歌单不存在都会走这里。
        bail!(t!(
            "Playlist request failed: %{error}",
            error = format!("HTTP {status}")
        ));
    }
    response
        .json()
        .await
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))
}

fn ensure_success_code(body: &Value) -> Result<()> {
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

async fn hydrate_tracks(http: &reqwest::Client, body: &mut Value) -> Result<usize> {
    let playlist = body
        .get_mut("playlist")
        .ok_or_else(|| anyhow!(t!("Invalid playlist response.")))?;
    let track_ids: Vec<String> = playlist
        .get("trackIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| track.get("id").and_then(value_as_u64))
        .map(|id| id.to_string())
        .collect();
    let embedded_count = playlist
        .get("tracks")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if track_ids.is_empty() || embedded_count >= track_ids.len() {
        return Ok(0);
    }

    let mut songs_by_id = HashMap::with_capacity(track_ids.len());
    for batch in track_ids.chunks(SONG_BATCH_SIZE) {
        for song in fetch_song_details(http, batch).await? {
            if let Some(id) = song.get("id").and_then(value_as_u64) {
                songs_by_id.insert(id.to_string(), song);
            }
        }
    }
    // 某些公开详情请求偶发漏行；把首次缺失项再请求一次，第二次仍没有的
    // 才视为已下架、删除或对未登录请求不可见。
    let first_missing: Vec<String> = track_ids
        .iter()
        .filter(|id| !songs_by_id.contains_key(*id))
        .cloned()
        .collect();
    for batch in first_missing.chunks(SONG_BATCH_SIZE) {
        for song in fetch_song_details(http, batch).await? {
            if let Some(id) = song.get("id").and_then(value_as_u64) {
                songs_by_id.insert(id.to_string(), song);
            }
        }
    }
    let tracks = track_ids
        .iter()
        .filter_map(|id| songs_by_id.remove(id))
        .collect::<Vec<_>>();
    let unavailable = track_ids.len().saturating_sub(tracks.len());
    playlist["tracks"] = Value::Array(tracks);
    Ok(unavailable)
}

async fn fetch_song_details(http: &reqwest::Client, ids: &[String]) -> Result<Vec<Value>> {
    let ids = format!("[{}]", ids.join(","));
    let response = http
        .get(SONG_DETAIL_API)
        .query(&[("ids", ids.as_str())])
        .header(reqwest::header::REFERER, REFERER)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
    let status = response.status();
    if !status.is_success() {
        bail!(t!(
            "Playlist request failed: %{error}",
            error = format!("HTTP {status}")
        ));
    }
    let detail: Value = response
        .json()
        .await
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
    ensure_success_code(&detail)?;
    Ok(detail
        .get("songs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[cfg(test)]
fn parse_response(body: &Value) -> Result<ImportedPlaylist> {
    parse_response_with_unavailable(body, 0)
}

fn parse_response_with_unavailable(
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

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_track_metadata_defensively() {
        let track = json!({
            "name": "晴天",
            "artists": [{"name": "周杰伦"}, {"name": ""}],
            "album": {"name": "叶惠美"},
            "duration": 269_000,
        });
        let parsed = parse_track(&track).expect("valid track");
        assert_eq!(parsed.title, "晴天");
        assert_eq!(parsed.artists, vec!["周杰伦"]);
        assert_eq!(parsed.album, "叶惠美");
        assert_eq!(parsed.duration_ms, 269_000);

        let string_duration = json!({
            "name": "晴天",
            "artists": [{"name": "周杰伦"}],
            "duration": "269000",
        });
        assert_eq!(
            parse_track(&string_duration)
                .expect("string duration")
                .duration_ms,
            269_000
        );

        let modern = json!({
            "name": "夜曲",
            "ar": [{"name": "周杰伦"}],
            "al": {"name": "十一月的萧邦"},
            "dt": 226_000,
        });
        let parsed = parse_track(&modern).expect("modern track fields");
        assert_eq!(parsed.artists, vec!["周杰伦"]);
        assert_eq!(parsed.album, "十一月的萧邦");
        assert_eq!(parsed.duration_ms, 226_000);
    }

    #[test]
    fn rejects_truncated_or_partially_unparseable_playlists() {
        let track = json!({
            "name": "晴天",
            "artists": [{"name": "周杰伦"}],
            "duration": 269_000,
        });
        let truncated = json!({
            "result": {"name": "测试", "trackCount": 2, "tracks": [track.clone()]}
        });
        let error = parse_response(&truncated).expect_err("truncated response");
        assert!(error.to_string().contains("1/2"));

        let unavailable = json!({
            "playlist": {
                "name": "测试",
                "trackCount": 2,
                "tracks": [track.clone()]
            }
        });
        let parsed = parse_response_with_unavailable(&unavailable, 1)
            .expect("known unavailable track should not reject the playlist");
        assert_eq!(parsed.tracks.len(), 1);
        assert_eq!(parsed.unavailable_tracks, 1);

        let malformed = json!({
            "result": {
                "name": "测试",
                "trackCount": 2,
                "tracks": [track, {"name": "缺少艺人"}]
            }
        });
        let error = parse_response(&malformed).expect_err("partially malformed response");
        assert!(error.to_string().contains("1/2"));
    }

    #[test]
    fn skips_tracks_without_title_or_artists() {
        assert!(parse_track(&json!({ "name": "" })).is_none());
        assert!(parse_track(&json!({
            "name": "无艺人",
            "artists": [{"name": "  "}],
        }))
        .is_none());
        assert!(parse_track(&json!({})).is_none());
    }
}
