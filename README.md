# ❖ SOLUS

**The fast, featherlight anime theatre with zero ads, built-in AniList sync, TV casting, mobile remote, and no browser required.**

https://github.com/user-attachments/assets/7f520a34-a56e-4465-b31e-c7046d798322

`Solus` is an anime theatre that lives directly inside your terminal. You get crisp cover art, real episode thumbnails, cinema-grade typography presets, local phone remote companion, direct Smart TV casting, and full AniList integration—all without opening a single browser tab, installing three ad-blockers, or fighting pop-up windows just to click Play.

```text
╔════════════════════════════════════════════════════════════════════════════════════════╗
║  ❖ SOLUS // TERMINAL ANIME THEATRE                                    [t] Discover [?] ║
╠════════════════════════════════════════════════════════════════════════════════════════╣
║  ╭──────────────────╮  ZANKYOU NO TERROR (Terror in Resonance)                         ║
║  │                  │  ★ 81%  ·  TV  ·  FINISHED  ·  11 Episodes  ·  [ SUB ]           ║
║  │   1200 × 1800    │  [ Drama ]  [ Mystery ]  [ Psychological ]  [ Thriller ]         ║
║  │   HD PORTRAIT    │                                                                  ║
║  │    COVER ART     │  In an alternate version of the present, Tokyo has been          ║
║  │                  │  decimated by a shocking terrorist attack, and the only hint     ║
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

Watching anime on the web usually means choosing between a bloated streaming site that eats 3 GB of RAM, or a free site where clicking the unpause button opens four new tabs for crypto scams and questionable browser games.

**Solus fixes all of that by skipping the browser entirely:**

- **No Ads. No Pop-ups. No Ad-Blocker Required.** — When you press `Enter` on an episode, it opens cleanly in your media player (`mpv`, `iina`, or `vlc`). No banners, no redirects, no nonsense.
- **Split-Deck Fuzzy Feature Command Hub (`s`)** — An omni-searchable control palette inspired by Raycast and Neovim Telescope. Search any feature, typography style, or setting in real time with Skim fuzzy matching, preview details on the live inspector card, and trigger actions with `Enter`—no need to memorize cryptic hotkeys.
- **Cinema-Grade Subtitles & Signage Collision Guard** — Four hand-tuned typography presets (`Studio Minimal`, `Netflix Clean`, `Anime Fansub`, `OLED Plate`) with custom stroke outlines, drop shadows, and pill backdrops. Dialogue that collides with translated Japanese signs is automatically lifted upward in real time so you never miss a beat.
- **Zero-Setup Local LAN Mobile Remote Companion (`m`)** — Scan an in-terminal ASCII QR code from your smartphone to launch an embedded OLED touch remote on your local Wi-Fi. Scrub through episodes, play/pause, adjust volume, skip intro/outro, and switch sub/dub right from your couch.
- **Native AirPlay & Smart TV Direct Streamer (`c`)** — Scan your local Wi-Fi for Apple TV, LG webOS, Samsung Tizen, and Sony Bravia TVs. Beam decrypted 1080p/4K master streams directly to television hardware decoders with a full-screen TV Command Deck.
- **Real-Time Anime4K Upscaling & Adaptive Ambilight** — Direct GPU GLSL shader upscaling reconstructs crisp line-art in real time, while cover-adaptive Ambilight extracts vibrant color histograms from anime posters to softly tint terminal borders.
- **Watch in Your Language** — Flip between `SUB` and `DUB` with one key (`d`), or press `o` to choose your video quality (`1080p`, `720p`, `480p`) and subtitle language—including **English, Arabic, Spanish, French, German, Russian, Portuguese, Italian**, and more.
- **Seamless AniList Compatibility** — Connect your AniList account by username or drop in your saved JSON backup file (`s` → AniList). Your **Continue Watching** and **Watchlist** stay in sync, and dropped shows stay gone.
- **Fluid Background Auto-Updater (`u`)** — Zero-lag startup update checks throttled with a 4-hour SQLite cache, atomic in-place binary swapping, and multi-architecture binary distribution.
- **Discovery That Actually Knows Your Taste (`w`)** — Tired of scrolling for an hour? Take an interactive taste quiz, press `f` to narrow down the exact vibe you want, and search by an anime you already love (*Death Note*, *Monster*, *Steins;Gate*). Solus curates personalized gems instead of generic top-10 lists.
- **Anime Wrapped & Viewing Telemetry (`a`)** — Press `a` to view your total watch time, completed episodes, favorite studio affinity radar, and seasonal pacing graphs rendered in high-density Braille charts.

---

## ❖ Simple Step-by-Step Installation

> [!TIP]
> **Recommended Window Size**: For the best theatre experience, run your terminal **Maximized / Full Screen**, or resize it to at least **125 columns wide × 38 rows tall**. This gives high-definition posters and widescreen episode stills room to shine without any cramped text.

---

### ❖ macOS Setup (Kitty Terminal)

#### Step 1: Install Kitty Terminal & Media Player
Open your regular Mac Terminal app and paste:
```bash
brew install --cask kitty && brew install mpv ani-cli
```
*(If you do not have Homebrew installed yet, copy-paste this first: `/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"`)*

#### Step 2: Install Solus
In the same terminal, paste:
```bash
cargo install --git https://github.com/abdullah-binmadhi/Solus.git
```
*(Need Rust? Install it with one paste: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)*

#### Step 3: Launch
Open your newly installed **Kitty** app from Applications and type:
```bash
solus
```

---

### ◈ Linux Setup (Kitty Terminal)

#### Step 1: Install Kitty & Media Player
Open your terminal and paste the command for your distribution:

**Ubuntu / Debian / Mint**:
```bash
sudo apt update && sudo apt install -y kitty mpv ffmpeg curl git fzf yt-dlp
git clone https://github.com/pystardust/ani-cli.git && sudo cp ani-cli/ani-cli /usr/local/bin/
```

**Arch Linux / Manjaro**:
```bash
sudo pacman -S kitty mpv ani-cli ffmpeg yt-dlp
```

**Fedora**:
```bash
sudo dnf install kitty mpv ffmpeg curl git fzf yt-dlp
git clone https://github.com/pystardust/ani-cli.git && sudo cp ani-cli/ani-cli /usr/local/bin/
```

#### Step 2: Install Solus
```bash
cargo install --git https://github.com/abdullah-binmadhi/Solus.git
```
*(Need Rust? Install it with: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)*

#### Step 3: Launch
Open **Kitty** and type:
```bash
solus
```

---

### ✦ Windows Setup (WezTerm)

On Windows, **WezTerm** brings full picture support so anime covers and episode preview stills look clean and sharp right out of the box.

#### Step 1: Install WezTerm, Video Player & Tools
Open **PowerShell** (search "PowerShell" in your Windows Start Menu) and paste this single setup command:
```powershell
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
iwr -useb get.scoop.sh | iex
scoop bucket add extras
scoop install wezterm mpv ani-cli git yt-dlp
```

#### Step 2: Install Solus
In the same PowerShell window, paste:
```powershell
cargo install --git https://github.com/abdullah-binmadhi/Solus.git
```
*(Need Rust on Windows? Install it with: `winget install Rustlang.Rustup`)*

#### Step 3: Launch
Open **WezTerm** from your Start Menu and type:
```bash
solus
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
| `w` | Open **For You // Tailored** (interactive recommendation engine) |
| `a` | Open **Anime Wrapped & Taste Analytics** (watch time, genres, telemetry) |
| `m` / `M` | Display **Mobile Touch Remote ASCII QR Code Modal** |
| `s` | Open **Split-Deck Fuzzy Feature Command Hub** |
| `/` | Open instant fuzzy title search |
| `!` | Open Dependency Health Doctor (validates mpv, ani-cli, ffmpeg) |
| `u` | Check for Solus releases in the background |
| `U` | Trigger immediate auto-update and restart |
| `Shift+R` | Refresh Home rows from AniList |
| `?` | Open keyboard shortcuts help modal |
| `q` / `Esc` | Quit Solus |

### Detail Screen & Split-Deck Episode Inspector
| Key | Action |
| :--- | :--- |
| `j` / `k` or `↑` / `↓` | Navigate across episode entries |
| `Enter` / `Space` | Play the selected episode |
| `Tab` / `p` | Toggle streaming provider between **ani-cli** and **AllAnime** |
| `c` / `C` | Open **AirPlay / DLNA TV Device Picker** to beam episode to Smart TV |
| `m` / `M` | Display **Mobile Touch Remote ASCII QR Code Modal** |
| `v` | Switch between **Split-Deck Inspector** (default) and **Horizontal List View** |
| `o` | Open **Stream & Subtitle Options** (`1080p`/`720p` + subtitle languages) |
| `w` | Mark the selected episode watched / unwatched |
| `+` | Add or remove the anime from your **Watchlist** |
| `n` | Play the next unwatched episode immediately |
| `d` | Switch audio between `SUB` and `DUB` |
| `s` | Open **Split-Deck Fuzzy Feature Command Hub** |
| `Esc` / `q` | Return to Home screen |

### Split-Deck Feature Command Hub (`s`)
| Key | Action |
| :--- | :--- |
| `Any typing` | Filter features, presets, and actions in real time using fuzzy scoring |
| `j` / `k` or `↑` / `↓` | Move selection pointer across filtered commands |
| `Ctrl+n` / `Ctrl+p` | Next / previous command item |
| `Enter` | Trigger the highlighted action (cycle preset, toggle option, launch modal, check updates) |
| `h` / `l` or `←` / `→` | Cycle option forward / backward on cycling rows |
| `Backspace` | Delete query characters |
| `Esc` | Clear query or exit Hub back to previous screen |

### Smart TV Command Deck (`c`)
| Key | Action |
| :--- | :--- |
| `Space` | Toggle TV playback (Play / Pause) |
| `h` / `l` or `←` / `→` | Seek ±10 seconds on the television |
| `q` / `Esc` | Stop casting and return to Detail screen |

### Tailored Discovery (`w`)
| Key | Action |
| :--- | :--- |
| `j` / `k` or `↑` / `↓` | Move through quiz choices or recommended anime |
| `Space` | Select multiple answers during the **6-Question Refinement (`f`)** |
| `Enter` | Confirm your choices / open the selected anime |
| `f` | Refine results with deeper mood questions + **Anchor Anime Search** |
| `r` | Retake the taste quiz from the start |
| `Esc` | Back to Home |

---

## ⬡ Configuration

Solus creates and synchronizes its configuration automatically at `~/.config/solus/config.toml` (with cross-platform fallbacks for macOS Application Support and Windows AppData):

```toml
# Solus Theatre Configuration (~/.config/solus/config.toml)
player            = "mpv"       # mpv | iina | vlc
quality           = "best"      # best | 1080p | 720p | 480p | 360p
audio_mode        = "sub"       # sub | dub
anilist_username  = ""          # Optional: AniList public username for auto-sync
anime4k_mode      = "off"       # off | balanced | ultra
ambilight_enabled = true        # Cover-adaptive RGB terminal border glow
auto_skip_intro   = true        # Automated AniSkip OP/ED jumping

[subtitle_style]
preset              = "studio_minimal" # studio_minimal | netflix_clean | anime_fansub | oled_plate | custom
collision_avoidance = true             # Real-time dialogue lifting over translated signage

[remote]
enabled = true                  # Background Tokio HTTP daemon on local LAN
port    = 8484                  # Touch client accessible at http://<LAN_IP>:8484

[cast]
default_protocol = "auto"       # auto | airplay | dlna

[update]
check_on_startup = true         # Background GitHub release checker
auto_install     = false        # Atomic binary replacement on new versions

[cache]
trending_ttl = 86400            # 24 hours
stable_ttl   = 604800           # 7 days
```

---

## ❖ Architecture & Ecosystem

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          ❖ SOLUS RUNTIME CORE                          │
├───────────────────┬─────────────────────┬──────────────────────────────┤
│  Terminal View    │  Playback & Media   │  Network & Protocols         │
├───────────────────┼─────────────────────┼──────────────────────────────┤
│ Ratatui TUI Deck  │ mpv IPC Unix Socket │ AniList GraphQL Client       │
│ Sixels/Kitty Img  │ Direct HLS Streamer │ Local LAN Remote (Tokio 8484)│
│ Fuzzy Skim Hub    │ Subtitle Styler/ASS │ AirPlay & DLNA TV Sender     │
│ Cover Ambilight   │ Anime4K GLSL Engine │ GitHub Release Auto-Updater  │
└───────────────────┴─────────────────────┴──────────────────────────────┘
```

---

## ❖ License & Credits

- Licensed under the **[MIT License](LICENSE)**.
- Metadata powered by **[AniList](https://anilist.co)**, **[ani.zip](https://api.ani.zip)**, and **[Kitsu](https://kitsu.app)**.
- Playback engine powered by **[`ani-cli`](https://github.com/pystardust/ani-cli)** and **[`mpv`](https://mpv.io/)**.
