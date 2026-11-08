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
            Self::Bilibili(_) => anyhow::bail!("This source does not support this operation"),
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
        }
    }

    pub async fn open_subsonic_info(&self) -> Result<ServerInfo> {
        self.subsonic()?.open_subsonic_info().await
    }

    pub async fn artists(&self) -> Result<Vec<Artist>> {
        self.subsonic()?.artists().await
    }

    pub async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        self.subsonic()?.artist_albums(id).await
    }

    pub async fn artist_top_songs(
        &self,
        artist: &Artist,
        count: u32,
        support: CapabilitySupport,
    ) -> Result<Vec<Song>> {
        Ok(self.bind_songs(
            self.subsonic()?
                .artist_top_songs(artist, count, support)
                .await?,
        ))
    }

    pub async fn albums(&self, size: u32, offset: u32, sort: AlbumSort) -> Result<Vec<Album>> {
        self.subsonic()?.albums(size, offset, sort).await
    }

    pub async fn albums_newest(&self, size: u32) -> Result<Vec<Album>> {
        self.subsonic()?.albums_newest(size).await
    }

    pub async fn albums_recent(&self, size: u32) -> Result<Vec<Album>> {
        self.subsonic()?.albums_recent(size).await
    }

    pub async fn albums_frequent(&self, size: u32) -> Result<Vec<Album>> {
        self.subsonic()?.albums_frequent(size).await
    }

    pub async fn albums_random(&self, size: u32) -> Result<Vec<Album>> {
        self.subsonic()?.albums_random(size).await
    }

    pub async fn genres(&self) -> Result<Vec<Genre>> {
        self.subsonic()?.genres().await
    }

    pub async fn albums_by_genre(&self, genre: &str, size: u32, offset: u32) -> Result<Vec<Album>> {
        self.subsonic()?.albums_by_genre(genre, size, offset).await
    }

    pub async fn album_songs(&self, id: &str) -> Result<Vec<Song>> {
        Ok(self.bind_songs(self.subsonic()?.album_songs(id).await?))
    }

    pub async fn similar_songs(&self, id: &str, count: u32) -> Result<Vec<Song>> {
        Ok(self.bind_songs(self.subsonic()?.similar_songs(id, count).await?))
    }

    pub async fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> Result<Vec<Song>> {
        Ok(self.bind_songs(
            self.subsonic()?
                .songs_by_genre(genre, count, offset)
                .await?,
        ))
    }

    pub async fn favorites(&self) -> Result<Favorites> {
        let mut favorites = self.subsonic()?.favorites().await?;
        favorites.songs = self.bind_songs(favorites.songs);
        Ok(favorites)
    }

    pub async fn set_favorite(&self, key: &FavoriteKey, starred: bool) -> Result<()> {
        self.subsonic()?.set_favorite(key, starred).await
    }

    pub async fn search(
        &self,
        query: &str,
        songs: u32,
        albums: u32,
        artists: u32,
    ) -> Result<SearchResults> {
        let mut result = self
            .subsonic()?
            .search(query, songs, albums, artists)
            .await?;
        result.songs = self.bind_songs(result.songs);
        Ok(result)
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.playlists().await,
            Self::Bilibili(provider) => Ok(provider
                .favorite_folders()
                .await?
                .into_iter()
                .map(|folder| Playlist {
                    id: folder.id.to_string(),
                    name: folder.title,
                    song_count: i32::try_from(folder.media_count).ok(),
                    ..Default::default()
                })
                .collect()),
        }
    }

    pub async fn playlist(&self, id: &str) -> Result<PlaylistDetails> {
        let mut playlist = match self {
            Self::OpenSubsonic(_) => self.subsonic()?.playlist(id).await?,
            Self::Bilibili(provider) => {
                let id = id
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("Invalid Bilibili folder ID"))?;
                let tracks = provider.favorite_tracks(id).await?;
                PlaylistDetails {
                    playlist: Playlist {
                        id: id.to_string(),
                        name: tracks.folder.title,
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

    pub async fn lyrics(&self, song: &Song, support: CapabilitySupport) -> Result<Lyrics> {
        self.subsonic()?.lyrics(song, support).await
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
        }
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        match self {
            Self::OpenSubsonic(_) => self.subsonic()?.get_bytes(url).await,
            Self::Bilibili(provider) => Ok(provider.artwork(url).await?),
        }
    }
}
