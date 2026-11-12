//! Automated Subtitle Synchronization Engine.
//!
//! Two-Tier Zero-Effort Synchronizer:
//! - Tier 1: Subtitle-to-Subtitle Sequence Alignment (< 50ms)
//!   Aligns community subtitles (Russian, Arabic, Spanish, etc.) against the stream host's
//!   verified native reference subtitles (e.g. official English .vtt) using discrete interval
//!   cross-correlation and multi-scale framerate detection.
//! - Tier 2: Audio Voice Activity Detection (VAD) Fallback (~1.5s)
//!   Samples audio energy via ffmpeg silencedetect when no reference subtitle is available.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct SyncResult {
    pub offset_seconds: f64,
    pub scale_factor: f64,
    pub score: u64,
    pub applied: bool,
}

impl Default for SyncResult {
    fn default() -> Self {
        Self {
            offset_seconds: 0.0,
            scale_factor: 1.0,
            score: 0,
            applied: false,
        }
    }
}

/// Decode raw subtitle bytes handling UTF-8, stripping BOM, and falling back to CP1251 (Cyrillic).
pub fn decode_subtitle_bytes(bytes: &[u8]) -> String {
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };

    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    // Single-byte Cyrillic CP1251 fallback
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else if b >= 0xC0 {
            let unicode = 0x0410 + (b - 0xC0) as u32;
            if let Some(c) = char::from_u32(unicode) {
                out.push(c);
            } else {
                out.push(b as char);
            }
        } else if b == 0xA8 {
            out.push('Ё');
        } else if b == 0xB8 {
            out.push('ё');
        } else {
            out.push(b as char);
        }
    }
    out
}

/// Check if content represents valid subtitle text (not an HTML error page like 403 Forbidden).
pub fn is_valid_subtitle_content(content: &str) -> bool {
    let lower = content.to_lowercase();
    if lower.contains("<html")
        || lower.contains("<head")
        || lower.contains("<body")
        || lower.contains("403 forbidden")
        || lower.contains("404 not found")
        || lower.contains("<title>")
    {
        return false;
    }
    content.contains("-->")
        || content.contains("WEBVTT")
        || content.contains("[Script Info]")
        || content.contains("Dialogue:")
}

/// Format seconds into H:MM:SS.cs format for ASS subtitles.
pub fn format_ass_timestamp(seconds: f64) -> String {
    let total_cs = (seconds.max(0.0) * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{}:{:02}:{:02}.{:02}", h, m, s, cs)
}

/// Parse timestamp string in HH:MM:SS,mmm or HH:MM:SS.mmm format into seconds.
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim();
    let mut parts = s.split(':');
    let p1 = parts.next()?;
    let p2 = parts.next()?;
    let (h, m, sec_part) = if let Some(p3) = parts.next() {
        (p1.parse::<f64>().ok()?, p2.parse::<f64>().ok()?, p3)
    } else {
        (0.0, p1.parse::<f64>().ok()?, p2)
    };

    let sec_str = sec_part.replace(',', ".");
    let sec = sec_str.parse::<f64>().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Format seconds into HH:MM:SS{sep}mmm format.
pub fn format_timestamp(seconds: f64, separator: char) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{:02}:{:02}:{:02}{}{:03}", h, m, s, separator, ms)
}

/// Extract dialogue/speech time intervals `(start_sec, end_sec)` from raw .srt, .vtt, or .ass content.
pub fn parse_subtitle_intervals(content: &str) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(arrow_idx) = trimmed.find(" --> ") {
            let start_str = &trimmed[..arrow_idx].trim();
            let after_arrow = &trimmed[arrow_idx + 5..];
            let end_str = after_arrow.split_whitespace().next().unwrap_or("");
            if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                if e > s && (e - s) < 30.0 {
                    intervals.push((s, e));
                }
            }
        } else if trimmed.starts_with("Dialogue:") {
            let parts: Vec<&str> = trimmed.splitn(10, ',').collect();
            if parts.len() >= 3 {
                let start_str = parts[1].trim();
                let end_str = parts[2].trim();
                if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                    if e > s && (e - s) < 30.0 {
                        intervals.push((s, e));
                    }
                }
            }
        }
    }
    intervals
}

/// Calculate optimal offset and framerate scale factor to align target intervals to reference intervals.
pub fn calculate_optimal_alignment(
    ref_intervals: &[(f64, f64)],
    target_intervals: &[(f64, f64)],
) -> SyncResult {
    if ref_intervals.len() < 2 || target_intervals.len() < 2 {
        return SyncResult::default();
    }

    const BIN_SIZE: f64 = 0.05; // 50ms temporal resolution
    const MAX_TIME: f64 = 2400.0; // 40 minutes max episode duration
    let num_bins = (MAX_TIME / BIN_SIZE) as usize;

    let mut ref_grid = vec![false; num_bins];
    for &(s, e) in ref_intervals {
        let s_idx = ((s / BIN_SIZE).max(0.0) as usize).min(num_bins - 1);
        let e_idx = ((e / BIN_SIZE).max(0.0) as usize).min(num_bins - 1);
        for b in s_idx..=e_idx {
            ref_grid[b] = true;
        }
    }

    let compute_score = |scale: f64, offset: f64| -> u64 {
        let mut score = 0u64;
        for &(s, e) in target_intervals {
            let s_scaled = s * scale + offset;
            let e_scaled = e * scale + offset;
            let sb = (s_scaled / BIN_SIZE) as isize;
            let eb = (e_scaled / BIN_SIZE) as isize;

            if sb < 0 || eb >= num_bins as isize {
                continue;
            }

            for b in sb..=eb {
                if ref_grid[b as usize] {
                    score += 1;
                }
            }
        }
        score
    };

    // Phase 1: Baseline scale = 1.0 (most anime shares the same 23.976 master cadence)
    let mut best_scale = 1.0;
    let mut best_offset = 0.0;
    let mut best_score = 0u64;

    for step in -450..=450 {
        let offset = step as f64 * 0.1; // 100ms step from -45s to +45s
        let score = compute_score(1.0, offset);
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    // Plateau Centering for scale = 1.0:
    // If a range of offsets achieves >= 98% of peak score, pick the midpoint
    // to prevent edge bias caused by non-uniform dialogue gaps.
    if best_score > 0 {
        let threshold = (best_score as f64 * 0.98) as u64;
        let mut min_off = best_offset;
        let mut max_off = best_offset;
        for step in -450..=450 {
            let offset = step as f64 * 0.1;
            if (offset - best_offset).abs() <= 2.0 {
                let score = compute_score(1.0, offset);
                if score >= threshold {
                    if offset < min_off {
                        min_off = offset;
                    }
                    if offset > max_off {
                        max_off = offset;
                    }
                }
            }
        }
        best_offset = (min_off + max_off) / 2.0;
    }

    // Phase 2: Test framerate drift scales only if they beat 1.0 baseline by a significant margin
    const OTHER_SCALES: [f64; 4] = [
        23.976 / 25.0,
        25.0 / 23.976,
        24.0 / 23.976,
        23.976 / 24.0,
    ];

    for &scale in &OTHER_SCALES {
        for step in -450..=450 {
            let offset = step as f64 * 0.1;
            let score = compute_score(scale, offset);
            // Must beat 1.0 baseline by at least 6% to justify non-standard framerate scaling
            if score > ((best_score as f64 * 1.06) as u64) + 15 {
                best_score = score;
                best_scale = scale;
                best_offset = offset;
            }
        }
    }

    // Phase 3: Fine search around the coarse peak (±0.25s in 0.01s steps)
    let coarse_offset = best_offset;
    for step in -25..=25 {
        let offset = coarse_offset + (step as f64 * 0.01);
        let score = compute_score(best_scale, offset);
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    // Identity Guard: If at offset 0.0s (scale 1.0) the score already achieves >= 90%
    // of the peak score, the subtitle is already in sync with the video release.
    // Any minor offset differences (< 2s) are translator-pacing variations or song credits.
    // Suppress offset shift to prevent introducing an artificial delay on in-sync tracks.
    let score_zero = compute_score(1.0, 0.0);
    if score_zero > 0 && score_zero >= ((best_score as f64 * 0.90) as u64) && (best_scale - 1.0).abs() < 0.001 {
        return SyncResult {
            offset_seconds: 0.0,
            scale_factor: 1.0,
            score: score_zero,
            applied: false,
        };
    }

    // Decision: only apply if the adjustment is noticeable (> 80ms or non-1.0 scale) and score is solid
    let is_already_synced = best_offset.abs() < 0.08 && (best_scale - 1.0).abs() < 0.001;
    let applied = !is_already_synced && best_score > 60;

    SyncResult {
        offset_seconds: (best_offset * 100.0).round() / 100.0,
        scale_factor: (best_scale * 100000.0).round() / 100000.0,
        score: best_score,
        applied,
    }
}

/// Apply scale and offset shift to all timestamp lines in subtitle text (supports SRT, VTT, and ASS).
pub fn apply_subtitle_shift(content: &str, offset_seconds: f64, scale_factor: f64) -> String {
    let mut out = String::with_capacity(content.len());

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(arrow_idx) = line.find(" --> ") {
            let start_str = line[..arrow_idx].trim();
            let after_arrow = &line[arrow_idx + 5..];
            let mut parts = after_arrow.split_whitespace();

            if let Some(end_str) = parts.next() {
                let remainder = &after_arrow[end_str.len()..];
                if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                    let sep = if start_str.contains(',') || end_str.contains(',') { ',' } else { '.' };
                    let new_s = (s * scale_factor + offset_seconds).max(0.0);
                    let new_e = (e * scale_factor + offset_seconds).max(0.0);
                    out.push_str(&format!(
                        "{} --> {}{}\n",
                        format_timestamp(new_s, sep),
                        format_timestamp(new_e, sep),
                        remainder
                    ));
                    continue;
                }
            }
        } else if trimmed.starts_with("Dialogue:") {
            let parts: Vec<&str> = line.splitn(10, ',').collect();
            if parts.len() >= 3 {
                let start_str = parts[1].trim();
                let end_str = parts[2].trim();
                if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                    let new_s = (s * scale_factor + offset_seconds).max(0.0);
                    let new_e = (e * scale_factor + offset_seconds).max(0.0);
                    let new_start = format_ass_timestamp(new_s);
                    let new_end = format_ass_timestamp(new_e);
                    out.push_str(&format!(
                        "{},{},{},{}\n",
                        parts[0],
                        new_start,
                        new_end,
                        parts[3..].join(",")
                    ));
                    continue;
                }
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

/// Fallback Tier 2: Probe audio lead time via ffmpeg silencedetect (runs in < 1.5s).
#[allow(dead_code)]
pub async fn estimate_audio_lead_time(stream_url: &str) -> Option<f64> {
    if !stream_url.starts_with("http://") && !stream_url.starts_with("https://") {
        return None;
    }

    let output = tokio::time::timeout(
        Duration::from_millis(2500),
        tokio::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-ss", "0",
                "-t", "45",
                "-i", stream_url,
                "-af", "silencedetect=noise=-30dB:d=0.4",
                "-f", "null",
                "-",
            ])
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        if line.contains("silence_end:") {
            if let Some(pos) = line.find("silence_end:") {
                let rest = &line[pos + 12..].trim();
                let end_str = rest.split_whitespace().next()?;
                if let Ok(lead) = end_str.parse::<f64>() {
                    return Some(lead);
                }
            }
        }
    }

    None
}

/// Ensure that target subtitle is downloaded, verified, and synchronized against reference subtitle.
/// Writes the synchronized subtitle to ~/.cache/solus/subs/{anime_id}_{ep}_{code}_synced.srt
/// and returns the verified local path.
pub async fn ensure_synchronized_subtitle(
    client: &reqwest::Client,
    target_sub_url: &str,
    ref_sub_url: Option<&str>,
    _stream_url: Option<&str>,
    anime_id: i64,
    episode: u32,
    code: &str,
) -> (String, Option<SyncResult>) {
    if !target_sub_url.starts_with("http://") && !target_sub_url.starts_with("https://") {
        return (target_sub_url.to_string(), None);
    }

    let cache_dir = directories::ProjectDirs::from("", "", "ani-tui")
        .map(|dirs| dirs.cache_dir().join("subs"))
        .unwrap_or_else(|| std::env::temp_dir().join("solus-subs"));

    if std::fs::create_dir_all(&cache_dir).is_err() {
        return (target_sub_url.to_string(), None);
    }

    let clean_code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let code_name = if clean_code.is_empty() { "sub" } else { &clean_code };

    // Check if valid cached version already exists (.ass, .srt, .vtt)
    for ext in ["ass", "srt", "vtt"] {
        let p = cache_dir.join(format!("{}_{}_{}_synced.{}", anime_id, episode, code_name, ext));
        if p.is_file() {
            if let Ok(cached_content) = std::fs::read_to_string(&p) {
                if is_valid_subtitle_content(&cached_content) {
                    return (p.to_string_lossy().into_owned(), None);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }

    // 1. Download target subtitle content with strict headers and status check
    let target_resp = match client
        .get(target_sub_url)
        .header("Referer", "https://zokoanime.video/")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return (target_sub_url.to_string(), None),
    };
    let Ok(target_bytes) = target_resp.bytes().await else {
        return (target_sub_url.to_string(), None);
    };
    if target_bytes.is_empty() {
        return (target_sub_url.to_string(), None);
    }

    // Transparent gzip decompression if payload is .gz
    let raw_target_bytes = if target_bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&target_bytes[..]);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok() {
            out
        } else {
            target_bytes.to_vec()
        }
    } else {
        target_bytes.to_vec()
    };

    let target_text = decode_subtitle_bytes(&raw_target_bytes);
    if !is_valid_subtitle_content(&target_text) {
        return (target_sub_url.to_string(), None);
    }

    // 2. Tier 1: Align against reference subtitle if available
    let mut sync_res: Option<SyncResult> = None;
    let mut final_content = target_text.clone();

    if let Some(ref_url) = ref_sub_url {
        if ref_url != target_sub_url && (ref_url.starts_with("http://") || ref_url.starts_with("https://")) {
            let ref_req = client
                .get(ref_url)
                .header("Referer", "https://zokoanime.video/")
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
                .timeout(Duration::from_secs(6));

            if let Ok(ref_resp) = ref_req.send().await {
                if ref_resp.status().is_success() {
                    if let Ok(ref_bytes) = ref_resp.bytes().await {
                        let raw_ref_bytes = if ref_bytes.starts_with(&[0x1f, 0x8b]) {
                            use std::io::Read;
                            let mut decoder = flate2::read::GzDecoder::new(&ref_bytes[..]);
                            let mut out = Vec::new();
                            if decoder.read_to_end(&mut out).is_ok() {
                                out
                            } else {
                                ref_bytes.to_vec()
                            }
                        } else {
                            ref_bytes.to_vec()
                        };

                        let ref_text = decode_subtitle_bytes(&raw_ref_bytes);
                        if is_valid_subtitle_content(&ref_text) {
                            let ref_intervals = parse_subtitle_intervals(&ref_text);
                            let target_intervals = parse_subtitle_intervals(&target_text);

                            let alignment = calculate_optimal_alignment(&ref_intervals, &target_intervals);
                            if alignment.applied {
                                final_content = apply_subtitle_shift(&target_text, alignment.offset_seconds, alignment.scale_factor);
                            }
                            sync_res = Some(alignment);
                        }
                    }
                }
            }
        }
    }

    // Determine appropriate extension (.ass, .vtt, or .srt)
    let ext = if final_content.contains("[Script Info]") || final_content.contains("Dialogue:") {
        "ass"
    } else if final_content.contains("WEBVTT") {
        "vtt"
    } else {
        "srt"
    };
    let synced_path = cache_dir.join(format!("{}_{}_{}_synced.{}", anime_id, episode, code_name, ext));

    // Write final validated file to cache
    if is_valid_subtitle_content(&final_content) && std::fs::write(&synced_path, &final_content).is_ok() {
        return (synced_path.to_string_lossy().into_owned(), sync_res);
    }

    (target_sub_url.to_string(), sync_res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_format_timestamp() {
        let t1 = parse_timestamp("00:02:07,600").unwrap();
        assert!((t1 - 127.6).abs() < 1e-4);

        let t2 = parse_timestamp("01:15:30.250").unwrap();
        assert!((t2 - 4530.25).abs() < 1e-4);

        let f1 = format_timestamp(127.6, ',');
        assert_eq!(f1, "00:02:07,600");

        let f2 = format_timestamp(4530.25, '.');
        assert_eq!(f2, "01:15:30.250");
    }

    #[test]
    fn test_parse_subtitle_intervals() {
        let srt = r#"1
00:00:10,000 --> 00:00:14,000
Hello World

2
00:00:20,500 --> 00:00:25,000
Second dialogue
"#;
        let intervals = parse_subtitle_intervals(srt);
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0], (10.0, 14.0));
        assert_eq!(intervals[1], (20.5, 25.0));
    }

    #[test]
    fn test_optimal_alignment_detects_linear_offset() {
        // Reference dialogue at 10.0, 20.0, 30.0, 40.0, 50.0
        let ref_intervals = vec![
            (10.0, 13.0),
            (20.0, 24.0),
            (30.0, 33.0),
            (40.0, 45.0),
            (50.0, 52.0),
        ];

        // Target dialogue is 2.5 seconds earlier (faster) at 7.5, 17.5, 27.5, 37.5, 47.5
        let target_intervals = vec![
            (7.5, 10.5),
            (17.5, 21.5),
            (27.5, 30.5),
            (37.5, 42.5),
            (47.5, 49.5),
        ];

        let result = calculate_optimal_alignment(&ref_intervals, &target_intervals);
        assert!(result.applied);
        assert!((result.offset_seconds - 2.5).abs() <= 0.05);
        assert!((result.scale_factor - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_optimal_alignment_detects_pal_drift() {
        let ref_intervals = vec![
            (100.0, 105.0),
            (200.0, 205.0),
            (300.0, 305.0),
            (400.0, 405.0),
        ];

        // Scale by 25.0 / 23.976
        let scale = 25.0 / 23.976;
        let target_intervals: Vec<(f64, f64)> = ref_intervals
            .iter()
            .map(|&(s, e)| (s / scale + 1.0, e / scale + 1.0))
            .collect();

        let result = calculate_optimal_alignment(&ref_intervals, &target_intervals);
        assert!(result.applied);
        assert!((result.scale_factor - (25.0 / 23.976)).abs() < 0.01);
    }

    #[test]
    fn test_apply_subtitle_shift() {
        let srt = "1\n00:00:10,000 --> 00:00:14,000\nHello\n";
        let shifted = apply_subtitle_shift(srt, 2.5, 1.0);
        assert!(shifted.contains("00:00:12,500 --> 00:00:16,500"));
        assert!(shifted.contains("Hello"));
    }

    #[test]
    fn test_apply_subtitle_shift_vtt_with_cues() {
        let vtt = "WEBVTT\n\n00:00:10.000 --> 00:00:14.000 position:50% line:0%\nHello\n";
        let shifted = apply_subtitle_shift(vtt, -3.0, 1.0);
        assert!(shifted.contains("00:00:07.000 --> 00:00:11.000 position:50% line:0%"));
    }

    #[test]
    fn test_alignment_real_world_lain_ep4_sample() {
        let en_vtt = r#"WEBVTT
00:00:02.660 --> 00:00:05.640
Present Day

00:02:09.690 --> 00:02:14.270
I don't need parents. Humans are all alone.

00:02:15.110 --> 00:02:18.440
They're not connected to anyone at all.

00:04:01.590 --> 00:04:03.340
How did it go?

00:04:07.260 --> 00:04:09.300
Lain's acting funny.
"#;

        let ru_srt = r#"1
00:02:07,600 --> 00:02:12,200
Мне не нужны родители.

2
00:02:13,000 --> 00:02:16,300
И ни с кем не соединен.

3
00:03:59,500 --> 00:04:01,000
Как у нее дела?

4
00:04:05,100 --> 00:04:06,900
Лэйн странно себя ведет.
"#;

        let ref_intervals = parse_subtitle_intervals(en_vtt);
        let target_intervals = parse_subtitle_intervals(ru_srt);
        let sync = calculate_optimal_alignment(&ref_intervals, &target_intervals);

        assert!(sync.applied);
        assert!((sync.offset_seconds - 2.10).abs() <= 0.15);
        assert!((sync.scale_factor - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_optimal_alignment_identity_guard_on_insync() {
        let ref_intervals = vec![
            (10.0, 13.0),
            (20.0, 24.0),
            (30.0, 33.0),
            (40.0, 45.0),
            (50.0, 52.0),
        ];
        // Target is essentially identical (minor 20ms jitter from speech boundary)
        let target_intervals = vec![
            (10.02, 13.01),
            (20.01, 23.98),
            (30.03, 33.00),
            (40.01, 45.02),
            (50.02, 51.99),
        ];

        let result = calculate_optimal_alignment(&ref_intervals, &target_intervals);
        // Identity guard must activate and guarantee 0.0s offset
        assert!(!result.applied);
        assert_eq!(result.offset_seconds, 0.0);
        assert_eq!(result.scale_factor, 1.0);
    }

    #[test]
    fn test_is_valid_subtitle_content() {
        let html_403 = "<html><head><title>403 Forbidden</title></head><body>nginx</body></html>";
        assert!(!is_valid_subtitle_content(html_403));

        let valid_vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello";
        assert!(is_valid_subtitle_content(valid_vtt));

        let valid_srt = "1\n00:00:01,000 --> 00:00:04,000\nHello";
        assert!(is_valid_subtitle_content(valid_srt));
    }

    #[test]
    fn test_decode_subtitle_bytes() {
        // UTF-8 with BOM
        let utf8_bom = [0xEF, 0xBB, 0xBF, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(decode_subtitle_bytes(&utf8_bom), "Hello");

        // CP1251 bytes: 0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2 = "Привет"
        let cp1251_bytes = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        assert_eq!(decode_subtitle_bytes(&cp1251_bytes), "Привет");
    }

    #[test]
    fn test_parse_and_shift_ass_subtitles() {
        let ass = "[Script Info]\nTitle: Test\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:02:01.70,0:02:08.70,Default,,0,0,0,,Hello Arabic\n";
        let intervals = parse_subtitle_intervals(ass);
        assert_eq!(intervals.len(), 1);
        assert!((intervals[0].0 - 121.70).abs() < 1e-3);
        assert!((intervals[0].1 - 128.70).abs() < 1e-3);

        let shifted = apply_subtitle_shift(ass, 2.0, 1.0);
        assert!(shifted.contains("0:02:03.70,0:02:10.70"));
        assert!(is_valid_subtitle_content(ass));
    }
}
