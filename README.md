# ❖ SOLUS

**A High-Definition Terminal Anime Theatre, Discovery Crucible & AniList Companion**

`Solus` is a keyboard-driven terminal anime theatre built in Rust with `ratatui`, `sqlx` (SQLite), and `ratatui-image`. Designed for modern GPU-accelerated terminals like **Kitty**, **Ghostty**, and **WezTerm**, Solus pairs pixel-accurate cover art and widescreen episode stills with a two-stage recommendation engine, genre-attuned episode chronicles, zero-token AniList synchronization, and seamless external playback via [`ani-cli`](https://github.com/pystardust/ani-cli).

```text
╔════════════════════════════════════════════════════════════════════════════════════════╗
║  ❖ SOLUS // TERMINAL ANIME THEATRE                                    [t] Crucible [?] ║
╠════════════════════════════════════════════════════════════════════════════════════════╣
║  ╭──────────────────╮  ZANKYOU NO TERROR (Terror in Resonance)                         ║
║  │                  │  ★ 81%  ·  TV  ·  FINISHED  ·  11 Episodes  ·  [ SUB ]           ║
║  │   1200 × 1800    │  [ Drama ]  [ Mystery ]  [ Psychological ]  [ Thriller ]         ║
║  │   HD PORTRAIT    │                                                                  ║
║  │    COVER ART     │  In an alternate version of the present, Tokyo has been          ║
║  │  (Kitty Image)   │  decimated by a shocking terrorist attack, and the only hint     ║
║  │                  │  to the identity of the culprit is a bizarre video uploaded...   ║
║  ╰──────────────────╯  [ ▶ Continue Ep 4 ]  [ - Remove Watchlist ]                     ║
╠════════════════════════════════════════════════════════════════════════════════════════╣
║  ❖ Episodes (11) [v: Split View] ───┬─ ❖ Episode Inspector ────────────────────────────╣
║    01. Falling                 󰄬    │  ╭──────────────────────╮  EPISODE 04            ║
║    02. Call & Response         󰄬    │  │     1280 × 768       │  Break Through         ║
║    03. Search & Destroy        󰄬    │  │  WIDESCREEN EPISODE  │  ❖ 24m · FHD · Aired   ║
║  ▸ 04. Break Through           ❖    │  │    STILL PREVIEW     │  [ Enter: Play ]       ║
║    05. Hide & Seek                  │  ╰──────────────────────╯  [ o: Stream ] [ w ]   ║
║    06. Ready or Not                 │  ─ Episode Synopsis ──────────────────────────── ║
║    07. Deuce                        │  Nine hacks into the police database while Lisa  ║
║    08. My Fair Lady                 │  runs away from home and ends up caught in the   ║
║    09. Highs & Lows                 │  crossfire of Sphinx's next riddle across Tokyo. ║
╚═════════════════════════════════════╩══════════════════════════════════════════════════╝
```

---

## ✦ Why Solus?

Most terminal media tools force a tradeoff between speed and atmosphere: either you use a barebones text prompt with zero visual context, or you open a heavy browser tab cluttered with ads and sluggish layouts.

**Solus** brings a cinematic home-theatre experience directly into your terminal without sacrificing instant keyboard navigation or offline-first reliability:

- **True Pixel Art in the Terminal** — Renders crisp `1200×1800` portrait posters and `1280×768` widescreen episode stills over the **Kitty Graphics Protocol**, with automatic halfblock fallback on standard truecolor terminals.
- **Split-Deck Episode Inspector** — Browse episodes in a dual-pane **Split-Deck View** (default) or toggle (`v`) into a **Horizontal Widescreen Card Strip**. Each episode displays its own unique broadcast title, widescreen thumbnail, runtime/airing telemetry, and full-paragraph synopsis.
- **Genre-Attuned Chronicles** — Short or sparse episode blurbs are expanded into rich, spoiler-guarded paragraphs written in the literary voice of the show's genre—from witty, chaotic comedic timing for **Comedy** (`ComedicRiot`) to bleak, suffocating psychological prose for **Thriller / Mystery / Dark** series (`GloomyNoir`).
- **The Tailored Crucible (`t` & `f`)** — A two-stage discovery engine that profiles your taste through a 10-question **Soul Resonance Quiz** and a 6-question **Multi-Select Refinement Crucible** (complete with live **Anchor Anime Search** that mines AniList community tags, studios, and recommendation graphs).
- **Zero-Token AniList Sync & Local Overrides** — Sync by AniList username or import your local AniList JSON/XML backup (`i` in Settings) with no OAuth token hassle. Local removals from **Continue Watching** (`x` / `d`) and **Watchlist** (`+`) are persisted in SQLite (`dismissed_sync`) so deleted shows never reappear on restart.
- **Stream & Subtitle Matrix (`o`)** — Inspect available M3U8 stream resolutions (`1080p`, `720p`, `480p`, `360p`) and soft-subtitle tracks (`English`, `Arabic`, `Spanish`, `French`, `German`, `Portuguese`, `Italian`, `Russian`, and more) before handing playback off to `mpv`, `iina`, or `vlc`.

---

## ◈ Core Features in Depth

### 1. Split-Deck Episode Inspector & Multi-Tier Hydration
When you open any anime on the Detail screen, Solus launches a background hydration pipeline that merges episode telemetry from three sources in real time:
1. **AniList GraphQL (`streamingEpisodes`)** — Official Crunchyroll/HIDIVE episode titles and thumbnails.
2. **`api.ani.zip` (TVDB / AniDB Bridge)** — High-resolution widescreen episode stills and episode-by-episode synopses keyed by AniList ID.
3. **Kitsu Episode Catalog** — Fallback episode titles, descriptions, and thumbnails.

Each episode's thumbnail is cached in an isolated Kitty image protocol slot so switching between episodes or toggling between **Split-Deck View** and **List View** (`v`) is instantaneous and flicker-free.

### 2. Genre-Attuned Synopsis Engine
Nothing breaks immersion like a one-sentence episode blurb floating in an empty box—or a wiki recap that spoils the final scene. Solus detects each anime's dominant literary tone (`GenreVoice`) and composes full, dense paragraphs that fill the inspector without spoiling third-act twists:

| Genre Archetype | Literary Voice | Tone & Atmosphere |
| :--- | :--- | :--- |
| **Comedy / Parody / Gag** | `ComedicRiot` | Witty, playful, chaotic, and deadpan; highlights schemes backfiring in record time |
| **Psychological / Thriller / Horror / Mystery** | `GloomyNoir` | Bleak, oppressive, and brooding; steeped in paranoia, moral rot, and quiet dread |
| **Sports / Competitive** | `FierceArena` | High-tempo tactical intensity, spatial awareness, stamina, and competitive hunger |
| **Romance / Slice of Life / Iyashikei** | `WarmHeartfelt` | Sunlit warmth, observant humor, and tender everyday connections |
| **Sci-Fi / Cyberpunk / Mecha** | `CyberDystopia` | Cerebral intrigue, neon-lit circuitry, and questions of human agency |
| **Action / Adventure / Fantasy** | `VisceralEpic` | Cinematic momentum, clashing convictions, and trials under fire |
| **Drama / Tragedy** | `MelancholicElegy` | Poignant introspection, unspoken regrets, and emotional gravity |

### 3. The Tailored Crucible (`t`) & 6-Question Refinement (`f`)
Press `t` from the Home screen to enter **For You // Tailored**:
- **Stage 1 — 10-Question Soul Resonance Quiz**: Evaluates your current mood, pacing preference, narrative complexity, emotional weight, and dealbreakers to assign your **Allagan / Ascian Persona** and **Astral / Umbral Polarity**.
- **Stage 2 — Press `f` to Refine Results**: Opens a 6-question multi-select refinement crucible:
  - **Questions 1–5 (Multi-Select Axes)**: Toggle multiple answers with `Space` and lock them in with `Enter` across narrative hooks, protagonist archetypes, world-building settings, visual direction, and emotional payoff.
  - **Question 6 (Live Anchor Anime Search)**: Type the name of an anime you love (e.g., *Death Note*, *Monster*, *Steins;Gate*). Solus queries AniList in real time to extract that show's high-ranked community tags, studio, and direct recommendation peers, boosting hidden gems that share its exact DNA into the `65%–99% Match` range.

### 4.AniList Synchronization & Persistent Dismissal Tracking
Whether you link your public AniList username or import a local backup file (`Settings` → `i`), Solus merges your remote lists (`CURRENT`, `PLANNING`, `REPEATING`) into your local SQLite database:
- Removing a show from **Continue Watching** (`x` or `d` on Home) or **Watchlist** (`+`) records a persistent dismissal in the local `dismissed_sync` table.
- Future AniList background syncs and JSON re-imports automatically respect your dismissals so removed shows stay gone until you explicitly re-add them or play a new episode.

---

## ⬡ Prerequisites

Before launching `solus`, ensure the following runtime tools are installed on your `$PATH`:

1. **[`ani-cli`](https://github.com/pystardust/ani-cli)** — Required for stream extraction and player handoff.
2. **A Supported Media Player**:
   - `mpv` (default and recommended)
   - `iina` (macOS native player; automatically used as fallback on macOS)
   - `vlc`
3. **A Truecolor / Kitty-Graphics Terminal** (recommended):
   - [Kitty](https://sw.kovidgoyal.net/kitty/), [Ghostty](https://ghostty.org/), or [WezTerm](https://wezfurlong.org/wezterm/) for full high-resolution poster and episode still rendering.

Verify your environment:

```bash
which ani-cli
which mpv   # or: which iina / which vlc
```

---

## ❖ Installation

### Build & Install from Source (Cargo)

```bash
git clone https://github.com/abdullah-binmadhi/Solus.git
cd Solus
cargo install --path . --force
```

Then launch Solus from anywhere:

```bash
solus
```

### Runtime Dependencies on macOS (Homebrew)

```bash
brew install ani-cli mpv curl grep aria2 ffmpeg git fzf yt-dlp
# Optional macOS player:
brew install --cask iina
```

### Runtime Dependencies on Linux

```bash
# Debian / Ubuntu
sudo apt install mpv vlc curl grep aria2 ffmpeg fzf yt-dlp

# Fedora
sudo dnf install mpv vlc curl grep aria2 ffmpeg fzf yt-dlp

# Arch Linux
sudo pacman -S mpv vlc curl grep aria2 ffmpeg fzf yt-dlp
```

---

## ◈ Keyboard Controls

### Home Screen
| Key | Action |
| :--- | :--- |
| `j` / `k` or `↑` / `↓` | Move between category rows (*Continue Watching*, *Watchlist*, *Trending*, etc.) |
| `h` / `l` or `←` / `→` | Scroll anime cards left / right |
| `Enter` | Open the **Detail & Episode Inspector** screen for the highlighted anime |
| `r` | Immediately resume the next unwatched episode of the highlighted anime |
| `x` / `d` / `Del` | Remove highlighted anime from *Continue Watching* or *Watchlist* |
| `t` | Open **The Tailored Crucible** (personalized recommendation engine) |
| `/` | Open instant local + AniList catalogue search |
| `s` | Open **Settings & AniList Sync / JSON Importer** |
| `!` | Open runtime dependency & player diagnostics |
| `?` | Open keyboard shortcuts help overlay |
| `Shift+R` | Force-refresh Home rows from AniList |
| `q` / `Esc` | Quit Solus |

### Detail Screen & Split-Deck Episode Inspector
| Key | Action |
| :--- | :--- |
| `h` / `l` or `↑` / `↓` | Navigate across episodes |
| `v` | Toggle between **Split-Deck View** (default) and **Horizontal List View** |
| `Enter` | Play the selected episode (or execute the focused top-bar action) |
| `o` | Open the **Stream & Subtitle Matrix** (`1080p`/`720p` + multi-language subtitles) |
| `w` | Toggle watched / unwatched status for the selected episode |
| `+` | Add or remove the anime from your **Watchlist** |
| `n` | Play the next unwatched episode immediately |
| `d` | Toggle audio mode (`SUB` ↔ `DUB`) for this anime |
| `m` | Remap the `ani-cli` provider search query if a title needs manual matching |
| `Tab` | Cycle focus between *Top Actions*, *Episodes*, and *More Like This* |
| `Esc` / `q` | Return to the Home screen |

### The Tailored Crucible (`t`)
| Key | Action |
| :--- | :--- |
| `j` / `k` or `↑` / `↓` | Navigate quiz options or recommended anime results |
| `Space` | Toggle multiple choices during the **6-Question Refinement Crucible (`f`)** |
| `Enter` | Confirm selection / lock in multi-select choices / open selected recommendation |
| `f` | Launch the **6-Question Refinement Crucible** (with live Anchor Anime Search) |
| `r` | Retake the 10-question Soul Resonance Quiz from the beginning |
| `Esc` | Return to the previous screen |

---

## ⬡ Configuration

Solus automatically generates its configuration file and SQLite database on first launch:

- **macOS**: `~/Library/Application Support/ani-tui/config.toml`
- **Linux**: `~/.config/ani-tui/config.toml`

```toml
quality          = "best"      # best | 1080p | 720p | 480p | 360p
audio_mode       = "sub"       # sub | dub
player           = "mpv"       # mpv | iina | vlc
anilist_username = ""          # Optional: public AniList username for automatic sync

[cache]
trending_ttl = 86400           # 24 hours
stable_ttl   = 604800          # 7 days
```

---

## ❖ Project Architecture

```text
src/
├── api/
│   ├── anilist.rs       # AniList GraphQL client (catalogues, tags, user lists, streaming episodes)
│   ├── kitsu.rs         # Multi-tier episode hydration (api.ani.zip / TVDB + Kitsu fallback)
│   ├── player.rs        # Detached ani-cli process spawner & custom player wrapper
│   └── subtitles.rs     # M3U8 resolution parser & multi-language subtitle extractor
├── db/
│   ├── mod.rs           # SQLite connection pool & idempotent schema migrations
│   ├── cache.rs         # Anime metadata, cover blobs, and category cache
│   ├── user.rs          # Watch history, watchlist, per-show overrides, and dismissed_sync
│   └── sync.rs          # TTL cache staleness tracking
├── services/
│   ├── importer.rs      # Zero-token AniList JSON & MAL XML local backup importer
│   ├── sync.rs          # Background AniList sync & heuristic recommendation rows
│   └── tailored.rs      # 10-Question Soul Resonance Quiz + 6-Question Refinement Crucible
├── ui/
│   ├── home.rs          # Curated Home theatre rows & hero banner
│   ├── detail.rs        # Split-Deck Episode Inspector, List View & Genre-Attuned Synopsis Engine
│   ├── tailored.rs      # Interactive quiz, multi-select refinement & Anchor Anime search UI
│   ├── stream_options.rs# Pre-flight resolution & subtitle selector modal
│   ├── playback.rs      # Live playback telemetry & error recovery screen
│   ├── settings.rs      # Preferences, AniList username sync & file importer overlay
│   ├── search.rs        # Instant local + network search overlay
│   ├── setup.rs         # Dependency verification overlay
│   ├── help.rs          # Keybinding reference & toast notifications
│   ├── theme.rs         # Solus crimson/obsidian/gold palette
│   └── components/
│       └── cover.rs     # Kitty Graphics Protocol & halfblock cover renderer
├── state/mod.rs         # Central application state & screen navigation
├── config.rs            # TOML configuration persistence
└── main.rs              # Async Tokio event loop, image scaling & background worker orchestration
```

---

## ✦ Development & Testing

Run the full unit and integration test suite (136 tests covering AniList/AniZip/Kitsu deserialization, SQLite migrations, `dismissed_sync` persistence, Tailored Crucible scoring, cover image scaling, and genre-attuned synopsis generation):

```bash
cargo check
cargo test
```

---

## ❖ License & Credits

- Licensed under the **[MIT License](LICENSE)**.
- Metadata powered by the **[AniList GraphQL API](https://anilist.co)**, **[ani.zip](https://api.ani.zip)**, and **[Kitsu](https://kitsu.app)**.
- Playback handoff powered by **[`ani-cli`](https://github.com/pystardust/ani-cli)**.
