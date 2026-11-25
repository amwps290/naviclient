use super::*;
use crate::source::MusicSource;

fn info(id: u64, owner: u64, cover: Option<&str>) -> serde_json::Value {
    json!({"code":0,"data":{"id":id,"title":"Music","media_count":3,"cover":cover,"upper":{"mid":owner,"name":"Owner"}}})
}

#[tokio::test]
async fn summary_cover_is_mapped_to_playlist_artwork_when_present() {
    let fixture = Fixture::new(|path| {
        assert!(path.contains("created/list-all"));
        json!({"code":0,"data":{"list":[{"id":5,"title":"Music","media_count":3,"cover":"http://i0.hdslb.com/bfs/archive/folder.jpg"}]}})
    });
    let source = MusicSource::Bilibili(fixture.provider());
    let folders = source.playlists().await.unwrap();
    assert_eq!(
        folders[0].cover_art.as_deref(),
        Some("https://i0.hdslb.com/bfs/archive/folder.jpg")
    );
    assert_eq!(fixture.paths.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn missing_summary_cover_can_be_read_from_owned_folder_info() {
    let fixture = Fixture::new(|path| {
        assert!(path.starts_with("/x/v3/fav/folder/info"));
        assert!(path.contains("media_id=5"));
        info(5, 123, Some("http://i0.hdslb.com/bfs/archive/folder.jpg"))
    });
    let result = fixture.provider().favorite_folder_cover(5).await.unwrap();
    assert_eq!(
        result.as_deref(),
        Some("https://i0.hdslb.com/bfs/archive/folder.jpg")
    );
    assert_eq!(
        fixture.paths.lock().unwrap().len(),
        1,
        "metadata must not enumerate video pages or start media"
    );
}

#[tokio::test]
async fn foreign_owner_wrong_id_and_unsafe_artwork_are_never_published() {
    for (id, owner, cover, expected) in [
        (
            5,
            124,
            Some("https://i0.hdslb.com/a.jpg"),
            Some(BilibiliError::PermissionDenied),
        ),
        (
            6,
            123,
            Some("https://i0.hdslb.com/a.jpg"),
            Some(BilibiliError::InvalidResponse),
        ),
        (5, 123, Some("https://evil.test/a.jpg"), None),
        (5, 123, None, None),
    ] {
        let fixture = Fixture::new(move |path| {
            assert!(path.starts_with("/x/v3/fav/folder/info"));
            info(id, owner, cover)
        });
        let result = fixture.provider().favorite_folder_cover(5).await;
        if let Some(expected) = expected {
            assert_eq!(result.unwrap_err(), expected);
        } else {
            assert!(result.unwrap().is_none());
        }
        assert_eq!(fixture.paths.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn invalid_folder_id_does_not_issue_a_request() {
    let fixture = Fixture::new(|_| panic!("invalid ID reached network"));
    assert_eq!(
        fixture
            .provider()
            .favorite_folder_cover(0)
            .await
            .unwrap_err(),
        BilibiliError::InvalidResponse
    );
    assert!(fixture.paths.lock().unwrap().is_empty());
}
