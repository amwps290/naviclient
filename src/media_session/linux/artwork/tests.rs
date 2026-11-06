use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use gpui_kit::{Image, ImageFormat};

use super::{write_cover, Artwork};

type Publication = Arc<RwLock<Option<(u64, String)>>>;

fn cover() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../../assets/default-cover.png").to_vec(),
    ))
}

fn other_cover() -> Arc<Image> {
    let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.into_inner()))
}

fn artwork() -> (Artwork, Publication) {
    let published = Arc::new(RwLock::new(None));
    (Artwork::new(published.clone()), published)
}

fn published_path(published: &Publication) -> PathBuf {
    let guard = published.read().unwrap_or_else(|error| error.into_inner());
    let url = url::Url::parse(&guard.as_ref().unwrap().1).unwrap();
    assert_eq!(url.scheme(), "file");
    assert!(url.query().is_none());
    url.to_file_path().unwrap()
}

#[tokio::test]
async fn publication_preserves_cover_bytes_and_uses_the_owned_directory() {
    let (mut artwork, published) = artwork();
    let image = cover();
    let directory = artwork.directory.as_ref().unwrap().path().to_owned();
    artwork.update(Some(image.clone())).await.unwrap();

    let path = published_path(&published);
    assert_eq!(path.parent(), Some(directory.as_path()));
    assert_eq!(
        path.file_name().unwrap(),
        format!("{:016x}.png", image.id).as_str()
    );
    assert_eq!(fs::read(&path).unwrap(), image.bytes);
    assert_eq!(published.read().unwrap().as_ref().unwrap().0, image.id);
    assert_eq!(artwork.current_file.as_ref(), Some(&path));
}

#[tokio::test]
async fn reusing_same_cover_does_not_rewrite_cached_bytes() {
    let (mut artwork, published) = artwork();
    let image = cover();
    artwork.update(Some(image.clone())).await.unwrap();
    let path = published_path(&published);
    // A marker detects another write without relying on filesystem clock precision.
    fs::write(&path, b"reuse marker").unwrap();
    let same_bytes = cover();
    assert_eq!(image.id, same_bytes.id);
    artwork.update(Some(same_bytes)).await.unwrap();
    assert_eq!(published_path(&published), path);
    assert_eq!(fs::read(path).unwrap(), b"reuse marker");
}

#[tokio::test]
async fn replacing_and_clearing_cover_retains_only_the_current_file() {
    let (mut artwork, published) = artwork();
    let directory = artwork.directory.as_ref().unwrap().path().to_owned();
    artwork.update(Some(cover())).await.unwrap();
    let first = published_path(&published);
    artwork.update(Some(other_cover())).await.unwrap();
    let second = published_path(&published);

    assert_ne!(first, second);
    assert!(!first.exists());
    assert!(second.exists());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    artwork.update(None).await.unwrap();
    assert!(published.read().unwrap().is_none());
    assert!(artwork.current_file.is_none());
    assert!(!second.exists());
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
}

#[tokio::test]
async fn invalid_replacement_removes_old_advertisement_and_can_retry() {
    let (mut artwork, published) = artwork();
    let valid = cover();
    artwork.update(Some(valid.clone())).await.unwrap();
    let first = published_path(&published);
    let invalid = Arc::new(Image::from_bytes(ImageFormat::Png, vec![]));
    artwork.update(Some(invalid)).await.unwrap();
    assert!(published.read().unwrap().is_none());
    assert!(artwork.current_file.is_none());
    assert!(!first.exists());

    artwork.update(Some(valid.clone())).await.unwrap();
    assert_eq!(published_path(&published), first);
    assert_eq!(fs::read(first).unwrap(), valid.bytes);
}

#[tokio::test]
async fn write_failure_clears_old_cover_and_later_retry_can_publish() {
    let (mut artwork, published) = artwork();
    artwork.update(Some(cover())).await.unwrap();
    let first = published_path(&published);
    let next = other_cover();
    let blocked = artwork
        .directory
        .as_ref()
        .unwrap()
        .path()
        .join(format!("{:016x}.png", next.id));
    fs::create_dir(&blocked).unwrap();

    artwork.update(Some(next.clone())).await.unwrap();
    assert!(published.read().unwrap().is_none());
    assert!(artwork.current_file.is_none());
    assert!(!first.exists());
    fs::remove_dir(&blocked).unwrap();
    artwork.update(Some(next.clone())).await.unwrap();
    assert_eq!(published_path(&published), blocked);
    assert_eq!(fs::read(blocked).unwrap(), next.bytes);
}

#[tokio::test]
async fn unavailable_directory_skips_artwork_and_scope_drop_cleans_active_files() {
    let (mut unavailable, published) = artwork();
    unavailable.directory = None;
    unavailable.update(Some(cover())).await.unwrap();
    assert!(published.read().unwrap().is_none());
    assert!(unavailable.current_file.is_none());

    let (mut active, published) = artwork();
    let directory = active.directory.as_ref().unwrap().path().to_owned();
    active.update(Some(cover())).await.unwrap();
    let path = published_path(&published);
    assert!(path.exists());
    drop(active);
    assert!(!path.exists());
    assert!(!directory.exists());
}

#[test]
fn file_urls_round_trip_paths_with_spaces_unicode_and_reserved_characters() {
    let directory = tempfile::Builder::new()
        .prefix("navidrome art 一 #")
        .tempdir()
        .unwrap();
    let file = write_cover(directory.path().to_owned(), cover()).unwrap();
    let url = url::Url::parse(&file.url).unwrap();
    assert_eq!(url.scheme(), "file");
    assert!(url.as_str().contains("%20"));
    assert!(url.as_str().contains("%23"));
    assert_eq!(url.to_file_path().unwrap(), file.path);
}
