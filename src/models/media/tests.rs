use super::*;
use crate::source::ProviderKind;
use serde_json::json;

fn saved_song(count: u32, part_title: &str) -> Song {
    let mut song: Song = serde_json::from_value(json!({
        "id": "BV1aJ411s7Nv:158355877",
        "title": "坂井泉水 161首单曲合集「1080P」",
        "bilibiliPart": {
            "bvid": "BV1aJ411s7Nv",
            "cid": 158355877,
            "index": 1,
            "count": count,
            "title": part_title,
            "uploader": "Uploader"
        }
    }))
    .unwrap();
    song.source = Some(SourceId::new(ProviderKind::Bilibili, "test"));
    song
}

#[test]
fn legacy_multi_part_titles_upgrade_and_round_trip_without_changing_identity() {
    let mut song = saved_song(162, "001. Boy「男孩」");
    let identity = song.media_key();
    song.normalize_bilibili_title();
    assert_eq!(song.title, "001. Boy「男孩」");
    assert_eq!(song.media_key(), identity);
    assert_eq!(
        song.bilibili_subtitle().as_deref(),
        Some("坂井泉水 161首单曲合集「1080P」 · P1/162")
    );

    let persisted = serde_json::to_value(&song).unwrap();
    let mut restored: Song = serde_json::from_value(persisted.clone()).unwrap();
    restored.normalize_bilibili_title();
    assert_eq!(serde_json::to_value(&restored).unwrap(), persisted);
    assert_eq!(restored.media_key(), identity);
}

#[test]
fn single_page_videos_and_unnamed_parts_keep_the_video_title() {
    for (count, part_title) in [(1, "upload.mp4"), (2, ""), (2, " \t ")] {
        let mut song = saved_song(count, part_title);
        let original_title = song.title.clone();
        song.normalize_bilibili_title();
        assert_eq!(song.title, original_title);
        assert_eq!(
            song.bilibili_subtitle().as_deref(),
            if count > 1 { Some("P1/2") } else { None }
        );
    }
}

#[test]
fn collection_context_does_not_repeat_the_title_or_number_single_page_videos() {
    let mut song = saved_song(1, "upload.mp4");
    song.title = "Song A".into();
    let part = song.bilibili_part.as_mut().unwrap();
    part.video_title = "Song A".into();
    part.collection_title = "Music collection".into();
    assert_eq!(
        song.bilibili_subtitle().as_deref(),
        Some("Music collection")
    );

    song.bilibili_part.as_mut().unwrap().collection_title = "Song A".into();
    assert!(song.bilibili_subtitle().is_none());
    song.bilibili_part.as_mut().unwrap().collection_title = " \t ".into();
    assert!(song.bilibili_subtitle().is_none());

    let part = song.bilibili_part.as_mut().unwrap();
    part.count = 2;
    part.video_title = "Video title".into();
    part.collection_title = "Music collection".into();
    assert_eq!(
        song.bilibili_subtitle().as_deref(),
        Some("Video title · P1/2")
    );
}
