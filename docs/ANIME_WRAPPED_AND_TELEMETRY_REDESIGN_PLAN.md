# ❖ Solus // Anime Wrapped & Taste Telemetry Redesign Plan

## 1. Current Layout Audit & Dead Space Diagnosis

Based on the telemetry dashboard screenshot (`Screenshot 2026-11-12 at 3.48.58 pm.png`):

| Component | Current Issue | Root Cause | Proposed Solution |
| :--- | :--- | :--- | :--- |
| **Window Frame** | 84% width / 88% height leaves heavy unused black gutters around the screen. | `centered_rect(84, 88)` | Expand to **94% width × 92% height** for an immersive widescreen command deck. |
| **Top Banner** | Left-aligned 2-line plain text with 75% horizontal void on the right. | Single paragraph widget | Dual-column banner: Left = Archetype & Epithet; Right = Cadence Radar (e.g. Night Owl) & Streak. |
| **KPI Stat Cards** | 5 cards with empty vertical margins; plain numbers without secondary context. | Fixed 5-column layout | **6-Card KPI Deck** adding Watch Breakdown (Hours + Minutes), Dub/Sub Ratio, or Active Streak. |
| **Genre Affinity** | 60% horizontal dead space to the right of bars; bars fixed at 16 chars. | Static bar math `(*pct / 100) * 16.0` | **Dynamic Bar Width** scaling to available width + dual-column layout (Top Genres + Tags). |
| **Era Distribution** | Only 4 lines of text in a 10-line tall container (60% vertical dead space). | Allocated 42% height of left column | Combine with Decade Trajectory or merge with Format distribution to free space for Top Titles. |
| **Formats Card** | Only 2 lines (`TV: 2`, `ONA: 1`) in a massive 12-line tall box (83% dead space!). | Allocated 45% height of right column | Replace with **Compact Segmented Ratio Bar** + new **Heavy Rotation / Top Anime** showcase! |
| **Velocity & Sparkline**| Small 3-line sparkline with 2 lines of generic text below. | Small widget constraint | Enriched 14-day velocity graph with day ticks, peak date, binge cadence, and engine telemetry. |

---

## 2. Sketch Comparison & Visual Blueprints

### ❖ Option A: The "High-Density Command Deck" (Recommended)
Maximizes telemetry density, introduces **Top Watched Anime (Heavy Rotation)**, visual **Format Segmented Meter**, and **Habit Cadence Radar**.

```
┌───────────────────────────────── ❖ SOLUS // ANIME WRAPPED & TASTE TELEMETRY ❖ ─────────────────────────────────┐
│ TASTE ARCHETYPE: ✦ Eclectic Polymath                           VIEWING CADENCE: ❖ Night Owl (23:00 - 02:00)    │
│ Leading Affinity: Cyberpunk & Psychological (4.7%)              CURRENT STREAK:  ◈ 2 Days Active • Peak: 10 ep/d│
├──────────────┬──────────────┬──────────────┬──────────────┬────────────────────────┬───────────────────────────┤
│ ❖ WATCH TIME │ ◈ EPISODES   │ ✦ COMPLETED  │ ⬡ COMPLETION │ ★ AVERAGE SCORE        │ 󰕾 AUDIO PREFERENCE        │
│ 5.8h (348m)  │ 16 ep        │ 0 / 3 shows  │ 0% [In-Flight│ 81.3% / 100            │ 100% Japanese Sub         │
├──────────────┴──────────────┴──────────────┴──────────────┴────────────────────────┴───────────────────────────┤
│                                                            │                                                           │
│ ◈ GENRE AFFINITY POLARITY                                  │ ❖ HEAVY ROTATION / TOP WATCHED TITLES                     │
│ Cyberpunk     ████████████████████████░░░░░░░   4.7% (2 ep)│ 1. ❖ Cyberpunk: Edgerunners                               │
│ Denpa         ████████████████████████░░░░░░░   4.7% (2 ep)│    ████████████████████ [10/10 ep • 100%] 3.6h • Trigger  │
│ Drama         ████████████████████████░░░░░░░   4.7% (2 ep)│ 2. ◈ Frieren: Beyond Journey's End                        │
│ Sci-Fi        ████████████████████████░░░░░░░   4.7% (2 ep)│    ████████░░░░░░░░░░░░ [ 4/28 ep •  14%] 1.5h • Madhouse │
│ Urban         ████████████████████████░░░░░░░   4.7% (2 ep)│ 3. ✦ Serial Experiments Lain                              │
│ Action        ████████████░░░░░░░░░░░░░░░░░░░   2.3% (1 ep)│    ████░░░░░░░░░░░░░░░░ [ 2/13 ep •  15%] 0.7h • Triangle │
│ Advertisement ████████████░░░░░░░░░░░░░░░░░░░   2.3% (1 ep)│                                                           │
│ Amnesia       ████████████░░░░░░░░░░░░░░░░░░░   2.3% (1 ep)│ Active Catalog Footprint: 3 Series • 41 Total Released Eps│
├────────────────────────────────────────────────────────────┼───────────────────────────────────────────────────────────┤
│ ❖ GOLDEN ERA & FORMAT DISTRIBUTION                         │ 󰓅 14-DAY VELOCITY & BINGE MATRIX                          │
│ Pre-2000 (Golden Age)  ████████████░░░░░░░░░░░░ 33% (1 tit)│ Peak Velocity: 10 ep/day   14 days ago 󰁔 today            │
│ 2000s (Classic Era)    ░░░░░░░░░░░░░░░░░░░░░░░░  0% (0 tit)│       ▄                                                   │
│ 2010s (Modern Era)     ░░░░░░░░░░░░░░░░░░░░░░░░  0% (0 tit)│       █                                                   │
│ 2020s (Current Era)    ████████████████████████ 67% (2 tit)│     ▄ █                                                   │
│ ────────────────────────────────────────────────────────── │  28 29 30 01 02 03 04 05 06 07 08 09 10 11 (Days)         │
│ FORMAT RATIO:                                              │ ───────────────────────────────────────────────────────── │
│ [ TV 67% ████████████████░░░░░░░░ ONA 33% ──── Movie 0% ]  │ Binge Cadence: High-intensity weekend sprint sessions     │
│ Breakdown: 2 TV Series • 1 Web ONA • 0 Feature Films       │ Hardware Pipeline: Anime4K GPU Shaders • OLED Minimal     │
├────────────────────────────────────────────────────────────┴───────────────────────────────────────────────────────────┤
│ [Esc / q / a] Return to Home    [r] Recalculate Live Telemetry    [e] Export Summary JSON    [s] Settings Hub          │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

### ◈ Option B: The "Spotify Wrapped Storyboard"
Features **Taste Polarities (MBTI-style spectrum gauges)**, **Release Timeline Radar**, and **Top Franchise DNA**.

```
┌───────────────────────────────── ❖ SOLUS // ANIME WRAPPED & TASTE TELEMETRY ❖ ─────────────────────────────────┐
│ TASTE ARCHETYPE: ✦ Eclectic Polymath                    SYNTHESIS: Local SQLite & AniList Verified Metadata    │
├──────────────┬──────────────┬──────────────┬──────────────┬────────────────────────┬───────────────────────────┤
│ ❖ WATCH TIME │ ◈ EPISODES   │ ✦ COMPLETED  │ ⬡ COMPLETION │ ★ AVERAGE SCORE        │ ◈ HIGHEST VELOCITY        │
│ 5.8 Hours    │ 16 Episodes  │ 0 / 3 Shows  │ 0.0%         │ 81.3% Mean Rating      │ 10 Episodes in 24h        │
├──────────────┴──────────────┼──────────────┴──────────────┼────────────────────────┴───────────────────────────┤
│ ◈ TASTE SPECTRUM POLARITIES │ ❖ TOP GENRES & THEMES       │ 󰓅 14-DAY BINGE VELOCITY                            │
│                             │                             │ Peak: 10 ep/day   •   Total 14d: 16 episodes           │
│ [ Umbral Dark vs Light ]    │ Cyberpunk      4.7% ████    │       ▄                                                │
│ Dark / Noir   ████████░░░░  │ Denpa          4.7% ████    │       █                                                │
│                             │ Drama          4.7% ████    │     ▄ █                                                │
│ [ Vintage vs Modern ]       │ Sci-Fi         4.7% ████    │                                                        │
│ Modern 2020s  ██████████░░  │ Urban          4.7% ████    │ Active Queue: 3 In-Flight Titles                   │
│                             │ Action         2.3% ██░░    │ Cadence: Late Night Session Specialist                 │
│ [ Depth vs Kinetic ]        │                             │                                                        │
│ Narrative     █████████░░░  │ ❖ LEADING ERA               │ ❖ TOP ANIME ROTATION                                   │
│                             │ 2020s Contemporary (67%)    │ 1. Cyberpunk: Edgerunners (10 ep • 3.6h)               │
│ Format: 67% TV • 33% ONA    │ Pre-2000 Golden Age (33%)   │ 2. Frieren: Beyond Journey's End (4 ep • 1.5h)         │
├─────────────────────────────┴─────────────────────────────┴────────────────────────────────────────────────────┤
│ [Esc / q / a] Return to Home    [r] Recalculate Live Telemetry    [e] Export Summary JSON    [s] Settings Hub          │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Key Enhancements & New Data Elements

### 1. Zero Dead-Space Geometry
* **Widescreen Modal**: Changed from `84% × 88%` to **`94% × 92%`**, utilizing full terminal real estate without cramped borders.
* **Elastic Progress Bars**: Bar character counts are dynamically computed based on `area.width` instead of fixed 14 or 16 chars:
  ```rust
  let bar_width = (inner_width.saturating_sub(label_width + 18)).clamp(10, 48);
  let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
  ```

### 2. New Element: Heavy Rotation / Top Watched Titles
* Synthesized directly from `history` and `playback_progress` in `src/db/user.rs`:
  ```sql
  SELECT a.id, a.title_english, a.title_romaji, a.episodes,
         COUNT(h.episode) as eps_watched,
         COALESCE(SUM(p.position_secs), COUNT(h.episode) * 1440.0) as watch_secs
  FROM history h
  JOIN anime a ON h.anime_id = a.id
  LEFT JOIN playback_progress p ON p.anime_id = h.anime_id AND p.episode = h.episode
  GROUP BY a.id
  ORDER BY eps_watched DESC, watch_secs DESC
  LIMIT 4
  ```
* Shows show progress bar `[10/10 ep • 100%]`, total watch hours, and studio/creator tags.

### 3. New Element: Viewing Cadence & Habit Radar
* Computes timestamp distribution of `history.watched_at` to detect:
  * **Night Owl** (22:00 - 04:00)
  * **Morning Brew** (06:00 - 11:00)
  * **Afternoon Binger** (12:00 - 18:00)
  * **Prime Time** (18:00 - 22:00)
* Calculates active streak days and highest 24-hour binge peak.

### 4. Compact Format Segmented Meter
* Replaces the oversized 12-line format box with a clean, segmented horizontal bar:
  `[ TV 67% ████████████████░░░░░░░░ ONA 33% ──── Movie 0% ]`
* Reclaims vertical height to display rich Top Titles.

### 5. Taste Spectrum & Polarity Gauges
* **Umbral vs Astral** (Dark / Cyberpunk / Psychological vs Vibrant / Shonen / Sol)
* **Vintage vs Contemporary** (Pre-2010 Cel-Shaded vs Modern 4K Mastered)
* **Narrative Depth** (Deep Philosophical vs Kinetic Pop)

---

## 4. Proposed Implementation Steps

1. **Database Aggregation (`src/db/user.rs`)**:
   - Add `top_shows: Vec<TopWatchedAnime>` to `UserAnalytics`.
   - Add `viewing_cadence: &'static str` and `active_streak_days: usize` to `UserAnalytics`.
2. **Dashboard UI Overhaul (`src/ui/analytics.rs`)**:
   - Update `centered_rect(94, 92)`.
   - Rebalance layout into Archetype Header, 6 KPI Cards, Split Deck (Genres + Era/Formats on Left, Top Shows + 14-Day Velocity on Right), and Footer.
   - Implement elastic dynamic bar rendering.
3. **Verification**:
   - Validate clean compilation, `cargo clippy`, and unit tests.
   - Run in terminal to verify 0 dead space and flawless scaling.
