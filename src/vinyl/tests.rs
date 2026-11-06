use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

use super::{Pending, VinylCover};

#[test]
fn failure_keeps_delivered_frames_and_cancels_the_remaining_job() {
    let mut cover = VinylCover::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(4);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok((0, frame.clone()))).unwrap();
    tx.send(Err(anyhow::anyhow!("frame preparation failed")))
        .unwrap();
    tx.send(Ok((1, frame.clone()))).unwrap();

    assert!(cover.poll_ready());
    assert!(Arc::ptr_eq(cover.frames[0].as_ref().unwrap(), &frame));
    assert!(cover.frames[1].is_none());
    assert!(cover.failed);
    assert!(cover.pending.is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
    assert!(tx.send(Ok((2, frame))).is_err());
}

#[test]
fn completed_job_retains_frames_without_marking_failure() {
    let mut cover = VinylCover::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(4);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok((0, frame.clone()))).unwrap();
    tx.send(Ok((1, frame.clone()))).unwrap();
    drop(tx);

    assert!(cover.poll_ready());
    assert!(Arc::ptr_eq(cover.frames[0].as_ref().unwrap(), &frame));
    assert!(Arc::ptr_eq(cover.frames[1].as_ref().unwrap(), &frame));
    assert!(!cover.failed);
    assert!(cover.pending.is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
}

#[test]
fn cancelling_job_discards_queued_frames_and_preserves_visible_rotation() {
    let mut cover = VinylCover::default();
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    cover.frames[0] = Some(frame.clone());
    cover.frames[1] = Some(frame.clone());
    let start = Instant::now();
    cover.tick(start, true);
    assert!(cover.tick(start + Duration::from_millis(50), true));
    let phase = cover.phase();

    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(4);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    let queued = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok((2, queued.clone()))).unwrap();
    cover.cancel_pending();

    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
    assert!(cover.frames[2].is_none());
    assert_eq!(Arc::strong_count(&queued), 1);
    assert!(Arc::ptr_eq(cover.frames[0].as_ref().unwrap(), &frame));
    assert!(Arc::ptr_eq(cover.frames[1].as_ref().unwrap(), &frame));
    assert_eq!(cover.phase(), phase);
    assert!(!cover.failed);
}

#[test]
fn cancelling_pending_work_unblocks_a_full_channel() {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    let pending = Pending {
        cancel: cancel.clone(),
        result,
    };
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok((0, frame.clone()))).unwrap();
    let producer = std::thread::spawn(move || tx.send(Ok((1, frame))));
    drop(pending);
    assert!(cancel.load(Ordering::Relaxed));
    assert!(producer.join().unwrap().is_err());
}

#[test]
fn starts_with_two_frames_and_never_catches_up_after_waiting() {
    let mut cover = VinylCover::default();
    let start = Instant::now();
    assert!(!cover.tick(start, true));
    assert!(!cover.tick(start + Duration::from_secs(5), true));
    assert_eq!(cover.phase(), 0.0);

    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    cover.frames[0] = Some(frame.clone());
    cover.frames[1] = Some(frame.clone());
    let ready = start + Duration::from_millis(5_050);
    assert!(cover.tick(ready, true));
    assert!((cover.phase() - 0.05 / 18.0).abs() < 0.00001);
    assert!(cover.frames[2].is_none()); // No full-cache requirement.

    let phase = cover.phase();
    assert!(!cover.tick(ready, false));
    assert!(!cover.tick(ready + Duration::from_secs(10), true));
    assert_eq!(cover.phase(), phase);
    assert!(cover.tick(ready + Duration::from_millis(10_040), true));

    let phase = cover.phase();
    assert!(!cover.tick(ready + Duration::from_millis(10_140), true));
    assert_eq!(cover.phase(), phase); // Wait for frame 2, without a jump.
    cover.frames[2] = Some(frame);
    assert!(cover.tick(ready + Duration::from_millis(10_190), true));
    assert!((cover.phase() - phase - 0.05 / 18.0).abs() < 0.00001);
}
