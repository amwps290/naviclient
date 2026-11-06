use super::*;
use serde_json::json;

fn playlist(ids: &[u64]) -> Value {
    json!({
        "trackIds": ids.iter().map(|id| json!({"id": id})).collect::<Vec<_>>(),
        "tracks": []
    })
}

#[test]
fn unidentified_or_fully_embedded_playlists_do_not_request_details() {
    for playlist in [
        Value::Null,
        json!({}),
        json!({"trackIds": [{"id": -1}, {"id": 1.5}, {"id": "bad"}, {}]}),
        json!({"trackIds": [{"id": 1}], "tracks": [{}]}),
        json!({"trackIds": [{"id": 1}], "tracks": [{}, {}], "trackCount": 1000}),
    ] {
        assert!(TrackHydration::new(&playlist).is_none());
    }
    let mut hydration = TrackHydration::new(&json!({
        "trackIds": [{"id": "0001"}, {"id": false}, {"id": 2}, {}],
        "tracks": [{}]
    }))
    .unwrap();
    assert_eq!(hydration.next_batch().unwrap(), ["1", "2"]);
}

#[test]
fn first_pass_batches_cover_each_id_once_at_every_500_track_boundary() {
    for count in [1, 499, 500, 501, 999, 1000, 1001] {
        let ids: Vec<_> = (1..=count).collect();
        let mut hydration = TrackHydration::new(&playlist(&ids)).unwrap();
        let mut sizes = Vec::new();
        let mut requested = Vec::new();
        while let Some(batch) = hydration.next_batch() {
            sizes.push(batch.len());
            requested.extend_from_slice(batch);
        }
        assert_eq!(
            requested,
            ids.iter().map(u64::to_string).collect::<Vec<_>>()
        );
        assert_eq!(sizes, ids.chunks(500).map(<[u64]>::len).collect::<Vec<_>>());
        assert!(hydration.next_batch().is_none());
    }
}

#[test]
fn a_prepared_plan_owns_its_ids_after_the_response_changes() {
    let mut body = playlist(&[1, 2]);
    let mut hydration = TrackHydration::new(&body).unwrap();
    body["trackIds"] = json!([{"id": 99}]);
    assert_eq!(hydration.next_batch().unwrap(), ["1", "2"]);
    assert_eq!(hydration.missing_ids(), ["1", "2"]);
}

#[test]
fn missing_retry_rows_restore_playlist_order_and_only_absent_details_count_as_unavailable() {
    let mut body = json!({
        "playlist": {
            "name": "Fixture",
            "trackCount": 4,
            "trackIds": [{"id": 1}, {"id": 2}, {"id": 3}, {"id": 4}],
            "tracks": [{"id": 2, "name": "stale embedded row"}]
        }
    });
    let mut hydration = TrackHydration::new(&body["playlist"]).unwrap();
    let song = |id| json!({"id": id, "name": format!("Track {id}"), "ar": [{"name": "Artist"}]});
    hydration.include(vec![song(3), song(1), song(99)]);
    assert_eq!(hydration.missing_ids(), ["2", "4"]);
    hydration.include(vec![song(2)]);
    let (tracks, unavailable) = hydration.finish();
    assert_eq!(
        tracks
            .iter()
            .map(|song| song["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(unavailable, 1);
    body["playlist"]["tracks"] = Value::Array(tracks);
    let imported =
        super::super::response::parse_response_with_unavailable(&body, unavailable).unwrap();
    assert_eq!(
        imported
            .tracks
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["Track 1", "Track 2", "Track 3"]
    );
    assert_eq!(imported.unavailable_tracks, 1);

    body["playlist"]["tracks"][1]["ar"] = json!([]);
    let error =
        super::super::response::parse_response_with_unavailable(&body, unavailable).unwrap_err();
    assert!(error.to_string().contains("2/4"));
}

#[test]
fn duplicate_detail_rows_use_the_last_value_and_repeated_ids_consume_it_once() {
    let mut hydration = TrackHydration::new(&playlist(&[2, 1, 2, 3])).unwrap();
    hydration.include(vec![
        json!({"id": 2, "name": "old"}),
        json!({"id": "0002", "name": "new"}),
        json!({"id": 1, "name": "first"}),
        json!({"id": -1}),
        json!({"id": true}),
        json!({"id": "invalid"}),
        json!({}),
    ]);
    assert_eq!(hydration.missing_ids(), ["3"]);
    let (tracks, unavailable) = hydration.finish();
    assert_eq!(
        tracks,
        [
            json!({"id": "0002", "name": "new"}),
            json!({"id": 1, "name": "first"})
        ]
    );
    assert_eq!(unavailable, 2);
}
