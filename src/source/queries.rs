//! Shared query boundary. Provider-specific protocol types remain inside the
//! adapter; server extension support is still negotiated independently.

use anyhow::Result;

use super::MusicSource;
use crate::api::OpenSubsonicProvider;
use crate::models::{
    Album, AlbumSort, Artist, CapabilitySupport, FavoriteKey, Favorites, Genre, Lyrics, Playlist,
    SearchResults, ServerInfo, Song,
};
use crate::playlist::{PlaylistDetails, PlaylistEdit, PlaylistEdited};

mod playlist_loading;

impl MusicSource {
    fn bind_songs(&self, mut songs: Vec<Song>) -> Vec<Song> {
        for song in &mut songs {
            song.source = Some(self.id().clone());
        }
        songs
    }

    fn subsonic(&self) -> Result<&OpenSubsonicProvider> {
        match self {
            Self::OpenSubsonic(adapter) => Ok(adapter.provider()),
            Self::Bilibili(_) | Self::LocalFolder(_) | Self::WebDav(_) => {
                anyhow::bail!("This source does not support this operation")
            }
        }
    }

    pub async fn ping(&self) -> Result<ServerInfo> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.ping().await,
            Self::Bilibili(provider) => {
                provider.verify().await?;
                let mut info = ServerInfo {
                    app_name: Some("Bilibili".into()),
                    protocol_version: "unknown".into(),
                    extensions_discovered: true,
                    ..Default::default()
                };
                info.refresh_capabilities();
                Ok(info)
            }
            Self::LocalFolder(provider) => provider.ping().await,
            Self::WebDav(provider) => provider.ping().await,
        }
    }

    pub async fn open_subsonic_info(&self) -> Result<ServerInfo> {
        self.subsonic()?.open_subsonic_info().await
    }

    pub async fn artists(&self) -> Result<Vec<Artist>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.artists()),
            Self::WebDav(provider) => Ok(provider.artists()),
            _ => self.subsonic()?.artists().await,
        }
    }

    pub async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.artist_albums(id)),
            Self::WebDav(provider) => Ok(provider.artist_albums(id)),
            _ => self.subsonic()?.artist_albums(id).await,
        }
    }

    pub async fn artist_top_songs(
        &self,
        artist: &Artist,
        count: u32,
        support: CapabilitySupport,
    ) -> Result<Vec<Song>> {
        let songs = match self {
            Self::LocalFolder(provider) => provider.artist_top_songs(artist, count),
            Self::WebDav(provider) => provider.artist_top_songs(artist, count),
            _ => self
                .subsonic()?
                .artist_top_songs(artist, count, support)
                .await?,
        };
        Ok(self.bind_songs(songs))
    }

    pub async fn albums(&self, size: u32, offset: u32, sort: AlbumSort) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums(size, offset, sort)),
            Self::WebDav(provider) => Ok(provider.albums(size, offset, sort)),
            _ => self.subsonic()?.albums(size, offset, sort).await,
        }
    }

    pub async fn albums_newest(&self, size: u32) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums_newest(size)),
            Self::WebDav(provider) => Ok(provider.albums_newest(size)),
            _ => self.subsonic()?.albums_newest(size).await,
        }
    }

    pub async fn albums_recent(&self, size: u32) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums_recent(size)),
            Self::WebDav(provider) => Ok(provider.albums_recent(size)),
            _ => self.subsonic()?.albums_recent(size).await,
        }
    }

    pub async fn albums_frequent(&self, size: u32) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums_frequent(size)),
            Self::WebDav(provider) => Ok(provider.albums_frequent(size)),
            _ => self.subsonic()?.albums_frequent(size).await,
        }
    }

    pub async fn albums_random(&self, size: u32) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums_random(size)),
            Self::WebDav(provider) => Ok(provider.albums_random(size)),
            _ => self.subsonic()?.albums_random(size).await,
        }
    }

    pub async fn genres(&self) -> Result<Vec<Genre>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.genres()),
            Self::WebDav(provider) => Ok(provider.genres()),
            _ => self.subsonic()?.genres().await,
        }
    }

    pub async fn albums_by_genre(&self, genre: &str, size: u32, offset: u32) -> Result<Vec<Album>> {
        match self {
            Self::LocalFolder(provider) => Ok(provider.albums_by_genre(genre, size, offset)),
            Self::WebDav(provider) => Ok(provider.albums_by_genre(genre, size, offset)),
            _ => self.subsonic()?.albums_by_genre(genre, size, offset).await,
        }
    }

    pub async fn album_songs(&self, id: &str) -> Result<Vec<Song>> {
        let songs = match self {
            Self::LocalFolder(provider) => provider.album_songs(id),
            Self::WebDav(provider) => provider.album_songs(id),
            _ => self.subsonic()?.album_songs(id).await?,
        };
        Ok(self.bind_songs(songs))
    }

    pub async fn similar_songs(&self, id: &str, count: u32) -> Result<Vec<Song>> {
        Ok(self.bind_songs(self.subsonic()?.similar_songs(id, count).await?))
    }

    pub async fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> Result<Vec<Song>> {
        let songs = match self {
            Self::LocalFolder(provider) => provider.songs_by_genre(genre, count, offset),
            Self::WebDav(provider) => provider.songs_by_genre(genre, count, offset),
            _ => self
                .subsonic()?
                .songs_by_genre(genre, count, offset)
                .await?,
        };
        Ok(self.bind_songs(songs))
    }

    pub async fn favorites(&self) -> Result<Favorites> {
        let mut favorites = match self {
            Self::LocalFolder(provider) => provider.favorites(),
            Self::WebDav(provider) => provider.favorites(),
            _ => self.subsonic()?.favorites().await?,
        };
        favorites.songs = self.bind_songs(favorites.songs);
        Ok(favorites)
    }

    pub async fn set_favorite(&self, key: &FavoriteKey, starred: bool) -> Result<()> {
        match self {
            Self::LocalFolder(provider) => provider.set_favorite(key, starred),
            Self::WebDav(provider) => provider.set_favorite(key, starred),
            _ => self.subsonic()?.set_favorite(key, starred).await,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        songs: u32,
        albums: u32,
        artists: u32,
    ) -> Result<SearchResults> {
        let mut result = match self {
            Self::LocalFolder(provider) => provider.search(query, songs, albums, artists),
            Self::WebDav(provider) => provider.search(query, songs, albums, artists),
            _ => self
                .subsonic()?
                .search(query, songs, albums, artists)
                .await?,
        };
        result.songs = self.bind_songs(result.songs);
        Ok(result)
    }

    pub async fn search_bilibili_videos(
        &self,
        keyword: &str,
        page: u32,
    ) -> Result<super::bilibili::discovery::SearchPage> {
        match self {
            Self::Bilibili(provider) => Ok(provider.search_videos(keyword, page).await?),
            _ => anyhow::bail!("This source does not support Bilibili video search"),
        }
    }

    pub async fn open_bilibili_video(
        &self,
        input: &str,
    ) -> Result<super::bilibili::discovery::VideoTracks> {
        match self {
            Self::Bilibili(provider) => Ok(provider.open_video(input).await?),
            _ => anyhow::bail!("This source does not support Bilibili videos"),
        }
    }

    pub async fn bilibili_folder_cover(
        &self,
        folder_id: u64,
    ) -> Result<Option<String>, super::bilibili::client::BilibiliError> {
        match self {
            Self::Bilibili(provider) => provider.favorite_folder_cover(folder_id).await,
            _ => Err(super::bilibili::client::BilibiliError::InvalidResponse),
        }
    }

    pub async fn prepare_bilibili_favorite(
        &self,
        bvid: &str,
    ) -> Result<super::bilibili::favorites::FavoriteSnapshot> {
        match self {
            Self::Bilibili(provider) => Ok(provider.prepare_video_favorite(bvid).await?),
            _ => anyhow::bail!("This source does not support Bilibili favorites"),
        }
    }
    pub async fn change_bilibili_favorite(
        &self,
        snapshot: &super::bilibili::favorites::FavoriteSnapshot,
        operation: super::bilibili::favorites::FavoriteOperation,
    ) -> Result<super::bilibili::favorites::FavoriteOutcome> {
        match self {
            Self::Bilibili(provider) => {
                Ok(provider.change_video_favorite(snapshot, operation).await?)
            }
            _ => anyhow::bail!("This source does not support Bilibili favorites"),
        }
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.playlists().await,
            Self::LocalFolder(_) | Self::WebDav(_) => Ok(Vec::new()),
            Self::Bilibili(provider) => Ok(provider
                .favorite_folders()
                .await?
                .into_iter()
                .map(|folder| Playlist {
                    id: folder.id.to_string(),
                    name: folder.title,
                    cover_art: folder
                        .cover
                        .as_deref()
                        .and_then(|cover| provider.cover_url(cover).ok()),
                    song_count: i32::try_from(folder.media_count).ok(),
                    ..Default::default()
                })
                .collect()),
        }
    }

    pub async fn playlist(&self, id: &str) -> Result<PlaylistDetails> {
        let mut playlist = match self {
            Self::OpenSubsonic(_) => self.subsonic()?.playlist(id).await?,
            Self::LocalFolder(_) | Self::WebDav(_) => {
                anyhow::bail!("Playlists are not available for this source yet")
            }
            Self::Bilibili(provider) => {
                let id = id
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("Invalid Bilibili folder ID"))?;
                let tracks = provider.favorite_tracks(id).await?;
                PlaylistDetails {
                    playlist: Playlist {
                        id: id.to_string(),
                        name: tracks.folder.title,
                        cover_art: tracks
                            .folder
                            .cover
                            .as_deref()
                            .and_then(|cover| provider.cover_url(cover).ok()),
                        song_count: i32::try_from(tracks.songs.len()).ok(),
                        duration: i32::try_from(
                            tracks
                                .songs
                                .iter()
                                .map(|song| song.duration.unwrap_or_default() as u64)
                                .sum::<u64>(),
                        )
                        .ok(),
                        ..Default::default()
                    },
                    songs: tracks.songs,
                    unavailable_count: tracks.unavailable,
                }
            }
        };
        playlist.songs = self.bind_songs(playlist.songs);
        Ok(playlist)
    }

    pub async fn edit_playlist(&self, edit: &PlaylistEdit) -> Result<PlaylistEdited> {
        self.subsonic()?.edit_playlist(edit).await
    }

    pub async fn lyrics(
        &self,
        song: &Song,
        support: impl Into<crate::models::ServerCapabilities>,
    ) -> Result<Lyrics> {
        match self {
            Self::LocalFolder(provider) => {
                let provider = provider.clone();
                let song = song.clone();
                tokio::task::spawn_blocking(move || provider.lyrics(&song))
                    .await
                    .map_err(|error| anyhow::anyhow!("Local lyrics lookup interrupted: {error}"))?
            }
            Self::WebDav(provider) => provider.lyrics(song).await,
            _ => self.subsonic()?.lyrics(song, support).await,
        }
    }

    pub async fn update_now_playing(&self, id: &str, position: u32) -> Result<()> {
        self.subsonic()?.update_now_playing(id, position).await
    }

    pub async fn scrobble(&self, id: &str, submission: bool) -> Result<()> {
        self.subsonic()?.scrobble(id, submission).await
    }

    pub fn cover_url(&self, id: &str, size: u32) -> Result<String> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.cover_url(id, size),
            Self::Bilibili(provider) => Ok(provider.cover_url(id)?),
            Self::LocalFolder(provider) => provider.cover_url(id),
            Self::WebDav(provider) => provider.cover_url(id),
        }
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.get_bytes(url).await,
            Self::Bilibili(provider) => Ok(provider.artwork(url).await?),
            Self::LocalFolder(provider) => {
                let provider = provider.clone();
                let url = url.to_string();
                tokio::task::spawn_blocking(move || provider.cover_bytes(&url))
                    .await
                    .map_err(|error| anyhow::anyhow!("Local cover read interrupted: {error}"))?
            }
            Self::WebDav(provider) => provider.cover_bytes(url).await,
        }
    }
}
