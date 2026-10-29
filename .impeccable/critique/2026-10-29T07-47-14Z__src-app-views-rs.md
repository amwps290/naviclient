---
target: native desktop player UI in src/app/views.rs
total_score: 25
p0_count: 0
p1_count: 3
timestamp: 2026-10-29T07-47-14Z
slug: src-app-views-rs
---
# Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|------:|-----------|
| 1 | Visibility of System Status | 3 | Loading, buffering, retry and selection states are mature; some mutations still rely on subtle opacity or transient toasts. |
| 2 | Match System / Real World | 3 | Music controls are familiar, but API, ReplayGain and diagnostics language leaks into the default experience. |
| 3 | User Control and Freedom | 2 | Escape/back/cancel paths are good; queue inspection replaces browsing context and destructive queue changes have no undo. |
| 4 | Consistency and Standards | 3 | Shared kit components are strong, but albums, playlists, song rows and custom clickable metadata use different interaction grammar. |
| 5 | Error Prevention | 2 | Disabled states and confirmations exist, but immediate single-click playback and dense nested row targets invite mistakes. |
| 6 | Recognition Rather Than Recall | 2 | Tooltips help, while hover-only play controls, clickable lyrics and metadata links remain under-signaled. |
| 7 | Flexibility and Efficiency | 4 | Configurable shortcuts, multi-select, virtualization, context menus and mini-player are unusually strong. |
| 8 | Aesthetic and Minimalist Design | 2 | Now Playing has identity; Home and library pages give repeated grids and row actions too much equal visual weight. |
| 9 | Error Recovery | 2 | Playback and paging retry are good; several failures expose raw detail or lack localized undo/recovery. |
| 10 | Help and Documentation | 2 | Settings descriptions and tooltips are thorough, but non-obvious interaction rules have no visible teaching layer. |
| **Total** | | **25/40** | **Acceptable — significant polish needed** |

# Anti-Patterns Verdict

This does not read as obvious AI-generated UI. The cover-derived palette, rotating vinyl/tonearm, synced bilingual lyrics, mini-player, recovery states and desktop shortcuts are product-specific and coherent.

The weaker impression is engineer-complete but unevenly art-directed. The expressive listening surface feels like a distinct music product, while browsing and settings fall back to familiar sidebar, repeated equal grids, bordered panels and dense rows. Identity is concentrated in one destination instead of carried through everyday navigation.

The deterministic detector returned exit code 0 and JSON `[]`. This is a false-clean signal: the detector targets HTML/CSS and silently does not understand native GPUI Rust. Native-source fallback found 14 custom pointer surfaces with no adjacent explicit focus/key semantics, two hover-only custom play overlays, one border-plus-`shadow_xl` settings card, five animation declarations with no reduced-motion branch, 60 distinct pixel literals, 31 spacing variants, and nine duplicated bordered/tinted settings panel blocks.

Browser overlays were not applicable because this is a native GPUI surface with no DOM or injectable document.

# Overall Impression

The application already has the hard part: capable playback, a credible desktop workflow and a memorable Now Playing identity. The largest opportunity is to make daily browsing feel as intentional as listening. Professional polish will come less from decoration and more from clearer hierarchy, safer row behavior, contextual queue access, responsive compositions and consistent semantic controls.

# What's Working

1. Playback status communication is mature: skeletons, buffering duration, categorized failures, exact retry and buffered progress reduce uncertainty.
2. Desktop efficiency is excellent: customizable shortcuts, keyboard multi-select, virtualized lists, persistent playback controls, mini mode and context menus respect serious library users.
3. Now Playing has a real identity through cover-derived color, vinyl mechanics and synced lyrics rather than generic visual effects.

# Priority Issues

## [P1] Custom clickable surfaces are not accessibility-complete

Custom pointer surfaces include clickable metadata, album covers, genre cards, playlist rows, collapsible Home headings, lyric lines and mini-player information. The source contains fourteen such surfaces but no adjacent explicit focus, tab-index, key activation or focus-handle patterns. Album and artist hover play overlays are custom icon compositions without tooltips or keyboard hooks. Inactive lyrics can fall to 28% foreground opacity and mini controls use very small glyph/control dimensions.

Why it matters: keyboard and assistive-technology users cannot reliably discover or invoke major navigation and playback paths. Low-opacity lyrics and tiny controls also reduce low-vision and motor accessibility.

Fix: create one semantic interactive row/card primitive with focusability, Enter/Space activation, accessible name/state and visible focus. Label sliders and inputs, raise inactive lyric contrast, and keep compact control hit targets at least 28–32 px.

Suggested command: `$impeccable audit src/app/views.rs`

## [P1] Compact-window behavior is arithmetic rather than structurally responsive

The expanded sidebar is fixed at 216 px, Settings targets 960×650, player left/right zones reserve 260 px each, album cards are fixed at 176 px, and song rows reserve fixed metadata/action columns. The development plan still has the 900 px queue-overlap and recurring visual QA checks unverified.

Why it matters: at the documented 900×600 minimum, the player center, multi-action bar, translations and song columns have too little room. Hiding one technical chip below 1080 px is not a complete compact composition.

Fix: define regular and compact layouts. Auto-collapse the sidebar, hide secondary song columns, move secondary actions into overflow, collapse player side zones, and switch Settings navigation to a narrow-width tab/selector. Verify light/dark and all languages at 900×600.

Suggested command: `$impeccable adapt src/app/views.rs`

## [P1] Song rows conflate selection, navigation and immediate playback

Single click clears multi-selection and immediately plays, while the same row contains clickable album/artist metadata, favorite, play-next, add-to-queue and overflow actions. This contradicts familiar desktop-library selection behavior and the documented double-click expectation. Multi-selection is indicated with a 3 px colored side stripe rather than an explicit selection control.

Why it matters: users can interrupt playback while trying to select or inspect a row. Nested hit areas demand precision and the interaction grammar is hard to learn.

Fix: single click selects/focuses, double click or a dedicated affordance plays, and Enter plays the focused selection. Use a checkbox/check indicator for multi-select, keep one primary inline action, and reveal secondary actions on hover/focus plus the overflow menu.

Suggested command: `$impeccable distill src/app/views.rs`

## [P2] Browse pages do not inherit the player's identity or hierarchy

Home presents four similarly weighted collapsible album grids and opens with API connection status rather than listening context. Album and artist grids use rigid repeated tiles, while most emotional color and motion are reserved for Now Playing. Source metrics also show 60 distinct pixel values, 31 spacing variants and nine repeated one-off settings panel blocks, indicating local styling decisions that weaken system-level rhythm.

Why it matters: browsing feels like a library administration surface, then abruptly becomes a designed music player after entering Now Playing.

Fix: make Home choose a primary purpose—resume listening first—then subordinate recent, frequent, new and random content into differentiated rows. Carry a restrained trace of the current cover accent into selection and player-adjacent states. Consolidate spacing, radius, opacity and settings-section recipes into reusable tokens/components.

Suggested command: `$impeccable bolder src/app/views.rs`, followed by `$impeccable document`

## [P2] Queue is a destination when it should be contextual

Opening Queue replaces the full content route, interrupting discovery and losing spatial continuity. Settings can remain an infrequent modal destination, but Queue is a continuous listening utility.

Why it matters: inspecting or editing what plays next should not force users to leave the album, artist or search context they are browsing.

Fix: build a right-side Up Next drawer preserving the current view and scroll position. Pin the current track, support drag reorder and keyboard moves, provide playback history, and add undo for remove/clear actions. Keep `Q` as the toggle.

Suggested command: `$impeccable shape contextual Up Next drawer`

# Persona Red Flags

## Alex — Power User

Alex benefits from shortcuts, multi-select and virtualization. Friction appears when Queue replaces the library, reordering is discrete rather than direct, and the six-action multi-select bar gives every command equal weight. Alex expects drag reorder, preserved browse position and compact overflow for secondary batch actions.

## Jordan — First-Timer

Jordan is likely to misread song rows: a row click plays, metadata text navigates, covers and icons have separate behavior, album play is hover-only, and lyric lines can seek without teaching. Terms such as ReplayGain, kbps, sample rate and Navidrome API appear without a simple default layer.

## Sam — Accessibility-Dependent User

Sam cannot assume clickable `div` surfaces enter focus order or announce their roles. Artist/album links, covers, genre cards, playlists, Home headings, lyrics and mini-player information lack consistent explicit semantic behavior. Small mini controls, 28% inactive lyric opacity and the lack of reduced-motion handling are concrete barriers.

# Minor Observations

- Settings repeats bordered, tinted cards for most groups, making every preference look equally important.
- The Settings modal pairs a border with `shadow_xl`; one elevation treatment would be cleaner.
- The single `LIBRARY` group label is acceptable, but should not expand into repeated uppercase eyebrow scaffolding.
- Player error text truncates while the full detail lives in a mouse tooltip, weakening keyboard recovery.
- Favorite pending state relies mainly on 60% opacity; a real loading/disabled state would communicate better.
- Five animations exist with no reduced-motion branch; the 20-second repeating mini-player gradient is decorative rather than state-essential.
- Fixed 110 px form labels and 12 px muted text need long-translation and 200% scaling checks.

# Questions to Consider

- Is Home primarily for resuming, discovering or managing? Four equal grids avoid choosing.
- Is Queue a place users go, or context they should glance at while browsing?
- Should single click mean select, or interrupt what is currently playing?
- Which two or three traits besides the vinyl view should make every page unmistakably this player?
- How much technical audio detail belongs in the default interface versus an advanced diagnostics layer?

# Provisional Next Product Feature

After the UI consistency pass, the highest-value feature is a contextual Up Next drawer with playback history, drag reordering and undo. It improves an every-session workflow, reuses existing queue infrastructure and removes the largest navigation interruption. After that, native Windows/macOS media-session completion is the next foundational platform feature.
