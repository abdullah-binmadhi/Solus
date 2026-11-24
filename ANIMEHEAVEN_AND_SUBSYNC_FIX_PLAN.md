# Solus — Comprehensive Fix Plan: Multi-Provider Expansion, AnimeHeaven Stability & Automated Speech-VAD Subtitle Sync

**Status:** Updated Proposal / Pre-Implementation Plan (Awaiting Approval)
**Target Branch:** `dev`
**Scope:**
1. **Expand & Harden Streaming Providers ("More + Better Than ZokoAnime")** across **6–7 verified streams / 5 completely independent CDNs** (**MegaPlay Direct MAL**, **Vidmoly Direct via 9anime Bridge**, **AnimeHeaven Direct**, **AnimeGG Direct**, **ZokoAnime Direct + Hardsub**, **MegaPlay Hardsub**, and **HiAnime.fm Direct**), eliminating dead redirects and `/sub`-only gaps.
2. **Fix AnimeHeaven Crashes & Playback Failures** (remove 88 KB / 3s `ax.animeheaven.me` decoy, fix UTF-8 slice panics, harden MP4 NAL/demuxer handling, add multi-mirror failover).
3. **Repair Background & `SNAP TO VOICE` Subtitle Sync** with empirically proven **8 kHz Vocal-Band PCM + 4.0s Local-Mean Subtraction + Dialogue-Cluster Cross-Correlation (`-150s..+150s`)**.
*(IINA and VLC remain permanently removed; unified `libmpv` architecture only.)*

---

## 1. Executive Summary of Root Causes (Empirically Verified)

### A. Why Only 1–2 Providers Appear Today (& Better-Than-Zoko Gaps)

1. **`hianime.at` 301 Redirect Bottleneck + Tight Timeout Starves Multi-Server Discovery**
   - [`fetch_all_hianime_providers_identity`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1471-L1621) currently queries `https://hianime.at/search?...`, which performs a slow cross-domain redirect to `https://hianime.fm/` (`4.14s` observed just for search vs **`0.61s`** calling `https://hianime.fm/` directly).
   - Combined with 3 sequential HTTP round-trips inside a tight `4–7s` [`probe_timeout`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1241), HiAnime/MegaPlay discovery frequently times out completely, leaving only **ZokoAnime** (1 provider) or crashing **AnimeHeaven** (2).
   - Furthermore, [`extract_anime_id`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2819-L2837) blindly takes index-0 search hits instead of scoring with [`score_candidate_title`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1816).

2. **Missing `0.30s` Direct MAL Fast-Path for MegaPlay (`/sub` & `/hsub`)**
   - `https://megaplay.buzz/stream/mal/{mal_id}/{ep}/{sub|hsub|dub}` resolves **directly by MAL ID in `0.30s`** without scraping HiAnime at all (verified across *Tokyo Ghoul*, *Aoashi S2 Ep 8*, and *Chainsmoker Cat Ep 1*), yet Solus never calls this endpoint directly.
   - Calling `/hsub` independently yields an alternate CDN (`emeralddrift.world` vs `mysubsplay.com`), solving cases where `/sub` lacks working audio.

3. **Missing `/sub` -> `/hsub` Fallback Breaks Episodes Like *Chainsmoker Cat Ep 1***
   - Live probe of `https://zokoanime.video/stream/mal/63403/1/sub` returned **`0` bytes (`has __P: False`)**, whereas `https://zokoanime.video/stream/mal/63403/1/hsub` returned **`1,656` bytes (`has __P: True`)**.
   - [`fetch_stream_for_mal`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2561-L2572) hardcoded `sub` and never retried `hsub`. Both ZokoAnime and HiAnime ignored `hsub` tracks.

4. **No Clean Standard-HLS Provider Independent of Zoko/MegaPlay**
   - ZokoAnime serves `.ts.jpg` / disguised-PNG HLS segments that stress extractors and slow `ffmpeg` sub-sync.
   - Live testing across 25+ sites identified **`9anime.org.lv` -> `Vidmoly` (`Omega` server: `vidmoly.biz` / `vidmoly.net` -> `vmpx.online` / `vmget.online`)**, which responds in **`~0.40s–0.85s`**, serves **clean standard `#EXTM3U` MPEG-TS HLS (`1600x900` / `480p`, `h264 + aac 2ch` audio)** with **zero `.ts.jpg` or PNG hacks**, and works reliably even where Zoko/MegaPlay break (e.g., *Chainsmoker Cat Ep 1*, *Aoashi S2 Ep 8*, *One Piece*).

---

### B. Why AnimeHeaven Keeps Crashing / Failing Playback

1. **The 3-Second Decoy MP4 (`ax.animeheaven.me`) Always Wins Probe Selection**
   - Every AnimeHeaven `gate.php` page returns `https://ax.animeheaven.me/video.mp4?2c523c8cd9cc129bd13fc7407fa81220&eed805517af9a226eed805517af9a226` as `<source>` #1 (`Content-Length: 88566` bytes, 3-second video that exits `mpv` in `0.73s` during Solus's 1.5s/3.2s loading-screen transition).
   - Real streams (`co`/`rk`/`ci`/`cx.animeheaven.me`, **`137 MB–202 MB`**, matching the episode `hex_key`) sit at `<source>` #2+, alongside dead `ct`/`ck`/`&error`/`&error2` mirrors.
   - [`parse_animeheaven_gate_videos`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2074-L2101) did not exclude `ax.animeheaven.me` or mismatched `hex_key`s, and [`verify_mp4_reachable`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1203-L1229) only checked `Range: bytes=0-63`, letting the 88 KB decoy pass first every time.

2. **Raw UTF-8 Byte-Offset Slice Panics (`byte index N is not a char boundary`)**
   - [`parse_animeheaven_search_candidates`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2021), [`parse_animeheaven_episode_gate_key`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2049), [`parse_animeheaven_gate_videos`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2080), and [`parse_animegg_video_sources`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1789) slice `&str` at raw byte offsets (`+120` / `+260` / `+320`), panicking whenever multi-byte UTF-8 titles (e.g., `Tokyo Ghoul √A`, smart quotes, em-dashes) cross the boundary.

3. **Corrupt MP4 Packets (`Invalid NAL unit size` / `Packet corrupt`) & Range-Seek Stalls**
   - Real AnimeHeaven mirrors emit `[ffmpeg/demuxer] Packet corrupt` and `Invalid NAL unit size`. Global `http_multiple=1` in [`mpv_network_kv_args`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs#L1253) causes parallel byte-range stalls/crashes on seek without mirror fallback.

---

### C. Why Subtitle Auto-Sync (`SNAP TO VOICE` & Background Sync) Fails

1. **`ffmpeg 9.0.2` Rejects ZokoAnime `.ts.jpg` Segments (`rc=183` in `1.3s`):** [`extract_speech_intervals_ffmpeg`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs#L469-L536) omits `-extension_picky 0` before `-i`, and [`HlsProxyServer`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/hls_proxy.rs#L184) is skipped unless fake PNG headers exist.
2. **MegaPlay Video-Only `stream_url` Sent to Analyzer:** [`start_playback`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L10345) saves `opts.stream_url` (often video-only `index-f1.m3u8`) in `active_stream_url_for_sync` instead of `opts.audio_urls[0]`, leaving `ffmpeg -vn` with zero audio.
3. **`12s` Hard Timeout Kills 1080p HLS & HTTP MP4:** Sync pulls full `1080p` video streams (timing out at `25s` on *Aoashi S2 Ep 8*), and HTTP MP4 scans (`14.58s` measured) exceed [`subsync.rs:523`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs#L523).
4. **Fixed `-28 dB silencedetect` & OP-Song Windows:** Continuous anime BGM keeps audio > `-28 dB` (`> 25s` segments dropped), while [`auto_sync_subtitle_to_audio`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs#L711-L725) and [`start_playback`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L10364) sample `anchor_pos = 140.0` — dead inside 90s zero-cue OP songs (`86s..195s`).
5. **On-Demand HTTP Subtitle & Format Detection Bugs:** [`AutoSyncSubRequested`](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L687) refuses `http://` sub paths, `AutoSyncSubCompleted` clamps to `±120s`, and [`ensure_synchronized_subtitle`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs#L920-L927) checks `.contains("WEBVTT")` anywhere in the file body rather than line 1.

---

## 2. Expanded Provider Architecture & Live Candidate Audit

### 2.1 Empirical Audit of Streaming Sources (Tested Live Against `Chainsmoker Cat`, `Aoashi S2`, `Tokyo Ghoul`, `Frieren`)

| Candidate Source | Live Status | Speed / Anti-Bot | Plan Decision |
|---|---|---|---|
| **MegaPlay Direct MAL** (`megaplay.buzz/stream/mal/{mal}/{ep}/{sub\|dub}`) | **HTTP 200** | **`0.30s`**, No CF | **ADD Fast-Path (#1 Priority)** — direct HLS + softsubs + skip markers |
| **Vidmoly Direct** (`9anime.org.lv` `Omega` -> `vidmoly.biz/.net`) | **HTTP 200** | **`0.40–0.85s`**, No CF | **ADD New Provider** — clean MPEG-TS `900p`/`480p` `h264+aac`, zero `.ts.jpg` hacks |
| **AnimeHeaven Direct** (`co`/`rk`/`ci.animeheaven.me`) | **HTTP 200** | **`0.56s`**, `137–202 MB` MP4 | **REPAIR & Harden** — strip `ax` 3s decoy, safe UTF-8, multi-mirror fallback |
| **ZokoAnime Direct** (`zokoanime.video/stream/mal/{mal}/{ep}/{sub\|hsub\|dub}`) | **HTTP 200** | **`0.37s`**, No CF | **UPGRADE** — add auto `/sub -> /hsub` fallback (fixes *Chainsmoker Cat Ep 1*) |
| **MegaPlay (Hardsub)** (`megaplay.buzz/stream/mal/{mal}/{ep}/hsub`) | **HTTP 200** | **`0.35s`**, Alt CDN (`emeralddrift`) | **ADD Distinct Provider** — independent CDN/encode when `/sub` audio fails |
| **AnimeGG Direct** (`animegg.org`) | **HTTP 200** | **`1.29s`**, No CF | **HARDEN** — UTF-8 safe slices + `>= 512 KiB` MP4 verification |
| **HiAnime Direct** (`hianime.fm` + `9anime.org.lv` `HD-1`/`HD-2`) | **HTTP 200** | **`0.61s`** (was `4.14s` redirect) | **REPAIR** — drop `hianime.at` redirect, score titles properly, parse `hsub` |
| **GogoAnimes / Anitaku / AniVibe** (`gogoanimes.cv`, etc.) | Redundant / Empty | Mirrors `megaplay/s-2` | Reuse only `9anime.org.lv` (faster & includes `Vidmoly`) |
| *AllAnime / AnimePahe / TokyoInsider / WCO* | HTTP 403 / DDoS | Cloudflare Turnstile / DDoS-Guard | **Exclude** — violates no-anti-bot-bypass constraint & brittle |
| *AnimeKai / VidWish / S3Taku / SunshineRays93 / Kaido* | Dead / 404 / Parked | — | **Exclude** — verified dead endpoints |

### 2.2 Expanded Multi-CDN Discovery Topology (`probe_timeout = 6..9s` concurrent)

```
AnimeSearchIdentity (AniList ID, MAL ID, English, Romaji, Synonyms, Format)
   |
   +--> [1] MegaPlay Direct MAL (sub/dub + distinct hsub CDN)      [0.30s]  --> HLS + Subs + Skip
   +--> [2] 9anime.org.lv Bridge -> Vidmoly Direct + HD-1 / HD-2  [0.85s]  --> Clean MPEG-TS (vmpx/vmget)
   +--> [3] AnimeHeaven Direct -> co/rk/ci (strip ax 88KB decoy)  [0.90s]  --> 1080p MP4 (+ Mirror 2, 3)
   +--> [4] ZokoAnime Direct (MAL sub -> auto hsub fallback)      [0.40s]  --> HLS + Multi-Lang Subs
   +--> [5] AnimeGG Direct (UTF-8 safe + size-verified MP4)       [1.30s]  --> 1080p/720p/480p MP4
   +--> [6] HiAnime.fm Direct (no hianime.at 4s redirect!)        [1.20s]  --> Scored Title + Extra Servers
   +--> [7] External Community Subtitles (OpenSubtitles pool)     [1.50s]  --> Pooled across ALL providers
   |
   v
Deduplicate by normalized (host + path) -> Rank clean direct/MPEG-TS/audio-verified streams first ->
Unified `all_subtitles` injected into EVERY provider!
```

### 2.3 Hardened AnimeHeaven Resolution Pipeline

```
search.php?s=<alias> (UTF-8 char-boundary safe slicing)
   -> score_candidate_title() -> anime.php -> gate_key [UTF-8 safe]
   -> gate.php (Cookie: key=<hex_key>) -> parse_animeheaven_gate_videos(html, Some(hex_key))
   -> DROP: ax.animeheaven.me / wrong hex_key / &error / &error2
   -> RANK CDN hosts: co.* > rk.* > ci.* > cx.* > others
   -> Concurrent Range: bytes=0-63 probe across ALL mirrors:
        Verify ISO BMFF (`ftyp`/`moov`/`mdat`) or EBML, NOT HTML, Content-Range/Length >= 512 KiB
   -> Emit `AnimeHeaven Direct` with qualities: `1080p`, `1080p · Mirror 2`, `1080p · Mirror 3`
```

### 2.4 Next-Gen Vocal-Envelope PCM Cross-Correlator (`SubSync Engine v2`)

```
Audio Source Selector (`resolve_sync_stream_url`):
  1. `opts.audio_urls[0]` (MegaPlay demuxed audio = zero video)
  2. Lowest-height `QualityOption` (`360p`/`480p`/`480p Vidmoly` before `1080p`)
  3. Local `HlsProxyServer` wrapper when `.ts.jpg`/PNG disguised, else `-extension_picky 0`
   |
   v
Dialogue-Cluster Window Finder (`select_dialogue_windows`):
  - Scans full `sub_cues` timeline; avoids empty 86s..195s OP gaps completely
  - Window A: cluster near `cur_pos` (if >= 8 cues around `cur_pos`), else post-OP body (180s..450s)
  - Window B: later act cluster (480s..900s) for automatic retry if Window A < Z-threshold
   |
   v
ffmpeg Fast Pipe (`timeout: 22s remote / 8s local`, in-memory cache per URL+window):
  - `-af "highpass=f=300,lowpass=f=3000,aresample=8000" -ac 1 -f s16le pipe:1`
   - 10Hz Frame RMS (`E[t] = ln(1 + rms)`) minus **4.0s Running Local Mean (`M[t]`)** -> cancels BGM/OP!
   |
   v
Bidirectional Onset/Gap Correlation (`delta in [-150.0s .. +150.0s]`):
  - +2.0 cue onset (0..300ms), +1.0 body, -0.5 silence gaps (+-300ms)
  - Coarse 0.20s -> Fine 0.05s around top 3 peaks -> Accept if Z-score >= 2.6
```
*(Empirically verified on ZokoAnime Tokyo Ghoul Ep 2: `9.70s`, `+0.20s`, `Z = 3.49`; AnimeHeaven Tokyo Ghoul Ep 2: `8.53s`, `+0.10s`, `Z = 3.44`.)*

---

## 3. Exact File-by-File Implementation Changes

### File 1: `src/api/subtitles.rs`

1. **Direct MegaPlay MAL Fast-Path & Separate Hardsub Discovery**
   - Add `fetch_megaplay_direct_providers(client, mal_id, episode, is_dub) -> Vec<(StreamInfo, ProviderOption)>`:
     - When `!is_dub`, concurrently query `https://megaplay.buzz/stream/mal/{mal_id}/{episode}/sub` and `https://megaplay.buzz/stream/mal/{mal_id}/{episode}/hsub`; when `is_dub`, query `/dub`.
     - Parse via [`parse_megaplay_embed`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1665) + verify audio tracks / segments via [`probe_hls_segment_health`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1135).
     - Emit `MegaPlay Direct` (`id: "megaplay_mal"`) for `/sub` (`/dub`) and, when `/hsub` resolves to a distinct `master_url`, emit `MegaPlay (Hardsub)` (`id: "megaplay_hsub"`, `supports_sub: true`).

2. **New `9anime.org.lv` + `Vidmoly Direct` Independent Provider**
   - Add `fetch_9anime_and_vidmoly_providers(client, identity, episode, is_dub) -> Vec<ProviderOption>`:
     - Search `https://9anime.org.lv/?s={alias}` using up to 2 aliases, strictly score `/anime/<slug>/` results with `score_candidate_title` (`>= 15`, preferring `-dub/` when `is_dub` and excluding `-dub/` when `!is_dub`).
     - Fetch `https://9anime.org.lv/{ep_slug}-episode-{episode}/` (handling `{slug}-dub-episode-{episode}` and matching exact episode href fallback on the anime page when slug differs).
     - Decode `<option value="<base64>">Label</option>` tabs:
       - **`Vidmoly Direct` (`vidmoly.biz` / `vidmoly.net` -> "Omega" tab):** Fetch embed with `Referer: https://9anime.org.lv/`, extract `master.m3u8` URL (`parse_vidmoly_master_m3u8`), fetch variant ladder with `Referer: https://vidmoly.biz/`, verify health (`needs_hls_proxy = false`), and emit `ProviderOption { id: "vidmoly", name: "Vidmoly Direct", ... }`.
       - **`9anime MegaPlay` (`gogoanime.me.uk`):** Extract nested iframe (`megaplay.buzz/stream/s-2/...`) as additional verified server when its `master_url` differs from existing entries (`HD-1` / `HD-2` fallback).

3. **Repair ZokoAnime `/hsub` Fallback & HiAnime Direct (`hianime.fm`)**
   - In [`fetch_stream_for_mal`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2561): when `!is_dub`, probe `/sub` first; if empty or `parse_stream_embed` returns `Err(())`, automatically fall back to `/hsub` (fixing *Chainsmoker Cat Ep 1*). Also extract `ZokoAnime (Hardsub)` if distinct when both work.
   - Update [`fetch_all_hianime_providers_identity`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1471) & `fetch_all_hianime_providers`:
     - Replace `https://hianime.at/` with `https://hianime.fm/` (eliminates ~3.5s redirect stall).
     - Replace [`extract_anime_id`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2819) with scored selection using `score_candidate_title` over search cards (`class="dynamic-name"` + `href`), avoiding OVA/sequel mismatches.
     - Accept `data-type="hsub"` fallback alongside `data-type="sub"` when `!is_dub`.
   - Raise `probe_timeout` in [`fetch_stream_info_with_identity`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L1241) from `clamp(4, 7)` to `clamp(6, 9)` (concurrent via `tokio::join!`).
   - Normalize CDN deduplication key (host + path without ephemeral token query) and rank verified providers by stream reliability/purity:
     `MegaPlay Direct` / `Vidmoly Direct` -> `AnimeHeaven Direct` -> `AnimeGG Direct` -> `ZokoAnime` -> `MegaPlay (Hardsub)` -> `HiAnime/9anime Alts`, with unified `all_subtitles` copied into every provider!

4. **UTF-8 Safety & AnimeHeaven Decoy Elimination**
   - Add `fn slice_at_most_bytes(s: &str, start: usize, max_len: usize) -> &str` (char-boundary safe) and replace unsafe byte slices at lines `1789`, `2021`, `2049`, `2080`.
   - Upgrade [`parse_animeheaven_gate_videos`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2074) to accept `expected_hex_key: Option<&str>`, drop `ax.animeheaven.me`, `&error`, `&error2`, and mismatched keys, and sort mirrors `co.` > `rk.` > `ci.` > `cx.`.
   - Upgrade `verify_mp4_reachable` -> `verify_mp4_stream(client, url, referer, min_total_bytes)` verifying container header (`ftyp` / EBML) + total stream size (`>= 512_000` bytes from `Content-Range` / `Content-Length`).
   - Probe all AnimeHeaven mirrors concurrently in [`fetch_animeheaven_provider`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/subtitles.rs#L2104) and expose verified backups (`1080p`, `1080p · Mirror 2`, `1080p · Mirror 3`).

---

### File 2: `src/api/player.rs`

1. **Direct HTTP MP4 Demuxer Stability (`libmpv`)**
   - In [`mpv_network_kv_args`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs#L1253): add flag/helper or conditional check for direct progressive MP4 URLs (non-`.m3u8` / AnimeHeaven / AnimeGG) so pipelined `http_multiple=1` is disabled in favor of single persistent HTTP range connections (`http_persistent=1`) plus `demuxer-lavf-hacks=yes`, preventing parallel-seek Packet Corrupt stalls/crashes.
2. **Lua HUD Feedback Update**
   - Update [`snap_sub_to_voice`](file:///Users/abdullahbinmadhi/Projects/anime/src/api/player.rs#L1101-L1103) OSD text from `"(-90s..+90s)"` to `"(-150s..+150s)"` with clear analysis progress feedback.

---

### File 3: `src/services/subsync.rs`

1. **Strict WebVTT vs SRT Extension Detection + Referer Retry ([`ensure_synchronized_subtitle`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/subsync.rs#L813-L927))**
   - Retry without `Referer: https://zokoanime.video/` when external OpenSubtitles/MegaPlay subtitle URLs return `401`/`403`.
   - Detect `"vtt"` only when the first non-empty UTF-8 BOM-stripped line starts with `"WEBVTT"`.
2. **Vocal-Envelope PCM Extractor (`8 kHz Mono s16le` Pipe + In-Memory Cache)**
   - Implement `extract_vocal_envelope_ffmpeg(stream_url, referer, start_secs, dur_secs)`:
     - Pass `-extension_picky 0 -allowed_extensions ALL -allowed_segment_extensions ALL` before `-i` for HLS URLs (with automatic fallback without `-extension_picky 0` for older ffmpeg).
     - Decode `highpass=f=300,lowpass=f=3000,aresample=8000 -ac 1 -f s16le pipe:1` with `22s` remote timeout (`8s` local/proxy).
     - Store `10 Hz` log-RMS frames (`ln(1 + rms)`) in an in-memory cache keyed by `(url, window_start)`.
3. **4.0s Local-Mean Subtraction + Dialogue-Cluster Window Finder + `[-150s..+150s]` Correlator**
   - Implement `select_dialogue_windows(sub_cues, cur_pos)` avoiding zero-cue OP/ED gaps.
   - Apply `4.0s` centered running-mean subtraction to audio energy, score against full `sub_cues` bidirectional mask (`+2.0` onset, `+1.0` body, `-0.5` gap) across `[-150.0, +150.0]`, refine to `0.05s`, accept when `Z >= 2.6`, and automatically try Window B if Window A is ambiguous.

---

### File 4: `src/main.rs` & `src/services/ipc.rs`

1. **Optimal Sync Audio Selection (`start_playback` & `StreamSwitchReady`)**
   - Add `resolve_sync_stream_url(opts, q_pool, proxy_base)`:
     - Priority 1: `opts.audio_urls.first()` (MegaPlay demuxed audio).
     - Priority 2: Lowest resolution non-empty `QualityOption` (`360p`/`480p` first, `480p Vidmoly`, etc. — cuts HLS sync download by ~85%, wrapped in `HlsProxyServer` if `.m3u8` proxy is active).
     - Priority 3: `opts.stream_url`.
   - Allow `AppMessage::AutoSyncSubRequested` ([main.rs:687](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L687)) to download/cache uncached `http://` sub paths on demand via `ensure_synchronized_subtitle` instead of aborting.
   - Pass `start_pos` >= `185.0` (else `0.0`) on launch background sync ([main.rs:10364](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L10364)) so `select_dialogue_windows` avoids hardcoded `140s` OP windows, and widen clamp at [main.rs:723](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L723) to `[-150_000, 150_000]`.
2. **Early Startup Failover for Multi-Mirror MP4 Providers (`ipc.rs` + `main.rs`)**
   - In [`src/services/ipc.rs`](file:///Users/abdullahbinmadhi/Projects/anime/src/services/ipc.rs#L882-L905), distinguish early pre-playback stream errors (`!stream_started && elapsed < 6s`) as `PLAYBACK_EXIT_STREAM_FAILED` so `main.rs` automatically retries verified `Mirror 2` (or falls back cleanly) before surfacing an error toast.

---

## 4. Verification Plan (Post-Implementation)

1. **Unit Tests (`cargo test`)**
   - `parse_animeheaven_search_candidates`, `parse_animeheaven_episode_gate_key`, `parse_animeheaven_gate_videos`, and `parse_animegg_video_sources` UTF-8 boundary panic tests + decoy `ax.animeheaven.me` / wrong `hex_key` rejection.
   - `parse_vidmoly_master_m3u8` + `9anime.org.lv` base64 `<option>` tab extraction unit tests (clean `Omega` + `HD-1`/`HD-2`).
   - `hianime.fm` scored title matching (`Tokyo Ghoul` vs `Tokyo Ghoul √A` / `Jack` / `Pinto`).
   - `subsync` synthetic BGM-flooded (`-12 dB` steady noise) `-14.5s` and `+108.0s` shift recovery (`±0.15s` tolerance) + OP gap exclusion test (`select_dialogue_windows`).
   - `resolve_sync_stream_url` preferring `audio_urls[0]` and `480p`/`360p` over `1080p`.

2. **Live Integration Probes & Release Run**
   - Live probe **Tokyo Ghoul**, **Chainsmoker Cat Ep 1**, **Aoashi S2 Ep 8**, and **Frieren Ep 1** across the expanded provider registry — verify **4–6 distinct providers** appear (including **Vidmoly Direct**, **MegaPlay Direct**, **AnimeHeaven Direct**, **ZokoAnime (Hardsub/Sub)**, **MegaPlay Hardsub**, **AnimeGG**), **0** `ax.animeheaven.me` decoys, and *Chainsmoker Cat Ep 1* plays with clean sound.
   - Test live `auto_sync_subtitle_to_audio` against Vidmoly, ZokoAnime `.ts.jpg`, MegaPlay demuxed audio, and AnimeHeaven MP4 (`< 10s` per window).
   - Commit & push to `dev` once approved and verify live playback in `cargo run --release`.
