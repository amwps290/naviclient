use std::time::{Duration, Instant};

use rand::{rngs::StdRng, RngCore, SeedableRng};

use super::*;
use crate::api::OpenSubsonicProvider;
use crate::app::playback_occurrence::Selection;
use crate::app::playback_start::{select_state, PreparedStart, StartError, StartTarget};
use crate::app::session_state::SessionState;
use crate::app::song_scope::SongScope;
use crate::models::{PlaybackQueue, QueueEntry, QueueOrigin, Song, TranscodingQuality};
use crate::playback_timeline::PlaybackTimeline;

type StepSnapshot = (usize, Option<(Vec<usize>, Vec<usize>)>);

fn step_snapshot(step: Option<&QueueStep>) -> Option<StepSnapshot> {
    step.map(|step| match step {
        QueueStep::KeepHistory(index) => (*index, None),
        QueueStep::WithHistory { index, history } => (*index, Some(history.snapshot())),
    })
}

/// The prior inline selection policy, with only its RNG made controllable.
fn prior_random(navigation: &QueueNavigation<'_>, rng: &mut impl Rng) -> usize {
    let len = navigation.len;
    let current = navigation.current.unwrap_or(0);
    if len <= 1 {
        return current;
    }
    let recent: HashSet<usize> = navigation.history.recent((len - 1).min(8)).collect();
    let mut candidates: Vec<usize> = (0..len).filter(|index| !recent.contains(index)).collect();
    if candidates.is_empty() {
        candidates = (0..len).filter(|index| *index != current).collect();
    }
    candidates.choose(rng).copied().unwrap_or(current)
}

fn prior_next(
    navigation: &QueueNavigation<'_>,
    natural_end: bool,
    reserved: Option<usize>,
    rng: &mut impl Rng,
) -> Option<StepSnapshot> {
    let next = next_queue_index(
        navigation.modes,
        navigation.current?,
        navigation.len,
        natural_end,
        navigation.history.forward.last().copied(),
        reserved,
        || prior_random(navigation, rng),
    )?;
    let mut history = navigation.history.clone();
    if navigation.modes.shuffle && history.restore_forward().is_none() {
        history.advance(next);
    }
    Some((next, Some(history.snapshot())))
}

struct NoDraw;

impl RngCore for NoDraw {
    fn next_u32(&mut self) -> u32 {
        panic!("this choice must not draw randomness")
    }

    fn next_u64(&mut self) -> u64 {
        panic!("this choice must not draw randomness")
    }

    fn fill_bytes(&mut self, _: &mut [u8]) {
        panic!("this choice must not draw randomness")
    }

    fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
        panic!("this choice must not draw randomness")
    }
}

#[test]
fn next_plans_match_the_previous_policy_for_all_modes_targets_and_history_boundaries() {
    for shuffle in [false, true] {
        for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            for len in 0usize..=5 {
                for current in [
                    None,
                    Some(0),
                    Some(1),
                    Some(len.saturating_sub(1)),
                    Some(len),
                    Some(usize::MAX),
                ] {
                    for (played, forward) in [
                        (vec![], vec![]),
                        (vec![0, 1], vec![2]),
                        (vec![1, 0, 1], vec![0, 2]),
                        (vec![usize::MAX], vec![len]),
                    ] {
                        let history = ShuffleHistory::restore(played, forward);
                        let before = history.snapshot();
                        let navigation = QueueNavigation {
                            current,
                            len,
                            modes: PlayModes { shuffle, repeat },
                            history: &history,
                        };
                        for natural_end in [false, true] {
                            for reserved in [
                                None,
                                Some(0),
                                Some(len.saturating_sub(1)),
                                Some(len),
                                Some(usize::MAX),
                            ] {
                                let mut expected_rng = StdRng::seed_from_u64(42);
                                let mut actual_rng = expected_rng.clone();
                                let expected = prior_next(
                                    &navigation,
                                    natural_end,
                                    reserved,
                                    &mut expected_rng,
                                );
                                let actual = navigation.next_playback(
                                    natural_end,
                                    reserved,
                                    &mut actual_rng,
                                );
                                assert_eq!(step_snapshot(actual.as_ref()), expected);
                                assert_eq!(actual_rng.next_u64(), expected_rng.next_u64());
                                assert_eq!(history.snapshot(), before);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn random_selection_preserves_recent_window_duplicates_invalid_history_and_small_queue_fallbacks() {
    for len in 0usize..=12 {
        for played in [
            vec![],
            vec![0; 12],
            (0..len).collect(),
            (0..len).rev().collect(),
            vec![usize::MAX, 1, 1, 0, 2, 3, 4, 5, 6, 7, 8, 9],
        ] {
            let history = ShuffleHistory::restore(played.clone(), vec![]);
            let navigation = QueueNavigation {
                current: Some(len.saturating_sub(1)),
                len,
                modes: PlayModes {
                    shuffle: true,
                    repeat: RepeatMode::Off,
                },
                history: &history,
            };
            if len <= 1 {
                assert_eq!(
                    navigation.random_index(&mut NoDraw),
                    navigation.current.unwrap()
                );
                continue;
            }
            let recent: Vec<_> = played.iter().rev().take((len - 1).min(8)).collect();
            let mut allowed: Vec<_> = (0..len).filter(|index| !recent.contains(&index)).collect();
            if allowed.is_empty() {
                allowed = (0..len)
                    .filter(|index| Some(*index) != navigation.current)
                    .collect();
            }
            for seed in 0..32 {
                let mut expected_rng = StdRng::seed_from_u64(seed);
                let mut actual_rng = expected_rng.clone();
                let index = navigation.random_index(&mut actual_rng);
                assert_eq!(index, prior_random(&navigation, &mut expected_rng));
                assert!(allowed.contains(&index));
                assert_eq!(actual_rng.next_u64(), expected_rng.next_u64());
            }
        }
    }
}

#[test]
fn preload_target_and_playback_plan_share_priority_without_consuming_randomness() {
    for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
        for natural_end in [false, true] {
            for (forward, reserved, fallback_target) in [
                (vec![2], Some(3), Some(2)),
                (vec![], Some(3), Some(3)),
                (vec![4], Some(3), None),
                (vec![], Some(4), None),
            ] {
                let history = ShuffleHistory::restore(vec![0, 1], forward);
                let before = history.snapshot();
                let navigation = QueueNavigation {
                    current: Some(1),
                    len: 4,
                    modes: PlayModes {
                        shuffle: true,
                        repeat,
                    },
                    history: &history,
                };
                let expected = if repeat == RepeatMode::One && natural_end {
                    Some(1)
                } else {
                    fallback_target
                };
                assert_eq!(
                    navigation.next_index(natural_end, reserved, &mut NoDraw),
                    expected
                );
                let step = navigation.next_playback(natural_end, reserved, &mut NoDraw);
                assert_eq!(
                    step_snapshot(step.as_ref()).map(|(index, _)| index),
                    expected
                );
                assert_eq!(history.snapshot(), before);
            }
        }
    }
}

#[test]
fn previous_plans_prepare_a_history_branch_without_changing_the_source() {
    for shuffle in [false, true] {
        for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            for len in 0usize..=4 {
                for current in [
                    None,
                    Some(0),
                    Some(1),
                    Some(len.saturating_sub(1)),
                    Some(usize::MAX),
                ] {
                    for played in [vec![], vec![1], vec![0, 2, 1], vec![1, 1]] {
                        let history = ShuffleHistory::restore(played, vec![3, 2]);
                        let before = history.snapshot();
                        let navigation = QueueNavigation {
                            current,
                            len,
                            modes: PlayModes { shuffle, repeat },
                            history: &history,
                        };
                        let expected = current.and_then(|index| {
                            if len == 0 {
                                None
                            } else if shuffle {
                                let mut copy = history.clone();
                                copy.previous().map(|index| (index, Some(copy.snapshot())))
                            } else {
                                let target = if index > 0 {
                                    Some(index - 1)
                                } else if repeat != RepeatMode::Off {
                                    Some(len - 1)
                                } else {
                                    None
                                };
                                target.map(|index| (index, None))
                            }
                        });
                        assert_eq!(
                            step_snapshot(navigation.previous_playback().as_ref()),
                            expected
                        );
                        assert_eq!(history.snapshot(), before);
                    }
                }
            }
        }
    }
}

fn state(modes: PlayModes) -> AppState {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(
            (0..3)
                .map(|index| Song {
                    id: "same".into(),
                    title: format!("Occurrence {index}"),
                    duration: Some(180),
                    ..Default::default()
                })
                .collect(),
            QueueOrigin::Playlist,
        ),
        playback_modes: modes,
        shuffle_history: ShuffleHistory::restore(vec![0, 1], vec![2]),
        ..Default::default()
    };
    let song = state.queue.song(1).unwrap().clone();
    select_state(
        &mut state,
        1,
        &song,
        TranscodingQuality::Original,
        Selection::New,
    );
    state.song_selection.toggle(SongScope::Queue, 2, 3);
    state
}

fn prepared(state: &AppState, step: QueueStep) -> PreparedStart {
    let (index, history) = match step {
        QueueStep::KeepHistory(index) => (index, None),
        QueueStep::WithHistory { index, history } => (index, Some(history)),
    };
    let api = OpenSubsonicProvider::new("http://127.0.0.1:1", "fixture-user", "fixture-password")
        .unwrap();
    let start = PreparedStart::prepare(
        StartTarget::existing(&state.queue, index).unwrap(),
        Some(&api),
        TranscodingQuality::Original,
        Duration::ZERO,
        Selection::New,
    )
    .unwrap();
    if let Some(history) = history {
        start.with_history(history)
    } else {
        start
    }
}

#[test]
fn next_and_previous_history_are_committed_only_when_the_audio_command_is_accepted() {
    for shuffle in [false, true] {
        for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::One] {
            for forward in [false, true] {
                for accepted in [false, true] {
                    let mut state = state(PlayModes { shuffle, repeat });
                    let mut session = SessionState::new(Instant::now());
                    session.set_position(PlaybackTimeline {
                        position: Duration::from_secs(42),
                        duration: Some(Duration::from_secs(180)),
                    });
                    let old_entries: Vec<_> =
                        state.queue.entries().iter().map(QueueEntry::id).collect();
                    let old_occurrence = state.occurrence.id();
                    let old_selection = state.song_selection.clone();
                    let old_history = state.shuffle_history.snapshot();
                    let step = if forward {
                        state
                            .queue_navigation()
                            .next_playback(false, None, &mut NoDraw)
                    } else {
                        state.queue_navigation().previous_playback()
                    }
                    .unwrap();
                    let (target, expected_history) = step_snapshot(Some(&step)).unwrap();
                    assert_eq!(state.queue_index, Some(1));
                    assert_eq!(state.shuffle_history.snapshot(), old_history);

                    let result =
                        prepared(&state, step).commit(&mut state, &mut session, |_| accepted);

                    assert_eq!(
                        state
                            .queue
                            .entries()
                            .iter()
                            .map(QueueEntry::id)
                            .collect::<Vec<_>>(),
                        old_entries
                    );
                    assert_eq!(state.song_selection, old_selection);
                    if accepted {
                        assert!(result.is_ok());
                        assert_eq!(state.queue_index, Some(target));
                        assert_eq!(
                            state.now_playing.as_ref().unwrap().title,
                            format!("Occurrence {target}")
                        );
                        assert_eq!(
                            state.shuffle_history.snapshot(),
                            expected_history.unwrap_or(old_history)
                        );
                        assert_ne!(state.occurrence.id(), old_occurrence);
                        assert_eq!(session.position(Duration::ZERO), Duration::ZERO);
                    } else {
                        assert!(matches!(result, Err(StartError::OutputUnavailable)));
                        assert_eq!(state.queue_index, Some(1));
                        assert_eq!(state.now_playing.as_ref().unwrap().title, "Occurrence 1");
                        assert_eq!(state.shuffle_history.snapshot(), old_history);
                        assert_eq!(state.occurrence.id(), old_occurrence);
                        assert_eq!(session.position(Duration::ZERO), Duration::from_secs(42));
                    }
                }
            }
        }
    }
}

#[test]
fn a_prepared_step_cannot_start_a_different_duplicate_occurrence_after_the_queue_moves() {
    for forward in [false, true] {
        let mut state = state(PlayModes {
            shuffle: true,
            repeat: RepeatMode::Off,
        });
        let step = if forward {
            state
                .queue_navigation()
                .next_playback(false, None, &mut NoDraw)
        } else {
            state.queue_navigation().previous_playback()
        }
        .unwrap();
        let (index, _) = step_snapshot(Some(&step)).unwrap();
        let start = prepared(&state, step);
        state.edit_queue().move_entry(index, 1);
        let history = state.shuffle_history.snapshot();
        let current = state.queue_index;
        let occurrence = state.occurrence.id();
        assert!(matches!(
            start.commit(
                &mut state,
                &mut SessionState::new(Instant::now()),
                |_| panic!("stale occurrence must not send audio")
            ),
            Err(StartError::Stale)
        ));
        assert_eq!(state.shuffle_history.snapshot(), history);
        assert_eq!(state.queue_index, current);
        assert_eq!(state.occurrence.id(), occurrence);
    }
}
