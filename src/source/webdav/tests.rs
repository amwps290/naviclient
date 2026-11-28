use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use super::*;
use crate::models::{AlbumSort, FavoriteKey, FavoriteKind};
use crate::source::{ProviderKind, SourceId};

#[test]
fn parses_webdav_propfind_xml_and_infers_metadata() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/Music/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/Music/Jay%20Chou/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/Music/01%20-%20Sunny%20Day.flac</D:href>
    <D:propstat>
      <D:prop>
        <D:getcontentlength>31457280</D:getcontentlength>
        <D:getetag>"abc123"</D:getetag>
        <D:getlastmodified>Sat, 28 Nov 2026 10:00:00 GMT</D:getlastmodified>
      </D:prop>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    let req_url = Url::parse("https://nas.local/dav/Music/").unwrap();
    let entries = client::parse_propfind_xml(xml, &req_url).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].is_collection);
    assert_eq!(entries[0].file_name, "Jay Chou");
    assert!(!entries[1].is_collection);
    assert_eq!(entries[1].file_name, "01 - Sunny Day.flac");
    assert_eq!(entries[1].content_length, Some(31457280));
    assert_eq!(entries[1].etag.as_deref(), Some("abc123"));

    let source_id = SourceId::new(ProviderKind::WebDav, "nas-1");
    let track = index::infer_metadata_from_path(
        &source_id,
        &entries[1],
        "/dav",
        None,
        None,
        None,
    );
    assert_eq!(track.song.title, "Sunny Day");
    assert_eq!(track.song.track, Some(1));
    assert_eq!(track.song.album, "Music");
    assert_eq!(track.song.suffix.as_deref(), Some("flac"));
}

#[tokio::test]
async fn scans_mock_webdav_server_resolves_auth_stream_cover_and_lrc() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        // Handle up to 10 sequential requests for the test.
        for _ in 0..10 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or_default();
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();

            let (status, content_type, body): (&str, &str, Vec<u8>) = match (method, path) {
                ("PROPFIND", "/dav/" | "/dav") => (
                    "207 Multi-Status",
                    "application/xml",
                    br#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/ArtistA/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#
                        .to_vec(),
                ),
                ("PROPFIND", "/dav/ArtistA/" | "/dav/ArtistA") => (
                    "207 Multi-Status",
                    "application/xml",
                    br#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/ArtistA/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/ArtistA/AlbumX/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#
                        .to_vec(),
                ),
                ("PROPFIND", "/dav/ArtistA/AlbumX/" | "/dav/ArtistA/AlbumX") => (
                    "207 Multi-Status",
                    "application/xml",
                    br#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/ArtistA/AlbumX/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/ArtistA/AlbumX/01%20Starlight.flac</d:href><d:propstat><d:prop><d:getcontentlength>1024</d:getcontentlength><d:getetag>"e1"</d:getetag></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/ArtistA/AlbumX/01%20Starlight.lrc</d:href><d:propstat><d:prop><d:getcontentlength>30</d:getcontentlength></d:prop></d:propstat></d:response>
  <d:response><d:href>/dav/ArtistA/AlbumX/cover.jpg</d:href><d:propstat><d:prop><d:getcontentlength>10</d:getcontentlength></d:prop></d:propstat></d:response>
</d:multistatus>"#
                        .to_vec(),
                ),
                ("GET", "/dav/ArtistA/AlbumX/01%20Starlight.lrc") => (
                    "200 OK",
                    "text/plain",
                    b"[00:05.00]Under the starlight\n".to_vec(),
                ),
                ("GET", "/dav/ArtistA/AlbumX/cover.jpg") => (
                    "200 OK",
                    "image/jpeg",
                    b"coverbytes".to_vec(),
                ),
                _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
            };

            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let provider = WebDavProvider::with_data_dir(
        SourceId::new(ProviderKind::WebDav, "wd-1"),
        "Home NAS".into(),
        format!("http://{addr}/dav"),
        "/".into(),
        "alice".into(),
        "secret".into(),
        NetworkRoute::Direct,
        dir.path().to_path_buf(),
    )
    .unwrap();

    let summary = provider.rescan().await.unwrap();
    assert_eq!(summary.total_tracks, 1);
    assert_eq!(summary.parsed_tracks, 1);

    let albums = provider.albums(10, 0, AlbumSort::Name);
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "AlbumX");
    assert_eq!(albums[0].artist, "ArtistA");
    assert!(albums[0].cover_art.is_some());

    let songs = provider.album_songs(&albums[0].id);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].title, "Starlight");
    assert_eq!(songs[0].track, Some(1));

    // Cover fetch via WebDAV
    let cover_url = provider
        .cover_url(albums[0].cover_art.as_deref().unwrap())
        .unwrap();
    assert_eq!(provider.cover_bytes(&cover_url).await.unwrap(), b"coverbytes");

    // Lyrics fetch via sibling .lrc
    let lyrics = provider.lyrics(&songs[0]).await.unwrap();
    assert!(lyrics.is_synced());
    assert_eq!(lyrics.lines[0].text, "Under the starlight");

    // Playback resolution attaches Basic Auth header.
    let playback = provider.resolve_playback(&songs[0]).unwrap();
    assert!(playback.http.headers().contains_key("authorization"));
    assert!(playback.http.url().ends_with("/dav/ArtistA/AlbumX/01%20Starlight.flac"));

    // Local favorites persistence
    provider
        .set_favorite(&FavoriteKey::new(FavoriteKind::Song, &songs[0].id), true)
        .unwrap();
    assert_eq!(provider.favorites().songs.len(), 1);

    // Folder browse walks virtual hierarchy derived from WebDAV paths.
    let root = provider.folder_listing("");
    assert_eq!(root.folders.len(), 1);
    assert_eq!(root.folders[0].name, "ArtistA");
    let artist_dir = provider.folder_listing(&root.folders[0].id);
    assert_eq!(artist_dir.folders.len(), 1);
    assert_eq!(artist_dir.folders[0].name, "AlbumX");
    let album_dir = provider.folder_listing(&artist_dir.folders[0].id);
    assert_eq!(album_dir.songs.len(), 1);
    assert_eq!(album_dir.songs[0].title, "Starlight");

    drop(server);
}
