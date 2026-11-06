use super::*;

#[test]
fn reconnect_only_retries_network_and_server_failures() {
    // 解码失败、本地输出问题重试也不会变好，直接交给用户。
    for kind in [
        StreamFailure::Decode,
        StreamFailure::Output,
        StreamFailure::Preparation,
        StreamFailure::Http(404),
        StreamFailure::Http(401),
    ] {
        assert!(matches!(
            reconnect_decision(Some(kind), 0, 2),
            ReconnectDecision::DoNotRetry
        ));
    }
    // 没有分类信息时不猜，保持原有「直接报错」行为。
    assert!(matches!(
        reconnect_decision(None, 0, 2),
        ReconnectDecision::DoNotRetry
    ));
    for kind in [
        StreamFailure::Connect,
        StreamFailure::Timeout,
        StreamFailure::NoData,
        StreamFailure::Truncated,
        StreamFailure::Http(503),
    ] {
        assert!(matches!(
            reconnect_decision(Some(kind), 0, 2),
            ReconnectDecision::Retry { attempt: 1, .. }
        ));
    }
}

#[test]
fn reconnect_backs_off_and_stops_after_the_limit() {
    assert!(matches!(
        reconnect_decision(Some(StreamFailure::Timeout), 0, 2),
        ReconnectDecision::Retry { attempt: 1, delay } if delay == Duration::from_millis(1500)
    ));
    assert!(matches!(
        reconnect_decision(Some(StreamFailure::Timeout), 1, 2),
        ReconnectDecision::Retry { attempt: 2, delay } if delay == Duration::from_secs(4)
    ));
    // 次数用尽后必须停下来，否则会无限重连。
    assert!(matches!(
        reconnect_decision(Some(StreamFailure::Timeout), 2, 2),
        ReconnectDecision::Exhausted
    ));
}

#[test]
fn waiting_retries_once_at_the_deadline_from_the_failed_position() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let failed_at = Duration::from_millis(42_250);
    assert_eq!(
        state.schedule(song, Some(StreamFailure::Timeout), failed_at, now),
        ReconnectDecision::Retry {
            attempt: 1,
            delay: Duration::from_millis(1500),
        }
    );
    let playback = PlaybackState::default();

    assert_eq!(
        state.poll(
            now + Duration::from_millis(1499),
            Some(song),
            true,
            &playback
        ),
        ReconnectPoll::Idle
    );
    assert_eq!(
        state.poll(
            now + Duration::from_millis(1500),
            Some(song),
            true,
            &playback
        ),
        ReconnectPoll::Retry(ReconnectAttempt {
            attempt: 1,
            position: failed_at,
        })
    );
    assert_eq!(
        state.poll(now + Duration::from_secs(10), Some(song), true, &playback),
        ReconnectPoll::Idle
    );
}

#[test]
fn successive_failures_keep_the_budget_until_exhausted() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let mut now = Instant::now();
    let position = Duration::from_secs(40);
    let playback = PlaybackState::default();

    for (attempt, delay, failed_at) in [
        (1, Duration::from_millis(1500), position),
        (2, Duration::from_secs(4), position + Duration::from_secs(1)),
    ] {
        assert_eq!(
            state.schedule(song, Some(StreamFailure::Timeout), failed_at, now),
            ReconnectDecision::Retry { attempt, delay }
        );
        assert_eq!(
            state.poll(now + delay, Some(song), true, &playback),
            ReconnectPoll::Retry(ReconnectAttempt {
                attempt,
                position: failed_at,
            })
        );
        now += delay;
    }

    assert_eq!(
        state.schedule(song, Some(StreamFailure::Timeout), position, now),
        ReconnectDecision::Exhausted
    );
    assert_eq!(
        state.poll(now + Duration::from_secs(20), Some(song), true, &playback),
        ReconnectPoll::Idle
    );
}

#[test]
fn recovery_requires_more_than_five_seconds_of_error_free_progress() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let failed_at = Duration::from_secs(40);
    state.schedule(song, Some(StreamFailure::Connect), failed_at, now);
    assert!(matches!(
        state.poll(
            now + Duration::from_secs(2),
            Some(song),
            true,
            &PlaybackState::default()
        ),
        ReconnectPoll::Retry(_)
    ));
    let now = now + Duration::from_secs(2);

    let mut playback = PlaybackState {
        active: true,
        position: failed_at + RECOVERY_CONFIRMATION,
        ..Default::default()
    };
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    playback.position += Duration::from_millis(1);
    playback.active = false;
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    playback.active = true;
    playback.paused = true;
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    playback.paused = false;
    playback.error = Some("still interrupted".into());
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    playback.error = None;
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Recovered
    );
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );

    assert!(matches!(
        state.schedule(song, Some(StreamFailure::Timeout), playback.position, now),
        ReconnectDecision::Retry { attempt: 1, .. }
    ));
}

#[test]
fn waiting_is_not_confirmed_as_recovered_by_an_old_audio_snapshot() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let failed_at = Duration::from_secs(40);
    state.schedule(song, Some(StreamFailure::Timeout), failed_at, now);
    let playback = PlaybackState {
        active: true,
        position: Duration::from_secs(50),
        ..Default::default()
    };

    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    assert_eq!(
        state.poll(now + Duration::from_secs(2), Some(song), true, &playback),
        ReconnectPoll::Retry(ReconnectAttempt {
            attempt: 1,
            position: failed_at
        })
    );
}

#[test]
fn explicit_cancellation_prevents_same_song_retry_and_resets_its_budget() {
    let song = PlaybackId::new();
    let now = Instant::now();
    let playback = PlaybackState::default();
    for already_dispatched in [false, true] {
        let mut state = PlaybackReconnect::default();
        state.schedule(
            song,
            Some(StreamFailure::Timeout),
            Duration::from_secs(40),
            now,
        );
        if already_dispatched {
            assert!(matches!(
                state.poll(now + Duration::from_secs(2), Some(song), true, &playback),
                ReconnectPoll::Retry(_)
            ));
        }

        state.cancel();
        assert_eq!(
            state.poll(now + Duration::from_secs(20), Some(song), true, &playback),
            ReconnectPoll::Idle
        );
        assert!(matches!(
            state.schedule(
                song,
                Some(StreamFailure::Timeout),
                Duration::ZERO,
                now + Duration::from_secs(20)
            ),
            ReconnectDecision::Retry { attempt: 1, .. }
        ));
    }
}

#[test]
fn changing_tracks_discards_the_plan_before_its_deadline() {
    let first = PlaybackId::new();
    let second = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let playback = PlaybackState::default();
    state.schedule(
        first,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );

    assert_eq!(
        state.poll(now, Some(second), true, &playback),
        ReconnectPoll::Idle
    );
    // Returning to the same song later must not revive its canceled plan.
    assert_eq!(
        state.poll(now + Duration::from_secs(20), Some(first), true, &playback),
        ReconnectPoll::Idle
    );
}

#[test]
fn stopping_discards_the_plan_even_if_the_song_is_selected_again() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let playback = PlaybackState::default();
    state.schedule(
        song,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );

    assert_eq!(state.poll(now, None, true, &playback), ReconnectPoll::Idle);
    assert_eq!(
        state.poll(now + Duration::from_secs(20), Some(song), true, &playback),
        ReconnectPoll::Idle
    );
}

#[test]
fn missing_server_or_queue_discards_waiting_and_recovery_state() {
    let song = PlaybackId::new();
    let now = Instant::now();
    let playback = PlaybackState::default();
    for already_dispatched in [false, true] {
        let mut state = PlaybackReconnect::default();
        state.schedule(
            song,
            Some(StreamFailure::Timeout),
            Duration::from_secs(40),
            now,
        );
        if already_dispatched {
            state.poll(now + Duration::from_secs(2), Some(song), true, &playback);
        }

        assert_eq!(
            state.poll(now + Duration::from_secs(3), Some(song), false, &playback),
            ReconnectPoll::Idle
        );
        assert_eq!(
            state.poll(now + Duration::from_secs(20), Some(song), true, &playback),
            ReconnectPoll::Idle
        );
        assert!(matches!(
            state.schedule(
                song,
                Some(StreamFailure::Timeout),
                Duration::ZERO,
                now + Duration::from_secs(20)
            ),
            ReconnectDecision::Retry { attempt: 1, .. }
        ));
    }
}

#[test]
fn failure_on_another_song_starts_a_new_budget_and_replaces_the_plan() {
    let first = PlaybackId::new();
    let second = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let playback = PlaybackState::default();
    state.schedule(
        first,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );
    let second_failed_at = Duration::from_secs(10);

    assert!(matches!(
        state.schedule(second, Some(StreamFailure::Timeout), second_failed_at, now),
        ReconnectDecision::Retry { attempt: 1, .. }
    ));
    assert_eq!(
        state.poll(now + Duration::from_secs(2), Some(second), true, &playback),
        ReconnectPoll::Retry(ReconnectAttempt {
            attempt: 1,
            position: second_failed_at
        })
    );
}

#[test]
fn terminal_or_unclassified_failure_cancels_a_previous_retry() {
    let song = PlaybackId::new();
    let now = Instant::now();
    let playback = PlaybackState::default();
    for failure in [
        Some(StreamFailure::Decode),
        Some(StreamFailure::Http(401)),
        None,
    ] {
        let mut state = PlaybackReconnect::default();
        state.schedule(
            song,
            Some(StreamFailure::Timeout),
            Duration::from_secs(40),
            now,
        );

        assert_eq!(
            state.schedule(song, failure, Duration::from_secs(40), now),
            ReconnectDecision::DoNotRetry
        );
        assert_eq!(
            state.poll(now + Duration::from_secs(20), Some(song), true, &playback),
            ReconnectPoll::Idle
        );
    }
}

#[test]
fn recovery_is_measured_from_the_latest_failure_position() {
    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    state.schedule(
        song,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );
    state.poll(
        now + Duration::from_secs(2),
        Some(song),
        true,
        &PlaybackState::default(),
    );
    let now = now + Duration::from_secs(2);
    state.schedule(
        song,
        Some(StreamFailure::Timeout),
        Duration::from_secs(44),
        now,
    );
    state.poll(
        now + Duration::from_secs(4),
        Some(song),
        true,
        &PlaybackState::default(),
    );
    let now = now + Duration::from_secs(4);
    let mut playback = PlaybackState {
        active: true,
        position: Duration::from_secs(46),
        ..Default::default()
    };

    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Idle
    );
    playback.position = Duration::from_secs(50);
    assert_eq!(
        state.poll(now, Some(song), true, &playback),
        ReconnectPoll::Recovered
    );
}

#[test]
fn a_new_occurrence_of_the_same_song_discards_the_old_retry_plan_and_budget() {
    let mut state = PlaybackReconnect::default();
    let first = PlaybackId::new();
    let second = PlaybackId::new();
    let now = Instant::now();
    state.schedule(
        first,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );
    assert_eq!(
        state.poll(now, Some(second), true, &PlaybackState::default()),
        ReconnectPoll::Idle
    );
    assert_eq!(
        state.poll(
            now + Duration::from_secs(20),
            Some(first),
            true,
            &PlaybackState::default()
        ),
        ReconnectPoll::Idle
    );
    assert!(matches!(
        state.schedule(second, Some(StreamFailure::Timeout), Duration::ZERO, now),
        ReconnectDecision::Retry { attempt: 1, .. }
    ));
}

#[test]
fn pending_seek_and_buffering_cannot_confirm_recovery() {
    use crate::audio::test_transport::Fixture;

    let song = PlaybackId::new();
    let mut state = PlaybackReconnect::default();
    let now = Instant::now();
    let fixture = Fixture::new(PlaybackState {
        active: true,
        output_connected: true,
        position: Duration::from_secs(40),
        ..Default::default()
    });
    state.schedule(
        song,
        Some(StreamFailure::Timeout),
        Duration::from_secs(40),
        now,
    );
    assert!(matches!(
        state.poll(
            now + Duration::from_secs(2),
            Some(song),
            true,
            &fixture.audio.progress()
        ),
        ReconnectPoll::Retry(_)
    ));
    fixture.audio.seek(Duration::from_secs(50));
    assert_eq!(
        state.poll(
            now + Duration::from_secs(3),
            Some(song),
            true,
            &fixture.audio.progress()
        ),
        ReconnectPoll::Idle
    );
    let playback = PlaybackState {
        active: true,
        position: Duration::from_secs(50),
        ..Default::default()
    };
    fixture.publish_next(playback.clone());
    assert_eq!(
        state.poll(
            now + Duration::from_secs(3),
            Some(song),
            true,
            &fixture.audio.progress()
        ),
        ReconnectPoll::Idle
    );
    fixture.publish(PlaybackState {
        output_connected: true,
        ..playback
    });
    assert_eq!(
        state.poll(
            now + Duration::from_secs(3),
            Some(song),
            true,
            &fixture.audio.progress()
        ),
        ReconnectPoll::Recovered
    );
}
