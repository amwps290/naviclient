use super::*;

struct Polls {
    start: Instant,
    redraw: PlaybackRedrawState,
}

impl Polls {
    fn new() -> Self {
        let start = Instant::now();
        Self {
            start,
            redraw: PlaybackRedrawState::new(start),
        }
    }

    fn visible(playback: &PlaybackState) -> Self {
        let mut polls = Self::new();
        assert!(polls.at(0, request(playback)));
        polls
    }

    fn at(&mut self, milliseconds: u64, request: RedrawRequest) -> bool {
        self.redraw
            .should_redraw(self.start + Duration::from_millis(milliseconds), request)
    }
}

fn request(playback: &PlaybackState) -> RedrawRequest {
    RedrawRequest {
        playback: playback.clone(),
        presentation: PlaybackPresentation::Standard,
        visible: true,
        changed: false,
        scrolling: false,
    }
}

fn playing() -> PlaybackState {
    PlaybackState {
        active: true,
        duration: Some(Duration::from_secs(120)),
        ..PlaybackState::default()
    }
}

#[test]
fn first_visible_poll_renders_even_without_active_playback() {
    let playback = PlaybackState::default();
    let mut polls = Polls::new();
    assert!(polls.at(50, request(&playback)));
    assert!(!polls.at(100, request(&playback)));
}

#[test]
fn unchanged_playback_does_not_redraw_on_a_timer() {
    for playback in [PlaybackState::default(), playing()] {
        let mut polls = Polls::visible(&playback);
        for milliseconds in [50, 250, 500, 60_000] {
            assert!(!polls.at(milliseconds, request(&playback)));
        }
    }
}

#[test]
fn standard_view_retains_clock_change_until_cadence_expires() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.position = Duration::from_millis(50);
    assert!(!polls.at(50, request(&playback)));
    assert!(!polls.at(249, request(&playback)));
    assert!(polls.at(250, request(&playback)));
    assert!(!polls.at(500, request(&playback)));
}

#[test]
fn now_playing_clock_uses_the_short_cadence() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.position = Duration::from_millis(49);
    let now_playing = |playback: &PlaybackState| RedrawRequest {
        presentation: PlaybackPresentation::NowPlaying,
        ..request(playback)
    };
    assert!(!polls.at(49, now_playing(&playback)));
    assert!(polls.at(50, now_playing(&playback)));
    playback.position = Duration::from_millis(99);
    assert!(!polls.at(99, now_playing(&playback)));
    assert!(polls.at(100, now_playing(&playback)));
}

#[test]
fn opening_now_playing_uses_pending_clock_change_at_its_faster_cadence() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.position = Duration::from_millis(100);
    assert!(!polls.at(100, request(&playback)));
    assert!(polls.at(
        110,
        RedrawRequest {
            presentation: PlaybackPresentation::NowPlaying,
            ..request(&playback)
        }
    ));
}

#[test]
fn leaving_now_playing_restores_the_standard_cadence() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.position = Duration::from_millis(49);
    assert!(!polls.at(
        49,
        RedrawRequest {
            presentation: PlaybackPresentation::NowPlaying,
            ..request(&playback)
        }
    ));
    assert!(!polls.at(50, request(&playback)));
    assert!(polls.at(250, request(&playback)));
}

#[test]
fn message_redraw_restarts_clock_cadence_and_consumes_pending_changes() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.position = Duration::from_millis(50);
    assert!(!polls.at(50, request(&playback)));
    assert!(polls.at(
        60,
        RedrawRequest {
            changed: true,
            ..request(&playback)
        }
    ));
    assert!(!polls.at(250, request(&playback)));
    playback.position = Duration::from_millis(300);
    assert!(!polls.at(300, request(&playback)));
    assert!(polls.at(310, request(&playback)));
}

#[test]
fn hidden_startup_waits_until_visible_even_after_cadence_expires() {
    let playback = playing();
    let mut polls = Polls::new();
    for milliseconds in [50, 250, 60_000] {
        assert!(!polls.at(
            milliseconds,
            RedrawRequest {
                visible: false,
                ..request(&playback)
            }
        ));
    }
    assert!(polls.at(60_001, request(&playback)));
}

#[test]
fn hidden_window_suppresses_playback_and_scroll_redraws() {
    let mut playback = playing();
    let mut polls = Polls::visible(&playback);
    playback.paused = true;
    playback.position = Duration::from_secs(12);
    assert!(!polls.at(
        60_000,
        RedrawRequest {
            visible: false,
            scrolling: true,
            ..request(&playback)
        }
    ));
    assert!(polls.at(60_001, request(&playback)));
    assert!(!polls.at(60_002, request(&playback)));
}

#[test]
fn messages_and_commands_still_notify_while_hidden() {
    let playback = PlaybackState::default();
    let mut polls = Polls::new();
    assert!(polls.at(
        1,
        RedrawRequest {
            visible: false,
            changed: true,
            ..request(&playback)
        }
    ));
    assert!(!polls.at(
        2,
        RedrawRequest {
            visible: false,
            ..request(&playback)
        }
    ));
    assert!(polls.at(3, request(&playback)));
}

#[test]
fn showing_window_again_renders_unchanged_playback_immediately() {
    let playback = playing();
    let mut polls = Polls::visible(&playback);
    assert!(!polls.at(
        1,
        RedrawRequest {
            visible: false,
            ..request(&playback)
        }
    ));
    assert!(polls.at(2, request(&playback)));
    assert!(!polls.at(3, request(&playback)));
}

#[test]
fn transport_changes_render_immediately_before_clock_cadence() {
    let baseline = playing();
    let mut variants = vec![baseline.clone(); 6];
    variants[0].active = false;
    variants[1].paused = true;
    variants[2].ended = true;
    variants[3].error = Some("stream unavailable".to_string());
    variants[4].duration = Some(Duration::from_secs(180));
    variants[5].transition_id = Some(7);

    for playback in variants {
        let mut polls = Polls::visible(&baseline);
        assert!(polls.at(1, request(&playback)), "{playback:?}");
        assert!(!polls.at(2, request(&playback)), "{playback:?}");
    }
}

#[test]
fn buffer_seek_and_diagnostic_changes_follow_clock_cadence() {
    let baseline = playing();
    let mut variants = vec![baseline.clone(); 9];
    variants[0].buffered = Duration::from_secs(10);
    variants[1].fully_buffered = true;
    variants[2].seek_serial = 2;
    variants[3].stall = Some(Duration::from_secs(3));
    variants[4].loudness_db = -12.0;
    variants[5].loudness_peak = 0.8;
    variants[6].output_name = Some("headphones".to_string());
    variants[7].output_sample_rate = 48_000;
    variants[8].output_channels = 2;

    for playback in variants {
        let mut polls = Polls::visible(&baseline);
        assert!(!polls.at(1, request(&playback)), "{playback:?}");
        assert!(polls.at(250, request(&playback)), "{playback:?}");
    }
}

#[test]
fn reverting_a_clock_change_keeps_a_pending_redraw() {
    let baseline = playing();
    let mut polls = Polls::visible(&baseline);
    let mut changed = baseline.clone();
    changed.buffered = Duration::from_secs(3);
    assert!(!polls.at(50, request(&changed)));
    assert!(!polls.at(100, request(&baseline)));
    assert!(polls.at(250, request(&baseline)));
    assert!(!polls.at(500, request(&baseline)));
}

#[test]
fn lyric_scroll_redraws_paused_playback_until_scroll_finishes() {
    let playback = PlaybackState {
        paused: true,
        ..playing()
    };
    let mut polls = Polls::visible(&playback);
    for milliseconds in [1, 2, 3] {
        assert!(polls.at(
            milliseconds,
            RedrawRequest {
                scrolling: true,
                ..request(&playback)
            }
        ));
    }
    assert!(!polls.at(4, request(&playback)));
    assert!(!polls.at(60_000, request(&playback)));
}

#[test]
fn settings_mini_and_browsing_views_use_standard_presentation() {
    assert_eq!(
        PlaybackPresentation::for_view(View::NowPlaying, false, false),
        PlaybackPresentation::NowPlaying
    );
    for view in [
        View::Home,
        View::Favorites,
        View::Artists,
        View::Albums,
        View::Genres,
        View::GenreDetail,
        View::Playlists,
        View::Search,
        View::ArtistDetail,
        View::AlbumDetail,
        View::PlaylistDetail,
    ] {
        for settings_open in [false, true] {
            for mini in [false, true] {
                assert_eq!(
                    PlaybackPresentation::for_view(view, settings_open, mini),
                    PlaybackPresentation::Standard
                );
            }
        }
    }
    for (settings_open, mini) in [(true, false), (false, true), (true, true)] {
        assert_eq!(
            PlaybackPresentation::for_view(View::NowPlaying, settings_open, mini),
            PlaybackPresentation::Standard
        );
    }
}

#[test]
fn cover_animation_requires_visible_active_unpaused_now_playing() {
    assert!(PlaybackPresentation::NowPlaying.animate_cover(&playing(), true));
    for (visible, active, paused) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
        (true, false, true),
        (false, false, false),
        (false, true, true),
        (false, false, true),
    ] {
        let playback = PlaybackState {
            active,
            paused,
            ..PlaybackState::default()
        };
        assert!(!PlaybackPresentation::NowPlaying.animate_cover(&playback, visible));
    }
    for visible in [false, true] {
        for active in [false, true] {
            for paused in [false, true] {
                let playback = PlaybackState {
                    active,
                    paused,
                    ..PlaybackState::default()
                };
                assert!(!PlaybackPresentation::Standard.animate_cover(&playback, visible));
            }
        }
    }
}
