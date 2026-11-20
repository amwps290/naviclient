//! Incremental `getPlaylist` JSON decoding for servers without playlist paging.

use std::io::{self, Read};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::de::{self, DeserializeSeed, Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::api::OpenSubsonicProvider;
use crate::models::{Playlist, Song};
use crate::playlist::loading::{PlaylistProgress, PlaylistUpdate, PLAYLIST_BATCH_SIZE};
use crate::source::SourceId;

const RESPONSE_CHUNKS: usize = 4;
const RESPONSE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn load(
    provider: &OpenSubsonicProvider,
    source: SourceId,
    playlist_id: &str,
    updates: Sender<PlaylistUpdate>,
) -> Result<PlaylistProgress> {
    let mut response = provider.playlist_response(playlist_id).await?;
    let (chunks, receiver) = mpsc::channel(RESPONSE_CHUNKS);
    let requested_id = playlist_id.to_string();
    let parser = tokio::task::spawn_blocking(move || {
        parse(ChannelReader::new(receiver), source, requested_id, updates)
    });

    let read_result: Result<()> = async {
        while let Some(chunk) = tokio::time::timeout(RESPONSE_IDLE_TIMEOUT, response.chunk())
            .await
            .context("getPlaylist response stopped arriving")?
            .map_err(reqwest::Error::without_url)
            .context("read getPlaylist response failed")?
        {
            if chunks.send(chunk.to_vec()).await.is_err() {
                break;
            }
        }
        Ok(())
    }
    .await;
    drop(chunks);
    let parsed = parser
        .await
        .context("playlist parser stopped unexpectedly")?;
    read_result?;
    parsed
}

struct ChannelReader {
    receiver: Receiver<Vec<u8>>,
    current: Vec<u8>,
    offset: usize,
}

impl ChannelReader {
    fn new(receiver: Receiver<Vec<u8>>) -> Self {
        Self {
            receiver,
            current: Vec::new(),
            offset: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        while self.offset == self.current.len() {
            let Some(chunk) = self.receiver.blocking_recv() else {
                return Ok(0);
            };
            self.current = chunk;
            self.offset = 0;
        }
        let remaining = &self.current[self.offset..];
        let count = output.len().min(remaining.len());
        output[..count].copy_from_slice(&remaining[..count]);
        self.offset += count;
        Ok(count)
    }
}

fn parse(
    reader: impl Read,
    source: SourceId,
    requested_id: String,
    updates: Sender<PlaylistUpdate>,
) -> Result<PlaylistProgress> {
    let mut sink = PlaylistSink {
        updates,
        source,
        requested_id,
        batch: Vec::with_capacity(PLAYLIST_BATCH_SIZE),
        progress: PlaylistProgress::default(),
    };
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    RootSeed { sink: &mut sink }
        .deserialize(&mut deserializer)
        .context("invalid getPlaylist response")?;
    deserializer
        .end()
        .context("trailing data in getPlaylist response")?;
    sink.flush().map_err(anyhow::Error::msg)?;
    Ok(sink.progress)
}

struct PlaylistSink {
    updates: Sender<PlaylistUpdate>,
    source: SourceId,
    requested_id: String,
    batch: Vec<Song>,
    progress: PlaylistProgress,
}

impl PlaylistSink {
    fn metadata(&mut self, fields: &Map<String, Value>) -> Result<(), String> {
        let mut fields = fields.clone();
        fields
            .entry("id")
            .or_insert_with(|| Value::String(self.requested_id.clone()));
        fields
            .entry("name")
            .or_insert_with(|| Value::String(String::new()));
        let playlist: Playlist = serde_json::from_value(Value::Object(fields))
            .map_err(|error| format!("invalid playlist metadata: {error}"))?;
        if playlist.id != self.requested_id {
            return Err(format!(
                "getPlaylist returned {:?} for requested playlist {:?}",
                playlist.id, self.requested_id
            ));
        }
        self.updates
            .blocking_send(PlaylistUpdate::Metadata(playlist))
            .map_err(|_| "playlist update receiver closed".to_string())
    }

    fn song(&mut self, mut song: Song) -> Result<(), String> {
        song.source = Some(self.source.clone());
        self.batch.push(song);
        if self.batch.len() == PLAYLIST_BATCH_SIZE {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.batch.is_empty() {
            return Ok(());
        }
        self.progress.songs = self.progress.songs.saturating_add(self.batch.len());
        let songs = std::mem::replace(&mut self.batch, Vec::with_capacity(PLAYLIST_BATCH_SIZE));
        self.updates
            .blocking_send(PlaylistUpdate::Batch {
                songs,
                progress: self.progress,
            })
            .map_err(|_| "playlist update receiver closed".to_string())
    }
}

struct RootSeed<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> DeserializeSeed<'de> for RootSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(RootVisitor { sink: self.sink })
    }
}

struct RootVisitor<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> Visitor<'de> for RootVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a Subsonic response object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut found = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "subsonic-response" {
                if found {
                    return Err(A::Error::custom("duplicate subsonic-response"));
                }
                found = true;
                map.next_value_seed(ResponseSeed { sink: self.sink })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if !found {
            return Err(A::Error::custom(
                "server response is not a Subsonic response",
            ));
        }
        Ok(())
    }
}

struct ResponseSeed<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> DeserializeSeed<'de> for ResponseSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(ResponseVisitor { sink: self.sink })
    }
}

#[derive(Default, Deserialize)]
struct SubsonicError {
    code: Option<i64>,
    message: Option<String>,
}

struct ResponseVisitor<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> Visitor<'de> for ResponseVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Subsonic response fields")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut status = None;
        let mut error = None;
        let mut playlist = false;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "status" => status = Some(map.next_value::<String>()?),
                "error" => error = Some(map.next_value::<SubsonicError>()?),
                "playlist" => {
                    if playlist {
                        return Err(A::Error::custom("duplicate playlist"));
                    }
                    playlist = true;
                    map.next_value_seed(PlaylistSeed { sink: self.sink })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        if status.as_deref() != Some("ok") {
            let error = error.unwrap_or_default();
            return Err(A::Error::custom(format!(
                "Subsonic error {}: {}",
                error.code.unwrap_or(-1),
                error.message.as_deref().unwrap_or("unknown server error")
            )));
        }
        if !playlist {
            return Err(A::Error::custom("getPlaylist response has no playlist"));
        }
        Ok(())
    }
}

struct PlaylistSeed<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> DeserializeSeed<'de> for PlaylistSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(PlaylistVisitor { sink: self.sink })
    }
}

struct PlaylistVisitor<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> Visitor<'de> for PlaylistVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("playlist metadata and entries")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = Map::new();
        let mut entries = false;
        let mut fields_after_entries = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "entry" {
                if entries {
                    return Err(A::Error::custom("duplicate playlist entry array"));
                }
                self.sink.metadata(&fields).map_err(A::Error::custom)?;
                entries = true;
                map.next_value_seed(SongsSeed { sink: self.sink })?;
            } else {
                let value = map.next_value::<Value>()?;
                if entries {
                    fields_after_entries = true;
                }
                fields.insert(key, value);
            }
        }
        if !entries || fields_after_entries {
            self.sink.metadata(&fields).map_err(A::Error::custom)?;
        }
        self.sink.flush().map_err(A::Error::custom)
    }
}

struct SongsSeed<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> DeserializeSeed<'de> for SongsSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_seq(SongsVisitor { sink: self.sink })
    }
}

struct SongsVisitor<'a> {
    sink: &'a mut PlaylistSink,
}

impl<'de> Visitor<'de> for SongsVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of playlist songs")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(song) = sequence.next_element::<Song>()? {
            self.sink.song(song).map_err(A::Error::custom)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ProviderKind;
    use std::io::Cursor;
    use std::sync::mpsc as std_mpsc;

    struct ShortReads(Cursor<Vec<u8>>);

    impl Read for ShortReads {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let limit = output.len().min(7);
            self.0.read(&mut output[..limit])
        }
    }

    struct GatedRead {
        first: Cursor<Vec<u8>>,
        second: Cursor<Vec<u8>>,
        release: Option<std_mpsc::Receiver<()>>,
    }

    impl Read for GatedRead {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let read = self.first.read(output)?;
            if read > 0 {
                return Ok(read);
            }
            if let Some(release) = self.release.take() {
                release.recv().map_err(io::Error::other)?;
            }
            self.second.read(output)
        }
    }

    #[test]
    fn playlist_entries_are_batched_across_short_response_reads() {
        let entries: Vec<_> = (0..300)
            .map(|index| serde_json::json!({"id":index.to_string(),"title":format!("Track {index}")}))
            .collect();
        let document = serde_json::to_vec(&serde_json::json!({
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "playlist": {
                    "id": "large",
                    "name": "Large playlist",
                    "songCount": 300,
                    "entry": entries
                }
            }
        }))
        .unwrap();
        let source = SourceId::new(ProviderKind::OpenSubsonic, "server");
        let (updates, mut receiver) = mpsc::channel(8);
        let progress = parse(
            ShortReads(Cursor::new(document)),
            source.clone(),
            "large".into(),
            updates,
        )
        .unwrap();

        assert_eq!(progress.songs, 300);
        let metadata = receiver.try_recv().unwrap();
        assert!(matches!(metadata, PlaylistUpdate::Metadata(Playlist { id, .. }) if id == "large"));
        for (expected, end) in [(128, 128), (128, 256), (44, 300)] {
            let PlaylistUpdate::Batch { songs, progress } = receiver.try_recv().unwrap() else {
                panic!("expected playlist batch")
            };
            assert_eq!(songs.len(), expected);
            assert_eq!(progress.songs, end);
            assert!(songs
                .iter()
                .all(|song| song.source.as_ref() == Some(&source)));
        }
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn first_batch_is_published_while_the_response_tail_is_still_blocked() {
        let first_entries = (0..PLAYLIST_BATCH_SIZE)
            .map(|index| format!(r#"{{"id":"{index}","title":"Track {index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let first = format!(
            r#"{{"subsonic-response":{{"status":"ok","playlist":{{"id":"large","name":"Large","entry":[{first_entries},"#
        )
        .into_bytes();
        let second = br#"{"id":"last","title":"Last"}]}}}"#.to_vec();
        let (release, gate) = std_mpsc::channel();
        let reader = GatedRead {
            first: Cursor::new(first),
            second: Cursor::new(second),
            release: Some(gate),
        };
        let source = SourceId::new(ProviderKind::OpenSubsonic, "server");
        let (updates, mut receiver) = mpsc::channel(4);
        let (done, finished) = std_mpsc::channel();
        let parser = std::thread::spawn(move || {
            let result = parse(reader, source, "large".into(), updates);
            done.send(result).unwrap();
        });

        assert!(matches!(
            receiver.blocking_recv().unwrap(),
            PlaylistUpdate::Metadata(_)
        ));
        let PlaylistUpdate::Batch { songs, progress } = receiver.blocking_recv().unwrap() else {
            panic!("expected first playlist batch")
        };
        assert_eq!(songs.len(), PLAYLIST_BATCH_SIZE);
        assert_eq!(progress.songs, PLAYLIST_BATCH_SIZE);
        assert!(finished.try_recv().is_err());

        release.send(()).unwrap();
        let PlaylistUpdate::Batch { songs, progress } = receiver.blocking_recv().unwrap() else {
            panic!("expected final playlist batch")
        };
        assert_eq!(songs.len(), 1);
        assert_eq!(progress.songs, PLAYLIST_BATCH_SIZE + 1);
        assert_eq!(
            finished.recv().unwrap().unwrap().songs,
            PLAYLIST_BATCH_SIZE + 1
        );
        parser.join().unwrap();
    }

    #[test]
    fn empty_and_failed_responses_are_distinguished() {
        let source = SourceId::new(ProviderKind::OpenSubsonic, "server");
        let (updates, mut receiver) = mpsc::channel(4);
        let empty =
            br#"{"subsonic-response":{"status":"ok","playlist":{"id":"p","name":"Empty"}}}"#;
        assert_eq!(
            parse(Cursor::new(empty), source.clone(), "p".into(), updates).unwrap(),
            PlaylistProgress::default()
        );
        assert!(matches!(
            receiver.try_recv().unwrap(),
            PlaylistUpdate::Metadata(Playlist { id, .. }) if id == "p"
        ));

        let (updates, _) = mpsc::channel(4);
        let failed =
            br#"{"subsonic-response":{"status":"failed","error":{"code":50,"message":"Denied"}}}"#;
        let error = parse(Cursor::new(failed), source, "p".into(), updates).unwrap_err();
        assert!(format!("{error:#}").contains("Subsonic error 50: Denied"));
    }
}
