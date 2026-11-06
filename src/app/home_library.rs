//! Home album sections, request identity, and artwork prefetching.

use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HomeAlbumSection {
    Recent,
    Frequent,
    Newest,
    Random,
}

impl HomeAlbumSection {
    pub(super) const ALL: [Self; 4] = [Self::Recent, Self::Frequent, Self::Newest, Self::Random];

    fn key(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Frequent => "frequent",
            Self::Newest => "newest",
            Self::Random => "random",
        }
    }
}

#[derive(Default)]
struct AlbumSection {
    albums: Vec<Album>,
    load: LibraryLoad,
}

#[derive(Default)]
pub(super) struct HomeAlbums {
    sections: [AlbumSection; 4],
}

impl HomeAlbums {
    pub(super) fn albums(&self, section: HomeAlbumSection) -> &[Album] {
        &self.sections[section as usize].albums
    }

    pub(super) fn loading(&self, section: HomeAlbumSection) -> bool {
        self.sections[section as usize].load.loading()
    }

    pub(super) fn begin(
        &mut self,
        section: HomeAlbumSection,
        requests: &mut LibraryRequests,
    ) -> LibraryRequest {
        self.sections[section as usize].load.begin(requests)
    }

    pub(super) fn complete(
        &mut self,
        section: HomeAlbumSection,
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    ) -> Option<Result<&[Album], &str>> {
        let section = &mut self.sections[section as usize];
        Some(match section.load.complete(request, result)? {
            Ok(albums) => {
                section.albums = albums;
                Ok(&section.albums)
            }
            Err(error) => Err(error),
        })
    }
}

impl NavidromeApp {
    pub(super) fn load_home_albums(&mut self, section: HomeAlbumSection) {
        let Some(api) = self.api.clone() else { return };
        let request = self
            .state
            .home_albums
            .begin(section, &mut self.library_requests);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = match section {
                HomeAlbumSection::Recent => api.albums_recent(30).await,
                HomeAlbumSection::Frequent => api.albums_frequent(30).await,
                HomeAlbumSection::Newest => api.albums_newest(30).await,
                HomeAlbumSection::Random => api.albums_random(30).await,
            }
            .map_err(error_message);
            let _ = tx.send(Msg::HomeAlbums {
                section,
                request,
                result,
            });
        });
    }

    pub(super) fn finish_home_albums(
        &mut self,
        section: HomeAlbumSection,
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    ) {
        let Some(result) = self.state.home_albums.complete(section, request, result) else {
            return;
        };
        match result {
            Ok(albums) => {
                let (featured, covers) = home_album_covers(section, albums);
                if section == HomeAlbumSection::Recent {
                    self.ensure_cover(featured.as_deref(), true);
                }
                self.preload_covers(covers);
            }
            // These Home enhancements preserve existing data on failure.
            Err(error) => log::warn!("{} albums load failed: {error}", section.key()),
        }
    }
}

fn home_album_covers(section: HomeAlbumSection, albums: &[Album]) -> (Option<String>, Vec<String>) {
    let featured = (section == HomeAlbumSection::Recent)
        .then(|| albums.first().and_then(|album| album.cover_art.clone()))
        .flatten();
    let covers = albums
        .iter()
        .filter_map(|album| album.cover_art.clone())
        .skip(usize::from(featured.is_some()))
        .take(30)
        .collect();
    (featured, covers)
}

#[cfg(test)]
mod tests;
