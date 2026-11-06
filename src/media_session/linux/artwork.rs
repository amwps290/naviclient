//! Connection-owned temporary media artwork and publication after file preparation.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use gpui_kit::Image;

pub(super) struct Artwork {
    directory: Option<tempfile::TempDir>,
    current_file: Option<PathBuf>,
    published: Arc<RwLock<Option<(u64, String)>>>,
}

impl Artwork {
    pub(super) fn new(published: Arc<RwLock<Option<(u64, String)>>>) -> Self {
        Self {
            directory: tempfile::Builder::new()
                .prefix("navidrome-media-art-")
                .tempdir()
                .ok(),
            current_file: None,
            published,
        }
    }

    pub(super) async fn update(&mut self, cover: Option<Arc<Image>>) -> anyhow::Result<()> {
        let current_id = self
            .published
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|(id, _)| *id);
        if current_id == cover.as_ref().map(|cover| cover.id) {
            return Ok(());
        }
        let file = if let (Some(directory), Some(cover)) = (&self.directory, cover) {
            let directory = directory.path().to_owned();
            tokio::task::spawn_blocking(move || write_cover(directory, cover))
                .await?
                .map(Some)
                .unwrap_or_else(|error| {
                    log::warn!("media artwork unavailable: {error}");
                    None
                })
        } else {
            None
        };
        // Publish the new local URL before removing the previous image. On a
        // file failure, clear its advertisement and remove the previous image.
        *self
            .published
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            file.as_ref().map(|file| (file.id, file.url.clone()));
        if let Some(path) = self.current_file.take() {
            let _ = std::fs::remove_file(path);
        }
        self.current_file = file.map(|file| file.path);
        Ok(())
    }
}

struct CoverFile {
    id: u64,
    path: PathBuf,
    url: String,
}

fn write_cover(directory: PathBuf, cover: Arc<Image>) -> anyhow::Result<CoverFile> {
    let format = image::guess_format(&cover.bytes)?;
    let extension = format.extensions_str().first().copied().unwrap_or("img");
    let path = directory.join(format!("{:016x}.{extension}", cover.id));
    std::fs::write(&path, &cover.bytes)?;
    let url =
        url::Url::from_file_path(&path).map_err(|_| anyhow::anyhow!("invalid artwork path"))?;
    Ok(CoverFile {
        id: cover.id,
        path,
        url: url.to_string(),
    })
}

#[cfg(test)]
mod tests;
