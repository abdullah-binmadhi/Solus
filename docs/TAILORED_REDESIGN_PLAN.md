# SOLUS — Tailored Algorithm & Interactive Results Tuner Redesign Plan
**Target Files:** `src/services/tailored.rs`, `src/ui/tailored.rs`, `src/main.rs`  
**Goals:**
1. Expand recommendations from **15 to 30 suggestions**.
2. Fix the **Wizard Recommendation Algorithm** so suggestions are genuinely accurate to your answers (no more *Black Clover Season 2*, *One Piece*, or *Dragon Ball Super* at `99% MATCH` when you pick **Cozy Slice-of-Life**!).
3. Replace the tiring **6-Question `[f] Refine Results` Popup** with a **zero-question, non-intimidating Interactive Live Tuner & One-Key Feedback System**.

---

## 1. Root-Cause Audit: Why Your Screenshot Showed Inaccurate Results

In your screenshot, your profile was **`COZY SLICE-OF-LIFE FAN`**, yet `#1` was *Black Clover*, `#2` was *One Piece*, `#5` was *The World's Strongest*, `#9` was *Dragon Ball Super*, and `#3` / `#11` were *Season 2* sequels.

Here are the **5 exact bugs** in `src/services/tailored.rs` causing this:

1. **Hidden `Action` Pollution in Questions 4, 5, 6, and 9:**
   - Even when you choose **Q1: Cozy & Heartwarming** (`Slice of Life`, `Iyashikei`, `Comedy`), Question 5 Option 0 (*Combat*), Question 6 Option 1 (*Balanced*), and Question 9 Option 0 (*Popcorn Fun*) all secretly add `+35 Action`, `+20 Action`, and `+25 Action` + `+20 Comedy`.
   - As a result, any battle shounen with `Action` + `Comedy` (*One Piece*, *Black Clover*) accumulates **+125 points** from secondary questions and outscores actual *Slice of Life* anime!
2. **`tags_neg` on Questions 1–9 Are Completely Ignored:**
   - `score_anime()` only checks `tags_neg` on Question 10 (`deal_breakers`). The negative tags defined on Questions 1–9 (and opposing mood genres like `Action` / `Battle Shounen` when `Cozy` is chosen) are never subtracted or filtered.
3. **No Primary Mood & Setting Coherence Gate:**
   - A show that has **zero** overlap with your **Q1 Mood** (e.g., has neither `Slice of Life` nor `Iyashikei`) can still hit `99% MATCH` purely from secondary tags + AniList score bonus.
4. **Sequel Seasons (`Season 2`, `2nd Season`, `Part 2`) Leak Into Fresh Discovery:**
   - Shows like *Black Clover Season...*, *The Ramparts of Ice Season 2*, and *Aoashi Season 2* appear even when you haven't watched Season 1.
5. **Flat `score / 150.0` Clamping Makes Everything `99% MATCH`:**
   - Base score starts at `40` + quality bonus `~30` = `70`, so matching just 2 tags pushes the score past `150`, clamping the top 6 shows to identical `[99% MATCH]` badges.

---

## 2. Algorithm Upgrade Architecture (30 Accurate Suggestions)

### A. Hierarchical Weighting & Primary Mood Gate
Instead of summing flat tag weights across all questions equally:
- **Tier 1 — Core Mood & Tone (Q1 + Q6): 50% of total match weight**
  - Must match at least one primary genre/tag from Q1 Mood to exceed `82%` match.
  - **Cross-Mood Clash Penalty:**
    - If Q1 is **Cozy & Heartwarming**, shows tagged `Action`, `Shounen`, `Battle`, `Gore`, `Horror`, or `Psychological` without `Slice of Life` / `Iyashikei` receive a **`-45 pt` clash penalty**.
    - If Q1 is **Dark, Bleak & Gritty**, pure `Iyashikei` / `Cute Girls Doing Cute Things` receive a **`-45 pt` clash penalty**.
    - If Q1 is **Deep & Mind-Bending**, generic low-plot battle comedies without `Mystery` / `Psychological` / `Thriller` / `Sci-Fi` are penalized.
- **Tier 2 — World Setting & Conflict (Q3 + Q5 + Q9): 30% of total match weight**
  - Contextualized by Q1 so choosing *"Direct & Entertaining"* in Q9 while in *Cozy* mode boosts `Slice of Life` / `Comedy` / `Iyashikei` instead of `Action`!
- **Tier 3 — Protagonist & Format Fit (Q2 + Q4 + Q7 + Q8): 20% of total match weight**
  - Enforces `tags_neg` across **all 10 questions** (not just Q10).

### B. Automatic Sequel Guard
- Any candidate whose English or Romaji title contains sequel markers (`Season 2`, `Season 3`, `Season 4`, `2nd Season`, `3rd Season`, `Part 2`, `Cour 2`, `II`, `III`) is **automatically excluded** unless Season 1 is already in your watched history.

### C. True Differentiated Match Percentage (`75%`–`98%`)
- Computes a true weighted ratio (`matched_weight / max_possible_weight`) blended with Bayesian community rating (`score / 100.0`), producing natural, honest spread (e.g., `#1 [97% MATCH]`, `#2 [94% MATCH]`, `#3 [92% MATCH]`, ..., `#30 [78% MATCH]`).
- Returns **30 suggestions** (`matches.truncate(30)`), and increases AniList candidate pool fetching (`perPage: 50` across primary + secondary tag queries) so 30 high-accuracy candidates are always available.

---

## 3. Redesigning `[f] Refine Results` — Zero Questions, Zero Fatigue

Instead of opening a 6-step questionnaire modal when you press `[f]`, we replace it with a **Live Interactive Tuner Drawer** + **Direct List Feedback Keys** right on the 30-results screen.

### How It Works (Non-Intimidating & Instant):
1. **Direct 1-Key Actions While Browsing the 30 Results (No Modal Needed!):**
   - **`[m]` More Like This:** Press `m` on any anime you like in the list (e.g., *The Ramparts of Ice*). Solus immediately locks onto that show's genre/theme DNA, fetches AniList community peer recommendations for it, and re-ranks all 30 results around it in real time!
   - **`[x]` Dismiss / Less Like This:** Press `x` on any anime you don't want (e.g., *Black Clover*). It vanishes from your 30 results immediately and down-weights its non-core tags.

2. **Pressing `[f]` Opens the Compact "Live Vibe Tuner" Bar (Top Dock — Zero Questions!):**
   - Instead of asking you 6 pages of questions, pressing `[f]` expands a sleek **3-row interactive pill dock** at the top of the Results screen.
   - You move with `h/l` and `j/k` and tap `Space`/`Enter` to toggle pills — **watching the 30 results below update live on every single keypress!**
   - **Row 1 — Discovery Tier:**
     `[ ● All Picks ]   [ ○ Hidden Gems (Under-the-Radar) ]   [ ○ Cult Classics (82%+) ]   [ ○ New & Modern (2022+) ]`
   - **Row 2 — Live Genre Boost / Hide Chips (Auto-extracted from the results):**
     `[ + Slice of Life ]  [ + Iyashikei ]  [ ○ Romance ]  [ ○ Comedy ]  [ ○ Drama ]  [ - Action ]  [ - Shounen ]`
     *(Tap `Space` to cycle a tag: `○ Neutral` → `+ Must Have / Boost` (Green) → `- Hide Tag` (Red))*
   - **Row 3 — Quick Toggles:**
     `[󰄬 Hide Sequels (S2+)]   [○ Finished Only]   [○ Short 1-Cour (≤13 eps)]   [c Clear Tuners]`

---

## 4. ASCII Wireframe Sketch of the Redesigned 30-Match Screen + Live Tuner

```text
╭─ 󰋽 TAILORED RECOMMENDATIONS • 30 MATCHES ────────────────────────────────────────────────────────────────────────────────────────────────╮
│  Taste Profile:  COZY SLICE-OF-LIFE FAN  ·  Drawn to relaxing pacing, gentle atmospheres, and heartwarming comfort                       │
├─ 󰒓 LIVE VIBE TUNER [Active — h/l/j/k to select, Space to toggle, f/Esc to close] ────────────────────────────────────────────────────────┤
│  Discovery:   [ ● All Picks ]   [ ○ Hidden Gems ]   [ ○ Acclaimed 82%+ ]   [ ○ Modern 2022+ ]   [󰄬 Hide Sequels]   [ ○ Finished Only ]   │
│  Tune Tags:   [ + Slice of Life ]   [ + Iyashikei ]   [ ○ Comedy ]   [ ○ Romance ]   [ ○ School ]   [ - Action ]   [ - Shounen ]         │
├──────────────────────────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────┤
│   1.  [97% MATCH]  Frieren: Beyond Journey's End         │  ╭──────────────────╮  Frieren: Beyond Journey's End                          │
│         Slice of Life   Adventure   Fantasy              │  │                  │    Sousou no Frieren                                    │
│                                                          │  │                  │                                                         │
│   2.  [95% MATCH]  Barakamon                             │  │   POSTER ART     │   [97% TASTE MATCH]    9.3   28 eps · TV · FINISHED   │
│         Slice of Life   Iyashikei   Comedy               │  │    (18 x 14)     │                                                         │
│                                                          │  │                  │  Genres: [Slice of Life] · [Adventure] · [Fantasy]      │
│   3.  [94% MATCH]  Yuru Camp (Laid-Back Camp)            │  │                  │                                                         │
│         Slice of Life   Iyashikei   Outdoors             │  │                  │  󰄬 Profile Fit: Matches Cozy & Heartwarming + Comfort   │
│                                                          │  ╰──────────────────╯  󰄬 Sequel Guard: Standalone / Season 1 Entry            │
│   4.  [92% MATCH]  Mushishi                              ├────────────────────────── ── STORY SYNOPSIS & OVERVIEW ── ────────────────────┤
│         Slice of Life   Iyashikei   Supernatural         │  Gentle warmth and lighthearted moments discovered in everyday life.          │
│                                                          │                                                                               │
│   5.  [91% MATCH]  Bocchi the Rock!                      │  After a ten-year quest alongside her party, the elven mage Frieren           │
│         Slice of Life   Comedy   Music                   │  bids farewell to her comrades as she embarks on a quiet journey to           │
│                                                          │  understand human mortality, cherishing the small, peaceful moments…          │
│   6.  [89% MATCH]  Skip and Loafer                       │                                                                               │
│         Slice of Life   Romance   School                 │                                                                               │
│                                                          │                                                                               │
│   7.  [88% MATCH]  Natsume's Book of Friends             │                                                                               │
│         Slice of Life   Iyashikei   Supernatural         │                                                                               │
│   ... (Scrolls smoothly through 1..30)                   │                                                                               │
├──────────────────────────────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────┤
│  [f] Live Tuner   [m] More Like This   [x] Dismiss Show   [Enter] Play   [d] Details   [+] Watchlist   [r] Profile   [Esc] Home          │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

---

## 5. Implementation Steps

1. **Step 1 — Overhaul `score_anime` & Candidate Generation (`src/services/tailored.rs`):**
   - Increase output cap from `15` to **`30`** (`matches.truncate(30)`) in both `generate_tailored_matches_advanced` and `generate_tailored_matches`.
   - Fix Question 5, 6, and 9 tag weights so they don't inject raw `Action` into non-Action moods.
   - Enforce `tags_neg` across all 10 questions and add the **Primary Mood Coherence Gate** + **Cross-Mood Clash Penalty** (so *Black Clover*, *One Piece*, and *Dragon Ball Super* are completely filtered out of *Cozy Slice-of-Life*).
   - Add `is_sequel_season(&Anime)` filter so `Season 2` / `2nd Season` / `Part 2` entries are excluded unless watched.
   - Replace flat `score / 150.0` clamping with normalized weighted ratio scoring so the 30 results have genuine, gradual percentage scores (`97%`, `95%`, `93%` ... `78%`).

2. **Step 2 — Replace the 6-Question Refinement Modal with the Live Vibe Tuner (`src/state/mod.rs`, `src/ui/tailored.rs`, `src/main.rs`):**
   - Replace the 6-step questionnaire state with lightweight live tuner controls:
     - `tuner_open: bool` (toggled with `[f]`)
     - `tuner_row: usize` (`0` = Discovery & Scope row, `1` = Dynamic Genre/Tag pills row)
     - `tuner_col: usize`
     - `tuner_discovery_tier: usize` (`0` = All, `1` = Hidden Gems, `2` = Acclaimed 82%+, `3` = Modern 2022+)
     - `tuner_hide_sequels: bool` (default `true`)
     - `tuner_finished_only: bool`
     - `tuner_boosted_tags: HashSet<String>` (`+` green boost)
     - `tuner_muted_tags: HashSet<String>` (`-` red exclusion)
     - `tuner_dismissed_ids: HashSet<i64>` (`[x]` instant dismiss)
     - `tuner_anchor_anime: Option<(i64, String)>` (`[m]` "More Like This" on any highlighted result)
   - Store the base pool of scored candidates in `state.tailored_pool: Vec<TailoredMatch>` so toggling any pill in the Live Tuner or pressing `[x]` / `[m]` **instantly re-filters and re-sorts the 30 visible results in `<1ms` with zero loading screen** (and when `[m]` is pressed, also fetches AniList peer recommendations in the background and merges them into the pool seamlessly!).
