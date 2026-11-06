use std::panic::catch_unwind;
use std::sync::{Arc, Barrier};
use std::thread;

use rodio::{buffer::SamplesBuffer, ChannelCount, SampleRate};

use super::new_eq_handle;
use crate::audio::eq::Equalizer;
use crate::models::EqSettings;

#[test]
fn clones_publish_coherent_settings_and_revisions_to_other_threads() {
    let handle = new_eq_handle(EqSettings::default());
    let publisher = handle.clone();
    assert!(handle.shares_state(&publisher));
    assert!(!handle.shares_state(&new_eq_handle(EqSettings::default())));
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = barrier.clone();
    let worker = thread::spawn(move || {
        worker_barrier.wait();
        for version in 1..=200 {
            publisher.set(EqSettings {
                enabled: version % 2 == 0,
                gains_db: [version as f32; 10],
            });
        }
    });
    barrier.wait();
    for _ in 0..500 {
        let (version, settings) = handle.snapshot();
        assert_eq!(settings.gains_db, [version as f32; 10]);
        assert_eq!(settings.enabled, version != 0 && version % 2 == 0);
    }
    worker.join().unwrap();
    assert_eq!(handle.snapshot().0, 200);
}

#[test]
fn unchanged_settings_publish_revisions_and_wrapping_still_refreshes() {
    let handle = new_eq_handle(EqSettings::default());
    assert!(handle.changed_since(0).is_none());
    handle.set(EqSettings::default());
    handle.set(EqSettings::default());
    assert_eq!(handle.changed_since(0), Some((2, EqSettings::default())));
    assert!(handle.changed_since(2).is_none());

    handle.set_version_for_test(u64::MAX);
    handle.set(EqSettings::default());
    assert_eq!(
        handle.changed_since(u64::MAX),
        Some((0, EqSettings::default()))
    );
    assert!(handle.changed_since(0).is_none());
}

#[test]
fn owned_snapshot_survives_later_edits_and_filters_read_the_latest_revision() {
    let handle = new_eq_handle(EqSettings::default());
    let snapshot = handle.snapshot();
    for gain in [3.0, -4.0, 7.0] {
        handle.set(EqSettings {
            enabled: true,
            gains_db: [gain; 10],
        });
    }
    assert_eq!(snapshot, (0, EqSettings::default()));
    assert_eq!(
        handle.changed_since(snapshot.0),
        Some((
            3,
            EqSettings {
                enabled: true,
                gains_db: [7.0; 10],
            }
        ))
    );
}

#[test]
fn poisoned_state_keeps_existing_audio_and_ignores_new_publications() {
    let handle = new_eq_handle(EqSettings::default());
    let source = || {
        SamplesBuffer::new(
            ChannelCount::new(2).unwrap(),
            SampleRate::new(48_000).unwrap(),
            vec![0.25; 8192],
        )
    };
    let mut active = Equalizer::new(source(), handle.clone());
    assert_eq!(active.next(), Some(0.25));
    assert!(catch_unwind(|| {
        let _guard = handle.inner.lock().unwrap();
        panic!("injected shared-state failure");
    })
    .is_err());
    handle.set(EqSettings {
        enabled: true,
        gains_db: [12.0; 10],
    });
    assert!(handle.changed_since(0).is_none());
    assert!(active.all(|sample| sample == 0.25));
    {
        let recovered = handle
            .inner
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(recovered.version, 0);
        assert_eq!(recovered.settings, EqSettings::default());
    }
    assert!(catch_unwind(|| handle.snapshot()).is_err());
    assert!(catch_unwind(|| Equalizer::new(source(), handle.clone())).is_err());
}
