# Solus: In-Player Crunchyroll/Netflix Buttons, Live Provider Filtering & 3-Layer Smart Subtitle Sync Plan

> **Goal:** Upgrade Solus's playback experience across all `libmpv` engines (`Auto-Best`, `mpv`, `mpv.net`, `Celluloid`) with:
> 1. **Clickable Crunchyroll / Netflix-style In-Player Buttons** (`⏭ SKIP INTRO`, `⏭ SKIP OUTRO`, `⏭ NEXT EPISODE`) driven by AniSkip + mouse hover + single-key shortcuts.
> 2. **Live Episode Provider Probing & Strict Filtering** so providers that do **not** have the selected anime/episode never appear in the Episode Launch Menu (`[o]`) and never throw "Playback error".
> 3. **3-Layer Clever Subtitle Synchronization** combining pre-play **Onset-Weighted & Speech-Locked Auto-Sync** (fixing subtle 0.5s–2.0s lags like *NieR:Automata Ver1.1a Cour 2*), a 1-click **`⚡ SNAP TO VOICE`** + **`[-0.5s][-0.1s][+0.1s][+0.5s]` Interactive Sync Bar** inside the player, and per-anime persistent sync offset memory.

---

## Table of Contents
1. [Root-Cause Analysis (Why *NieR:Automata* Had Slow Subs & Provider Errors)](#1-root-cause-analysis)
2. [System Architecture Overview](#2-system-architecture-overview)
3. [Feature 1: Crunchyroll / Netflix-Style In-Player Interactive Buttons](#3-feature-1-in-player-buttons)
4. [Feature 2: Live Provider Episode Probing & Automatic Filtering](#4-feature-2-provider-filtering)
5. [Feature 3: 3-Layer Clever Subtitle Synchronization](#5-feature-3-smart-subtitle-sync)
6. [Visual UI & Player Overlay Sketches](#6-visual-ui-and-player-sketches)
7. [File-by-File Implementation Plan](#7-file-by-file-implementation-plan)
8. [Verification & Live Testing Checklist](#8-verification-and-testing-checklist)

---

## 1. Root-Cause Analysis

Inspecting Solus's codebase revealed **exact reasons** behind all three friction points you experienced on *NieR:Automata Ver1.1a Cour 2*:

### A. Why Switching Providers Triggered "Playback Error"
In `src/api/subtitles.rs:387–448` (`default_providers`) and `949–995`, Solus statically populated **4 providers** (`ZokoAnime`, `HD-1`, `AnimePahe`, `GogoAnime`) regardless of whether they actually hosted that episode:
- Only `ZokoAnime` resolved a real `master_url`.
- `HD-1` had `master_url: None` and empty quality URLs, while `AnimePahe` / `GogoAnime` depended blindly on `ani-cli` succeeding later at launch time without verifying if the show or episode existed there.
- **Fix:** Probe each candidate provider concurrently before displaying the list and run `info.providers.retain(|p| p.is_verified_available)`. Any provider missing the episode simply **does not appear**.

### B. Why *NieR:Automata Ver1.1a Cour 2* Subtitles Were Slightly Slow
Four interacting gaps caused the subtle lag:
1. **`needs_subsync` skipped native `.vtt` tracks** (`src/main.rs:9796`): Only `[Auto-Synced]` community subtitles triggered `ensure_synchronized_subtitle`; primary `.vtt` tracks were assumed in-sync even when the video CDN encoded a broadcast recap/sponsor bumper (`~1.2s–2.0s` shift).
2. **`0.82` Identity Guard swallowed 0.5s–1.8s lags** (`src/services/subsync.rs:304–315`): Dialogue lines average 3–4 seconds, so a subtitle track shifted by `1.0s` still overlaps ~84% with the reference intervals — triggering the identity guard (`>= 82%`) and suppressing the fix!
3. **Cour 2 (`Season 1 Part 2`) episode mapping ambiguity** (`src/api/subtitles.rs:1173`): Split-cour sequels like *NieR:Automata Ver1.1a Cour 2* (`AniList #171032`) map on AniZip/OpenSubtitles either as `S01E13+` or `S02E01`, causing naive fallback lookups to match wrong-timing releases or skip alignment.
4. **Tier 2 audio voice detection was unwired** (`src/services/subsync.rs:386` `estimate_audio_lead_time` had `#[allow(dead_code)]`): When no second reference subtitle existed, there was zero comparison against the actual Japanese/English dialogue onset in the audio stream.

### C. Why In-Player Skip Intro & Next Episode Buttons Were Missing
In `src/services/ipc.rs:878`, Solus only did a background silent seek when `auto_skip_intro == true` and AniSkip had an exact match — with **no clickable overlay pill**, **no `+85s` Skip Intro fallback** when AniSkip lacked community markers, and **no Next Episode button** near `[ED]` / credits.

---

## 2. System Architecture Overview

```
+======================================================================================+
|                           SOLUS STREAM + SYNC ORCHESTRATOR                           |
+======================================================================================+
|                                                                                      |
|  1. PROVIDER PROBING (`src/api/subtitles.rs`, 3.5s bounded concurrent tokio::join!)   |
|     +-- ZokoAnime Direct (`zokoanime.video/stream/mal/...` + HEAD/GET `.m3u8`)       |
|     +-- HiAnime HD-1 Direct (`hianime.at` search -> server extract -> `.m3u8`)       |
|     +-- AllAnime Direct (`api.allanime.day` GraphQL verified episode stream)         |
|     +-- Legacy CLI Sources (`AnimePahe` / `GogoAnime` ONLY if episode verified)      |
|            |                                                                         |
|            v  Strict filter: `providers.retain(|p| p.verified && has_playable_url)`   |
|     [ Unavailable providers are completely hidden from Playback Options ]              |
|                                                                                      |
|  2. 3-LAYER SMART SUBTITLE SYNC (`src/services/subsync.rs` + `solus-audio.lua`)      |
|     +-- Layer 1 (Pre-Launch): Cour2 absolute-ep check + 3x Cue-Onset Cross-Corr      |
|     |    + `0.95` Identity Guard + `ffmpeg silencedetect` Voice Onset Alignment       |
|     +-- Layer 2 (In-Player): `[ ⚡ SNAP TO VOICE (y) ]` cue-lock + `[-0.5][-0.1]..`  |
|     +-- Layer 3 (Memory): Per-Anime `sub_delay_ms` persisted to SQLite + Remote      |
|                                                                                      |
|  3. CRUNCHYROLL / NETFLIX IN-PLAYER BUTTON ENGINE (`ipc.rs` <-> `solus-audio.lua`)   |
|     +-- IPC sends `script-message solus-overlay-ctx <json>` (OP/ED bounds, Next Ep)  |
|     +-- Lua `mouse-pos` + `osd-dimensions` hit-testing on 1280x720 ASS canvas        |
|     +-- Conditional `MBTN_LEFT` click + Keyboard (`Tab` Skip OP, `N` Next Ep, `S` Sync)|
|     +-- stdout bridge (`__SOLUS_NEXT_EPISODE__`, `set_property sub-delay`) -> Solus  |
+======================================================================================+
```

---

## 3. Feature 1: In-Player Crunchyroll & Netflix Buttons

Works across **all unified `libmpv` engines (`Auto-Best`, `mpv`, `mpv.net`, `Celluloid`)** via one shared Lua overlay controller in `solus-audio.lua`:

### How Clickable ASS Buttons Work Inside `libmpv`
1. `ipc.rs` resolves AniSkip `[OP]` and `[ED]` intervals AND looks up the next episode number/title (`ep + 1`, checking aired count / queue).
2. `ipc.rs` pushes a lightweight JSON context message to `libmpv`:
   ```json
   { "command": ["script-message", "solus-player-ctx", "{\"ep\":14,\"next_ep\":15,\"next_title\":\"Ep 15\",\"op_start\":82.0,\"op_end\":171.5,\"ed_start\":1290.0,\"ed_end\":1378.0,\"auto_skip\":false}"] }
   ```
3. Inside `solus-audio.lua`, a dedicated `ass-events` overlay (`btn_ov`) renders crisp floating glass pills in the bottom-right corner and tracks cursor hover (`mouse-pos` mapped against `osd-dimensions` normalized to `1280x720`).
4. When the cursor is over a button hitbox, `MBTN_LEFT` intercepts the click and triggers the button action; anywhere else on the video canvas, left-click continues normal pause/play behavior.

### Button Rules & Fallback Intelligence

| Button Pill | When It Appears | Action on Click / Key | Smart Fallback When AniSkip Has No Data |
| :--- | :--- | :--- | :--- |
| **`⏭ SKIP INTRO`**<br>*(Hotkeys: `Tab` or Click)* | Inside AniSkip `[OP]` window **OR** during `0:05..2:30` when mouse moves | Seeks cleanly to `op_end` (or `+85s` when fallback) & shows OSD badge `✦ Skipped Intro (+85s)`. | If `auto_skip_intro` is ON, shows a 2.0s pill `[ ⏭ Skipping Intro... (Esc Cancel) ]` so you can override it anytime; if OFF, shows `[ ⏭ SKIP INTRO ]` like Netflix/Crunchyroll. |
| **`⏭ SKIP OUTRO`**<br>*(Hotkey: `Tab` or Click)* | Inside AniSkip `[ED]` window | Seeks past credits or jumps to post-credits scene / triggers Next Episode. | Works alongside `Next Episode` pill seamlessly. |
| **`⏭ NEXT EPISODE`**<br>*(Hotkey: `N` or Click)* | During `[ED]` window, final `<= 90s`, **`>= 88%` watched**, OR bottom-right mouse hover in final `20%` | Emits `__SOLUS_NEXT_EPISODE__\n` over Lua stdout -> `main.rs` marks current episode watched if `>= 85%`, closes cleanly, and **immediately launches Episode `N+1`** (using pre-warmed cache if queued/prefetched!). | Shows exact next episode label, e.g. `⏭ Next: Ep 15 (0:08)` with a subtle auto-advance progress bar if Watch Queue Auto-Advance is enabled. |

---

## 4. Feature 2: Live Provider Episode Probing & Strict Filtering

No more clicking a broken provider and getting slapped with `"Playback failed"` after launching:

1. **Concurrent Provider Resolution (`fetch_stream_info_with_network` in `src/api/subtitles.rs`)**:
   - Instead of calling `default_providers()` with dummy URLs, Solus probes each candidate provider in parallel (bounded by a 3.5s per-provider timeout):
     - **`ZokoAnime` (`id: "zoko"`)**: Checks `zokoanime.video/stream/mal/{mal_id}/{ep}/{sub|dub}` + validates `#EXTM3U` response.
     - **`HiAnime HD-1` (`id: "hd1"`)**: Queries `hianime.at` server list for that exact episode & checks both `HD-1` and `Vidstreaming` server items (`data-type="sub"` / `"dub"`), extracting its own real `.m3u8` master URL & qualities (not an empty placeholder!).
     - **`AllAnime Direct` (`id: "allanime"`)**: Queries AllAnime GraphQL `episode(showId, translationType, episodeString)` and extracts verified MP4/M3U8 source URLs directly with zero `ani-cli` dependency.
     - **`AnimePahe` / `GogoAnime`**: Included **only if** `ani-cli` binary exists on the host AND episode lookup confirms the episode index exists on that source (`has_episode == true`).
2. **Strict Filtering Before Rendering Column 1 (`Provider`)**:
   - Each `ProviderOption` gains:
     ```rust
     pub is_verified: bool,
     pub verified_res: Option<String>, // e.g. "1080p", "1080p BD"
     ```
   - `info.providers.retain(|p| p.is_verified);`
   - If the user toggles `[a] Sub <-> Dub`, `trigger_stream_detection` re-runs and **filters out providers that lack the Dub or Sub track for that episode**.
   - While loading (`state.is_loading_subs`), the `Provider` column displays `Probing available sources…` instead of showing unverified placeholders.

---

## 5. Feature 3: 3-Layer Clever Subtitle Synchronization

Designed so you **almost never** encounter slow/early subtitles — and when a weird encode ever drifts, it fixes in **one click**:

### Layer 1: Pre-Play Automated Prevention (`src/services/subsync.rs`)
1. **Apply Safe Sync to ALL External & Native `.vtt`/`.srt`/`.ass` Tracks**:
   - Even primary `.vtt` tracks can lag behind TV broadcast encodes by `+0.5s` to `+2.0s` (recap bumpers, sponsor slates, studio logo cuts — the exact *NieR:Automata Ver1.1a Cour 2* issue).
2. **Onset-Weighted Edge Cross-Correlation (Fixes 0.4s–2.0s "Slightly Slow" Subtitles)**:
   - Instead of uniform boolean interval overlap, score cue **speech onsets** (first `350ms` of each spoken cue) with **3x weight**, and use a `25ms` bin resolution.
   - Tighten the **Identity Guard** threshold from `0.82` -> **`0.95`** (with plateau tolerance `>= 0.25s`), so a subtle `0.6s–1.5s` lag is never falsely ignored as "already in sync".
3. **Cour 2 / Split-Season Episode Verification (`src/api/subtitles.rs`)**:
   - When querying `api.ani.zip/mappings?anilist_id=...` for split seasons (e.g. *Cour 2*, *Season 2 Part 2*), verify exact `(season_number, episode_number)` OR AniList title year matching before accepting OpenSubtitles/Stremio tracks so Cour 1 episodes never contaminate Cour 2 sync.
4. **Tier 2 Audio Voice-Onset Anchor (`estimate_audio_lead_time` wired in!)**:
   - When no reliable second reference track exists, run fast non-blocking `ffmpeg -ss 0 -t 60 -af silencedetect` on the first 60s of the HLS audio stream to locate `first_speech_sec` (skipping OP music if `aniskip` reports `op_start < 15.0`) and compare `first_sub_cue_sec - first_speech_sec`.
   - If the gap is between `0.35s` and `4.5s` (classic broadcast logo/bumper offset), automatically align timestamps before spawning `mpv`.

### Layer 2: In-Player `⚡ SNAP TO VOICE` + Interactive Sync Bar (`solus-audio.lua`)
Even if a release has a unique weird mid-episode ad cut, you don't need to leave fullscreen or dig out your phone:
- Press **`S`** (or click **`[Sub Sync]`** in the Info HUD) to toggle the compact **Subtitle Sync Bar** along the bottom of the video.
- Clickable buttons + hotkeys:
  - **`-0.5s`** (`Z`) · **`-0.1s`** (`z`) · **`⚡ SNAP TO VOICE`** (`y`) · **`+0.1s`** (`x`) · **`+0.5s`** (`X`) · **`Reset`** (`0` inside Sync Bar)
- **How `⚡ SNAP TO VOICE` (`y` / Click) Works**:
  1. Press **`y`** (or click `⚡ SNAP TO VOICE`) the exact moment a character starts speaking a line.
  2. `solus-audio.lua` fetches the cached/local subtitle file path from `track-list` / `solus_sub_path`, scans dialogue cues within `±12s` of current `time-pos`, picks the nearest cue start `t_cue` matching the current spoken phrase window, and sets `sub-delay = time-pos - t_cue` instantaneously!
  3. Shows OSD confirmation: `✦ Snapped Subtitles to Voice (-1.20s)` — zero audio stutter, instant visual lock.

### Layer 3: Per-Anime Persistent Offset Memory (`db/user.rs` + `ipc.rs`)
- Whenever you adjust `sub-delay` inside `mpv`/`mpv.net`/`Celluloid` (via `[ -0.1s ]`, `⚡ SNAP TO VOICE`, `z`/`x`, or Mobile Remote), `ipc.rs` observes `sub-delay` changes and persists the offset per **`(profile_id, anime_id)`** in SQLite (`anime_subtitle_offsets` table).
- When you launch **Episode 15** of the same anime (*NieR:Automata Cour 2*), Solus automatically starts with that exact `-1.20s` calibration applied!

---

## 6. Visual UI & Player Sketches

### Sketch 1: Crunchyroll / Netflix-Style In-Player Interactive Overlay (`mpv`, `mpv.net`, `Celluloid`)

```
+---------------------------------------------------------------------------------------------+
|  SOLUS // NIE:R AUTOMATA COUR 2 - EP 14                              [ Sub ]  [ 1080p HD ]   |
|                                                                                             |
|                                                                                             |
|                         ( 2B speaks on screen... )                                          |
|                                                                                             |
|                                      [ Subtle Dialogue Caption ]                            |
|                                                                                             |
|                                                                                             |
|                                              +-------------------------------------------+  |
|                                              |  >>| SKIP INTRO  [Tab / Click]            |  |
|                                              +-------------------------------------------+  |
|                                              (During OP / First 2:15 hover)                 |
|                                                                                             |
|                                    -- OR near Ending / Credits --                           |
|                                                                                             |
|                                              +-------------------------------------------+  |
|                                              |  >>| NEXT EPISODE: Ep 15  [N / Click]     |  |
|                                              +-------------------------------------------+  |
|                                                                                             |
|   -- When user presses [S] or sees out-of-sync dialogue --                                  |
|  +---------------------------------------------------------------------------------------+  |
|  |  SUB SYNC (-1.20s) :  [-0.5s]  [-0.1s]   [ ⚡ SNAP TO VOICE (y) ]   [+0.1s]  [+0.5s]  |  |
|  +---------------------------------------------------------------------------------------+  |
+---------------------------------------------------------------------------------------------+
```

### Sketch 2: Filtered Episode Launch Menu (`[o]` / `Enter`) — Only Verified Providers Shown

```
+---------------------------------------------------------------------------------------------+
|  > NieR:Automata Ver1.1a Cour 2 • Ep 14    [Sub + Dub]   [Modern Digital • 2024]             |
|  Engine: [ Auto-Best (mpv) ] ([p])   Audio: [ Sub ] ([a])   SubSync: [ Auto -1.2s ] ([y])   |
|  Shaders: [ Balanced 2x ] ([u])   Network: [ NET:FULL ] ([n])   [PiP:OFF]   [i HUD]         |
+------------------+------------------------+------------------------+------------------------+
|  PLAYER          |  VERIFIED PROVIDERS    |  STREAM QUALITY        |  SUBTITLES             |
| > Auto-Best *    | > ZokoAnime  [✓ 1080p] | > 1080p FHD (Direct)   | > English (Locked)     |
|   mpv (100%)     |   HiAnime HD-1[✓ 1080p]|   720p HD              |   Arabic [Auto-Synced] |
|                  |                        |   480p SD              |   Russian [Auto-Synced]|
|                  |  (Dead sources hidden: |                        |   No subtitles         |
|                  |   AnimePahe & Gogo     |                        |                        |
|                  |   unavailable for Ep14)|                        |  Saved Offset: -1.20s  |
+------------------+------------------------+------------------------+------------------------+
| [ Enter Launch ]   Left/Right Col  Up/Down Pick  [y] Toggle SubSync  [p] Engine   [ Esc ]   |
+---------------------------------------------------------------------------------------------+
```

---

## 7. File-by-File Implementation Plan

1. **`src/api/subtitles.rs` — Concurrent Provider Availability Probing & Filtering**:
   - Add `is_verified: bool` & `verified_res: Option<String>` (`#[serde(default)]`) to `ProviderOption`.
   - Implement parallel probing inside `fetch_stream_info_with_network`:
     - Probe `ZokoAnime`, `HiAnime HD-1` (full embed parse for HD-1/Vidstreaming), and `AllAnime` GraphQL concurrently with `tokio::join!` / bounded timeouts.
     - Only include `AnimePahe` / `GogoAnime` when `ani-cli` is available AND episode existence checks succeed (or mark unavailable and strip via `providers.retain(|p| p.is_verified)`).
     - Ensure switching `[a] Sub <-> Dub` re-filters providers based on actual track verification (`supports_dub` / `supports_sub`).

2. **`src/services/subsync.rs` — Onset-Weighted Auto-Sync + Voice-Lead Wiring**:
   - Add `parse_subtitle_cues_with_text` + onset-weighted alignment scoring (`3x` bonus for `[t_start .. t_start + 0.35s]`) with fine `25ms` bins and `0.95` Identity Guard threshold so `0.4s–2.0s` lags are reliably detected.
   - Wire `estimate_audio_lead_time` (`ffmpeg silencedetect`) for single-track streams / subtle TV bumper shifts when an audio track URL is available and delay exceeds `0.35s`.
   - Respect Cour 2 `(season_number, episode_number)` verification in `fetch_external_subtitles`.

3. **`src/db/mod.rs` & `src/db/user.rs` — Per-Anime Subtitle Offset Memory**:
   - Add `anime_subtitle_offsets (profile_id, anime_id, sub_code, delay_ms, updated_at)` table (or `anime_settings` column) + `get_anime_sub_delay_ms` / `save_anime_sub_delay_ms`.
   - Load saved per-anime offset automatically in `open_playback_options` / `start_playback`, and update it whenever the user snaps or adjusts delay inside the player.

4. **`src/api/player.rs` — Interactive In-Player Buttons & Sub Sync Bar in `SOLUS_AUDIO_LUA`**:
   - Add `YOUTUBE_INPUT_CONF` bindings: `Tab` (`solus-skip-intro`), `N` (`solus-next-episode`), `S` (`solus-toggle-sub-bar`), `y` (`solus-snap-sub-voice`), `z`/`x` (`-0.1s`/`+0.1s`), `Z`/`X` (`-0.5s`/`+0.5s`).
   - Add `btn_ov` clickable ASS overlay inside `SOLUS_AUDIO_LUA` with `mouse-pos` + `osd-dimensions` coordinate transform and conditional `MBTN_LEFT` click dispatch:
     - `⏭ SKIP INTRO` / `⏭ SKIP OUTRO` pill (`Tab` / Click)
     - `⏭ NEXT EPISODE` pill (`N` / Click -> prints `__SOLUS_NEXT_EPISODE__\n`)
     - Interactive `SUB SYNC` bar with clickable `[-0.5s] [-0.1s] [⚡ SNAP TO VOICE] [+0.1s] [+0.5s] [Reset]` buttons + live subtitle cue parser for `⚡ SNAP TO VOICE`.

5. **`src/services/ipc.rs` & `src/main.rs` — Bi-Directional Overlay Context & Next-Episode Handler**:
   - Send `solus-player-ctx` via `ipc.rs` as soon as AniSkip intervals + next episode metadata settle.
   - Observe `sub-delay` property changes in `ipc.rs` and emit `AppMessage::SubDelayChanged(delay_ms)` so Solus persists per-anime delay + syncs the Mobile Remote UI live.
   - Listen for `__SOLUS_NEXT_EPISODE__` on child `stdout` in `src/main.rs` (just like `__SOLUS_AUDIO_CYCLE__`), mark completion if `>= 85%`, and seamlessly spawn `episode + 1`!

6. **`src/ui/play_options.rs` — Clean Provider UI & SubSync Chip**:
   - Update Column 1 (`Provider`) to show loading state while probing (`Probing sources…`), verified resolution badges (`[✓ 1080p]`), and hide unavailable providers completely.
   - Show per-anime saved subtitle delay badge & `[y] Auto-Sync Sub` toggle chip in the header.

---

## 8. Verification & Live Testing Checklist
1. **Unit Tests (`cargo test`)**:
   - Verify onset-weighted alignment catches a `+1.1s` / `-1.2s` *NieR:Automata*-style subtitle shift that previously triggered the `0.82` Identity Guard.
   - Verify `ProviderOption` strict filtering hides empty/unverified providers cleanly.
   - Verify SQLite per-anime `sub_delay_ms` roundtrip persistence.
   - Verify `solus-audio.lua` & `youtube-input.conf` contain clickable button bindings (`solus-skip-intro`, `solus-next-episode`, `solus-snap-sub-voice`, `solus-toggle-sub-bar`).
2. **Live Playback Test (`cargo build --release`)**:
   - Open *NieR:Automata Ver1.1a Cour 2* Ep 14 -> verify only verified working providers appear (zero broken fallback errors).
   - Verify `[ ⏭ SKIP INTRO ]` & `[ ⏭ NEXT EPISODE ]` interactive pills appear & respond to both mouse click and keyboard (`Tab` / `N`).
   - Verify `[S]` Sub Sync bar & `[ ⚡ SNAP TO VOICE (y) ]` immediately snap slow subtitles to dialogue and persist for Episode 15.
