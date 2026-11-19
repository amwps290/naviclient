use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

use super::{Pending, VinylCover};

struct SnapshotProbe {
    cover: VinylCover,
    outgoing: bool,
}

impl gpui_kit::Render for SnapshotProbe {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{div, px, ParentElement, Styled};
        div()
            .flex()
            .size(px(60.0))
            .children(
                self.outgoing
                    .then(|| self.cover.render_frozen(30.0))
                    .flatten(),
            )
            .children(self.cover.render(30.0, self.cover.phase()))
    }
}

#[gpui_pre::test]
fn outgoing_snapshot_retains_only_visible_textures_and_releases_them_after_removal(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, Image, ImageFormat, VisualTestContext};

    cx.update(gpui_kit::init);
    let first = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| {
                let mut cover = VinylCover::default();
                cover.source = Some(first.clone());
                let now = Instant::now();
                cover.tick(now, true);
                cover.tick(now + Duration::from_millis(50), true);
                SnapshotProbe {
                    cover,
                    outgoing: false,
                }
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let source = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../assets/default-cover.png").to_vec(),
    ));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
        let initial = probe
            .read(cx)
            .cover
            .composite
            .borrow()
            .displayed
            .clone()
            .unwrap();
        assert!(window.has_image_atlas_entry(&initial));
        // The unrotated artwork stays on the CPU; only its sampled texture uploads.
        assert!(!window.has_image_atlas_entry(&first));
        let _ = window.draw(cx);
        assert!(Arc::ptr_eq(
            &initial,
            probe
                .read(cx)
                .cover
                .composite
                .borrow()
                .displayed
                .as_ref()
                .unwrap(),
        ));
        probe.update(cx, |probe, cx| {
            let now = Instant::now();
            probe.cover.tick(now, false);
            probe.cover.tick(now, true);
            probe.cover.tick(now + Duration::from_millis(50), true);
            cx.notify();
        });
        let _ = window.draw(cx);
        let visible = probe
            .read(cx)
            .cover
            .composite
            .borrow()
            .displayed
            .clone()
            .unwrap();
        assert!(!window.has_image_atlas_entry(&initial));
        assert!(window.has_image_atlas_entry(&visible));
        probe.update(cx, |probe, cx| {
            probe.cover.freeze(window);
            let phase = probe.cover.frozen.as_ref().unwrap().phase;
            assert!(phase > 0.0 && phase < 1.0);
            probe.cover.prepare(source.clone(), false, &runtime, window);
            assert!(probe.cover.source.is_none());
            assert_eq!(probe.cover.frozen.as_ref().unwrap().phase, phase);
            probe.outgoing = true;
            // Simulate delivery of the incoming track while the old record is
            // still rendered. Its texture must not evict the outgoing one.
            probe.cover.source = Some(Arc::new(RenderImage::new(vec![Frame::new(
                RgbaImage::new(4, 4),
            )])));
            cx.notify();
        });
        assert!(window.has_image_atlas_entry(&visible));
        let _ = window.draw(cx);
        let incoming = probe
            .read(cx)
            .cover
            .composite
            .borrow()
            .displayed
            .clone()
            .unwrap();
        assert!(!Arc::ptr_eq(&visible, &incoming));
        assert!(window.has_image_atlas_entry(&visible));
        assert!(window.has_image_atlas_entry(&incoming));
        probe.update(cx, |probe, cx| {
            probe.cover.release_frozen(window);
            probe.outgoing = false;
            cx.notify();
        });
        assert!(!window.has_image_atlas_entry(&visible));
        assert!(window.has_image_atlas_entry(&incoming));
        let _ = window.draw(cx);
        assert!(Arc::ptr_eq(
            &incoming,
            probe
                .read(cx)
                .cover
                .composite
                .borrow()
                .displayed
                .as_ref()
                .unwrap(),
        ));
        probe.update(cx, |probe, _| {
            probe.cover.prepare(
                Arc::new(Image::from_bytes(ImageFormat::Png, vec![])),
                false,
                &runtime,
                window,
            );
        });
        assert!(!window.has_image_atlas_entry(&incoming));
    });
}

#[test]
fn decode_failure_stops_the_job_without_advancing_rotation() {
    let mut cover = VinylCover::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    tx.send(Err(anyhow::anyhow!("artwork decoding failed")))
        .unwrap();

    assert!(cover.poll_ready());
    assert!(cover.source.is_none());
    assert!(cover.failed);
    assert!(cover.pending.is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
    let start = Instant::now();
    cover.tick(start, true);
    assert!(!cover.tick(start + Duration::from_millis(50), true));
    assert_eq!(cover.phase(), 0.0);
}

#[test]
fn completed_job_retains_one_source_without_marking_failure() {
    let mut cover = VinylCover::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok(frame.clone())).unwrap();
    drop(tx);

    assert!(cover.poll_ready());
    assert!(Arc::ptr_eq(cover.source.as_ref().unwrap(), &frame));
    assert!(!cover.failed);
    assert!(cover.pending.is_none());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
}

#[test]
fn cancelling_job_discards_queued_source_and_preserves_visible_rotation() {
    let mut cover = VinylCover::default();
    let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    cover.source = Some(frame.clone());
    let start = Instant::now();
    cover.tick(start, true);
    assert!(cover.tick(start + Duration::from_millis(50), true));
    let phase = cover.phase();

    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    cover.pending = Some(Pending {
        cancel: cancel.clone(),
        result,
    });
    let queued = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
    tx.send(Ok(queued.clone())).unwrap();
    cover.cancel_pending();

    assert!(cancel.load(Ordering::Relaxed));
    assert!(!cover.poll_ready());
    assert_eq!(Arc::strong_count(&queued), 1);
    assert!(Arc::ptr_eq(cover.source.as_ref().unwrap(), &frame));
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
    tx.send(Ok(frame.clone())).unwrap();
    let producer = std::thread::spawn(move || tx.send(Ok(frame)));
    drop(pending);
    assert!(cancel.load(Ordering::Relaxed));
    assert!(producer.join().unwrap().is_err());
}

#[test]
fn starts_when_artwork_is_ready_and_never_catches_up_after_waiting() {
    let mut cover = VinylCover::default();
    let start = Instant::now();
    assert!(!cover.tick(start, true));
    assert!(!cover.tick(start + Duration::from_secs(5), true));
    assert_eq!(cover.phase(), 0.0);

    cover.source = Some(Arc::new(RenderImage::new(vec![Frame::new(
        RgbaImage::new(2, 2),
    )])));
    let ready = start + Duration::from_millis(5_050);
    assert!(cover.tick(ready, true));
    assert!((cover.phase() - 0.05 / 18.0).abs() < 0.00001);

    let phase = cover.phase();
    assert!(!cover.tick(ready, false));
    assert!(!cover.tick(ready + Duration::from_secs(10), true));
    assert_eq!(cover.phase(), phase);
    assert!(cover.tick(ready + Duration::from_millis(10_040), true));
    // Rotation continues through former 3-degree cache boundaries without
    // waiting for another prepared frame.
    let phase = cover.phase();
    assert!(cover.tick(ready + Duration::from_millis(10_140), true));
    assert!((cover.phase() - phase - 0.1 / 18.0).abs() < 0.00001);
}
