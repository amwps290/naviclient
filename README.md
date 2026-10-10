# Navidrome Client

A native Rust GUI client for [Navidrome](https://www.navidrome.org) using the
Subsonic API. It uses the official [`gpui-kit`](https://github.com/longbridge/gpui-kit)
framework for the interface and
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
- Home page with collapsible sections: recently played, frequently played,
  random albums and newest albums, fed by Navidrome's album lists
- Collapsible Home sections keep the page compact as more album lists are added
- Persistent volume with mute/restore, mouse-wheel adjustment, and a compact
  vertical slider shown above the player on hover
- ReplayGain volume normalization (Off / Track / Album) so loud and quiet
  tracks play at a similar level, with peak-based anti-clipping; falls back to
  locally measured loudness when the server provides no ReplayGain metadata
- Double-click song playback with highlighted and animated now-playing rows
- Dedicated now-playing screen with Navidrome lyrics and synced lyric highlighting
- Create, rename, and delete Navidrome playlists; add songs or whole albums,
  save the current queue as a playlist, and move or remove individual entries.
  Duplicate tracks keep their order; shared playlists owned by others are read-only.
- Song actions are available from right-click and the “…” button in song lists,
  search results, playlists, and the queue
- Linux MPRIS media session for desktop media controls: track metadata, local cover
  artwork, playback status, position, volume, play/pause, previous/next, and seek.
  Controls work while minimized; a missing session bus does not prevent startup.
- Repeated songs and albums have independent hover controls, including across
  Home sections and within the playback queue
- Rotating vinyl cover art while the lyrics page is playing in the foreground;
  pauses at the current angle and reuses cached textures at up to 20 fps.
  Only the current cover is cached (180 frames at 192×192, about 25 MiB of
  pixels plus GPU atlas storage); preparation runs once in a throttled background
  worker, delivering the next visible frames first so rotation starts without
  waiting for the full cache. Its resolution is capped on high-DPI displays to
  keep memory bounded.
- Mini-player mode that shrinks the main window into a compact, always-on-top
  widget with cover art, song title, prev/next/play controls, a thin bottom
  progress line, and a dynamic cover-colored animated background
- Server settings persisted locally as JSON
- Navidrome-synced favorites for artists, albums, and songs
- Light theme by default, with light, dark, and system-following appearance options
- Borderless client-drawn window with native minimize, maximize, resize, and close behavior
- Cross-platform desktop support: Windows, macOS, and Linux

## Build and Run

Use a recent stable Rust toolchain (verified with Rust 1.96.1). The UI depends
on `gpui-kit` 0.7.1, which supplies matching GPUI, platform, component and icon
crates. `Cargo.lock` pins the complete dependency set.

```bash
cargo run --locked
```

For a release build:

```bash
cargo build --release --locked
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

UI code imports GPUI through `gpui_kit` and styled components through
`gpui_kit::component`. Kit initializes the platform and window root; the
application asset source combines custom player icons with Kit's built-in icons.

- `src/api.rs` - Subsonic API client, authentication, and JSON models parsing
- `src/audio.rs` - background audio worker using `rodio`
- `src/app.rs` - GPUI application state, navigation, library views, and player
- `src/vinyl.rs` - progressive rotation frames, bounded texture cache, and animation timing
- `src/media_session.rs` and `src/media_session/linux.rs` - shared media commands and Linux MPRIS integration
- `src/playlist.rs` and `src/app/playlists.rs` - playlist edit validation, dialogs, and song menus
- `src/api/playlist_tests.rs` - HTTP contract tests for playlist editing
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
