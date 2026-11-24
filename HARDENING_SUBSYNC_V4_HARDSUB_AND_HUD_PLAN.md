# Hardsub Eradicator Precision, SubSync v4 Accuracy & HUD Controls Integration Plan

> Deep diagnostic and implementation blueprint fixing:
> 1. **Smart Inpaint** residual text artifacts (eliminating baked English text completely)
> 2. **Frosted Blur**, **Total Blackout**, and **Frame Lift** subtitle alignment drift
> 3. **SubSync (`[y]`)** false `93%` matches (e.g. AnimeHeaven) where a subtitle overlaps random vocals without belonging to that spoken sentence — enforcing true language/speech cadence verification
> 4. **Skip Intro / Skip Ending / Next Episode** random timing popups — removing auto-popups and embedding them permanently inside the **Solus Info HUD (`[i]` / `[I]`)** directly below the **Sub Sync** panel (plus fixing RTL/UTF-8 clipping in the HUD preview)

---

## 1. Root-Cause Findings (Code & Visual Evidence)

| Area | Symptom Observed | Exact Root Cause in Codebase |
|---|---|---|
| **Smart Inpaint** | English hardsub leaves ugly blocky/jagged halos & visible text fragments | Single-pass `solus_hs_inpaint` uses hard per-pixel thresholds (`luma > 0.70`, `local_max - local_min > 0.42`) without **mask dilation**; compressed 4:2:0 anti-aliased edges (`luma 0.25..0.68`) & shadows leak outside the mask and pollute neighboring samples. |
| **Frosted Blur & Total Blackout Alignment** | Arabic subtitles render slightly **below** the blur / blackout band | 1) Both shaders fade out early at `bot - 0.015 .. 0.970` (`y = 0.955..0.970`), leaving the bottom 4.5% of the video uncovered.<br>2) `mpv` defaults to `--sub-use-margins=yes`, pushing `sub-pos=96` downward into letterbox bars on 16:10 Retina screens while `//!HOOK MAIN` only shades inside video coordinates (`0.0..1.0`). |
| **Frame Lift Alignment** | `Frame Lift` leaves baked text unseparated / Arabic misaligned | `video-align-y=-0.65` has **zero** shift on 16:9 windows (no black-bar slack) and only `~3%` shift on 16:10 fullscreen, so baked EN (`y=0.78..0.96`) stays right behind Arabic (`sub-pos=99`). |
| **SubSync `[y]` (`AnimeHeaven` `93%` False Lock)** | Reports `93%` confidence while matching an unrelated vocal & wrong subtitle timing | 1) **Consensus Bypass Bug**: `subsync.rs:1287` had `(windows_agree || best_off.abs() <= 25.0)` — forcing lock even when Window 1 and Window 2 completely disagreed!<br>2) **Weak `r=0.08` threshold + inflated Z-score**: Coarse overlap of 300–3000 Hz noise across 2 short windows inflates `Z` to `~5.0` (`93%`) even when speech pauses, onset timing, and line durations don't match.<br>3) **No Episode-Wide / Gap Verification**: Random adjacent scenes share active voice density at unrelated offsets (`±45s`).<br>4) **Reference-Audio 1-Window OP Trap**: `probe_start = 130.0` (`65s`) frequently hits the OP song or misses cuts; `ensure_synchronized_subtitle` cached `{anime_id}_{ep}_{code}_synced` without hashing `provider_id` / `sub_url`, cross-contaminating providers! |
| **HUD / Skip Controls & Live Sub Preview** | Random/inaccurate Skip Intro/Outro/Next Ep popups; `[Ø...` UTF-8 & text collision in HUD | 1) `render_player_buttons` draws floating pills on `in_op` / `in_ed` / `op_fallback` / `near_end`.<br>2) `string.sub(live_txt, 1, 59)` slices multi-byte UTF-8 Arabic byte-wise (`Ø`), and long RTL text overlaps `[sync_status_msg]` on the same line. |

---

## 2. Architecture & Visual Layout Upgrades

### A. 2-Pass Morphological GPU Smart Inpaint (`src/services/shaders.rs`)

Instead of fragile single-pass pixel thresholds, upgrade `solus_hs_inpaint` to a **true 2-pass `libplacebo` (`//!HOOK MAIN`) pipeline**:

```
Frame Texture (HOOKED)
  │
  ├──► PASS 1 (//!SAVE HS_MASK):
  │      Inside [top, bot]: detect glyph cores (high local contrast ΔL > 0.24
  │      AND/OR bright neutral strokes luma > 0.52, sat < 0.34) + soft dark outline
  │      gradient → outputs continuous text probability field mask ∈ [0.0, 1.0].
  │
  └──► PASS 2 (//!HOOK MAIN, //!BIND HOOKED, //!BIND HS_MASK):
         1) Morphological 7×7 Dilation: max HS_MASK over ±7px radius so ALL
            anti-aliased edges, shadows, and 4:2:0 ringing around English letters
            become solid mask = 1.0.
         2) Outer-Ring Masked Reconstruction: sample clean background pixels from
            vertical rings (±12px, ±22px, ±34px above/below text lines) + wide
            horizontal anchors (±18px, ±32px), weighted by (1.0 - HS_MASK(sample))².
            Poluted text pixels get ZERO weight!
         3) Smooth Blend + Gentle Scrim: blend clean reconstructed background cleanly
            over dilated mask, plus soft dark glass scrim (0.52 tint only inside
            detected text strip) so baked English is 100% erased without blocky halos.
```

*(Note: Live `mpv --vo=gpu-next` compilation of multi-pass `//!SAVE` + `//!BIND` will be tested directly via CLI before committing.)*

---

### B. Exact Coordinate Lock for Blur, Blackout, and Frame Lift (`src/api/player.rs`)

Root problem fixed cleanly by eliminating the coordinate-space mismatch between `libplacebo` (`HOOKED_pos`) and `libass` (`sub-use-margins`):

1. **For `FrostedBlur`, `SmartInpaint`, `TotalMatte`, `TopEdge`**:
   - Explicitly force **`sub-use-margins=no`** and **`sub-ass-force-margins=no`** in both Rust launch flags AND `[h]`/`[H]` Lua overrides! This locks `libass` subtitle vertical positions **100% inside the active video frame** (`0.0..1.0`) regardless of monitor aspect ratio (16:9, 16:10 MacBook Retina, 21:9).
   - Extend shader bottom boundary cleanly down to **`bot = 0.998`** (no fade-out gap above `y = 1.0`!).
   - Anchor Arabic subtitle center safely inside each band preset:

| `HardsubBandHeight` | Video Shader Band (`top .. bot`) | `sub-pos` | `sub-margin-y` | Arabic Baseline Band |
|---|---|---|---|---|
| **Compact (12%)** | `0.835 .. 0.998` | `93` | `14` | `0.855 .. 0.960` |
| **Standard (19%)** | `0.765 .. 0.998` | `91` | `16` | `0.790 .. 0.955` |
| **Tall (25%)** | `0.695 .. 0.998` | `89` | `18` | `0.720 .. 0.950` |

```
  16:9 / 16:10 Video Frame (HOOKED_pos.y)
  0.765 ───────────────────────────────────  ◄── Soft feathered top of shader
        │  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
        │  ░░░  [Arabic Subtitle]  ░░░░░░ │  ◄── sub-pos=91, sub-use-margins=no
        │  ░░░   (dead-centered)   ░░░░░░ │      100% enclosed inside shader!
  0.998 ───────────────────────────────────  ◄── Reaches flush bottom edge
```

2. **For `FrameLift`**:
   - Set **`video-zoom=-0.16`** (~89.5% frame scale) + **`video-align-y=-1.0`** (flush top) + **`sub-use-margins=yes`** + **`sub-ass-force-margins=yes`** + **`sub-pos=99`** + **`sub-margin-y=6`** + **`sub-scale=0.88`**.
   - Even on a strict 16:9 screen, `video-zoom=-0.16` guarantees a **10.5%+ pure obsidian cinema tray strictly underneath the anime picture** (`~19%` on 16:10 MacBook). Arabic sits completely in the black margin below the video frame (`0.90..0.99`), never colliding with baked English inside the video frame (`0.72..0.88`)!
   - *(Optional polish: pair Frame Lift with optional subtle `solus_hs_matte` OR keep full clean lifted frame unobstructed by default since the tray already separates the languages visually! Switching `[h]` cleanly resets `video-zoom=0` and `video-align-y=0` when leaving Frame Lift.)*

---

### C. Integrated Solus Info HUD (`[i]` / `[I]`) Controls & Clean SubSync Preview (`src/api/player.rs`)

1. **Remove ALL auto-triggered / random / hover floating pills** (`in_op`, `in_ed`, `op_fallback`, `near_end`, `skip_op`, `skip_ed`, `next_ep`) outside the HUD.
2. **Embed a dedicated Quick Controls Row directly beneath the Sub Sync Bar** whenever HUD (`[i]`/`[I]`) or `[S]` is open (`y = 494..548`, full `x = 42..1238` width matching the HUD card):

```
 ┌─────────────────────────────────────────────────────────────────────────┐
 │ ◈ SOLUS INFO HUD  [1080p FHD]  [FULLSCREEN]  [PINNED 1Hz]               │
 │ Chainsmoker Cat Episode 1                                               │
 │ ... (Video / Audio / Mastering columns + Demuxer Cache + Keybind bar)   │
 └─────────────────────────────────────────────────────────────────────────┘
 ┌─────────────────────────────────────────────────────────────────────────┐
 │ SUB SYNC +0.00s  [-5s] [-1s] [-0.2s] [◈ AUTO-SYNC VOICE y] [+0.2s]...   │
 │ Live: "انتبه لما يحيط بك لتجنب التدخين"     Status: [Pressed y: Locked] │
 └─────────────────────────────────────────────────────────────────────────┘
 ┌─────────────────────────────────────────────────────────────────────────┐
 │ CONTROLS   [>>| Skip Intro (+85s / OP)]   [>>| Skip Ending]   [>>| Next Ep 2 [n]] │
 └─────────────────────────────────────────────────────────────────────────┘
```

- **Button behavior (always available inside HUD when user opens `[I]`)**:
  - `Skip Intro`: Uses `player_ctx.op_end` if current time is near AniSkip OP window, otherwise seeks cleanly `+85.0s` (`do_skip_intro_op`).
  - `Skip Ending`: Seeks to `player_ctx.ed_end` (or `dur - 12.0` if inside final 3 min, else `+85s`) cleanly (`do_skip_outro_ed`).
  - `Next Episode`: Triggers `do_next_episode()` whenever `player_ctx.next_ep` exists (shows dimmed `No Next Ep` pill if final episode).
  - Keyboard shortcuts `[Tab]` and `[n]`/`[N]` continue to work anytime without needing HUD open.
- **UTF-8 & RTL Fix in `find_active_cue_preview`**:
  - Replace byte-wise `string.sub(live_txt, 1, 59)` with proper **UTF-8 codepoint truncation** `utf8_truncate(str, max_chars)` so multi-byte Arabic characters never split into `[Ø...`.
  - Render `Live Subtitle` (`x = 62..640`) and `Sync Status` (`x = 660..1218`) as **two separate capped columns** so long Arabic cues never collide visually over status text.

---

### D. SubSync v4 — Language/Voice-Accurate Alignment & Rejection of Accidental Overlaps (`src/services/subsync.rs`, `src/main.rs`)

Why did `[y]` report **93%** on AnimeHeaven even when the subtitle didn't match the vocal? Because envelope correlation only checked noisy 300–3000 Hz volume bursts on 2 windows with `r >= 0.08` and bypassed window disagreement via `|| best_off.abs() <= 25.0`.

We replace that with a **4-Stage Precision Verification Pipeline (`SubSync v4`)**:

#### Stage 1: Per-Provider / Per-URL Cache Isolation + Stream Language Targeting
1. **Fix `ensure_synchronized_subtitle` cache key** ([subsync.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs)):
   - Key cache file by `{anime_id}_{episode}_{code}_{provider_slug}_{url_hash8}_synced.{ext}` so switching between `ZokoAnime`, `MegaPlay`, and `AnimeHeaven` (or different OpenSubtitles tracks) **never reuses another provider's pre-shifted file**!
   - Remove the unguarded blind Tier-2 `estimate_audio_lead_time` first-silence shift inside `ensure_synchronized_subtitle` so external subtitles aren't arbitrarily pre-shifted by opening sound effects before Tier-A / Tier-B analyzes real dialogue.
2. **Target the Exact Audio Language Stream (`sub` vs `dub`)**:
   - Inspect/pass `-map 0:a:m:language:jpn?` / `-map 0:a:0` (matching `is_dub`: Japanese `jpn/ja` for Sub, English `eng/en` for Dub) to `ffmpeg` in `extract_vocal_envelope_ffmpeg` so dual-audio streams never accidentally correlate against the wrong language's voice track!

#### Stage 2: Tier-A Upgraded Multi-Window Reference-Audio Waveform Lock
When ANY Clean sibling stream exists (`resolve_clean_ref_sync_source`):
1. Probe **2–3 non-OP/ED windows (`60s` each at `~20%`, `~50%`, `~75%` of episode)** instead of just one `130s` clip inside the OP!
2. Extend cross-correlation search range from `±60s` to **`±90s`** cleanly with stricter multi-window agreement (`|delta_1 - delta_2| <= 0.40s`, `r >= 0.45`, `Z >= 4.2`), guaranteeing zero false OP/BGM locks.
3. For `ref_sub` alignment (`calculate_optimal_alignment`): prefer **provider-native VTTs** (`!is_auto_synced && !opensubtitles`), and verify ≥ 35% of overlapping cues actually align closely before trusting `delta_sub_vs_ref`.

#### Stage 3: Tier-B Upgraded `20 Hz` Harmonic Speech-Activity Detector (`50ms` bins, Music/SFX Resistant)
Inside `pcm_s16le_to_speech_features` (from raw `8 kHz s16le` PCM — pure Rust, zero extra binary dependencies!):
- Compute **50ms frames (`20 Hz`, 400 samples/frame)** giving **2× tighter temporal precision (`50ms` vs `100ms`)** for crisp syllable & sentence boundaries.
- For each frame compute TWO features:
  1. **Log-RMS Speech Band Energy (`300–2800 Hz`)** relative to an episode-window **noise floor (`25th percentile` + centered 6s baseline)** so quiet room tone drops cleanly to `0.0` while real long monologues stay high instead of being attenuated by 4s mean subtraction.
  2. **Short-Term Syllable Modulation & ZCR Gate**: Speech alternates consonants/vowels at 3–8 Hz (`ZCR` variance + frame-to-frame RMS attack `|E[t] - E[t-1]|`). Continuous music chords, wind, and hum have flat envelope variance and are strongly down-weighted (`0.25x`), whereas crisp speech onsets get boosted!

#### Stage 4: **4-Window Episode Consensus + Structural Verification Gate (Fixes the "93% Wrong Vocal" Bug!)**
Instead of 2 overlapping macro windows + broken bypass:
1. **Extract 4 Independent Windows (`45s` each)** spread across distinct acts of the episode (skipping OP `40..170s` and ED final `160s`), centered on dialogue-dense clusters **around `cur_pos_secs` + 3 other distant scenes (≥ 180s apart)** using `tokio::join!`.
2. **NO `|| best_off.abs() <= 25.0` Bypass**:
   - Compute per-window `best_offset` and full Pearson correlation curve `r_w(delta)` over `[-150s..+150s]`.
   - Require **Consensus Agreement**: at least **2 windows** (or **majority ≥ 3 of 4** when 4 succeed) MUST independently peak within **`±0.80s`** of the consensus offset!
   - Detect **Mid-Episode Cut Shifts**: if Window 1+2 cleanly agree on `δ₁` and Window 3+4 cleanly agree on `δ₂` (`|δ₁ - δ₂| > 1.5s`), lock onto the **current playback act's offset (`cur_pos_secs`)** and explicitly notify: `"Scene-Locked +X.XXs (mid-ep cut detected at min ~M)"`!
3. **4-Factor Structural Verification Gate (`verify_sync_structural_fit`)** — directly solves *"subtitle and voice happened to appear at the same time, but subtitle doesn't belong to that vocal"*:
   At candidate `best_offset`:
   - **Check 1 — `Inside-vs-Gap Contrast Ratio` (`>= 1.55x`)**: Mean speech energy **inside** shifted subtitle cues `[s_i + off, e_i + off]` divided by mean speech energy in **pure silent gaps between cues** (`>= 0.70s` away from any cue boundary). If speech keeps happening during subtitle gaps, the offset is a coincidence → reject!
   - **Check 2 — `Onset Attack Alignment Rate` (`>= 55%`)**: Fraction of shifted cues (with `>= 0.5s` preceding silence) that exhibit a genuine speech onset rise in `[s_i + off - 0.25s, s_i + off + 0.65s]`. Random unrelated overlap fails onset attack matching!
   - **Check 3 — `Cue-Length vs Active-Speech Rank Correlation` (`Spearman r_len >= 0.30`)**: Short cues (`< 1.3s`, e.g. `"ماذا؟"`) must overlap short speech bursts, and long multi-clause cues (`> 3.2s`) must overlap long continuous vocal segments! Misaligned vocals put long subtitles over gasps or short cues over monologues → rejected cleanly!
   - **Check 4 — `Pearson Peak Strength` (`best_r >= 0.24` and `Z >= 3.2`)**: Never accept `r = 0.08` noise!
4. **Honest, Mathematically Calibrated Confidence %**:
   - Combine normalized `best_r`, `contrast_ratio`, `onset_rate`, and `agreeing_windows`:
     - Only genuine multi-scene locks with high onset & contrast (`r ≥ 0.45`, `contrast ≥ 2.2`, `onset ≥ 75%`, `3+/4 windows agree`) report `85%–99%`.
     - If any structural gate fails, return **`applied = false`** with an exact diagnostic reason (`"Rejected accidental overlap (onset 32%, contrast 1.12x) — check episode/cut or adjust manually"`), **never** falsely applying `+50s` or reporting `93%`!

---

## 3. Exact File-by-File Implementation Plan

1. **[src/services/shaders.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/services/shaders.rs)**
   - Rewrite `"inpaint"` shader in `ensure_hardsub_shader_file` as a 2-pass (`//!SAVE HS_MASK` + `//!HOOK MAIN` `//!BIND HS_MASK`) morphological dilated-mask + outer-ring background reconstructor + gentle contrast scrim.
   - Update `"blur"` and `"matte"` shaders to keep full attenuation right down to **`bot = 0.998`** (removing the bottom gap where subs slipped underneath).
   - Update shader unit tests (`test_hardsub_eradicator_glsl_shaders_generated_cleanly`) to assert multi-pass `HS_MASK` directives and verify live `mpv --vo=gpu-next` compilation.

2. **[src/api/player.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs)**
   - Update `HardsubBandHeight::bounds()` to `(0.835, 0.998)`, `(0.765, 0.998)`, `(0.695, 0.998)` matching centered `sub-pos` (`93`, `91`, `89`) and `sub-use-margins=no` / `sub-ass-force-margins=no` in both `shared_mpv_kv_flags` and `SOLUS_AUDIO_LUA`.
   - Upgrade `FrameLift` in both Rust flags and `SOLUS_AUDIO_LUA`: `video-zoom=-0.16`, `video-align-y=-1.0`, `sub-use-margins=yes`, `sub-ass-force-margins=yes`, `sub-pos=99`, `sub-margin-y=6`, resetting `video-zoom=0` and `video-align-y=0` cleanly on mode change.
   - Update `SOLUS_AUDIO_LUA` section 5 (`render_player_buttons`):
     - Remove random/auto/hover corner pills (`in_op`, `in_ed`, `op_fallback`, `near_end`).
     - Embed dedicated clickable `Skip Intro`, `Skip Ending`, and `Next Episode` buttons inside the HUD panel below the `SUB SYNC` bar (`y = 494..548`).
     - Add UTF-8 safe `utf8_truncate` helper for `Live Subtitle` preview and separate the cue preview (`x=62`) from `Sync Status` (`x=660`) cleanly.
   - Update `player.rs` unit tests for new flags (`sub-use-margins=no`, `video-zoom=-0.16`, HUD button bindings).

3. **[src/services/subsync.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs)**
   - Scope `ensure_synchronized_subtitle` cache filenames cleanly by `(anime_id, episode, code, provider_id, url_hash8)` and remove blind pre-shift Tier-2 silence shifting.
   - Support target audio stream selection (`dub: bool` -> `jpn` vs `eng` `-map` lookup via `ffprobe`/stream tag fallback) in `extract_vocal_envelope_ffmpeg`.
   - Upgrade envelope extraction to **20 Hz (`50ms`)** syllable-modulated harmonic speech features + noise-floor percentile baseline (`pcm_s16le_to_vocal_envelope`).
   - Upgrade `select_dialogue_windows` to select **4 non-OP/ED dialogue scenes** spanning the episode (`cur_pos` + 3 distributed acts).
   - Remove the `|| best_off.abs() <= 25.0` bypass; implement **strict multi-window consensus + mid-episode cut detection + 4-factor Structural Fit Verification Gate** (`contrast_ratio`, `onset_rate`, `duration_rank_corr`, `pearson_r >= 0.24`) and honest calibrated confidence.
   - Upgrade `sync_subtitle_with_reference_or_vad` Tier-A Reference-Audio to correlate **2–3 non-OP windows (`±90s` lag range)** with strict agreement gating.
   - Add unit tests proving: (a) unrelated dense dialogue overlapping by coincidence is **rejected cleanly (`applied == false`)** instead of locking at `93%`, (b) duration-mismatched scenes are rejected, and (c) true dialogue bursts with clean pauses lock within `±0.15s`.

4. **[src/main.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs)**
   - Pass `provider_id` & `is_dub` context to `ensure_synchronized_subtitle`, `sync_subtitle_with_reference_or_vad`, and `AutoSyncSubRequested`.
   - Tighten `resolve_clean_ref_sync_source` so `ref_sub` prefers verified provider-native subtitles (not unverified `[Auto-Synced]` community tracks).

---

## 4. Verification Checklist (Pre-Commit)

- [ ] `mpv --no-config --vo=gpu-next --frames=1` passes on all 3 shaders (`blur`, `matte`, multi-pass `inpaint` with `HS_MASK`) with **zero** `libplacebo`/GLSL errors.
- [ ] All unit tests pass (`cargo test --quiet -- --test-threads=4 --skip test_fetch_external_subtitles_finds`) including new accidental-overlap rejection tests and alignment flag tests.
- [ ] `cargo build --release` builds cleanly.
- [ ] Live playback spot check:
  - Test *Chainsmoker Cat* / *Tokyo Ghoul* in **Smart Inpaint**, **Frosted Blur**, **Total Blackout**, and **Frame Lift** (`16:10` fullscreen + `16:9` window).
  - Test `[y]` SubSync on **AnimeHeaven** verifying clean lock OR honest rejection on mismatched cuts without bogus `93%`.
  - Press `[I]` and verify `[Skip Intro]`, `[Skip Ending]`, and `[Next Episode]` appear cleanly below `SUB SYNC`, UTF-8 Arabic preview never corrupts (`[Ø`), and no random skip pills pop up mid-episode.
