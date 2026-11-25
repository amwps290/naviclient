use super::*;

fn songs() -> Vec<Song> {
    vec![
        Song {
            id: "a".into(),
            title: "稻香".into(),
            artist: "周杰伦".into(),
            album: "魔杰座".into(),
            ..Default::default()
        },
        Song {
            id: "b".into(),
            title: "Love Story".into(),
            artist: "Taylor Swift".into(),
            album: "Fearless".into(),
            ..Default::default()
        },
        Song {
            id: "a".into(),
            title: "稻香".into(),
            artist: "周杰伦".into(),
            album: "魔杰座".into(),
            ..Default::default()
        },
    ]
}

#[test]
fn chinese_multifield_terms_keep_order_and_duplicate_occurrences() {
    let index = PlaylistSearchIndex::build(&songs(), &Cancellation::default()).unwrap();
    for query in ["稻", "周杰伦 稻香", "魔杰座 周杰", "  稻香   稻香  "] {
        assert_eq!(
            index.matching(&QueryTerms::new(query), &Cancellation::default()),
            Some(vec![0, 2])
        );
    }
    assert_eq!(
        index.matching(
            &QueryTerms::new("周杰伦 fearless"),
            &Cancellation::default()
        ),
        Some(vec![])
    );
}

#[test]
fn unicode_case_width_and_whitespace_are_normalized() {
    let index = PlaylistSearchIndex::build(&songs(), &Cancellation::default()).unwrap();
    for query in [
        "LOVE story",
        "ＬＯＶＥ　ＳＴＯＲＹ",
        "story\tTaylor",
        "fearless love",
    ] {
        assert_eq!(
            index.matching(&QueryTerms::new(query), &Cancellation::default()),
            Some(vec![1])
        );
    }
    assert_eq!(
        QueryTerms::new("LOVE love STORY"),
        QueryTerms::new("story love")
    );
    assert!(QueryTerms::new("　\n\t").is_empty());
}

#[test]
fn field_boundaries_do_not_create_false_matches_and_punctuation_is_kept() {
    let songs = [Song {
        title: "ab".into(),
        artist: "cd".into(),
        album: "A.B".into(),
        ..Default::default()
    }];
    let index = PlaylistSearchIndex::build(&songs, &Cancellation::default()).unwrap();
    assert_eq!(
        index.matching(&QueryTerms::new("bc"), &Cancellation::default()),
        Some(vec![])
    );
    assert_eq!(
        index.matching(&QueryTerms::new("a.b cd"), &Cancellation::default()),
        Some(vec![0])
    );
}

#[test]
fn bilibili_search_includes_uploader_video_part_and_collection() {
    let songs = [Song {
        title: "歌曲".into(),
        bilibili_part: Some(crate::models::BilibiliPart {
            bvid: "BV1".into(),
            cid: 1,
            index: 2,
            count: 3,
            title: "第二幕".into(),
            uploader: "音乐空间".into(),
            video_title: "夏日演唱会".into(),
            collection_title: "现场合集".into(),
        }),
        ..Default::default()
    }];
    let index = PlaylistSearchIndex::build(&songs, &Cancellation::default()).unwrap();
    for query in ["音乐空间", "演唱会 第二幕", "现场合集", "音乐 第二 夏日"] {
        assert_eq!(
            index.matching(&QueryTerms::new(query), &Cancellation::default()),
            Some(vec![0])
        );
    }
}

#[test]
fn incremental_batches_match_whole_index_and_cancellation_stops_work() {
    let songs = songs();
    let mut index = PlaylistSearchIndex::default();
    index.append(&songs[..1], &Cancellation::default()).unwrap();
    index.append(&songs[1..], &Cancellation::default()).unwrap();
    assert_eq!(index.len(), 3);
    assert_eq!(
        index.matching(&QueryTerms::new("稻香"), &Cancellation::default()),
        Some(vec![0, 2])
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(PlaylistSearchIndex::build(&songs, &cancel).is_none());
    assert!(index.matching(&QueryTerms::new("稻香"), &cancel).is_none());
}

#[test]
#[ignore = "manual metadata-search benchmark; no wall-clock assertions"]
fn playlist_search_benchmark() {
    use std::time::Instant;
    for count in [1000, 10000] {
        let songs = (0..count)
            .map(|i| Song {
                id: i.to_string(),
                title: format!("歌曲 Track {i}"),
                artist: format!("Artist {}", i % 100),
                album: format!("Album {}", i / 10),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        let index = PlaylistSearchIndex::build(&songs, &Cancellation::default()).unwrap();
        let build = started.elapsed();
        let started = Instant::now();
        let result = index
            .matching(&QueryTerms::new("Track 999"), &Cancellation::default())
            .unwrap();
        eprintln!(
            "playlist search benchmark: {count} tracks, build={build:?}, match={:?}, matches={}",
            started.elapsed(),
            result.len()
        );
    }
}

#[test]
fn large_index_retains_all_positions_without_deduplicating_song_ids() {
    let songs = (0..10000)
        .map(|i| Song {
            id: "same".into(),
            title: format!("Track {i}"),
            artist: "Artist".into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let index = PlaylistSearchIndex::build(&songs, &Cancellation::default()).unwrap();
    assert_eq!(index.len(), 10000);
    assert_eq!(
        index.matching(&QueryTerms::new("Track 9999"), &Cancellation::default()),
        Some(vec![9999])
    );
    assert_eq!(
        index
            .matching(&QueryTerms::new(""), &Cancellation::default())
            .unwrap()
            .len(),
        10000
    );
}
