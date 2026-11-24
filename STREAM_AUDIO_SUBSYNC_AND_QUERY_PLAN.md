# Solus Playback Integrity, Zero-Prompt Resolver & Automated Voice-Sync Plan

> **Scope & Status**: Investigation & Architecture Plan (awaiting review before Rust edits).
> **Targets**:
> 1. Fix ZokoAnime Ep 1 playback failure & MegaPlay HD-1 Ep 1 silent playback (*Chainsmoker Cat* / *Yani Neko* & general CDNs)
> 2. Eliminate the unnecessary `Choose Playback Query` modal & fix wrong-anime provider matching (*AnimeGG* OVA mismatch)
> 3. Elevate the Sub Sync Bar above mpv's OSC controls + fix HUD codec overflow and missing-glyph `□` boxes
> 4. Replace naive `±15s` cue snapping with an automated **Speech-VAD `SNAP TO VOICE` engine** + provider-scoped subtitle delay persistence

---

## 1. Empirical Root-Cause Analysis

### 1A. ZokoAnime Ep 1 Fails to Play (*Chainsmoker Cat*, MAL `63403`)
Live probe comparison between Episode 1 and Episode 2 on ZokoAnime (`hls1.drama1.cfd`):

| Episode | Segment 0 URL | Leading Magic Bytes | `mpv` Result |
| :--- | :--- | :--- | :--- |
| **Ep 1** | `.../1080/seg_00000.ts.jpg` | `b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR...IEND\xaeB`\x82'` (fake 1×1 PNG header, TS sync `0x47` starts at byte offset `252`) | **FAILS (`rc=2`)**: `[lavf] error reading packet: Invalid data found when processing input. ...treating it as fatal error.` |
| **Ep 2** | `.../1080/seg_00000.ts.jpg` | `b'G@\x11\x10\x00B\xf0%...'` (`0x47` at offset `0`) | Plays normally |

1. **Disguised PNG Wrapper on TS Segments**: Even with `allowed_segment_extensions=ALL`, `ffmpeg/libavformat` probes the first bytes of `seg_00000.ts.jpg`, sees `\x89PNG`, fails to decode MPEG-TS packets, and aborts fatally. Stripping the leading fake PNG prefix up to the first verified MPEG-TS sync triplet (`buf[i] == 0x47 && buf[i+188] == 0x47 && buf[i+376] == 0x47`) immediately yields a valid `1920x1080` H.264 + `48 kHz` AAC transport stream (`ffprobe`-verified).
2. **Redundant `--audio-file=master.m3u8` on Muxed Zoko Streams**: `derive_master_playlist_url()` (`src/api/player.rs:422-433`) matches *any* URL ending in `/index.m3u8` two directories deep. Because Zoko variant URLs end in `/1080/index.m3u8`, Solus attaches `--audio-file=.../master.m3u8` even though Zoko streams already carry muxed audio — doubling HTTP requests and stalling startup (`mpv` probe timed out at 15s on Ep 2 with redundant `--audio-file`).

---

### 1B. MegaPlay HD-1 Ep 1 Plays With No Sound (*Chainsmoker Cat*)
Live probe of MegaPlay HD-1 (`fetch.nexabloom.top/.../master.m3u8`):

```m3u8
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aac",NAME="Japanese",DEFAULT=YES,AUTOSELECT=YES,LANGUAGE="ja",CHANNELS="2",URI="audio/ja/index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aac",NAME="English",DEFAULT=NO,AUTOSELECT=NO,LANGUAGE="en",CHANNELS="2",URI="audio/en/index.m3u8"
... (11 audio languages total)
#EXT-X-STREAM-INF:BANDWIDTH=5500000,RESOLUTION=1920x1080,CODECS="avc1.4d401f,mp4a.40.2",AUDIO="aac",NAME="1080p"
index-f1.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2",AUDIO="aac",NAME="720p"
index-f2.m3u8
```

- **Root Cause**:
  1. `parse_megaplay_embed()` (`src/api/subtitles.rs:1372`) already parses `ResolvedStreamLadder` including `audio_variants` (`audio/ja/index.m3u8`, etc.), **but drops them** when building `QualityOption` and `PlayOptions`.
  2. Solus passes the video-only playlist `index-f1.m3u8` directly to `mpv`.
  3. `derive_master_playlist_url()` (`src/api/player.rs:424`) returns `None` because `index-f1.m3u8` does **not** end with `"/index.m3u8"`.
  4. Result: `mpv` plays a **video-only HLS stream with zero `--audio-file` attached** -> **100% silent playback**.

---

### 1C. Unnecessary "Choose Playback Query" Modal & AnimeGG OVA Mismatch
1. **Redundant Modal (`src/main.rs:3385`, `src/db/cache.rs:96-114`)**:
   - `open_episode_options_flow()` opens `Screen::PlaybackQuery` whenever `anime.playback_queries().len() > 1`.
   - Because `Anime::playback_queries()` appends `"{title} {season_year}"` whenever `season_year` is present, almost **every** anime forces the user to pick between e.g. `"Tokyo Ghoul"` and `"Tokyo Ghoul 2014"` despite already clicking the exact AniList entry.
2. **Wrong-Anime Matching on AnimeGG (`src/api/subtitles.rs:1445-1500`)**:
   - Searching `"Tokyo Ghoul"` on AnimeGG returns:
     `['/series/tokyo-kushu', '/series/tokyo-ghoul-2nd-season', '/series/tokyo-ghoul-jack', '/series/tokyo-ghoul-pinto', '/series/tokyo-ghoulre', ...]`
   - Season 1 on AnimeGG uses the Romaji slug `/series/tokyo-kushu`.
   - `fetch_animegg_provider()` only searches `query` (`"Tokyo Ghoul"`) and scores slug substrings naively (`+10` per matching word). `/series/tokyo-ghoul-jack` and `-pinto` score **+20** (`tokyo` + `ghoul`), beating `/series/tokyo-kushu` (**+10**) — so AnimeGG serves the **640×360 OVA** instead of Season 1 Episode 1, while Solus overlays Season 1 subtitles onto an unrelated episode!

---

### 1D. Sub Sync Bar Clashes With mpv OSC + Info HUD Visual Glitches
Observed directly from the *Tokyo Ghoul* screenshot (`1.39.06 pm`):
1. **OSC Overlap & Stolen Clicks**: Sub Sync Bar renders at `y = 642..698` (`src/api/player.rs:1033-1042`) inside the bottom zone occupied by mpv's built-in `osc.lua`, visually overlapping the timeline/buttons and allowing OSC to intercept `MBTN_LEFT` clicks.
2. **Codec Text Overflow**: `video-codec` (`"H.264 / AVC / MPEG-4 AVC / MPEG-4 PART 10"`) at `x=66` overflows into Column 2 (`x=460`), colliding directly with `"Output : 48.0 kHz · 2 ch"`.
3. **Missing Glyph `□`**: `⚡` (`U+26A1`) is missing from the default macOS libass fallback font, rendering as `□ DEMUXER CACHE` and `□ SNAP TO VOICE [y]`.
4. **HUD Stale on Click**: `handle_btn_click()` (`src/api/player.rs:1053-1076`) calls `render_player_buttons()` without `render_info_hud()`, so HUD `Subs: ON (+0.70s)` does not update immediately on click. Clicking `X` (`sub_cls`) only closes the bar without clearing HUD state cleanly.

---

### 1E. Why `SNAP TO VOICE` Failed & Subtitles Desynced Wildly on AnimeGG
1. **`snap_sub_to_voice()` in `SOLUS_AUDIO_LUA` (`src/api/player.rs:927-958`) Never Listens to Audio**:
   - It only searches `±15.0s` for the single nearest cue timestamp to `cur_t`.
   - When subtitles are shifted by `>15s` (e.g., OVA mismatch or alternate broadcast cut), it exits with `"Speak while hovering a dialogue scene (+-15s) to snap"` and changes nothing.
   - Within `±15s`, it blindly picks the nearest cue timestamp even during silence, BGM, or overlapping dialogue lines.
2. **Backend Audio Alignment Limitations (`src/services/subsync.rs:395-432`)**:
   - `estimate_audio_lead_time()` only probes `0..45s` for the first `silence_end`, lacks `Referer` headers and `allowed_segment_extensions=ALL`, has no voice bandpass filter (so OP music/SFX trigger false speech), and clamps corrections to `0.40..=4.5s`.
3. **Cross-Provider Offset Pollution (`src/db/user.rs:1005-1035`)**:
   - `anime_subtitle_delays` is keyed only by `(profile_id, anime_id)`. Fixing a `+18s` cut offset on **AnimeGG** silently shifts **ZokoAnime** and **MegaPlay** (which were already synced at `0.00s`) by `+18s` on subsequent launches.

  5. **MegaPlay HD-2 / Distinct Alt Mirror** (`id = "hd2"`) + **AnimeHeaven Direct** (`id = "animeheaven"`) — Expanded 5-Provider Roster for Maximum Uptime & Clean Direct Streams:
     - **ZokoAnime** (`id = "zoko"`) — MAL-ID direct muxed HLS + local PNG-unwrapper proxy
     - **MegaPlay HD-1** (`id = "hd1"`) — HiAnime TCDN demuxed HLS + explicit `--audio-file` ladder wiring
     - **AnimeHeaven Direct** (`id = "animeheaven"`) — Independent direct MP4 (`search.php` -> `anime.php` `gatea("...")` -> `gate.php` with `Cookie: key=...` -> `<source src="https://...video.mp4?...">`)
     - **AnimeGG Direct** (`id = "animegg"`) — Direct MP4 with multi-alias (`title_romaji` + AniList `synonyms`) & strict TV/OVA disambiguation
     - **MegaPlay HD-2 / Distinct Alt Mirror** (`id = "hd2"`) — Distinct HiAnime server or alternate non-duplicate CDN mirror

---

## 2. Architecture & Design Sketches

### Pillar 1 — Local HLS Segment Unwrapper Proxy & Explicit Audio Attachment

```
  Upstream CDN (e.g. Zoko Ep 1)            MegaPlay HD-1 (Demuxed HLS)
  .m3u8 + seg_00000.ts.jpg                 master.m3u8 -> index-f1.m3u8 + audio/{ja,en,...}/index.m3u8
            │                                            │
            ▼ (if disguised image header detected)       ▼
 ┌────────────────────────────────────┐    ┌───────────────────────────────────────────┐
 │ Solus Local Proxy (127.0.0.1:PORT) │    │ Explicit Ladder Audio Wiring              │
 │  • /playlist?url=...               │    │ QualityOption.audio_urls = [ja, en, ...]  │
 │    Rewrites relative & absolute    │    │ QualityOption.has_demuxed_audio = true    │
 │    URIs -> /seg?url=...            │    │   ├─ MegaPlay -> --audio-file=audio/ja/...│
 │  • /seg?url=...                    │    │   │               + remaining lang tracks  │
 │    Forwards Referer & User-Agent   │    │   └─ Zoko (muxed) -> NO --audio-file!     │
 │    Detects \x89PNG / GIF8 / \xFF\xD8│    └───────────────────────────────────────────┘
 │    Strips prefix -> verified 0x47  │                  │
 │    MPEG-TS sync triplet            │                  ▼
 └────────────────┬───────────────────┘    mpv / mpv.net / Celluloid
                  └───────────────────────► demuxer-lavf-o=allowed_extensions=ALL,
                                            allowed_segment_extensions=ALL
```

- **Deep Segment Health Probe (Provider Verification)**:
  - During `fetch_stream_info_with_network()`, inspect segment 0 of the top variant using HTTP `Range: bytes=0-4095` (or single GET fallback):
    1. Reject non-2xx / HTML error pages (dead servers never appear in the Provider list).
    2. Inspect magic bytes:
       - Direct `0x47` MPEG-TS triplet or MP4 (`ftyp`/`moof`): verified native stream.
       - Disguised image prefix (`PNG` / `GIF` / `JPEG`) containing a valid `0x47` triplet within first 4 KB: verified, mark `needs_hls_proxy = true` so playback automatically routes through `hls_proxy`.
       - Garbage / unrecoverable HTML/image without TS sync: mark variant/provider failed.

---

### Pillar 2 — Zero-Prompt Multi-Alias Identity Resolver

```
  User selects Anime Ep
          │
          ▼  (NO Screen::PlaybackQuery modal!)
 ┌─────────────────────────────────────────────────────────────────────────┐
 │ AnimeSearchIdentity                                                     │
 │  • anilist_id, mal_id                                                   │
 │  • aliases: [title_english, title_romaji, title_native, synonyms...]    │
 │  • season_year, expected_episodes, format ("TV" / "OVA" / "MOVIE" ...)  │
 └──────┬──────────────────────────┬──────────────────────────┬────────────┘
        ▼                          ▼                          ▼
   ZokoAnime                  HiAnime / MegaPlay         AnimeGG Direct
 Exact MAL ID lookup     Try aliases in priority     Search all aliases
 (100% deterministic)    Score title + year + ep     Prefer exact TV/Romaji slug
                                                     (/series/tokyo-kushu = TV 12ep)
                                                     Heavily penalize -ova/-jack/
                                                     -pinto/-movie/-re when format=TV
                                                     Drop AnimeGG if score < min_thresh
```

---

### Pillar 3 — Elevated Sub Sync Bar & Clean Bento Info HUD (1280×720 Canvas)

```
  y=26  ┌────────────────────────────────────────────────────────────────────────────┐
        │ ◈ SOLUS INFO HUD   [1080p FHD]   [WINDOWED]   [PINNED 1Hz]                 │
        │ Tokyo Ghoul — Episode 1                                                    │
        │ 01:14 / 24:10 (5%)  ·  -22:56 left  ·  Speed 1.00x                         │
        │ ▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  │
        │ ▶ VIDEO & QUALITY        ♪ AUDIO & CHANNELS       ◈ MASTERING & ENGINE     │
        │ Stream : 1920x1080 1080p Codec  : AAC · 128 kbps  Shaders : Fused + A4K    │
        │ Codec  : H.264 · HLS ABR Output : 48.0 kHz · 2 ch VideoOut: gpu-next       │
        │ Rate   : 23.98 fps       Volume : 100% · 1.00x    Dropped : 0 frames       │
        │ Decode : GPU · vt        Subs   : ON (+18.40s)    A/V Sync: +0.2 ms        │
        │ ◈ DEMUXER CACHE   24.5s ahead  ·  18.2 MiB  ·  2.4 MB/s  ·  Healthy        │
  y=378 └────────────────────────────────────────────────────────────────────────────┘
  y=390 ┌────────────────────────────────────────────────────────────────────────────┐
        │ SUB SYNC +18.40s │ [-5s][-1s][-0.2s][◈ AUTO-SYNC VOICE (y)][+0.2s][+1s][+5s][Rst][X]│
        │ Active Cue @ 01:14.2->01:17.0: "You don't understand what I am." [94% Lock]│
  y=484 └────────────────────────────────────────────────────────────────────────────┘

        (Standalone [S] mode without HUD -> docked at y=516..610, well above OSC!)
        [>>| SKIP INTRO [Tab]] (y=540..584)     [>>| NEXT EPISODE: Ep 2 [n/N]] (y=590..634)
  y=640 - - - - - - - - - - - mpv Bottom OSC Zone (No Clash!) - - - - - - - - - - - -
```

Fixes included:
- **Short Canonical Video Codec** (`mp.get_property("video-codec-name")` -> uppercase + alias map `AVC1 -> H.264`, `HEVC -> HEVC`, clamped to 18 chars): never touches Column 2 (`x=460`).
- **Universal Glyphs**: Replace `⚡` with `◈` and `⏭` with `>>|` across both HUD and Buttons (zero `□` Tofu boxes).
- **Wider Manual Bounds + Coarse/Fine Steps**: `[-5s]`, `[-1s]`, `[-0.2s]`, `[◈ AUTO-SYNC VOICE (y)]`, `[+0.2s]`, `[+1s]`, `[+5s]`, `[Reset]`, `[X]`, clamped to `±120.0s`, plus live active cue preview line so the user can visually verify dialogue alignment immediately.
- **Synchronized HUD Re-render**: Every click/hotkey on Sub Sync refreshes both `render_player_buttons()` and `render_info_hud()`.

---

### Pillar 4 — Automated Speech-VAD `SNAP TO VOICE` Engine (`-90s..+90s`)

```
  [y] or Click [◈ AUTO-SYNC VOICE] in mpv
          │  mp.commandv("script-message", "solus-ipc-auto-sync-sub", cur_t)
          ▼
  src/services/ipc.rs (IPC Reader)
          │  AppMessage::AutoSyncSubRequested { time_pos_secs }
          ▼
  src/main.rs (Background tokio::spawn — non-blocking!)
          │
          ├─► 1. Dual-Window Voice-Bandpass Extraction (ffmpeg -> stderr, zero disk bloat)
          │      Input: active playable URL (local hls_proxy URL if wrapped, else CDN + Referer)
          │      Window A (current scene): [max(0, cur_t - 45), cur_t + 45] (90s)
          │      Window B (anchor scene):  [max(120, cur_t + 60), cur_t + 150] (90s, skips OP)
          │      Filter: highpass=f=240,lowpass=f=3200,afftdn=nf=-25,
          │              silencedetect=noise=-28dB:d=0.35
          │      Output: spoken speech intervals [(s_i, e_i)] (drops <0.25s pops & >25s BGM)
          │
          ├─► 2. Wide Cross-Correlation (calculate_speech_vad_alignment)
          │      Parse subtitle cues [(s_j, e_j)] (strips signs/karaoke/styles)
          │      Coarse scan: -90.0s .. +90.0s (step 0.10s) -> keep top-3 peaks
          │      Fine scan:   peak ± 0.60s (step 0.02s) + FPS drift scales (23.976/24/25)
          │      Confidence:  Z-score = (best - mean) / std_dev, Prominence = best / 2nd_peak
          │
          └─► 3. Live Apply & Provider-Scoped Persist
                 If Z >= 2.6 && prominence >= 1.12:
                   • IPC set_property sub-delay <offset_secs>
                   • IPC script-message solus-sync-status {"ok":true,"offset":...,"conf":...}
                   • Persist (profile_id, anime_id, provider_id, delay_ms) in SQLite
                 Else (e.g. instrumental OP/ED):
                   • Preserve existing sub-delay; send solus-sync-status {"ok":false,...}
                   • OSD asks user to seek to spoken dialogue and press [y] (or use ±5s/±1s)
```

- **Auto-Sync on Cross-Provider Fallback**: When launching a stream where subtitles come from a *different* source than the video provider (e.g., AnimeGG Direct paired with Zoko/HiAnime/OpenSubtitles `.vtt`) and no cached provider delay exists yet, Solus automatically runs `auto_sync_subtitle_to_audio()` asynchronously once playback starts at `t=60..210s` and applies the correction live with a non-intrusive OSD notice.

---

## 3. Exact File-by-File Change Plan

1. **`src/services/hls_proxy.rs` (NEW) & `src/services/mod.rs`**
   - Expose `pub mod hls_proxy;` in `src/services/mod.rs`.
   - Implement `HlsProxyHandle` binding `127.0.0.1:0` with `tokio::net::TcpListener`, shared `reqwest::Client`, and registered upstream `Referer`.
   - `strip_disguised_image_prefix(bytes: &[u8]) -> &[u8]`: detects `\x89PNG`, `GIF87a`/`GIF89a`, `\xFF\xD8\xFF`, finds the first verified `0x47` MPEG-TS triplet (`i`, `i+188`, `i+376`), and returns `&bytes[i..]` (passes raw TS or MP4 bytes untouched).
   - Playlist proxy endpoint `/m3u8?u=...`: fetches upstream `.m3u8`, resolves every media/variant/key URI against base URL, rewrites `.m3u8` -> `/m3u8?u=...` and `.ts`/`.ts.jpg`/media segments -> `/seg?u=...`.
   - Segment proxy endpoint `/seg?u=...`: fetches upstream bytes, strips disguised image header if present, serves with `video/mp2t` + `Content-Length`.

2. **`src/api/subtitles.rs`**
   - Extend `QualityOption`:
     ```rust
     #[serde(default)] pub audio_urls: Vec<String>,
     #[serde(default)] pub has_demuxed_audio: bool,
     #[serde(default)] pub needs_hls_proxy: bool,
     ```
   - Extend `ProviderOption`: add `#[serde(default)] pub needs_hls_proxy: bool`.
   - Add `AnimeSearchIdentity` struct and update `fetch_stream_info_with_identity(...)` (keeping `fetch_stream_info_with_network(...)` delegating to it so existing callers/tests compile cleanly).
   - Attach resolved `audio_urls` from `ResolvedStreamLadder::audio_variants` (ordered by `is_dub`: Japanese-first for Sub, English-first for Dub, keeping all tracks for `#` live switching) onto every `QualityOption` returned by `parse_m3u8_ladder` / `parse_megaplay_embed` / `parse_stream_embed`.
   - Add `verify_stream_health(client, ladder_or_url, referer)`: Range-probe segment 0 of the top variant -> return `(reachable: bool, needs_hls_proxy: bool)`.
   - Upgrade `fetch_animegg_provider()` to iterate `AnimeSearchIdentity.aliases()`, rank slugs against expected format/season/episode count, penalize OVA/spin-off/sequel tokens when `format == "TV"`, and return `None` below confidence threshold.

3. **`src/api/anilist.rs`, `src/db/cache.rs`, `src/db/mod.rs`**
   - Query `synonyms` in AniList GraphQL (`src/api/anilist.rs`), store on `Anime` (`pub synonyms: Vec<String>` + SQLite column `synonyms TEXT NOT NULL DEFAULT '[]'` with `ALTER TABLE` migration in `src/db/mod.rs`).
   - Add `Anime::search_identity(&self) -> AnimeSearchIdentity` in `src/db/cache.rs`.

4. **`src/api/player.rs`**
   - Extend `PlayOptions`: `pub audio_urls: Vec<String>`, `pub has_demuxed_audio: bool`.
   - `mpv_network_kv_args()`: append `allowed_extensions=ALL,allowed_segment_extensions=ALL` inside `demuxer-lavf-o=...`.
   - `shared_mpv_kv_flags()`:
     - When `opts.has_demuxed_audio` (or `!opts.audio_urls.is_empty()`), emit `--audio-file=<url>` for each entry in `opts.audio_urls` instead of guessing from `derive_master_playlist_url()`.
     - When `has_demuxed_audio == false`, emit **no** redundant `--audio-file`.
   - `SOLUS_AUDIO_LUA` + `YOUTUBE_INPUT_CONF`:
     - Clean up `render_info_hud()`: use `video-codec-name` mapped to short names (`H.264`, `HEVC`, `AV1`, `VP9`) + 18-char clamp; replace `⚡` with `◈`.
     - Elevate Sub Sync Bar to `y = 390..484` (with HUD) / `y = 516..610` (standalone) + add `[-5s]`, `[-1s]`, `[-0.2s]`, `[◈ AUTO-SYNC VOICE (y)]`, `[+0.2s]`, `[+1s]`, `[+5s]`, `[Reset]`, `[X]` + Live Active Subtitle Cue Preview row + expand delay bounds to `±120s`.
     - Elevate Skip Intro/Outro pill (`y = 540..584`) and Next Episode pill (`y = 590..634`).
     - Replace `snap_sub_to_voice()` body with `mp.commandv("script-message", "solus-ipc-auto-sync-sub", tostring(cur_t))` + OSD `"◈ Listening to dialogue waveform (Speech-VAD)..."`, and register `mp.register_script_message("solus-sync-status", ...)` to display lock confidence & active delay immediately in both the Sync Bar and HUD.
     - Update `handle_btn_click()` and `adjust_sub_delay()` to re-render both `render_player_buttons()` and `render_info_hud()`.

5. **`src/services/subsync.rs`**
   - Implement `extract_speech_intervals_ffmpeg(stream_url, referer, start_secs, duration_secs)` with `highpass=f=240,lowpass=f=3200,afftdn=nf=-25,silencedetect=noise=-28dB:d=0.35` (falling back without `afftdn` if unsupported by older ffmpeg builds).
   - Implement `calculate_speech_vad_alignment(audio_intervals, sub_intervals)` with `-90.0s..+90.0s` search window, 20ms refinement, framerate drift scales, and Z-score + prominence confidence scoring (`confidence_pct: u8`).
   - Implement `auto_sync_subtitle_to_audio(...)` running dual-window extraction cleanly with timeouts so slow CDNs cannot hang.

6. **`src/services/ipc.rs`**
   - Detect `"solus-ipc-auto-sync-sub"` in `client-message` lines, parse current timestamp arg, and send `AppMessage::AutoSyncSubRequested { time_pos_secs }` over `mpsc::Sender<AppMessage>`.
   - Add `send_sync_result(sock, delay_secs, confidence_pct, ok, message)` to push `set_property sub-delay` + `script-message solus-sync-status` live into running `mpv`.

7. **`src/db/user.rs` & `src/db/mod.rs`**
   - Add migration & helpers for `anime_provider_sub_delays (profile_id, anime_id, provider_id, delay_ms, updated_at)` (`PRIMARY KEY (profile_id, anime_id, provider_id)`).
   - Load delay on episode selection/provider switch for `(profile_id, anime_id, provider_id)` (falling back to legacy `anime_subtitle_delays` when migrating).

8. **`src/main.rs` & `src/state/mod.rs`**
   - In `open_episode_options_flow()` (`src/main.rs:3385`): remove the `else if anime.playback_queries().len() > 1 { state.open_playback_query_picker(&anime); }` branch; always proceed directly into `open_playback_options(...)` and pass `anime.search_identity()` to `trigger_stream_detection()`.
   - When `needs_hls_proxy` is true for the chosen quality/stream, spawn/reuse `HlsProxyHandle`, rewrite `opts.stream_url` (and `opts.audio_urls` if needed) to `http://127.0.0.1:<port>/m3u8?u=...`, and keep the handle alive for the active playback session.
   - Add `AppMessage::AutoSyncSubRequested { time_pos_secs }` & `AppMessage::AutoSyncSubCompleted { anime_id, provider_id, result }` handlers in `main.rs`, plus automatic background Speech-VAD sync on cross-provider subtitle fallback when no provider-scoped offset is cached.

---

## 4. Verification Plan

### Automated Unit Tests (`cargo test`)
1. `hls_proxy::tests`:
   - `strip_disguised_image_prefix()` strips 252-byte fake PNG header cleanly to `0x47` MPEG-TS triplet; passes raw `0x47` TS segments untouched; rewrites relative & absolute M3U8 URLs to local proxy endpoints.
2. `subtitles::tests`:
   - MegaPlay demuxed `master.m3u8` (`index-f1.m3u8` + `audio/ja/index.m3u8` + `audio/en/index.m3u8`) populates `QualityOption::audio_urls` (ordered `ja`-first for Sub, `en`-first for Dub) with `has_demuxed_audio = true`.
   - Zoko muxed `master.m3u8` sets `has_demuxed_audio = false`.
   - `AnimeGG` matcher picks `/series/tokyo-kushu` over `/series/tokyo-ghoul-jack` for `Tokyo Ghoul` TV.
3. `player::tests`:
   - `shared_mpv_kv_flags()` attaches `--audio-file=http://.../audio/ja/index.m3u8` for MegaPlay, omits `--audio-file` for Zoko muxed streams, and includes `allowed_extensions=ALL,allowed_segment_extensions=ALL`.
   - `SOLUS_AUDIO_LUA` contains `solus-ipc-auto-sync-sub`, `solus-sync-status`, `y = 390` / `y = 516` Sub Sync coordinates (no `y = 642`), short `video-codec-name`, and zero `⚡` glyphs.
4. `subsync::tests`:
   - `calculate_speech_vad_alignment()` recovers synthetic offsets at `+18.40s`, `-42.50s`, and `+0.70s` with `Z >= 2.6`, and safely rejects uncorrelated/silent tracks.
5. `db::user::tests`:
   - Provider-scoped subtitle delays isolate `"zoko"`, `"hd1"`, and `"animegg"`.

### Live Verification (`cargo run --release`)
1. **Chainsmoker Cat Ep 1 — ZokoAnime**: Starts cleanly via local HLS PNG-unwrapper proxy with `1920x1080` video + audio.
2. **Chainsmoker Cat Ep 1 — MegaPlay HD-1**: Plays `1920x1080` video **with Japanese audio** (`#` cycles English/Spanish/etc. audio renditions).
3. **Tokyo Ghoul Ep 1**: Selecting the anime skips any `Playback Query` modal, AnimeGG matches Season 1 (or drops cleanly if unavailable), Info HUD shows crisp `H.264` with zero overlap and zero `□` glyphs, Sub Sync Bar sits cleanly above mpv's OSC with live cue preview, and pressing `y` / clicking `◈ AUTO-SYNC VOICE` snaps subtitles within `±90s` accurately to spoken voice.
