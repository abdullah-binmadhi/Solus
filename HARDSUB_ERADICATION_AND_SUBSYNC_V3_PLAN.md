# Solus — Implementation Plan: Complete Hardsub Eradication Toolkit & Reference-Audio Synchronizer v3

**Status:** Proposal / Pre-Implementation Engineering Plan (Awaiting Approval)
**Target Branch:** `dev`
**Triggered By:**
- [Screenshot 1 (Tokyo Ghoul)](/var/folders/h9/zlyf71dx5gj9vb98x_nrv6jc0000gn/T/kitty-dropped-files-081EE733-4285-4428-A8B1-13C5FE47DB12-39034-00000A47092EBA7F/Screenshot%202026-11-24%20at%206.26.38%E2%80%AFpm.png): 2-line baked English subtitle (`"We're going to hit the bookstore / together..."`) sticking out above a 1-line Arabic `--sub-border-style=opaque-box`.
- [Screenshot 2 (Chainsmoker Cat Ep 2)](/var/folders/h9/zlyf71dx5gj9vb98x_nrv6jc0000gn/T/kitty-dropped-files-56A94DB0-1580-47B5-BD41-3B80D30487E6-39034-00000A4847B6E468/Screenshot%202026-11-24%20at%206.30.19%E2%80%AFpm.png): Poisoned `+52950ms` auto-sync delay carried over on `AnimeHeaven Direct`, truncated provider pills (`[✓ 1080p...`), and un-utilized clean timing already present on sibling `Clean` providers (`MegaPlay Direct` / `ZokoAnime`).

---

## PART 1 — Complete Hardsub Eradication Toolkit (`libmpv` + GLSL GPU Shaders)

### Why `--sub-border-style=opaque-box` Alone Failed
`opaque-box` only draws a tight rectangle around the **active external subtitle's own glyph bounding box**. When Arabic is 1 short line and baked English is 2 taller/wider lines (as shown in Screenshot 1), the entire upper English line (`"We're going to hit the bookstore"`) and wide tails bleed outside the box.

### Full Suite of Hardsub Eradication Features (6 Integrated Tools)

| # | Feature / Engine Mode | How It Works Technically (`mpv v0.42` / `libplacebo v7.387`) | Best Use Case |
|---|---|---|---|
| **1** | **GPU `Frosted Blur` Scrim** *(New Default)* | Multi-tap separable `.glsl` GPU fragment blur (`//!HOOK MAIN`) + dark gradient scrim (`y ∈ [band_top..band_bot]`, feathered `smoothstep` borders). Dissolves **ALL 1–3 lines** of baked English across the full horizontal width into a soft Apple-TV-style frosted glass band while Arabic renders crisply on top. | **Default everyday watching** — erases 2-line English completely while preserving ambient anime color motion. |
| **2** | **GPU `Smart Text Inpainter`** | `.glsl` pixel-level subtitle detector inside the ROI (`luma > 0.78 && sat < 0.16 && local_min_luma < 0.14` white core + black halo) that fills **only** detected English text + border pixels using clean surrounding background samples. | Bright outdoor scenes / faces near the bottom where you want to keep background anime art visible with minimal blur band. |
| **3** | **GPU `Total Blackout Matte`** | Full-width pitch-black (`#F4050508`) cinema matte band (`y ∈ [band_top..band_bot]`) drawn at the GPU `MAIN` shader stage underneath `libass`. **Zero** pixels of baked text can ever bleed through regardless of font size, color, or line count. | Stubborn multi-line / stylized hardsubs or bright flashes underneath subtitles. |
| **4** | **`Frame Lift` Letterbox Shift** *(Auto-Aspect Smart)* | Notice 16:10/ultrawide displays (Screenshot 1) already have bottom black letterbox space! `--video-align-y=-0.65` gently shifts video slightly upward + places Arabic cleanly into the pure black bottom bar while the GLSL blur/matte erases the baked English zone. | **16:10 MacBooks / ultrawide monitors** — zero overlap between Arabic and anime artwork! |
| **5** | **`Top Edge` Dual-Zone Mode** | Moves external Arabic subtitles to the upper edge (`--sub-pos=7`, `--sub-margin-y=18`) **paired with** optional bottom `Frosted Blur` / `Blackout Matte` (instead of leaving raw English distracting at the bottom). | Users who prefer top-anchored Arabic while completely hiding bottom English text. |
| **6** | **Live `Band Height` Presets + Calibration Hotkeys** | 3 selectable vertical coverage bands right inside `mpv` & `PlaybackOptions`:<br>• **`Compact (12%)`**: single-line hardsubs (`y: 0.85..0.97`)<br>• **`Standard (19%)`**: standard 2-line hardsubs like *Tokyo Ghoul* (`y: 0.78..0.97`)<br>• **`Tall (25%)`**: wide/3-line/elevated hardsubs (`y: 0.72..0.97`)<br>**Hotkeys inside `mpv`**: `[h]` cycles **Eradication Engine** (`Frosted Blur` → `Smart Inpaint` → `Total Blackout` → `Top Edge` → `Off`); `[H]` (`Shift+H`) cycles **Band Height** (`Compact 12%` → `Standard 19%` → `Tall 25%`) + live dashed calibration outline toast! | Adapts in 1 keypress to any studio/fansub subtitle position. |

### UI Upgrade: Non-Truncated Provider Badges (`src/ui/play_options.rs`)
In Screenshot 2 (`Col 1: Provider`), `[✓ 1080p · Clean]` gets clipped to `[✓ 1080p...`. Switching to **left/prefix compact high-contrast pills**:
```
Provider Column (never truncated):
  [○] [CLN 1080] MegaPlay Direct
  [○] [CLN 1080] ZokoAnime
  [○] [EN  900p] Vidmoly Direct      (amber warning pill)
  [●] [EN  1080] AnimeHeaven Direct  (amber warning pill)
```
Plus 2-line Subtitles Footer when `EN` baked provider + Arabic is selected:
```
  ⚠ Baked EN Sub in stream
  [b] Mode: Frosted Blur   [B] Band: 19%
```

---

## PART 2 — Why SubSync Mis-Locked (`+52950ms`) & Next-Gen Reference Synchronizer v3

### Root Causes of Screenshot 2 (`Delay: +52950ms` & Sync Drift)

1. **Unnormalized Dot-Product Bias (`src/services/subsync.rs:995`):**
   - `correlate_envelope_against_cues` computes `sum(norm_env * mask)` **without dividing by `||mask_centered||_2`**. When shifting `delta = +52.95s` slides more subtitle cues into `[win_start..win_end]`, the template energy jumps and produces a fake `Z >= 2.6` peak purely from cue density!
2. **Single-Window Early Exit (`src/services/subsync.rs:1115`):**
   - Returning immediately on `Windows[0]` (`if res.applied { return res; }`) lets a one-scene BGM coincidence commit `+52.95s` into SQLite permanently across **all episodes** of the anime!
3. **Zero Cross-Provider Reference Audio Sync (Ignoring Clean Sibling Streams!):**
   - Look at Screenshot 2 (*Chainsmoker Cat Ep 2*): `MegaPlay Direct` and `ZokoAnime` are **already resolved right alongside `AnimeHeaven Direct`**, with verified `.vtt` subtitles matching their timeline cleanly (`delay = 0`).
   - Instead of synchronizing **AnimeHeaven audio** vs **sparse Arabic text cues** (low SNR during music/silence), we can cross-correlate **Clean Provider Audio (`MegaPlay`/`Zoko`) directly against Target Provider Audio (`AnimeHeaven`/`Vidmoly`/`AnimeGG`)**!
   - Both streams share identical voices, BGM, OP songs, and SFX — **true normalized Audio-to-Audio NCC routinely hits `Z = 10..25` and locks within `±0.02s` (1 frame)!**
4. **Poisoned SQLite Cross-Episode Carryover:**
   - `save_anime_provider_subtitle_delay_ms` currently stores delays keyed only by `(profile, anime_id, provider_id)` without `(episode)` or sanity/clear recovery, so one false Episode 1 spike (`+52950ms`) corrupts Episode 2, 3, 4 forever!

### Next-Gen 3-Tier Synchronization Pipeline (`SubSync v3`)

```
Need Subtitle Timing for Target Stream (AnimeHeaven / Vidmoly / AnimeGG / OpenSubtitles)
   |
   +--> [TIER A] Sibling Reference Stream Available? (e.g. Clean MegaPlay/Zoko audio_urls[0] or 360p)
   |      1. Download ~55s low-bitrate audio envelope from Ref Stream + Target Stream concurrently (~2.5s)
   |      2. Compute Audio-to-Audio Pearson NCC (normalized by sqrt(sum(ref^2)*sum(tgt^2)))
   |         -> Yields EXACT video timeline delta (`delta_video = t_target - t_ref`, +/-0.02s!)
   |      3. If subtitle is from OpenSubtitles/foreign (not Ref's native sub):
   |         -> Tier-1 Text Align (sub vs Ref .vtt in <50ms) -> `delta_sub_vs_ref`
   |      4. FINAL DELAY = `delta_video + delta_sub_vs_ref` (exact, zero BGM error!)
   |
   +--> [TIER B] Audio-Only Mode (No Sibling Clean Provider Available -> Upgraded Speech-VAD v3):
          1. True Zero-Mean Pearson NCC: divide by `sqrt(sum((m - mean_m)^2))` at every `delta`
             (eliminates `+52.95s` cue-density bias!)
          2. Joint 2-Window Consensus (`Win A` around 200s + `Win B` around 520s):
             - Compute `r_A(delta)` and `r_B(delta)` over `[-90s .. +90s]` (refined `0.02s`)
             - Require BOTH windows agree (`|best_A - best_B| <= 0.60s`) OR joint normalized Z >= 3.2!
             - Single-window fallback allowed ONLY for short episodes (< 6 min) with `Z >= 3.5`!
          3. Background Auto-Sync Safety Gate:
             - Background auto-sync on launch NEVER commits `|offset| > 25.0s` silently unless corroborated by Tier-A Audio-to-Audio OR Joint 2-Window Consensus (`Z >= 3.6`)!
             - Store auto-synced delays per `(anime_id, episode, provider_id)` in memory (and only persist user-adjusted or high-confidence locks). Add `[0]` shortcut hint in `PlaybackOptions` (`Delay` badge turns amber when `|delay| > 15s`!).
```

---

## 3. Architectural Sketches

### 3.1 GLSL Hardsub Eradication Shader Dataflow (`src/services/shaders.rs` + `src/api/player.rs`)

```
Decoded Frame (MAIN texture in libplacebo)
   |
   +--> GPU Pass (`//!HOOK MAIN` inside `solus_hardsub_{blur,inpaint,matte}_{compact,std,tall}.glsl`)
   |      - Normalizes screen Y coordinate (`HOOKED_pos.y`)
   |      - Checks `y in [band_top, band_bot]` with smooth feathered mask `w`
   |      - Branch by Engine:
   |          * FrostedBlur:  9-tap Gaussian Blur + `vec3 * 0.18` dark cinema scrim
   |          * SmartInpaint: Detect white-core + dark-halo English strokes -> replace from neighbor backdrop
   |          * TotalMatte:   `mix(orig, vec3(0.02, 0.02, 0.03), w * 0.96)`
   |
   +--> `libass` Subtitle Renderer renders crisp Arabic track cleanly ABOVE modified region!
```

*(Note: Generating the tiny parameterized `.glsl` files dynamically into `~/.cache/solus/shaders/` — matching `band_top`/`band_bot` cleanly — lets Lua `[h]` and `[H]` swap them instantaneously via `change-list` without restarting `mpv`!)*

---

## 4. Exact File-by-File Implementation Changes

### File 1: `src/services/shaders.rs`
1. **Generate Hardsub Eradication GLSL Shaders (`ensure_hardsub_shader(mode, band_height) -> Option<PathBuf>`):**
   - `HardsubShieldMode` upgraded from 3 modes to 6 clean presets:
     - `FrostedBlur` *(new default!)*
     - `SmartInpaint`
     - `TotalMatte`
     - `FrameLift` *(shifts video slightly upward via `video-align-y=-0.65` + lower `FrostedBlur`)*
     - `TopEdge` *(`sub-pos=7` + optional bottom blur)*
     - `Off`
   - Add `HardsubBandHeight` (`Compact` 12% `0.85..0.97`, `Standard` 19% `0.78..0.97`, `Tall` 25% `0.72..0.97`).
   - Emit compact verified `libplacebo`-compatible GLSL (`//!HOOK MAIN`, `//!BIND HOOKED`) into `Config::cache_dir()/shaders/solus_hs_*.glsl` so switching modes/heights works both at startup (`glsl-shaders-append`) and live via Lua `[h]`/`[H]`.

### File 2: `src/services/subsync.rs`
1. **Add Tier-A Audio-to-Audio Waveform Cross-Correlator (`correlate_reference_and_target_audio`):**
   - Extracts `8 kHz` 10 Hz spectral/energy flux envelopes from `ref_audio_url` and `target_audio_url` concurrently (`t = 55s` starting after intro or at `120s`).
   - Computes exact Pearson normalized cross-correlation over `delta ∈ [-60.0s .. +60.0s]` (`0.10s` -> `0.02s` sub-frame refinement).
   - Composes with Tier-1 subtitle-to-subtitle alignment (`ref_sub_url` vs `target_sub_url`) if an external/OpenSubtitles track is used.
2. **Upgrade Speech-VAD v3 (`correlate_envelope_against_cues` + `auto_sync_subtitle_to_audio`):**
   - Replace unnormalized dot product with **true zero-mean Pearson Normalized Cross-Correlation (`||mask - mean||_2`)**.
   - Evaluate BOTH `Window A` and `Window B`, compute joint correlation profile `score_joint(delta)`, require multi-window consensus (`|peak_A - peak_B| <= 0.6s`, joint `Z >= 3.0`), and enforce strict false-lock safety gate (`|offset| > 25s` rejected unless joint Z >= 3.8).

### File 3: `src/api/player.rs`
1. **Update `PlayOptions` + `shared_mpv_kv_flags`:**
   - Replace old `opaque-box`-only flags with the new GLSL shader path (`ensure_hardsub_shader` appended to `glsl-shaders`), `FrameLift` (`video-align-y=-0.65`) when enabled, crisp outline styling (`sub-border-size=2.6`, `sub-shadow-offset=1.0`, `sub-margin-y=26`, `sub-pos=96`), plus `secondary-sid=no` & `sub-auto=no`.
2. **Live Hotkeys `[h]` (Engine Cycle) & `[H]` (Band Height Cycle) in `SOLUS_AUDIO_LUA`:**
   - Swaps only the `solus_hs_*.glsl` entry inside `mpv`'s `glsl-shaders` list (preserving Anime4K!) and updates `sub-pos` / `video-align-y` live with OSD toast (`"◈ Hardsub Eradicator: Frosted Blur [Std 19%]"`).

### File 4: `src/ui/play_options.rs` & `src/state/mod.rs` & `src/main.rs`
1. **Non-Truncated Left Prefix Provider Badges (`src/ui/play_options.rs`):**
   - Format rows as `[CLN 1080] MegaPlay Direct`, `[EN 900p] Vidmoly Direct`, `[MUX 1080] ...` so provider badge & quality are ALWAYS 100% visible regardless of name length.
   - Show `[b] Eradicator: <Mode>  [B] Band: <Size>` in `Col 3` footer AND highlight `Delay: +XXXXXms [0 Reset]` in amber if `|subtitle_delay_ms| > 15_000`.
2. **Wire Clean Sibling Audio-to-Audio Reference into `auto_sync_subtitle_to_audio` (`src/main.rs`):**
   - Store `state.active_ref_sync_source: Option<(String, Option<String>)>` alongside `active_stream_url_for_sync` pointing to the highest-priority `Clean` sibling provider (`MegaPlay Direct` / `ZokoAnime` `audio_urls[0]` or `360p` + its native `.vtt`!).
   - Both startup background sync AND in-player `SNAP TO VOICE` (`[y]` / click) automatically run **Tier-A Audio-to-Audio Lock** first (`~2.5s`, `±0.02s`), falling back seamlessly to **Speech-VAD v3 (Pearson + Joint 2-Window)** when no clean sibling exists!
   - Guard episode-isolated persistence so a single-episode delay is stored with confidence checks and `[0]` clears stale poisoned DB entries cleanly.

---

## 5. Verification Plan

1. **Unit Tests (`cargo test`)**
   - `test_pearson_ncc_rejects_cue_density_bias_and_prevents_false_52s_spike`: feeds uneven cue distribution matching the `+52.95s` failure mode and verifies Pearson normalization rejects false spikes while locking the true shift.
   - `test_audio_to_audio_flux_correlator_recovers_bumper_offset`: synthesizes two identical audio envelopes (BGM + OP + speech) shifted by `+7.42s` and verifies exact recovery within `±0.03s` (`Z > 8.0`).
   - `test_hardsub_glsl_shaders_compile_syntax_and_mpv_kv_flags`: validates generated GLSL `//!HOOK MAIN` shaders across all engines (`FrostedBlur`, `SmartInpaint`, `TotalMatte`, `FrameLift`, `TopEdge`) and checks Lua hotkey bindings `[h]`/`[H]`.
   - `test_provider_prefix_badges_never_clip_embedded_tag`: checks `[CLN 1080]` and `[EN 900p]` layout formatting inside `PlaybackOptions`.

2. **Live `mpv` Shader & SubSync Smoke Check**
   - Launch `mpv` on an HTTP stream with generated `solus_hs_blur_std.glsl` + `--vo=gpu-next` to verify **0 GLSL compilation errors** in `mpv --msg-level=vo=v`!
   - Run live Audio-to-Audio + Speech-VAD v3 against *Chainsmoker Cat Ep 2* (`MegaPlay Direct` ↔ `AnimeHeaven Direct` & `Vidmoly`) and verify exact sub-0.1s alignment.
