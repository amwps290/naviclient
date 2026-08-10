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
- Audio playback with play/pause, stop, previous/next, volume, seek, and queue
  progression
- Server settings persisted locally as JSON
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

## Architecture

- `src/api.rs` - Subsonic API client, authentication, and JSON models parsing
- `src/audio.rs` - background audio worker using `rodio`
- `src/app.rs` - GPUI application state, navigation, library views, and player
- `src/models.rs` - shared API data structures
- `src/config.rs` - persisted server settings
- `src/msg.rs` - background task results delivered to the UI

Playback currently downloads the selected track to a temporary file and decodes
it with `rodio`. This keeps the UI responsive and supports MP3, FLAC, AAC,
Vorbis, and other formats supported by `rodio`'s bundled decoders.
