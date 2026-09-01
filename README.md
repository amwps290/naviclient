# Navidrome Client

A native Rust GUI client for [Navidrome](https://www.navidrome.org) using the
Subsonic API. It uses `GPUI` and `gpui-component` for the interface and
`rodio` for audio playback, so there is no Tauri, Flutter, WebView, or
JavaScript runtime.

## Features

- Subsonic authentication with the standard `u/t/s` token flow
- Home, artists, albums, playlists, and search views
- Album cover art loaded from the Navidrome API
- Native system font discovery and multilingual text rendering through GPUI
- Responsive album and artist grids with virtualized-ready component foundations
- Audio playback with play/pause, stop, previous/next, seek, and queue
  progression
- Editable playback queue: play next, append to queue, remove, reorder,
  clear-after-current and clear-all, plus append-all from albums and playlists
- Navidrome now-playing and scrobble sync: reports the current track and
  records completed plays (50% or 4-minute threshold), failures never interrupt
  local playback
- Persistent volume with mute/restore, mouse-wheel adjustment, and a compact
  vertical slider shown above the player on hover
- Double-click song playback with highlighted and animated now-playing rows
- Dedicated now-playing screen with Navidrome lyrics and synced lyric highlighting
- Mini-player mode that shrinks the main window into a compact, always-on-top
  widget with cover art, song title, prev/next/play controls, a thin bottom
  progress line, and a dynamic cover-colored animated background
- Server settings persisted locally as JSON
- Navidrome-synced favorites for artists, albums, and songs
- Light theme by default, with light, dark, and system-following appearance options
- Borderless client-drawn window with native minimize, maximize, resize, and close behavior
- Cross-platform desktop support: Windows, macOS, and Linux

## Build and Run

```bash
cargo run
```

For a release build:

```bash
cargo build --release
```

The first launch opens Server Settings automatically. Enter the Navidrome
server URL, username, and password, then click **Save and Connect**. The
settings window also provides Paste buttons beside each field if the system
clipboard shortcut is unavailable.

Config is stored in the platform config directory, e.g.:

- Windows: `%APPDATA%\rs\navidrome\navidrome-client\config.json`
- macOS: `~/Library/Application Support/rs/navidrome/navidrome-client/config.json`
- Linux: `~/.config/navidrome-client/config.json`

Played audio is cached separately in the configured audio cache directory.
The folder can be changed from Settings, completed tracks are reused without
another download, partial original tracks resume through HTTP range requests
when supported, and the oldest cache files are removed at startup when the
cache exceeds 4 GiB.

Diagnostic logs are written to the platform local data directory under
`logs/navidrome-client.log`. The file rotates to `navidrome-client.log.old` at
5 MiB. Stream URLs are logged without authentication query parameters.

## Development Roadmap

The detailed, checkbox-based implementation roadmap is maintained in
[`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md). Update it with the completion
commit whenever a milestone is finished.

## Architecture

- `src/api.rs` - Subsonic API client, authentication, and JSON models parsing
- `src/audio.rs` - background audio worker using `rodio`
- `src/app.rs` - GPUI application state, navigation, library views, and player
- `src/models.rs` - shared API data structures
- `src/config.rs` - persisted server settings
- `src/msg.rs` - background task results delivered to the UI

Playback uses a read-through disk cache and a buffered HTTP reader. Servers
with byte-range support can start large files without a full download, resume
partial cache files, and seek by requesting only the required part of the
track. Settings provides Original, 128, 192, 256, and 320 kbps quality levels;
transcoded levels request an MP3 stream from Navidrome, and each quality profile
has an isolated cache. The decoded audio queue absorbs longer network
interruptions, while completed
tracks play directly from local storage. MP3, FLAC, AAC, Vorbis, and other
formats supported by `rodio` remain available.
