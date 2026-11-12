# ❖ SOLUS

**The fast, featherlight anime player with zero ads, built-in AniList sync, and no browser required.**

https://github.com/user-attachments/assets/7f520a34-a56e-4465-b31e-c7046d798322

`Solus` is an anime theatre that lives directly inside your terminal. You get crisp cover art, real episode thumbnails, multiple subtitle languages, and full AniList integration—all without opening a single browser tab, installing three ad-blockers, or fighting pop-up windows just to click Play.

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
- **Lighter Than a Single Browser Tab** — While Chrome turns your laptop fans into a jet engine, Solus uses a tiny fraction of your RAM and launches in a blink.
- **Faster & Safer Than Anime Websites** — Everything runs locally on your machine. You browse at keyboard speed without loading heavy web trackers or shady scripts.
- **Watch in Your Language** — Flip between `SUB` and `DUB` with one key (`d`), or press `o` to choose your video quality (`1080p`, `720p`, `480p`) and subtitle language—including **English, Arabic, Spanish, French, German, Russian, Portuguese, Italian**, and more.
- **Seamless AniList Compatibility** — Connect your AniList account by username or drop in your saved JSON backup file (`s` → `i`). Your **Continue Watching** and **Watchlist** stay in sync, and when you remove a show you dropped (`x`), it stays gone.
- **Discovery That Actually Knows Your Taste (`t`)** — Tired of scrolling for an hour? Take a quick multi-choice taste quiz, press `f` to narrow down the exact vibe you want, and type in an anime you already love (like *Death Note* or *Monster*). Solus finds something special instead of handing you the same generic top-10 list.
- **Every Episode Looks & Reads Like a Feature** — Each episode comes with its own widescreen preview thumbnail and a full, spoiler-free synopsis written to match the show's genre—witty and chaotic for comedies, dark and gloomy for thrillers.

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
| `t` | Open **For You // Tailored** (interactive recommendation engine) |
| `/` | Open instant search |
| `s` | Open **Settings & AniList Sync / Backup Importer** |
| `!` | Check player & streaming setup |
| `?` | Open keyboard shortcuts help |
| `Shift+R` | Refresh Home rows from AniList |
| `q` / `Esc` | Quit Solus |

### Detail Screen & Split-Deck Episode Inspector
| Key | Action |
| :--- | :--- |
| `h` / `l` or `↑` / `↓` | Navigate across episodes |
| `v` | Switch between **Split-Deck View** (default) and **Horizontal List View** |
| `Enter` | Play the selected episode |
| `o` | Open **Stream & Subtitle Options** (`1080p`/`720p` + subtitle languages) |
| `w` | Mark the selected episode watched / unwatched |
| `+` | Add or remove the anime from your **Watchlist** |
| `n` | Play the next unwatched episode immediately |
| `d` | Switch audio between `SUB` and `DUB` |
| `m` | Manually adjust the stream search title if a show has an alternate name |
| `Tab` | Switch focus between *Top Buttons*, *Episodes*, and *More Like This* |
| `Esc` / `q` | Back to Home |

### Tailored Discovery (`t`)
| Key | Action |
| :--- | :--- |
| `j` / `k` or `↑` / `↓` | Move through quiz choices or recommended anime |
| `Space` | Select multiple answers during the **6-Question Refinement (`f`)** |
| `Enter` | Confirm your choices / open the selected anime |
| `f` | Refine your results with 6 deeper questions + **Example Anime Search** |
| `r` | Retake the taste quiz from the start |
| `Esc` | Back |

---

## ⬡ Configuration

Solus creates its config file automatically on your first launch:

- **macOS**: `~/Library/Application Support/solus/config.toml`
- **Linux**: `~/.config/solus/config.toml`
- **Windows**: `%APPDATA%\solus\config.toml`

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

## ❖ License & Credits

- Licensed under the **[MIT License](LICENSE)**.
- Metadata powered by **[AniList](https://anilist.co)**, **[ani.zip](https://api.ani.zip)**, and **[Kitsu](https://kitsu.app)**.
- Playback powered by **[`ani-cli`](https://github.com/pystardust/ani-cli)**.
