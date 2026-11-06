# Navidrome Client

A native Rust GUI client for [Navidrome](https://www.navidrome.org) using the
Subsonic API. It uses the official [`gpui-kit`](https://github.com/longbridge/gpui-kit)
framework for the interface and
`rodio` for audio playback, so there is no Tauri, Flutter, WebView, or
JavaScript runtime.

## Features

- Subsonic authentication with the standard `u/t/s` token flow
- OpenSubsonic capability discovery after connection: server type, server and
  protocol versions, and supported extension versions are shown in Settings.
  Servers without the discovery endpoint remain fully usable in Subsonic
  compatibility mode, and the probe never blocks the ordinary connection. The
  `OpenSubsonicProvider` keeps per-feature supported / unsupported / unknown
  capabilities for structured lyrics, discovery, playback reporting, indexed
  queues, advanced transcoding, internet radio, and podcasts. Structured lyrics
  already use this matrix to avoid known-unsupported requests without regressing
  servers whose capabilities cannot be probed.
- Multiple saved servers with one-click switching: the sidebar shows which
  library you are browsing and the Settings page lists every server with its
  address and account. Switching stops playback, saves the current queue, and
  reconnects to the other server. Server state is fully isolated per entry:
  play queue and position, measured loudness, genre cache, and cover cache each
  get their own namespace, so two servers with colliding song/cover ids can
  never see each other's data. Legacy single-server configs and caches are
  migrated automatically on first launch. Passwords are kept in the system
  credential store when available (Windows Credential Manager; platforms
  without a backend keep using the config file and a note in Settings shows
  the current mode). Removing a server asks whether to also delete its local
  data (saved session, loudness and genre caches, stored credential)
- Home, artists, albums, playlists, and search views
- Album cover art loaded from the Navidrome API
- Native font discovery with a CJK-capable UI family selected per platform:
  Microsoft YaHei/JhengHei UI on Windows, PingFang on macOS, and
  Noto/Source Han/WenQuanYi/Sarasa on Linux when installed. Missing candidates
  safely retain GPUI's system fallback, and compact UI text stays at 12px or
  larger for consistent Chinese rendering.
- Interface languages: English, Simplified Chinese (简体中文) and Traditional
  Chinese (繁體中文), switchable from Settings and applied instantly without a
  restart, or “System” to follow the OS language (Hant/TW/HK/MO regions get
  Traditional, other Chinese locales Simplified, everything else English). Every
  label, empty state, tooltip, dialog, desktop notification and player error is
  translated, with `%{name}` placeholders preserved across languages. The
  default locale is embedded at build time from `locales/*.yml`, missing keys
  fall back to the English source text, and `src/i18n_tests.rs` fails the suite
  if a `t!()` key is missing from any catalog, is no longer used, or changes its
  placeholder set
- Responsive album and artist grids with virtualized-ready component foundations
- Audio playback with play/pause, stop, previous/next, seek, and queue
  progression. Controls respond immediately to accepted commands; listening
  history, track notifications, Scrobble thresholds and retry confirmation wait
  for connected output and observed progress. Failed startup buffering creates
  no listening event or track notification.
- Stall detection and recovery: when audio stops arriving (bad Wi-Fi, overloaded
  server, idle-timeout proxy) the player bar and mini player switch to a
  "Buffering Ns" state instead of looking frozen, and playback resumes from the
  interrupted position. Failures are classified as network, server, decode, or
  local output problems so users can tell whose fault it is; network and 5xx
  failures reconnect automatically twice with backoff before reporting an error,
  and a Retry button re-opens the stream at the exact failure point on demand
- Next-track preloading near the end of a fully buffered track. Prepares a
  bounded startup buffer, then sleeps until needed; supports shuffle and repeat,
  cancels stale queue/quality/seek requests, and falls back to ordinary playback.
  Prepared tracks join the current output stream at the sample boundary without
  waiting for UI polling, including across sample-rate and channel changes.
- Editable playback queue: play next, append to queue, remove, reorder,
  clear-after-current and clear-all, plus append-all from albums and playlists
- Song radio from any track action menu or the Now Playing screen: the seed starts
  immediately while the shared recommendation engine builds a 24-track
  continuation from server similarity, same-artist and same-genre facts.
  Artist radio uses its own artist context, popular tracks and bounded album
  candidates instead of forwarding the first popular song to song radio. Both
  modes apply the same local listening-history deferral, deterministic ranking,
  ID de-duplication and artist/album diversity rules as Daily Mix, and attach an
  explainable reason plus a persistent SongRadio/ArtistRadio queue origin.
  Servers without Sonic Similarity use the artist/genre fallbacks. In-flight
  results are scoped by generation and server, so a manual queue edit or server
  switch cannot overwrite the user's Up Next. Artist pages continue to show up
  to 12 popular songs and prefer stable artist IDs when the server advertises
  the OpenSubsonic Top Songs extension.
- Opt-in automatic continuation in the Up Next drawer: when ordinary sequential
  playback naturally reaches the queue boundary, the shared recommendation
  engine appends up to 20 queue-related tracks and resumes playback. Repeat-one,
  repeat-all, shuffle and manual Next never trigger generation. Any manual
  insert, append, removal, reorder or clear operation cancels an in-flight task;
  generation and server checks reject late results. Generated queue rows carry
  compact Daily Mix, Song radio, Artist radio or Auto source badges, and the
  preference plus queue origins survive restart.
- Contextual queue drawer toggled from the player or `Q`, with `Up Next` and
  `History` views. History reuses the server-scoped listening events rather than
  guessing from queue indexes, so shuffle order, loops and duplicate tracks are
  preserved; reconnects do not create duplicate events. Up to 200 recent plays
  are shown newest-first and can be replayed by double-click, Enter or the row
  menu. Browsing and scroll position stay intact while the drawer is inspected
- Import playlists from NetEase Cloud Music or QQ Music: paste a direct link,
  mobile share link, official short link, or full share message. Public playlist
  metadata is fetched without a third-party login or audio streams, with source
  counts checked to prevent silent truncation. Each track is fuzzy-matched
  against your own library using title, primary artist and duration, then
  reviewed side by side as matched, unmatched or unchecked before a same-named
  Navidrome playlist is created. Matching runs four workers with live progress,
  tolerates isolated request failures and remains cancellable
- Independent shuffle toggle and repeat control (off / all / one) in the
  player's transport controls, persisted across restarts. Shuffle keeps a
  play history so Previous returns actually played tracks and short-term
  repeats are avoided; legacy single-mode configs and sessions migrate to
  the new two-dimensional mode automatically
- Navidrome now-playing and scrobble sync: reports the current track and
  records completed plays (50% or 4-minute threshold), failures never interrupt
  local playback
- Daily Mix on Home: one click builds and plays up to 30 unique tracks from
  favorites, frequent/recent listening, similarity, genres, new releases and
  style-filtered exploration. The deterministic per-server daily snapshot uses
  a small familiar anchor, mostly same-style related tracks, and a bounded
  in-style discovery slice; it limits repeated artists and albums, defers the
  latest 50 locally played tracks and the previous mix, excludes the current
  queue, and supports append or “refresh mix”. Results are cached by local date
  and variant; servers without similarity support still use the other sources.
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
- A listening-first Home surface: the current track (or latest recent album)
  becomes a cover-colored Continue Listening feature, while recent, frequent,
  newest, and random albums are limited to short previews with Random collapsed
  by default
- Responsive desktop compact mode below 1040px: the sidebar becomes an icon rail,
  the player narrows its metadata/control zones, and song tables hide secondary
  columns and actions without removing them from menus
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
- Single-click song selection, double-click playback, and highlighted animated
  now-playing rows; frequent actions stay inline while the full action set remains
  available from the row menu
- Dedicated now-playing screen with Navidrome lyrics and synced lyric highlighting
- Create, rename, and delete Navidrome playlists; add songs or whole albums,
  save the current queue as a playlist, and move or remove individual entries.
  Duplicate tracks keep their order; shared playlists owned by others are read-only.
- Artist and album names are clickable links everywhere they appear: song
  rows (artist column and album subtitle), the player bar, the now-playing
  header, album cards, and the album detail header all navigate to the
  artist or album page on click, with a hover highlight to signal the link.
  Items without a server-provided artist/album id fall back to static text
- Song lists support batch selection: Ctrl+click toggles rows, Shift+click
  selects a range from the keyboard cursor, Ctrl+A selects all, Esc clears.
  A floating action bar offers play / play-next / add-to-queue / add-to-
  playlist / favorite (smart direction) for the whole selection, and Enter
  plays it. Queue batch play and Enter jump to the first selected position and
  preserve the complete queue. Selection follows queue entries through
  insertions and reordering, including duplicate songs; removing selected entries
  keeps the surviving selections and clears a removed cursor
- Shift+Up/Down extends the multi-selection around its anchor (folding back past
  the anchor shrinks it naturally), and “Song info…” in the song menu opens a
  read-only dialog with track, disc, duration, format (suffix, bit rate, file
  size), file path and server id
- Song actions are available from right-click and the “…” button in song lists,
  search results, playlists, and the queue
- Linux MPRIS media session for desktop media controls: track metadata, local cover
  artwork, playback status, position, volume, play/pause, previous/next, and seek.
  Controls work while minimized; a missing session bus does not prevent startup.
- Repeated songs and albums have independent hover controls, including across
  Home sections and within the playback queue
- Rotating vinyl cover art while the lyrics page is visible;
  pauses only when the window is hidden and reuses cached textures at up to 20 fps.
  Only the current cover is cached (120 frames at 192×192, about 17 MiB of
  pixels plus GPU atlas storage); preparation runs once in a throttled background
  worker, delivering the next visible frames first so rotation starts without
  waiting for the full cache. Its resolution is capped on high-DPI displays to
  keep memory bounded.
- Cover loading keeps at most four downloads/decodes in flight, requests 384px
  artwork, and explicitly evicts decoded GPUI textures with a 96-item LRU to
  bound the largest cover-related GPU/system-memory allocation.
- Mini-player mode that shrinks the main window into a compact, always-on-top
  widget (260x56) with cover art, song title, prev/next/play controls, a thin
  bottom progress line, and a dynamic cover-colored animated background. The
  cover glows with the track's accent color while playing; scrolling the wheel
  over the window adjusts volume and briefly flashes it on the progress line
- Server settings persisted locally as JSON
- Language preference persisted locally (`System` by default); the Windows tray
  menu keeps the language it was built with until the next launch, while the
  Linux tray menu refreshes with the next playback-state update
- Navidrome-synced favorites for artists, albums, and songs
- Track-change desktop notifications on Linux (D-Bus) with the cached cover,
  title, artist and album; notifications send in the background, and rapid
  switching keeps the latest pending track and replaces the previous notification
  instead of stacking, and failures are silently ignored
- Linux system tray (StatusNotifierItem via ksni): left click toggles
  playback, the menu offers play/pause, previous, next, show window and exit,
  with both the play/pause label and the icon (eighth note / pause bars on a
  dusk-gradient badge) synced to the player state; Windows keeps its
  tray-icon implementation
- Optional "close button minimizes" setting (off by default): the window X
  button and platform close minimize instead of quitting, while tray Exit
  still quits; some Wayland compositors ignore minimize requests
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

Configuration, playback sessions, measured loudness, genre caches, and local
listening history are written through same-directory temporary files and
atomically replaced, so an interrupted save cannot leave a truncated JSON
document. Listening history is isolated per server, bounded to the latest 1000
attempts, and measures actual playback progress without counting seek jumps.

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

## Translations

UI copy lives in `locales/`, one flat `"key": "text"` line per string, where the
key is the English source text used by `t!("…")` in the code and the value is
what that language shows. The catalogs are embedded at build time:

- `locales/en.yml` - source catalog (value equals key)
- `locales/zh-CN.yml` - 简体中文
- `locales/zh-TW.yml` - 繁體中文

To add or change a string:

1. Use it in the code as `t!("Full English sentence")`; keep interpolation as
   `%{name}` (rust-i18n's syntax - `{name}` is **not** replaced and would be
   shown literally).
2. Add the same key to all three catalogs, keeping every `%{name}` placeholder
   exactly once.
3. Run `cargo test i18n`. The guard tests fail when a `t!()` key is missing from
   a catalog, when a catalog still carries a key the code no longer uses, when a
   translation drops or renames a placeholder, or when the runtime switch
   between the three languages stops working.

Do not translate server-provided metadata (titles, artists, album and playlist
names), log messages, or MPRIS protocol values such as `Playing`/`Paused`.

## Product and Development Roadmaps

Product positioning, capability boundaries, information architecture, and the
ordered discovery roadmap are defined in [`PRODUCT_PLAN.md`](PRODUCT_PLAN.md).
The checkbox-based engineering backlog and completion record remain in
[`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md). Update the latter with the
completion commit whenever a milestone is finished; product scope changes must
be reflected in the former before implementation starts.

## Architecture

UI code imports GPUI through `gpui_kit` and styled components through
`gpui_kit::component`. Kit initializes the platform and window root; the
application asset source combines custom player icons with Kit's built-in icons.
Application icon loading and enumeration use one embedded-file registry, keeping
custom asset precedence and the existing icon paths.

Audio implementation modules import dependencies explicitly from their owning
modules rather than relying on parent wildcard imports. Decoder buffer sizes,
startup/stall timing, HTTP retry counts and disk-cache capacity live with their
implementations; the facade does not re-export internal stream types for them.

Shared decode-completion flags and cache-byte counters keep their atomic fields
private. Consumers use narrow query methods; publication retains the existing
acquire/release ordering for completion and relaxed ordering for byte progress.

- `src/api.rs` and `src/api/` - Subsonic client facade, authentication, and API modules
- `src/api/lyrics.rs` and `src/api/lyrics/response.rs` - capability-aware lyrics requests and legacy fallback, separate from pure variant selection, timestamp offsets and translation pairing; HTTP contract tests preserve request order and empty-result/error handling
- `src/audio.rs` - public audio handle, shared playback state and worker startup; no HTTP, file-cache or decoder implementation imports
- `src/audio/worker.rs` - worker startup, loop-owned playback resources and ordered playback phases
- `src/audio/worker/commands.rs` and `src/audio/worker/playback.rs` - command dispatch and seek coalescing, prepared-stream attachment, preloading eligibility and playback publication; shared worker ownership retains cancellation and device teardown order
- `src/audio/transport.rs` - pending playback intent, shared command ordering, worker acknowledgements and actual output progress for listening/reporting
- `src/audio/device_output.rs` - owned device stream, player and format; successful replacement before releasing old output, with stable EQ and gapless handles
- `src/audio/output_track.rs` - prepared-stream diagnostics and per-track loudness/gain/EQ/output-format assembly, with private source ownership and explicit sample consumption
- `src/audio/eq.rs` and `src/audio/eq/shared.rs` - native-rate per-channel filtering and opaque shared settings/revision publication; readers copy changed settings before updating coefficients, and cloned handles survive output replacement
- `src/audio/loudness_meter.rs` - cumulative raw-sample RMS/peak accounting and transparent source wrapping, separate from device discovery and output effects
- `src/audio/decoder.rs` - decoder construction, bounded PCM production and terminal-result classification
- `src/audio/buffered_stream.rs` - startup-buffer ownership, nonblocking playback consumption and stall timing
- `src/audio/stream_control.rs` - shared cancellation, decoder sleeping/promotion and lifecycle guards, preparation reuse waits, monotonic arrival timing, synthetic silence and first I/O failure retention
- `src/audio/preparation.rs` and `src/audio/preparation_state.rs` - foreground preparation ownership, cancellation on replacement/stop/shutdown, completion identity, wait timing, pause intent and thread creation failures
- `src/audio/track_state.rs` - current stream source, decode diagnostics, gapless clock boundaries and shared playback snapshots
- `src/audio/track_transition.rs` - reserved-track start/cancel arbitration, immutable boundary/loudness metadata and once-only event consumption, separate from sample-chain ownership
- `src/audio/stream_cache.rs` - disk-cache ownership, contiguous byte progress, partial-file reuse, speculative isolation and verified publication
- `src/audio/http_stream.rs` - HTTP byte cursors, cache reuse, range reads and retries
- `src/audio/stream_response.rs` - validated audio responses, range bounds, source length checks and bounded server-error diagnostics
- `src/audio/next_track.rs` - deferred reservations, prepared-stream reuse, armed handoff ownership and worker shutdown cleanup
- `src/audio/preload.rs` - next-track job scheduling, stale-result rejection and prepared-stream reuse, separate from shared stream control
- `src/audio/gapless.rs` - continuous output and sample-accurate prepared-track handoff, using the separate transition state machine
- `src/playback_timeline.rs` - shared song duration selection and bounded, saturating relative seeks for UI and system media controls
- `src/app.rs` and `src/app/` - GPUI orchestration, views, and isolated feature state
- `src/app/titlebar_view.rs`, `src/app/sidebar_view.rs`, `src/app/navigation.rs` and `src/app/view_state.rs` - window navigation, shared section presentation, isolated navigation/overlay transitions and detail-page relationships
- `src/app/home_view.rs`, `src/app/home_feature_view.rs`, `src/app/home_section_view.rs` and `src/app/daily_mix_view.rs` - Home composition, listening feature, lazy album sections and Daily Mix controls
- `src/app/player_controls_view.rs` and `src/app/favorite_button_view.rs` - shared player styles, shuffle/repeat controls and favorite buttons
- `src/app/player_view.rs` - responsive player bar layout, current-track artwork, metadata, navigation and accessory controls
- `src/app/player_progress_view.rs` - playback progress, buffering feedback and playback retry
- `src/app/volume_view.rs` and `src/app/volume.rs` - volume panel rendering and shared wheel-volume dispatch for the player bar and mini player
- `src/app/library_view.rs` and `src/app/library_skeleton_view.rs` - shared page controls and loading placeholders
- `src/app/browse_view.rs` - library browse pages, local sort menus and genre grids
- `src/app/album_grid_view.rs` and `src/app/artist_grid_view.rs` - reusable library artwork cards
- `src/app/metadata_links.rs` and `src/app/library_navigation.rs` - artist and album links sharing small navigation projections with song menus
- `src/app/equalizer_view.rs` - player equalizer popover, presets and band controls
- `src/app/scrolling_text_view.rs` and `src/app/scrolling_text_state.rs` - scrolling labels, bounded font-aware measurement cache and animation timing
- `src/app/settings_view.rs`, `src/app/settings_overlay.rs` and `src/app/settings_navigation_view.rs` - settings composition, overlay and shared section presentation
- `src/app/playback_settings_view.rs`, `src/app/desktop_settings_view.rs` and `src/app/appearance_settings_view.rs` - section-specific preference controls
- `src/app/shortcuts.rs`, `src/app/shortcut_edit.rs` and `src/app/shortcut_config.rs` - key dispatch, recording/conflict workflows and shortcut-only save-failure rollback
- `src/global_shortcuts.rs` and `src/global_shortcuts/` - shared binding validation and parsed registration keys, owned registration replacement and cleanup, and a separate Windows API/message-loop adapter
- `src/app/shortcut_repeat.rs` - matched-key repeat suppression with a shared 250ms clock, preserving selection-movement exemptions across library resets
- `src/app/shortcut_settings_view.rs` and `src/app/shortcut_row_view.rs` - shortcut section composition and shared app/global binding rows, preserving platform restrictions and feedback
- `src/app/server_settings_view.rs`, `src/app/server_list_view.rs` and `src/app/server_form_view.rs` - connection status, server rows and edit form
- `src/app/server_profiles.rs`, `src/app/server_lifecycle.rs` and `src/app/server_cache.rs` - profile changes, outgoing-server persistence, reconnect resets and restoration of server-scoped caches
- `src/app/server_profile_edit.rs`, `src/app/server_remove_dialog.rs` and `src/app/server_removal_state.rs` - server form workflows, removal dialog rendering and events, and single-use removal targets independent of reconnecting library state
- `src/app/queue_edits.rs` - owned queue additions, selected-index corrections, shuffle-history remapping and removal outcomes
- `src/app/queue_navigation.rs` - borrowed queue progression, controllable shuffle choices and prepared forward/backward history shared with preload targeting
- `src/app/playback_modes.rs` - shared shuffle/repeat edits and mode-only persistence, retaining requested runtime modes on save failure and avoiding unchanged radio saves
- `src/app/song_selection.rs` - scoped cursor, multi-selection, fixed range anchors and bounded remapping after list changes
- `src/app/song_scope.rs` - seven typed list scopes shared by rendering, selection, scrolling, playback and menus, with stable IDs, page/drawer routing and layout rules
- `src/app/song_playback.rs` - shared row, detail-page and multi-selection queue actions, validated playback plans and existing queue targeting; full-list appends retain scope origins while selected appends remain manual
- `src/app/song_view.rs` and `src/app/song_row_view.rs` - virtualized list layout with its multi-selection toolbar, and individual song rows with their playing/hover indicators
- `src/app/song_row_actions.rs` - shared row event dispatch with queue occurrence resolution for delayed clicks, moves, removals and menu opening
- `src/app/song_info_view.rs` - read-only track metadata dialog
- `src/app/playback_occurrence.rs` - playback identity, terminal handling and reporting once per occurrence
- `src/app/playback_start.rs` - prepared stream requests and accepted queue, selection, shuffle-history and resume-position updates
- `src/app/preload_state.rs` and `src/app/playback_preload.rs` - accepted next-track reservations, occurrence/quality reuse checks and one-time gapless event application
- `src/app/session_state.rs` - prepared session restoration, server identity checks, bounded resume/display/save timelines and save throttling
- `src/config/server_writer.rs` - serial background server-cache writes, one pending snapshot per server and data kind, retry ownership, flushes and cancellation before cache deletion
- `src/config/server_writer/mailbox.rs` - private synchronization, retry deadlines and fresh-attempt receipts; immutable write tasks carry their own revision through completion, preserving newer pending snapshots
- `src/app/polling.rs` and `src/app/redraw_state.rs` - background polling, playback redraw cadence, visibility recovery and shared cover animation eligibility
- `src/typography.rs` - installed-font discovery and platform/locale-aware UI font selection
- `src/vinyl.rs` and `src/vinyl/frames.rs` - current-cover texture ownership, progressive frame delivery and cancellation, and bounded background raster generation
- `src/vinyl/rotation.rs` - texture-independent rotation timing and adjacent-frame selection, advancing only when both frames are drawable
- `src/media_session.rs` and `src/media_session/` - shared media commands and snapshots, Linux MPRIS and Windows SMTC integration
- `src/media_session/linux/interfaces.rs` and `src/media_session/linux/artwork.rs` - MPRIS interface contracts and connection-owned local artwork, with separate file preparation, publication, replacement and directory cleanup
- `src/tray.rs` and `src/tray/` - shared commands and platform routing, backend-owned Linux SNI handles and Windows message loops, centralized Linux menu callbacks and isolated ARGB conversion
- `src/app/playback_transport.rs` - shared playback controls for UI, shortcuts, tray and media commands, plus manual stream retry
- `src/app/media_commands.rs` and `src/app/media_command_state.rs` - media command dispatch, queue capabilities, playback and seek decisions from the audio handle's current state
- `src/playlist.rs` - playlist edit validation and ordered song occurrences
- `src/app/playlists.rs` and `src/app/playlist_edit_state.rs` - serial playlist writes with request identity and initiating workflow
- `src/app/playlist_dialog.rs` and `src/app/playlist_dialog_view.rs` - playlist dialog inputs, focus, rendering and actions
- `src/app/song_menu.rs` - song menus with library snapshots and queue occurrence identity
- `src/import/matcher.rs` and `src/import/scoring.rs` - bounded library searches, retry and ordered matching orchestration, separate from pure metadata normalization, scoring and automatic acceptance
- `src/import/http.rs` - shared metadata request sending, HTTP status checks and JSON errors; each source retains its request format and API status rules
- `src/import/netease.rs`, `src/import/netease/hydration.rs` and `src/import/netease/response.rs` - ordered 500-track detail batches and one missing-track retry pass, separate from metadata decoding and complete-playlist validation
- `src/import/qq.rs` and `src/import/qq/response.rs` - public musicu POST requests separated from layered API checks, metadata decoding and unavailable-track accounting
- `src/recommendation.rs` and `src/recommendation/` - shared recommendation contracts, listening affinity, deterministic candidate ranking and multi-pass selection; selection owns uniqueness, diversity and Daily Mix bucket counts together
- `src/recommendation/daily.rs` and `src/recommendation/daily/` - Daily Mix orchestration, versioned cache snapshots, style admission and bounded fact collection with ID-only album request plans; shared metadata and genre-seed helpers keep affinity, style and provider requests consistent
- `src/recommendation/radio.rs` and `src/recommendation/radio/collection.rs` - song and artist radio orchestration and provider collection, preserving source phases and completion ordering; finalization handles source failures and caps the pool at 180 before shared ranking selects up to 24 tracks
- `src/test_support/http.rs` - test-only HTTP request capture with scripted responses and endpoint handlers shared by metadata import, Daily Mix, radio, lyrics and playlist editing tests; one transport owns header limits, read/write timeouts and worker cleanup
- `src/api/playlists/tests.rs` - playlist state simulation and HTTP contract tests using the shared transport, retaining ordered form parameters and large-playlist coverage
- `src/models.rs` and `src/models/` - shared API and playback data structures
- `src/config.rs` and `src/config/` - configuration persistence, server-scoped caches and migrations
- `src/secrets.rs`, `src/secrets/migration.rs` and `src/secrets/windows_credentials.rs` - credential facade, ordered config migration and Windows API adapter; isolated migration tests cover partial success, failure retention, retries and active-profile mirrors
- `src/msg.rs` - background task results delivered to the UI

Playback uses a read-through disk cache and a buffered HTTP reader. Servers
with byte-range support can start large files without a full download, resume
partial cache files, and seek by requesting only the required part of the
track. Settings provides Original, 128, 192, 256, and 320 kbps quality levels;
transcoded levels request an MP3 stream from Navidrome, and each quality profile
has an isolated cache. The decoded audio queue absorbs longer network
interruptions, while completed
tracks play directly from local storage. MP3, FLAC, AAC, Vorbis, and other
formats supported by `rodio` remain available. Ogg Opus sources cannot be
decoded locally (Symphonia has no released Opus decoder yet), so on the
Original quality level they automatically fall back to a 320 kbps server
transcode — the quality pill and the stream log show the effective level, and
a decode error explains the cause if the server refuses to transcode.

Transport controls share the latest requested position and playback intent while
the audio worker processes commands. Consecutive relative seeks accumulate,
and a seek immediately after selecting a track uses that track's position and
duration. Worker acknowledgements restore the live playback clock; stale results
cannot overwrite a newer request.
Buttons, shortcuts, tray actions and system media controls share the same
play/pause decisions. Each command reads the audio handle's latest accepted
state, so failed preparation or delivery cannot suppress a later retry or make
the next toggle use a pause state that was never accepted.

Manual queue edits update entries, the selected index and both shuffle-history
branches together. Single and batch play-next operations share the same ordered
insertion path. Play-next and append consume prepared song collections across
the application and queue storage, avoiding a second metadata copy; browsed
collections and retained menu snapshots are copied only at the action boundary.
Adding play-next tracks to an empty queue selects its first entry
and enables Play while waiting for an explicit playback request. Stop immediately
finishes the partial listening event, clears playback and pending lyrics/radio
state, and cancels pending track notifications; queued songs remain available.

Queue navigation prepares next/previous targets and optional history updates
without changing application state. Preload targeting shares the same selection
policy, including forward-history and reservation priority, the eight-entry
shuffle exclusion window, and natural versus manual repeat-one behavior.
Prepared history is committed through the existing accepted-audio start path.

Shuffle, repeat and radio-start mode edits share one persistence boundary.
Failed saves keep the requested modes in memory and in later session snapshots;
starting radio changes modes only when disabling repeat-one, preserving shuffle.

Queue edits also remap the list cursor, selected entries and range anchor.
Appending radio or continuation tracks keeps selection on existing entries.
Accepted queue replacements, restored sessions and changed library/history lists
clear selection in the affected scope; failed starts and stale or failed search
results preserve it. Toolbar counts and favorite summaries borrow selected songs;
playlist and favorite batch actions copy only their required IDs or keys. Delayed
queue row events follow the original occurrence through moves and insertions,
and become inert when that occurrence is removed or the queue is restored.

Playback requests prepare the signed URL, effective quality, cache identity and
starting position before changing the queue or current selection. Queue
replacement, shuffle navigation, radio starts and automatic continuation commit
only when the audio worker accepts the command. Rejected requests preserve the
selected track, shuffle history and restored position; deleting the current
entry stops playback if its replacement cannot start. Preloading uses the same
stream-profile preparation as foreground playback.

Session loading and legacy migration share the same server-address comparison,
including omitted HTTP schemes, trailing slashes, host case and default ports;
reverse-proxy paths retain their case. Restoration prepares queue entries,
selection, shuffle history and the resume position together. Saved positions,
displayed progress, seeking and startup requests share the current track's bounds,
while unknown track lengths keep their saved offset.
Play resumes a saved offset or starts from zero; Retry uses the current logical
position, so a finished track's old output position cannot restart it at the end.

Session, loudness, genre, listening-history and Daily Mix files share one
background writer so ordinary updates and periodic saves avoid serialization and
filesystem waits on the UI thread. Each server keeps its latest pending snapshot
for each data kind; failed writes retry after five seconds, including empty
collections, while other kinds can still be saved. Server switches and exit wait
for submitted saves. A switch back can use the latest unsaved snapshots, and
deleting a server's local data cancels all its pending retries and waits for any
active replacement before removing the files. Existing JSON formats and filenames
are preserved; startup reads, legacy migration and global preferences retain
their existing persistence paths.

Each queue entry has a runtime identity that survives moves and removals.
Replaying an entry creates a new playback occurrence; retrying its stream retains
that occurrence's reporting, listening history and media-session identity.
Repeated copies of the same song therefore have distinct media track paths, so
an old absolute-position command cannot seek the next occurrence. Runtime
identities are recreated on restoration; the saved session format is unchanged.

Playlist writes retain whether they came from a song menu, a playlist dialog or
the import review. Accepted results update only that workflow; failed imports
keep their matched tracks in order for retry, and stale or duplicate responses
cannot close a later dialog. Queue menu actions follow the selected occurrence
through moves and insertions. Removing or replacing that occurrence invalidates
the old Play/Remove action, even when the new queue contains the same song IDs.

Scrolling labels cache widths by text, font size and the full font configuration,
including family, weight, style, features and fallbacks. The cache retains at most
20,000 measurements and evicts the least recently used entry when full. Changing
fonts therefore measures the new layout without a bulk cache reset. Navigation
links carry only artist or album metadata, share ID validation with song menus,
and render as static text when the target ID is absent or blank. Song-to-album
navigation preserves the artist ID for the album detail header.

Settings headers and navigation share section titles and ordering. Preference
sections render independently, resolving the audio cache directory only in
Playback. Server form rendering, saving and cancelling share the same target
selection; an obsolete edit target cannot fall back to another server. An
unconfigured active server opens its form automatically, while configured
servers open their forms through the row's edit button.

Main titlebars and sidebar entries share section titles and icons. Expanded and
collapsed sidebar entries use the same navigation action and library-loading
rules; detail pages keep their library section selected, and both the back button
and Escape use the same parent mapping. Home album sections build their grids
only when expanded, preserving the six-album preview limit and the default
collapsed Random section. Navigation state is tested without a window: Escape
clears multi-selection before closing settings, then the queue drawer, then
returning from Now Playing or a detail page. Repeated Now Playing entry keeps
the original return page; entry and exit reset lyric following without
discarding cached lyrics or pending requests.

Preloading starts within the last 15 seconds only after the current track's
remaining audio has been decoded, keeping its playback independent of network
bandwidth. One speculative task prepares up to 16 chunks of 4096 frames
(about 1.37 seconds / 512 KiB of stereo PCM at 48 kHz), with the existing 1 MiB
HTTP read-ahead buffer. The decoder then waits on a condition variable. Queue
edits, seeks, playback-mode changes, quality changes and cache-directory changes
invalidate the preparation. A reserved shuffle choice is reused when advancing.
Frontend reservations are recorded after the audio worker accepts the command.
Reusing a preparation requires the same queue occurrence, audio quality and a
zero start position. A stale transition cannot consume a newer reservation, and
queue identity checks prevent another copy of the same song from being promoted.
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
