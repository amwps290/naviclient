use super::*;
use crate::models::Song;
use crate::playlist::loading::{
    playlist_update_channel, PlaylistFailureKind, PlaylistProgress, PlaylistUpdate,
};
use crate::source::MusicSource;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

struct Gate(Arc<AtomicBool>);

impl Drop for Gate {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn wait_gate(released: &AtomicBool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !released.load(Ordering::SeqCst) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
}

fn spawn_load(
    provider: BilibiliProvider,
) -> (tokio::task::JoinHandle<()>, Receiver<PlaylistUpdate>) {
    let (sender, receiver) = playlist_update_channel();
    let task = tokio::spawn(async move {
        MusicSource::Bilibili(provider)
            .load_playlist("5", sender)
            .await;
    });
    (task, receiver)
}

async fn next_batch(receiver: &mut Receiver<PlaylistUpdate>) -> Vec<Song> {
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(update) = receiver.recv().await {
            match update {
                PlaylistUpdate::Batch { songs, .. } => return songs,
                PlaylistUpdate::Failed(error) => {
                    panic!("load failed before a batch: {}", error.message)
                }
                PlaylistUpdate::Complete(_) => panic!("load completed before the expected batch"),
                _ => {}
            }
        }
        panic!("stream closed before the expected batch")
    })
    .await
    .unwrap()
}

async fn finish(receiver: &mut Receiver<PlaylistUpdate>) -> (Vec<Song>, PlaylistProgress) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut songs = Vec::new();
        while let Some(update) = receiver.recv().await {
            match update {
                PlaylistUpdate::Batch { songs: batch, .. } => songs.extend(batch),
                PlaylistUpdate::Complete(progress) => return (songs, progress),
                PlaylistUpdate::Failed(error) => panic!("load failed: {}", error.message),
                _ => {}
            }
        }
        panic!("stream closed without completion")
    })
    .await
    .unwrap()
}

fn bvid(index: usize) -> String {
    format!("BV1GJ411x7h{index}")
}

fn media(index: usize) -> serde_json::Value {
    json!({"type":2,"bvid":bvid(index)})
}

fn video_index(path: &str) -> usize {
    let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
    let (_, value) = url.query_pairs().find(|(name, _)| name == "bvid").unwrap();
    value.chars().last().unwrap().to_digit(10).unwrap() as usize
}

async fn wait_for_views(fixture: &Fixture, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let loaded = fixture
                .paths
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.contains("web-interface/view"))
                .count();
            if loaded >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn first_page_is_visible_while_the_later_page_is_still_waiting() {
    let released = Arc::new(AtomicBool::new(false));
    let waiting = released.clone();
    let fixture = Fixture::new(move |path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") && path.contains("pn=1") {
            page(123, true, json!([media(1)]))
        } else if path.contains("resource/list") {
            wait_gate(&waiting);
            page(123, false, json!([media(2)]))
        } else {
            let index = video_index(path);
            video(&bvid(index), &[index as u64 * 10])
        }
    });
    let _gate = Gate(released.clone());
    let (task, mut receiver) = spawn_load(fixture.provider());
    let mut songs = next_batch(&mut receiver).await;
    assert_eq!(songs[0].id, format!("{}:10", bvid(1)));
    assert!(!released.load(Ordering::SeqCst));
    assert!(
        !task.is_finished(),
        "later pagination must not block publication of the first page"
    );
    released.store(true, Ordering::SeqCst);
    let (remaining, progress) = finish(&mut receiver).await;
    songs.extend(remaining);
    assert_eq!(
        crate::playlist::song_ids(&songs),
        [format!("{}:10", bvid(1)), format!("{}:20", bvid(2))]
    );
    assert_eq!((progress.pages, progress.videos, progress.songs), (2, 2, 2));
    task.await.unwrap();
}

#[tokio::test]
async fn slow_predecessor_keeps_order_and_bounds_completed_lookahead() {
    let released = Arc::new(AtomicBool::new(false));
    let waiting = released.clone();
    let fixture = Fixture::new(move |path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, false, json!((1..=6).map(media).collect::<Vec<_>>()))
        } else {
            let index = video_index(path);
            if index == 2 {
                wait_gate(&waiting);
            }
            video(&bvid(index), &[index as u64 * 10])
        }
    });
    let _gate = Gate(released.clone());
    let (task, mut receiver) = spawn_load(fixture.provider());
    let mut songs = next_batch(&mut receiver).await;
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].id, format!("{}:10", bvid(1)));
    wait_for_views(&fixture, 5).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let views = fixture
        .paths
        .lock()
        .unwrap()
        .iter()
        .filter(|path| path.contains("web-interface/view"))
        .count();
    assert_eq!(
        views, 5,
        "one published predecessor plus at most four in-flight or waiting results"
    );
    assert!(
        receiver.try_recv().is_err(),
        "a later result must not pass the blocked predecessor"
    );
    released.store(true, Ordering::SeqCst);
    let (remaining, progress) = finish(&mut receiver).await;
    songs.extend(remaining);
    assert_eq!(
        crate::playlist::song_ids(&songs),
        (1..=6)
            .map(|index| format!("{}:{}", bvid(index), index * 10))
            .collect::<Vec<_>>()
    );
    assert_eq!(progress.songs, 6);
    task.await.unwrap();
}

#[tokio::test]
async fn a_small_pending_batch_flushes_while_the_next_video_is_waiting() {
    let released = Arc::new(AtomicBool::new(false));
    let waiting = released.clone();
    let fixture = Fixture::new(move |path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, false, json!((1..=3).map(media).collect::<Vec<_>>()))
        } else {
            let index = video_index(path);
            if index == 3 {
                wait_gate(&waiting);
            }
            video(&bvid(index), &[index as u64 * 10])
        }
    });
    let _gate = Gate(released.clone());
    let (task, mut receiver) = spawn_load(fixture.provider());
    assert_eq!(
        next_batch(&mut receiver).await[0].id,
        format!("{}:10", bvid(1))
    );
    assert_eq!(
        next_batch(&mut receiver).await[0].id,
        format!("{}:20", bvid(2))
    );
    assert!(!released.load(Ordering::SeqCst));
    released.store(true, Ordering::SeqCst);
    let (remaining, progress) = finish(&mut receiver).await;
    assert_eq!(remaining[0].id, format!("{}:30", bvid(3)));
    assert_eq!(progress.songs, 3);
    task.await.unwrap();
}

#[tokio::test]
async fn large_multi_part_video_uses_bounded_batches_with_exact_completion_count() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, false, json!([media(1)]))
        } else {
            video(&bvid(1), &(1..=257).collect::<Vec<_>>())
        }
    });
    let (task, mut receiver) = spawn_load(fixture.provider());
    let mut batch_sizes = Vec::new();
    let mut songs = Vec::new();
    let progress = loop {
        match tokio::time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap()
        {
            PlaylistUpdate::Batch { songs: batch, .. } => {
                batch_sizes.push(batch.len());
                songs.extend(batch);
            }
            PlaylistUpdate::Complete(progress) => break progress,
            PlaylistUpdate::Failed(error) => panic!("unexpected failure: {}", error.message),
            _ => {}
        }
    };
    assert_eq!(batch_sizes, [128, 128, 1]);
    assert_eq!(progress.songs, 257);
    assert_eq!(progress.videos, 1);
    assert_eq!(
        crate::playlist::song_ids(&songs),
        (1..=257)
            .map(|cid| format!("{}:{cid}", bvid(1)))
            .collect::<Vec<_>>()
    );
    assert!(songs.iter().all(|song| song
        .source
        .as_ref()
        .is_some_and(|source| source.profile_id == "test")));
    task.await.unwrap();
}

#[tokio::test]
async fn later_page_failure_preserves_published_batches_without_a_complete_result() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") && path.contains("pn=1") {
            page(123, true, json!([media(1)]))
        } else if path.contains("resource/list") {
            json!({"code":-412,"data":null})
        } else {
            video(&bvid(1), &[10])
        }
    });
    let (task, mut receiver) = spawn_load(fixture.provider());
    assert_eq!(next_batch(&mut receiver).await.len(), 1);
    let error = loop {
        match receiver.recv().await.unwrap() {
            PlaylistUpdate::Failed(error) => break error,
            PlaylistUpdate::Complete(_) => panic!("a failed page cannot complete a partial folder"),
            _ => {}
        }
    };
    assert_eq!(error.kind, PlaylistFailureKind::Other);
    task.await.unwrap();
    assert_eq!(
        fixture.provider().favorite_tracks(5).await.unwrap_err(),
        BilibiliError::RateLimited
    );
}

#[tokio::test]
async fn unavailable_and_empty_folders_complete_without_song_batches() {
    for unavailable in [false, true] {
        let fixture = Fixture::new(move |path| {
            if path.contains("list-all") {
                folders()
            } else if path.contains("resource/list") {
                page(
                    123,
                    false,
                    if unavailable {
                        json!([media(1)])
                    } else {
                        json!([])
                    },
                )
            } else {
                json!({"code":-404,"data":null})
            }
        });
        let (task, mut receiver) = spawn_load(fixture.provider());
        let (songs, progress) = finish(&mut receiver).await;
        assert!(songs.is_empty());
        assert_eq!(progress.songs, 0);
        assert_eq!(progress.unavailable_count, usize::from(unavailable));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn cancelling_the_traversal_stops_detail_work_and_later_pagination() {
    let released = Arc::new(AtomicBool::new(false));
    let waiting = released.clone();
    let fixture = Fixture::new(move |path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, true, json!((1..=6).map(media).collect::<Vec<_>>()))
        } else {
            wait_gate(&waiting);
            let index = video_index(path);
            video(&bvid(index), &[index as u64 * 10])
        }
    });
    let _gate = Gate(released.clone());
    let (task, mut receiver) = spawn_load(fixture.provider());
    assert!(matches!(
        receiver.recv().await,
        Some(PlaylistUpdate::Metadata(_))
    ));
    wait_for_views(&fixture, 4).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(receiver.recv().await.is_none());
    released.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let paths = fixture.paths.lock().unwrap();
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.contains("web-interface/view"))
            .count(),
        4
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.contains("resource/list"))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_fast_later_failure_flushes_the_resolved_prefix_before_reporting_error() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, false, json!((1..=3).map(media).collect::<Vec<_>>()))
        } else {
            let index = video_index(path);
            if index == 3 {
                json!({"code":-412,"data":null})
            } else {
                video(&bvid(index), &[index as u64 * 10])
            }
        }
    });
    let (task, mut receiver) = spawn_load(fixture.provider());
    let mut songs = Vec::new();
    loop {
        match receiver.recv().await.unwrap() {
            PlaylistUpdate::Batch { songs: batch, .. } => songs.extend(batch),
            PlaylistUpdate::Failed(_) => break,
            PlaylistUpdate::Complete(_) => panic!("a failed video cannot complete a folder"),
            _ => {}
        }
    }
    assert_eq!(
        crate::playlist::song_ids(&songs),
        [format!("{}:10", bvid(1)), format!("{}:20", bvid(2))]
    );
    task.await.unwrap();
}

#[tokio::test]
async fn later_account_mismatch_is_reported_as_an_authorization_failure() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") && path.contains("pn=1") {
            page(123, true, json!([media(1)]))
        } else if path.contains("resource/list") {
            page(999, false, json!([media(2)]))
        } else {
            video(&bvid(1), &[10])
        }
    });
    let (task, mut receiver) = spawn_load(fixture.provider());
    assert_eq!(next_batch(&mut receiver).await.len(), 1);
    loop {
        match receiver.recv().await.unwrap() {
            PlaylistUpdate::Failed(error) => {
                assert_eq!(error.kind, PlaylistFailureKind::Authorization);
                break;
            }
            PlaylistUpdate::Complete(_) => panic!("account mismatch cannot finish the load"),
            _ => {}
        }
    }
    task.await.unwrap();
}

#[tokio::test]
async fn folder_limit_is_enforced_before_loading_any_video_or_completing() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else {
            let mut response = page(123, false, json!([media(1)]));
            response["data"]["info"]["media_count"] = json!(20_001);
            response
        }
    });
    let (task, mut receiver) = spawn_load(fixture.provider());
    assert!(matches!(
        receiver.recv().await,
        Some(PlaylistUpdate::Failed(_))
    ));
    assert!(receiver.recv().await.is_none());
    task.await.unwrap();
    assert!(!fixture
        .paths
        .lock()
        .unwrap()
        .iter()
        .any(|path| path.contains("web-interface/view")));
}
