//! 应用层纯逻辑测试（从 app.rs 拆出）。

use std::collections::VecDeque;
use std::time::Duration;

use gpui_kit::hsla;

use super::{
    accent_foreground, advance_index, album_page_offset, contrast_ratio, cover_request_needed,
    effective_volume, extract_cover_palette, format_file_size, global_shortcut_conflict,
    history_index_after_insert, loudness_gain_factor, match_shortcut, normalize_volume,
    playback_technical_info, queue_index_after_move, queue_index_after_remove, range_anchor,
    readable_accent, reconnect_decision, replay_gain_factor, restored_volume, seek_target,
    selection_range, shortcut_conflict, should_scrobble, song_row_click_action, sort_album_songs,
    sort_artists, touch_lru, ReconnectDecision, ShuffleHistory, SongRowClickAction,
};
use crate::audio::StreamFailure;
use crate::models::{
    Artist, ArtistSort, GlobalShortcutSettings, PlayModes, RepeatMode, ShortcutAction,
    ShortcutSettings, Song, TranscodingQuality, VolumeNormalization,
};
use gpui_kit::{Keystroke, Modifiers};

fn default_shortcut(key: &str, modifiers: Modifiers) -> Option<ShortcutAction> {
    match_shortcut(
        &ShortcutSettings::default(),
        &Keystroke {
            key: key.to_string(),
            modifiers,
            key_char: None,
        },
    )
}

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
fn album_pagination_offsets_are_contiguous() {
    assert_eq!(album_page_offset(0, 100), 0);
    assert_eq!(album_page_offset(1, 100), 100);
    assert_eq!(album_page_offset(2, 100), 200);
    assert_eq!(album_page_offset(3, 50), 150);
}

#[test]
fn shortcut_mapping_covers_playback_controls() {
    let none = Modifiers::default();
    assert_eq!(
        default_shortcut("space", none),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        default_shortcut("left", none),
        Some(ShortcutAction::SeekBack)
    );
    assert_eq!(
        default_shortcut("right", none),
        Some(ShortcutAction::SeekForward)
    );
    assert_eq!(default_shortcut("up", none), Some(ShortcutAction::VolumeUp));
    assert_eq!(
        default_shortcut("down", none),
        Some(ShortcutAction::VolumeDown)
    );
    assert_eq!(
        default_shortcut("m", none),
        Some(ShortcutAction::ToggleMute)
    );
    assert_eq!(
        default_shortcut("l", none),
        Some(ShortcutAction::ToggleNowPlaying)
    );
    assert_eq!(
        default_shortcut("q", none),
        Some(ShortcutAction::ToggleQueue)
    );
    assert_eq!(
        default_shortcut("escape", none),
        Some(ShortcutAction::CloseOverlays)
    );
    assert_eq!(default_shortcut("x", none), None);
}

#[test]
fn shortcut_mapping_uses_ctrl_for_navigation_and_search() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    assert_eq!(
        default_shortcut("f", ctrl),
        Some(ShortcutAction::FocusSearch)
    );
    assert_eq!(
        default_shortcut("left", ctrl),
        Some(ShortcutAction::Previous)
    );
    assert_eq!(default_shortcut("right", ctrl), Some(ShortcutAction::Next));
    assert_eq!(default_shortcut("space", ctrl), None);
}

#[test]
fn shortcut_mapping_selects_list_rows_with_shift_and_plays_with_enter() {
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let none = Modifiers::default();
    assert_eq!(
        default_shortcut("up", shift),
        Some(ShortcutAction::SelectPrevious)
    );
    assert_eq!(
        default_shortcut("down", shift),
        Some(ShortcutAction::SelectNext)
    );
    // 裸方向键仍是音量控制，不被 Shift 导航占用。
    assert_eq!(default_shortcut("up", none), Some(ShortcutAction::VolumeUp));
    assert_eq!(
        default_shortcut("down", none),
        Some(ShortcutAction::VolumeDown)
    );
    assert_eq!(
        default_shortcut("enter", none),
        Some(ShortcutAction::PlaySelected)
    );
    assert_eq!(default_shortcut("enter", shift), None);
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

fn test_artist(name: &str, album_count: i32) -> Artist {
    Artist {
        id: name.to_string(),
        name: name.to_string(),
        album_count: Some(album_count),
        cover_art: None,
    }
}

#[test]
fn artist_sort_name_is_case_insensitive_ascending() {
    let mut artists = vec![
        test_artist("beatles", 1),
        test_artist("Adele", 1),
        test_artist("coldplay", 1),
    ];
    sort_artists(&mut artists, ArtistSort::Name);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Adele", "beatles", "coldplay"]);
}

#[test]
fn artist_sort_name_desc_reverses_ascending_order() {
    let mut artists = vec![
        test_artist("Adele", 1),
        test_artist("beatles", 1),
        test_artist("coldplay", 1),
    ];
    sort_artists(&mut artists, ArtistSort::NameDesc);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["coldplay", "beatles", "Adele"]);
}

#[test]
fn artist_sort_album_count_orders_by_size_then_name() {
    let mut artists = vec![
        test_artist("Adele", 4),
        test_artist("coldplay", 9),
        test_artist("beatles", 9),
        test_artist("unknown", 0),
    ];
    sort_artists(&mut artists, ArtistSort::AlbumCount);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["beatles", "coldplay", "Adele", "unknown"]);
}

#[test]
fn artist_sort_handles_missing_album_counts() {
    let mut artists = vec![
        Artist {
            id: "a".to_string(),
            name: "A".to_string(),
            album_count: None,
            cover_art: None,
        },
        test_artist("B", 2),
    ];
    sort_artists(&mut artists, ArtistSort::AlbumCount);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["B", "A"]);
}

fn album_song(title: &str, disc: Option<i32>, track: Option<i32>) -> Song {
    Song {
        id: title.to_string(),
        title: title.to_string(),
        disc,
        track,
        ..Song::default()
    }
}

#[test]
fn album_songs_sort_by_disc_then_track() {
    let mut songs = vec![
        album_song("B1", Some(2), Some(1)),
        album_song("A3", Some(1), Some(3)),
        album_song("A1", Some(1), Some(1)),
        album_song("A2", Some(1), Some(2)),
        album_song("B2", Some(2), Some(2)),
    ];
    sort_album_songs(&mut songs);
    let titles: Vec<&str> = songs.iter().map(|song| song.title.as_str()).collect();
    assert_eq!(titles, ["A1", "A2", "A3", "B1", "B2"]);
}

#[test]
fn album_songs_without_disc_or_track_sort_last_and_stably() {
    let mut songs = vec![
        album_song("Unknown", None, None),
        album_song("Track 2", None, Some(2)),
        album_song("Track 1", Some(1), Some(1)),
    ];
    sort_album_songs(&mut songs);
    let titles: Vec<&str> = songs.iter().map(|song| song.title.as_str()).collect();
    // 缺失碟号/曲目号的排在有效条目之后；Track 2 虽有曲目号但无碟号，
    // 与 Unknown 一样进入“碟号缺失”分组，再按（曲目号, 标题）排。
    assert_eq!(titles, ["Track 1", "Track 2", "Unknown"]);
}

#[gpui_pre::test]
fn light_theme_hint_contrast_is_darkened_for_readability(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};

        // 基线：框架默认亮色主题的提示灰（neutral-500）在小字号下不足。
        Theme::change(ThemeMode::Light, None, cx);
        let white = hsla(0.0, 0.0, 1.0, 1.0);
        let before = contrast_ratio(cx.theme().colors.muted_foreground, white);
        assert!(
            (4.0..5.0).contains(&before),
            "基线应为 neutral-500（约 4.7:1），实际 {before}"
        );

        // 修正后：亮色提示灰加深到 neutral-600（约 7.4:1），小字号也清晰。
        crate::app::adjust_light_theme_contrast(cx);
        let after = contrast_ratio(cx.theme().colors.muted_foreground, white);
        assert!(
            (7.0..8.0).contains(&after),
            "修正后应为 neutral-600（约 7.4:1），实际 {after}"
        );
        assert!(after > before);

        // 幂等：重复调用（如后续启动路径复用）不改变结果。
        crate::app::adjust_light_theme_contrast(cx);
        assert_eq!(
            contrast_ratio(cx.theme().colors.muted_foreground, white),
            after
        );

        // 深色主题不受影响：切换到 Dark 后提示灰是另一套值。
        Theme::change(ThemeMode::Dark, None, cx);
        assert_ne!(
            contrast_ratio(cx.theme().colors.muted_foreground, white),
            after
        );
    });
}

#[test]
fn shortcut_mapping_selects_all_with_ctrl_a_only() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    let none = Modifiers::default();
    assert_eq!(default_shortcut("a", ctrl), Some(ShortcutAction::SelectAll));
    // 无修饰或 Shift 组合不触发全选。
    assert_eq!(default_shortcut("a", none), None);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(default_shortcut("a", shift), None);
    // 既有 Ctrl 快捷键不受影响。
    assert_eq!(
        default_shortcut("f", ctrl),
        Some(ShortcutAction::FocusSearch)
    );
}

#[test]
fn custom_shortcut_replaces_default_and_reports_conflicts() {
    let mut settings = ShortcutSettings::default();
    settings.set(ShortcutAction::TogglePlayback, "ctrl-p".to_string());

    assert_eq!(
        match_shortcut(&settings, &Keystroke::parse("ctrl-p").unwrap()),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        match_shortcut(&settings, &Keystroke::parse("space").unwrap()),
        None
    );
    assert_eq!(
        shortcut_conflict(&settings, ShortcutAction::Next, "ctrl-p"),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        shortcut_conflict(&settings, ShortcutAction::Next, "ctrl-n"),
        None
    );

    settings.set(ShortcutAction::Next, "space".to_string());
    assert_eq!(
        shortcut_conflict(
            &settings,
            ShortcutAction::TogglePlayback,
            ShortcutAction::TogglePlayback.default_binding()
        ),
        Some(ShortcutAction::Next)
    );
}

#[test]
fn global_shortcut_conflicts_are_scoped_to_global_playback_bindings() {
    let mut settings = GlobalShortcutSettings::default();
    settings.set(ShortcutAction::Next, "ctrl-alt-n".to_string());

    assert_eq!(
        global_shortcut_conflict(&settings, ShortcutAction::Previous, "ctrl-alt-n"),
        Some(ShortcutAction::Next)
    );
    assert_eq!(
        global_shortcut_conflict(&settings, ShortcutAction::Previous, "ctrl-alt-p"),
        None
    );
}

#[test]
fn selection_range_covers_anchor_to_clicked_in_order() {
    // 正向
    assert_eq!(
        selection_range(2, 5),
        [2, 3, 4, 5]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    // 反向（点击行在锚点之前）
    assert_eq!(
        selection_range(7, 3),
        [3, 4, 5, 6, 7]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    // 同一行
    assert_eq!(
        selection_range(4, 4),
        [4].into_iter().collect::<std::collections::BTreeSet<_>>()
    );
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

#[test]
fn range_anchor_prefers_multi_anchor_then_cursor_then_fallback() {
    // 已有多选锚点优先：Shift 操作可围绕固定锚点连续扩展。
    assert_eq!(range_anchor(Some(4), Some(9), 2), 4);
    // 无多选时用键盘光标（Shift+点击 从光标拉范围）。
    assert_eq!(range_anchor(None, Some(9), 2), 9);
    // 两者皆无（新开多选）：锚点即本次目标行。
    assert_eq!(range_anchor(None, None, 2), 2);
}

#[test]
fn reconnect_only_retries_network_and_server_failures() {
    // 解码失败、本地输出问题重试也不会变好，直接交给用户。
    for kind in [
        StreamFailure::Decode,
        StreamFailure::Output,
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
fn cover_request_needed_never_burns_an_id_against_a_palette_revisit() {
    // 网格/队列预取：已有图像、不需要调色板 → 不请求。
    assert!(!cover_request_needed(true, false, false));
    assert!(!cover_request_needed(true, true, false));
    // 预取过的封面在播放时必须能补提调色板（历史 bug：只增不减的
    // 请求集合把 id 永久“烧掉”，播放页/侧边栏动态配色拿不到数据）。
    assert!(cover_request_needed(true, false, true));
    // 调色板已在 → 不重复请求。
    assert!(!cover_request_needed(true, true, true));
    // 图像还没有 → 总要请求。
    assert!(cover_request_needed(false, false, false));
    assert!(cover_request_needed(false, false, true));
    assert!(cover_request_needed(false, true, false));
}

#[test]
fn cover_texture_lru_refreshes_usage_and_evicts_the_oldest() {
    let mut lru = VecDeque::new();
    assert_eq!(touch_lru(&mut lru, 1, 3), None);
    assert_eq!(touch_lru(&mut lru, 2, 3), None);
    assert_eq!(touch_lru(&mut lru, 3, 3), None);
    assert_eq!(lru, VecDeque::from([3, 2, 1]));

    assert_eq!(touch_lru(&mut lru, 1, 3), None);
    assert_eq!(lru, VecDeque::from([1, 3, 2]));
    assert_eq!(touch_lru(&mut lru, 4, 3), Some(2));
    assert_eq!(lru, VecDeque::from([4, 1, 3]));
}

#[test]
fn song_rows_select_on_single_click_and_play_on_double_click() {
    assert_eq!(
        song_row_click_action(Modifiers::default(), 1),
        SongRowClickAction::Select
    );
    assert_eq!(
        song_row_click_action(Modifiers::default(), 2),
        SongRowClickAction::Play
    );
    assert_eq!(
        song_row_click_action(
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
            1,
        ),
        SongRowClickAction::ToggleMulti
    );
    assert_eq!(
        song_row_click_action(
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            1,
        ),
        SongRowClickAction::RangeMulti
    );
}
