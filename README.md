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
- Next-track preloading near the end of a fully buffered track. Prepares a
  bounded startup buffer, then sleeps until needed; supports shuffle and repeat,
  cancels stale queue/quality/seek requests, and falls back to ordinary playback.
  Prepared tracks join the current output stream at the sample boundary without
  waiting for UI polling, including across sample-rate and channel changes.
- Editable playback queue: play next, append to queue, remove, reorder,
  clear-after-current and clear-all, plus append-all from albums and playlists
- Independent shuffle toggle and repeat control (off / all / one) in the
  player's transport controls, persisted across restarts. Shuffle keeps a
  play history so Previous returns actually played tracks and short-term
  repeats are avoided; legacy single-mode configs and sessions migrate to
  the new two-dimensional mode automatically
- Navidrome now-playing and scrobble sync: reports the current track and
  records completed plays (50% or 4-minute threshold), failures never interrupt
  local playback
- Home page with collapsible sections: recently played, frequently played,
  random albums and newest albums, fed by Navidrome's album lists
- Album and artist pages with a sort menu (recently added, name, artist, or
  year for albums; name A–Z/Z–A or album count for artists). Sort orders use
  server-side `getAlbumList2` sorting so incremental paging keeps working, are
  persisted across restarts, and legacy configs safely default to newest-added
  and name order
- Genre browsing: a Genres sidebar entry lists every genre with album and song
  counts (Subsonic `getGenres`), and each genre page shows its albums with the
  same incremental paging as the Albums page (`getAlbumList2 type=byGenre`)
- Search results can be filtered to All, Artists, Albums, or Songs with a
  segmented control; empty filtered categories show an explicit hint
- Multi-disc albums list their tracks in (disc, track) order, with missing
  numbers sorted last instead of breaking the order
- Per-view refresh buttons on Home, Favorites, Artists, Albums, Genres, and
  Search reload only that view's data instead of the whole library; a failed
  album page (Albums or a genre page) shows an inline "try again" row, empty
  search results and empty genres get explicit empty states
- Collapsible Home sections keep the page compact as more album lists are added
- Persistent volume with mute/restore, mouse-wheel adjustment, and a compact
  vertical slider shown above the player on hover
- Audio output device management: pick a specific output device (or follow the
  OS default) from Settings, persisted across restarts, with the active
  device's real sample rate and channel count shown for diagnostics.
  Switching devices keeps the current track and position, rebuilding the
  stream on the new output while preserving pause state; a missing target
  device falls back to the system default
- 10-band graphic equalizer (31.25 Hz to 16 kHz, +/-12 dB per band) with
  eight built-in presets, opened from an EQ button in the bottom player bar
  as a popover (click outside to dismiss, drag sliders while listening).
  Filtering runs in the audio worker after ReplayGain gain and before output
  conversion, so loudness measurement stays EQ-independent; changes take
  effect on the playing track within ~20 ms without restarting it
- ReplayGain volume normalization (Off / Track / Album) so loud and quiet
  tracks play at a similar level, with peak-based anti-clipping; falls back to
  locally measured loudness when the server provides no ReplayGain metadata
- Double-click song playback with highlighted and animated now-playing rows
- Dedicated now-playing screen with Navidrome lyrics and synced lyric highlighting
- Create, rename, and delete Navidrome playlists; add songs or whole albums,
  save the current queue as a playlist, and move or remove individual entries.
  Duplicate tracks keep their order; shared playlists owned by others are read-only.
- Artist and album names are clickable links everywhere they appear: song
  rows (artist column and album subtitle), the player bar, the now-playing
  header, album cards, and the album detail header all navigate to the
  artist or album page on click, with a hover highlight to signal the link.
  Items without a server-provided artist/album id fall back to static text
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
  widget (260x56) with cover art, song title, prev/next/play controls, a thin
  bottom progress line, and a dynamic cover-colored animated background. The
  cover glows with the track's accent color while playing; scrolling the wheel
  over the window adjusts volume and briefly flashes it on the progress line
- Server settings persisted locally as JSON
- Navidrome-synced favorites for artists, albums, and songs
- Light theme by default, with light, dark, and system-following appearance
  options; the light theme's secondary text (hints, subtitles, metadata) is
  darkened from neutral-500 to neutral-600 so small text stays readable on
  tinted cards, while the dark theme keeps the stock palette
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
cache exceeds 4 GiB. Album cover art bytes are also cached to disk under the
same directory (`covers/`), capped at 256 MiB with oldest-first pruning, so
restarts render grids instantly without re-downloading every cover.

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
- `src/audio/preload.rs` - cancellable next-track preparation and decoder suspension
- `src/audio/gapless.rs` - continuous output, track-boundary events and atomic cancellation
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

Preloading starts within the last 15 seconds only after the current track's
remaining audio has been decoded, keeping its playback independent of network
bandwidth. One speculative task prepares up to 16 chunks of 4096 frames
(about 1.37 seconds / 512 KiB of stereo PCM at 48 kHz), with the existing 1 MiB
HTTP read-ahead buffer. The decoder then waits on a condition variable. Queue
edits, seeks, playback-mode changes, quality changes and cache-directory changes
invalidate the preparation. A reserved shuffle choice is reused when advancing.
Each track is normalized to the output device format with its own ReplayGain
and loudness meter. At a natural end, the output callback starts the prepared
track in the same sample request; the UI consumes a single transition event to
update the queue, lyrics, scrobbling and media metadata without restarting audio.
Cancelling and starting a prepared track share one decision, so queue edits also
work at the boundary. Codec delay/padding is trimmed when supported by the
decoder and file metadata; silence recorded in the file is preserved. If the
next track is not ready, playback uses the ordinary switching path.
Unfinished speculative cache files are discarded; validated complete files join
the ordinary audio cache. A cancelled in-flight read can take up to the I/O
timeout to return, while foreground playback can proceed independently.
