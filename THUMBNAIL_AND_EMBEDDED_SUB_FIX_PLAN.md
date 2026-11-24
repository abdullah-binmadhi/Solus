# Solus — Implementation Plan: Persistent Episode Thumbnails & Embedded/Hardsub Dual-Subtitle Elimination

**Status:** Proposal / Pre-Implementation Investigation Plan
**Target Branch:** `dev`
**Scope:**
1. **Fix Episode Thumbnails Disappearing After Closing Playback** (proven Kitty `StatefulKitty` one-shot `TransmitAndPlace` wipe by `PlaybackOptions` `Clear` widget + `terminal.clear()` across screen transitions).
2. **Eliminate Double Subtitles on Embedded / Hardsubbed Providers** when selecting Arabic or other non-English languages, plus **visual provider badges (`Clean` vs `Hardsub EN` vs `Muxed Sub`)** and live **Hardsub Shield (`Opaque Box` / `Top` / `Off`)**.

---

## 1. Root Cause Investigation & Visual Diagnostics

### Issue A: Why the Episode Thumbnail Disappears After Playing & Closing the Player

Observed in [Screenshot 1](/Users/abdullahbinmadhi/Desktop/Screenshot%202026-11-24%20at%203.51.36%E2%80%AFpm.png) (`DETAIL Chainsmoker Cat — E1 08:24 / 23:42`):
- The left anime poster is rendered cleanly, while the `Episode 1 Inspector` thumbnail slot (`thumb_area`) is **completely black empty cells** — notably **without** the purple fallback text (`"EPISODE 1 HIGH-DEF PREVIEW"`). That proves [`render_episode_thumbnail_slot`](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/detail.rs#L633-L701) returned `true` and ran `hd_image_widget()`, yet Kitty displayed nothing.

Three interacting bugs in our Ratatui + `ratatui-image` (`StatefulKitty`) lifecycle cause this:

1. **One-Shot `TransmitAndPlace` Swallowed by `PlaybackOptions` `Clear` Overlay:**
   - Inside `ratatui-image` (`kitty.rs`), [`StatefulKitty`](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/components/cover.rs#L316) starts in `KittyProtoState::TransmitAndPlace(png_escape_seq)`.
   - On the **first** render for an image ID/geometry, `StatefulKitty::render()` consumes the transmit escape payload (`_Gq=2,i=...,a=T,U=1...`) and permanently flips itself to `KittyProtoState::Place`, emitting only bare Unicode placeholder cells (`U+10EEEE`) on every future draw.
   - When `Screen::PlaybackOptions` is active ([main.rs:2196-2199](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L2196-L2199)), `ui::detail::render` draws first (consuming `TransmitAndPlace` for `thumb_area` or any newly loaded/resized thumbnail) and `ui::play_options::render_overlay` immediately executes `frame.render_widget(Clear, layout.modal_rect)` right on top of `thumb_area` **inside the same `terminal.draw()` closure** — wiping the transmit escape before stdout ever flushes to Kitty! (Whereas `cols[0]` top cover extends above `modal_rect` rows `1..5`, or was already transmitted earlier.)

2. **`terminal.clear()` (`ESC[2J`) on Every Screen Transition Without Kitty Re-Transmission:**
   - [`main.rs:2165-2169`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L2165-L2169) executes `terminal.clear()` whenever `state.screen != last_screen` (`Detail -> PlaybackOptions -> Detail` or `Playback -> Detail`).
   - Returning to `Detail` leaves `episode_thumbnails[ep]` and `episode_fallback_thumbnails[ep]` stuck in `KittyProtoState::Place` — so if the image ID was wiped, evicted from Kitty's graphics RAM, or never flushed cleanly due to the `Clear` overlap, it stays black forever.

3. **`thumbnail_geometry_key` Misses Initial Geometry Mismatch & Fallback Cover Source:**
   - In [`render_episode_thumbnail_slot`](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/detail.rs#L641-L667), `if had_prev_geo` skips rebuilding `state.episode_thumbnails` when switching into a layout for the first time (`None -> Some(geo_key)`), and fallback rendering only checks `state.current_card_img` without falling back to `state.cover_raw_cache` when `current_card_img` is `None`.

---

### Issue B: Why Non-English Subtitles Show Double Text (Arabic + Embedded English) & Lack Provider Indicators

Two distinct causes create overlapping subtitles when selecting Arabic (or French/Spanish/etc.) while English appears fine:

1. **Burned-In Hardsub Providers (`Zoko /hsub`, `MegaPlay (Hardsub)`, `Vidmoly Sub`, `AnimeHeaven`, `AnimeGG`) Overlap Bottom Arabic Text:**
   - Episodes like *Chainsmoker Cat Ep 1* only exist on `/hsub` (`Zoko /hsub`, `MegaPlay /hsub`) or burned-in sites (`Vidmoly`, `AnimeHeaven`, `AnimeGG`).
   - These streams bake English subtitle pixels directly into the bottom `85%..97%` strip of the video frame.
   - [`SubtitleStyleConfig::to_mpv_flags`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/substyler.rs#L176-L196) positions external Arabic subtitles at default `sub-pos=100` / `sub-margin-y=38` with transparent background (`Custom` preset `backdrop_opacity=0.0`), drawing Arabic directly **on top of** the burned-in English words -> unreadable double subtitles.
   - Meanwhile `PlaybackOptions` (`Col 1: Provider`) currently only shows `[✓ 1080p]` with **zero indicator** of whether that provider is `Clean` (raw video) vs `Hardsub EN` (baked English) vs `Muxed Sub`!

2. **Container / HLS Soft-Embedded Tracks Can Co-Exist Without Strict Isolation:**
   - [`shared_mpv_kv_flags`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs#L1368-L1377) currently emits `sub-auto=fuzzy` and omits **`secondary-sid=no`**, leaving room for embedded MP4/MKV/HLS tracks to remain selected or auto-loaded alongside `--sub-file`.

---

## 2. Architecture & UI Design Sketches

### 2.1 Persistent Thumbnail Lifecycle (`Clear`-Proof + Always Re-Transmittable)

```
Screen::Detail (no modal covering viewport)
   |
   +--> Lazy protocol build if missing:
   |      episode_thumbnails[ep]          <-- from episode_raw_thumbnails[ep] (zero network!)
   |      episode_fallback_thumbnails[ep] <-- from current_card_img OR cover_raw_cache[anime_id]
   |
   +--> On modal open (PlaybackOptions / Search / Settings / Help / Download / BatchQueue):
   |      ui::detail::render(..., suppress_kitty_underlay = true)
   |      -> NEVER consumes `TransmitAndPlace` beneath a `Clear` modal!
   |
   +--> On Playback launch -> close mpv -> return to Screen::Detail (PlaybackDone / go_back / Modal close):
          state.invalidate_detail_image_protocols():
            - clears `episode_thumbnails`        (keeps `episode_raw_thumbnails` in RAM!)
            - clears `episode_fallback_thumbnails`
            - clears `fitted_cover_cache`        (keeps `cover_raw_cache` in RAM!)
            - triggers immediate fresh `TransmitAndPlace` on next Detail frame!
```

### 2.2 Provider Indicators & Hardsub Shield UI Sketch (`PlaybackOptions` Modal)

```
+---------------------------------------------------------------------------------------------+
|  > Chainsmoker Cat - Episode 1 (Sub)     [Player: Auto-Best]   [Audio: Sub]   [Shaders:...]  |
+-------------------+-------------------------+--------------------+--------------------------+
| Player            | Provider                | Quality            | Subtitles (All Pooled)   |
|  [●] Auto Best    |  [○] Vidmoly [✓900p·EN] |  [●] 900p          |  [ ] None (Off)          |
|  [○] mpv          |  [●] MegaPlay[✓1080·CLN]|  [○] 480p          |  [ ] English             |
|  [○] mpv.net      |  [○] Zoko    [✓1080·EN] |                    |  [●] Arabic (العربية)    |
|  [○] Celluloid    |  [○] MP Hsub [✓1080·EN] |                    |  ----------------------- |
|                   |  [○] AHeaven [✓1080·EN] |                    |  ⚠ Provider has Baked EN |
|                   |                         |                    |  [h] Shield: Opaque Mask |
+-------------------+-------------------------+--------------------+--------------------------+
  CLN = Clean Raw Video (Zero burned sub) | EN = Baked EN Hardsub | MUX = Soft Muxed Track
```

*(Note: When a user with non-English subtitles like **Arabic** opens an episode that HAS a `Clean` provider like `MegaPlay Direct`, Solus auto-ranks/selects the `Clean` provider first. When only `Hardsub EN` providers exist — e.g. Chainsmoker Cat Ep 1 — the amber `[·Hardsub EN]` indicator and **Hardsub Shield** automatically protect the Arabic subtitle!)*

### 2.3 `libmpv` Double-Subtitle Prevention Pipeline

```
Selected Provider + Subtitle Language
   |
   +-- Soft Isolation (ALWAYS applied):
   |     `secondary-sid=no`, `sub-auto=no`
   |     Only the chosen `--sub-file` (or explicit `sid=no` if None) is active!
   |
   +-- If `provider.embedded_sub == HardsubEn` AND `sub_lang` is NOT English and NOT None:
         |
         +--> Shield Mode 1 (Default): `Opaque Mask`
         |      `--sub-border-style=opaque-box`
         |      `--sub-back-color=#EE0A0A0E`  (93% dark cinema pill covers baked EN pixels!)
         |      `--sub-margin-y=24`, `--sub-pos=98`, `--sub-ass-override=force`
         |
         +--> Shield Mode 2 (Hotkey `[h]` inside mpv or `[h]` in Options): `Top Edge`
         |      `--sub-pos=8`, `--sub-border-style=outline-and-shadow` (cleanly above top bars!)
         |
         +--> Shield Mode 3: `Off` (user's raw custom style)
```

---

## 3. Exact File-by-File Implementation Plan

### File 1: `src/state/mod.rs`
1. **Add `HardsubShieldMode` Enum & State (`Mask` [default] / `TopEdge` / `Off`) + `invalidate_detail_image_protocols(&mut self)`:**
   - `AppState::invalidate_detail_image_protocols(&mut self)` clears ONLY the encoded `StatefulProtocol` caches (`fitted_cover_cache`, `detail_cache`, `episode_thumbnails`, `episode_fallback_thumbnails`), preserving `cover_raw_cache`, `episode_raw_thumbnails`, and `current_card_img` in RAM so Detail thumbnails & covers re-transmit to Kitty in `< 2ms` without re-downloading.
   - Add `pub hardsub_shield_mode: HardsubShieldMode` (default `OpaqueMask`) toggled via `[h]` in `PlaybackOptions` or inside `mpv`.

### File 2: `src/ui/detail.rs`
1. **Guard `render_episode_thumbnail_slot` & `render_info` Against Modal Overlays (`suppress_kitty_underlay`):**
   - Pass `render_detail_screen(frame, state, suppress_images: bool)` (used as `false` in `Screen::Detail` when no modal is open, `true` under `PlaybackOptions`, `PlaybackQuery`, `Search`, `Help`, `Settings`, `DownloadModal`, `BatchQueueModal`, etc.) so `Clear` modals **never** overwrite and silently destroy one-shot `StatefulKitty` `TransmitAndPlace` payloads!
   - Wait — even simpler for `Detail` + `PlaybackOptions`: skip calling `StatefulKitty::render()` **only inside rects overlapped by an active modal** (so top-left anime cover `cols[0]` can stay visible if outside `modal_rect`, OR safely rebuilds via `invalidate_detail_image_protocols` when modal closes!).
2. **Fix First-Time `geo_key` Mismatch & Fallback Cover Source:**
   - Remove the `if had_prev_geo` restriction in [`render_episode_thumbnail_slot`](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/detail.rs#L644) so whenever `state.thumbnail_geometry_key != Some(geo_key)` OR `!state.episode_thumbnails.contains_key(&ep)` while `state.episode_raw_thumbnails.contains_key(&ep)`, `episode_thumbnails` immediately constructs/scales a fresh protocol at the exact `thumb_area` cell size.
   - For `episode_fallback_thumbnails`: check `state.current_card_img.as_ref().or_else(|| state.cover_raw_cache.get(&anime_id))` so episodes without per-episode CDN stills always display a crisp cover-derived preview.

### File 3: `src/main.rs`
1. **Trigger `invalidate_detail_image_protocols` on Returns to `Screen::Detail`:**
   - Whenever `last_screen != state.screen && state.screen == Screen::Detail` (including closing `PlaybackOptions`, closing `PlaybackQuery`, returning from `Playback`, or cancelling modals) AND inside [`PlaybackDone`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L1044) & [`start_playback`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L10032), call `state.invalidate_detail_image_protocols()` so every Kitty placement re-transmits freshly after `terminal.clear()` or `mpv` window closure.
2. **Propagate `embedded_sub` & `hardsub_shield` into `PlayOptions` and Clean-First Provider Ordering:**
   - When the user's preferred subtitle language is neither English nor `"none"` (e.g., `Arabic`), sort `state.playback_providers` / pick initial provider cursor preferring `EmbeddedSubtitleKind::None` (`Clean`) first.
   - Populate `PlayOptions.embedded_sub` and `PlayOptions.hardsub_shield` in `LaunchNow`, `StreamSwitchReady`, and instant launch paths (`handle_playback_options` / `start_playback`).
   - Handle `[h]` hotkey inside `Screen::PlaybackOptions` (`Col 3` Subtitle Shield cycle: `Opaque Mask` -> `Top Edge` -> `Off`).

### File 4: `src/api/subtitles.rs`
1. **Track `EmbeddedSubtitleKind` (`None` / `HardsubEn` / `SoftMuxed`) on `ProviderOption`:**
   - Add `#[serde(default)] pub embedded_sub: EmbeddedSubtitleKind` with concise `badge_label()` helper (`"Clean"`, `"Hardsub EN"`, `"Muxed Sub"`).
   - Detect `#EXT-X-MEDIA:TYPE=SUBTITLES` in [`parse_m3u8_ladder`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs) to spot soft-muxed HLS streams (`SoftMuxed`).
   - Tag every provider accurately:
     - `zoko` `/sub` -> `None`, `/hsub` fallback -> `HardsubEn` (`"ZokoAnime (Hardsub)"`)
     - `megaplay_mal` `/sub` -> `None` (unless HLS has `#EXT-X-MEDIA:TYPE=SUBTITLES`), `/hsub` -> `HardsubEn`
     - `vidmoly` -> `!is_dub` => `HardsubEn`, `is_dub` => `None`
     - `animeheaven` & `animegg` -> `!is_dub` => `HardsubEn`, `is_dub` => `None`
     - `hd1` / `hd2` / `hianime` -> `HardsubEn` when `data-type="hsub"`, else `None` / `SoftMuxed`.

### File 5: `src/ui/play_options.rs`
1. **Render Provider Embedded-Subtitle Indicators in `Col 1 (Provider)`:**
   - Display `[✓ 1080p · Clean]` (green/crisp), `[✓ 900p · Hardsub EN]` (amber warning tint), or `[✓ 1080p · Muxed Sub]` (cyan) cleanly within `p_budget`.
2. **Render Hardsub Warning & Shield Toggle in `Col 3 (Subtitles)`:**
   - When the selected provider has `HardsubEn` AND the highlighted subtitle language is non-English & non-Off (e.g. `Arabic`), reserve 2 bottom footer rows inside `layout.subtitle_rect` showing:
     - Row 1: `⚠ Baked EN Sub in stream` (amber)
     - Row 2: `[h] Shield: Opaque Mask` / `Top Edge` / `Off`

### File 6: `src/api/player.rs`
1. **Strict Soft-Track Isolation + Hardsub Shield Flags + Live `[h]` Hotkey in `SOLUS_AUDIO_LUA`:**
   - In [`shared_mpv_kv_flags`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs#L1368-L1377):
     - Always emit `secondary-sid=no` and `sub-auto=no` (prevents duplicate container/auto-loaded subtitles).
     - Apply `PlayOptions.hardsub_shield` **after** `sub_style.to_mpv_flags()` when active (`HardsubEn` + non-English/non-None subtitle):
       - `OpaqueMask`: `sub-border-style=opaque-box`, `sub-back-color=0.04/0.04/0.06/0.94`, `sub-margin-y=24`, `sub-pos=98`, `sub-ass-override=force`.
       - `TopEdge`: `sub-border-style=outline-and-shadow`, `sub-pos=8`, `sub-margin-y=22`, `sub-ass-override=force`.
   - In `SOLUS_AUDIO_LUA`: bind `h` / `H` to `solus_cycle_hardsub_shield()` so users can toggle **Mask ↔ Top ↔ Off** live during playback with instant OSD confirmation.

---

## 4. Verification Plan

1. **Automated Unit Tests (`cargo test`)**
   - `test_invalidate_detail_image_protocols_preserves_raw_caches_and_forces_retransmit`: verifies raw caches (`episode_raw_thumbnails`, `cover_raw_cache`) stay intact while protocol caches clear for fresh Kitty `TransmitAndPlace`.
   - `test_render_episode_thumbnail_slot_rebuilds_from_raw_and_updates_initial_geometry`: tests initial `None -> Some(geo_key)` rebuild + lazy rebuild after clear.
   - `test_provider_embedded_sub_classification_and_arabic_clean_ranking`: verifies `/hsub` -> `HardsubEn`, `/sub` -> `Clean`, and Clean-first ordering for non-English subtitles.
   - `test_mpv_flags_emit_secondary_sid_no_and_hardsub_shield_opaque_box`: verifies exact `secondary-sid=no`, `sub-auto=no`, `sub-border-style=opaque-box`, `sub-back-color=0.04/0.04/0.06/0.94` when Hardsub Shield is enabled.

2. **Interactive Smoke Verification**
   - Open *Chainsmoker Cat* Episode 1 in Kitty, verify `Ep 1` thumbnail renders -> press Enter -> open `PlaybackOptions` -> observe `[✓ 900p · Hardsub EN]` / `[✓ 1080p · Hardsub EN]` indicators & `Shield: Opaque Mask` warning -> launch `mpv` -> close `mpv` -> confirm **Episode 1 thumbnail remains 100% visible**.
   - Select Arabic on a `HardsubEn` stream and verify clean opaque mask / top-edge `[h]` switching with zero unreadable overlapping text.
