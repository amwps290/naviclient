use super::*;
use crate::lyrics::service::LoadOutcome;
use crate::models::{LyricLine, Lyrics};
use crate::source::{ProviderKind, SourceId};
use gpui_kit::component::Root;
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

fn media(id: &str) -> MediaKey {
    MediaKey::new(SourceId::new(ProviderKind::OpenSubsonic, "fixture"), id)
}

fn line(start_ms: Option<u64>, text: &str) -> LyricLine {
    LyricLine {
        text: text.into(),
        start_ms,
        ..Default::default()
    }
}

/// Two sung lines at 1.2 s and 5 s, followed by an untimed credit.
fn document() -> Document {
    Document::native(Lyrics {
        lines: vec![
            line(Some(1200), "first"),
            line(Some(5000), "second"),
            line(None, "credits"),
        ],
        ..Default::default()
    })
}

fn word_document() -> Document {
    Document::native(crate::lyrics::yrc::parse(
        "[1000,3000](1000,1000,0)你(2000,1000,0)好(3000,1000,0)呀",
    ))
}

fn starts(document: &Document) -> Vec<Option<u64>> {
    document
        .display_lyrics()
        .lines
        .iter()
        .map(|line| line.start_ms)
        .collect()
}

fn cache(directory: &tempfile::TempDir) -> LyricsCache {
    LyricsCache::at(directory.path().join("cache.json"))
}

/// Song "a" after `document` loaded for it.
fn showing(document: Document) -> LyricsState {
    let mut requests = LibraryRequests::default();
    let mut lyrics = LyricsState::default();
    let request = lyrics.begin(&media("a"), &mut requests).unwrap();
    assert!(lyrics.finish(request, LoadOutcome::ready(document)));
    lyrics
}

fn open_bar() -> TimingBar {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    bar
}

fn offset(lyrics: &LyricsState) -> i64 {
    lyrics.document.as_ref().unwrap().offset_ms
}

#[test]
fn aligning_makes_the_clicked_line_start_at_the_playback_position() {
    let mut document = document();
    align(&mut document, 1, Duration::from_millis(7300));
    assert_eq!(document.offset_ms, 2300);
    assert_eq!(starts(&document), [Some(3500), Some(7300), None]);
}

#[test]
fn aligning_a_line_that_is_sung_before_its_timestamp_moves_lyrics_earlier() {
    let mut document = document();
    align(&mut document, 1, Duration::from_millis(3200));
    assert_eq!(document.offset_ms, -1800);
    assert_eq!(starts(&document), [Some(0), Some(3200), None]);
}

#[test]
fn aligning_replaces_the_previous_offset_instead_of_adding_to_it() {
    let mut document = document();
    document.offset_ms = -3000;
    // The first line is displayed at zero, 1.8 s later than its shifted time.
    assert_eq!(starts(&document)[0], Some(0));
    align(&mut document, 0, Duration::from_millis(2000));
    assert_eq!(document.offset_ms, 800);
    assert_eq!(starts(&document), [Some(2000), Some(5800), None]);
}

#[test]
fn aligning_snaps_to_hundredths_of_a_second() {
    for (position, offset) in [
        (7304, 2300),
        (7305, 2310),
        (5004, 0),
        (2996, -2000),
        (2995, -2010),
    ] {
        let mut document = document();
        align(&mut document, 1, Duration::from_millis(position));
        assert_eq!(document.offset_ms, offset, "position={position}");
    }
}

#[test]
fn aligning_follows_corrected_word_times() {
    let mut document = word_document();
    assert!(document.set_word_start(0, 0, 500));
    align(&mut document, 0, Duration::from_millis(2000));
    assert_eq!(document.offset_ms, 1500);
    assert_eq!(starts(&document), [Some(2000)]);
}

#[test]
fn aligning_an_untimed_or_missing_line_changes_nothing() {
    let mut document = document();
    document.offset_ms = 150;
    align(&mut document, 2, Duration::from_millis(9000));
    align(&mut document, 3, Duration::from_millis(9000));
    assert_eq!(document.offset_ms, 150);
}

#[test]
fn reset_restores_the_source_timing_including_word_corrections() {
    let mut document = word_document();
    assert!(!adjusted(&document));
    assert!(document.set_word_start(0, 1, 2300));
    assert!(adjusted(&document));
    document.offset_ms = 700;
    reset(&mut document);
    assert!(!adjusted(&document));
    assert_eq!(document.display_lyrics(), word_document().lyrics);
}

#[test]
fn an_offset_alone_counts_as_adjusted() {
    let mut document = document();
    document.offset_ms = -100;
    assert!(adjusted(&document));
}

#[test]
fn shift_label_names_the_direction_instead_of_showing_a_sign() {
    for (offset, seconds, direction) in [
        (300, "0.3", "later"),
        (-500, "0.5", "earlier"),
        (12_000, "12.0", "later"),
        (-2340, "2.34", "earlier"),
        (1234, "1.234", "later"),
        (-4, "0.004", "earlier"),
    ] {
        let label = shift_label(offset);
        assert!(
            label.starts_with(&format!("{seconds} ")) && label.ends_with(direction),
            "offset={offset} label={label}"
        );
    }
}

#[test]
fn shift_label_for_unshifted_lyrics_shows_no_amount() {
    let label = shift_label(0);
    assert!(!label.is_empty());
    assert!(!label.contains(|c: char| c.is_ascii_digit()), "{label}");
}

#[test]
fn bar_closes_when_another_song_starts_and_stays_closed_on_return() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    bar.sync(Some(&media("a")), true, true, None);
    assert!(bar.is_open());
    bar.sync(Some(&media("b")), true, true, None);
    assert!(!bar.is_open());
    bar.sync(Some(&media("a")), true, true, None);
    assert!(!bar.is_open());
}

#[test]
fn bar_closes_when_the_lyrics_are_no_longer_synced() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    assert!(bar.is_open());
    bar.sync(Some(&media("a")), false, true, None);
    assert!(!bar.is_open());
}

#[test]
fn bar_closes_when_playback_stops() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    assert!(bar.is_open());
    bar.sync(None, false, true, None);
    assert!(!bar.is_open());
}

#[test]
fn pending_alignment_ends_when_the_lyrics_leave_the_screen_but_the_bar_stays() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    bar.set_aligning(true);
    bar.sync(Some(&media("a")), true, true, None);
    assert!(bar.is_aligning());
    bar.sync(Some(&media("a")), true, false, None);
    assert!(!bar.is_aligning());
    assert!(bar.is_open());
}

#[test]
fn pending_alignment_ends_with_the_bar() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    bar.set_aligning(true);
    assert!(bar.is_aligning());
    bar.sync(Some(&media("b")), true, true, None);
    assert!(!bar.is_aligning());
    bar.open(media("b"));
    bar.set_aligning(true);
    assert!(bar.is_aligning());
    bar.close();
    assert!(!bar.is_aligning());
    assert!(!bar.is_open());
}

#[test]
fn alignment_cannot_be_armed_while_the_bar_is_closed() {
    let mut bar = TimingBar::default();
    bar.set_aligning(true);
    assert!(!bar.is_aligning());
}

#[test]
fn reopening_the_bar_does_not_resume_an_earlier_alignment() {
    let mut bar = TimingBar::default();
    bar.open(media("a"));
    bar.set_aligning(true);
    assert!(bar.is_aligning());
    bar.open(media("a"));
    assert!(bar.is_open());
    assert!(!bar.is_aligning());
}

#[test]
fn toggling_opens_the_bar_for_the_song_on_screen_and_closes_it_again() {
    let mut bar = TimingBar::default();
    bar.toggle(Some(&media("a")));
    bar.sync(Some(&media("a")), true, true, None);
    assert!(bar.is_open());
    bar.toggle(Some(&media("a")));
    assert!(!bar.is_open());
}

#[test]
fn nudges_add_up_and_are_remembered_for_the_song() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    for action in [
        TimingAction::Nudge(-100),
        TimingAction::Nudge(-100),
        TimingAction::Nudge(500),
    ] {
        bar.perform(action, &mut lyrics, &cache).unwrap();
    }
    assert_eq!(offset(&lyrics), 300);
    assert_eq!(
        starts(lyrics.document.as_ref().unwrap()),
        [Some(1500), Some(5300), None]
    );
    assert_eq!(cache.preference(&media("a")).offset_ms, 300);
    assert!(bar.is_open());
}

#[test]
fn reset_action_returns_the_song_to_its_source_timing() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    bar.perform(TimingAction::Nudge(500), &mut lyrics, &cache)
        .unwrap();
    bar.perform(TimingAction::Reset, &mut lyrics, &cache)
        .unwrap();
    assert_eq!(offset(&lyrics), 0);
    assert_eq!(cache.preference(&media("a")).offset_ms, 0);
}

#[test]
fn align_action_arms_the_next_click_and_a_second_press_disarms_it() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    bar.perform(TimingAction::ToggleAlign, &mut lyrics, &cache)
        .unwrap();
    assert!(bar.is_aligning());
    bar.perform(TimingAction::ToggleAlign, &mut lyrics, &cache)
        .unwrap();
    assert!(!bar.is_aligning());
    assert_eq!(offset(&lyrics), 0);
}

#[test]
fn close_action_closes_the_bar_and_keeps_the_offset() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    bar.perform(TimingAction::Nudge(500), &mut lyrics, &cache)
        .unwrap();
    bar.perform(TimingAction::Close, &mut lyrics, &cache)
        .unwrap();
    assert!(!bar.is_open());
    assert_eq!(offset(&lyrics), 500);
}

#[test]
fn an_armed_click_aligns_its_line_once_and_is_remembered() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    bar.set_aligning(true);
    let position = Duration::from_millis(7300);
    assert!(bar.align_click(1, position, &mut lyrics, &cache).unwrap());
    assert_eq!(offset(&lyrics), 2300);
    assert_eq!(cache.preference(&media("a")).offset_ms, 2300);
    assert!(!bar.is_aligning());
    assert!(bar.is_open());
    // The next click is an ordinary one again.
    let later = Duration::from_millis(9000);
    assert!(!bar.align_click(0, later, &mut lyrics, &cache).unwrap());
    assert_eq!(offset(&lyrics), 2300);
}

#[test]
fn a_click_is_left_to_seek_when_no_alignment_is_pending() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut lyrics = showing(document());
    let mut bar = open_bar();
    let position = Duration::from_millis(7300);
    assert!(!bar.align_click(1, position, &mut lyrics, &cache).unwrap());
    assert_eq!(offset(&lyrics), 0);
    assert!(cache.preference(&media("a")).media.is_none());
}

#[test]
fn alignment_holds_the_last_rendered_focus_until_it_is_cancelled() {
    let mut bar = open_bar();
    bar.sync(Some(&media("a")), true, true, Some(0));
    bar.set_aligning(true);
    // Playback and highlighting would otherwise move to the next long line.
    bar.sync(Some(&media("a")), true, true, Some(1));
    assert_eq!(bar.display_line(Some(1)), Some(0));
    bar.set_aligning(false);
    assert_eq!(bar.display_line(Some(1)), Some(1));
}

#[test]
fn alignment_started_during_the_intro_keeps_all_rows_unemphasized() {
    let mut bar = open_bar();
    bar.sync(Some(&media("a")), true, true, None);
    bar.set_aligning(true);
    bar.sync(Some(&media("a")), true, true, Some(0));
    assert_eq!(bar.display_line(Some(0)), None);
    bar.close();
    assert_eq!(bar.display_line(Some(0)), Some(0));
}

/// Hosts the real bar the way the player does, on plain state.
struct Probe {
    bar: TimingBar,
    lyrics: LyricsState,
    cache: LyricsCache,
    width: f32,
}

impl Probe {
    fn act(&mut self, action: TimingAction, cx: &mut Context<Self>) {
        self.bar
            .perform(action, &mut self.lyrics, &self.cache)
            .unwrap();
        cx.notify();
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lyrics = self.lyrics.lyrics();
        self.bar.sync(
            self.lyrics.media(),
            lyrics.is_some_and(|lyrics| lyrics.is_synced()),
            true,
            lyrics.and_then(|lyrics| lyrics.display_line_index(Duration::from_millis(2000))),
        );
        let bar = self
            .lyrics
            .document
            .as_ref()
            .filter(|_| self.bar.is_open())
            .map(|document| timing_bar(document, self.bar.is_aligning(), cx, Self::act));
        div().size_full().child(
            v_flex().w(px(self.width)).children(bar).child(
                div()
                    .id("lyrics-list-start")
                    .debug_selector(|| "lyrics-list-start".into())
                    .h(px(1.0)),
            ),
        )
    }
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn bar_on_screen(
    cx: &mut TestAppContext,
    directory: &tempfile::TempDir,
) -> (Entity<Probe>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let cache = cache(directory);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| Probe {
                bar: open_bar(),
                lyrics: showing(document()),
                cache,
                width: 500.0,
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    (probe.unwrap(), visual)
}

fn click(visual: &mut VisualTestContext, button: &'static str) {
    let bounds = visual
        .debug_bounds(button)
        .unwrap_or_else(|| panic!("{button} is not on screen"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    draw(visual);
}

#[gpui_pre::test]
fn each_step_button_shifts_the_song_by_its_label(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    for (button, total) in [
        ("lyrics-earlier-100", -100),
        ("lyrics-earlier-500", -600),
        ("lyrics-later-100", -500),
        ("lyrics-later-500", 0),
        ("lyrics-later-500", 500),
    ] {
        click(&mut visual, button);
        visual.update(|_, cx| assert_eq!(offset(&probe.read(cx).lyrics), total, "{button}"));
    }
}

#[gpui_pre::test]
fn reset_button_clears_the_shift(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    click(&mut visual, "lyrics-later-500");
    click(&mut visual, "lyrics-timing-reset");
    visual.update(|_, cx| assert_eq!(offset(&probe.read(cx).lyrics), 0));
}

#[gpui_pre::test]
fn reset_button_is_inert_while_the_song_is_unadjusted(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    // A timing change would cancel this refresh; a disabled button must not.
    visual.update(|_, cx| {
        probe.update(cx, |probe, _| {
            let mut requests = LibraryRequests::default();
            probe.lyrics.refresh(&media("a"), &mut requests);
        })
    });
    click(&mut visual, "lyrics-timing-reset");
    visual.update(|_, cx| assert!(probe.read(cx).lyrics.loading()));
}

#[gpui_pre::test]
fn align_button_arms_and_disarms_the_next_click(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    click(&mut visual, "lyrics-align");
    visual.update(|_, cx| assert!(probe.read(cx).bar.is_aligning()));
    click(&mut visual, "lyrics-align");
    visual.update(|_, cx| assert!(!probe.read(cx).bar.is_aligning()));
}

#[gpui_pre::test]
fn close_button_takes_the_bar_off_screen(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    click(&mut visual, "lyrics-timing-close");
    visual.update(|_, cx| assert!(!probe.read(cx).bar.is_open()));
    assert!(visual.debug_bounds("lyrics-align").is_none());
}

#[gpui_pre::test]
fn alignment_hint_keeps_the_list_in_place_on_narrow_and_wide_panels(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    for width in [320.0, 440.0, 740.0] {
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.width = width;
                cx.notify();
            });
        });
        draw(&mut visual);
        let before = visual.debug_bounds("lyrics-list-start").unwrap();
        click(&mut visual, "lyrics-align");
        assert_eq!(
            visual.debug_bounds("lyrics-list-start").unwrap(),
            before,
            "width={width}"
        );
        click(&mut visual, "lyrics-align");
        assert_eq!(
            visual.debug_bounds("lyrics-list-start").unwrap(),
            before,
            "width={width}"
        );
    }
}

#[gpui_pre::test]
fn changing_the_offset_does_not_move_the_step_buttons(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (probe, mut visual) = bar_on_screen(cx, &directory);
    let buttons = [
        "lyrics-earlier-500",
        "lyrics-earlier-100",
        "lyrics-later-100",
        "lyrics-later-500",
    ];
    let before = buttons.map(|button| visual.debug_bounds(button).unwrap());
    for offset in [1234, -12_345, 3_456_789, 0] {
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe
                    .lyrics
                    .change_timing(&probe.cache, |document| document.offset_ms = offset)
                    .unwrap();
                cx.notify();
            });
        });
        draw(&mut visual);
        for (button, expected) in buttons.iter().zip(&before) {
            assert_eq!(
                visual.debug_bounds(button).unwrap(),
                *expected,
                "offset={offset}, button={button}"
            );
        }
    }
}

struct EscapeProbe {
    timing: TimingBar,
    menu_open: bool,
    observed_escapes: usize,
    _subscriptions: Vec<Subscription>,
}

impl Render for EscapeProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let host = cx.entity().downgrade();
        div().size_full().child(
            Button::new("lyrics-escape-menu")
                .debug_selector(|| "lyrics-escape-menu".into())
                .label("⋯")
                .dropdown_menu(|menu, _, _| {
                    menu.item(gpui_kit::component::menu::PopupMenuItem::new(
                        "Lyrics actions",
                    ))
                })
                .on_open_change(move |open, _, cx| {
                    let _ = host.update(cx, |this, cx| {
                        this.menu_open = *open;
                        cx.notify();
                    });
                }),
        )
    }
}

#[gpui_pre::test]
fn escape_cancels_alignment_before_the_menu_and_app_shortcuts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|cx| {
                let interceptor =
                    timing_escape(cx, |this: &mut EscapeProbe| Some(&mut this.timing));
                let observer = cx.observe_keystrokes(|this: &mut EscapeProbe, event, _, _| {
                    if event.keystroke.key == "escape" {
                        this.observed_escapes += 1;
                    }
                });
                let mut timing = open_bar();
                timing.set_aligning(true);
                EscapeProbe {
                    timing,
                    menu_open: false,
                    observed_escapes: 0,
                    _subscriptions: vec![interceptor, observer],
                }
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    click(&mut visual, "lyrics-escape-menu");
    visual.update(|_, cx| assert!(probe.read(cx).menu_open));
    visual.simulate_keystrokes("escape");
    draw(&mut visual);
    visual.update(|_, cx| {
        let probe = probe.read(cx);
        assert!(!probe.timing.is_aligning());
        assert!(probe.menu_open);
        assert_eq!(probe.observed_escapes, 0);
    });
    // With no armed click, Escape returns to its normal focused-menu action.
    visual.simulate_keystrokes("escape");
    draw(&mut visual);
    visual.update(|_, cx| {
        let probe = probe.read(cx);
        assert!(!probe.menu_open);
        assert_eq!(probe.observed_escapes, 1);
    });
}
