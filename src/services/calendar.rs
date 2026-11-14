//! Seasonal Anime & Movie Release Calendar service.
//! Handles local timezone conversion, week window boundaries, episode/movie bucketing,
//! and personal watchlist/history status tracking.

use crate::api::anilist::CalendarRelease;
use crate::db::cache::Anime;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

const MONTH_SHORT: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN",
    "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

const MONTH_TITLE: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const WEEKDAY_SHORT: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const WEEKDAY_FULL: [&str; 7] = [
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
];

/// Interactive scope filter toggled via `[f]` inside the calendar modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarFilter {
    #[default]
    All,
    TrackedOnly,
    MoviesAndSpecials,
}

impl CalendarFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::TrackedOnly,
            Self::TrackedOnly => Self::MoviesAndSpecials,
            Self::MoviesAndSpecials => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::TrackedOnly => "Tracked",
            Self::MoviesAndSpecials => "Movies",
        }
    }
}

/// Metadata describing a single day inside a 7-day Monday..=Sunday calendar week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekDayMeta {
    pub day_idx: usize,       // 0 = Mon .. 6 = Sun
    pub short_name: &'static str,
    pub full_name: &'static str,
    pub date_short: String,   // "Nov 09"
    pub day_num: u32,         // 9
    pub ymd: i64,             // 20261109
}

/// Complete boundary metadata for a selected calendar week (`week_offset = 0` is This Week).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekBounds {
    pub week_offset: i32,
    pub start_unix: i64,      // Inclusive Monday 00:00:00 local time (as UTC epoch)
    pub end_unix: i64,        // Inclusive Sunday 23:59:59 local time (as UTC epoch)
    pub query_prev_ymd: i64,  // Day before Monday (for AniList startDate_greater)
    pub query_next_ymd: i64,  // Day after Sunday (for AniList startDate_lesser)
    pub days: Vec<WeekDayMeta>,
    pub range_label: String,  // "NOV 9 – NOV 15, 2026"
}

/// A single enriched release entry ready for rendering inside a day's timeline.
#[derive(Debug, Clone)]
pub struct CalendarDayEntry {
    pub release: CalendarRelease,
    pub is_tracked: bool,
    pub is_watched_episode: bool,
    pub is_movie_or_special: bool,
    pub local_hhmm: String,
    pub day_label: String,
}

impl CalendarDayEntry {
    pub fn anime(&self) -> &Anime {
        &self.release.anime
    }
}

/// Read the OS local UTC offset in seconds (e.g., `+10800` for UTC+3) using SQLite's `'localtime'`.
pub async fn local_tz_offset_secs(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT CAST(strftime('%s', datetime('now', 'localtime')) - strftime('%s', 'now') AS INTEGER)",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

/// Convert a civil (year, month 1..=12, day 1..=31) date to days since 1970-01-01 (Howard Hinnant).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let m_i = m as i64;
    let d_i = d as i64;
    let y = y - if m_i <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m_i + if m_i > 2 { -3 } else { 9 }) + 2) / 5 + d_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Convert days since 1970-01-01 to civil (year, month 1..=12, day 1..=31).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

/// Return `0..=6` where `0 = Monday`, ..., `6 = Sunday` for days since `1970-01-01` (Thursday).
pub fn weekday_mon0_from_days(z: i64) -> usize {
    (z + 3).rem_euclid(7) as usize
}

/// Break a Unix epoch timestamp into local `(year, month, day, hour, minute)`.
pub fn local_components(unix_ts: i64, tz_offset_secs: i64) -> (i64, u32, u32, u32, u32) {
    let local_secs = unix_ts + tz_offset_secs;
    let days = local_secs.div_euclid(86400);
    let rem = local_secs.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    (year, month, day, hour, minute)
}

/// Return `YYYYMMDD` integer from `(year, month, day)`.
pub fn ymd_int(year: i64, month: u32, day: u32) -> i64 {
    year * 10_000 + (month as i64) * 100 + (day as i64)
}

/// Current local weekday index (`0 = Mon` .. `6 = Sun`).
pub fn today_weekday_idx(now_unix: i64, tz_offset_secs: i64) -> usize {
    let local_days = (now_unix + tz_offset_secs).div_euclid(86400);
    weekday_mon0_from_days(local_days)
}

/// Compute exact Monday..=Sunday week boundaries for any relative `week_offset` (`0` = current week).
pub fn compute_week_bounds(now_unix: i64, tz_offset_secs: i64, week_offset: i32) -> WeekBounds {
    let today_days = (now_unix + tz_offset_secs).div_euclid(86400);
    let today_wday = weekday_mon0_from_days(today_days) as i64;
    let monday_days = today_days - today_wday + (week_offset as i64) * 7;
    let sunday_days = monday_days + 6;

    let start_unix = monday_days * 86400 - tz_offset_secs;
    let end_unix = (sunday_days + 1) * 86400 - 1 - tz_offset_secs;

    let (py, pm, pd) = civil_from_days(monday_days - 1);
    let (ny, nm, nd) = civil_from_days(sunday_days + 1);
    let query_prev_ymd = ymd_int(py, pm, pd);
    let query_next_ymd = ymd_int(ny, nm, nd);

    let mut days = Vec::with_capacity(7);
    for i in 0..7 {
        let d_epoch = monday_days + i as i64;
        let (y, m, d) = civil_from_days(d_epoch);
        let mon_title = MONTH_TITLE.get((m as usize).saturating_sub(1)).copied().unwrap_or("Jan");
        days.push(WeekDayMeta {
            day_idx: i,
            short_name: WEEKDAY_SHORT[i],
            full_name: WEEKDAY_FULL[i],
            date_short: format!("{} {:02}", mon_title, d),
            day_num: d,
            ymd: ymd_int(y, m, d),
        });
    }

    let (y0, m0, d0) = civil_from_days(monday_days);
    let (y6, m6, d6) = civil_from_days(sunday_days);
    let m0_s = MONTH_SHORT.get((m0 as usize).saturating_sub(1)).copied().unwrap_or("JAN");
    let m6_s = MONTH_SHORT.get((m6 as usize).saturating_sub(1)).copied().unwrap_or("JAN");
    let range_label = if y0 == y6 {
        format!("{} {} – {} {}, {}", m0_s, d0, m6_s, d6, y6)
    } else {
        format!("{} {}, {} – {} {}, {}", m0_s, d0, y0, m6_s, d6, y6)
    };

    WeekBounds {
        week_offset,
        start_unix,
        end_unix,
        query_prev_ymd,
        query_next_ymd,
        days,
        range_label,
    }
}

/// Human label for `week_offset` (`"THIS WEEK"`, `"LAST WEEK"`, `"NEXT WEEK"`, etc.).
pub fn format_week_offset_tag(week_offset: i32) -> String {
    match week_offset {
        0 => "THIS WEEK".to_string(),
        -1 => "LAST WEEK".to_string(),
        1 => "NEXT WEEK".to_string(),
        n if n < -1 => format!("{} WEEKS AGO", -n),
        n => format!("IN {} WEEKS", n),
    }
}

/// Live countdown status badge text (no emojis).
pub fn format_countdown(airing_at: i64, now_unix: i64) -> String {
    if airing_at <= now_unix {
        return "AIRED".to_string();
    }
    let diff = airing_at - now_unix;
    if diff < 3600 {
        let mins = (diff / 60).max(1);
        format!("IN {}m", mins)
    } else if diff < 86400 {
        let hours = diff / 3600;
        let mins = (diff % 3600) / 60;
        if mins == 0 {
            format!("IN {}h", hours)
        } else {
            format!("IN {}h {:02}m", hours, mins)
        }
    } else {
        let days = diff / 86400;
        let hours = (diff % 86400) / 3600;
        if hours == 0 {
            format!("IN {}d", days)
        } else {
            format!("IN {}d {}h", days, hours)
        }
    }
}

/// Organize a week's raw AniList releases into 7 Monday..=Sunday day lists, applying filters,
/// watched status, and pinning Tracked shows first on each day.
pub fn build_week_buckets(
    releases: &[CalendarRelease],
    filter: CalendarFilter,
    tracked_ids: &HashSet<i64>,
    watched_map: &HashMap<i64, HashSet<u32>>,
    bounds: &WeekBounds,
    tz_offset_secs: i64,
) -> ([Vec<CalendarDayEntry>; 7], [bool; 7]) {
    let mut buckets: [Vec<CalendarDayEntry>; 7] = Default::default();
    let mut day_has_tracked = [false; 7];

    for rel in releases {
        let is_tracked = tracked_ids.contains(&rel.anime.id);
        let is_watched_episode = watched_map
            .get(&rel.anime.id)
            .is_some_and(|eps| eps.contains(&rel.episode));
        let is_movie_or_special = rel.is_movie_premiere
            || matches!(rel.anime.format.as_deref(), Some("MOVIE" | "OVA" | "SPECIAL"));

        // Map item cleanly onto one of the 7 day buckets (matching YMD first so movies never drift)
        let (y, m, d, hr, min) = local_components(rel.airing_at, tz_offset_secs);
        let ymd = ymd_int(y, m, d);
        let day_idx = if let Some(pos) = bounds.days.iter().position(|dm| dm.ymd == ymd) {
            pos
        } else if rel.airing_at >= bounds.start_unix && rel.airing_at <= bounds.end_unix {
            let local_days = (rel.airing_at + tz_offset_secs).div_euclid(86400);
            weekday_mon0_from_days(local_days)
        } else {
            continue;
        };

        if is_tracked {
            day_has_tracked[day_idx] = true;
        }

        match filter {
            CalendarFilter::All => {}
            CalendarFilter::TrackedOnly if !is_tracked => continue,
            CalendarFilter::MoviesAndSpecials if !is_movie_or_special => continue,
            _ => {}
        }

        let local_hhmm = if rel.is_movie_premiere {
            "PREMIERE".to_string()
        } else {
            format!("{:02}:{:02}", hr, min)
        };

        let meta = &bounds.days[day_idx];
        let day_label = format!("{}, {}", meta.full_name, meta.date_short);

        buckets[day_idx].push(CalendarDayEntry {
            release: rel.clone(),
            is_tracked,
            is_watched_episode,
            is_movie_or_special,
            local_hhmm,
            day_label,
        });
    }

    for day_list in &mut buckets {
        day_list.sort_by(|a, b| {
            b.is_tracked
                .cmp(&a.is_tracked)
                .then_with(|| a.release.airing_at.cmp(&b.release.airing_at))
                .then_with(|| b.anime().score.cmp(&a.anime().score))
                .then_with(|| a.anime().id.cmp(&b.anime().id))
        });
    }

    (buckets, day_has_tracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_conversion_and_weekday_accuracy() {
        // 2026-11-14 is Saturday (Mon=0..Sat=5)
        let days = days_from_civil(2026, 11, 14);
        assert_eq!(civil_from_days(days), (2026, 11, 14));
        assert_eq!(weekday_mon0_from_days(days), 5);

        // Leap day 2024-02-29 is Thursday (3)
        let leap = days_from_civil(2024, 2, 29);
        assert_eq!(civil_from_days(leap), (2024, 2, 29));
        assert_eq!(weekday_mon0_from_days(leap), 3);
    }

    #[test]
    fn test_compute_week_bounds_prev_this_next_week() {
        // 2026-11-14 16:50:00 +03:00
        let days_sat = days_from_civil(2026, 11, 14);
        let tz = 3 * 3600;
        let now_unix = days_sat * 86400 + 16 * 3600 + 50 * 60 - tz;

        let this_week = compute_week_bounds(now_unix, tz, 0);
        assert_eq!(this_week.days[0].ymd, 20261109); // Mon Nov 9
        assert_eq!(this_week.days[6].ymd, 20261115); // Sun Nov 15
        assert_eq!(this_week.query_prev_ymd, 20261108);
        assert_eq!(this_week.query_next_ymd, 20261116);
        assert_eq!(this_week.range_label, "NOV 9 – NOV 15, 2026");

        let prev_week = compute_week_bounds(now_unix, tz, -1);
        assert_eq!(prev_week.days[0].ymd, 20261102);
        assert_eq!(prev_week.days[6].ymd, 20261108);

        let next_week = compute_week_bounds(now_unix, tz, 1);
        assert_eq!(next_week.days[0].ymd, 20261116);
        assert_eq!(next_week.days[6].ymd, 20261122);
    }

    #[test]
    fn test_build_week_buckets_pins_tracked_first_and_filters_correctly() {
        let tz = 0;
        let sat_days = days_from_civil(2026, 11, 14);
        let now = sat_days * 86400 + 12 * 3600;
        let bounds = compute_week_bounds(now, tz, 0);

        let regular_tv = CalendarRelease {
            schedule_id: 1,
            airing_at: sat_days * 86400 + 10 * 3600,
            episode: 4,
            is_movie_premiere: false,
            anime: Anime {
                id: 100,
                title_romaji: "Morning TV".to_string(),
                format: Some("TV".to_string()),
                score: Some(80),
                ..Default::default()
            },
        };
        let tracked_tv = CalendarRelease {
            schedule_id: 2,
            airing_at: sat_days * 86400 + 18 * 3600,
            episode: 10,
            is_movie_premiere: false,
            anime: Anime {
                id: 200,
                title_romaji: "Tracked Evening".to_string(),
                format: Some("TV".to_string()),
                score: Some(92),
                ..Default::default()
            },
        };
        let movie = CalendarRelease {
            schedule_id: -300,
            airing_at: days_from_civil(2026, 11, 13) * 86400 + 12 * 3600,
            episode: 1,
            is_movie_premiere: true,
            anime: Anime {
                id: 300,
                title_romaji: "Premiere Film".to_string(),
                format: Some("MOVIE".to_string()),
                score: Some(85),
                ..Default::default()
            },
        };

        let mut tracked = HashSet::new();
        tracked.insert(200);
        let mut watched_map = HashMap::new();
        watched_map.insert(200, [10u32].into_iter().collect());

        let (buckets, has_tracked) = build_week_buckets(
            &[regular_tv.clone(), tracked_tv.clone(), movie.clone()],
            CalendarFilter::All,
            &tracked,
            &watched_map,
            &bounds,
            tz,
        );

        assert!(has_tracked[5]);
        assert!(!has_tracked[4]);
        assert_eq!(buckets[5].len(), 2);
        assert_eq!(buckets[5][0].anime().id, 200); // Tracked pinned first despite later airtime
        assert!(buckets[5][0].is_watched_episode);
        assert_eq!(buckets[4].len(), 1);
        assert_eq!(buckets[4][0].local_hhmm, "PREMIERE");

        let (movie_buckets, _) = build_week_buckets(
            &[regular_tv, tracked_tv, movie],
            CalendarFilter::MoviesAndSpecials,
            &tracked,
            &watched_map,
            &bounds,
            tz,
        );
        assert_eq!(movie_buckets[5].len(), 0);
        assert_eq!(movie_buckets[4].len(), 1);
    }

    #[test]
    fn test_format_countdown_and_week_tag() {
        assert_eq!(format_countdown(1000, 1200), "AIRED");
        assert_eq!(format_countdown(1900, 1000), "IN 15m");
        assert_eq!(format_countdown(1000 + 8100, 1000), "IN 2h 15m");
        assert_eq!(format_countdown(1000 + 90000, 1000), "IN 1d 1h");
        assert_eq!(format_week_offset_tag(-1), "LAST WEEK");
        assert_eq!(format_week_offset_tag(0), "THIS WEEK");
        assert_eq!(format_week_offset_tag(1), "NEXT WEEK");
    }
}
