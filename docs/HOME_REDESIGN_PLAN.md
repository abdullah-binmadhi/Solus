# SOLUS — Home Page UI/UX Redesign Blueprint
**Author:** Principal Frontend UI/UX Architect (Google Design System)  
**Target:** `src/ui/home.rs`, `src/state/mod.rs`, `src/main.rs`  
**Objective:** Transform the Solus Home Screen from a sparse, vertically constrained terminal list into the **most visually striking, spatially optimized Ratatui interface ever built**.

---

## 1. UX Audit of the Current Home Page (What's Wrong in the Screenshot)

Looking closely at the current screenshot, five major UI/UX bottlenecks stand out:

1. **Massive Dead Horizontal Space (The 1-Item Row Problem):**
   - `Continue Watching` has only **1 anime** (*Serial Experiments Lain*), leaving **85% of that 12-line horizontal strip completely blank** (where your wallpaper bleeds through).
   - Meanwhile, you have rich categories (`Trending`, `Popular`, `Top Rated`, `Seasonal`) hidden off-screen below the fold because each sparse row consumes 12 full vertical rows.
2. **The `"Because You Watched..."` Row is Cluttered & Redundant:**
   - It eats an entire 12-line vertical tier just to show 2 cards with generic `"• Shared genres"` labels, pushing `Trending` and `Popular` completely off the screen. **Per your instruction, we are permanently removing this section from the Home Screen.** (Contextual recommendations already live on the Detail screen under *"More Like This"* where they belong).
3. **Cramped, Unstructured Hero Banner:**
   - The top banner uses a tiny `22x10` thumbnail on the left while dumping 20+ genre tags into a huge wrap-around wall of yellow text (`Action · Drama · Fantasy · Mystery · Kaiju...`).
   - Action buttons aren't visible in the screenshot because the genre list + description overflowed the 8-line inner height and clipped the bottom CTA bar!
4. **Lack of Global Navigation & Scroll Context:**
   - There is no top navigation header or category pill bar. You can't tell where you are in the vertical stack (e.g., that `Trending`, `Popular`, `Top Rated`, and `Seasonal` exist below `My Watchlist`), nor can you see horizontal pagination (`1 / 8`) cleanly.
5. **Repetitive Card Chrome:**
   - Cards look uniform without visual hierarchy—no ranking badges (`#1`, `#2`) on Trending/Top Rated, no episode progress bar fill styling, and a clunky `▶ SELECTED` top border text that crowds the cover art.

---

## 2. The Google UI/UX Design Vision: *"Cinematic Bento & Dynamic Shelf"*

To make this the **best Ratatui design ever**, we apply three core Material You / Google TV principles adapted for high-density TUI:

### Pillar A: The "Bento Dock" (Solving Sparse Rows Forever)
When `Continue Watching` has **1 to 3 items**, rendering it as a full-width row wastes 60%–85% of the screen.
- **Smart Bento Split:** When both `Continue Watching` and `My Watchlist` are visible and `Continue Watching` has $\le 2$ items, we **dock them side-by-side in a single horizontal tier** (`Continue Watching` takes a proportional left Bento module, `My Watchlist` flows seamlessly on the right with a crisp vertical separator).
- **Result:** Zero dead black space in the middle of your screen, and **an entire extra category row (`Trending Now`) fits above the fold immediately!**

### Pillar B: The Widescreen "Glassmorphic" Hero Stage
Instead of a cramped 10-line box that clips its own buttons:
- **Top Header Bar integrated into the Hero:** Shows `✦ SOLUS` brand mark, **Interactive Category Pills** (`[ 1·Resume ]  [ 2·Watchlist ]  [ 3·Trending ]  [ 4·Popular ]  [ 5·Top Rated ]  [ 6·Seasonal ]`) so you always see all shelves and can jump with `1-6` or `j/k`, plus live AniList sync status on the far right.
- **3-Zone Hero Layout (Height: 11 rows, Zero Clipping Guaranteed):**
  1. **Left Zone (`28 cols`):** Wider 16:9 Cinema Frame for the cover/banner artwork with a subtle focal border.
  2. **Center Zone (`Min(0)`):**
     - **Row 1:** Category Focus Pill (`🔥 TRENDING #1`) + Studio/Format/Year (`TV · 2013 · 25 EPS · FINISHED`).
     - **Row 2:** **Bold High-Contrast Title** + Native Japanese subtitle in dim italics (`Attack on Titan  ·  進撃の巨人`).
     - **Row 3:** **Curated Genre Pills** (capped at top **4** genres rendered as sleek `[ Action ] [ Drama ] [ Dark Fantasy ]` badges—never a 20-tag text dump!).
     - **Rows 4–5:** Strictly 2-line clamped synopsis with clean ellipsis (`…`) so it **never** pushes buttons off-screen.
     - **Row 7:** **Tactile Action Deck** (`[  Watch E1 ]`  `[ + Watchlist ]`  `[ 󰘳 Details ]`  `[ 󰑐 Trailer/ Wizard ]`).
  3. **Right Zone (`24 cols` — The Telemetry HUD Card):**
     - Dedicated mini-panel on the right side of the Hero showing:
       - **Community Score Gauge:** `★ 8.5 / 10` with visual block meter `████████░░`
       - **Your Progress Gauge:** `0 / 25 eps` (`░░░░░░░░░░ 0%`)
       - **Audio / Shader Status:** `CC SUB/DUB` · `✦ Anime4K`

### Pillar C: Next-Gen Poster Cards & Micro-Telemetry
- **Remove `"Because You Watched"` completely** from `CategoryRow`, `HomeData` rendering, and vertical navigation.
- **Visual Progress Bar on Cards:** Instead of plain text `E2 19:07 [────]`, render a sleek high-contrast unicode progress gauge: `E2 · 19m  ━━━━━─────` with `theme.primary` fill and `theme.divider` track.
- **Rank Badges on Discovery Shelves:** In `Trending` and `Top Rated`, each card's top-left border displays a crisp rank chip (` #1 `, ` #2 `, ` #3 `) and the focused card illuminates with ` ✦ FOCUS ` and a double/glowing border.
- **Scroll Viewport Indicator:** Each row header shows a visual mini-scrollbar on the right: `──  Trending Now ─────────────────────────── ●●●○○ [ 1/20 ] ──`.

---

## 3. Full ASCII Wireframe Sketch (140×42 Terminal Viewport)

Here is how your new Home Screen will look in practice:

```text
 ✦ SOLUS   [1  Resume]  [2 󰋜 Watchlist]  [3  Trending]  [4  Popular]  [5  Top Rated]  [6 󰃭 Seasonal]        󰄬 abdullah3 · ✦ Anime Violet
╭─ ✦ FEATURED SPOTLIGHT ──────────────────────────────────────────────────────────────────────────────────────────┬─ TELEMETRY ────────────╮
│ ╭────────────────────────╮  🔥 TRENDING #1   ·   TV 2013   ·   25 EPS   ·   FINISHED                            │  SCORE    ★ 8.5 / 10   │
│ │                        │  ATTACK ON TITAN  ·  進撃の巨人                                                       │  ████████▌░  Top 1%    │
│ │      16:9 CINEMA       │  [ Action ]  [ Drama ]  [ Dark Fantasy ]  [ Mystery ]                                │                        │
│ │       KEY VISUAL       │                                                                                      │  PROGRESS   0 / 25 EPS │
│ │      (26x7 CELLS)      │  Several hundred years ago, humans were nearly exterminated by giant beings          │  ░░░░░░░░░░  Unwatched │
│ │                        │  called Titans. Survivors walled themselves inside three concentric cities…          │                        │
│ ╰────────────────────────╯  ██  Start E1 ██   [ + Watchlist ]   [ d Details ]   [ w Taste Wizard ]             │  AUDIO    SUB / DUB    │
╰─────────────────────────────────────────────────────────────────────────────────────────────────────────────────┴────────────────────────╯
╭─  Continue Watching (1) ──╮ ╭─ 󰋜 My Watchlist (8) ────────────────────────────────────────────────────────────────────── ●●○○○ [ 1/8 ] ─╮
│ ╭─ ✦ FOCUS ──────────────╮ │ │ ╭────────────────────╮  ╭────────────────────╮  ╭────────────────────╮  ╭────────────────────╮            │
│ │                        │ │ │ │                    │  │                    │  │                    │  │                    │            │
│ │   [Cover Art: Lain]    │ │ │ │  [Made in Abyss]   │  │  [Saga of Tanya]   │  │  [Black Clover]    │  │  [Smoking Behind]  │            │
│ │                        │ │ │ │                    │  │                    │  │                    │  │                    │            │
│ │ Serial Experiments L…  │ │ │ │ Made in Abyss: Me… │  │ Saga of Tanya th…  │  │ Black Clover Sea…  │  │ Smoking Behind t…  │            │
│ │ E2 · 19:07  ━━━━━━──── │ │ │ │ ★ 8.0 · MOVIE      │  │ ★ 8.0 · TV         │  │ ★ 8.8 · TV         │  │ ★ 8.1 · TV         │            │
│ ╰────────────────────────╯ │ │ ╰────────────────────╯  ╰────────────────────╯  ╰────────────────────╯  ╰────────────────────╯            │
╰────────────────────────────╯ ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────╯
╭─  Trending Now (20) ──────────────────────────────────────────────────────────────────────────────────────────────────── ●○○○○ [ 1/20 ] ╮
│ ╭─ #1 ───────────────────╮   ╭─ #2 ───────────────╮  ╭─ #3 ───────────────╮  ╭─ #4 ───────────────╮  ╭─ #5 ───────────────╮              │
│ │                        │   │                    │  │                    │  │                    │  │                    │              │
│ │  [Solo Leveling S2]    │   │   [Frieren: BJE]   │  │   [Dandadan]       │  │   [Orb: Movement]  │  │   [Blue Box]       │              │
│ │                        │   │                    │  │                    │  │                    │  │                    │              │
│ │ Solo Leveling: Arise…  │   │ Frieren: Beyond J… │  │ Dandadan           │  │ Orb: On the Move…  │  │ Blue Box           │              │
│ │ ★ 8.9 · TV · AIRING    │   │ ★ 9.3 · TV         │  │ ★ 8.7 · TV         │  │ ★ 8.6 · TV         │  │ ★ 8.4 · TV         │              │
│ ╰────────────────────────╯   ╰────────────────────╯  ╰────────────────────╯  ╰────────────────────╯  ╰────────────────────╯              │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
  [/] Search  ·  󰋽 [w] Wizard  ·  󰃭 [T] Theme  ·  󰘳 [Enter] Watch  ·   [r] Resume  ·  󰋜 [+] Watchlist  ·  󰒓 [s] Settings  ·   [?] Help
```

---

## 4. Before vs. After Spatial Efficiency Comparison

| Metric | Current Design (Screenshot) | Proposed Redesign |
| :--- | :--- | :--- |
| **Top Navigation Bar** | None (blind vertical scrolling) | **1-line Glass Header** with interactive shelf pills (`1`–`6`) & user badge |
| **Hero Action Buttons** | **Clipped/Hidden** when genres wrap to 2+ lines | **Always visible** (genres capped at 4 pills; synopsis clamped to 2 lines) |
| **Hero Right Area** | Empty purple space + unwrapped text | **Dedicated Telemetry HUD** (Score bar, Progress gauge, Sub/Dub indicator) |
| **1-Item `Continue Watching`** | Wastes **85% of a full row** (dead black space) | **Smart Bento Split:** Docks side-by-side with `My Watchlist` when $\le 2$ items |
| **`Because You Watched`** | Takes 12 vertical lines for 2 weak cards | **Removed completely** — frees 12 full rows for `Trending` & `Popular` |
| **Above-the-Fold Discovery** | 0 discovery rows visible (only Resume + Watchlist + Because You Watched) | **`Trending Now` (and/or `Popular`) immediately visible** on launch! |
| **Card Progress Bar** | ASCII `[────]` | High-contrast Unicode track `━━━━━━────` with dual-tone color |

---

## 5. Technical Implementation Plan

### Step 1: Remove `"Because You Watched"` (`CategoryRow::Recommended`)
- **`src/state/mod.rs`**:
  - Remove `CategoryRow::Recommended` from the `CategoryRow` enum (or bypass it in Home navigation so Home only cycles through: `ContinueWatching` $\leftrightarrow$ `Watchlist` $\leftrightarrow$ `Trending` $\leftrightarrow$ `Popular` $\leftrightarrow$ `TopRated` $\leftrightarrow$ `Seasonal`).
- **`src/main.rs`**:
  - Update `move_home_row()`, `current_home_anime()`, and number-key shortcuts (`1`–`6`) so vertical navigation (`j`/`k` or `Down`/`Up`) smoothly transitions across the 6 core shelves without ever stopping on `Recommended`.
- **`src/ui/home.rs`**:
  - Delete `recommended_label()` and remove the `"recommended"` tuple from `render_rows()`.

### Step 2: Build the Top Navigation & Shelf Switcher Bar (`render_top_bar`)
- **`src/ui/home.rs`**:
  - Allocate `Constraint::Length(1)` at the top of `home::render()`.
  - Render `✦ SOLUS` brand badge on the left, dynamic shelf pills in the center showing all non-empty rows with their active highlight (`theme.primary` background on the currently focused shelf), and user/theme telemetry on the right.

### Step 3: Redesign the Hero Stage (`render_featured`)
- **`src/ui/home.rs`**:
  - Split the Hero (`Constraint::Length(10)`) into 3 horizontal zones:
    1. **Cinema Cover (`Constraint::Length(26)`)**: Wider aspect ratio for key visuals.
    2. **Editorial Metadata (`Constraint::Min(0)`)**:
       - Line 1: Category badge + Format/Season/Status.
       - Line 2: Bold Title + Native Title (`anime.title_native`).
       - Line 3: Up to 4 styled genre pills (`[ Action ]`).
       - Lines 4–5: Word-wrapped description strictly clamped to 2 lines (`max_chars = (width * 2).saturating_sub(6)`).
       - Line 6: Spacer / launch toast.
       - Line 7: High-contrast Pill Action Buttons (`Start/Continue`, `+ Watchlist`, `d Details`, `r Resume`).
    3. **Right Telemetry HUD (`Constraint::Length(26)`)**:
       - Separated by a subtle vertical border (`Borders::LEFT`), displaying a visual 10-block score meter (`████████░░`), watched episode ratio meter, and audio/shader badges.

### Step 4: Implement the Smart Bento Shelf Layout (`render_rows` & `render_bento_pair`)
- **`src/ui/home.rs`**:
  - When rendering visible rows, check if `Continue Watching` and `My Watchlist` are both present and `continue_watching.len() <= 2`.
  - If true, combine them into a **single vertical slot** (`BentoRow::Split`) using `Layout::default().direction(Direction::Horizontal)`:
    - Left container width: `(continue_watching.len() as u16) * (CARD_WIDTH + CARD_GAP) + 4`
    - Right container width: `Constraint::Min(0)` for `My Watchlist`
  - Both containers get a clean rounded outer frame (`BorderType::Rounded`) with active border illumination (`theme.primary` when focused, `theme.border_idle` when unfocused) and a dot-pagination indicator (`●●○○○ [ 1/8 ]`) in the top-right title rail.
  - Navigating `Down` (`j`) or `Right` (`l` at the end of `Continue Watching`) seamlessly shifts focus from `Continue Watching` $\rightarrow$ `My Watchlist` $\rightarrow$ `Trending`.

### Step 5: Upgrade Card Chrome (`render_card`)
- **`src/ui/home.rs`**:
  - Add optional rank badge (`#1`, `#2`, etc.) on `Trending` and `Top Rated` cards.
  - Replace `[────]` in progress labels with a colorized `Line` containing `━` (filled, `theme.primary`) and `─` (unfilled, `theme.divider`).
  - Refine selected card state: glowing `theme.border_focus` rounded border, `✦ FOCUS` top badge, and high-contrast white title without ugly underline clutter.
