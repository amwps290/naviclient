use gpui_kit::{hsla, ImageFormat};

use super::*;

fn image() -> Arc<GpuiImage> {
    Arc::new(GpuiImage::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../../assets/default-cover.png").to_vec(),
    ))
}

fn palette() -> (Hsla, Hsla) {
    (hsla(0.15, 0.5, 0.4, 1.0), hsla(0.3, 0.7, 0.6, 1.0))
}

fn decoded(image: Option<Arc<GpuiImage>>, palette: Option<(Hsla, Hsla)>) -> DecodedCover {
    DecodedCover { image, palette }
}

#[test]
fn requests_coalesce_per_id_and_cache_original_handles_and_palette() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    let a = cache.begin("a", true, &mut requests).unwrap();
    assert!(cache.begin("a", false, &mut requests).is_none());
    let b = cache.begin("b", false, &mut requests).unwrap();
    let cover = image();
    let update = cache
        .complete(&a, Ok(decoded(Some(cover.clone()), Some(palette()))))
        .unwrap();
    assert!(update.replaced.is_none());
    assert!(!update.follow_up_palette);
    assert!(Arc::ptr_eq(cache.image("a").unwrap(), &cover));
    assert_eq!(cache.palette("a"), Some(palette()));
    assert!(cache.begin("a", true, &mut requests).is_none());
    assert!(cache
        .complete(&b, Ok(decoded(Some(cover.clone()), None)))
        .is_some());
    assert_eq!(cache.images().count(), 2);
    assert!(cache.images().all(|cached| Arc::ptr_eq(cached, &cover)));
}

#[test]
fn server_reset_rejects_old_success_and_failure_without_consuming_new_request() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let old = state.covers.begin("same-id", true, &mut requests).unwrap();
    state = crate::app::AppState::default();
    let current = state.covers.begin("same-id", true, &mut requests).unwrap();
    assert!(state
        .covers
        .complete(&old, Ok(decoded(Some(image()), Some(palette()))))
        .is_none());
    assert!(state
        .covers
        .complete(&old, Err("old failure".into()))
        .is_none());
    assert!(state.covers.image("same-id").is_none());
    assert!(state.covers.palette("same-id").is_none());
    assert!(state
        .covers
        .complete(&current, Ok(decoded(Some(image()), None)))
        .is_some());
    assert!(state.covers.image("same-id").is_some());
}

#[test]
fn pending_grid_request_remembers_palette_demand_and_duplicates_cannot_consume_follow_up() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    let grid = cache.begin("a", false, &mut requests).unwrap();
    assert!(cache.begin("a", true, &mut requests).is_none());
    assert!(cache.begin("a", false, &mut requests).is_none());
    let cover = image();
    let update = cache
        .complete(&grid, Ok(decoded(Some(cover.clone()), None)))
        .unwrap();
    assert!(update.follow_up_palette);
    let follow_up = cache.begin("a", true, &mut requests).unwrap();
    assert!(cache
        .complete(&grid, Ok(decoded(Some(image()), None)))
        .is_none());
    assert!(cache
        .complete(&grid, Err("duplicate error".into()))
        .is_none());
    let update = cache
        .complete(&follow_up, Ok(decoded(None, Some(palette()))))
        .unwrap();
    assert!(!update.follow_up_palette);
    assert!(update.replaced.is_none());
    assert!(Arc::ptr_eq(cache.image("a").unwrap(), &cover));
    assert_eq!(cache.palette("a"), Some(palette()));
    assert!(!cache.request_needed("a", true));
}

#[test]
fn completed_grid_image_can_later_request_palette() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    let grid = cache.begin("a", false, &mut requests).unwrap();
    let update = cache
        .complete(&grid, Ok(decoded(Some(image()), None)))
        .unwrap();
    assert!(!update.follow_up_palette);
    assert!(!cache.request_needed("a", false));
    assert!(cache.request_needed("a", true));
    let follow_up = cache.begin("a", true, &mut requests).unwrap();
    cache.complete(&follow_up, Ok(decoded(None, Some(palette()))));
    assert!(!cache.request_needed("a", true));
}

#[test]
fn recognized_image_with_failed_palette_extraction_does_not_loop() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    let request = cache.begin("a", true, &mut requests).unwrap();
    // The PNG signature is recognized, but the image is too short to extract color.
    let result = crate::app::covers::artwork::decode_cover(b"\x89PNG\r\n\x1a\n".to_vec(), true);
    assert!(result.image.is_some());
    assert!(result.palette.is_none());
    let update = cache.complete(&request, Ok(result)).unwrap();
    assert!(!update.follow_up_palette);
    assert!(cache.image("a").is_some());
    assert!(!cache.request_needed("a", true));
    assert!(cache.begin("a", true, &mut requests).is_none());
}

#[test]
fn failed_and_empty_decodes_do_not_create_entries_or_retry_repeatedly() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    for (id, result) in [
        ("failed", Err("offline".into())),
        ("empty", Ok(decoded(None, None))),
    ] {
        let request = cache.begin(id, false, &mut requests).unwrap();
        assert!(cache.begin(id, true, &mut requests).is_none());
        let update = cache.complete(&request, result).unwrap();
        assert!(!update.follow_up_palette);
        assert!(cache.image(id).is_none());
        assert!(cache.palette(id).is_none());
        assert!(!cache.request_needed(id, true));
        assert!(cache.begin(id, false, &mut requests).is_none());
        assert!(cache
            .complete(&request, Ok(decoded(Some(image()), Some(palette()))))
            .is_none());
    }
    assert!(cache.entries.is_empty());
    assert_eq!(cache.images().count(), 0);
}

#[test]
fn failure_cache_stays_bounded_and_evicted_ids_can_retry() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    for i in 0..=MAX_FAILED_COVERS {
        let request = cache.begin(&i.to_string(), false, &mut requests).unwrap();
        cache.complete(&request, Err("failed".into()));
        // Duplicate failures must not alter FIFO ordering or consume capacity.
        assert!(cache.complete(&request, Err("duplicate".into())).is_none());
    }
    assert_eq!(cache.failed.len(), MAX_FAILED_COVERS);
    assert_eq!(cache.failed_order.len(), MAX_FAILED_COVERS);
    assert!(cache.entries.is_empty());
    assert!(cache.begin("1", false, &mut requests).is_none());
    let retry = cache.begin("0", false, &mut requests).unwrap();
    assert!(cache
        .complete(&retry, Ok(decoded(Some(image()), None)))
        .is_some());
    assert!(!cache.request_needed("0", false));
}

#[test]
fn replacement_returns_old_handle_but_identical_image_ids_are_not_released() {
    let mut requests = LibraryRequests::default();
    for same_bytes in [true, false] {
        let mut cache = CoverCache::default();
        let grid = cache.begin("a", false, &mut requests).unwrap();
        let original = image();
        cache.complete(&grid, Ok(decoded(Some(original.clone()), None)));
        let mut bytes = original.bytes.clone();
        if !same_bytes {
            // PNG permits trailing bytes; change the content hash while keeping a valid image.
            bytes.push(1);
        }
        let replacement = Arc::new(GpuiImage::from_bytes(ImageFormat::Png, bytes));
        let request = cache.begin("a", true, &mut requests).unwrap();
        let update = cache
            .complete(
                &request,
                Ok(decoded(Some(replacement.clone()), Some(palette()))),
            )
            .unwrap();
        if same_bytes {
            assert!(update.replaced.is_none());
        } else {
            assert!(Arc::ptr_eq(update.replaced.as_ref().unwrap(), &original));
        }
        assert!(Arc::ptr_eq(cache.image("a").unwrap(), &replacement));
    }
}

#[test]
fn palette_without_image_still_requests_image_and_failed_follow_up_preserves_palette() {
    let mut requests = LibraryRequests::default();
    let mut cache = CoverCache::default();
    let request = cache.begin("a", true, &mut requests).unwrap();
    cache.complete(&request, Ok(decoded(None, Some(palette()))));
    assert!(cache.image("a").is_none());
    assert!(cache.request_needed("a", false));
    let follow_up = cache.begin("a", false, &mut requests).unwrap();
    cache.complete(&follow_up, Err("download failed".into()));
    assert_eq!(cache.palette("a"), Some(palette()));
    assert!(!cache.request_needed("a", false));
}
