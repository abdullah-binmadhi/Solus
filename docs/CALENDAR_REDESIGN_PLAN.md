# SOLUS — Seasonal & Movie Release Calendar (`[c]`) Design Plan
**Role:** Apple Principal UI/UX Architect  
**Target Files:** `src/ui/calendar.rs` (new), `src/services/calendar.rs` (new), `src/api/anilist.rs`, `src/state/mod.rs`, `src/ui/home.rs`, `src/ui/settings.rs`, `src/main.rs`  
**Shortcut:** **`[c]`** from the Home Screen (and accessible from the `[s]` Command Hub)

---

## 1. Design Philosophy: Why Most Anime Calendars Fail (And How We Fix It)

Most anime schedule views cram 7 vertical columns onto one screen, truncating every title to `"Sousou no..."` and drowning the user in 90 tiny text boxes with no artwork, no synopses, and no connection to their personal watchlist.

Following **Apple Human Interface Principles** (*Clarity, Deference, Depth*), the Solus Release Calendar is designed around three core ideas:

1. **Zero Clutter — The 7-Day Segmented Ribbon + Split Stage:**
   - Instead of 7 squished columns, the top features a tactile **7-Day Segmented Day Bar (`Mon` – `Sun`)** with live episode counts and a **`●` dot indicator** on days when shows from your *Continue Watching / Watchlist* air.
   - Below the ribbon, the screen splits cleanly into a **Chronological Daily Release Timeline (Left)** and an **Apple TV+ Episode & Poster Inspector Card (Right)**.
2. **Time-Machine Week Navigation (`Prev Week ‹` · `This Week` · `› Next Week`):**
   - Seamlessly step backward (`[` or `H`) to see episodes and movies you missed last week, or step forward (`]` or `L`) into upcoming weeks to see future episode drops and theatrical/movie releases.
   - Press **`[t]` (Today)** at any time to snap straight back to **This Week + Today's Releases**.
3. **Personal Context & Instant Actionability:**
   - Shows you are already watching or have in your Watchlist are **pinned to the top of each day** with a `★ TRACKED` badge and show whether you've already watched that episode (`✓ Watched`) or are behind (`1 ep behind`).
   - Press **`[Enter]`** on any aired episode in the calendar to **stream that exact episode immediately**, or press **`[+]`** to add a newly discovered seasonal show to your Watchlist without leaving the calendar.

---

## 2. ASCII Wireframe Sketch (Apple TV+ / Fantastical Inspired Popup)

```text
╭─ 󰃭 RELEASE CALENDAR · SEASONAL ANIME & MOVIES ────────────────────────────────────────────────────────────────────────────────────────────╮
│  [‹ Prev Week (H / [)]    NOV 9 – NOV 15, 2026  (THIS WEEK)    [Next Week (L / ]) ›]         Filter [f]: [ ● All ] [ ○ Tracked ] [ ○ Movies ] │
├───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│   MON 09 (6)     TUE 10 ● (8)     WED 11 (7)     THU 12 ● (9)     FRI 13 (11)    ╭─ TODAY · SAT 14 ● (14) ─╮    SUN 15 ● (10)             │
├────────────────────────────────────────────────────────────────────┬─────────────┴─────────────────────────┴──────────────────────────────┤
│  ★ YOUR TRACKED RELEASES TODAY (2)                                 │  ╭──────────────────╮  Frieren: Beyond Journey's End                 │
│  ▸ 18:00  [AIRED ✓]   Frieren: Beyond Journey's End      [EP 10]   │  │                  │    Sousou no Frieren                           │
│    21:30  [IN 4h 48m] Dandadan                           [EP 07]   │  │                  │                                                │
│                                                                    │  │   POSTER ART     │   [EPISODE 10 OF 28]   [󰄬 AIRED · READY]       │
│  ── ALL SEASONAL & MOVIE RELEASES (12) ──────────────────────────  │  │    (18 x 13)     │    9.3   ·   TV   ·   Aired Today at 18:00    │
│    12:00  [AIRED]     Look Back (Theatrical Release)     [MOVIE]   │  │                  │                                                │
│    16:30  [AIRED]     Blue Box (Ao no Hako)              [EP 08]   │  │                  │  󰄬 Status:  In Your Watchlist (Watched Ep 9)   │
│    17:00  [AIRED]     Shangri-La Frontier                [EP 31]   │  │                  │  󰃭 Schedule: Saturdays at 18:00 (Local Time)   │
│    18:30  [IN 1h 48m] Orb: On the Movements of Earth     [EP 07]   │  ╰──────────────────╯  Genres: [Adventure] · [Drama] · [Fantasy]     │
│    19:00  [IN 2h 18m] Ranma ½ (2024)                     [EP 06]   ├────────────────────────── ── SYNOPSIS & RELEASE INFO ── ─────────────┤
│    20:00  [IN 3h 18m] Bleach: Thousand-Year Blood War    [EP 33]   │  Next Unwatched: Episode 10 is available to stream right now.        │
│    22:00  [IN 5h 18m] Blue Lock                          [EP 30]   │                                                                      │
│    23:00  [IN 6h 18m] Solo Leveling: ReAwakening         [MOVIE]   │  After a ten-year quest alongside her party, the elven mage Frieren  │
│    23:30  [IN 6h 48m] Dragon Ball Daima                  [EP 06]   │  bids farewell to her comrades and embarks on a quiet journey to     │
│                                                                    │  understand human mortality and the passage of time…                 │
├────────────────────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────┤
│  [h/l] Day   [j/k] Episode   [H/L or [/]] Prev/Next Week   [t] Today   [f] Filter   [Enter] Play Ep   [+] Watchlist   [d] Details   [Esc] │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

---

## 3. High-Impact Enhancements (Non-Bloated, High Utility)

1. **Smart "Your Tracked Releases" Priority Section:**
   - At the top of each day's timeline, any airing show that is in your **Continue Watching** or **My Watchlist** is automatically grouped under `★ YOUR TRACKED RELEASES` so your personal shows are always the very first items you see on every day.
   - Days on the 7-day ribbon that contain at least one of your tracked shows display a subtle **`●` dot indicator** (`TUE 10 ●`).

2. **Live Status Badges in Your Local Timezone:**
   - Every airing timestamp from AniList (`airingAt` Unix epoch) is converted to your system's **local time** (`HH:MM`).
   - Dynamic status pills make scanning effortless:
     - **`[AIRED ✓]`** (Emerald) — Episode has already aired AND you've watched it in Solus.
     - **`[AIRED]`** (Cyan) — Episode has aired and is ready to stream right now.
     - **`[IN 2h 15m]`** (Amber) — Airing later today with a live countdown.
     - **`[MOVIE]` / `[FINALE]`** (Gold/Magenta) — Special format callouts so theatrical movies, OVAs, and season finales stand out immediately.

3. **3-Mode Non-Intrusive Scope Filter (`[f]`):**
   - Tap **`[f]`** to cycle between:
     1. **`● All Releases`** — All seasonal TV episodes + movies releasing that week (excluding adult/18+ and low-relevance noise).
     2. **`● Tracked Only`** — Turns the calendar into a pure personal simulcast tracker showing *only* shows from your Continue Watching & Watchlist.
     3. **`● Movies & Specials`** — Filters the week to only `MOVIE`, `OVA`, and `SPECIAL` releases.

4. **Multi-Week Time Travel with Instant Week Cache (`[H / L]` or `[` / `]`):**
   - `week_offset: i32` (`0` = This Week, `-1` = Last Week, `+1` = Next Week, etc., spanning `-4..=+4` weeks).
   - Fetched weeks are cached in memory (`HashMap<i32, Vec<CalendarEntry>>`) so flipping back and forth between *Last Week*, *This Week*, and *Next Week* is **instantaneous (`0ms`)**.

5. **Direct Episode Playback (`[Enter]`):**
   - If the highlighted calendar entry has **already aired**, pressing **`[Enter]`** opens that anime and immediately begins streaming **that specific episode number** (`entry.episode`).
   - If the episode airs in the future, pressing **`[Enter]`** opens the anime's Detail view (or plays your next unwatched episode) and informs you when Episode `N` releases.

---

## 4. Ergonomic Keybindings Summary

| Key | Action |
| :--- | :--- |
| **`[c]`** | Open Release Calendar from Home (or close Calendar when open) |
| **`[h]` / `[l]`** or **`[←]` / `[→]`** | Move between days of the week (`Mon` → `Sun`) |
| **`[1]` – `[7]`** | Jump directly to day `1` (Mon) through `7` (Sun) |
| **`[j]` / `[k]`** or **`[↑]` / `[↓]`** | Move up/down through episodes airing on the selected day |
| **`[H]` / `[L]`** or **`[`** / **`]`** | Jump to **Previous Week** / **Upcoming Week** |
| **`[t]`** | Jump back to **Today** (Current Week + Today's Day) |
| **`[f]`** | Cycle filter (`All Releases` → `Tracked Only` → `Movies & Specials`) |
| **`[Enter]`** | Stream this episode (if aired) or open show details (if upcoming) |
| **`[+]`** | Add / remove highlighted anime from **My Watchlist** |
| **`[d]`** | Open full Anime Detail screen |
| **`[Esc]` / `[q]` / `[c]`** | Close Calendar and return to Home |

---

## 5. Technical Implementation Plan

1. **AniList Weekly Schedule Query (`src/api/anilist.rs`):**
   - Add `CalendarItem` struct (`schedule_id`, `airing_at: i64`, `time_until_airing: i64`, `episode: u32`, `anime: Anime`).
   - Add `fetch_weekly_schedule(&self, week_start_unix: i64, week_end_unix: i64, now: i64) -> Result<Vec<CalendarItem>>` querying `Page.airingSchedules(airingAt_greater: $start, airingAt_lesser: $end, sort: [TIME, ID])` (fetching up to 2 pages concurrently so full 7-day schedules are never truncated), filtering out `isAdult: true` and `MUSIC` entries, and upserting discovered `Anime` into the SQLite cache.
2. **Calendar Service & Local Timezone Day Grouping (`src/services/calendar.rs`):**
   - Compute exact Monday `00:00:00` to Sunday `23:59:59` local week bounds for any `week_offset: i32`.
   - Bucket `CalendarItem` entries into `7` day slots (`0 = Mon` .. `6 = Sun`) in local time.
   - Sort each day's entries: Tracked shows (`Continue Watching` + `Watchlist`) pinned first (sorted by airing time), followed by all seasonal & movie releases sorted by airing time (with quality/popularity tie-breaking).
3. **Calendar UI & State (`src/ui/calendar.rs`, `src/state/mod.rs`, `src/main.rs`, `src/ui/home.rs`):**
   - Add `Screen::Calendar` (or modal overlay) with `calendar_week_offset: i32`, `calendar_day_idx: usize` (defaulting to Today's weekday `0..6`), `calendar_cursor: usize`, `calendar_filter: CalendarFilter`, `calendar_cache: HashMap<i32, Vec<CalendarItem>>`, and `is_loading_calendar: bool`.
   - Bind **`[c]`** on `Screen::Home`, add **`[c] Calendar`** to the Home Bottom Action Rail (`src/ui/home.rs`), and register **Release Calendar** in the `[s]` Command Hub (`src/ui/settings.rs`).
   - Add unit tests for week bound math, day bucketing, filter modes, and sorting.
