use super::*;
use crate::media_session;

fn context(position: u64, duration: Option<u64>) -> Option<PlaybackTimeline> {
    Some(PlaybackTimeline {
        position: Duration::from_secs(position),
        duration: duration.map(Duration::from_secs),
    })
}

#[test]
fn disconnected_empty_and_invalid_queue_contexts_have_no_capabilities() {
    for modes in [
        PlayModes::default(),
        PlayModes {
            shuffle: true,
            repeat: RepeatMode::All,
        },
    ] {
        for (connected, index, len) in [
            (false, Some(1), 3),
            (true, None, 3),
            (true, Some(0), 0),
            (true, Some(3), 3),
            (true, Some(usize::MAX), 3),
        ] {
            assert_eq!(
                MediaCapabilities::for_queue(connected, index, len, modes, 5),
                MediaCapabilities::default()
            );
        }
    }
}

#[test]
fn linear_queue_capabilities_follow_the_first_and_last_boundaries() {
    for (index, next, previous) in [(0, true, false), (1, true, true), (2, false, true)] {
        assert_eq!(
            MediaCapabilities::for_queue(true, Some(index), 3, PlayModes::default(), 0),
            MediaCapabilities {
                play: true,
                next,
                previous
            }
        );
    }
    assert_eq!(
        MediaCapabilities::for_queue(true, Some(0), 1, PlayModes::default(), 0),
        MediaCapabilities {
            play: true,
            next: false,
            previous: false
        }
    );
}

#[test]
fn either_repeat_mode_allows_manual_navigation_at_both_ends() {
    for repeat in [RepeatMode::One, RepeatMode::All] {
        for (index, len) in [(0, 1), (0, 3), (2, 3)] {
            assert_eq!(
                MediaCapabilities::for_queue(
                    true,
                    Some(index),
                    len,
                    PlayModes {
                        shuffle: false,
                        repeat
                    },
                    0
                ),
                MediaCapabilities {
                    play: true,
                    next: true,
                    previous: true
                }
            );
        }
    }
}

#[test]
fn shuffle_previous_requires_played_history_and_next_works_for_a_single_song() {
    for repeat in [RepeatMode::Off, RepeatMode::One, RepeatMode::All] {
        for history_len in 0..3 {
            let capabilities = MediaCapabilities::for_queue(
                true,
                Some(0),
                1,
                PlayModes {
                    shuffle: true,
                    repeat,
                },
                history_len,
            );
            assert_eq!(
                capabilities,
                MediaCapabilities {
                    play: true,
                    next: true,
                    previous: history_len > 1
                }
            );
        }
    }
}

#[test]
fn relative_seek_distinguishes_the_exact_end_from_advancing_to_next() {
    assert_eq!(
        relative_seek(context(9, Some(10)), 1_000_000),
        SeekAction::Seek(Duration::from_secs(10))
    );
    assert_eq!(
        relative_seek(context(9, Some(10)), 1_000_001),
        SeekAction::Next
    );
    assert_eq!(
        relative_seek(context(10, Some(10)), 0),
        SeekAction::Seek(Duration::from_secs(10))
    );
    assert_eq!(relative_seek(None, 1_000_000), SeekAction::Ignore);
}

#[test]
fn relative_seek_preserves_microseconds_and_handles_extreme_offsets() {
    assert_eq!(
        relative_seek(context(1, Some(10)), 250_000),
        SeekAction::Seek(Duration::from_millis(1250))
    );
    assert_eq!(
        relative_seek(context(1, Some(10)), i64::MIN),
        SeekAction::Seek(Duration::ZERO)
    );
    assert_eq!(
        relative_seek(context(1, Some(10)), i64::MAX),
        SeekAction::Next
    );
    assert_eq!(
        relative_seek(context(0, None), i64::MAX),
        SeekAction::Seek(Duration::from_micros(i64::MAX as u64))
    );
    assert_eq!(
        relative_seek(
            Some(PlaybackTimeline {
                position: Duration::MAX,
                duration: None
            }),
            1
        ),
        SeekAction::Seek(Duration::MAX)
    );
}

#[test]
fn absolute_seek_rejects_old_song_and_other_server_track_paths() {
    let current = media_session::track_id("https://first.test", "same-song", None);
    for stale in [
        media_session::track_id("https://second.test", "same-song", None),
        media_session::track_id("https://first.test", "other-song", None),
    ] {
        assert_eq!(
            absolute_seek(
                context(1, Some(10)),
                Some(&current),
                &stale,
                Duration::from_secs(5)
            ),
            SeekAction::Ignore
        );
    }
    assert_eq!(
        absolute_seek(None, Some(&current), &current, Duration::ZERO),
        SeekAction::Ignore
    );
    assert_eq!(
        absolute_seek(context(0, None), None, "", Duration::ZERO),
        SeekAction::Ignore
    );
}

#[test]
fn absolute_seek_accepts_the_end_and_rejects_positions_after_it() {
    let path = "/track";
    for position in [Duration::ZERO, Duration::from_secs(10)] {
        assert_eq!(
            absolute_seek(context(5, Some(10)), Some(path), path, position),
            SeekAction::Seek(position)
        );
    }
    assert_eq!(
        absolute_seek(
            context(5, Some(10)),
            Some(path),
            path,
            Duration::from_secs(10) + Duration::from_micros(1)
        ),
        SeekAction::Ignore
    );
    assert_eq!(
        absolute_seek(context(5, None), Some(path), path, Duration::MAX),
        SeekAction::Seek(Duration::MAX)
    );
}
