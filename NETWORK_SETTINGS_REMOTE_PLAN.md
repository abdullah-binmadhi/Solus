# SOLUS ARCHITECTURAL PLAN: Adaptive Network Modes, HLS Auto-ABR, Settings Hub v2 & Multi-Theme Mobile Remote

> **Neovim Navigation Tip**: Use `/^## ` to jump between major sections or `]m` / `[m`.
> All sketches, tables, and diagrams are formatted to fit 84-96 column Neovim splits.

---

## 1. Executive Summary & Core Goals

This plan integrates three tightly connected upgrades into Solus (`v1.0.7` -> next):

1. **Adaptive Network Modes + HLS Stream Ladder + Live Auto-ABR & Buffer Telemetry**:
   - Port Phaenna's network resilience system cleanly to Solus's **anime HLS (`.m3u8`) +
     external synced subtitles + multi-player (`mpv` / `iina` / `vlc` / `ani-cli`)** stack.
   - Replace `yt-dlp -J` with zero-overhead HLS master-playlist ladder parsing directly inside
     `src/api/subtitles.rs`, seamless `mpv`/`iina` IPC hot-switching that preserves subtitles,
     sub-delay, referrer, and split-audio tracks, and background artwork throttling.
2. **Settings Control Hub v2 Redesign (`[s]` — `src/ui/settings.rs`)**:
   - Fix the dead bottom void (~35-45% empty black space), missing scroll viewport, and lack
     of real-time diagnostics visible in the current Control Hub.
   - Make Settings the **live mission control deck** where users both **see what is happening
     right now** (active network mode, live mpv buffer gauge, throughput, stalls, HLS ladder
     rungs, Auto-ABR history, remote status) and **control everything** cleanly with zero
     overflow across desktop and compact/Raspberry Pi viewports.
3. **Mobile LAN Remote Complete Aesthetic Overhaul (`src/services/remote.rs`)**:
   - Preserve the proven single-card layout and 4-tab bottom drawer navigation while removing
     the generic "AI slop" dark-card styling and broken mobile Nerd Font glyphs (`□`).
   - Introduce a **12-Theme Multi-Genre Design System** (Y2K, Anime/Manga, Gaming/Mecha,
     Zen/Washi, Lo-Fi/Hi-Fi) that transforms **colors, surface textures, button & card shapes,
     custom inline SVG icon packs, scrubber geometry, typography, and embedded character art**
     (served locally from Solus's bundled `assets/loading/y2k_0..10.jpg` gallery via LAN HTTP).
   - Add live **Network Mode** switching and **Buffer Telemetry** directly to the phone UI,
     with bidirectional live theme sync between the phone and Solus Settings (`[s]`).

---

## 2. Domain Translation: Phaenna (YouTube/DASH) -> Solus (Anime/HLS)

| Capability | Phaenna (yt-dlp / DASH) | Solus (Anime HLS / Multi-Player) |
| :--- | :--- | :--- |
| **Stream Ladder** | `yt-dlp -J` separate video/audio URLs | Parse `#EXT-X-STREAM-INF` (`RESOLUTION`, `BANDWIDTH`, `AVERAGE-BANDWIDTH`, `FRAME-RATE`, `CODECS`, `AUDIO`) + `#EXT-X-MEDIA:TYPE=AUDIO` directly in `src/api/subtitles.rs` |
| **CDN Link TTL** | 3 hours + `expire=` query param | Signed expiry (`exp`/`expire`/`expires`/`e` > 120s left) OR conservative **10-min (`600s`)** TTL; auto re-resolves asynchronously if expired on stall |
| **Hot-Switch IPC** | `start`, `user-agent`, `audio-files`, `loadfile replace` | Must ALSO preserve `referrer=https://zokoanime.video/` (else HTTP 403), `sub-files` / `sid=no` + `sub-visibility`, `sub-delay`, `force-media-title`, and split-audio `master.m3u8` |
| **Loading Guard** | No image splash in playlist | Solus boots `mpv` with Y2K JPEG at `playlist-pos=0`; gate ALL buffer/stall telemetry on `!in_loading_screen && stream_started` |
| **Force IPv4** | `ytdl-raw-options-append=force-ipv4=` | `reqwest::ClientBuilder::local_address(0.0.0.0)` across API/stream/artwork + `yt-dlp --force-ipv4` + `vlc --ipv4` + `mpv` reconnect/IPv4 lavf options |
| **Bg Throttling** | Pause thumbnails during `paused_for_cache` | Pause covers, banners, and episode thumbnails on `PLAYBACK_PAUSED_FOR_CACHE`; tiered semaphores (`12/4` -> `2/1` -> `1/1`); compact AniList `/medium/` & Kitsu `medium` thumbs |
| **Global Shortcut** | `[W]` / `[p]->[w]` | `[w]`/`[W]` is taken on Solus (Wizard / Toggle Watched) -> use **`[N]` (Shift+N)** on Home/Detail/Settings, and **`[n]`/`[N]`** + click in PlaybackOptions |

---

## 3. End-to-End Pipeline Architecture

```text
+---------------------------------------------------------------------------------------+
|                   CONFIG & PERSISTENCE (config.toml + AppState)                       |
|  network_mode: Normal | CongestedWifi | UltraSaver     force_ipv4: bool (def false)   |
|  auto_abr_downshift: bool (def true)                   remote.theme_id / art_mode     |
+------------------+----------------------------------------+---------------------------+
                   |                                        |
      +------------v-------------------------+   +----------v--------------------------+
      | 1. HLS Stream Ladder & HTTP Layer    |   | 2. Network-Aware Artwork Gate       |
      |    (src/api/subtitles.rs)            |   |    (src/main.rs + src/api/kitsu.rs) |
      | - Parse master.m3u8 variants:        |   | - Normal:  12 idle / 4 playing      |
      |   height, tbr_kbps, fps, vcodec,     |   | - WIFI:     2 idle / 1 playing      |
      |   audio_group, audio_uri             |   | - ECO:      1 slot, minimal look-   |
      | - Cache ResolvedStreamLadder by      |   |   ahead, AniList /medium/ & Kitsu   |
      |   (anime_id, episode, is_dub)        |   |   medium thumbs, skip eager banners |
      | - Fallback tbr heuristic if CDN      |   | - Instant pause whenever atomic     |
      |   strips BANDWIDTH                   |   |   PLAYBACK_PAUSED_FOR_CACHE == true |
      +------------------+-------------------+   +-------------------------------------+
                         | PlayOptions (+ network_mode, force_ipv4, initial tier)
                         v
+---------------------------------------------------------------------------------------+
| 3. Player Launch Cushion (src/api/player.rs & src/services/downloader.rs)             |
| - mpv / direct iina:                                                                  |
|     --stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,    |
|                     reconnect_delay_max=3,multiple_requests=1                         |
|     --demuxer-lavf-o=http_persistent=1,http_multiple=1                                |
|     --hls-bitrate=<max|1800000|900000>   --demuxer-readahead-secs=<45|120|180>        |
|     --cache-pause-initial=yes            --cache-pause-wait=<2.5|4.5|6.0>             |
|     --network-timeout=<15|12|10>         --cache-on-disk=yes (WIFI/ECO)               |
| - vlc:     --network-caching=<3000|8000|12000>  [--ipv4]                              |
| - ani-cli: clamped -q <cap> + wrapper network flags   | yt-dlp: [--force-ipv4]        |
+------------------+--------------------------------------------------------------------+
                   | Unix Socket / Windows Named Pipe (JSON-RPC IPC)
                   v
+---------------------------------------------------------------------------------------+
| 4. Live IPC Telemetry & Auto-ABR Engine (src/services/ipc.rs <--> src/main.rs)        |
|                                                                                       |
|  mpv observes IDs 6..9 (gated on !in_loading_screen && stream_started):               |
|    [6] paused-for-cache       -> updates PLAYBACK_PAUSED_FOR_CACHE atomic immediately |
|    [7] cache-buffering-state  -> 0..100% fill gauge                                   |
|    [8] cache-speed            -> live bytes/sec throughput                            |
|    [9] demuxer-cache-state    -> fw-bytes + cache-duration (secs buffered ahead)      |
|            |                                                                          |
|            | AppMessage::PlaybackBufferTelemetry (500ms throttle / instant on stall)  |
|            v                                                                          |
|  main.rs::maybe_trigger_auto_abr_downshift():                                         |
|    Guards: !offline_local, !vlc, elapsed >= 6s, time_pos > 2s, cooldown >= 25s        |
|    Triggers (either):                                                                 |
|      (A) Multi-stall: >= 2 stalls in 60s sliding window                               |
|      (B) Throughput starvation: cache_speed_kbps < 60% of current_tbr_kbps            |
|            |                                                                          |
|            +--> Fresh ladder? --> select_lower_tier() --> HotSwitchStream IPC         |
|            +--> Stale ladder? --> async re-resolve    --> HotSwitchStream IPC         |
|            +--> Lowest rung in Normal mode? --> Toast + OSD suggesting [N] NET:WIFI   |
|                                                                                       |
|  Live Surfaces Updated Simultaneously:                                                |
|    [Settings Control Hub v2]   [Playback Options]   [Detail / Home]   [Mobile Remote] |
+---------------------------------------------------------------------------------------+
```

---

## 4. Network Mode Profiles (`NetworkMode` in `src/config.rs`)

| Parameter | `Normal` (`NET:FULL`) | `CongestedWifi` (`NET:WIFI`) | `UltraSaver` (`NET:ECO`) |
| :--- | :--- | :--- | :--- |
| Badge Color | Neutral / Dim | Amber `Rgb(251,191,36)` | Emerald `Rgb(80,250,123)` |
| `max_height_cap()` | `None` | `Some(720)` | `Some(480)` |
| `max_fps_cap()` | `None` | `Some(30)` | `Some(30)` |
| `hls_bitrate_cap()` | `"max"` | `"1800000"` | `"900000"` |
| `vod_readahead_secs()` | `45` | `120` | `180` |
| `vod_cache_pause_wait_secs()` | `2.5` | `4.5` | `6.0` |
| `use_disk_cache()` | `false` | `true` (`cache-on-disk=yes`) | `true` (`cache-on-disk=yes`) |
| `network_timeout_secs()` | `15` | `12` | `10` |
| `vlc_network_caching_ms()` | `3000` | `8000` | `12000` |
| `prefer_compact_artwork()` | `false` | `true` | `true` |
| Bg Image Semaphores (idle/play) | `12 / 4` | `2 / 1` | `1 / 1` |

### Two-Step HLS Downshift Algorithm (`ResolvedStreamLadder::select_lower_tier`)
1. **Step 1 — Same Resolution, Lower FPS / Bitrate**:
   - If `current_fps > 40.0` (or another variant at `height == current_height` exists with
     `tbr_kbps <= current_tbr_kbps * 0.85`), pick the lowest-cost variant at the same height
     with `fps <= 30.5`, preferring `av01` > `vp9`/`hev1` > `avc1`.
2. **Step 2 — Strictly Lower Resolution**:
   - Pick the highest `height < current_height` (e.g. `1080p -> 720p -> 480p -> 360p`),
     preferring `fps <= 30.5`, efficient codecs, and lower bitrate if tied.
3. **Bitrate Estimation Fallback** (when an HLS proxy strips `BANDWIDTH=`):
   - `>=1080p`: `3200 kbps`, `>=720p`: `1600 kbps`, `>=480p`: `850 kbps`, `<480p`: `500 kbps`.

---

## 5. Redesign #1: Settings Control Hub v2 (`[s]` — `src/ui/settings.rs`)

### 5.1 What Was Wrong in the Current Screenshot
- **~40% empty vertical deadspace** at the bottom of both the Left Action List and Right
  Contextual Inspector.
- **No list scrolling**: adding Network Mode, Auto-ABR, Force IPv4, and Remote Theme overflows
  off-screen on 32-38 row terminals.
- **Zero live awareness**: opening `[s]` shows static config badges, not whether playback is
  stalling, how many seconds are buffered, what rung Auto-ABR picked, or what theme is live on
  your phone.
- **Flat navigation**: users must linearly step through all categories instead of jumping
  straight to `Network`, `Remote`, or `Subtitles`.

### 5.2 Redesigned Desktop Layout (`width >= 96`, `height >= 28`)

```text
╭─ ❖ SOLUS // CONTROL HUB v2 ────────────────────── [ NET:WIFI ] · IPv4:OFF · ABR:ON ───────╮
│ ╭─  Search / Category ────────────────────────╮╭─ ◈ LIVE STREAM & NETWORK PULSE ───────╮ │
│ │ ❖ > Type to filter (or Tab / 1-5 category)...││ ● PLAYING  Frieren Ep 12  [720p]      │ │
│ │ [All] 1:Net 2:Play 3:Remote 4:Subs 5:System  ││ Buf 48s [██████░░░░] 2.4 MB/s  Stl:0  │ │
│ ╰──────────────────────────────────────────────╯╰───────────────────────────────────────╯ │
│ ╭─ ◈ Controls (18) ───────────────────────╮╭─ ❖ Inspector: Network Mode ────────────────╮ │
│ │ ── NETWORK & STREAMING                  ││  NETWORK & STREAMING   Live IPC Push Ready │ │
│ │ ▶ Network Mode           [ NET:WIFI ▾ ] ││  Shapes HLS cap, mpv cache cushion, disk   │ │
│ │   Auto-ABR Downshift           [● ON  ] ││  buffering, and background artwork budget. │ │
│ │   Force IPv4 Routing           [○ OFF ] ││                                            │ │
│ │   Step Down Quality Now  [ 720p->480p ] ││  PROFILE      FULL    ▶ WIFI ◀     ECO     │ │
│ │                                         ││  Height Cap   Max       720p      480p     │ │
│ │ ── TELEVISION & MOBILE REMOTE           ││  HLS Cap      Max     1.8 Mbps   900 kbps  │ │
│ │   Remote Theme        [ Lain Wired ▾ ]  ││  Readahead    45s       120s      180s     │ │
│ │   Remote Art Layer         [ Blend ▾ ]  ││  Pause Wait   2.5s      4.5s      6.0s     │ │
│ │   Mobile Remote QR          [ QR ◈ ]    ││  Disk Cache   Off        ON        ON      │ │
│ │   Open Remote in Browser  [ :8484 ↗ ]   ││  VLC Cache    3.0s      8.0s     12.0s     │ │
│ │   Mobile Remote Daemon      [● :8484 ]  ││  Bg Artwork   12x       2x/Cmp    1x/Cmp   │ │
│ │   Scan & Beam to TV      [ AirPlay ↗ ]  ││                                            │ │
│ │                                         ││  [Enter/→] Next Profile  [←] Prev Profile  │ │
│ │ ── VISUAL & PLAYBACK ENGINE             │╰────────────────────────────────────────────╯ │
│ │   Anime4K Shader Engine   [ ultra 4k ▾ ]│╭─ ◈ LIVE TELEMETRY & SESSION MONITOR ───────╮ │
│ │   Adaptive Ambilight           [○ OFF ] ││  LADDER   1080p -> [720p] -> 480p -> 360p  │ │
| │   Auto-Skip OP/ED              [● ON  ] ││  BUFFER   48.2s / 120s  (34.2 MB cached)   │ │
│ │                                        ┃││  RATE     2.4 MB/s · Fill 100% · Stalls: 0 │ │
│ │ ── CINEMATIC SUBTITLES                 │││  ABR LOG  Idle (cooldown clear · 25s rule) │ │
│ │   Typography Preset    [ Netflix Cln ▾]│││  BG ART   Throttled: 1 slot · Compact ON   │ │
│ │   Signage Collision Guard      [● ON  ]│││  REMOTE   192.168.1.42:8484 · Lain Wired   │ │
│ ╰─────────────────────────────────────────╯╰────────────────────────────────────────────╯ │
│  [↑/↓] Move  [←/→] Cycle  [Enter/Space] Apply  [Tab] Tab  [N] Net Mode  [Esc] Close       │
╰───────────────────────────────────────────────────────────────────────────────────────────╯
```

### 5.3 Inspector Views for Other Key Settings

**When `Remote Theme` or `Remote Art Layer` is focused (Upper-Right Card):**
```text
╭─ ❖ Inspector: Remote Theme ───────────────────────────────────────────────────────────────╮
│  TELEVISION & MOBILE REMOTE                                   Live WebSocket Sync Active  │
│  Changes colors, CSS textures, button geometry, SVG icon pack, and character art live.    │
│                                                                                           │
│  Theme:       [01/12] Lain Wired CRT          Genre:     Y2K / Phosphor Terminal          │
│  Shape:       Chamfered 0px Bezel             Icons:     Sharp Wireframe SVG              │
│  Texture:     CRT Scanlines + Vignette        Character: y2k_0.jpg (CRT Phosphor)         │
│  Swatches:    [██ BG #060b08]  [██ PRI #6effa8]  [██ ACC #ffd066]  [██ BDR #243e30]       │
│  Art Layer:   Blend (Poster + Ambient Character)   ·   Connected Phones: 1 live           │
│                                                                                           │
│  [Enter/→] Next Theme   [←] Prev Theme   (Updates open phones immediately over WS)        │
╰───────────────────────────────────────────────────────────────────────────────────────────╯
```

**When Playback is Stalling (Top Pulse + Lower Telemetry Card turn Amber):**
```text
╭─ ◈ LIVE STREAM & NETWORK PULSE ─────────╮
│ ▲ BUFFERING 42%  Frieren Ep 12  [1080p] │
│ Buf 1.8s [█░░░░░░░░░]  310 KB/s  Stl:2  │
╰─────────────────────────────────────────╯
╭─ ◈ LIVE TELEMETRY & SESSION MONITOR ──────────────────────────────────────────────────────╮
│  LADDER   [1080p] -> 720p (ABR Target) -> 480p -> 360p                                    │
│  BUFFER   1.8s / 45s (1.4 MB) · Fill 42% · PAUSED FOR CACHE                               │
│  RATE     310 KB/s (<60% of 3.2 Mbps) · Stalls: 2 in 60s                                  │
│  ABR LOG  Auto-downshifting 1080p -> 720p via mpv IPC...                                  │
│  BG ART   PAUSED (100% bandwidth yielded to video stream)                                 │
╰───────────────────────────────────────────────────────────────────────────────────────────╯
```

### 5.4 Settings Hub Interaction & Compact Fallback
- **Category Tabs (`settings_category: usize`, `0..=5`)**:
  - `0: All`, `1: Network`, `2: Playback`, `3: Remote`, `4: Subtitles`, `5: System`.
  - Pressing `Tab` / `BackTab` switches category tabs; typing ANY printable letter immediately
    switches into fuzzy search across all items (`SkimMatcherV2`); `Backspace` deletes filter;
    `Esc` clears filter first, then dismisses modal.
  - Note: Digit keys `1..=5` jump category tabs **only when `settings_query.is_empty()`**, so
    typing numbers like `"4k"` or `"8484"` into search always works!
- **Real Viewport Scrolling**: `settings_scroll_offset` tracks the visual line of the focused
  row + category header and draws a crisp proportional `┃`/`│` right-edge scrollbar.
- **Contextual `Manual ABR Step-Down` Action**: Appears enabled when playing with a lower HLS
  rung available; pressing `Enter` immediately triggers a seamless `HotSwitchStream` downshift
  from inside Settings so users can manually drop quality mid-episode without closing `mpv`!
- **Compact / Raspberry Pi Layout (`width < 96` or `height < 28`)**:
  - Collapses the persistent Lower Telemetry card into the 2-row Top Pulse strip so both
    Left Controls and Upper Inspector remain generous and readable down to `80x24`.

---

## 6. Quick-Access Surface Updates (`PlaybackOptions`, `Detail`, `Home`, `Help`)

- **Playback Options (`src/ui/play_options.rs`)**:
  - Header expands cleanly from `Length(4)` to `Length(5)` (invariant before & after stream
    load so columns NEVER shift):
    - Row 1: Title + `[Sub + Dub]` + `[Modern • 2023]`
    - Row 2: `Player` (`[p]`) · `Audio` (`[a]`) · `Anime4K` (`[u]`) · `Delay` (`[/]`)
    - Row 3: `Network: [ NET:WIFI ] ([n]) · Buf: 48s (34.2 MB) · 2.4 MB/s · Stl:0 · 100%`
      (or when idle: `Network: [ NET:WIFI ] ([n]) · Cap <=720p · Readahead 120s · ABR:ON`)
  - Adds clickable `network_chip_rect: Rect` to `PlaybackOptionsLayout` for mouse support.
  - Quality column title shows `Stream Quality (<=720p)` when capped and shows bitrate tags
    (`1080p · 3.4M`, `720p · 1.6M`) when parsed from `#EXT-X-STREAM-INF`.
- **Detail Screen (`src/ui/detail.rs`)**:
  - Top bar shows `[ NET:WIFI ]` / `[ NET:ECO ]` pill (or subtle `NET:FULL` when wide).
  - Active `Playback` line appends live telemetry (`· 720p · Buf 48s · 2.4 MB/s · Stl:0`).
  - Pressing **`[N]`** cycles Network Mode live (pushes IPC update to running `mpv`/`iina`).
- **Home Screen (`src/ui/home.rs`)**:
  - Top bar shows `[ NET:WIFI ]` / `[ NET:ECO ]` pill whenever `network_mode != Normal`.
  - Pressing **`[N]`** cycles Network Mode live and adjusts background cover concurrency.
- **Help Command Matrix (`src/ui/help.rs`)**:
  - Documents `[N]` (`Cycle network mode`) in System & Workspace quadrant.

---

## 7. Redesign #2: Mobile LAN Remote Overhaul (`src/services/remote.rs`)

### 7.1 Design Philosophy: Zero "AI Slop", Preserved Muscle Memory
What makes the current `REMOTE_HTML` feel generic/broken:
- Default dark SaaS card (`#07080c` + `#0f1118` + `#e63946` red glow + generic rounded rects).
- Nerd Font Private Use Area glyphs (`󰄬`, `󰕿`, `󰕾`) render as missing-glyph tofu (`□`) on
  iOS Safari and Android Chrome.
- Zero visual personality matching Solus's signature Y2K / anime loading screens.

**What stays identical (zero navigation re-learning):**
- Single-column mobile deck (`max-width: 440px`) with Header -> Hero Meta -> Scrubber ->
  Transport (`-10s` / `Play` / `+10s`) -> 2x2 Quick Actions -> Volume -> Fixed 4-Tab Bottom
  Nav (`Subtitles`, `Episodes`, `Audio & Speed`, `Queue`) + slide-up drawers.

**What transforms completely:**
1. **100% Custom Inline SVG Icon Packs** (7 distinct SVG stroke/fill families — zero broken
   Nerd Font glyphs on iOS/Android).
2. **Embedded Character Art Endpoint (`GET /assets/art/{0..10}.jpg`)**:
   - Serves Solus's existing 11 `include_bytes!()` character artworks from
     `src/services/loading_art.rs` (`pub fn raw_y2k_asset(idx: usize) -> &'static [u8]`)
     over local LAN HTTP with `Content-Type: image/jpeg` and `Cache-Control: public, max-age=86400`.
   - Zero external internet needed, zero binary bloat (bytes are already compiled into Solus).
3. **6-Axis Structural Theme Engine (`data-theme` + `data-shape` + `data-art` + SVG pack)**:
   Switching themes does NOT merely swap CSS hex values—it physically reshapes the UI:
   - **Colors & Polarity**: OLED dark, CRT phosphor, high-contrast light Manga ink, warm Washi
     rice paper, Game Boy olive LCD, gunmetal VFD.
   - **Procedural CSS Textures**: CRT scanlines, manga screentone halftone dots, Japanese washi
     paper grain, pixel dot-matrix grid, mecha caution stripes, brushed metallic ridges.
   - **Button & Card Geometry**:
     - `chamfer` (`clip-path: polygon(...)` 45-deg armored/hardware cuts — Mecha, Cyber, Akira)
     - `bezel` (`0px` sharp CRT frame + recessed 1px phosphor bevel — Lain Wired, Section-9)
     - `comic` (`2.5px solid` ink outline + hard `4px 4px 0` offset shadow — Manga, Shibuya)
     - `pixel` (stepped 8-bit pixel borders + chunky tactile A/B pads — Arcade 16-Bit, DMG)
     - `chrome` (liquid-metallic glossy capsule pills + 4-point star sparkles — Chrome Y2K)
     - `zen` (whisper-thin hairline borders + organic soft radii + hanko seal dot — Kyoto/Washi)
     - `deck` (physical Hi-Fi cassette rectangular keycaps + recessed LED grooves — Cassette)
   - **Scrubber Geometry**: phosphor laser beam, segmented LED bar, manga ink bar + diamond
     thumb, pixel blocks, tape counter strip, quiet hairline.
   - **Typography**: System monospace / condensed geometric / editorial serif / pixel styling.
   - **Character Art Integration (`Companion` | `Blend` | `Poster`)**:
     - `Companion` (default): Framed loading-screen-style character art strip at the top of the
       deck (with theme corner brackets / caption tag, seamlessly pairing with the anime poster
       in Hero Meta, and expanding cleanly when idle).
     - `Blend`: Subtle ambient blurred + themed character art backdrop behind the remote canvas
       with the anime poster sharp in front.
     - `Poster`: Pure playback card with anime cover art only.

### 7.2 Mobile Remote Layout Sketch (Same Structure + Network Telemetry + Character Art)

```text
+--------------------------------------------+
| [✦] SOLUS        [◈ Theme]  [FS]  (● LIVE) |  <-- Header (new Theme Studio trigger)
+--------------------------------------------+
| +----------------------------------------+ |
| | [=== CHARACTER ART STRIP / BANNER ===] | |  <-- Optional Companion Art (/assets/art/N.jpg)
| |  y2k_0.jpg · CRT PHOSPHOR   [ART:CMP]  | |      styled w/ theme frame, brackets & tint
| +----------------------------------------+ |
| | +------+  Sousou no Frieren            | |
| | |POSTER|  Ep 12 — A Real Hero          | |  <-- Exact same Hero Meta
| | |      |  [SUB·JPN] [ENG] [720p]       | |
| | +------+                               | |
| |                                        | |
| |  ===========●------------------------  | |  <-- Theme-shaped Scrubber (LED/CRT/Ink/Tape)
| |  08:14                         -15:46  | |
| |                                        | |
| |  [ NET:WIFI ▾ ]   Buf 48s · 2.4 MB/s   | |  <-- NEW: Tap to cycle Net Mode + Live Buffer
| |                                        | |      (flips amber "BUFFERING 42%" on stall)
| |       [ -10s ]   [  ▶  ]   [ +10s ]    | |  <-- Theme-shaped Transport + SVG Icons
| |                                        | |
| |  [ Skip Intro ]     [ Jump Recap +85s ]| |  <-- Same 2x2 Quick Actions (SVG Icons)
| |  [ Audio: JPN ]     [ Subtitles On/Off]| |
| |                                        | |
| |  [v-] ========●----------------- [v+] 80%|  <-- Same Volume Row (SVG Icons)
| +----------------------------------------+ |
|                                            |
| +----------+----------+----------+--------+|
| | Subtitles| Episodes | Audio/Net| Queue  ||  <-- Exact same 4-Tab Fixed Bottom Nav
| +----------+----------+----------+--------+|
+--------------------------------------------+

[Theme Studio Slide-Up Drawer — opened via Header [◈ Theme]]
+--------------------------------------------+
|                   ──────                   |
|  ◈ REMOTE THEME STUDIO                 [✕] |
|  [All]  [Y2K]  [Anime]  [Gaming]  [Zen]    |
|  +------------------+ +------------------+ |
|  | [art] Lain Wired | | [art] Chrome Y2K | |  <-- Visual mini-preview cards showing
|  | CRT · Bezel · ●  | | Ice · Capsule    | |      swatch, geometry, character thumb
|  +------------------+ +------------------+ |
|  +------------------+ +------------------+ |
|  | [art] Manga Ink  | | [art] NERV HUD   | |
|  | Tone · Comic     | | Mecha · Chamfer  | |
|  +------------------+ +------------------+ |
|  CHARACTER LAYER: [Companion] [Blend] [Off]|
|  CHARACTER ART:   [◀] 01 CRT Phosphor [▶]  |
+--------------------------------------------+

[Audio, Speed & Video Drawer — adds Network & ABR Section]
+--------------------------------------------+
|  PLAYBACK SPEED:  [0.75x][1.0x][1.25x][1.5]|
|  ANIME4K:         [Off] [Bal 2x] [Ultra 4K]|
|  NETWORK MODE:    [FULL] [● WIFI] [ECO]    |  <-- NEW: live IPC mode switch from phone
|  AUTO-ABR:        [● ON] · Buf 48s · Stl:0 |  <-- NEW: toggle ABR + live cache readout
|  AUDIO TRACKS:    (Japanese / English ...) |
+--------------------------------------------+
```

### 7.3 The 12 Curated Remote Themes

| # | ID | Name | Category | Polarity | Geometry | SVG Pack | Texture | Default Art |
|---:|---|---|---|---|---|---|---|---|
| 1 | `lain-wired` | Lain Wired CRT | Y2K | Dark | `bezel` (0px) | `wire-crt` | CRT scanlines + phosphor glow | `y2k_0` CRT Phosphor |
| 2 | `chrome-y2k` | Chrome Star Y2K | Y2K | Dark | `chrome` pill | `y2k-star` | Halftone starlight + chrome sheen | `y2k_9` Ice Matrix |
| 3 | `cyan-halo` | Cyan Halo Net | Y2K | Dark | `chamfer` | `y2k-star` | Soft grid + cyan halo | `y2k_8` Cyan Halo |
| 4 | `neon-cat` | Neon Catgirl Cyber | Anime | Dark | `chamfer` | `y2k-star` | Diagonal cyber grid | `y2k_5` Neon Cat |
| 5 | `shibuya-punk` | Shibuya Punk Zine | Anime | Dark | `comic` | `manga-ink` | Checkerboard + screentone | `y2k_7` Cyber Violet |
| 6 | `manga-ink` | Manga Ink & Tone | Anime | **Light** | `comic` (3px) | `manga-ink` | Paper grain + Ben-Day dots | `y2k_2` Ink Sketch |
| 7 | `akira-tokyo` | Akira Neo-Tokyo | Anime | Dark | `chamfer` | `mecha-hud` | Speed-stripe crimson HUD | `y2k_10` Teal Signal |
| 8 | `nerv-hud` | NERV Tactical HUD | Gaming | Dark | `chamfer` | `mecha-hud` | Warning stripes + hex reticle | `y2k_4` Mint Star |
| 9 | `arcade-16bit` | Arcade 16-Bit | Gaming | Dark | `pixel` | `pixel-8bit` | Pixel dot-matrix + CRT glow | `y2k_1` Cyber Audio |
| 10 | `pocket-dmg` | Pocket DMG LCD | Gaming | **Light** | `pixel` | `pixel-8bit` | Olive dot-matrix LCD grid | `y2k_6` Halftone Star |
| 11 | `kyoto-sumi` | Kyoto Zen Sumi | Zen | Dark | `zen` | `zen-brush` | Subtle sumi ink grain + hanko | `y2k_3` Midnight Deck |
| 12 | `washi-zen` | Washi Daylight Zen | Zen | **Light** | `zen` | `zen-brush` | Warm rice-paper texture | `y2k_2` Ink Sketch |
| *(opt)* | `cassette-hifi` | Midnight Cassette | Hi-Fi | Dark | `deck` | `hifi-deck` | Brushed aluminum + VFD amber | `y2k_3` Midnight Deck |

*(We will ship a tight **12-theme** roster—replacing `cyan-halo` with `cassette-hifi` so all 5
genres—Y2K, Anime/Manga, Gaming/Mecha, Zen Dark/Light, and Hi-Fi Cassette—are richly covered!)*

---

## 8. File-by-File Implementation Blueprint

### Phase 1 — Config & Data Models (`src/config.rs`, `src/state/mod.rs`)
- **`src/config.rs`**:
  - Add `NetworkMode { Normal, CongestedWifi, UltraSaver }` with all profile getters + `next()` / `prev()`.
  - Extend `Config` with `network_mode: NetworkMode`, `force_ipv4: bool`, `auto_abr_downshift: bool` (all `#[serde(default)]` for zero-friction migration).
  - Extend `RemoteConfig` with `theme_id: String` (default `"lain-wired"`), `art_mode: String` (default `"companion"`), `art_index: Option<usize>`.
  - Add `Quality::height_px()` & `Quality::clamped_for_network(mode)`.
- **`src/state/mod.rs`**:
  - Add `LivePlaybackTelemetry` struct (`anime_id`, `episode`, `is_dub`, `is_offline_local`, `started_at`, `current_height`, `current_fps`, `current_tbr_kbps`, `current_url`, `paused_for_cache`, `buffering_pct`, `cache_duration_secs`, `cache_fw_bytes`, `cache_speed_bps`, `stall_count`, `recent_stall_times`, `last_abr_switch_at`, `abr_downshift_count`, `last_abr_note`, `prompted_congested_mode`).
  - Add fields on `AppState`: `live_playback`, `stream_ladder_cache: HashMap<(i64, u32, bool), ResolvedStreamLadder>`, `network_mode`, `force_ipv4`, `auto_abr_downshift`, `settings_category: usize`, `settings_scroll_offset: usize`, `remote_ws_clients: usize`.

### Phase 2 — HLS Stream Ladder & Network Shaping (`src/api/subtitles.rs`, `src/api/player.rs`, `src/api/kitsu.rs`, `src/services/downloader.rs`, `src/services/queue.rs`)
- **`src/api/subtitles.rs`**:
  - Add `ResolvedVideoVariant`, `ResolvedAudioVariant`, `ResolvedStreamLadder`, `parse_m3u8_ladder`, `parse_url_expire_unix`, `is_fresh`, `select_best_variant`, `select_lower_tier`, `estimate_tbr_kbps_for_height`, and `build_http_client(force_ipv4, timeout_secs)`.
  - Attach optional `height`/`tbr_kbps`/`fps` to `QualityOption` and `ladder: Option<ResolvedStreamLadder>` to `StreamInfo`.
- **`src/api/player.rs`**:
  - Extend `PlayOptions` with `network_mode` + `force_ipv4`.
  - Inject verified `mpv` / `iina` / `vlc` / `ani-cli` network cache, reconnect, timeout, `hls-bitrate`, and disk-cache flags.
- **`src/services/downloader.rs`**: Pass `--force-ipv4` to `yt-dlp` when enabled.
- **`src/api/kitsu.rs`**: Support `prefer_compact: bool` (`medium` before `original`).
- **`src/services/queue.rs`**: Pass `network_mode` + `force_ipv4` into `resolve_item_prefetch` and cache resolved ladders.

### Phase 3 — Live mpv IPC Telemetry, Hot-Switch & Live Mode Updates (`src/services/ipc.rs`, `src/main.rs`)
- **`src/services/ipc.rs`**:
  - Add `PLAYBACK_PAUSED_FOR_CACHE: AtomicBool` + `is_playback_paused_for_cache()`.
  - Observe IDs `6..=9` (`paused-for-cache`, `cache-buffering-state`, `cache-speed`, `demuxer-cache-state`), strictly gated on `!in_loading_screen && stream_started`.
  - Emit throttled `AppMessage::PlaybackBufferTelemetry` (500ms cadence; immediate on stall transitions).
  - Add `HotSwitchRequest`, `send_hot_switch_stream`, and `send_set_network_mode`.
- **`src/main.rs`**:
  - Add `AppMessage::{PlaybackBufferTelemetry, StreamLadderResolved, AbrRefreshReady, RemoteNetworkModeChanged, RemoteAutoAbrToggled, RemoteThemeChanged, RemoteClientsChanged}`.
  - Implement `maybe_trigger_auto_abr_downshift`, `manual_abr_step_down`, `cycle_network_mode`, and `bg_image_acquire` (pauses on `PLAYBACK_PAUSED_FOR_CACHE`, uses tiered semaphores + AniList `/medium/` compact URLs).
  - Wire `[N]` on Home/Detail/Settings, `[n]`/`[N]` + mouse click on PlaybackOptions.

### Phase 4 — Redesigned Settings Hub v2 + Quick Surfaces (`src/ui/settings.rs`, `src/ui/play_options.rs`, `src/ui/detail.rs`, `src/ui/home.rs`, `src/ui/help.rs`)
- **`src/ui/settings.rs`**:
  - Full v2 Control Hub: Top Live Pulse Header + Category Tab Ribbon (`All/Net/Play/Remote/Subs/System`) + Scrollable Action Deck (`┃/│` scrollbar) + Two-Tier Right Stage (Contextual Inspector + Persistent Live Telemetry Monitor) + compact viewport fallback.
  - Add `HubActionId::{NetworkMode, AutoAbrDownshift, ForceIpv4, ManualAbrStepDown, RemoteTheme, RemoteArtMode}`.
- **`src/ui/play_options.rs`, `detail.rs`, `home.rs`, `help.rs`**:
  - Add invariant 5-row header in `PlaybackOptions` with clickable `NET:*` chip + live buffer bar; surface `NET:*` pills + live buffer stats on Detail/Home; document `[N]` in Help.

### Phase 5 — Multi-Theme Character-Backed Mobile Remote (`src/services/loading_art.rs`, `src/services/remote.rs`)
- **`src/services/loading_art.rs`**:
  - Expose `pub const Y2K_ART_COUNT: usize = 11`, `pub fn raw_y2k_art(idx: usize) -> (&'static [u8], &'static str)` so `remote.rs` serves embedded artwork with zero duplication.
- **`src/services/remote.rs`**:
  - Add `GET /assets/art/{0..10}.jpg` route in `handle_http_client` (returns raw bytes + `Cache-Control: public, max-age=86400`).
  - Track active WebSocket client count (`AtomicUsize`) and notify `AppMessage::RemoteClientsChanged`.
  - Extend `RemotePlaybackState` with `network_mode`, `auto_abr`, `force_ipv4`, `current_quality`, `buffer_secs`, `cache_mb`, `cache_speed_kbps`, `paused_for_cache`, `buffering_pct`, `stall_count`, `remote_theme`, `remote_art_mode`, `remote_art_index`.
  - Extend `handle_remote_action` with `set_network_mode`, `cycle_network_mode`, `toggle_auto_abr`, `set_remote_theme`, `set_remote_art`.
  - Rebuild `REMOTE_HTML` with:
    - 12 distinct multi-genre themes (`data-theme` + `data-shape` + procedural CSS textures)
    - 7 custom inline SVG icon packs (zero broken Nerd Font glyphs!)
    - Embedded character art banner/backdrop (`Companion` / `Blend` / `Poster` modes + 11-art picker)
    - Header `◈ Theme` Studio slide-up drawer
    - Live `NET:*` pill + buffer telemetry strip on the main card & Network/ABR controls inside the `Audio, Speed & Video` drawer.

---

## 9. Verification & Test Plan

1. **Unit Tests (`cargo test`)**:
   - `config::tests`: `NetworkMode` parameters, `Quality::clamped_for_network`, legacy `config.toml` backwards-compatible deserialization.
   - `api::subtitles::tests`: `#EXT-X-STREAM-INF` full ladder parsing (`BANDWIDTH`, `FRAME-RATE`, `CODECS`, `AUDIO`), expiry URL parsing, bitrate fallback, and two-step `select_lower_tier`.
   - `api::player::tests`: `mpv`/`iina`/`vlc` flag generation across all 3 `NetworkMode`s + `force_ipv4`.
   - `services::ipc::tests`: parsing `demuxer-cache-state`, `cache-buffering-state`, `cache-speed`, `paused-for-cache`, loading-screen gating, `loadfile replace` (`reason="stop"`) not triggering EOF/quit, and hot-switch JSON command ordering.
   - `services::remote::tests`: `/assets/art/0.jpg` serving, `set_network_mode` / `set_remote_theme` action handling, and `RemotePlaybackState` JSON round-trip.
   - `ui::settings::tests` & `ui::play_options::tests` (`TestBackend`): desktop (`140x42`) & compact (`80x24`) rendering with zero panics, zero clipping, invariant `PlaybackOptions` geometry, and scroll clamping across all 18+ hub actions.
2. **Manual Smoke Tests**:
   - Verify `[s]` Settings Hub v2 live telemetry while playing in `mpv`, cycling `NET:FULL -> WIFI -> ECO` live and confirming `mpv` OSD + buffer fill changes.
   - Simulate/trigger manual & automatic ABR step-down (`1080p -> 720p`) and verify exact timestamp resume, external subtitles, sub-delay, and audio stay intact.
   - Open `http://<lan-ip>:8484` on phone/browser: test all 12 themes, SVG icons, character art modes, network mode pill, and live two-way theme sync with Solus Settings `[s]`.
