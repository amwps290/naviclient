use super::*;
use crate::app::playback::occurrence::Selection;
use crate::app::playback::start::select_state;
use crate::source::{MediaKey, ProviderKind, SourceId};

#[test]
fn browsing_reset_keeps_the_player_queue_lyrics_pending_artwork_and_controls() {
    let source = SourceId::new(ProviderKind::OpenSubsonic, "playing");
    let song = Song {
        id: "same".into(),
        source: Some(source.clone()),
        cover_art: Some("cover".into()),
        ..Default::default()
    };
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(vec![song.clone(), song.clone()], QueueOrigin::Playlist),
        queue_drawer_open: true,
        playback_modes: PlayModes {
            shuffle: true,
            repeat: RepeatMode::All,
        },
        ..Default::default()
    };
    select_state(
        &mut state,
        1,
        &song,
        TranscodingQuality::Original,
        Selection::New,
    );
    state.shuffle_history.start(1);
    let entries: Vec<_> = state.queue.entries().iter().map(QueueEntry::id).collect();
    let occurrence = state.occurrence.id();
    state.song_selection.toggle(SongScope::Queue, 1, 2);
    let mut requests = LibraryRequests::default();
    let lyrics = state
        .lyrics
        .begin(&MediaKey::new(source.clone(), "same"), &mut requests)
        .unwrap();
    let cover = state
        .covers
        .begin(&source, "cover", false, &mut requests)
        .unwrap();
    state.view = View::NowPlaying;
    state.reset_browsing();
    assert_eq!(state.view, View::Home);
    assert_eq!(
        state
            .queue
            .entries()
            .iter()
            .map(QueueEntry::id)
            .collect::<Vec<_>>(),
        entries
    );
    assert_eq!(state.occurrence.id(), occurrence);
    assert!(state.occurrence.active());
    assert_eq!(state.queue_index, Some(1));
    assert_eq!(state.now_playing.as_ref().unwrap().source, Some(source));
    assert_eq!(state.shuffle_history.snapshot(), (vec![1], vec![]));
    assert!(state.playback_modes.shuffle);
    assert_eq!(state.playback_modes.repeat, RepeatMode::All);
    assert!(state.queue_drawer_open);
    assert!(state.song_selection.contains(SongScope::Queue, 1));
    assert!(state
        .lyrics
        .complete(lyrics, Ok(crate::models::Lyrics::default())));
    assert!(state
        .covers
        .complete(&cover, Err("request still belongs to the player".into()))
        .is_some());
}

#[test]
fn browsing_reset_discards_page_selection_and_old_page_requests() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let favorite = state.favorites.begin_load(&mut requests);
    state.song_selection.toggle(SongScope::Favorites, 0, 1);
    state.reset_browsing();
    assert!(state.song_selection.cursor().is_none());
    assert!(state.song_selection.multi().is_none());
    assert!(state
        .favorites
        .complete_load(favorite, Ok(Favorites::default()))
        .is_none());
}
