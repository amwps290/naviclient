use crate::listening_stats::log_store::is_qualified_play;
use crate::listening_stats::{aggregate_insights, ListeningLog, ListeningRange};
use crate::models::{ListeningEvent, Song};
use crate::source::{ProviderKind, SourceId};

fn sample_song(id: &str, title: &str, artist: &str, album: &str, genre: &str, dur: i32) -> Song {
    Song {
        source: Some(SourceId::new(ProviderKind::LocalFolder, "local-1")),
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        genre: Some(genre.to_string()),
        duration: Some(dur),
        ..Default::default()
    }
}

#[test]
fn qualification_threshold_accepts_completed_30s_or_half_short_track() {
    assert!(!is_qualified_play(0, Some(180), false));
    assert!(!is_qualified_play(15, Some(180), false));
    assert!(is_qualified_play(30, Some(180), false));
    assert!(is_qualified_play(10, Some(180), true));
    // Short 20s interlude: 10s >= 50%
    assert!(is_qualified_play(10, Some(20), false));
    assert!(!is_qualified_play(9, Some(20), false));
}

#[test]
fn jsonl_log_persists_deduplicates_and_merges_legacy_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("listening-log.jsonl");
    let mut log = ListeningLog::open(path.clone());
    assert!(log.entries().is_empty());

    let song = sample_song("s1", "Track 1", "Artist A", "Album 1", "Jazz", 200);
    let short_skip = ListeningEvent {
        song: song.clone(),
        started_at: 1_700_000_000,
        listened_seconds: 5,
        completed: false,
    };
    assert!(!log.record_event(&short_skip).unwrap());

    let valid = ListeningEvent {
        song: song.clone(),
        started_at: 1_700_000_100,
        listened_seconds: 120,
        completed: true,
    };
    assert!(log.record_event(&valid).unwrap());
    // Duplicate (same timestamp + song_id) is ignored.
    assert!(!log.record_event(&valid).unwrap());

    // Reopen from disk.
    let reopened = ListeningLog::open(path);
    assert_eq!(reopened.entries().len(), 1);
    assert_eq!(reopened.entries()[0].listened_secs, 120);

    // Merging with legacy in-memory history deduplicates existing entry and adds new ones.
    let older = ListeningEvent {
        song: sample_song("s2", "Track 2", "Artist B", "Album 2", "Rock", 180),
        started_at: 1_699_999_000,
        listened_seconds: 95,
        completed: false,
    };
    let merged = reopened.merged_entries(&[valid, older]);
    assert_eq!(merged.len(), 2);
}

#[test]
fn aggregation_computes_kpis_rankings_and_time_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = ListeningLog::open(dir.path().join("log.jsonl"));

    let now = 1_760_000_000u64;
    let s1 = sample_song("s1", "Song One", "Artist A", "Album A", "Electronic", 200);
    let s2 = sample_song("s2", "Song Two", "Artist A", "Album A", "Electronic", 180);
    let s3 = sample_song("s3", "Old Track", "Artist B", "Album B", "Classical", 300);

    // Two recent plays (within 7 days)
    log.record_event(&ListeningEvent {
        song: s1.clone(),
        started_at: now - 3_600,
        listened_seconds: 200,
        completed: true,
    })
    .unwrap();
    log.record_event(&ListeningEvent {
        song: s1.clone(),
        started_at: now - 7_200,
        listened_seconds: 150,
        completed: false,
    })
    .unwrap();
    log.record_event(&ListeningEvent {
        song: s2,
        started_at: now - 2 * 86_400,
        listened_seconds: 180,
        completed: true,
    })
    .unwrap();
    // One older play (45 days ago -> outside 30d, inside AllTime)
    log.record_event(&ListeningEvent {
        song: s3,
        started_at: now - 45 * 86_400,
        listened_seconds: 300,
        completed: true,
    })
    .unwrap();

    let week = aggregate_insights(log.entries(), ListeningRange::Last7Days, now);
    assert_eq!(week.play_count, 3);
    assert_eq!(week.completed_count, 2);
    assert_eq!(week.total_listened_secs, 530);
    assert_eq!(week.unique_songs, 2);
    assert_eq!(week.unique_artists, 1);
    assert_eq!(week.top_songs[0].song.id, "s1");
    assert_eq!(week.top_songs[0].play_count, 2);
    assert_eq!(week.top_artists[0].name, "Artist A");
    assert_eq!(week.top_genres[0].name, "Electronic");
    assert!(week.peak_hour.is_some());

    let all = aggregate_insights(log.entries(), ListeningRange::AllTime, now);
    assert_eq!(all.play_count, 4);
    assert_eq!(all.total_listened_secs, 830);
    assert_eq!(all.unique_artists, 2);
    assert_eq!(all.unique_albums, 2);
    assert_eq!(all.top_genres.len(), 2);
}
