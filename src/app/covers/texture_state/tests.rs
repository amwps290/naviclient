use super::*;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::ImageFormat;

fn image() -> Arc<Image> {
    static NEXT_IMAGE: AtomicU64 = AtomicU64::new(0);
    let mut bytes = include_bytes!("../../../../assets/default-cover.png").to_vec();
    // GPUI hashes image bytes, so separate handles with identical bytes share an ID.
    // A unique PNG trailer creates distinct test images while keeping valid bytes.
    bytes.extend_from_slice(&NEXT_IMAGE.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes))
}

fn textures(capacity: usize) -> CoverTextures {
    CoverTextures::new(image().id(), capacity)
}

fn ids(images: &[Arc<Image>]) -> HashSet<u64> {
    images.iter().map(|image| image.id()).collect()
}

#[test]
fn usage_refreshes_retention_and_evicts_the_oldest_image() {
    let mut textures = textures(3);
    let covers = [image(), image(), image(), image()];
    for cover in &covers[..3] {
        textures.mark_used(cover);
    }
    assert!(textures.take_releases().is_empty());
    textures.mark_used(&covers[0]);
    textures.mark_used(&covers[3]);
    assert_eq!(
        ids(&textures.take_releases()),
        HashSet::from([covers[1].id()])
    );
    assert_eq!(textures.used.len(), 3);
    assert!(textures.take_releases().is_empty());
}

#[test]
fn reusing_an_evicted_cover_cancels_its_pending_release() {
    let mut textures = textures(2);
    let covers = [image(), image(), image()];
    for cover in &covers {
        textures.mark_used(cover);
    }
    textures.mark_used(&covers[0]);
    assert_eq!(
        ids(&textures.take_releases()),
        HashSet::from([covers[1].id()])
    );
    assert_eq!(
        ids(&textures.used.iter().cloned().collect::<Vec<_>>()),
        HashSet::from([covers[0].id(), covers[2].id()])
    );
}

#[test]
fn repeated_usage_does_not_consume_more_retention_slots() {
    let mut textures = textures(2);
    let first = image();
    let second = image();
    for _ in 0..10 {
        textures.mark_used(&first);
    }
    textures.mark_used(&second);
    assert_eq!(textures.used.len(), 2);
    assert!(textures.take_releases().is_empty());
}

#[test]
fn the_default_cover_never_uses_a_slot_or_enters_a_release_batch() {
    let default = image();
    let mut textures = CoverTextures::new(default.id(), 1);
    let cover = image();
    textures.mark_used(&cover);
    textures.mark_used(&default);
    textures.retire(default.clone());
    assert!(textures.take_releases().is_empty());
    textures.reset([default.clone(), cover.clone()]);
    assert_eq!(ids(&textures.take_releases()), HashSet::from([cover.id()]));
    assert!(textures.used.is_empty());
}

#[test]
fn retiring_a_replaced_image_frees_its_slot_before_the_successor_is_used() {
    for replaced_index in [0, 1] {
        let mut textures = textures(2);
        let covers = [image(), image(), image()];
        textures.mark_used(&covers[0]);
        textures.mark_used(&covers[1]);
        textures.retire(covers[replaced_index].clone());
        textures.mark_used(&covers[2]);
        assert_eq!(
            ids(&textures.take_releases()),
            HashSet::from([covers[replaced_index].id()])
        );
        assert_eq!(textures.used.len(), 2);
    }
}

#[test]
fn evicted_handles_remain_alive_without_a_source_cache_until_the_batch_is_dropped() {
    let mut textures = textures(1);
    let old = image();
    let old_id = old.id();
    let weak = Arc::downgrade(&old);
    textures.mark_used(&old);
    drop(old);
    textures.mark_used(&image());
    assert!(weak.upgrade().is_some());
    let releases = textures.take_releases();
    assert_eq!(ids(&releases), HashSet::from([old_id]));
    assert!(weak.upgrade().is_some());
    assert!(textures.take_releases().is_empty());
    drop(releases);
    assert!(weak.upgrade().is_none());
}

#[test]
fn retirement_holds_a_replaced_source_until_the_window_can_release_it() {
    let mut textures = textures(1);
    let retired = image();
    let weak = Arc::downgrade(&retired);
    textures.retire(retired);
    assert!(weak.upgrade().is_some());
    let releases = textures.take_releases();
    assert_eq!(releases.len(), 1);
    assert!(weak.upgrade().is_some());
    drop(releases);
    assert!(weak.upgrade().is_none());
}

#[test]
fn retiring_an_evicted_source_and_repeating_retirement_releases_it_once() {
    let mut textures = textures(1);
    let covers = [image(), image(), image()];
    textures.mark_used(&covers[0]);
    textures.mark_used(&covers[1]);
    textures.retire(covers[0].clone());
    textures.retire(covers[0].clone());
    textures.mark_used(&covers[0]);
    textures.mark_used(&covers[2]);
    let releases = textures.take_releases();
    assert_eq!(releases.len(), 2);
    assert_eq!(
        ids(&releases),
        HashSet::from([covers[0].id(), covers[1].id()])
    );
    assert!(textures.take_releases().is_empty());
}

#[test]
fn a_stale_view_cannot_displace_the_current_cover_with_a_retired_source() {
    let mut textures = textures(1);
    let retired = image();
    let current = image();
    textures.mark_used(&retired);
    textures.retire(retired.clone());
    textures.mark_used(&current);
    textures.mark_used(&retired);
    assert_eq!(
        ids(&textures.take_releases()),
        HashSet::from([retired.id()])
    );
    assert_eq!(textures.used.front().unwrap().id(), current.id());
    // Source bytes can be decoded again after release and tracked normally.
    textures.mark_used(&retired);
    assert_eq!(
        ids(&textures.take_releases()),
        HashSet::from([current.id()])
    );
    assert_eq!(textures.used.front().unwrap().id(), retired.id());
}

#[test]
fn server_reset_preserves_all_pending_handles_and_keeps_new_server_images() {
    let mut textures = textures(1);
    let covers = [image(), image(), image(), image(), image()];
    textures.mark_used(&covers[0]);
    textures.mark_used(&covers[1]);
    textures.retire(covers[2].clone());
    textures.reset([covers[0].clone(), covers[1].clone(), covers[3].clone()]);
    textures.mark_used(&covers[4]);
    let releases = textures.take_releases();
    assert_eq!(releases.len(), 4);
    assert_eq!(ids(&releases), ids(&covers[..4]));
    assert_eq!(textures.used.front().unwrap().id(), covers[4].id());
    assert!(textures.take_releases().is_empty());
}

#[test]
fn consecutive_server_resets_accumulate_releases_without_duplicates_or_lost_handles() {
    let mut textures = textures(2);
    let covers = [image(), image(), image()];
    textures.mark_used(&covers[0]);
    textures.reset([covers[0].clone()]);
    textures.mark_used(&covers[1]);
    textures.reset([covers[0].clone(), covers[1].clone(), covers[2].clone()]);
    let releases = textures.take_releases();
    assert_eq!(releases.len(), 3);
    assert_eq!(ids(&releases), ids(&covers));
    assert!(textures.used.is_empty());
    assert!(textures.take_releases().is_empty());
}

#[test]
fn distinct_handles_with_identical_content_share_one_retention_slot_and_release() {
    let mut textures = textures(1);
    let first = image();
    let same_content = Arc::new(Image::from_bytes(ImageFormat::Png, first.bytes.clone()));
    assert_eq!(first.id(), same_content.id());
    assert!(!Arc::ptr_eq(&first, &same_content));
    textures.mark_used(&first);
    textures.mark_used(&same_content);
    assert_eq!(textures.used.len(), 1);
    assert!(textures.take_releases().is_empty());
    textures.reset([first, same_content]);
    assert_eq!(textures.take_releases().len(), 1);
    assert!(textures.take_releases().is_empty());
}
