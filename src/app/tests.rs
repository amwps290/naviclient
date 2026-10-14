//! 应用层纯逻辑测试（从 app.rs 拆出）。

use std::time::Duration;

use gpui_kit::hsla;

use super::{
    accent_foreground, advance_index, album_page_offset, contrast_ratio, effective_volume,
    extract_cover_palette, format_file_size, loudness_gain_factor, match_shortcut,
    normalize_volume, playback_technical_info, queue_index_after_move,
    queue_index_after_remove, readable_accent, replay_gain_factor, restored_volume,
    seek_target, should_scrobble, Shortcut, ShuffleHistory,
};
use crate::models::{PlaybackMode, Song, TranscodingQuality, VolumeNormalization};
use gpui_kit::Modifiers;

#[test]
fn preloading_waits_for_safe_current_audio_buffer_and_near_end() {
    let mut playback = crate::audio::PlaybackState {
        active: true,
        duration: Some(Duration::from_secs(180)),
        position: Duration::from_secs(165),
        fully_buffered: true,
        ..Default::default()
    };
    assert!(super::should_preload_next(&playback));
    playback.fully_buffered = false;
    assert!(!super::should_preload_next(&playback));
    playback.fully_buffered = true;
    playback.position = Duration::from_secs(164);
    assert!(!super::should_preload_next(&playback));
    playback.position = Duration::from_secs(165);
    playback.paused = true;
    assert!(!super::should_preload_next(&playback));
    playback.paused = false;
    playback.duration = None;
    assert!(!super::should_preload_next(&playback));
    playback.duration = Some(playback.position);
    assert!(!super::should_preload_next(&playback));
    playback.duration = Some(Duration::from_secs(180));
    playback.error = Some("stream failed".into());
    assert!(!super::should_preload_next(&playback));
}

#[test]
fn preload_target_matches_playback_modes_and_manual_repeat_one_skip() {
    for (mode, current, natural, expected) in [
        (PlaybackMode::Sequential, 0, true, Some(1)),
        (PlaybackMode::Sequential, 2, true, None),
        (PlaybackMode::RepeatAll, 2, true, Some(0)),
        (PlaybackMode::RepeatOne, 1, true, Some(1)),
        (PlaybackMode::RepeatOne, 1, false, Some(2)),
        (PlaybackMode::RepeatOne, 2, false, Some(0)),
    ] {
        assert_eq!(
            super::next_queue_index(mode, current, 3, natural, None, None, || panic!(
                "non-shuffle selected randomly"
            )),
            expected
        );
    }
    assert_eq!(
        super::next_queue_index(PlaybackMode::RepeatAll, 0, 0, true, None, None, || 0),
        None
    );
    assert_eq!(
        super::next_queue_index(PlaybackMode::RepeatOne, 0, 1, true, None, None, || 0),
        Some(0)
    );
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
        PlaybackMode::Shuffle,
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
        super::next_queue_index(PlaybackMode::Shuffle, 2, 4, true, None, Some(3), || panic!(
            "reserved choice must be reused"
        )),
        Some(3)
    );
    assert_eq!(
        super::next_queue_index(
            PlaybackMode::Shuffle,
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
fn duplicate_items_have_distinct_hover_and_element_keys() {
    let first = super::item_instance_key("recent", "album", 0, "same-id");
    assert_ne!(
        first,
        super::item_instance_key("frequent", "album", 0, "same-id")
    );
    assert_ne!(
        first,
        super::item_instance_key("recent", "album", 1, "same-id")
    );
    assert_ne!(
        first,
        super::item_instance_key("recent", "song", 0, "same-id")
    );
    assert_ne!(
        super::item_instance_key("queue", "song", 0, "repeat"),
        super::item_instance_key("queue", "song", 1, "repeat")
    );
}

#[test]
fn cover_accent_remains_readable_on_light_and_dark_backgrounds() {
    let source = hsla(0.15, 0.9, 0.62, 1.0);
    let light = hsla(0.0, 0.0, 0.96, 1.0);
    let dark = hsla(0.0, 0.0, 0.08, 1.0);

    assert!(contrast_ratio(readable_accent(source, light), light) >= 4.5);
    assert!(contrast_ratio(readable_accent(source, dark), dark) >= 4.5);
}

#[test]
fn accent_button_foreground_prefers_the_stronger_contrast() {
    let accent = hsla(0.12, 0.7, 0.72, 1.0);
    let foreground = accent_foreground(accent);
    assert!(contrast_ratio(foreground, accent) >= 4.5);
}

#[test]
fn extracts_palette_from_default_cover() {
    let palette = extract_cover_palette(include_bytes!("../../assets/default-cover.png"));
    assert!(palette.is_some());
}

#[test]
fn displays_original_stream_metadata() {
    let song = Song {
        suffix: Some("flac".to_string()),
        bit_rate: Some(941),
        size: Some(25_165_824),
        duration: Some(214),
        ..Song::default()
    };

    let info = playback_technical_info(&song, TranscodingQuality::Original);

    assert_eq!(info.format, "FLAC");
    assert_eq!(info.tooltip, "Current stream: FLAC · 941 kbps · 24.0 MB");
}

#[test]
fn displays_transcoded_stream_profile_and_estimated_size() {
    let song = Song {
        suffix: Some("flac".to_string()),
        size: Some(25_165_824),
        duration: Some(240),
        ..Song::default()
    };

    let info = playback_technical_info(&song, TranscodingQuality::Kbps192);

    assert_eq!(info.format, "MP3");
    assert_eq!(
        info.tooltip,
        "Current stream: MP3 · 192 kbps · ~5.5 MB\nSource file: FLAC · ~838 kbps · 24.0 MB"
    );
}

#[test]
fn formats_file_sizes_for_compact_player_display() {
    assert_eq!(format_file_size(800), "800 B");
    assert_eq!(format_file_size(1_536), "1.5 KB");
    assert_eq!(format_file_size(3 * 1024 * 1024), "3.0 MB");
}

#[test]
fn normalizes_volume_to_a_safe_output_range() {
    assert!((normalize_volume(0.42) - 0.42).abs() < f32::EPSILON);
    assert_eq!(normalize_volume(-0.5), 0.0);
    assert_eq!(normalize_volume(1.5), 1.0);
    assert!((normalize_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
}

#[test]
fn muting_only_changes_effective_output_volume() {
    assert!((effective_volume(0.64, false) - 0.64).abs() < f32::EPSILON);
    assert_eq!(effective_volume(0.64, true), 0.0);
}

#[test]
fn restores_a_safe_nonzero_volume_after_muting() {
    assert!((restored_volume(0.42) - 0.42).abs() < f32::EPSILON);
    assert!((restored_volume(0.0) - 0.7).abs() < f32::EPSILON);
    assert!((restored_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
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
fn scrobbles_at_half_duration_or_four_minutes() {
    assert!(!should_scrobble(
        Duration::from_secs(10),
        Some(Duration::from_secs(30))
    ));
    assert!(should_scrobble(
        Duration::from_secs(15),
        Some(Duration::from_secs(30))
    ));
    assert!(should_scrobble(
        Duration::from_secs(241),
        Some(Duration::from_secs(600))
    ));
    assert!(should_scrobble(Duration::from_secs(240), None));
    assert!(!should_scrobble(Duration::from_secs(120), None));
    assert!(!should_scrobble(
        Duration::ZERO,
        Some(Duration::from_secs(300))
    ));
}

#[test]
fn replay_gain_applies_db_gain_in_track_mode() {
    // -3.5 dB ≈ 0.668；0 dB = 1.0；+6 dB ≈ 1.995
    let factor = replay_gain_factor(VolumeNormalization::Track, Some(-3.5), None, None, None);
    assert!((factor - 10f32.powf(-3.5 / 20.0)).abs() < 0.001);
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, Some(0.0), None, None, None),
        1.0
    );
    let boost = replay_gain_factor(VolumeNormalization::Track, Some(6.0), None, None, None);
    assert!((boost - 10f32.powf(6.0 / 20.0)).abs() < 0.001);
}

#[test]
fn replay_gain_album_mode_uses_album_values() {
    let factor = replay_gain_factor(VolumeNormalization::Album, None, None, Some(-2.0), None);
    assert!((factor - 10f32.powf(-2.0 / 20.0)).abs() < 0.001);
    // Track 模式下 album 增益不生效。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, None, None, Some(-2.0), None),
        1.0
    );
}

#[test]
fn replay_gain_off_ignores_metadata_and_prevents_clipping() {
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Off, Some(-8.0), None, None, None),
        1.0
    );
    // 缺失增益数据时安全回退为 1.0。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, None, Some(0.9), None, None),
        1.0
    );
    // 峰值防削波：增益被限制在 1/peak。
    let clamped = replay_gain_factor(
        VolumeNormalization::Track,
        Some(10.0),
        Some(0.8),
        None,
        None,
    );
    assert!((clamped - 1.25).abs() < 0.001);
    // 结果始终落在安全范围内。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, Some(100.0), None, None, None),
        4.0
    );
}

#[test]
fn measured_loudness_maps_rms_to_gain() {
    // 目标 -16 dBFS：rms -12 → 增益 -4 dB ≈ 0.631；rms -24 → +8 dB ≈ 2.512
    let loud = loudness_gain_factor(-12.0, 0.0);
    assert!((loud - 10f32.powf(-4.0 / 20.0)).abs() < 0.001);
    let quiet = loudness_gain_factor(-24.0, 0.0);
    assert!((quiet - 10f32.powf(8.0 / 20.0)).abs() < 0.001);
    // 峰值防削波：峰值 0.9 时增益不超过 1/0.9。
    let clamped = loudness_gain_factor(-24.0, 0.9);
    assert!((clamped - (1.0 / 0.9)).abs() < 0.001);
}

#[test]
fn playback_mode_defaults_to_sequential_and_cycles_in_order() {
    assert_eq!(PlaybackMode::default(), PlaybackMode::Sequential);
    assert_eq!(PlaybackMode::Sequential.next(), PlaybackMode::RepeatAll);
    assert_eq!(PlaybackMode::RepeatAll.next(), PlaybackMode::RepeatOne);
    assert_eq!(PlaybackMode::RepeatOne.next(), PlaybackMode::Shuffle);
    assert_eq!(PlaybackMode::Shuffle.next(), PlaybackMode::Sequential);
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
fn album_pagination_offsets_are_contiguous() {
    assert_eq!(album_page_offset(0, 100), 0);
    assert_eq!(album_page_offset(1, 100), 100);
    assert_eq!(album_page_offset(2, 100), 200);
    assert_eq!(album_page_offset(3, 50), 150);
}

#[test]
fn shortcut_mapping_covers_playback_controls() {
    let none = Modifiers::default();
    assert_eq!(match_shortcut("space", none), Shortcut::TogglePlayback);
    assert_eq!(match_shortcut("left", none), Shortcut::SeekBack);
    assert_eq!(match_shortcut("right", none), Shortcut::SeekForward);
    assert_eq!(match_shortcut("up", none), Shortcut::VolumeUp);
    assert_eq!(match_shortcut("down", none), Shortcut::VolumeDown);
    assert_eq!(match_shortcut("m", none), Shortcut::ToggleMute);
    assert_eq!(match_shortcut("l", none), Shortcut::ToggleNowPlaying);
    assert_eq!(match_shortcut("q", none), Shortcut::ToggleQueue);
    assert_eq!(match_shortcut("escape", none), Shortcut::CloseOverlays);
    assert_eq!(match_shortcut("x", none), Shortcut::None);
}

#[test]
fn shortcut_mapping_uses_ctrl_for_navigation_and_search() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    assert_eq!(match_shortcut("f", ctrl), Shortcut::FocusSearch);
    assert_eq!(match_shortcut("left", ctrl), Shortcut::Previous);
    assert_eq!(match_shortcut("right", ctrl), Shortcut::Next);
    assert_eq!(match_shortcut("space", ctrl), Shortcut::None);
}

#[test]
fn shortcut_mapping_selects_list_rows_with_shift_and_plays_with_enter() {
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let none = Modifiers::default();
    assert_eq!(match_shortcut("up", shift), Shortcut::SelectPrevious);
    assert_eq!(match_shortcut("down", shift), Shortcut::SelectNext);
    // 裸方向键仍是音量控制，不被 Shift 导航占用。
    assert_eq!(match_shortcut("up", none), Shortcut::VolumeUp);
    assert_eq!(match_shortcut("down", none), Shortcut::VolumeDown);
    assert_eq!(match_shortcut("enter", none), Shortcut::PlaySelected);
    assert_eq!(match_shortcut("enter", shift), Shortcut::None);
}

#[test]
fn seek_target_clamps_to_track_bounds() {
    let duration = Some(Duration::from_secs(180));
    assert_eq!(
        seek_target(Duration::from_secs(3), duration, -5),
        Duration::ZERO
    );
    assert_eq!(
        seek_target(Duration::from_secs(178), duration, 5),
        Duration::from_secs(180)
    );
    assert_eq!(
        seek_target(Duration::from_secs(60), duration, 5),
        Duration::from_secs(65)
    );
    assert_eq!(
        seek_target(Duration::from_secs(60), duration, -5),
        Duration::from_secs(55)
    );
}
