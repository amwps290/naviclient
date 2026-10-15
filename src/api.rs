use std::fmt::Write;

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};
use rand::Rng;
use serde_json::Value;
use url::Url;

use crate::playlist::{can_edit, song_ids, PlaylistDetails, PlaylistEdit, PlaylistEdited};

#[cfg(test)]
mod playlist_tests;

use crate::models::{
    Album, AlbumSort, Artist, FavoriteKey, FavoriteKind, Favorites, Genre, LyricLine, Lyrics,
    Playlist, SearchResults, ServerInfo, Song,
};

#[derive(Clone)]
pub struct Api {
    client: reqwest::Client,
    base_url: String,
    username: String,
    password: String,
}

impl Api {
    pub fn new(server_url: &str, username: &str, password: &str) -> Result<Self> {
        let base_url = if server_url.contains("://") {
            server_url.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", server_url.trim_end_matches('/'))
        };

        let probe = Url::parse(&format!("{base_url}/rest/ping.view"))?;
        if !matches!(probe.scheme(), "http" | "https") {
            return Err(anyhow!("server URL must use http or https"));
        }

        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn auth_params(&self) -> (String, String) {
        let mut rng = rand::thread_rng();
        let salt: String = (0..16)
            .map(|_| {
                let digit = rng.gen_range(0..10u8);
                char::from(b'0' + digit)
            })
            .collect();
        let mut hasher = Md5::new();
        hasher.update(self.password.as_bytes());
        hasher.update(salt.as_bytes());
        let token = format!("{:x}", hasher.finalize());
        (token, salt)
    }

    fn url_for(&self, view: &str, params: &[(&str, &str)]) -> Result<String> {
        let (token, salt) = self.auth_params();
        let mut url = Url::parse(&format!("{}/rest/{}.view", self.base_url, view))?;
        let mut query = url.query_pairs_mut();
        query.append_pair("u", &self.username);
        query.append_pair("t", &token);
        query.append_pair("s", &salt);
        query.append_pair("v", "1.16.1");
        query.append_pair("c", "navidrome-client");
        query.append_pair("f", "json");
        for (key, value) in params {
            query.append_pair(key, value);
        }
        drop(query);
        Ok(url.to_string())
    }

    async fn get_json(&self, view: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.request_json(view, params, false).await
    }

    async fn request_json(&self, view: &str, params: &[(&str, &str)], post: bool) -> Result<Value> {
        // Large playlists belong in a form body, not in a URL that a proxy may truncate.
        let request = if post {
            self.client.post(self.url_for(view, &[])?).form(params)
        } else {
            self.client.get(self.url_for(view, params)?)
        };
        let response = request
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("request to {view} failed"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .with_context(|| format!("read response from {view} failed"))?;

        if !status.is_success() {
            return Err(anyhow!(
                "server returned HTTP {status}: {}",
                text.chars().take(240).collect::<String>()
            ));
        }

        let root: Value = serde_json::from_str(&text).with_context(|| {
            format!(
                "invalid JSON from {view}: {}",
                text.chars().take(160).collect::<String>()
            )
        })?;
        let body = root
            .get("subsonic-response")
            .context("server response is not a Subsonic response")?;

        if body.get("status").and_then(Value::as_str) != Some("ok") {
            let code = body
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_i64);
            let message = body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown server error");
            return Err(anyhow!("Subsonic error {}: {message}", code.unwrap_or(-1)));
        }

        Ok(body.clone())
    }

    pub async fn ping(&self) -> Result<ServerInfo> {
        let body = self.get_json("ping", &[]).await?;
        Ok(ServerInfo {
            version: body
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            app_name: body.get("app").and_then(Value::as_str).map(str::to_string),
        })
    }

    pub async fn artists(&self) -> Result<Vec<Artist>> {
        let body = self.get_json("getArtists", &[]).await?;
        let mut artists = Vec::new();
        for index in body["artists"]["index"].as_array().into_iter().flatten() {
            for artist in index["artist"].as_array().into_iter().flatten() {
                artists.push(serde_json::from_value(artist.clone())?);
            }
        }
        Ok(artists)
    }

    pub async fn artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        let body = self.get_json("getArtist", &[("id", artist_id)]).await?;
        let mut albums = Vec::new();
        for album in body["artist"]["album"].as_array().into_iter().flatten() {
            albums.push(serde_json::from_value(album.clone())?);
        }
        Ok(albums)
    }

    pub async fn albums(&self, size: u32, offset: u32, sort: AlbumSort) -> Result<Vec<Album>> {
        let mut params = album_sort_params(sort);
        params.push(("size", size.to_string()));
        params.push(("offset", offset.to_string()));
        let params: Vec<(&str, &str)> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let body = self.get_json("getAlbumList2", &params).await?;
        parse_album_list2(&body)
    }

    /// 首页“最近添加”区块（`type=newest`，与专辑页排序状态解耦）。
    pub async fn albums_newest(&self, size: u32) -> Result<Vec<Album>> {
        self.albums_by_type("newest", size).await
    }

    /// 最近播放的专辑列表（依赖 Navidrome/Subsonic 的 `type=recent`）。
    pub async fn albums_recent(&self, size: u32) -> Result<Vec<Album>> {
        self.albums_by_type("recent", size).await
    }

    /// 播放次数最多的专辑列表（`type=frequent`）。
    pub async fn albums_frequent(&self, size: u32) -> Result<Vec<Album>> {
        self.albums_by_type("frequent", size).await
    }

    /// 随机专辑列表（`type=random`）。
    pub async fn albums_random(&self, size: u32) -> Result<Vec<Album>> {
        self.albums_by_type("random", size).await
    }

    async fn albums_by_type(&self, list_type: &str, size: u32) -> Result<Vec<Album>> {
        let body = self
            .get_json(
                "getAlbumList2",
                &[("type", list_type), ("size", &size.to_string())],
            )
            .await?;
        parse_album_list2(&body)
    }

    /// 全部流派（`getGenres`）。
    pub async fn genres(&self) -> Result<Vec<Genre>> {
        let body = self.get_json("getGenres", &[]).await?;
        let mut genres = Vec::new();
        for genre in body["genres"]["genre"].as_array().into_iter().flatten() {
            genres.push(serde_json::from_value(genre.clone())?);
        }
        Ok(genres)
    }

    /// 指定流派的专辑列表（`getAlbumList2 type=byGenre`），支持增量分页。
    pub async fn albums_by_genre(&self, genre: &str, size: u32, offset: u32) -> Result<Vec<Album>> {
        let body = self
            .get_json(
                "getAlbumList2",
                &[
                    ("type", "byGenre"),
                    ("genre", genre),
                    ("size", &size.to_string()),
                    ("offset", &offset.to_string()),
                ],
            )
            .await?;
        parse_album_list2(&body)
    }

    pub async fn album_songs(&self, album_id: &str) -> Result<Vec<Song>> {
        let body = self.get_json("getAlbum", &[("id", album_id)]).await?;
        let mut songs = Vec::new();
        for song in body["album"]["song"].as_array().into_iter().flatten() {
            songs.push(serde_json::from_value(song.clone())?);
        }
        Ok(songs)
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        let body = self.get_json("getPlaylists", &[]).await?;
        let mut playlists = Vec::new();
        for playlist in body["playlists"]["playlist"]
            .as_array()
            .into_iter()
            .flatten()
        {
            playlists.push(serde_json::from_value(playlist.clone())?);
        }
        Ok(playlists)
    }

    pub async fn favorites(&self) -> Result<Favorites> {
        let body = self.get_json("getStarred2", &[]).await?;
        let starred = &body["starred2"];
        Ok(Favorites {
            artists: parse_array(starred, "artist")?,
            albums: parse_array(starred, "album")?,
            songs: parse_array(starred, "song")?,
        })
    }

    pub async fn lyrics(&self, song: &Song) -> Result<Lyrics> {
        let structured = self
            .get_json("getLyricsBySongId", &[("id", &song.id)])
            .await;

        if let Ok(body) = &structured {
            if let Some(lyrics) = parse_structured_lyrics(body) {
                return Ok(lyrics);
            }
        }

        match self
            .get_json(
                "getLyrics",
                &[("artist", &song.artist), ("title", &song.title)],
            )
            .await
        {
            Ok(body) => Ok(parse_plain_lyrics(&body).unwrap_or_default()),
            Err(_) if structured.is_ok() => Ok(Lyrics::default()),
            Err(fallback_error) => Err(structured
                .expect_err("successful structured lyrics handled above")
                .context(format!(
                    "legacy lyrics request also failed: {fallback_error:#}"
                ))),
        }
    }

    pub async fn set_favorite(&self, key: &FavoriteKey, starred: bool) -> Result<()> {
        let view = if starred { "star" } else { "unstar" };
        self.get_json(view, &[(favorite_param(key.kind), &key.id)])
            .await?;
        Ok(())
    }

    /// 通知 Navidrome 当前正在播放的歌曲（更新“正在播放”列表）。
    pub async fn update_now_playing(&self, song_id: &str, time_secs: u32) -> Result<()> {
        self.get_json(
            "updateNowPlaying",
            &[("id", song_id), ("time", &time_secs.to_string())],
        )
        .await?;
        Ok(())
    }

    /// 向 Navidrome 提交 Scrobble（记录播放次数/最近播放）。submission=true 表示一次完整播放。
    pub async fn scrobble(&self, song_id: &str, submission: bool) -> Result<()> {
        let mut params = vec![("id", song_id)];
        if submission {
            params.push(("submission", "true"));
        }
        self.get_json("scrobble", &params).await?;
        Ok(())
    }

    pub async fn playlist(&self, playlist_id: &str) -> Result<PlaylistDetails> {
        let body = self.get_json("getPlaylist", &[("id", playlist_id)]).await?;
        Ok(serde_json::from_value(body["playlist"].clone())?)
    }

    pub async fn edit_playlist(&self, edit: &PlaylistEdit) -> Result<PlaylistEdited> {
        if let Some(id) = edit.id() {
            let current = self.playlist(id).await?;
            if !can_edit(&current.playlist, &self.username) {
                return Err(anyhow!("Only the playlist owner can edit this playlist."));
            }
            edit.validate_entries(&song_ids(&current.songs))?;
        }
        let mut params: Vec<(&str, String)> = Vec::new();
        let endpoint = match edit {
            PlaylistEdit::Create { name, songs } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(anyhow!("Enter a playlist name."));
                }
                params.push(("name", name.into()));
                params.extend(songs.iter().cloned().map(|id| ("songId", id)));
                "createPlaylist"
            }
            PlaylistEdit::Rename { id, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(anyhow!("Enter a playlist name."));
                }
                params.extend([("playlistId", id.clone()), ("name", name.into())]);
                "updatePlaylist"
            }
            PlaylistEdit::Delete { id } => {
                params.push(("id", id.clone()));
                "deletePlaylist"
            }
            PlaylistEdit::Append { id, songs } => {
                if songs.is_empty() {
                    return Err(anyhow!("Choose at least one song."));
                }
                params.push(("playlistId", id.clone()));
                params.extend(songs.iter().cloned().map(|id| ("songIdToAdd", id)));
                "updatePlaylist"
            }
            PlaylistEdit::Remove { id, index, .. } => {
                params.extend([
                    ("playlistId", id.clone()),
                    ("songIndexToRemove", index.to_string()),
                ]);
                "updatePlaylist"
            }
            PlaylistEdit::Move {
                id,
                expected,
                from,
                to,
            } => {
                let mut songs = expected.clone();
                let song = songs.remove(*from);
                songs.insert(*to, song);
                params.push(("playlistId", id.clone()));
                params.extend(songs.into_iter().map(|id| ("songId", id)));
                "createPlaylist"
            }
        };
        let params: Vec<_> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let body = self.request_json(endpoint, &params, true).await?;
        let id = match edit.id() {
            Some(id) => id.to_string(),
            None => body["playlist"]["id"]
                .as_str()
                .context("Created playlist response is missing its ID")?
                .to_string(),
        };
        // Read-after-write refreshes are separate: a failed refresh must never
        // present a successful append as failed and encourage adding it twice.
        Ok(PlaylistEdited {
            id,
            deleted: matches!(edit, PlaylistEdit::Delete { .. }),
        })
    }

    pub async fn search(
        &self,
        query: &str,
        song_count: u32,
        album_count: u32,
        artist_count: u32,
    ) -> Result<SearchResults> {
        let body = self
            .get_json(
                "search3",
                &[
                    ("query", query),
                    ("songCount", &song_count.to_string()),
                    ("albumCount", &album_count.to_string()),
                    ("artistCount", &artist_count.to_string()),
                ],
            )
            .await?;
        let result = &body["searchResult3"];
        Ok(SearchResults {
            artists: parse_array(result, "artist")?,
            albums: parse_array(result, "album")?,
            songs: parse_array(result, "song")?,
        })
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    pub fn stream_url(&self, id: &str, max_bit_rate: Option<u32>) -> Result<String> {
        let max_bit_rate = max_bit_rate.map(|value| value.to_string());
        let mut params = vec![("id", id), ("estimateContentLength", "true")];
        if let Some(max_bit_rate) = max_bit_rate.as_deref() {
            params.push(("maxBitRate", max_bit_rate));
            params.push(("format", "mp3"));
        }
        self.url_for("stream", &params)
    }

    pub fn cover_url(&self, id: &str, size: u32) -> Result<String> {
        self.url_for("getCoverArt", &[("id", id), ("size", &size.to_string())])
    }
}

fn favorite_param(kind: FavoriteKind) -> &'static str {
    match kind {
        FavoriteKind::Artist => "artistId",
        FavoriteKind::Album => "albumId",
        FavoriteKind::Song => "id",
    }
}

/// 从 `getAlbumList2` 响应体中解析专辑列表。
fn parse_album_list2(body: &Value) -> Result<Vec<Album>> {
    let mut albums = Vec::new();
    for album in body["albumList2"]["album"].as_array().into_iter().flatten() {
        albums.push(serde_json::from_value(album.clone())?);
    }
    Ok(albums)
}

/// 将专辑排序映射为 `getAlbumList2` 查询参数（除 size/offset 外）。
/// `byYear` 使用 fromYear > toYear 的倒序区间，年份新→旧。
fn album_sort_params(sort: AlbumSort) -> Vec<(&'static str, String)> {
    match sort {
        AlbumSort::NewestAdded => vec![("type", "newest".to_string())],
        AlbumSort::Name => vec![("type", "alphabeticalByName".to_string())],
        AlbumSort::Artist => vec![("type", "alphabeticalByArtist".to_string())],
        AlbumSort::Year => vec![
            ("type", "byYear".to_string()),
            ("fromYear", "9999".to_string()),
            ("toYear", "0".to_string()),
        ],
    }
}

fn parse_array<T>(value: &Value, key: &str) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut out = Vec::new();
    for item in value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(serde_json::from_value(item.clone())?);
    }
    Ok(out)
}

/// 解析单个 structuredLyrics 变体为歌词行（不含翻译）。
fn parse_lyric_lines(variant: &Value) -> Option<Vec<LyricLine>> {
    let offset = variant
        .get("offset")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let raw_lines = variant.get("line")?.as_array()?;
    Some(
        raw_lines
            .iter()
            .map(|line| {
                let start_ms = line.get("start").and_then(Value::as_i64).map(|start| {
                    let adjusted = i128::from(start) + i128::from(offset);
                    adjusted.clamp(0, i128::from(u64::MAX)) as u64
                });
                LyricLine {
                    start_ms,
                    text: line
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    translation: None,
                }
            })
            .collect(),
    )
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

/// 变体排序键：同步优先、行数多者优先。
fn variant_rank(variant: &Value) -> (bool, usize) {
    (
        variant
            .get("synced")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        variant
            .get("line")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
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

fn parse_structured_lyrics(body: &Value) -> Option<Lyrics> {
    let variants = body
        .get("lyricsList")?
        .get("structuredLyrics")?
        .as_array()?;
    let usable = variants
        .iter()
        .filter(|lyrics| {
            lyrics
                .get("line")
                .and_then(Value::as_array)
                .is_some_and(|lines| !lines.is_empty())
        })
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

fn parse_plain_lyrics(body: &Value) -> Option<Lyrics> {
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
            .map(|line| LyricLine {
                start_ms: None,
                text: line.to_string(),
                translation: None,
            })
            .collect(),
    })
}

pub fn format_duration(seconds: Option<i32>) -> String {
    let seconds = seconds.unwrap_or(0).max(0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        let mut out = String::new();
        let _ = write!(out, "{hours}:{minutes:02}:{secs:02}");
        out
    } else {
        let mut out = String::new();
        let _ = write!(out, "{minutes}:{secs:02}");
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;

    #[test]
    fn formats_duration() {
        assert_eq!(format_duration(Some(65)), "1:05");
        assert_eq!(format_duration(Some(3661)), "1:01:01");
        assert_eq!(format_duration(None), "0:00");
    }

    #[test]
    fn builds_subsonic_auth_query() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api.url_for("ping", &[]).unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

        assert_eq!(params["u"], "alice");
        assert_eq!(params["v"], "1.16.1");
        assert_eq!(params["c"], "navidrome-client");
        assert_eq!(params["f"], "json");
        assert_eq!(params["t"].len(), 32);
        assert_eq!(params["s"].len(), 16);
    }

    #[test]
    fn update_now_playing_uses_subsonic_parameters() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api
            .url_for("updateNowPlaying", &[("id", "song-9"), ("time", "42")])
            .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["id"], "song-9");
        assert_eq!(params["time"], "42");
    }

    #[test]
    fn scrobble_marks_completed_playback() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api
            .url_for("scrobble", &[("id", "song-9"), ("submission", "true")])
            .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["id"], "song-9");
        assert_eq!(params["submission"], "true");
    }

    #[test]
    fn recent_album_list_uses_the_recent_type() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api
            .url_for("getAlbumList2", &[("type", "recent"), ("size", "30")])
            .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["type"], "recent");
        assert_eq!(params["size"], "30");
    }

    #[test]
    fn home_album_sections_use_supported_list_types() {
        for list_type in ["frequent", "random"] {
            let api = Api::new("http://example.test", "alice", "secret").unwrap();
            let url = api
                .url_for("getAlbumList2", &[("type", list_type), ("size", "30")])
                .unwrap();
            let parsed = Url::parse(&url).unwrap();
            let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
            assert_eq!(params["type"], list_type);
        }
    }

    #[test]
    fn album_sort_params_map_to_subsonic_list_types() {
        assert_eq!(
            album_sort_params(AlbumSort::NewestAdded),
            vec![("type", "newest".to_string())]
        );
        assert_eq!(
            album_sort_params(AlbumSort::Name),
            vec![("type", "alphabeticalByName".to_string())]
        );
        assert_eq!(
            album_sort_params(AlbumSort::Artist),
            vec![("type", "alphabeticalByArtist".to_string())]
        );
        assert_eq!(
            album_sort_params(AlbumSort::Year),
            vec![
                ("type", "byYear".to_string()),
                ("fromYear", "9999".to_string()),
                ("toYear", "0".to_string()),
            ]
        );
    }

    #[test]
    fn albums_by_genre_uses_subsonic_parameters() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api
            .url_for(
                "getAlbumList2",
                &[
                    ("type", "byGenre"),
                    ("genre", "Jazz"),
                    ("size", "100"),
                    ("offset", "300"),
                ],
            )
            .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["type"], "byGenre");
        assert_eq!(params["genre"], "Jazz");
        assert_eq!(params["size"], "100");
        assert_eq!(params["offset"], "300");
    }

    #[test]
    fn genre_parses_subsonic_camel_case_counts() {
        let genre: Genre = serde_json::from_value(serde_json::json!({
            "value": "Progressive Rock",
            "songCount": 42,
            "albumCount": 7,
        }))
        .expect("genre should deserialize");
        assert_eq!(genre.value, "Progressive Rock");
        assert_eq!(genre.song_count, Some(42));
        assert_eq!(genre.album_count, Some(7));

        let minimal: Genre = serde_json::from_value(serde_json::json!({"value": "Rock"}))
            .expect("genre without counts should deserialize");
        assert_eq!(minimal.song_count, None);
        assert_eq!(minimal.album_count, None);
    }

    #[test]
    fn sorted_album_request_includes_type_size_and_offset() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let mut params = album_sort_params(AlbumSort::Artist);
        params.push(("size", "100".to_string()));
        params.push(("offset", "300".to_string()));
        let params: Vec<(&str, &str)> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let url = api.url_for("getAlbumList2", &params).unwrap();
        let parsed = Url::parse(&url).unwrap();
        let query: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(query["type"], "alphabeticalByArtist");
        assert_eq!(query["size"], "100");
        assert_eq!(query["offset"], "300");
    }

    #[test]
    fn stream_url_includes_optional_max_bit_rate() {
        let api = Api::new("http://example.test", "alice", "secret").unwrap();
        let url = api.stream_url("song-1", Some(320)).unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

        assert_eq!(params["id"], "song-1");
        assert_eq!(params["maxBitRate"], "320");
        assert_eq!(params["format"], "mp3");
        assert_eq!(params["estimateContentLength"], "true");
    }

    #[test]
    fn accepts_urls_without_scheme() {
        let api = Api::new("localhost:4533", "alice", "secret").unwrap();
        assert_eq!(api.base_url(), "http://localhost:4533");
    }

    #[test]
    fn uses_subsonic_favorite_parameter_names() {
        assert_eq!(favorite_param(FavoriteKind::Artist), "artistId");
        assert_eq!(favorite_param(FavoriteKind::Album), "albumId");
        assert_eq!(favorite_param(FavoriteKind::Song), "id");
    }

    fn test_lyric_line(start_ms: Option<u64>, text: &str) -> LyricLine {
        LyricLine {
            start_ms,
            text: text.to_string(),
            translation: None,
        }
    }

    #[test]
    fn translations_pair_by_nearest_timestamp_within_tolerance() {
        let mut primary = vec![
            test_lyric_line(Some(0), "hello"),
            test_lyric_line(Some(5000), "world"),
        ];
        let candidate = vec![
            test_lyric_line(Some(100), "你好"),
            test_lyric_line(Some(5100), "世界"),
            test_lyric_line(Some(9000), "多余行"),
        ];
        pair_translations(&mut primary, &candidate);
        assert_eq!(primary[0].translation.as_deref(), Some("你好"));
        assert_eq!(primary[1].translation.as_deref(), Some("世界"));
    }

    #[test]
    fn translations_reject_far_and_duplicate_and_empty_text() {
        let mut primary = vec![
            test_lyric_line(Some(0), "a"),
            test_lyric_line(Some(10_000), "b"),
            test_lyric_line(Some(20_000), "same"),
        ];
        let candidate = vec![
            test_lyric_line(Some(30_000), "超出容差"),
            test_lyric_line(Some(20_000), "same"),
            test_lyric_line(Some(10_000), ""),
        ];
        pair_translations(&mut primary, &candidate);
        assert_eq!(primary[0].translation, None);
        // 空文本不作为翻译。
        assert_eq!(primary[1].translation, None);
        // 与原文相同也不重复显示。
        assert_eq!(primary[2].translation, None);
    }

    #[test]
    fn unsynced_translations_pair_by_line_index_only() {
        let mut primary = vec![test_lyric_line(None, "one"), test_lyric_line(None, "two")];
        let candidate = vec![test_lyric_line(None, "一"), test_lyric_line(None, "二")];
        pair_translations(&mut primary, &candidate);
        assert_eq!(primary[0].translation.as_deref(), Some("一"));
        assert_eq!(primary[1].translation.as_deref(), Some("二"));

        // 行数不同时不做行号对齐，宁缺毋滥。
        let mut primary = vec![test_lyric_line(None, "one")];
        pair_translations(&mut primary, &candidate);
        assert_eq!(primary[0].translation, None);
    }

    #[test]
    fn structured_lyrics_translation_variant_is_extracted() {
        let body = serde_json::json!({
            "lyricsList": { "structuredLyrics": [
                {
                    "lang": "eng",
                    "synced": true,
                    "line": [
                        {"start": 0, "value": "hello"},
                        {"start": 5000, "value": "world"},
                    ]
                },
                {
                    "lang": "zho",
                    "synced": true,
                    "line": [
                        {"start": 0, "value": "你好"},
                        {"start": 5000, "value": "世界"},
                    ]
                },
            ]}
        });
        let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
        assert_eq!(lyrics.lines[0].text, "hello");
        assert_eq!(lyrics.lines[0].translation.as_deref(), Some("你好"));
        assert_eq!(lyrics.lines[1].text, "world");
        assert_eq!(lyrics.lines[1].translation.as_deref(), Some("世界"));
    }

    #[test]
    fn same_language_variants_do_not_become_translations() {
        let body = serde_json::json!({
            "lyricsList": { "structuredLyrics": [
                {
                    "lang": "eng",
                    "synced": true,
                    "line": [{"start": 0, "value": "hello"}]
                },
                {
                    "lang": "eng",
                    "synced": false,
                    "line": [{"value": "hello (alternate)"}]
                },
            ]}
        });
        let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
        assert_eq!(lyrics.lines[0].text, "hello");
        assert_eq!(lyrics.lines[0].translation, None);
    }

    #[test]
    fn parses_synced_structured_lyrics_with_offset() {
        let body = serde_json::json!({
            "lyricsList": {
                "structuredLyrics": [{
                    "synced": true,
                    "offset": -100,
                    "line": [
                        {"start": 1000, "value": "First line"},
                        {"start": 2500, "value": "Second line"}
                    ]
                }]
            }
        });

        let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
        assert!(lyrics.is_synced());
        assert_eq!(lyrics.lines[0].start_ms, Some(900));
        assert_eq!(
            lyrics.active_line_index(Duration::from_millis(2_000)),
            Some(0)
        );
        assert_eq!(
            lyrics.active_line_index(Duration::from_millis(3_000)),
            Some(1)
        );
    }

    #[test]
    fn parses_plain_lyrics_lines() {
        let body = serde_json::json!({
            "lyrics": {
                "artist": "Artist",
                "title": "Song",
                "value": "First line\nSecond line"
            }
        });

        let lyrics = parse_plain_lyrics(&body).expect("plain lyrics should parse");
        assert!(!lyrics.is_synced());
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[1].text, "Second line");
    }
}
