//! QQ 音乐公开歌单拉取。
//!
//! 使用当前网页的 `musicu` 接口，只读取用于曲库匹配的元数据，不请求音源、
//! 不需要登录。`song_num` 取足够大的值避免大型歌单被默认分页截断；响应中的
//! 声明数量仍用于完整性校验。

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use rust_i18n::t;
use serde_json::Value;

use super::{ImportedPlaylist, ImportedTrack};

const MUSICU_API: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const REFERER: &str = "https://y.qq.com/";

pub(super) async fn fetch(http: &reqwest::Client, id: &str) -> Result<ImportedPlaylist> {
    request_musicu(http, id)
        .await
        .and_then(|body| parse_response(&body))
}

async fn request_musicu(http: &reqwest::Client, id: &str) -> Result<Value> {
    let disstid = id
        .parse::<u64>()
        .map_err(|_| anyhow!(t!("Unrecognized playlist link.")))?;
    let payload = serde_json::json!({
        "comm": {"ct": 24, "cv": 0},
        "req": {
            "module": "music.srfDissInfo.aiDissInfo",
            "method": "uniform_get_Dissinfo",
            "param": {
                "disstid": disstid,
                "enc_host_uin": "",
                "tag": 1,
                "userinfo": 1,
                "song_begin": 0,
                "song_num": 10000
            }
        }
    });
    let response = http
        .post(MUSICU_API)
        .json(&payload)
        .header(reqwest::header::REFERER, REFERER)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
    response_json(response).await
}

async fn response_json(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
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

fn parse_response(body: &Value) -> Result<ImportedPlaylist> {
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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_qq_playlist_and_converts_seconds_to_milliseconds() {
        let body = json!({
            "code": 0,
            "req": {
                "code": 0,
                "data": {
                    "code": 0,
                    "subcode": 0,
                    "dirinfo": {"title": "通勤歌单", "songnum": 2},
                    "songlist": [
                        {
                            "name": "爱人错过",
                            "singer": [{"name": "告五人"}],
                            "album": {"name": "我肯定在几百年前就说过爱你"},
                            "interval": 292
                        },
                        {
                            "title": "漠河舞厅",
                            "singer": [{"name": "柳爽"}, {"name": ""}],
                            "album": {"name": "1st.星球"},
                            "interval": "333"
                        }
                    ]
                }
            }
        });
        let playlist = parse_response(&body).expect("valid QQ playlist");
        assert_eq!(playlist.name, "通勤歌单");
        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.tracks[0].artists, vec!["告五人"]);
        assert_eq!(playlist.tracks[0].duration_ms, 292_000);
        assert_eq!(playlist.tracks[1].duration_ms, 333_000);
        assert_eq!(playlist.unavailable_tracks, 0);
    }

    #[test]
    fn accounts_for_unavailable_tracks_and_rejects_malformed_rows() {
        let unavailable = json!({
            "code": 0,
            "req": {"code": 0, "data": {
                "code": 0,
                "subcode": 0,
                "dirinfo": {"title": "测试", "songnum": 2},
                "songlist": [{
                    "name": "晴天",
                    "singer": [{"name": "周杰伦"}],
                    "interval": 269
                }]
            }}
        });
        let playlist = parse_response(&unavailable).expect("one known unavailable track");
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.unavailable_tracks, 1);

        let malformed = json!({
            "code": 0,
            "req": {"code": 0, "data": {
                "code": 0,
                "subcode": 0,
                "dirinfo": {"title": "测试", "songnum": 2},
                "songlist": [
                    {
                        "name": "晴天",
                        "singer": [{"name": "周杰伦"}],
                        "interval": 269
                    },
                    {"name": "缺少歌手"}
                ]
            }}
        });
        let error = parse_response(&malformed).expect_err("malformed row must not be hidden");
        assert!(error.to_string().contains("1/2"));
    }

    #[test]
    fn rejects_private_empty_and_error_responses() {
        assert!(parse_response(&json!({
            "code": 0,
            "req": {"code": 0, "data": {
                "code": 0,
                "subcode": 0,
                "dirinfo": {"title": "空", "songnum": 0},
                "songlist": []
            }}
        }))
        .is_err());
        assert!(parse_response(&json!({"code": 4000})).is_err());
        assert!(parse_response(&json!({
            "code": 0,
            "req": {"code": 0, "data": {"code": 0, "subcode": 4000}}
        }))
        .is_err());
    }
}
