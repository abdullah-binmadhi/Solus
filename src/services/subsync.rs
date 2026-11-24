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
/// Performs intelligent speech filtering:
/// 1. Discards micro-intervals (< 0.35s) created by ASS karaoke syllable timing and frame-by-frame typesetting animations.
/// 2. Discards ASS non-dialogue styling layers (OP/ED song romanizations, karaoke, logos, signs, and staff credits).
/// 3. Deduplicates coincident timestamps produced by multi-layer ASS outline/blur effects.
pub fn parse_subtitle_intervals(content: &str) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(arrow_idx) = trimmed.find(" --> ") {
            let start_str = &trimmed[..arrow_idx].trim();
            let after_arrow = &trimmed[arrow_idx + 5..];
            let end_str = after_arrow.split_whitespace().next().unwrap_or("");
            if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                // Spoken dialogue is at least 0.35s and less than 30s
                if e > s && (e - s) >= 0.35 && (e - s) < 30.0 {
                    intervals.push((s, e));
                }
            }
        } else if trimmed.starts_with("Dialogue:") {
            let parts: Vec<&str> = trimmed.splitn(10, ',').collect();
            if parts.len() >= 3 {
                // If style is available, filter out known karaoke, song, or typesetting animation styles
                if parts.len() >= 4 {
                    let style = parts[3].trim().to_lowercase();
                    if style.contains("rom")
                        || style.contains("op")
                        || style.contains("ed")
                        || style.contains("karaoke")
                        || style.contains("hit the road")
                        || style.contains("logo")
                        || style.contains("effect")
                        || style.contains("sign")
                        || style.contains("staff")
                    {
                        continue;
                    }
                }

                // If text contains karaoke tags \k or \K, skip
                if parts.len() >= 10 {
                    let text = parts[9].to_lowercase();
                    if text.contains(r"\k") {
                        continue;
                    }
                }

                let start_str = parts[1].trim();
                let end_str = parts[2].trim();
                if let (Some(s), Some(e)) = (parse_timestamp(start_str), parse_timestamp(end_str)) {
                    // Ignore sub-350ms slices (particle effects, animated letter fades, karaoke syllable ticks)
                    if e > s && (e - s) >= 0.35 && (e - s) < 30.0 {
                        // Deduplicate layered styles with identical start timestamp (within 20ms)
                        if let Some(&(last_s, _)) = intervals.last() {
                            if (s - last_s).abs() < 0.02 {
                                continue;
                            }
                        }
                        intervals.push((s, e));
                    }
                }
            }
        }
    }
    intervals
}

/// Calculate optimal offset and framerate scale factor to align target intervals to reference intervals.
/// Uses fine 25ms bins + 3x speech-onset weighting so 0.4s-2.0s lags are caught sharply.
pub fn calculate_optimal_alignment(
    ref_intervals: &[(f64, f64)],
    target_intervals: &[(f64, f64)],
) -> SyncResult {
    if ref_intervals.len() < 2 || target_intervals.len() < 2 {
        return SyncResult::default();
    }

    const BIN_SIZE: f64 = 0.025; // 25ms temporal resolution
    const ONSET_BINS: usize = 14; // 350ms dialogue onset window weighted 3x
    const MAX_TIME: f64 = 2400.0;
    let num_bins = (MAX_TIME / BIN_SIZE) as usize;

    let mut ref_grid = vec![0u8; num_bins];
    for &(s, e) in ref_intervals {
        let s_idx = ((s / BIN_SIZE).max(0.0) as usize).min(num_bins - 1);
        let e_idx = ((e / BIN_SIZE).max(0.0) as usize).min(num_bins - 1);
        for b in s_idx..=e_idx {
            let w = if b < s_idx + ONSET_BINS { 3 } else { 1 };
            if w > ref_grid[b] {
                ref_grid[b] = w;
            }
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

            let onset_end = sb + ONSET_BINS as isize;
            for b in sb..=eb {
                let rw = ref_grid[b as usize] as u64;
                if rw > 0 {
                    let tw = if b < onset_end { 2 } else { 1 };
                    score += rw * tw;
                }
            }
        }
        score
    };

    // Phase 1: Baseline scale = 1.0
    let mut best_scale = 1.0;
    let mut best_offset = 0.0;
    let mut best_score = 0u64;

    for step in -450..=450 {
        let offset = step as f64 * 0.1;
        let score = compute_score(1.0, offset);
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    // Plateau Centering for scale = 1.0
    if best_score > 0 {
        let threshold = (best_score as f64 * 0.985) as u64;
        let mut min_off = best_offset;
        let mut max_off = best_offset;
        for step in -450..=450 {
            let offset = step as f64 * 0.1;
            if (offset - best_offset).abs() <= 1.5 {
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

    // Phase 2: Test framerate drift scales only if they beat 1.0 baseline clearly
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
            if score > ((best_score as f64 * 1.08) as u64) + 30 {
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

    // Precision Identity Guard:
    // Suppress tiny <0.25s noise, or suppress >=0.25s ONLY if 0.0s scores >= 95% of peak.
    let score_zero = compute_score(1.0, 0.0);
    let identity_thresh = if best_offset.abs() < 0.25 { 0.85 } else { 0.95 };
    if score_zero > 0
        && score_zero >= ((best_score as f64 * identity_thresh) as u64)
        && (best_scale - 1.0).abs() < 0.001
    {
        return SyncResult {
            offset_seconds: 0.0,
            scale_factor: 1.0,
            score: score_zero,
            applied: false,
        };
    }

    let is_already_synced = best_offset.abs() < 0.15 && (best_scale - 1.0).abs() < 0.001;
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

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechVadSyncResult {
    pub offset_seconds: f64,
    pub scale_factor: f64,
    pub confidence_pct: u8,
    pub z_score: f64,
    pub prominence: f64,
    pub applied: bool,
    pub reason: String,
}

impl Default for SpeechVadSyncResult {
    fn default() -> Self {
        Self {
            offset_seconds: 0.0,
            scale_factor: 1.0,
            confidence_pct: 0,
            z_score: 0.0,
            prominence: 0.0,
            applied: false,
            reason: "Insufficient speech signal".to_string(),
        }
    }
}

/// Convert `ffmpeg silencedetect` stderr log lines inside `[window_start, window_start + window_duration]`
/// into absolute episode-time spoken speech intervals `(start_secs, end_secs)`.
pub fn parse_silencedetect_speech_intervals(
    stderr: &str,
    window_start: f64,
    window_duration: f64,
) -> Vec<(f64, f64)> {
    let mut silences: Vec<(f64, f64)> = Vec::new();
    let mut pending_start: Option<f64> = None;

    for line in stderr.lines() {
        if let Some(pos) = line.find("silence_start:") {
            let val_s = line[pos + 14..].split_whitespace().next().unwrap_or("");
            if let Ok(t) = val_s.parse::<f64>() {
                pending_start = Some(t.max(0.0));
            }
        } else if let Some(pos) = line.find("silence_end:") {
            let val_s = line[pos + 12..].split_whitespace().next().unwrap_or("");
            if let Ok(end_t) = val_s.parse::<f64>() {
                let start_t = pending_start.take().unwrap_or(0.0).min(end_t);
                silences.push((start_t, end_t.min(window_duration)));
            }
        }
    }
    if let Some(open_s) = pending_start {
        if open_s < window_duration {
            silences.push((open_s, window_duration));
        }
    }

    let mut speech = Vec::new();
    let mut cursor = 0.0_f64;
    for (sil_s, sil_e) in silences {
        if sil_s > cursor {
            let dur = sil_s - cursor;
            if (0.25..=25.0).contains(&dur) {
                speech.push((window_start + cursor, window_start + sil_s));
            }
        }
        cursor = cursor.max(sil_e);
    }
    if window_duration > cursor {
        let dur = window_duration - cursor;
        if (0.25..=25.0).contains(&dur) {
            speech.push((window_start + cursor, window_start + window_duration));
        }
    }
    speech
}

/// Extract speech-band (240 Hz - 3200 Hz) vocal activity intervals from a media URL or file using `ffmpeg`.
pub async fn extract_speech_intervals_ffmpeg(
    stream_url: &str,
    referer: Option<&str>,
    start_secs: f64,
    duration_secs: f64,
) -> Vec<(f64, f64)> {
    let ffmpeg_cmd = if std::path::Path::new("/opt/homebrew/bin/ffmpeg").exists() {
        "/opt/homebrew/bin/ffmpeg"
    } else if std::path::Path::new("/usr/local/bin/ffmpeg").exists() {
        "/usr/local/bin/ffmpeg"
    } else {
        "ffmpeg"
    };
    let is_http = stream_url.starts_with("http://") || stream_url.starts_with("https://");
    let headers_val = referer
        .filter(|r| !r.trim().is_empty())
        .map(|r| {
            format!(
                "Referer: {r}\r\nUser-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36\r\n"
            )
        });

    for af_filter in [
        "highpass=f=240,lowpass=f=3200,afftdn=nf=-25,silencedetect=noise=-28dB:d=0.35",
        "highpass=f=240,lowpass=f=3200,silencedetect=noise=-28dB:d=0.35",
    ] {
        let mut cmd = tokio::process::Command::new(ffmpeg_cmd);
        cmd.arg("-hide_banner").arg("-nostats").arg("-vn");
        if is_http {
            if let Some(ref hdrs) = headers_val {
                cmd.args(["-headers", hdrs]);
            }
            if stream_url.contains(".m3u8") || stream_url.contains("/m3u8") {
                cmd.args([
                    "-allowed_extensions",
                    "ALL",
                    "-allowed_segment_extensions",
                    "ALL",
                ]);
            }
        }
        cmd.args([
            "-ss",
            &format!("{:.1}", start_secs.max(0.0)),
            "-t",
            &format!("{:.1}", duration_secs.clamp(15.0, 180.0)),
            "-i",
            stream_url,
            "-af",
            af_filter,
            "-f",
            "null",
            "-",
        ]);
        let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(12), cmd.output()).await else {
            continue;
        };
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("No such filter: 'afftdn'") {
            continue;
        }
        let intervals = parse_silencedetect_speech_intervals(&stderr, start_secs.max(0.0), duration_secs);
        if !intervals.is_empty() || out.status.success() {
            return intervals;
        }
    }
    Vec::new()
}

/// Wide-range (-90s..+90s) Speech-VAD vs Subtitle onset cross-correlation with Z-score & prominence validation.
/// Returns `offset_seconds` matching mpv's `sub-delay` sign convention (`t_audio ≈ t_sub + offset_seconds`).
pub fn calculate_speech_vad_alignment(
    audio_speech: &[(f64, f64)],
    sub_cues: &[(f64, f64)],
) -> SpeechVadSyncResult {
    if audio_speech.len() < 3 || sub_cues.len() < 3 {
        return SpeechVadSyncResult {
            reason: "Too few dialogue or speech intervals in scene".to_string(),
            ..Default::default()
        };
    }

    const BIN: f64 = 0.05; // 50ms coarse grid
    const ONSET_BINS: usize = 7; // 350ms onset weight 3x
    let max_t = audio_speech
        .iter()
        .map(|i| i.1)
        .chain(sub_cues.iter().map(|i| i.1))
        .fold(0.0_f64, f64::max)
        + 120.0;
    let num_bins = ((max_t / BIN).ceil() as usize).clamp(4000, 120_000);

    let mut audio_grid = vec![0u8; num_bins];
    for &(s, e) in audio_speech {
        let sb = ((s / BIN).max(0.0) as usize).min(num_bins - 1);
        let eb = ((e / BIN).max(0.0) as usize).min(num_bins - 1);
        for b in sb..=eb {
            let w = if b < sb + ONSET_BINS { 3 } else { 1 };
            audio_grid[b] = audio_grid[b].max(w);
        }
    }

    // Focus subtitle cues near the probed audio span (with ±95s margin)
    let a_min = audio_speech.iter().map(|i| i.0).fold(f64::INFINITY, f64::min) - 95.0;
    let a_max = audio_speech.iter().map(|i| i.1).fold(0.0_f64, f64::max) + 95.0;
    let local_subs: Vec<(f64, f64)> = sub_cues
        .iter()
        .copied()
        .filter(|&(s, e)| e >= a_min && s <= a_max)
        .collect();
    let active_subs = if local_subs.len() >= 3 { &local_subs[..] } else { sub_cues };

    let score_fn = |scale: f64, offset: f64| -> u64 {
        let mut total = 0u64;
        for &(s, e) in active_subs {
            let sb = ((s * scale + offset) / BIN).round() as isize;
            let eb = ((e * scale + offset) / BIN).round() as isize;
            if sb < 0 || eb >= num_bins as isize {
                continue;
            }
            let onset_end = sb + ONSET_BINS as isize;
            for b in sb..=eb {
                let aw = audio_grid[b as usize] as u64;
                if aw > 0 {
                    let sw = if b < onset_end { 3 } else { 1 };
                    total += aw * sw;
                }
            }
        }
        total
    };

    // Coarse scan across -90.0s .. +90.0s in 0.10s steps
    let mut coarse: Vec<(f64, u64)> = Vec::with_capacity(1801);
    for step in -900..=900 {
        let off = step as f64 * 0.1;
        coarse.push((off, score_fn(1.0, off)));
    }

    let n = coarse.len() as f64;
    let mean = coarse.iter().map(|&(_, sc)| sc as f64).sum::<f64>() / n;
    let var = coarse
        .iter()
        .map(|&(_, sc)| {
            let d = sc as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    let std_dev = var.sqrt().max(1.0);

    // Select best peak, refine around top 3 peaks at 20ms resolution
    coarse.sort_by_key(|&(_, sc)| std::cmp::Reverse(sc));
    let mut top_peaks: Vec<f64> = Vec::new();
    for &(off, _) in &coarse {
        if top_peaks.iter().all(|p| (p - off).abs() >= 2.0) {
            top_peaks.push(off);
            if top_peaks.len() >= 3 {
                break;
            }
        }
    }

    let mut best_offset = 0.0_f64;
    let mut best_scale = 1.0_f64;
    let mut best_score = 0u64;
    for peak in &top_peaks {
        for sub_step in -30..=30 {
            let off = peak + (sub_step as f64 * 0.02);
            let sc = score_fn(1.0, off);
            if sc > best_score {
                best_score = sc;
                best_offset = off;
            }
        }
    }

    // Also test common broadcast speed conversions around top peak
    for &scale in &[23.976 / 25.0, 25.0 / 23.976, 24.0 / 23.976, 23.976 / 24.0] {
        for sub_step in -25..=25 {
            let off = best_offset + (sub_step as f64 * 0.05);
            let sc = score_fn(scale, off);
            if sc > ((best_score as f64 * 1.10) as u64) + 30 {
                best_score = sc;
                best_scale = scale;
                best_offset = off;
            }
        }
    }

    let second_peak = coarse
        .iter()
        .find(|&&(off, _)| (off - best_offset).abs() >= 2.0)
        .map(|&(_, sc)| sc.max(1) as f64)
        .unwrap_or(1.0);

    let z_score = (best_score as f64 - mean) / std_dev;
    let prominence = best_score as f64 / second_peak;
    let confidence_pct = ((z_score / 5.5).clamp(0.0, 1.0) * 100.0).round() as u8;
    let applied = best_score >= 30 && z_score >= 2.4 && prominence >= 1.08;

    let reason = if applied {
        format!("Speech-VAD match ({confidence_pct}% confidence)")
    } else {
        "No clear spoken dialogue match near this scene — seek to dialogue and press [y]".to_string()
    };

    SpeechVadSyncResult {
        offset_seconds: (best_offset * 100.0).round() / 100.0,
        scale_factor: (best_scale * 100000.0).round() / 100000.0,
        confidence_pct,
        z_score,
        prominence,
        applied,
        reason,
    }
}

static VOCAL_ENV_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<(String, u64), Vec<f32>>>,
> = std::sync::OnceLock::new();

fn vocal_env_cache() -> &'static std::sync::Mutex<std::collections::HashMap<(String, u64), Vec<f32>>> {
    VOCAL_ENV_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Convert raw `8 kHz mono s16le` PCM into a 10 Hz (100ms/frame) vocal modulation envelope
/// using `ln(1 + rms)` minus a centered 4.0s running local mean to cancel steady BGM/OPs.
pub fn pcm_s16le_to_vocal_envelope(pcm_bytes: &[u8]) -> Vec<f32> {
    const SAMPLES_PER_FRAME: usize = 800; // 100ms at 8000 Hz
    const LOCAL_RADIUS: usize = 20; // ±2.0s => 4.0s window
    let sample_count = pcm_bytes.len() / 2;
    let frame_count = sample_count / SAMPLES_PER_FRAME;
    if frame_count < 20 {
        return Vec::new();
    }

    let mut log_rms = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let base = f * SAMPLES_PER_FRAME * 2;
        let mut sum_sq = 0.0_f64;
        for s in 0..SAMPLES_PER_FRAME {
            let idx = base + s * 2;
            let sample = i16::from_le_bytes([pcm_bytes[idx], pcm_bytes[idx + 1]]) as f64;
            sum_sq += sample * sample;
        }
        let rms = (sum_sq / SAMPLES_PER_FRAME as f64).sqrt();
        log_rms.push((1.0 + rms).ln() as f32);
    }

    let mut env = Vec::with_capacity(frame_count);
    let mut prefix = Vec::with_capacity(frame_count + 1);
    prefix.push(0.0_f64);
    for &v in &log_rms {
        prefix.push(prefix.last().copied().unwrap_or(0.0) + v as f64);
    }
    const WIDE_RADIUS: usize = 30; // ±3.0s centered window keeps full monologues intact
    for i in 0..frame_count {
        let lo = i.saturating_sub(WIDE_RADIUS);
        let hi = (i + WIDE_RADIUS + 1).min(frame_count);
        let local_mean = ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32;
        env.push((log_rms[i] - local_mean).max(0.0));
    }

    // Subtract window's 25th-percentile ambient floor so silent pauses clamp cleanly to 0.0
    let mut sorted = env.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 4];
    for v in &mut env {
        *v = (*v - floor).max(0.0);
    }

    // Soft-cap extreme SFX spikes at 3x std-dev
    let mean = env.iter().map(|&v| v as f64).sum::<f64>() / env.len() as f64;
    let var = env
        .iter()
        .map(|&v| {
            let d = v as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / env.len() as f64;
    let cap = (mean + 3.0 * var.sqrt()).max(0.5) as f32;
    for v in &mut env {
        *v = (*v).min(cap);
    }
    env
}

/// Extract 10 Hz vocal-band (`300 Hz - 3000 Hz`) PCM envelope via `ffmpeg` (`s16le pipe:1`),
/// supporting `.ts.jpg` HLS via `-extension_picky 0` and optional audio stream language selection.
pub async fn extract_vocal_envelope_ffmpeg(
    stream_url: &str,
    referer: Option<&str>,
    start_secs: f64,
    duration_secs: f64,
) -> Result<Vec<f32>, String> {
    extract_vocal_envelope_ffmpeg_for_lang(stream_url, referer, start_secs, duration_secs, None).await
}

pub async fn extract_vocal_envelope_ffmpeg_for_lang(
    stream_url: &str,
    referer: Option<&str>,
    start_secs: f64,
    duration_secs: f64,
    target_lang: Option<&str>,
) -> Result<Vec<f32>, String> {
    let win_start_key = (start_secs.max(0.0) * 10.0).round() as u64;
    let lang_key = target_lang.unwrap_or("default");
    let cache_key = (format!("{stream_url}#{lang_key}"), win_start_key);
    if let Ok(guard) = vocal_env_cache().lock() {
        if let Some(cached) = guard.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    let ffmpeg_cmd = if std::path::Path::new("/opt/homebrew/bin/ffmpeg").exists() {
        "/opt/homebrew/bin/ffmpeg"
    } else if std::path::Path::new("/usr/local/bin/ffmpeg").exists() {
        "/usr/local/bin/ffmpeg"
    } else {
        "ffmpeg"
    };

    let is_http = stream_url.starts_with("http://") || stream_url.starts_with("https://");
    let is_local_proxy = stream_url.starts_with("http://127.0.0.1:");
    let is_hls = stream_url.contains(".m3u8") || stream_url.contains("/m3u8");
    let timeout_secs = if !is_http || is_local_proxy { 10 } else { 22 };
    let headers_val = referer
        .filter(|r| !r.trim().is_empty())
        .map(|r| {
            format!(
                "Referer: {r}\r\nUser-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\r\n"
            )
        });

    let mut map_attempts: Vec<Option<String>> = Vec::new();
    if let Some(lang) = target_lang.filter(|l| !l.trim().is_empty()) {
        map_attempts.push(Some(format!("0:a:m:language:{lang}")));
    }
    map_attempts.push(None);

    let mut last_err = String::from("ffmpeg failed to decode audio");
    'outer: for map_spec in &map_attempts {
        for use_picky_flag in [true, false] {
            if !is_hls && use_picky_flag {
                continue;
            }
            let mut cmd = tokio::process::Command::new(ffmpeg_cmd);
            cmd.arg("-hide_banner").arg("-nostats").arg("-loglevel").arg("error");
            if is_http {
                cmd.args([
                    "-reconnect", "1",
                    "-reconnect_streamed", "1",
                    "-reconnect_delay_max", "2",
                    "-rw_timeout", "10000000",
                    "-analyzeduration", "2000000",
                    "-probesize", "1000000",
                ]);
                if !is_local_proxy {
                    if let Some(ref hdrs) = headers_val {
                        cmd.args(["-headers", hdrs]);
                    }
                }
                if is_hls {
                    if use_picky_flag {
                        cmd.args(["-extension_picky", "0"]);
                    }
                    cmd.args([
                        "-allowed_extensions", "ALL",
                        "-allowed_segment_extensions", "ALL",
                    ]);
                }
            }
            cmd.args([
                "-ss", &format!("{:.1}", start_secs.max(0.0)),
                "-t", &format!("{:.1}", duration_secs.clamp(30.0, 150.0)),
                "-i", stream_url,
            ]);
            if let Some(ref m) = map_spec {
                cmd.args(["-map", m]);
            }
            cmd.args([
                "-vn", "-sn", "-dn",
                "-af", "highpass=f=300,lowpass=f=3000,aresample=8000",
                "-ac", "1",
                "-f", "s16le",
                "pipe:1",
            ]);

            let out = match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    last_err = format!("ffmpeg launch failed: {e}");
                    break 'outer;
                }
                Err(_) => {
                    last_err = format!("Audio analysis timed out after {timeout_secs}s");
                    break;
                }
            };

            let stderr = String::from_utf8_lossy(&out.stderr);
            if use_picky_flag && stderr.contains("Unrecognized option 'extension_picky'") {
                continue;
            }
            if map_spec.is_some() && !out.status.success() {
                break; // Try default untagged audio stream
            }
            let env = pcm_s16le_to_vocal_envelope(&out.stdout);
            if !env.is_empty() {
                if let Ok(mut guard) = vocal_env_cache().lock() {
                    if guard.len() > 32 {
                        guard.clear();
                    }
                    guard.insert(cache_key, env.clone());
                }
                return Ok(env);
            }
            if !stderr.trim().is_empty() {
                last_err = stderr.lines().last().unwrap_or("No audio stream found").trim().to_string();
            } else if out.stdout.is_empty() {
                last_err = "No audio stream decoded from media source".to_string();
            }
            break;
        }
    }

    Err(last_err)
}

/// Select up to 4 dialogue-dense 75s audio windows spanning the episode (avoiding OP/ED gaps).
pub fn select_dialogue_windows(sub_cues: &[(f64, f64)], cur_pos_secs: f64) -> Vec<(f64, f64)> {
    const WIN_DUR: f64 = 75.0;
    let max_cue_t = sub_cues.iter().map(|c| c.1).fold(0.0_f64, f64::max);
    if max_cue_t <= 15.0 {
        return vec![(cur_pos_secs.max(0.0), WIN_DUR)];
    }

    let mut windows = Vec::new();
    let cur_center = cur_pos_secs.max(37.5);
    let cur_start = (cur_center - 37.5).max(0.0);
    let cur_cues = sub_cues
        .iter()
        .filter(|&&(s, e)| e >= cur_start && s <= cur_start + WIN_DUR)
        .count();
    if cur_pos_secs >= 180.0 && cur_cues >= 6 {
        windows.push((cur_start, WIN_DUR));
    }

    let mut scored: Vec<(f64, usize)> = Vec::new();
    let mut t = 180.0_f64;
    let upper = (max_cue_t - 40.0).max(200.0);
    while t <= upper {
        let count = sub_cues
            .iter()
            .filter(|&&(s, e)| e >= t && s <= t + WIN_DUR)
            .count();
        if count >= 5 {
            scored.push((t, count));
        }
        t += 45.0;
    }
    if scored.is_empty() {
        let mut t0 = 0.0_f64;
        while t0 <= upper {
            let count = sub_cues
                .iter()
                .filter(|&&(s, e)| e >= t0 && s <= t0 + WIN_DUR)
                .count();
            if count >= 4 {
                scored.push((t0, count));
            }
            t0 += 45.0;
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    for (cand_start, _) in scored {
        if windows.iter().all(|&(ws, _): &(f64, f64)| (ws - cand_start).abs() >= 180.0) {
            windows.push((cand_start, WIN_DUR));
            if windows.len() >= 4 {
                break;
            }
        }
    }
    if windows.is_empty() {
        windows.push((cur_pos_secs.max(0.0), WIN_DUR));
    }
    windows
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StructuralFitMetrics {
    pub inside_vs_gap_contrast: f64,
    pub onset_hit_rate: f64,
    pub isolated_cues: usize,
    pub duration_corr: f64,
    pub duration_cues: usize,
    pub duration_std: f64,
}

impl StructuralFitMetrics {
    pub fn passes(&self) -> bool {
        if self.inside_vs_gap_contrast < 1.45 {
            return false;
        }
        if self.isolated_cues >= 5 && self.onset_hit_rate < 0.50 {
            return false;
        }
        if self.duration_cues >= 8 && self.duration_std >= 0.45 && self.duration_corr < 0.18 {
            return false;
        }
        true
    }
}

/// Evaluate whether `delta` truly aligns spoken sentences with silent pauses, cue onsets, and cue lengths
/// — rejecting accidental overlaps where speech and unrelated subtitles merely happen to occur simultaneously.
pub fn evaluate_structural_fit(
    win_envs: &[(f64, &[f32])],
    sub_cues: &[(f64, f64)],
    delta: f64,
) -> StructuralFitMetrics {
    const BIN: f64 = 0.10;
    let mut sum_inside = 0.0_f64;
    let mut cnt_inside = 0usize;
    let mut sum_gap = 0.0_f64;
    let mut cnt_gap = 0usize;
    let mut onset_hits = 0usize;
    let mut isolated_cues = 0usize;
    let mut cue_durs: Vec<f64> = Vec::new();
    let mut voice_durs: Vec<f64> = Vec::new();

    for &(win_start, env) in win_envs {
        let n = env.len();
        if n < 80 {
            continue;
        }
        let mut sorted = env.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let p40 = (sorted[(sorted.len() * 40) / 100] as f64).max(0.05);

        let mut inside_mask = vec![false; n];
        let mut near_cue_mask = vec![false; n];
        for &(s, e) in sub_cues {
            let ss = s + delta - win_start;
            let ee = e + delta - win_start;
            if ee < -0.6 || ss > (n as f64) * BIN + 0.6 {
                continue;
            }
            let ib0 = ((ss / BIN).round() as isize).max(0) as usize;
            let ib1 = ((ee / BIN).round() as isize).min(n as isize - 1);
            if ib1 >= ib0 as isize {
                for b in ib0..=(ib1 as usize) {
                    inside_mask[b] = true;
                }
            }
            let nb0 = (((ss - 0.60) / BIN).floor() as isize).max(0) as usize;
            let nb1 = (((ee + 0.60) / BIN).ceil() as isize).min(n as isize - 1);
            if nb1 >= nb0 as isize {
                for b in nb0..=(nb1 as usize) {
                    near_cue_mask[b] = true;
                }
            }
        }

        for b in 0..n {
            if inside_mask[b] {
                sum_inside += env[b] as f64;
                cnt_inside += 1;
            } else if !near_cue_mask[b] {
                sum_gap += env[b] as f64;
                cnt_gap += 1;
            }
        }

        for idx in 0..sub_cues.len() {
            let (s, e) = sub_cues[idx];
            let dur = e - s;
            if !(0.4..=12.0).contains(&dur) {
                continue;
            }
            let ss = s + delta - win_start;
            let ee = e + delta - win_start;
            if ss < 0.7 || ee > (n as f64) * BIN - 0.4 {
                continue;
            }

            let sb = (ss / BIN).round() as isize;
            let eb = (ee / BIN).round() as isize;
            let b0 = (sb - 1).max(0) as usize;
            let b1 = (eb + 1).min(n as isize - 1) as usize;
            let active_bins = (b0..=b1).filter(|&b| env[b] as f64 > p40).count();
            cue_durs.push(dur);
            voice_durs.push(active_bins as f64 * BIN);

            let prev_gap = if idx == 0 {
                10.0
            } else {
                s - sub_cues[idx - 1].1
            };
            if prev_gap >= 0.45 {
                isolated_cues += 1;
                let pre0 = (sb - 6).max(0) as usize;
                let pre1 = (sb - 2).max(0) as usize;
                let on0 = (sb - 1).max(0) as usize;
                let on1 = (sb + 6).min(n as isize - 1) as usize;
                let pre_avg = if pre1 >= pre0 {
                    (pre0..=pre1).map(|b| env[b] as f64).sum::<f64>() / (pre1 - pre0 + 1) as f64
                } else {
                    0.0
                };
                let on_peak = (on0..=on1).map(|b| env[b] as f64).fold(0.0_f64, f64::max);
                if on_peak > pre_avg + 0.08 && on_peak >= p40 {
                    onset_hits += 1;
                }
            }
        }
    }

    let contrast = if cnt_inside >= 15 && cnt_gap >= 15 {
        let m_in = sum_inside / cnt_inside as f64;
        let m_gap = (sum_gap / cnt_gap as f64).max(0.025);
        m_in / m_gap
    } else if cnt_inside >= 15 {
        // Dense scene with few long gaps: fall back to permissive pass on contrast
        1.6
    } else {
        0.0
    };

    let onset_rate = if isolated_cues > 0 {
        onset_hits as f64 / isolated_cues as f64
    } else {
        1.0
    };

    let (dur_corr, dur_std) = if cue_durs.len() >= 8 {
        let nf = cue_durs.len() as f64;
        let mx = cue_durs.iter().sum::<f64>() / nf;
        let my = voice_durs.iter().sum::<f64>() / nf;
        let mut sxx = 0.0_f64;
        let mut syy = 0.0_f64;
        let mut sxy = 0.0_f64;
        for (&x, &y) in cue_durs.iter().zip(voice_durs.iter()) {
            let dx = x - mx;
            let dy = y - my;
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        let std_x = (sxx / nf).sqrt();
        let denom = (sxx * syy).sqrt();
        let r = if denom > 1e-6 { sxy / denom } else { 0.0 };
        (r, std_x)
    } else {
        (1.0, 0.0)
    };

    StructuralFitMetrics {
        inside_vs_gap_contrast: contrast,
        onset_hit_rate: onset_rate,
        isolated_cues,
        duration_corr: dur_corr,
        duration_cues: cue_durs.len(),
        duration_std: dur_std,
    }
}

/// Compute exact zero-mean Pearson normalized cross-correlation over `delta in [-150s..+150s]`
/// alongside its full coarse score curve `(offset_secs, pearson_r)`.
fn correlate_envelope_with_curve(
    audio_env: &[f32],
    win_start: f64,
    sub_cues: &[(f64, f64)],
) -> (SpeechVadSyncResult, Vec<(f64, f64)>) {
    const BIN: f64 = 0.10;
    if audio_env.len() < 150 || sub_cues.len() < 3 {
        return (
            SpeechVadSyncResult {
                reason: "Too few audio frames or subtitle cues".to_string(),
                ..Default::default()
            },
            Vec::new(),
        );
    }

    let n = audio_env.len();
    let n_f = n as f64;
    let a_mean = audio_env.iter().map(|&v| v as f64).sum::<f64>() / n_f;
    let a_std = (audio_env
        .iter()
        .map(|&v| {
            let d = v as f64 - a_mean;
            d * d
        })
        .sum::<f64>()
        / n_f)
        .sqrt()
        .max(1e-6);
    let norm_env: Vec<f64> = audio_env.iter().map(|&v| (v as f64 - a_mean) / a_std).collect();

    let score_offset = |delta: f64| -> f64 {
        let mut mask = vec![0.0_f64; n];
        for &(s, e) in sub_cues {
            let ss = s + delta - win_start;
            let ee = e + delta - win_start;
            if ee < -0.5 || ss > n_f * BIN + 0.5 {
                continue;
            }
            let pre_lo = ((ss - 0.3) / BIN).floor() as isize;
            let pre_hi = (ss / BIN).floor() as isize;
            for b in pre_lo.max(0)..pre_hi.min(n as isize) {
                if mask[b as usize] == 0.0 {
                    mask[b as usize] = -0.5;
                }
            }
            let post_lo = (ee / BIN).ceil() as isize;
            let post_hi = ((ee + 0.3) / BIN).ceil() as isize;
            for b in post_lo.max(0)..post_hi.min(n as isize) {
                if mask[b as usize] == 0.0 {
                    mask[b as usize] = -0.5;
                }
            }
        }
        let mut overlapping = 0usize;
        for &(s, e) in sub_cues {
            let ss = s + delta - win_start;
            let ee = e + delta - win_start;
            if ee < 0.0 || ss > n_f * BIN {
                continue;
            }
            overlapping += 1;
            let sb = (ss / BIN).round() as isize;
            let eb = (ee / BIN).round() as isize;
            let onset_end = sb + 3;
            for b in sb.max(0)..=eb.min(n as isize - 1) {
                let w = if b < onset_end { 2.0 } else { 1.0 };
                if w > mask[b as usize] {
                    mask[b as usize] = w;
                }
            }
        }
        if overlapping < 3 {
            return 0.0;
        }
        let mut sum_m = 0.0_f64;
        let mut sum_mm = 0.0_f64;
        let mut sum_am = 0.0_f64;
        for (&a, &m) in norm_env.iter().zip(mask.iter()) {
            sum_m += m;
            sum_mm += m * m;
            sum_am += a * m;
        }
        let m_mean = sum_m / n_f;
        let m_var = sum_mm - n_f * m_mean * m_mean;
        if m_var <= 1e-4 {
            return 0.0;
        }
        sum_am / (n_f * m_var).sqrt()
    };

    // Coarse scan: -150.0s .. +150.0s in 0.20s steps
    let mut coarse: Vec<(f64, f64)> = Vec::with_capacity(1501);
    for step in -750..=750 {
        let delta = step as f64 * 0.20;
        coarse.push((delta, score_offset(delta)));
    }

    let cn = coarse.len() as f64;
    let c_mean = coarse.iter().map(|&(_, sc)| sc).sum::<f64>() / cn;
    let c_std = (coarse
        .iter()
        .map(|&(_, sc)| {
            let d = sc - c_mean;
            d * d
        })
        .sum::<f64>()
        / cn)
        .sqrt()
        .max(1e-6);

    let mut ranked = coarse.clone();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut top_peaks: Vec<f64> = Vec::new();
    for &(off, _) in &ranked {
        if top_peaks.iter().all(|p| (p - off).abs() >= 3.0) {
            top_peaks.push(off);
            if top_peaks.len() >= 3 {
                break;
            }
        }
    }

    let mut best_offset = 0.0_f64;
    let mut best_score = f64::NEG_INFINITY;
    for peak in &top_peaks {
        for sub_step in -50..=50 {
            let off = (peak + sub_step as f64 * 0.05).clamp(-150.0, 150.0);
            let sc = score_offset(off);
            if sc > best_score {
                best_score = sc;
                best_offset = off;
            }
        }
    }

    let second_peak = ranked
        .iter()
        .find(|&&(off, _)| (off - best_offset).abs() >= 3.0)
        .map(|&(_, sc)| sc)
        .unwrap_or(c_mean);
    let z_score = (best_score - c_mean) / c_std;
    let prominence = if second_peak > 0.0 {
        best_score / second_peak
    } else {
        2.0
    };
    let confidence_pct = (((z_score - 2.2) / 3.2).clamp(0.0, 1.0) * 49.0 + 50.0)
        .round()
        .clamp(0.0, 99.0) as u8;
    let extreme_ok = best_offset.abs() <= 25.0 || z_score >= 3.6;
    let applied = z_score >= 3.2 && best_score >= 0.16 && extreme_ok;
    let reason = if applied {
        format!("Voice-Locked ({confidence_pct}% confidence, Z={z_score:.2})")
    } else {
        format!("Weak voice match (r={best_score:.2}, Z={z_score:.2}) — seek to clear dialogue and retry")
    };

    let res = SpeechVadSyncResult {
        offset_seconds: (best_offset * 100.0).round() / 100.0,
        scale_factor: 1.0,
        confidence_pct: if applied {
            confidence_pct
        } else {
            ((z_score / 3.5).clamp(0.0, 0.49) * 100.0).round() as u8
        },
        z_score,
        prominence,
        applied,
        reason,
    };
    (res, coarse)
}

/// Correlate a 10 Hz vocal modulation envelope over `[win_start, win_start + len*0.1]`
/// against all subtitle cues using zero-mean Pearson NCC (`-150s..+150s`) + structural fit check.
pub fn correlate_envelope_against_cues(
    audio_env: &[f32],
    win_start: f64,
    sub_cues: &[(f64, f64)],
) -> SpeechVadSyncResult {
    let mut res = correlate_envelope_with_curve(audio_env, win_start, sub_cues).0;
    if res.applied {
        let fit = evaluate_structural_fit(&[(win_start, audio_env)], sub_cues, res.offset_seconds);
        if !fit.passes() {
            res.applied = false;
            res.confidence_pct = res.confidence_pct.min(45);
            res.reason = format!(
                "Rejected false overlap (contrast {:.2}x, onset {:.0}%)",
                fit.inside_vs_gap_contrast,
                fit.onset_hit_rate * 100.0
            );
        }
    }
    res
}

/// Cross-correlate two 10 Hz audio envelopes (`ref_env` from Clean sibling, `target_env` from active stream)
/// extracted over the exact same timestamp range, returning `(delta_secs, z_score, confidence_pct)`
/// where `t_target = t_ref + delta_secs`.
pub fn correlate_audio_envelopes(
    ref_env: &[f32],
    target_env: &[f32],
    max_shift_secs: f64,
) -> Option<(f64, f64, u8)> {
    const BIN: f64 = 0.10;
    let n = ref_env.len().min(target_env.len());
    if n < 150 {
        return None;
    }

    let max_lag = ((max_shift_secs / BIN).round() as isize).clamp(20, (n / 3) as isize);
    let mut scores: Vec<(f64, f64)> = Vec::with_capacity((max_lag * 2 + 1) as usize);

    // At delta = lag * 0.1s: target[i + lag] corresponds to ref[i] (target happens delta AFTER ref)
    for lag in -max_lag..=max_lag {
        let (r_start, t_start, overlap) = if lag >= 0 {
            let l = lag as usize;
            (0usize, l, n - l)
        } else {
            let l = (-lag) as usize;
            (l, 0usize, n - l)
        };
        if overlap < 100 {
            continue;
        }
        let ov_f = overlap as f64;
        let mut s_r = 0.0_f64;
        let mut s_t = 0.0_f64;
        for k in 0..overlap {
            s_r += ref_env[r_start + k] as f64;
            s_t += target_env[t_start + k] as f64;
        }
        let m_r = s_r / ov_f;
        let m_t = s_t / ov_f;
        let mut num = 0.0_f64;
        let mut vr = 0.0_f64;
        let mut vt = 0.0_f64;
        for k in 0..overlap {
            let dr = ref_env[r_start + k] as f64 - m_r;
            let dt = target_env[t_start + k] as f64 - m_t;
            num += dr * dt;
            vr += dr * dr;
            vt += dt * dt;
        }
        let denom = (vr * vt).sqrt();
        let r = if denom > 1e-6 { num / denom } else { 0.0 };
        scores.push((lag as f64 * BIN, r));
    }
    if scores.len() < 10 {
        return None;
    }

    let mean = scores.iter().map(|&(_, sc)| sc).sum::<f64>() / scores.len() as f64;
    let var = scores
        .iter()
        .map(|&(_, sc)| {
            let d = sc - mean;
            d * d
        })
        .sum::<f64>()
        / scores.len() as f64;
    let std = var.sqrt().max(1e-6);

    let (best_idx, &(best_off, best_r)) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))?;

    // Parabolic sub-bin interpolation across neighbors [best_idx-1, best_idx, best_idx+1] for ±0.02s precision
    let refined_off = if best_idx > 0 && best_idx + 1 < scores.len() {
        let y0 = scores[best_idx - 1].1;
        let y1 = best_r;
        let y2 = scores[best_idx + 1].1;
        let denom = 2.0 * (y0 - 2.0 * y1 + y2);
        if denom.abs() > 1e-7 {
            let shift = ((y0 - y2) / denom).clamp(-0.5, 0.5);
            best_off + shift * BIN
        } else {
            best_off
        }
    } else {
        best_off
    };

    let z = (best_r - mean) / std;
    if best_r < 0.35 || z < 3.6 {
        return None;
    }
    let conf = (((z - 3.0) / 8.0).clamp(0.0, 1.0) * 29.0 + 70.0)
        .round()
        .clamp(70.0, 99.0) as u8;
    Some(((refined_off * 100.0).round() / 100.0, z, conf))
}

/// Automated 4-window dialogue-clustered `ffmpeg` PCM extraction + structural cadence gate for `SNAP TO VOICE`.
pub async fn auto_sync_subtitle_to_audio(
    stream_url: &str,
    referer: Option<&str>,
    sub_path: &str,
    cur_pos_secs: f64,
) -> SpeechVadSyncResult {
    auto_sync_subtitle_to_audio_for_lang(stream_url, referer, sub_path, cur_pos_secs, None).await
}

pub async fn auto_sync_subtitle_to_audio_for_lang(
    stream_url: &str,
    referer: Option<&str>,
    sub_path: &str,
    cur_pos_secs: f64,
    target_lang: Option<&str>,
) -> SpeechVadSyncResult {
    let sub_text = match std::fs::read_to_string(sub_path) {
        Ok(t) => t,
        Err(_) => {
            return SpeechVadSyncResult {
                reason: "Active subtitle file not cached locally".to_string(),
                ..Default::default()
            };
        }
    };
    let sub_cues = parse_subtitle_intervals(&sub_text);
    if sub_cues.len() < 3 {
        return SpeechVadSyncResult {
            reason: "Too few dialogue cues in subtitle file".to_string(),
            ..Default::default()
        };
    }

    let windows = select_dialogue_windows(&sub_cues, cur_pos_secs);
    let env_futs = windows.iter().map(|&(w_start, w_dur)| {
        extract_vocal_envelope_ffmpeg_for_lang(stream_url, referer, w_start, w_dur, target_lang)
    });
    let env_results = futures_util::future::join_all(env_futs).await;

    struct ParsedWin {
        win_start: f64,
        env: Vec<f32>,
        res: SpeechVadSyncResult,
        curve: Vec<(f64, f64)>,
    }

    let mut parsed_windows: Vec<ParsedWin> = Vec::new();
    let mut last_err = String::from("Audio envelope extraction failed");

    for (&(w_start, _), env_res) in windows.iter().zip(env_results.into_iter()) {
        match env_res {
            Ok(env) => {
                let (res, curve) = correlate_envelope_with_curve(&env, w_start, &sub_cues);
                parsed_windows.push(ParsedWin {
                    win_start: w_start,
                    env,
                    res,
                    curve,
                });
            }
            Err(err) => {
                last_err = err;
            }
        }
    }

    if parsed_windows.is_empty() {
        return SpeechVadSyncResult {
            reason: last_err,
            ..Default::default()
        };
    }

    // Strict Multi-Window Consensus (NO <= 25s disagreement bypass!)
    if parsed_windows.len() >= 2 {
        let base_len = parsed_windows[0].curve.len();
        if base_len > 100 && parsed_windows.iter().all(|w| w.curve.len() == base_len) {
            let mut joint: Vec<(f64, f64)> = Vec::with_capacity(base_len);
            let w_cnt = parsed_windows.len() as f64;
            for bi in 0..base_len {
                let off = parsed_windows[0].curve[bi].0;
                let avg_r = parsed_windows.iter().map(|w| w.curve[bi].1).sum::<f64>() / w_cnt;
                joint.push((off, avg_r));
            }
            let j_mean = joint.iter().map(|&(_, s)| s).sum::<f64>() / joint.len() as f64;
            let j_std = (joint
                .iter()
                .map(|&(_, s)| {
                    let d = s - j_mean;
                    d * d
                })
                .sum::<f64>()
                / joint.len() as f64)
                .sqrt()
                .max(1e-6);

            if let Some(&(best_off, best_j)) = joint.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                let j_z = (best_j - j_mean) / j_std;
                let best_bin_idx = ((best_off / 0.20).round() as isize + 750)
                    .clamp(0, base_len as isize - 1) as usize;
                let agreeing_wins: Vec<&ParsedWin> = parsed_windows
                    .iter()
                    .filter(|w| {
                        let r_at = w.curve[best_bin_idx].1;
                        let max_w = w.curve.iter().map(|&(_, r)| r).fold(0.0_f64, f64::max);
                        (w.res.offset_seconds - best_off).abs() <= 0.85
                            || (r_at >= 0.18 && max_w > 0.10 && r_at >= max_w * 0.72)
                    })
                    .collect();

                if agreeing_wins.len() >= 2 && j_z >= 3.2 && best_j >= 0.20 {
                    let env_refs: Vec<(f64, &[f32])> = agreeing_wins
                        .iter()
                        .map(|w| (w.win_start, w.env.as_slice()))
                        .collect();
                    let fit = evaluate_structural_fit(&env_refs, &sub_cues, best_off);
                    if fit.passes() {
                        let r_term = ((best_j - 0.18) / 0.42).clamp(0.0, 1.0);
                        let c_term = ((fit.inside_vs_gap_contrast - 1.45) / 2.2).clamp(0.0, 1.0);
                        let o_term = ((fit.onset_hit_rate - 0.45) / 0.45).clamp(0.0, 1.0);
                        let w_term = (agreeing_wins.len() as f64 / parsed_windows.len() as f64).clamp(0.0, 1.0);
                        let conf_f = 55.0 + 44.0 * (0.35 * r_term + 0.25 * c_term + 0.25 * o_term + 0.15 * w_term);
                        let conf = conf_f.round().clamp(55.0, 99.0) as u8;
                        return SpeechVadSyncResult {
                            offset_seconds: (best_off * 100.0).round() / 100.0,
                            scale_factor: 1.0,
                            confidence_pct: conf,
                            z_score: j_z,
                            prominence: 1.6,
                            applied: true,
                            reason: format!(
                                "Multi-Scene Voice Lock ({conf}%, {}/{} scenes, contrast {:.1}x)",
                                agreeing_wins.len(),
                                parsed_windows.len(),
                                fit.inside_vs_gap_contrast
                            ),
                        };
                    }
                    return SpeechVadSyncResult {
                        offset_seconds: 0.0,
                        scale_factor: 1.0,
                        confidence_pct: 38,
                        z_score: j_z,
                        prominence: 1.0,
                        applied: false,
                        reason: format!(
                            "Rejected accidental overlap (contrast {:.2}x, onset {:.0}%) — subtitles may not match cut",
                            fit.inside_vs_gap_contrast,
                            fit.onset_hit_rate * 100.0
                        ),
                    };
                }

                // Mid-episode commercial cut detection: lock to the window closest to cur_pos_secs ONLY if clean & strong
                if let Some(local_w) = parsed_windows
                    .iter()
                    .min_by(|a, b| (a.win_start + 37.5 - cur_pos_secs).abs().total_cmp(&(b.win_start + 37.5 - cur_pos_secs).abs()))
                {
                    if local_w.res.applied && local_w.res.z_score >= 3.8 {
                        let fit = evaluate_structural_fit(
                            &[(local_w.win_start, local_w.env.as_slice())],
                            &sub_cues,
                            local_w.res.offset_seconds,
                        );
                        if fit.passes() && fit.inside_vs_gap_contrast >= 1.75 {
                            return SpeechVadSyncResult {
                                offset_seconds: local_w.res.offset_seconds,
                                scale_factor: 1.0,
                                confidence_pct: local_w.res.confidence_pct.clamp(60, 88),
                                z_score: local_w.res.z_score,
                                prominence: local_w.res.prominence,
                                applied: true,
                                reason: format!(
                                    "Scene-Locked {:+.2}s (mid-ep cut differs across scenes)",
                                    local_w.res.offset_seconds
                                ),
                            };
                        }
                    }
                }

                return SpeechVadSyncResult {
                    offset_seconds: 0.0,
                    scale_factor: 1.0,
                    confidence_pct: 30,
                    z_score: j_z,
                    prominence: 1.0,
                    applied: false,
                    reason: format!(
                        "No cross-scene dialogue consensus (0/{} scenes agreed) — adjust manually or check sub file",
                        parsed_windows.len()
                    ),
                };
            }
        }
    }

    // Single-window strict fallback with structural verification gate
    let best = parsed_windows
        .into_iter()
        .max_by(|a, b| a.res.z_score.total_cmp(&b.res.z_score));
    if let Some(w) = best {
        let mut r = w.res;
        if r.applied {
            let fit = evaluate_structural_fit(&[(w.win_start, w.env.as_slice())], &sub_cues, r.offset_seconds);
            if !fit.passes() || fit.inside_vs_gap_contrast < 1.65 {
                r.applied = false;
                r.confidence_pct = r.confidence_pct.min(42);
                r.reason = format!(
                    "Rejected single-scene false overlap (contrast {:.2}x, onset {:.0}%)",
                    fit.inside_vs_gap_contrast,
                    fit.onset_hit_rate * 100.0
                );
            }
        }
        return r;
    }

    SpeechVadSyncResult {
        reason: last_err,
        ..Default::default()
    }
}

/// Unified Tier-A + Tier-B Synchronizer v4:
/// 1. Multi-window Audio-to-Audio waveform lock against a Clean sibling provider (±90s, non-OP windows),
///    composed cleanly with verified Tier-1 provider-native subtitle alignment.
/// 2. Falls back to 4-Window Cadence + Structural-Fit Tier-B Speech-VAD v4.
pub async fn sync_subtitle_with_reference_or_vad(
    client: &reqwest::Client,
    target_stream_url: &str,
    target_referer: Option<&str>,
    target_sub_path: &str,
    cur_pos_secs: f64,
    ref_stream_url: Option<&str>,
    ref_referer: Option<&str>,
    ref_sub_url: Option<&str>,
) -> SpeechVadSyncResult {
    sync_subtitle_with_reference_or_vad_for_lang(
        client,
        target_stream_url,
        target_referer,
        target_sub_path,
        cur_pos_secs,
        ref_stream_url,
        ref_referer,
        ref_sub_url,
        None,
    )
    .await
}

pub async fn sync_subtitle_with_reference_or_vad_for_lang(
    client: &reqwest::Client,
    target_stream_url: &str,
    target_referer: Option<&str>,
    target_sub_path: &str,
    cur_pos_secs: f64,
    ref_stream_url: Option<&str>,
    ref_referer: Option<&str>,
    ref_sub_url: Option<&str>,
    target_lang: Option<&str>,
) -> SpeechVadSyncResult {
    if let Some(r_url) = ref_stream_url.filter(|u| !u.is_empty() && *u != target_stream_url) {
        let sub_text = std::fs::read_to_string(target_sub_path).unwrap_or_default();
        let cues = parse_subtitle_intervals(&sub_text);
        let max_t = cues.iter().map(|c| c.1).fold(0.0_f64, f64::max);

        // Two widely spaced non-OP windows when episode length permits
        let w1_start = if max_t > 450.0 { 205.0 } else if max_t > 240.0 { 135.0 } else { 20.0 };
        let has_w2 = max_t > 680.0;
        let w2_start = (max_t * 0.60).clamp(440.0, (max_t - 110.0).max(440.0));

        let ((r1, t1), (r2, t2)) = if has_w2 {
            let (a, b, c, d) = tokio::join!(
                extract_vocal_envelope_ffmpeg_for_lang(r_url, ref_referer, w1_start, 65.0, target_lang),
                extract_vocal_envelope_ffmpeg_for_lang(target_stream_url, target_referer, w1_start, 65.0, target_lang),
                extract_vocal_envelope_ffmpeg_for_lang(r_url, ref_referer, w2_start, 65.0, target_lang),
                extract_vocal_envelope_ffmpeg_for_lang(target_stream_url, target_referer, w2_start, 65.0, target_lang),
            );
            ((a, b), (c, d))
        } else {
            let (a, b) = tokio::join!(
                extract_vocal_envelope_ffmpeg_for_lang(r_url, ref_referer, w1_start, 65.0, target_lang),
                extract_vocal_envelope_ffmpeg_for_lang(target_stream_url, target_referer, w1_start, 65.0, target_lang),
            );
            ((a, b), (Err(String::new()), Err(String::new())))
        };

        let c1 = match (r1, t1) {
            (Ok(re), Ok(te)) => correlate_audio_envelopes(&re, &te, 90.0),
            _ => None,
        };
        let c2 = match (r2, t2) {
            (Ok(re), Ok(te)) => correlate_audio_envelopes(&re, &te, 90.0),
            _ => None,
        };

        let waveform_lock = match (c1, c2) {
            (Some((d1, z1, _)), Some((d2, z2, _))) if (d1 - d2).abs() <= 0.45 => {
                let z = 0.5 * (z1 + z2);
                let conf = (((z - 3.0) / 7.5).clamp(0.0, 1.0) * 24.0 + 75.0).round().clamp(75.0, 99.0) as u8;
                Some((0.5 * (d1 + d2), z, conf))
            }
            (Some((d, z, conf)), None) | (None, Some((d, z, conf))) if z >= 4.4 => Some((d, z, conf)),
            (Some((d1, z1, conf1)), Some((d2, z2, conf2))) => {
                if z1 >= z2 + 1.2 && z1 >= 4.8 {
                    Some((d1, z1, conf1))
                } else if z2 >= z1 + 1.2 && z2 >= 4.8 {
                    Some((d2, z2, conf2))
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some((delta_video, z, conf)) = waveform_lock {
            let mut delta_sub_vs_ref = 0.0_f64;
            if let Some(rs_url) = ref_sub_url.filter(|u| !u.is_empty() && *u != target_sub_path) {
                if let Ok(resp) = client
                    .get(rs_url)
                    .header("Referer", ref_referer.unwrap_or("https://zokoanime.video/"))
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            let ref_txt = decode_subtitle_bytes(&bytes);
                            if is_valid_subtitle_content(&ref_txt) {
                                let ref_cues = parse_subtitle_intervals(&ref_txt);
                                let align = calculate_optimal_alignment(&ref_cues, &cues);
                                if align.applied {
                                    delta_sub_vs_ref = align.offset_seconds;
                                }
                            }
                        }
                    }
                }
            }
            let total_offset = ((delta_video + delta_sub_vs_ref) * 100.0).round() / 100.0;
            return SpeechVadSyncResult {
                offset_seconds: total_offset,
                scale_factor: 1.0,
                confidence_pct: conf,
                z_score: z,
                prominence: 2.0,
                applied: true,
                reason: format!("Reference-Audio Locked ({conf}% confidence, Z={z:.2})"),
            };
        }
    }

    auto_sync_subtitle_to_audio_for_lang(target_stream_url, target_referer, target_sub_path, cur_pos_secs, target_lang).await
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

fn url_fnv1a_hash8(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", ((h ^ (h >> 32)) & 0xffff_ffff) as u32)
}

/// Ensure that target subtitle is downloaded, verified, and aligned against a verified provider reference subtitle.
/// Scopes cache filenames by `(anime_id, episode, code, provider_id, url_hash)` to prevent cross-provider pollution.
pub async fn ensure_synchronized_subtitle(
    client: &reqwest::Client,
    target_sub_url: &str,
    ref_sub_url: Option<&str>,
    stream_url: Option<&str>,
    anime_id: i64,
    episode: u32,
    code: &str,
) -> (String, Option<SyncResult>) {
    ensure_synchronized_subtitle_for_provider(
        client,
        target_sub_url,
        ref_sub_url,
        stream_url,
        anime_id,
        episode,
        code,
        None,
    )
    .await
}

pub async fn ensure_synchronized_subtitle_for_provider(
    client: &reqwest::Client,
    target_sub_url: &str,
    ref_sub_url: Option<&str>,
    _stream_url: Option<&str>,
    anime_id: i64,
    episode: u32,
    code: &str,
    provider_id: Option<&str>,
) -> (String, Option<SyncResult>) {
    if !target_sub_url.starts_with("http://") && !target_sub_url.starts_with("https://") {
        return (target_sub_url.to_string(), None);
    }

    let cache_dir = crate::config::Config::cache_dir()
        .map(|dir| dir.join("subs"))
        .unwrap_or_else(|_| std::env::temp_dir().join("solus-subs"));

    if std::fs::create_dir_all(&cache_dir).is_err() {
        return (target_sub_url.to_string(), None);
    }

    let clean_code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let code_name = if clean_code.is_empty() { "sub" } else { &clean_code };
    let prov_slug: String = provider_id
        .unwrap_or("def")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let prov_part = if prov_slug.is_empty() { "def" } else { &prov_slug };
    let url_hash = url_fnv1a_hash8(target_sub_url);

    // Check if valid provider/URL-scoped cached version already exists (.ass, .srt, .vtt)
    for ext in ["ass", "srt", "vtt"] {
        let p = cache_dir.join(format!(
            "{}_{}_{}_{}_{}_synced.{}",
            anime_id, episode, code_name, prov_part, url_hash, ext
        ));
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

    // 1. Download target subtitle content (retry without Zoko Referer on 401/403 from external CDNs)
    let mut target_resp = client
        .get(target_sub_url)
        .header("Referer", "https://zokoanime.video/")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(8))
        .send()
        .await;
    if target_resp.as_ref().is_ok_and(|r| !r.status().is_success()) || target_resp.is_err() {
        target_resp = client
            .get(target_sub_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(8))
            .send()
            .await;
    }
    let target_resp = match target_resp {
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

    let final_content = crate::services::substyler::apply_signage_collision_avoidance(&final_content);

    // Determine appropriate extension (.ass, .vtt, or .srt)
    let first_non_empty = final_content
        .trim_start_matches('\u{feff}')
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(str::trim)
        .unwrap_or("");
    let ext = if final_content.contains("[Script Info]") || final_content.contains("Dialogue:") {
        "ass"
    } else if first_non_empty.starts_with("WEBVTT") {
        "vtt"
    } else {
        "srt"
    };
    let synced_path = cache_dir.join(format!(
        "{}_{}_{}_{}_{}_synced.{}",
        anime_id, episode, code_name, prov_part, url_hash, ext
    ));

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
    fn test_onset_weighted_alignment_catches_subtle_1_2s_lag() {
        // Simulates NieR:Automata Ver1.1a Cour 2 subtle 1.2s slow subtitle lag
        let ref_intervals = vec![
            (10.0, 14.0),
            (22.0, 26.2),
            (38.5, 42.0),
            (55.0, 59.5),
            (72.0, 76.0),
        ];
        let target_intervals: Vec<(f64, f64)> = ref_intervals
            .iter()
            .map(|&(s, e)| (s + 1.2, e + 1.2))
            .collect();

        let result = calculate_optimal_alignment(&ref_intervals, &target_intervals);
        assert!(result.applied, "Subtle 1.2s lag must not be swallowed by Identity Guard");
        assert!((result.offset_seconds + 1.2).abs() <= 0.15);
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
    fn test_parse_subtitle_intervals_filters_ass_karaoke_and_typesetting() {
        let ass = r#"[Script Info]
Title: NHK Sample
[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:07.15,0:00:07.19,Hit the Road,,0,0,0,,{\bord1.5}Karaoke Frame 1
Dialogue: 0,0:00:07.19,0:00:07.23,Hit the Road,,0,0,0,,{\bord1.5}Karaoke Frame 2
Dialogue: 0,0:01:32.64,0:01:34.44,NHK Dialogue,,0,0,0,,Actual Spoken Dialogue 1
Dialogue: 0,0:01:37.75,0:01:40.91,NHK Dialogue,,0,0,0,,Actual Spoken Dialogue 2
Dialogue: 0,0:01:37.75,0:01:40.91,NHK Dialogue - Border,,0,0,0,,Outline duplicate
"#;
        let intervals = parse_subtitle_intervals(ass);
        // Only the 2 actual spoken dialogue lines should remain!
        assert_eq!(intervals.len(), 2);
        assert!((intervals[0].0 - 92.64).abs() < 1e-2);
        assert!((intervals[1].0 - 97.75).abs() < 1e-2);
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

    #[test]
    fn test_speech_vad_alignment_recovers_large_positive_and_negative_offsets() {
        let base_cues = vec![
            (10.0, 12.2),
            (15.5, 18.0),
            (24.0, 27.5),
            (31.2, 33.4),
            (42.0, 45.1),
            (53.5, 56.0),
            (64.0, 66.8),
        ];

        // Case 1: Audio happens 18.40s AFTER subtitle timestamps -> mpv sub-delay = +18.40s
        let audio_late: Vec<(f64, f64)> = base_cues.iter().map(|&(s, e)| (s + 18.40, e + 18.40)).collect();
        let res_pos = calculate_speech_vad_alignment(&audio_late, &base_cues);
        assert!(res_pos.applied, "Expected confident match, got {:?}", res_pos);
        assert!((res_pos.offset_seconds - 18.40).abs() <= 0.15, "got {}", res_pos.offset_seconds);
        assert!(res_pos.z_score >= 2.4);

        // Case 2: Audio happens 32.70s BEFORE subtitle timestamps -> mpv sub-delay = -32.70s
        let shifted_subs: Vec<(f64, f64)> = base_cues.iter().map(|&(s, e)| (s + 50.0, e + 50.0)).collect();
        let audio_early: Vec<(f64, f64)> = shifted_subs.iter().map(|&(s, e)| (s - 32.70, e - 32.70)).collect();
        let res_neg = calculate_speech_vad_alignment(&audio_early, &shifted_subs);
        assert!(res_neg.applied, "Expected negative match, got {:?}", res_neg);
        assert!((res_neg.offset_seconds + 32.70).abs() <= 0.15, "got {}", res_neg.offset_seconds);
    }

    #[test]
    fn test_parse_silencedetect_speech_intervals() {
        let stderr = "\
[silencedetect @ 0x1] silence_start: 0\n\
[silencedetect @ 0x1] silence_end: 4.5 | silence_duration: 4.5\n\
[silencedetect @ 0x1] silence_start: 7.2\n\
[silencedetect @ 0x1] silence_end: 10.0 | silence_duration: 2.8\n";
        let speech = parse_silencedetect_speech_intervals(stderr, 120.0, 15.0);
        assert_eq!(speech.len(), 2);
        assert!((speech[0].0 - 124.5).abs() < 0.01);
        assert!((speech[0].1 - 127.2).abs() < 0.01);
        assert!((speech[1].0 - 130.0).abs() < 0.01);
        assert!((speech[1].1 - 135.0).abs() < 0.01);
    }

    #[test]
    fn test_pcm_envelope_and_correlator_recovers_shift_under_bgm() {
        // Synthesize 90s of 8 kHz s16le PCM: continuous steady BGM + strong vocal-band bursts
        // shifted by -14.5s relative to subtitle cues (so audio occurs 14.5s BEFORE sub -> offset = -14.5s).
        let sample_rate = 8000usize;
        let total_secs = 100usize;
        let win_start = 180.0_f64;
        let true_offset = -14.5_f64;

        let sub_cues = vec![
            (198.0, 200.5),
            (203.0, 206.0),
            (211.0, 214.2),
            (219.5, 222.0),
            (227.0, 230.5),
            (236.0, 239.0),
            (245.5, 249.0),
            (256.0, 259.5),
            (265.0, 268.0),
        ];

        let mut pcm = Vec::with_capacity(sample_rate * total_secs * 2);
        for i in 0..(sample_rate * total_secs) {
            let t = win_start + (i as f64 / sample_rate as f64);
            // Constant BGM floor (~2500 amplitude)
            let bgm = 2500.0 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            // Speech bursts at t_audio = t_sub + true_offset
            let in_speech = sub_cues
                .iter()
                .any(|&(s, e)| t >= s + true_offset && t <= e + true_offset);
            let voice = if in_speech {
                12000.0 * (2.0 * std::f64::consts::PI * 950.0 * t).sin()
            } else {
                0.0
            };
            let sample = (bgm + voice).clamp(-32767.0, 32767.0) as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }

        let env = pcm_s16le_to_vocal_envelope(&pcm);
        assert!(env.len() >= 900);
        let res = correlate_envelope_against_cues(&env, win_start, &sub_cues);
        assert!(res.applied, "Expected confident BGM-cancelled lock, got {:?}", res);
        assert!(
            (res.offset_seconds - true_offset).abs() <= 0.25,
            "Expected ~{}, got {}",
            true_offset,
            res.offset_seconds
        );
    }

    #[test]
    fn test_select_dialogue_windows_skips_op_gap() {
        let mut cues = vec![
            (10.0, 14.0),
            (20.0, 25.0),
            (35.0, 39.0),
            (50.0, 54.0),
        ];
        // Silent OP gap from 55s to 195s, then dense dialogue 195s..320s
        for k in 0..16 {
            let s = 198.0 + k as f64 * 6.5;
            cues.push((s, s + 3.0));
        }
        let wins = select_dialogue_windows(&cues, 140.0);
        assert!(!wins.is_empty());
        assert!(
            wins[0].0 >= 180.0,
            "Window should skip 140s OP gap and land in post-OP dialogue, got {:?}",
            wins[0]
        );
    }

    #[test]
    fn test_pearson_ncc_rejects_cue_density_bias_and_prevents_false_52s_spike() {
        // Constant/noisy background envelope across 90s + 3 real speech bursts near t=185, 200, 215
        let win_start = 175.0;
        let mut env = vec![0.45_f32; 900];
        for b in (100..150).chain(250..300).chain(400..450) {
            env[b] = 1.65;
        }
        // Dense cue cluster 53s earlier (would inflate naive sum-over-cues near +53s)
        let mut cues = Vec::new();
        for k in 0..15 {
            let s = 122.0 + k as f64 * 2.1;
            cues.push((s, s + 1.4));
        }
        // True dialogue cues near offset 0.0
        cues.extend([(185.0, 190.0), (200.0, 205.0), (215.0, 220.0)]);

        let res = correlate_envelope_against_cues(&env, win_start, &cues);
        assert!(
            !res.applied || res.offset_seconds.abs() < 1.0,
            "Pearson NCC must reject dense +53s false lock, got {:?}",
            res
        );
    }

    #[test]
    fn test_audio_to_audio_flux_correlator_recovers_bumper_offset() {
        let len = 900;
        let true_bins: usize = 125; // +12.50s studio bumper delay on target stream
        let mut ref_env = vec![0.0_f32; len];
        for i in 0..len {
            // Non-periodic multi-syllable speech energy pattern
            let s = ((i as f64 * 0.17).sin() * (i as f64 * 0.43).cos()).max(0.0);
            let gate = if (i / 17) % 3 == 0 { 1.0 } else { 0.1 };
            ref_env[i] = (s * gate) as f32;
        }
        let mut target_env = vec![0.05_f32; len];
        for i in true_bins..len {
            target_env[i] = 0.85 * ref_env[i - true_bins] + 0.02 * ((i % 7) as f32 * 0.1);
        }

        let got = correlate_audio_envelopes(&ref_env, &target_env, 60.0);
        assert!(got.is_some(), "Expected confident audio-to-audio lock");
        let (offset, corr, conf) = got.unwrap();
        assert!(
            (offset - 12.5).abs() <= 0.25,
            "Expected ~+12.5s offset, got {} (corr={}, conf={})",
            offset,
            corr,
            conf
        );
        assert!(corr >= 0.70, "Expected strong correlation, got {}", corr);
        assert!(conf >= 75, "Expected high confidence, got {}", conf);
    }

    #[test]
    fn test_structural_fit_rejects_accidental_vocal_overlap_and_duration_mismatch() {
        // Simulates "vocal happened in the same window, but subtitle doesn't match the spoken lines":
        // Audio keeps speaking through supposed silent subtitle gaps AND cue lengths don't track voice bursts.
        let win_start = 200.0;
        let mut env = vec![0.95_f32; 750]; // continuous chatter across gaps too
        let mut cues = Vec::new();
        for k in 0..10 {
            let s = 204.0 + k as f64 * 6.8;
            let dur = if k % 2 == 0 { 0.8 } else { 4.2 };
            // Invert active speech duration (short burst during long cue, loud gap during short cue)
            let sb = ((s - win_start) * 10.0) as usize;
            let eb = ((s + dur - win_start) * 10.0) as usize;
            for b in sb..eb.min(750) {
                env[b] = if k % 2 == 0 { 1.3 } else { 0.1 };
            }
            cues.push((s, s + dur));
        }

        let fit = evaluate_structural_fit(&[(win_start, env.as_slice())], &cues, 0.0);
        assert!(
            !fit.passes(),
            "Accidental vocal overlap must be rejected cleanly by structural gate, got {:?}",
            fit
        );
    }

    #[test]
    fn test_structural_fit_accepts_matched_dialogue_pauses_and_durations() {
        let win_start = 200.0;
        let mut env = vec![0.0_f32; 750];
        let mut cues = Vec::new();
        for k in 0..10 {
            let s = 205.0 + k as f64 * 6.8;
            let dur = if k % 2 == 0 { 0.9 } else { 3.8 };
            let sb = ((s - win_start) * 10.0) as usize;
            let eb = ((s + dur - win_start) * 10.0) as usize;
            for b in sb..eb.min(750) {
                env[b] = 1.4;
            }
            cues.push((s, s + dur));
        }

        let fit = evaluate_structural_fit(&[(win_start, env.as_slice())], &cues, 0.0);
        assert!(fit.passes(), "Genuine voice-to-cue match must pass structural fit: {:?}", fit);
        assert!(fit.inside_vs_gap_contrast >= 2.0);
        assert!(fit.onset_hit_rate >= 0.80);
        assert!(fit.duration_corr >= 0.50);
    }
}
