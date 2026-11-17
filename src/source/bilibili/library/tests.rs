use super::*;
use crate::source::bilibili::tests::{video, Fixture};

#[tokio::test]
async fn cumulative_part_limit_includes_already_published_batches() {
    let fixture = Fixture::new(|_| video("BV1GJ411x7h7", &[10, 11]));
    let provider = fixture.provider();
    for existing in [MAX_PARTS - 2, MAX_PARTS - 1] {
        let (sender, mut receiver) = playlist_update_channel();
        let mut updates = TrackUpdates::new(&sender, Instant::now());
        updates.progress.songs = existing;
        let result = provider
            .load_page_videos(vec!["BV1GJ411x7h7".into()], &mut updates)
            .await;
        if existing == MAX_PARTS - 2 {
            result.unwrap();
            assert_eq!(updates.progress.songs, MAX_PARTS);
            assert!(
                matches!(receiver.try_recv(), Ok(PlaylistUpdate::Batch { songs, .. }) if songs.len() == 2)
            );
        } else {
            assert_eq!(result.unwrap_err(), BilibiliError::LimitExceeded);
            assert_eq!(updates.progress.songs, existing);
            assert!(receiver.try_recv().is_err());
        }
    }
}
