use super::*;
use crate::audio::eq::new_eq_handle;
use crate::audio::gapless::tests::track;
use crate::audio::gapless::GaplessSource;
use crate::audio::preload::tests::{client, wait_for, wait_ready, wav, TestCache};
use crate::audio::stream_cache::cache_paths;
use crate::models::EqSettings;
use rodio::Sample;
use std::fs;

fn channels() -> ChannelCount {
    ChannelCount::new(2).unwrap()
}

fn rate() -> SampleRate {
    SampleRate::new(48_000).unwrap()
}

fn request(id: u64, key: &str, gain: f32) -> PreloadRequest {
    PreloadRequest {
        id,
        url: format!("http://127.0.0.1:1/{key}"),
        cache_key: key.into(),
        duration: Some(Duration::from_secs(12)),
        gain,
        transition: Transition::new(id, Some(Duration::from_secs(12))),
    }
}

struct Fixture {
    next: NextTrack,
    cache: TestCache,
    eq: EqHandle,
}

impl Fixture {
    fn new() -> Self {
        let cache = TestCache::new();
        fs::write(cache_paths(&cache.0, "song:128").0, wav()).unwrap();
        Self {
            next: NextTrack::new(client(), Slot::default()),
            cache,
            eq: new_eq_handle(EqSettings::default()),
        }
    }

    fn poll(&mut self, can_preload: bool) {
        self.next
            .poll(can_preload, &self.cache.0, channels(), rate(), &self.eq);
    }

    fn arm(&mut self) -> Arc<Transition> {
        let request = request(7, "song:128", 2.0);
        let transition = request.transition.clone();
        self.next.reserve(request);
        self.poll(true);
        wait_for(|| {
            self.poll(true);
            self.next.handoff.lock().unwrap().is_some()
        });
        transition
    }
}

fn collect(track: OutputTrack) -> Vec<Sample> {
    track.info.control.promote();
    wait_for(|| track.info.control.fully_decoded());
    GaplessSource::new(track, Slot::default(), channels(), rate()).collect()
}

#[test]
fn deferred_reservations_prepare_only_the_latest_occurrence_and_gain() {
    let mut fixture = Fixture::new();
    for id in 1..=20 {
        // Earlier requests have no cached source; only the last can succeed.
        let key = if id == 20 { "song:128" } else { "unavailable" };
        fixture.next.reserve(request(id, key, 2.0));
        fixture.poll(false);
        assert!(fixture.next.active.is_none());
        assert!(fixture.next.handoff.lock().unwrap().is_none());
    }
    fixture.poll(true);
    wait_for(|| {
        fixture.poll(true);
        fixture.next.handoff.lock().unwrap().is_some()
    });
    let queued = fixture.next.handoff.lock().unwrap().take().unwrap();
    assert_eq!(queued.transition.id, 20);
    let samples = collect(queued.track);
    assert_eq!(samples.len(), 12 * 48_000 * 2);
    assert!(samples.iter().all(|sample| (*sample - 0.5).abs() < 0.001));
}

#[test]
fn cancelling_before_eligibility_discards_the_deferred_request() {
    let mut fixture = Fixture::new();
    fixture.next.reserve(request(7, "song:128", 1.0));
    fixture.poll(false);
    fixture.next.cancel();
    fixture.poll(true);
    assert!(fixture.next.pending.is_none());
    assert!(fixture.next.active.is_none());
    assert!(fixture
        .next
        .take_prepared(Some(7), "song:128", Duration::ZERO)
        .is_none());
    assert!(fixture.next.handoff.lock().unwrap().is_none());
}

#[test]
fn frozen_transition_never_starts_deferred_preparation() {
    let mut fixture = Fixture::new();
    let request = request(7, "song:128", 1.0);
    request.transition.cancel();
    fixture.next.reserve(request);
    fixture.poll(true);
    assert!(fixture.next.active.is_none());
    assert!(fixture
        .next
        .take_prepared(Some(7), "song:128", Duration::ZERO)
        .is_none());
    assert!(fixture.next.handoff.lock().unwrap().is_none());
}

#[test]
fn ready_stream_arms_after_pause_and_keeps_its_decoder_asleep() {
    let mut fixture = Fixture::new();
    fixture.next.reserve(request(7, "song:128", 1.0));
    fixture.poll(true);
    wait_ready(fixture.next.preloader.as_mut().unwrap());
    fixture.poll(false);
    let control = fixture.next.armed.as_ref().unwrap().info.control.clone();
    assert!(!control.fully_decoded());
    assert!(control.check().is_ok());
    let queued = fixture
        .next
        .begin_play(Some(7), "song:128", Duration::ZERO)
        .unwrap();
    assert_eq!(collect(queued).len(), 12 * 48_000 * 2);
}

#[test]
fn cancellation_releases_armed_audio_and_wakes_the_sleeping_decoder() {
    let mut fixture = Fixture::new();
    fixture.arm();
    let control = fixture.next.armed.as_ref().unwrap().info.control.clone();
    fixture.next.cancel();
    assert!(control.check().is_err());
    assert!(fixture.next.handoff.lock().unwrap().is_none());
    assert!(fixture.next.take_started().is_none());
    fixture.poll(true);
    assert!(fixture.next.handoff.lock().unwrap().is_none());
    wait_for(|| Arc::strong_count(&control) == 1);
}

#[test]
fn frozen_armed_reservation_can_be_reused_once_by_matching_manual_play() {
    let mut fixture = Fixture::new();
    let transition = fixture.arm();
    transition.cancel();
    let queued = fixture
        .next
        .begin_play(Some(7), "song:128", Duration::ZERO)
        .unwrap();
    assert!(fixture
        .next
        .begin_play(Some(7), "song:128", Duration::ZERO)
        .is_none());
    let samples = collect(queued);
    assert_eq!(samples.len(), 12 * 48_000 * 2);
    assert!(samples.iter().all(|sample| (*sample - 0.5).abs() < 0.001));
    assert!(transition.event().is_none());
    assert!(fixture.next.take_started().is_none());
}

#[test]
fn different_occurrence_quality_or_raw_position_discards_armed_audio() {
    for (id, key, position) in [
        (None, "song:128", Duration::ZERO),
        (Some(8), "song:128", Duration::ZERO),
        (Some(7), "song:320", Duration::ZERO),
        (Some(7), "song:128", Duration::from_nanos(1)),
    ] {
        let mut fixture = Fixture::new();
        fixture.arm();
        let control = fixture.next.armed.as_ref().unwrap().info.control.clone();
        assert!(fixture.next.begin_play(id, key, position).is_none());
        assert!(control.check().is_err());
        assert!(fixture.next.handoff.lock().unwrap().is_none());
        assert!(fixture.next.take_started().is_none());
        wait_for(|| Arc::strong_count(&control) == 1);
    }
}

#[test]
fn ready_but_unarmed_stream_survives_freeze_and_manual_play_detachment() {
    let mut fixture = Fixture::new();
    let request = request(7, "song:128", 1.0);
    let transition = request.transition.clone();
    fixture.next.reserve(request);
    fixture.poll(true);
    wait_ready(fixture.next.preloader.as_mut().unwrap());
    transition.cancel();
    fixture.poll(false);
    assert!(fixture.next.handoff.lock().unwrap().is_none());
    assert!(fixture
        .next
        .begin_play(Some(7), "song:128", Duration::ZERO)
        .is_none());
    let stream = fixture
        .next
        .take_prepared(Some(7), "song:128", Duration::ZERO)
        .unwrap();
    wait_for(|| stream.source.control().fully_decoded());
    assert_eq!(stream.source.count(), 12 * 48_000 * 2);
    assert!(fixture
        .next
        .take_prepared(Some(7), "song:128", Duration::ZERO)
        .is_none());
    fixture.poll(true);
    assert!(fixture.next.handoff.lock().unwrap().is_none());
    assert!(transition.event().is_none());
}

#[test]
fn committed_boundary_reports_metadata_once_and_does_not_rearm() {
    let mut fixture = Fixture::new();
    let transition = fixture.arm();
    let mut output = GaplessSource::new(
        track(4800, 48_000, 2, 0.25, 1.0),
        fixture.next.handoff.clone(),
        channels(),
        rate(),
    );
    assert_eq!(
        output.by_ref().take(9600).collect::<Vec<_>>(),
        vec![0.25; 9600]
    );
    assert!(fixture.next.take_started().is_none());
    assert_eq!(output.next(), Some(0.5));
    let (next, event) = fixture.next.take_started().unwrap();
    assert_eq!(event.id, 7);
    assert_eq!(event.boundary, Duration::from_millis(100));
    assert_eq!(event.loudness_peak, 0.25);
    assert!((event.loudness_db - 20.0 * 0.25_f32.log10()).abs() < 0.01);
    assert_eq!(next.url, "http://127.0.0.1:1/song:128");
    assert_eq!(next.cache_key, "song:128");
    assert_eq!(next.gain, 2.0);
    assert_eq!(next.info.duration, Some(Duration::from_secs(12)));
    assert!(fixture.next.take_started().is_none());
    // Reading worker metadata must leave the frontend's event available.
    assert_eq!(transition.take_event().unwrap().id, 7);
    assert!(transition.take_event().is_none());
    fixture.poll(true);
    assert!(fixture.next.handoff.lock().unwrap().is_none());
    wait_for(|| next.info.control.fully_decoded());
    assert_eq!(output.count(), 12 * 48_000 * 2 - 1);
}

#[test]
fn dropping_coordinator_clears_handoff_before_output_can_cross_boundary() {
    let mut fixture = Fixture::new();
    let transition = fixture.arm();
    let control = fixture.next.armed.as_ref().unwrap().info.control.clone();
    let mut output = GaplessSource::new(
        track(1, 48_000, 2, 0.25, 1.0),
        fixture.next.handoff.clone(),
        channels(),
        rate(),
    );
    drop(fixture.next);
    assert!(control.check().is_err());
    assert_eq!(output.by_ref().collect::<Vec<_>>(), vec![0.25; 2]);
    assert!(transition.event().is_none());
    wait_for(|| Arc::strong_count(&control) == 1);
}

#[test]
fn unavailable_preloader_leaves_ordinary_playback_fallback_available() {
    let cache = TestCache::new();
    let slot = Slot::default();
    let mut next = NextTrack::with_preloader(None, slot.clone());
    next.reserve(request(7, "song:128", 1.0));
    next.poll(
        true,
        &cache.0,
        channels(),
        rate(),
        &new_eq_handle(EqSettings::default()),
    );
    assert!(slot.lock().unwrap().is_none());
    assert!(next
        .begin_play(Some(7), "song:128", Duration::ZERO)
        .is_none());
    assert!(next
        .take_prepared(Some(7), "song:128", Duration::ZERO)
        .is_none());
    assert!(next.take_started().is_none());
    next.cancel();
}
