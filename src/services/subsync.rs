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

/// Automated dual-window `ffmpeg` speech-band extraction + wide cross-correlation for `SNAP TO VOICE`.
pub async fn auto_sync_subtitle_to_audio(
    stream_url: &str,
    referer: Option<&str>,
    sub_path: &str,
    cur_pos_secs: f64,
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

    // Window A centered around the user's current playback timestamp
    let w1_start = (cur_pos_secs - 45.0).max(0.0);
    let mut speech = extract_speech_intervals_ffmpeg(stream_url, referer, w1_start, 90.0).await;

    let mut res = calculate_speech_vad_alignment(&speech, &sub_cues);
    if !res.applied || speech.len() < 6 {
        // Window B shifts past typical 90s OP song to lock onto dense main-episode dialogue
        let w2_start = if w1_start < 90.0 { 130.0 } else { (w1_start + 95.0).min(1200.0) };
        let speech_b = extract_speech_intervals_ffmpeg(stream_url, referer, w2_start, 90.0).await;
        if !speech_b.is_empty() {
            speech.extend(speech_b);
            res = calculate_speech_vad_alignment(&speech, &sub_cues);
        }
    }

    res
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

/// Ensure that target subtitle is downloaded, verified, and synchronized against reference subtitle
/// or Tier-2 audio speech onset (`ffmpeg silencedetect`).
/// Writes the verified/aligned subtitle to `~/.cache/solus/subs/{anime_id}_{ep}_{code}_synced.{ext}`
/// and returns the verified local path (so in-player `⚡ SNAP TO VOICE` can also inspect cue timings).
pub async fn ensure_synchronized_subtitle(
    client: &reqwest::Client,
    target_sub_url: &str,
    ref_sub_url: Option<&str>,
    stream_url: Option<&str>,
    anime_id: i64,
    episode: u32,
    code: &str,
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

    let final_content = crate::services::substyler::apply_signage_collision_avoidance(&final_content);

    // Tier 2 fallback: If no reference subtitle aligned this track, check audio lead onset for bumper shifts
    let mut final_content = final_content;
    if sync_res.as_ref().is_none_or(|r| !r.applied) {
        if let Some(s_url) = stream_url {
            let target_intervals = parse_subtitle_intervals(&final_content);
            if let Some(&first_sub) = target_intervals.iter().map(|i| &i.0).min_by(|a, b| a.total_cmp(b)) {
                if let Some(audio_lead) = estimate_audio_lead_time(s_url).await {
                    let diff = audio_lead - first_sub;
                    if (0.40..=4.5).contains(&diff.abs()) && first_sub < 25.0 {
                        let rounded = (diff * 100.0).round() / 100.0;
                        final_content = apply_subtitle_shift(&final_content, rounded, 1.0);
                        sync_res = Some(SyncResult {
                            offset_seconds: rounded,
                            scale_factor: 1.0,
                            score: 100,
                            applied: true,
                        });
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
}
