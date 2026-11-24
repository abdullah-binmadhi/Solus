# Solus Reliability Overhaul: Config Compatibility, Multi-Provider Engine, All Players & Next-Episode Freeze Fix

> **Status**: Investigated & Verified Live  
> **Scope**: Four critical fixes across config persistence, direct streaming providers, universal player selection, and non-blocking Next-Episode switching.

---

## 1. Root Cause Investigation Summary

### Bug 1: `config.toml` Parse Crash (`unknown variant 'auto-best', expected one of 'mpv', 'iina', 'vlc'`)
- **Root Cause**:
  - The `dev` branch changed `Player` serialization to write `player = "auto-best"` (as well as `performance_mode = "reference4_k"`) into the shared `~/Library/Application Support/solus/config.toml`.
  - Meanwhile, installed binaries on `PATH` (`/opt/homebrew/bin/solus` and `~/.cargo/bin/solus`) still run the older `main` `enum Player`, which uses strict `#[serde(rename_all = "lowercase")]` and only accepts `"mpv"`, `"iina"`, or `"vlc"`.
  - Running `solus` from `PATH` crashes immediately on line 3 of `config.toml`.
- **Solution — Dual-Field Backward- & Forward-Compatible Schema (`src/config.rs`)**:
  - Always serialize `player` strictly as a legacy-safe string (`"mpv"`, `"iina"`, or `"vlc"` via `Player::legacy_config_str()`).
  - Persist the full 6-variant selection (`"auto-best"`, `"mpv"`, `"iina"`, `"vlc"`, `"mpv.net"`, `"celluloid"`) in an additive field `player_engine: Option<Player>` (which older binaries ignore harmlessly).
  - Add lossy/fallback deserializers across `Config` enums (`player`, `player_engine`, `quality`, `audio_mode`, `network_mode`, `performance_mode`, `deband_mode`, `motion_mode`) so unknown or future TOML values **never** crash `Config::load()`.
  - On `Config::load()` with `config_version < 3`, self-heal existing `config.toml` (`config_version = 3`, `auto_skip_intro = false` one-time migration) so `player` is rewritten cleanly to a legacy-safe value alongside `player_engine`.

---

### Bug 2: Only 1 Provider Appears (`ZokoAnime` Only)
- **Root Cause (Empirically Verified Live)**:
  1. **MegaPlay HD-1 Silent Failure**: `hianime.at` exposes `HD-1` (`https://megaplay.buzz/stream/s-2/<ep_id>/<sub|dub>?s=tcdn`), but `fetch_all_hianime_providers` routed it through `parse_stream_embed()`, which only decodes ZokoAnime's `window.__P="..."` blob. In reality, MegaPlay serves `data-id` + `/stream/getSources?id=<data-id>` with AES-256-CBC encrypted JSON (`enc`) containing the live `1080p` HLS `master.m3u8` (`fetch.nexabloom.top`), 10+ `.vtt` subtitle tracks, and native `intro`/`outro` timestamps — so `HD-1` failed on every episode and was filtered out.
  2. **AllAnime CAPTCHA Gate**: `api.allanime.day/api` now returns `{"errors":[{"message":"NEED_CAPTCHA"}]}` for episode queries, causing `probe_allanime_provider` to return `None` every time. We will remove the CAPTCHA-gated AllAnime probe cleanly rather than attempting any CAPTCHA bypass.
  3. **Hardcoded ZokoAnime `Referer`**: `shared_mpv_kv_flags` (`src/api/player.rs`) hardcoded `referrer=https://zokoanime.video/` for every stream, causing HTTP 403 playback failures for streams hosted on other CDNs (`megaplay.buzz`, `animegg.org`, etc.).
- **Solution — 3 Independent, Live-Verified Direct Providers + Per-Provider `Referer`**:
  1. **`ZokoAnime`** (`https://zokoanime.video/stream/mal/<mal_id>/<ep>/<sub|dub>`): Direct multi-res HLS master + multi-language `.vtt` tracks + Sub & Dub (`Referer: https://zokoanime.video/`).
  2. **`MegaPlay HD-1`** (`https://hianime.at` search -> strict episode match -> `HD-1` server -> `megaplay.buzz/stream/getSources?id=<data-id>` -> pure-Rust AES-256-CBC decrypt via `aes` + `cbc`): Direct `1080p` HLS master (`#EXTM3U`) + 10+ `.vtt` tracks + native `intro`/`outro` bounds + Sub & Dub (`Referer: https://megaplay.buzz/`).
  3. **`AnimeGG Direct`** (`https://www.animegg.org/search/?q=...` -> `/series/<slug>` -> `/<slug>-episode-<ep>` -> `/embed/<id>`): Direct multi-resolution MP4 (`1080p`, `720p`, `480p`, `360p` via `vidcache.net` 302 redirect) + Sub & Dub (`Referer: https://www.animegg.org/`).
  - Remove `probe_allanime_provider` completely.
  - Add `referer: Option<String>` to `ProviderOption` and `PlayOptions`, wired into `shared_mpv_kv_flags`, `build_hot_switch_commands`, and `build_vlc_direct_args` (`--http-referrer`).
  - Probe all 3 providers concurrently with bounded per-provider timeouts (`<= 5s` total) and strict episode matching so only verified working providers for that exact episode & Sub/Dub mode appear with `[✓ 1080p]` badges.

---

### Bug 3: Only OS-Optimal Players Shown (Missing IINA, VLC, etc.)
- **Root Cause**:
  - Commit `c4ac943` removed `Player::Iina` and `Player::Vlc` from `enum Player`.
  - `AppState::host_players()` (`src/state/mod.rs:1612`) and `ui::setup::supported_players()` (`src/ui/setup.rs:37`) hide players not considered optimal for the host OS (so macOS only showed `Auto-Best` and `mpv`, even with `/Applications/IINA.app` and `/Applications/VLC.app` installed).
- **Solution — Full 6-Player Roster Always Visible Everywhere**:
  - Restore all 6 variants in `Player::ALL`:
    1. `AutoBest` — **Auto-Best** (automatically selects best installed `libmpv` engine)
    2. `Mpv` — **mpv** (Universal `libmpv` reference — macOS / Windows / Linux)
    3. `Iina` — **IINA** (macOS native `libmpv` GUI — passes `--no-stdin --keep-running --mpv-<kv>`, preserving Lua HUD, Skip/Next buttons, Sub Sync, GLSL shaders, and JSON IPC)
    4. `Vlc` — **VLC** (Universal cross-platform GUI — `--http-referrer`, `--sub-file`, `--sub-delay`, `--start-time`, `--fullscreen`, `--play-and-exit`)
    5. `MpvNet` — **mpv.net** (Windows native `libmpv` GUI)
    6. `Celluloid` — **Celluloid** (Linux GTK `libmpv` GUI)
  - Update `AppState::host_players()` and `ui::setup::supported_players()` to ALWAYS return all 6 `Player::ALL` entries regardless of OS.
  - Restore `has_iina` and `has_vlc` detection in `DependencyStatus` and render clear availability tags (e.g. bright when installed, dimmed `(Not Installed)` / `(Win)` / `(Linux)` / `(macOS)` otherwise).

---

### Bug 4: Pressing `N` / Clicking `NEXT EPISODE` Closes Episode & Freezes Solus Completely
- **Root Cause (Reproduced & Verified Live with `mpv`)**:
  1. **Infinite `script-message` Recursion Storm (`src/api/player.rs:975 & 1127`)**:
     - `mp.add_forced_key_binding("N", "solus-next-episode", do_next_episode)` registers a `script-message` handler named `"solus-next-episode"`.
     - Inside `do_next_episode()`, calling `mp.commandv("script-message", "solus-next-episode")` invoked `do_next_episode()` recursively forever!
     - Live `mpv` test confirmed 1 keypress produces infinite `{"event":"client-message","args":["solus-next-episode"]}` events.
  2. **256-Slot Channel Flood + Synchronous Blocking on UI Event Loop (`src/main.rs:570-613 & 9412-9594`)**:
     - `src/services/ipc.rs:809` pushed every recursive event into the main channel `tx`.
     - `src/main.rs` executed `launch_remote_stream_switch(...).await` synchronously inside the main TUI event loop for every queued message with no debounce — locking up rendering and keyboard input completely while killing each new player immediately.
  3. **Secondary Switch Gaps**:
     - Lowercase `n` was unbound (only uppercase `N` was bound).
     - `launch_remote_stream_switch` did not read cached `state.anime_mal_ids`, had no timeout around `mal_fut`, did not bump `state.playback_gen` prior to shutdown, and did not preserve `prev_provider_id` / `referer`.
- **Solution — One-Shot Distinct IPC Signal + Non-Blocking Background Switch**:
  1. **Lua (`src/api/player.rs`)**:
     - Rename key binding to `"solus-next-ep-key"` and bind **both** `"n"` and `"N"`.
     - Emit a distinct IPC message `mp.commandv("script-message", "solus-ipc-next-ep")` that does **not** collide with any key-binding handler name.
     - Guard `do_next_episode()` with `local next_ep_dispatched = false` so it fires **at most once** per player instance.
  2. **IPC (`src/services/ipc.rs`)**:
     - Match `client-message` containing `"solus-ipc-next-ep"` and guard with `let mut next_ep_sent = false;` in `drive_ipc_stream` to emit **at most one** `AppMessage::NextEpisodeRequested`.
  3. **Non-Blocking Main Loop (`src/main.rs`)**:
     - Add `AppMessage::NextEpisodeRequested` and `AppMessage::StreamSwitchReady`.
     - On `NextEpisodeRequested`: debounce (`is_switching_episode` + 1.5s cooldown), bump `state.playback_gen`, quit the current player immediately, update `state.selected_episode = Some(next_ep)`, show toast `"⏭ Loading Episode N..."`, and **`tokio::spawn`** the stream resolution (8s hard timeout) in the background so the TUI never freezes.
     - On `StreamSwitchReady`: match `switch_gen`, re-select previous provider + `referer` (or best verified fallback), select preferred subtitle with English fallback (`select_index_with_english_fallback`), and call `start_playback`.

---

## 2. Architecture & UI Sketches

### A. Non-Blocking Next-Episode Flow
```
  [mpv / IINA Player]
     │  Press [n]/[N] or Click [NEXT EPISODE]
     ▼
  SOLUS_AUDIO_LUA :: do_next_episode()
     ├─ guard: next_ep_dispatched == true -> return
     ├─ set next_ep_dispatched = true
     └─ mp.commandv("script-message", "solus-ipc-next-ep")   <-- distinct name, ZERO recursion!
           │
           ▼  (Unix Socket / Named Pipe JSON-RPC)
  services/ipc.rs :: drive_ipc_stream()
     ├─ match client-message + "solus-ipc-next-ep"
     ├─ guard: next_ep_sent == true -> ignore
     └─ send AppMessage::NextEpisodeRequested (once)
           │
           ▼  (Non-Blocking UI Event Loop in src/main.rs)
  NextEpisodeRequested Handler
     ├─ Debounce: ignore if is_switching_episode or <1.5s since last switch
     ├─ Bump state.playback_gen + state.switch_generation
     ├─ Quit current player + stop monitor (stale PlaybackDone safely dropped)
     ├─ Set state.selected_episode = Some(next_ep), toast "⏭ Loading Ep N..."
     └─ tokio::spawn (8s timeout):
           fetch_stream_info_with_network(anime_id, mal_id, query, next_ep, is_dub, ...)
           ──▶ AppMessage::StreamSwitchReady { switch_gen, prev_provider_id, stream_info, ... }
                 │
                 ▼
           StreamSwitchReady Handler
             ├─ Preserve Player, Quality, Audio Mode, Sub Delay, Provider & Referer
             ├─ Subtitle selection: preferred -> English fallback -> first track
             └─ start_playback(...)  <-- launches Ep N+1 cleanly!
```

### B. Multi-Provider Parallel Verification Pipeline
```
  open_playback_options / episode_switch
     │
     ├──▶ [1] ZokoAnime       (MAL -> zokoanime.video)         ─▶ HLS master + VTT  (Referer: zokoanime.video)
     ├──▶ [2] MegaPlay HD-1   (hianime.at -> AES-256-CBC enc)  ─▶ 1080p HLS + intro (Referer: megaplay.buzz)
     ├──▶ [3] AnimeGG Direct  (animegg.org -> /embed -> /play) ─▶ 1080p..360p MP4   (Referer: animegg.org)
     └──▶ [4] External Subs   (AniZip + OpenSubtitles REST)    ─▶ Community VTT/SRT
                                                                         │
                                                                         ▼
                                                       Strict Episode + Mode Filter
                                                       Only verified sources shown!
```

### C. Updated Playback Configuration Modal (`ui/play_options.rs`)
```
╭─  Playback Configuration ──────────────────────────────────────────────────────────────╮
│  Episode 1 · Sub  · Audio [a]: JP Sub  · Delay: +0.00s                                   │
│  Anime4K: ULTRA 4K · Perf: REF 4K · Deband: 2x · Network: NET:FULL                       │
├─ 󰐊 Player (All 6) ───┬─ 󰢹 Provider (Verified) ──┬─ 󰗃 Quality ──────┬─ 󰨖 Subtitles ─────┤
│ [●] Auto-Best        │ [●] ZokoAnime [✓ 1080p]  │ [●] 1080p        │ [○] No subtitles  │
│ [○] mpv (Universal)  │ [○] MegaPlay HD1[✓ 1080p]│ [○] 720p         │ [●] English       │
│ [○] IINA (macOS)     │ [○] AnimeGG   [✓ 1080p]  │ [○] 480p         │ [○] Arabic        │
│ [○] VLC (Universal)  │                          │ [○] 360p         │ [○] Spanish       │
│ [○] mpv.net (Win)    │                          │                  │ [○] French        │
│ [○] Celluloid (Linux)│                          │                  │                   │
╰──────────────────────┴──────────────────────────┴──────────────────┴───────────────────╯
```

### D. Backward/Forward Compatible `config.toml` Layout
```toml
quality = "best"
audio_mode = "sub"
player = "mpv"              # Always legacy-safe ("mpv" | "iina" | "vlc") for older binaries!
player_engine = "auto-best" # Full 6-variant engine preserved for current Solus!
```

---

## 3. Exact File-by-File Implementation Plan

1. **[Cargo.toml](file:///Users/abdullahbinmadhi/Projects/anime/Cargo.toml)**
   - Add `aes = "0.8"` and `cbc = { version = "0.1", features = ["std"] }` for pure-Rust MegaPlay AES-256-CBC decryption.

2. **[src/config.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/config.rs)**
   - Restore `Player::Iina` and `Player::Vlc` in `Player::ALL` (6 variants total: `AutoBest`, `Mpv`, `Iina`, `Vlc`, `MpvNet`, `Celluloid`).
   - Add `Player::legacy_config_str(&self) -> &'static str` returning `"mpv" | "iina" | "vlc"`.
   - Add `#[serde(default)] pub player_engine: Option<Player>` on `Config`.
   - Serialize `Config::player` as `legacy_config_str()` and `player_engine` as `Some(self.player)`.
   - Deserialize `Config` preferring `player_engine` if present, falling back to `player` (lossy, never fails), and add lossy deserializers for `Quality`, `AudioMode`, `NetworkMode`, `PerformanceMode`, `DebandMode`, `MotionMode`.
   - Bump `config_version = 3` migration (ensuring `auto_skip_intro = false` migrates cleanly once and `player` heals on save).

3. **[src/api/subtitles.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs)**
   - Add `#[serde(default)] pub referer: Option<String>` to `ProviderOption`.
   - Add `SubtitleOption::select_index_with_english_fallback(subs: &[SubtitleOption], preferred: &str) -> (usize, bool)`.
   - Replace broken `HD-1` Zoko-blob parsing with `fetch_megaplay_provider` (AES-256-CBC decrypt `enc` JSON -> HLS master + `.vtt` tracks + intro/outro).
   - Add `fetch_animegg_provider` (search -> `/series/...` -> `/...-episode-<ep>` -> `/embed/...` -> multi-resolution MP4 sources).
   - Remove CAPTCHA-gated `probe_allanime_provider`.
   - Wrap `mal_fut` in a timeout alongside `fetch_all_hianime_providers`, `fetch_animegg_provider`, and `fetch_external_subtitles`.

4. **[src/api/player.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs)**
   - Restore `has_iina` and `has_vlc` in `DependencyStatus` and `detect_dependencies()` (`iina`, `/Applications/IINA.app/Contents/MacOS/iina-cli`, `vlc`, `/Applications/VLC.app/Contents/MacOS/VLC`, Windows VLC paths).
   - Add `#[serde(default)] pub referer: Option<String>` to `PlayOptions` and wire it into `shared_mpv_kv_flags` instead of hardcoding `https://zokoanime.video/`.
   - Add `build_iina_direct_args` (`--no-stdin --keep-running --mpv-<kv>`) and `build_vlc_direct_args` (`--http-referrer`, `--sub-file`, `--sub-delay`, `--start-time`, `--fullscreen`, `--play-and-exit`) and route them in `spawn_async` / `ensure_player_wrapper`.
   - Fix `SOLUS_AUDIO_LUA` & `YOUTUBE_INPUT_CONF`: bind `n` and `N` to `"solus-next-ep-key"`, call `mp.commandv("script-message", "solus-ipc-next-ep")` (distinct name), and enforce one-shot `next_ep_dispatched = false`.

5. **[src/services/ipc.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/services/ipc.rs)**
   - Detect `line.contains("client-message") && line.contains("solus-ipc-next-ep")` with one-shot `next_ep_sent` guard in `drive_ipc_stream` -> emit `AppMessage::NextEpisodeRequested`.
   - Add `pub referer: Option<String>` to `HotSwitchRequest` and replace the hardcoded ZokoAnime referer in `build_hot_switch_commands`.

6. **[src/state/mod.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/state/mod.rs)**
   - Add `has_iina: bool`, `has_vlc: bool`, `is_switching_episode: bool`, `switch_generation: u64`, `last_next_ep_at: Option<std::time::Instant>`.
   - Update `AppState::host_players()` to return all 6 players (`crate::config::Player::ALL.to_vec()`).
   - Update `has_any_player()` (`has_mpv || has_iina || has_vlc || has_mpvnet || has_celluloid`).

7. **[src/ui/play_options.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/play_options.rs), [src/ui/setup.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/setup.rs), [src/ui/settings.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/settings.rs)**
   - Show all 6 players in Playback Options Col 0 and System Doctor (`!`), with clean installed vs missing/OS labels.
   - Show verified multi-provider results cleanly in Col 1.

8. **[src/main.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs)**
   - Add `AppMessage::NextEpisodeRequested` and `AppMessage::StreamSwitchReady`.
   - Refactor `launch_remote_stream_switch` into non-blocking `spawn_episode_stream_switch` (runs provider fetch in `tokio::spawn` with 8s timeout; passes `state.anime_mal_ids`; preserves `prev_provider_id`, `referer`, player, quality, audio, sub delay).
   - Handle `StreamSwitchReady` with `switch_gen` check and `SubtitleOption::select_index_with_english_fallback`.
   - Wire `Player::Iina` / `Player::Vlc` checks in `handle_playback_options` and `DependenciesReady`.

9. **[src/services/queue.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/services/queue.rs)**
   - Use `SubtitleOption::select_index_with_english_fallback` and propagate `referer` in `resolve_item_prefetch`.

---

## 4. Verification Checklist

1. **Automated Unit Tests (`cargo test --quiet`)**:
   - Config backward/forward round-trip test for all 6 `Player` variants (`player` always `"mpv"|"iina"|"vlc"`, `player_engine` restores exact variant) + unknown enum resilience.
   - MegaPlay AES-256-CBC decryption & JSON track/intro extraction test.
   - AnimeGG `videoSources` MP4 extraction test.
   - `SubtitleOption::select_index_with_english_fallback` test (missing preferred language -> English -> index 1).
   - `mpv` Lua non-recursion verification (key binding `"solus-next-ep-key"` != IPC `"solus-ipc-next-ep"` + one-shot guard).
   - All 6 players returned by `AppState::host_players()` and `ui::setup::supported_players()`.
2. **Release Live Verification (`cargo build --release` & `cargo run --release`)**:
   - Confirm `~/Library/Application Support/solus/config.toml` loads cleanly with both old and new binaries.
   - Confirm 2–3 verified providers (`ZokoAnime`, `MegaPlay HD-1`, `AnimeGG Direct`) appear and stream cleanly.
   - Confirm all 6 players appear in Playback Options and launch cleanly (`mpv`, `IINA`, `VLC`).
   - Confirm pressing `n`/`N` or clicking `NEXT EPISODE` cleanly closes Ep `k`, keeps TUI responsive, and starts Ep `k+1` with preserved settings + English sub fallback.
