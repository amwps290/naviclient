//! Library browsing, search, and favorites endpoints and response parsing.

use anyhow::Result;
use serde_json::Value;

use super::{parse_array, OpenSubsonicProvider};
use crate::models::{
    Album, AlbumSort, Artist, CapabilitySupport, FavoriteKey, FavoriteKind, Favorites, Genre,
    SearchResults, Song,
};

impl OpenSubsonicProvider {
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

    /// Return an artist's popular songs. OpenSubsonic's
    /// `topSongsByArtistId` extension avoids ambiguous display-name matching;
    /// compatible legacy servers keep using the standard `artist` parameter.
    pub async fn artist_top_songs(
        &self,
        artist: &Artist,
        count: u32,
        artist_id_support: CapabilitySupport,
    ) -> Result<Vec<Song>> {
        let params = artist_top_songs_params(artist, count, artist_id_support);
        let params: Vec<(&str, &str)> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let body = self.get_json("getTopSongs", &params).await?;
        parse_array(&body["topSongs"], "song")
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

    pub async fn similar_songs(&self, song_id: &str, count: u32) -> Result<Vec<Song>> {
        let count = count.to_string();
        let body = self
            .get_json("getSimilarSongs2", &[("id", song_id), ("count", &count)])
            .await?;
        parse_array(&body["similarSongs2"], "song")
    }

    pub async fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> Result<Vec<Song>> {
        let count = count.to_string();
        let offset = offset.to_string();
        let body = self
            .get_json(
                "getSongsByGenre",
                &[("genre", genre), ("count", &count), ("offset", &offset)],
            )
            .await?;
        parse_array(&body["songsByGenre"], "song")
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

    pub async fn set_favorite(&self, key: &FavoriteKey, starred: bool) -> Result<()> {
        let view = if starred { "star" } else { "unstar" };
        self.get_json(view, &[(favorite_param(key.kind), &key.id)])
            .await?;
        Ok(())
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

fn artist_top_songs_params(
    artist: &Artist,
    count: u32,
    artist_id_support: CapabilitySupport,
) -> Vec<(&'static str, String)> {
    let identity = if artist_id_support.is_supported() && !artist.id.trim().is_empty() {
        ("artistId", artist.id.clone())
    } else {
        ("artist", artist.name.clone())
    };
    vec![identity, ("count", count.max(1).to_string())]
}

#[cfg(test)]
mod tests;
