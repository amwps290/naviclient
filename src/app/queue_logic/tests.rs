use super::*;
use crate::models::Song;

#[test]
fn preload_target_matches_playback_modes_and_manual_repeat_one_skip() {
    let sequential = PlayModes {
        shuffle: false,
        repeat: RepeatMode::Off,
    };
    let repeat_all = PlayModes {
        shuffle: false,
        repeat: RepeatMode::All,
    };
    let repeat_one = PlayModes {
        shuffle: false,
        repeat: RepeatMode::One,
    };
    for (mode, current, natural, expected) in [
        (sequential, 0, true, Some(1)),
        (sequential, 2, true, None),
        (repeat_all, 2, true, Some(0)),
        (repeat_one, 1, true, Some(1)),
        (repeat_one, 1, false, Some(2)),
        (repeat_one, 2, false, Some(0)),
    ] {
        assert_eq!(
            super::next_queue_index(mode, current, 3, natural, None, None, || panic!(
                "non-shuffle selected randomly"
            )),
            expected
        );
    }
    assert_eq!(
        super::next_queue_index(repeat_all, 0, 0, true, None, None, || 0),
        None
    );
    assert_eq!(
        super::next_queue_index(repeat_one, 0, 1, true, None, None, || 0),
        Some(0)
    );
    // 随机 + 单曲循环：自然结束重播当前，手动切歌仍走随机链。
    let shuffle_one = PlayModes {
        shuffle: true,
        repeat: RepeatMode::One,
    };
    assert_eq!(
        super::next_queue_index(shuffle_one, 1, 3, true, None, None, || panic!(
            "natural end must replay the current track"
        )),
        Some(1)
    );
    assert_eq!(
        super::next_queue_index(shuffle_one, 1, 3, false, None, Some(2), || panic!(
            "manual next uses the reservation"
        )),
        Some(2)
    );
}

#[test]
fn auto_continue_only_triggers_at_a_natural_sequential_queue_boundary() {
    let sequential = PlayModes {
        shuffle: false,
        repeat: RepeatMode::Off,
    };
    assert!(super::should_auto_continue(
        true,
        sequential,
        Some(2),
        3,
        true
    ));
    assert!(!super::should_auto_continue(
        false,
        sequential,
        Some(2),
        3,
        true
    ));
    assert!(!super::should_auto_continue(
        true,
        sequential,
        Some(1),
        3,
        true
    ));
    assert!(!super::should_auto_continue(
        true,
        sequential,
        Some(2),
        3,
        false
    ));
    for modes in [
        PlayModes {
            shuffle: false,
            repeat: RepeatMode::All,
        },
        PlayModes {
            shuffle: false,
            repeat: RepeatMode::One,
        },
        PlayModes {
            shuffle: true,
            repeat: RepeatMode::Off,
        },
    ] {
        assert!(!super::should_auto_continue(true, modes, Some(2), 3, true));
    }
}

#[test]
fn auto_continue_result_rejects_queue_changes_and_stale_anchors() {
    let modes = PlayModes {
        shuffle: false,
        repeat: RepeatMode::Off,
    };
    let song = |id: &str| Song {
        id: id.into(),
        title: id.into(),
        ..Song::default()
    };
    let queue = vec![song("a"), song("anchor")];
    assert!(super::auto_continue_context_matches(
        true,
        modes,
        Some(1),
        SongList::Songs(&queue),
        2,
        "anchor"
    ));
    assert!(!super::auto_continue_context_matches(
        true,
        modes,
        Some(1),
        SongList::Songs(&queue),
        1,
        "anchor"
    ));
    assert!(!super::auto_continue_context_matches(
        true,
        modes,
        Some(1),
        SongList::Songs(&queue),
        2,
        "old-anchor"
    ));
    let mut edited = queue;
    edited.push(song("manual"));
    assert!(!super::auto_continue_context_matches(
        true,
        modes,
        Some(1),
        SongList::Songs(&edited),
        2,
        "anchor"
    ));
}

#[test]
fn shuffle_reservation_and_forward_history_do_not_draw_a_second_random_song() {
    let mut history = ShuffleHistory::default();
    history.start(0);
    assert_eq!(history.previous(), None);
    assert_eq!(history.snapshot(), (vec![0], vec![]));
    history.advance(2);
    assert_eq!(history.previous(), Some(0));
    let before = history.snapshot();
    let next = super::next_queue_index(
        PlayModes {
            shuffle: true,
            repeat: RepeatMode::Off,
        },
        0,
        4,
        true,
        history.forward.last().copied(),
        Some(3),
        || panic!("forward history has priority"),
    );
    assert_eq!(next, Some(2));
    assert_eq!(history.snapshot(), before);
    assert_eq!(history.restore_forward(), next);
    assert_eq!(
        super::next_queue_index(
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
            2,
            4,
            true,
            None,
            Some(3),
            || panic!("reserved choice must be reused")
        ),
        Some(3)
    );
    assert_eq!(
        super::next_queue_index(
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
            2,
            4,
            false,
            None,
            Some(3),
            || panic!("manual next uses the same reservation")
        ),
        Some(3)
    );
}

#[test]
fn queue_edits_remap_shuffle_history_by_occurrence() {
    // Positions 0 and 2 may have identical song IDs; their history is distinct.
    let mut history = ShuffleHistory::restore(vec![0, 2, 1], vec![3]);
    history.remap(|i| Some(if i >= 1 { i + 1 } else { i }));
    assert_eq!(history.snapshot(), (vec![0, 3, 2], vec![4]));
    history.remap(|i| Some(queue_index_after_move(3, 0, i)));
    assert_eq!(history.snapshot(), (vec![1, 0, 3], vec![4]));
    history.remap(|i| if i == 0 { None } else { Some(i - 1) });
    assert_eq!(history.snapshot(), (vec![0, 2], vec![3]));
}

#[test]
fn sequential_mode_stops_at_queue_boundaries() {
    assert_eq!(advance_index(2, 3, true, false), None); // 末尾 next → 不循环
    assert_eq!(advance_index(0, 3, false, false), None); // 开头 prev → 不循环
    assert_eq!(advance_index(1, 3, true, false), Some(2)); // 中间 next → 下一首
}

#[test]
fn repeat_modes_wrap_around_queue() {
    assert_eq!(advance_index(2, 3, true, true), Some(0));
    assert_eq!(advance_index(0, 3, false, true), Some(2));
    assert_eq!(advance_index(1, 3, true, true), Some(2));
}

#[test]
fn removing_item_before_current_shifts_index_down() {
    assert_eq!(queue_index_after_remove(0, Some(2), 3), Some(1));
    assert_eq!(queue_index_after_remove(1, Some(4), 4), Some(3));
}

#[test]
fn removing_current_falls_back_to_same_slot() {
    // 删除正在播放的歌曲后，改播原位置的下一首。
    assert_eq!(queue_index_after_remove(2, Some(2), 2), Some(1));
    assert_eq!(queue_index_after_remove(1, Some(1), 3), Some(1));
}

#[test]
fn removing_last_current_clears_the_queue_index() {
    assert_eq!(queue_index_after_remove(0, Some(0), 0), None);
}

#[test]
fn removing_item_after_current_keeps_index() {
    assert_eq!(queue_index_after_remove(3, Some(1), 3), Some(1));
    assert_eq!(queue_index_after_remove(2, None, 3), None);
}

#[test]
fn moving_item_before_current_to_after_shifts_index_down() {
    assert_eq!(queue_index_after_move(0, 3, 2), 1);
    assert_eq!(queue_index_after_move(1, 3, 2), 1);
}

#[test]
fn moving_item_after_current_to_before_shifts_index_up() {
    assert_eq!(queue_index_after_move(3, 0, 2), 3);
}

#[test]
fn moving_current_repositions_the_index() {
    assert_eq!(queue_index_after_move(2, 0, 2), 0);
    assert_eq!(queue_index_after_move(0, 3, 0), 3);
    assert_eq!(queue_index_after_move(2, 3, 2), 3);
}

#[test]
fn adjacent_moves_keep_relative_position() {
    assert_eq!(queue_index_after_move(1, 2, 2), 1);
    assert_eq!(queue_index_after_move(1, 2, 1), 2);
}

#[test]
fn playback_modes_default_to_sequential_and_independent() {
    assert_eq!(
        PlayModes::default(),
        PlayModes {
            shuffle: false,
            repeat: RepeatMode::Off
        }
    );
    // 随机与循环相互独立：可以同时开启随机 + 单曲循环。
    let both = PlayModes {
        shuffle: true,
        repeat: RepeatMode::One,
    };
    assert!(both.shuffle);
    assert_eq!(both.repeat, RepeatMode::One);
}

#[test]
fn shuffle_previous_returns_actually_played_tracks() {
    let mut history = ShuffleHistory::default();
    history.start(2);
    history.advance(5);
    history.advance(0);
    assert_eq!(history.previous(), Some(5)); // 回到真正播放过的上一首
    assert_eq!(history.previous(), Some(2));
    assert_eq!(history.previous(), None); // 无更多历史时停在当前
}

#[test]
fn shuffle_restores_forward_after_going_back() {
    let mut history = ShuffleHistory::default();
    history.start(2);
    history.advance(5);
    history.advance(0);
    assert_eq!(history.previous(), Some(5));
    assert_eq!(history.restore_forward(), Some(0)); // 前进恢复刚才回退的歌
    assert_eq!(history.restore_forward(), None);
}

#[test]
fn shuffle_next_excludes_recently_played_tracks() {
    let mut history = ShuffleHistory::default();
    history.start(0);
    for index in 1..=6 {
        history.advance(index);
    }
    let recent: Vec<usize> = history.recent(4).collect();
    assert_eq!(recent, vec![6, 5, 4, 3]); // 最近 4 首被排除
}

#[test]
fn history_index_shifts_only_from_insert_point() {
    assert_eq!(history_index_after_insert(0, 3, 2), 0);
    assert_eq!(history_index_after_insert(1, 3, 2), 1);
    // 插入点及之后平移 count
    assert_eq!(history_index_after_insert(3, 3, 2), 5);
    assert_eq!(history_index_after_insert(9, 3, 2), 11);
    // 与逐首插入（count=1）的既有语义一致
    for index in [0, 3, 5, 9] {
        let single = if index >= 3 { index + 1 } else { index };
        assert_eq!(history_index_after_insert(index, 3, 1), single);
    }
}
