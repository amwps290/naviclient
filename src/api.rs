use std::fmt::Write;

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};
use rand::Rng;
use serde_json::Value;
use url::Url;

use crate::models::{
    Album, Artist, FavoriteKey, FavoriteKind, Favorites, LyricLine, Lyrics, Playlist,
    SearchResults, ServerInfo, Song,
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
        let url = self.url_for(view, params)?;
        let response = self
            .client
            .get(&url)
            .send()
            .await
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

    pub async fn albums(&self, size: u32, offset: u32) -> Result<Vec<Album>> {
        let body = self
            .get_json(
                "getAlbumList2",
                &[
                    ("type", "newest"),
                    ("size", &size.to_string()),
                    ("offset", &offset.to_string()),
                ],
            )
            .await?;
        let mut albums = Vec::new();
        for album in body["albumList2"]["album"].as_array().into_iter().flatten() {
            albums.push(serde_json::from_value(album.clone())?);
        }
        Ok(albums)
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

    pub async fn playlist_songs(&self, playlist_id: &str) -> Result<Vec<Song>> {
        let body = self.get_json("getPlaylist", &[("id", playlist_id)]).await?;
        let mut songs = Vec::new();
        for song in body["playlist"]["entry"].as_array().into_iter().flatten() {
            songs.push(serde_json::from_value(song.clone())?);
        }
        Ok(songs)
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

fn parse_structured_lyrics(body: &Value) -> Option<Lyrics> {
    let variants = body
        .get("lyricsList")?
        .get("structuredLyrics")?
        .as_array()?;
    let selected = variants
        .iter()
        .filter(|lyrics| {
            lyrics
                .get("line")
                .and_then(Value::as_array)
                .is_some_and(|lines| !lines.is_empty())
        })
        .max_by_key(|lyrics| {
            let synced = lyrics
                .get("synced")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let line_count = lyrics
                .get("line")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (synced, line_count)
        })?;
    let offset = selected
        .get("offset")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let lines = selected
        .get("line")?
        .as_array()?
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
            }
        })
        .collect();

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
