//! Library browsing, search, and favorites endpoints and response parsing.

use anyhow::Result;
use serde_json::Value;

use super::{parse_array, OpenSubsonicProvider};
use crate::models::{
    Album, AlbumSort, Artist, CapabilitySupport, FavoriteKey, FavoriteKind, Favorites, FolderItem,
    FolderListing, Genre, SearchResults, Song,
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

    /// Browse directory hierarchy via Subsonic `getMusicFolders`, `getIndexes`, and `getMusicDirectory`.
    pub async fn folder_listing(&self, folder_id: &str) -> Result<FolderListing> {
        if folder_id.is_empty() {
            let body = self.get_json("getMusicFolders", &[]).await?;
            let roots: Vec<(String, String)> = body["musicFolders"]["musicFolder"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    let id = item
                        .get("id")
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_string)
                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                        })?;
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Music")
                        .to_string();
                    Some((id, name))
                })
                .collect();
            if roots.len() == 1 {
                return self.music_folder_indexes(&roots[0].0, &roots[0].1).await;
            }
            if roots.is_empty() {
                return self.music_folder_indexes("", "").await;
            }
            let folders = roots
                .into_iter()
                .map(|(id, name)| FolderItem {
                    id: format!("root:{id}"),
                    name,
                    cover_art: None,
                    song_count: None,
                })
                .collect();
            return Ok(FolderListing {
                id: String::new(),
                name: String::new(),
                folders,
                songs: Vec::new(),
            });
        }
        if let Some(music_folder_id) = folder_id.strip_prefix("root:") {
            return self.music_folder_indexes(music_folder_id, "").await;
        }
        let body = self
            .get_json("getMusicDirectory", &[("id", folder_id)])
            .await?;
        let dir = &body["directory"];
        let name = dir
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut folders = Vec::new();
        let mut songs = Vec::new();
        for child in dir["child"].as_array().into_iter().flatten() {
            if child.get("isDir").and_then(Value::as_bool).unwrap_or(false) {
                let Some(id) = child.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let title = child
                    .get("title")
                    .or_else(|| child.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string();
                let cover_art = child
                    .get("coverArt")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                folders.push(FolderItem {
                    id: id.to_string(),
                    name: title,
                    cover_art,
                    song_count: None,
                });
            } else if let Ok(song) = serde_json::from_value::<Song>(child.clone()) {
                songs.push(song);
            }
        }
        folders.sort_by_key(|f| f.name.to_lowercase());
        Ok(FolderListing {
            id: folder_id.to_string(),
            name,
            folders,
            songs,
        })
    }

    async fn music_folder_indexes(
        &self,
        music_folder_id: &str,
        folder_name: &str,
    ) -> Result<FolderListing> {
        let params: Vec<(&str, &str)> = if music_folder_id.is_empty() {
            Vec::new()
        } else {
            vec![("musicFolderId", music_folder_id)]
        };
        let body = self.get_json("getIndexes", &params).await?;
        let indexes = &body["indexes"];
        let mut folders = Vec::new();
        for group in indexes["index"].as_array().into_iter().flatten() {
            for artist in group["artist"].as_array().into_iter().flatten() {
                let Some(id) = artist.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let name = artist
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string();
                let cover_art = artist
                    .get("coverArt")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                folders.push(FolderItem {
                    id: id.to_string(),
                    name,
                    cover_art,
                    song_count: None,
                });
            }
        }
        let mut songs = Vec::new();
        for child in indexes["child"].as_array().into_iter().flatten() {
            if !child.get("isDir").and_then(Value::as_bool).unwrap_or(false) {
                if let Ok(song) = serde_json::from_value::<Song>(child.clone()) {
                    songs.push(song);
                }
            }
        }
        folders.sort_by_key(|f| f.name.to_lowercase());
        Ok(FolderListing {
            id: if music_folder_id.is_empty() {
                String::new()
            } else {
                format!("root:{music_folder_id}")
            },
            name: folder_name.to_string(),
            folders,
            songs,
        })
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
