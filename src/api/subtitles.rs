//! Subtitle and video stream detection module.
//! Dynamically fetches verified stream resolutions, master playlists, and subtitle tracks for a specific anime episode.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const XOR_KEY: [u8; 14] = [111, 116, 97, 107, 117, 45, 101, 109, 98, 101, 100, 45, 118, 49]; // "otaku-embed-v1"
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QualityOption {
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub tbr_kbps: Option<u32>,
    #[serde(default)]
    pub fps_x10: Option<u32>,
}

impl QualityOption {
    pub fn simple(label: impl Into<String>, url: impl Into<String>) -> Self {
        let label = label.into();
        let height = parse_height_from_label(&label);
        Self {
            label,
            url: url.into(),
            height,
            tbr_kbps: None,
            fps_x10: None,
        }
    }

    pub fn display_with_bitrate(&self) -> String {
        if let Some(tbr) = self.tbr_kbps.filter(|&k| k > 0) {
            if tbr >= 1000 {
                format!("{} · {:.1}M", self.label, tbr as f64 / 1000.0)
            } else {
                format!("{} · {}k", self.label, tbr)
            }
        } else {
            self.label.clone()
        }
    }
}

/// Video variant parsed from an HLS `#EXT-X-STREAM-INF` master playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedVideoVariant {
    pub label: String,
    pub url: String,
    pub height: u32,
    pub fps_x10: u32,
    pub tbr_kbps: u32,
    pub vcodec: Option<String>,
    pub audio_group: Option<String>,
}

/// Audio rendition parsed from an HLS `#EXT-X-MEDIA:TYPE=AUDIO` directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAudioVariant {
    pub group_id: String,
    pub code: String,
    pub name: String,
    pub uri: Option<String>,
    pub is_default: bool,
}

/// Pre-resolved HLS variant ladder cached per `(anime_id, episode, is_dub)` for instant Auto-ABR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResolvedStreamLadder {
    pub anime_id: i64,
    pub episode: u32,
    pub is_dub: bool,
    pub master_url: Option<String>,
    pub resolved_at_unix: i64,
    pub video_variants: Vec<ResolvedVideoVariant>,
    pub audio_variants: Vec<ResolvedAudioVariant>,
}

pub fn parse_height_from_label(label: &str) -> Option<u32> {
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

pub fn estimate_tbr_kbps_for_height(height: u32) -> u32 {
    if height >= 1080 {
        3200
    } else if height >= 720 {
        1600
    } else if height >= 480 {
        850
    } else {
        500
    }
}

pub fn parse_url_expire_unix(url: &str) -> Option<i64> {
    let query = url.split('?').nth(1)?;
    let query = query.split('#').next().unwrap_or(query);
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?.trim().to_ascii_lowercase();
        let val = parts.next().unwrap_or("").trim();
        if matches!(key.as_str(), "exp" | "expire" | "expires" | "e") {
            if let Ok(ts) = val.parse::<i64>() {
                if ts > 1_600_000_000 {
                    return Some(ts);
                }
            }
        }
    }
    None
}

fn codec_efficiency_rank(vcodec: Option<&str>) -> u8 {
    let c = vcodec.unwrap_or("").to_ascii_lowercase();
    if c.contains("av01") || c.contains("av1") {
        0
    } else if c.contains("vp9") || c.contains("vp09") || c.contains("hev1") || c.contains("hvc1") {
        1
    } else if c.contains("avc1") || c.contains("h264") {
        2
    } else {
        3
    }
}

impl ResolvedStreamLadder {
    pub fn is_fresh(&self, now_unix: i64) -> bool {
        if self.video_variants.is_empty() {
            return false;
        }
        if now_unix.saturating_sub(self.resolved_at_unix) >= 600 {
            return false;
        }
        for v in &self.video_variants {
            if let Some(exp) = parse_url_expire_unix(&v.url) {
                if exp - now_unix <= 120 {
                    return false;
                }
            }
        }
        if let Some(ref m) = self.master_url {
            if let Some(exp) = parse_url_expire_unix(m) {
                if exp - now_unix <= 120 {
                    return false;
                }
            }
        }
        true
    }

    pub fn audio_uri_for(&self, variant: &ResolvedVideoVariant, is_dub: bool) -> Option<String> {
        if let Some(ref group) = variant.audio_group {
            let group_matches: Vec<&ResolvedAudioVariant> = self
                .audio_variants
                .iter()
                .filter(|a| a.group_id == *group && a.uri.is_some())
                .collect();
            let chosen = if is_dub {
                group_matches
                    .iter()
                    .find(|a| a.code.starts_with("eng") || a.code == "en")
                    .or_else(|| group_matches.iter().find(|a| a.is_default))
                    .or_else(|| group_matches.first())
            } else {
                group_matches
                    .iter()
                    .find(|a| a.code.starts_with("jpn") || a.code == "ja")
                    .or_else(|| group_matches.iter().find(|a| a.is_default))
                    .or_else(|| group_matches.first())
            };
            if let Some(uri) = chosen.and_then(|a| a.uri.clone()) {
                return Some(uri);
            }
        }
        if let Some(pos) = variant.url.rfind("/index.m3u8") {
            let prefix = &variant.url[..pos];
            if let Some(parent_end) = prefix.rfind('/') {
                return Some(format!("{}/master.m3u8", &prefix[..parent_end]));
            }
        }
        None
    }

    pub fn select_best_variant(
        &self,
        preferred_height: Option<u32>,
        mode: crate::config::NetworkMode,
    ) -> Option<&ResolvedVideoVariant> {
        if self.video_variants.is_empty() {
            return None;
        }
        let effective_cap = match (preferred_height, mode.max_height_cap()) {
            (Some(p), Some(m)) => Some(p.min(m)),
            (Some(p), None) => Some(p),
            (None, Some(m)) => Some(m),
            (None, None) => None,
        };
        let fps_cap_x10 = mode.max_fps_cap().map(|f| f * 10 + 5);

        let candidates: Vec<&ResolvedVideoVariant> = if let Some(cap) = effective_cap {
            let under: Vec<&ResolvedVideoVariant> = self
                .video_variants
                .iter()
                .filter(|v| v.height <= cap)
                .collect();
            if under.is_empty() {
                // Fall back to the lowest available height if every stream exceeds cap.
                let min_h = self.video_variants.iter().map(|v| v.height).min().unwrap_or(0);
                self.video_variants
                    .iter()
                    .filter(|v| v.height == min_h)
                    .collect()
            } else {
                under
            }
        } else {
            self.video_variants.iter().collect()
        };

        let best_height = candidates.iter().map(|v| v.height).max()?;
        let at_best: Vec<&ResolvedVideoVariant> = candidates
            .into_iter()
            .filter(|v| v.height == best_height)
            .collect();

        let fps_filtered: Vec<&ResolvedVideoVariant> = if let Some(max_fps) = fps_cap_x10 {
            let under_fps: Vec<&ResolvedVideoVariant> = at_best
                .iter()
                .copied()
                .filter(|v| v.fps_x10 <= max_fps)
                .collect();
            if under_fps.is_empty() {
                at_best
            } else {
                under_fps
            }
        } else {
            at_best
        };

        if mode.is_congested() {
            fps_filtered.into_iter().min_by_key(|v| {
                (
                    codec_efficiency_rank(v.vcodec.as_deref()),
                    v.tbr_kbps,
                )
            })
        } else {
            fps_filtered.into_iter().max_by_key(|v| v.tbr_kbps)
        }
    }

    pub fn select_lower_tier(
        &self,
        current_height: u32,
        current_fps_x10: u32,
        current_tbr_kbps: u32,
    ) -> Option<&ResolvedVideoVariant> {
        if self.video_variants.is_empty() {
            return None;
        }
        let eff_tbr = if current_tbr_kbps > 0 {
            current_tbr_kbps
        } else {
            estimate_tbr_kbps_for_height(current_height)
        };

        // Step 1: Same resolution with lower FPS (<= 30.5fps) or noticeably lower bitrate.
        if current_fps_x10 > 400 {
            let max_tbr = (eff_tbr * 85) / 100;
            let same_h_lower_fps: Vec<&ResolvedVideoVariant> = self
                .video_variants
                .iter()
                .filter(|v| v.height == current_height && v.fps_x10 <= 305 && v.tbr_kbps <= max_tbr)
                .collect();
            if let Some(best) = same_h_lower_fps.into_iter().min_by_key(|v| {
                (codec_efficiency_rank(v.vcodec.as_deref()), v.tbr_kbps)
            }) {
                return Some(best);
            }
        }
        let same_h_lower_tbr: Vec<&ResolvedVideoVariant> = self
            .video_variants
            .iter()
            .filter(|v| v.height == current_height && v.tbr_kbps > 0 && v.tbr_kbps <= (eff_tbr * 80) / 100)
            .collect();
        if let Some(best) = same_h_lower_tbr.into_iter().min_by_key(|v| {
            (codec_efficiency_rank(v.vcodec.as_deref()), v.tbr_kbps)
        }) {
            return Some(best);
        }

        // Step 2: Highest resolution strictly below `current_height`, preferring <= 30.5fps & efficient codecs.
        let lower_height = self
            .video_variants
            .iter()
            .filter(|v| v.height < current_height)
            .map(|v| v.height)
            .max()?;
        let at_lower: Vec<&ResolvedVideoVariant> = self
            .video_variants
            .iter()
            .filter(|v| v.height == lower_height)
            .collect();
        let low_fps: Vec<&ResolvedVideoVariant> = at_lower
            .iter()
            .copied()
            .filter(|v| v.fps_x10 <= 305)
            .collect();
        let pool = if low_fps.is_empty() { at_lower } else { low_fps };
        pool.into_iter().min_by_key(|v| {
            (codec_efficiency_rank(v.vcodec.as_deref()), v.tbr_kbps)
        })
    }

    pub fn ladder_summary_rungs(&self, current_height: Option<u32>) -> Vec<(u32, bool)> {
        let mut heights: Vec<u32> = self.video_variants.iter().map(|v| v.height).collect();
        heights.sort_by(|a, b| b.cmp(a));
        heights.dedup();
        heights
            .into_iter()
            .map(|h| (h, current_height == Some(h)))
            .collect()
    }
}

pub fn build_http_client(force_ipv4: bool, timeout_secs: u64) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(timeout_secs.clamp(3, 6)))
        .timeout(Duration::from_secs(timeout_secs.max(4)));
    if force_ipv4 {
        builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    }
    builder.build().unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleOption {
    pub code: String,
    pub name: String,
    pub stream_label: String,
    pub url: String,
}

/// Explicit subtitle preference for launch intent, queue persistence, and profile defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtitleSelection {
    None,
    Track {
        code: String,
        name: String,
        url: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTrackOption {
    pub code: String,
    pub name: String,
    pub stream_label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub id: String,
    pub name: String,
    pub server_label: String,
    pub master_url: Option<String>,
    pub qualities: Vec<QualityOption>,
    pub subtitles: Vec<SubtitleOption>,
    pub supports_sub: bool,
    pub supports_dub: bool,
    #[serde(default)]
    pub is_verified: bool,
    #[serde(default)]
    pub verified_res: Option<String>,
}

pub fn default_providers() -> Vec<ProviderOption> {
    vec![
        ProviderOption {
            id: "zoko".to_string(),
            name: "ZokoAnime".to_string(),
            server_label: "ZokoAnime".to_string(),
            master_url: None,
            qualities: vec![
                QualityOption::simple("1080p", ""),
                QualityOption::simple("720p", ""),
                QualityOption::simple("480p", ""),
                QualityOption::simple("360p", ""),
            ],
            subtitles: vec![SubtitleOption::default_english()],
            supports_sub: true,
            supports_dub: true,
            is_verified: false,
            verified_res: None,
        },
        ProviderOption {
            id: "hd1".to_string(),
            name: "HD-1".to_string(),
            server_label: "HD-1".to_string(),
            master_url: None,
            qualities: vec![
                QualityOption::simple("1080p", ""),
                QualityOption::simple("720p", ""),
                QualityOption::simple("480p", ""),
            ],
            subtitles: vec![SubtitleOption::default_english()],
            supports_sub: true,
            supports_dub: true,
            is_verified: false,
            verified_res: None,
        },
        ProviderOption {
            id: "pahe".to_string(),
            name: "AnimePahe".to_string(),
            server_label: "AnimePahe".to_string(),
            master_url: None,
            qualities: vec![
                QualityOption::simple("1080p BD", ""),
                QualityOption::simple("720p BD", ""),
                QualityOption::simple("480p BD", ""),
            ],
            subtitles: vec![SubtitleOption::default_english()],
            supports_sub: true,
            supports_dub: false,
            is_verified: false,
            verified_res: None,
        },
        ProviderOption {
            id: "gogo".to_string(),
            name: "GogoAnime".to_string(),
            server_label: "GogoAnime".to_string(),
            master_url: None,
            qualities: vec![
                QualityOption::simple("1080p", ""),
                QualityOption::simple("720p", ""),
                QualityOption::simple("480p", ""),
                QualityOption::simple("360p", ""),
            ],
            subtitles: vec![SubtitleOption::default_english()],
            supports_sub: true,
            supports_dub: true,
            is_verified: false,
            verified_res: None,
        },
    ]
}

/// Filter out providers that did not verify available playable streams for this episode & audio mode.
pub fn filter_verified_providers(providers: Vec<ProviderOption>) -> Vec<ProviderOption> {
    providers
        .into_iter()
        .filter(|p| p.is_verified && p.master_url.as_deref().is_some_and(|u| !u.is_empty()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub master_url: Option<String>,
    pub qualities: Vec<QualityOption>,
    pub subtitles: Vec<SubtitleOption>,
    #[serde(default)]
    pub audio_tracks: Vec<AudioTrackOption>,
    pub providers: Vec<ProviderOption>,
    pub dub_available: bool,
    #[serde(default)]
    pub ladder: Option<ResolvedStreamLadder>,
}

impl StreamInfo {
    #[allow(dead_code)]
    pub fn fallback() -> Self {
        Self {
            master_url: None,
            qualities: vec![
                QualityOption::simple("1080p", ""),
                QualityOption::simple("720p", ""),
            ],
            subtitles: vec![SubtitleOption::default_english()],
            audio_tracks: Vec::new(),
            providers: default_providers(),
            dub_available: false,
            ladder: None,
        }
    }
}

impl SubtitleOption {
    pub fn default_english() -> Self {
        Self {
            code: "eng".to_string(),
            name: "English".to_string(),
            stream_label: "English".to_string(),
            url: String::new(),
        }
    }

    /// Extract a clean keyword suitable for ani-cli's ANI_CLI_SUB_LANG matching
    pub fn lang_keyword(&self) -> &str {
        if self.code.starts_with("ara") {
            "Arabic"
        } else if self.code.starts_with("rus") {
            "Russian"
        } else if self.code.starts_with("hin") {
            "Hindi"
        } else if self.code.starts_with("ind") {
            "Indonesian"
        } else if self.code.starts_with("urd") {
            "Urdu"
        } else if self.code.starts_with("spa") {
            "Spanish"
        } else if self.code.starts_with("fra") {
            "French"
        } else if self.code.starts_with("ger") || self.code.starts_with("deu") {
            "German"
        } else if self.code.starts_with("ita") {
            "Italian"
        } else if self.code.starts_with("por") {
            "Portuguese"
        } else if self.code.starts_with("tur") {
            "Turkish"
        } else if self.code.starts_with("vie") {
            "Vietnamese"
        } else if self.code.starts_with("jpn") {
            "Japanese"
        } else if self.code.starts_with("zho") || self.code.starts_with("chi") {
            "Chinese"
        } else if self.code.starts_with("kor") {
            "Korean"
        } else if self.code.starts_with("pol") {
            "Polish"
        } else if self.code.starts_with("tha") {
            "Thai"
        } else if self.code.starts_with("nld") || self.code.starts_with("dut") {
            "Dutch"
        } else if self.code.starts_with("swe") {
            "Swedish"
        } else if self.code.starts_with("ell") || self.code.starts_with("gre") {
            "Greek"
        } else if self.code.starts_with("heb") {
            "Hebrew"
        } else if self.code.starts_with("fas") || self.code.starts_with("per") {
            "Persian"
        } else if self.code.starts_with("fil") || self.code.starts_with("tgl") {
            "Tagalog"
        } else if self.code.starts_with("ukr") {
            "Ukrainian"
        } else if self.code.starts_with("ces") || self.code.starts_with("cze") {
            "Czech"
        } else if self.code.starts_with("hun") {
            "Hungarian"
        } else if self.code.starts_with("ron") || self.code.starts_with("rum") {
            "Romanian"
        } else if self.code.starts_with("dan") {
            "Danish"
        } else if self.code.starts_with("fin") {
            "Finnish"
        } else if self.code.starts_with("nor") {
            "Norwegian"
        } else if self.code.starts_with("msa") || self.code.starts_with("may") {
            "Malay"
        } else if self.code.starts_with("eng") {
            "English"
        } else if self.is_none() {
            "none"
        } else {
            // First alphabetic word from stream_label
            self.stream_label
                .split(|c: char| !c.is_alphabetic())
                .next()
                .filter(|w| !w.is_empty())
                .unwrap_or("English")
        }
    }

    /// Synthetic option representing explicit "No subtitles".
    pub fn none_option() -> Self {
        Self {
            code: "none".to_string(),
            name: "No subtitles".to_string(),
            stream_label: "Off".to_string(),
            url: String::new(),
        }
    }

    /// True if this entry disables subtitles explicitly.
    pub fn is_none(&self) -> bool {
        self.code.eq_ignore_ascii_case("none")
    }

    /// Convert this option into an explicit `SubtitleSelection`.
    pub fn to_selection(&self) -> SubtitleSelection {
        if self.is_none() {
            SubtitleSelection::None
        } else {
            SubtitleSelection::Track {
                code: self.code.clone(),
                name: self.name.clone(),
                url: (!self.url.is_empty()).then(|| self.url.clone()),
            }
        }
    }

    /// Ensure `"No subtitles"` is present once at index `0` of `subtitles`.
    pub fn ensure_none_option_first(subtitles: &mut Vec<SubtitleOption>) {
        subtitles.retain(|s| !s.is_none());
        subtitles.insert(0, Self::none_option());
    }
}

/// Prioritize core languages at the top of the subtitle track list:
/// 1. English
/// 2. Arabic
/// 3. Russian
/// 4. Indonesian (Bahasa Indonesia)
/// 5. Remaining languages in original order
pub fn prioritize_subtitles(subtitles: &mut [SubtitleOption]) {
    let lang_priority = |code: &str| -> usize {
        if code.starts_with("eng") {
            0
        } else if code.starts_with("ara") {
            1
        } else if code.starts_with("rus") {
            2
        } else if code.starts_with("ind") {
            3
        } else {
            4
        }
    };

    subtitles.sort_by_key(|s| lang_priority(&s.code));
}

fn resolve_hls_uri(base_url: &str, stream_path: &str) -> String {
    if stream_path.starts_with("http://") || stream_path.starts_with("https://") {
        stream_path.to_string()
    } else {
        format!("{}/{}", base_url, stream_path.trim_start_matches('/'))
    }
}

/// Parse complete video & audio variant ladder from an HLS master playlist.
pub fn parse_m3u8_ladder(
    master_url: &str,
    m3u8_content: &str,
    anime_id: i64,
    episode: u32,
    is_dub: bool,
    now_unix: i64,
) -> ResolvedStreamLadder {
    let base_url = match master_url.rfind('/') {
        Some(idx) => &master_url[..idx],
        None => master_url,
    };

    let mut audio_variants = Vec::new();
    let mut video_variants = Vec::new();
    let lines: Vec<&str> = m3u8_content.lines().map(|l| l.trim()).collect();

    for line in &lines {
        if !line.starts_with("#EXT-X-MEDIA:") {
            continue;
        }
        let media_type = extract_hls_attr(line, "TYPE").unwrap_or_default();
        if !media_type.eq_ignore_ascii_case("AUDIO") {
            continue;
        }
        let group_id = extract_hls_attr(line, "GROUP-ID").unwrap_or_default();
        let name_attr = extract_hls_attr(line, "NAME").unwrap_or_default();
        let lang_attr = extract_hls_attr(line, "LANGUAGE").unwrap_or_default();
        let is_default = extract_hls_attr(line, "DEFAULT")
            .is_some_and(|v| v.eq_ignore_ascii_case("YES"));
        let uri = extract_hls_attr(line, "URI")
            .filter(|u| !u.is_empty())
            .map(|u| resolve_hls_uri(base_url, &u));
        let (code, resolved_name) = resolve_lang_meta(&name_attr, &lang_attr);
        audio_variants.push(ResolvedAudioVariant {
            group_id,
            code,
            name: if !name_attr.is_empty() { name_attr } else { resolved_name },
            uri,
            is_default,
        });
    }

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let height = extract_hls_attr(line, "RESOLUTION")
                .and_then(|dim| dim.split('x').nth(1).map(|s| s.to_string()))
                .and_then(|h| h.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().ok())
                .unwrap_or(1080);

            let bw_bps = extract_hls_attr(line, "AVERAGE-BANDWIDTH")
                .or_else(|| extract_hls_attr(line, "BANDWIDTH"))
                .and_then(|v| v.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u64>().ok())
                .unwrap_or(0);
            let tbr_kbps = if bw_bps > 0 {
                ((bw_bps + 500) / 1000).max(1) as u32
            } else {
                estimate_tbr_kbps_for_height(height)
            };

            let fps_x10 = extract_hls_attr(line, "FRAME-RATE")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|&f| f > 0.0 && f < 240.0)
                .map(|f| (f * 10.0).round() as u32)
                .unwrap_or(240);

            let vcodec = extract_hls_attr(line, "CODECS").filter(|s| !s.is_empty());
            let audio_group = extract_hls_attr(line, "AUDIO").filter(|s| !s.is_empty());

            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with('#') || lines[j].is_empty()) {
                j += 1;
            }
            if j < lines.len() {
                let full_url = resolve_hls_uri(base_url, lines[j]);
                video_variants.push(ResolvedVideoVariant {
                    label: format!("{}p", height),
                    url: full_url,
                    height,
                    fps_x10,
                    tbr_kbps,
                    vcodec,
                    audio_group,
                });
            }
        }
        i += 1;
    }

    video_variants.sort_by_key(|v| std::cmp::Reverse((v.height, v.tbr_kbps)));

    if video_variants.is_empty() && !master_url.is_empty() {
        video_variants.push(ResolvedVideoVariant {
            label: "1080p".to_string(),
            url: master_url.to_string(),
            height: 1080,
            fps_x10: 240,
            tbr_kbps: estimate_tbr_kbps_for_height(1080),
            vcodec: None,
            audio_group: None,
        });
    }

    ResolvedStreamLadder {
        anime_id,
        episode,
        is_dub,
        master_url: (!master_url.is_empty()).then(|| master_url.to_string()),
        resolved_at_unix: now_unix,
        video_variants,
        audio_variants,
    }
}

/// Parse actual available video resolutions from an HLS master playlist.
pub fn parse_m3u8_qualities(master_url: &str, m3u8_content: &str) -> Vec<QualityOption> {
    let ladder = parse_m3u8_ladder(master_url, m3u8_content, 0, 0, false, 0);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for variant in ladder.video_variants {
        if seen.insert(variant.label.clone()) {
            result.push(QualityOption {
                label: variant.label,
                url: variant.url,
                height: Some(variant.height),
                tbr_kbps: Some(variant.tbr_kbps),
                fps_x10: Some(variant.fps_x10),
            });
        }
    }
    result
}

/// Prioritize audio tracks: Japanese original first, English dub second,
/// then major dub languages followed by remaining tracks in original order.
pub fn prioritize_audio_tracks(tracks: &mut [AudioTrackOption]) {
    let lang_priority = |code: &str| -> usize {
        if code.starts_with("jpn") || code == "ja" {
            0
        } else if code.starts_with("eng") || code == "en" {
            1
        } else if code.starts_with("ara") {
            2
        } else if code.starts_with("spa") {
            3
        } else if code.starts_with("por") {
            4
        } else if code.starts_with("fra") || code.starts_with("fre") {
            5
        } else if code.starts_with("ger") || code.starts_with("deu") {
            6
        } else if code.starts_with("rus") {
            7
        } else if code.starts_with("ind") {
            8
        } else {
            9
        }
    };

    tracks.sort_by_key(|t| lang_priority(&t.code));
}

fn extract_hls_attr(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let mut search_from = 0;
    while let Some(rel_idx) = line[search_from..].find(&needle) {
        let abs_idx = search_from + rel_idx;
        let boundary_ok = abs_idx == 0
            || matches!(
                line.as_bytes()[abs_idx - 1],
                b':' | b',' | b' ' | b'\t'
            );
        if boundary_ok {
            let val_part = &line[abs_idx + needle.len()..];
            if let Some(after_quote) = val_part.strip_prefix('"') {
                if let Some(end_q) = after_quote.find('"') {
                    return Some(after_quote[..end_q].trim().to_string());
                }
            } else {
                let end = val_part.find(',').unwrap_or(val_part.len());
                return Some(val_part[..end].trim().to_string());
            }
        }
        search_from = abs_idx + needle.len();
    }
    None
}

/// Parse actual available audio / dub language tracks from `#EXT-X-MEDIA:TYPE=AUDIO` entries in an HLS master playlist.
pub fn parse_m3u8_audio_tracks(m3u8_content: &str) -> Vec<AudioTrackOption> {
    let mut tracks = Vec::new();
    let mut seen_codes = HashSet::new();

    for raw_line in m3u8_content.lines() {
        let line = raw_line.trim();
        if !line.starts_with("#EXT-X-MEDIA:") {
            continue;
        }
        let media_type = extract_hls_attr(line, "TYPE").unwrap_or_default();
        if !media_type.eq_ignore_ascii_case("AUDIO") {
            continue;
        }

        let name_attr = extract_hls_attr(line, "NAME").unwrap_or_default();
        let lang_attr = extract_hls_attr(line, "LANGUAGE").unwrap_or_default();
        let is_default = extract_hls_attr(line, "DEFAULT")
            .is_some_and(|v| v.eq_ignore_ascii_case("YES"));

        let (code, resolved_name) = resolve_lang_meta(&name_attr, &lang_attr);
        if seen_codes.insert(code.clone()) {
            tracks.push(AudioTrackOption {
                code,
                name: resolved_name.clone(),
                stream_label: if !name_attr.is_empty() {
                    name_attr
                } else {
                    resolved_name
                },
                is_default,
            });
        }
    }

    prioritize_audio_tracks(&mut tracks);
    tracks
}

/// Dynamically fetch verified stream info (qualities, subtitles, and direct master URL)
/// for a specific anime episode. Automatically queries and merges community multi-language
/// subtitle tracks (Arabic, Spanish, French, German, Italian, etc.) if not already present in the stream.
pub async fn fetch_stream_info(
    anilist_id: i64,
    query_title: &str,
    episode: u32,
    is_dub: bool,
) -> StreamInfo {
    fetch_stream_info_with_mal(anilist_id, None, query_title, episode, is_dub).await
}

/// Variant of `fetch_stream_info` that reuses a cached MyAnimeList ID when available
/// to skip the AniList GraphQL `idMal` roundtrip.
pub async fn fetch_stream_info_with_mal(
    anilist_id: i64,
    mal_id_hint: Option<i64>,
    query_title: &str,
    episode: u32,
    is_dub: bool,
) -> StreamInfo {
    fetch_stream_info_with_network(
        anilist_id,
        mal_id_hint,
        query_title,
        episode,
        is_dub,
        false,
        5,
    )
    .await
}

/// Dynamically fetch verified stream info (qualities, subtitles, master URL, and strictly filtered
/// verified providers) for a specific anime episode.
pub async fn fetch_stream_info_with_network(
    anilist_id: i64,
    mal_id_hint: Option<i64>,
    query_title: &str,
    episode: u32,
    is_dub: bool,
    force_ipv4: bool,
    timeout_secs: u64,
) -> StreamInfo {
    let client = build_http_client(force_ipv4, timeout_secs);

    // Run primary MAL/Zoko, HiAnime (HD-1 / Zoko servers), AllAnime GraphQL, and external subtitles in parallel
    let mal_fut = async {
        let mal_id = match mal_id_hint {
            Some(mid) => Some(mid),
            None => fetch_mal_id(&client, anilist_id).await,
        };
        if let Some(mid) = mal_id {
            let (res, dub_avail) = tokio::join!(
                fetch_stream_for_mal(&client, mid, episode, is_dub),
                check_mal_dub_available(&client, mid, episode)
            );
            return (res.ok(), dub_avail);
        }
        (None, false)
    };

    let (
        (zoko_res, zoko_dub),
        hianime_res,
        allanime_res,
        ext_subs_res,
    ) = tokio::join!(
        mal_fut,
        tokio::time::timeout(
            Duration::from_secs(4),
            fetch_all_hianime_providers(&client, query_title, episode, is_dub)
        ),
        tokio::time::timeout(
            Duration::from_secs(4),
            probe_allanime_provider(&client, query_title, episode, is_dub)
        ),
        tokio::time::timeout(
            Duration::from_secs(4),
            fetch_external_subtitles(&client, anilist_id, query_title, episode),
        )
    );

    let ext_subs = ext_subs_res.unwrap_or_default();
    let hianime_provs = hianime_res.unwrap_or_default();
    let allanime_prov = allanime_res.ok().flatten();

    // Assemble base StreamInfo from the strongest verified source
    let mut info = if let Some(mut z_info) = zoko_res {
        z_info.dub_available = z_info.dub_available || zoko_dub || is_dub;
        z_info
    } else if let Some((best_hi_info, _)) = hianime_provs.first() {
        best_hi_info.clone()
    } else if let Some(ref aa) = allanime_prov {
        StreamInfo {
            master_url: aa.master_url.clone(),
            qualities: aa.qualities.clone(),
            subtitles: vec![SubtitleOption::default_english()],
            audio_tracks: Vec::new(),
            providers: Vec::new(),
            dub_available: is_dub || aa.supports_dub,
            ladder: None,
        }
    } else {
        StreamInfo {
            master_url: None,
            qualities: Vec::new(),
            subtitles: vec![SubtitleOption::default_english()],
            audio_tracks: Vec::new(),
            providers: Vec::new(),
            dub_available: false,
            ladder: None,
        }
    };

    if let Some(ref mut ladder) = info.ladder {
        ladder.anime_id = anilist_id;
        ladder.episode = episode;
        ladder.is_dub = is_dub;
    }

    // Merge external community subtitles (OpenSubtitles) into the stream's subtitle list
    let mut existing_codes: HashSet<String> = info.subtitles.iter().map(|s| s.code.clone()).collect();
    for ext in ext_subs {
        if !existing_codes.contains(&ext.code) {
            existing_codes.insert(ext.code.clone());
            info.subtitles.push(ext);
        }
    }

    prioritize_subtitles(&mut info.subtitles);
    SubtitleOption::ensure_none_option_first(&mut info.subtitles);

    let mut all_subtitles = info.subtitles.clone();
    SubtitleOption::ensure_none_option_first(&mut all_subtitles);
    let dub_avail = info.dub_available || !hianime_provs.is_empty() && hianime_provs.iter().any(|(_, p)| p.supports_dub);

    // Build strictly verified provider list — unavailable providers never appear!
    let mut verified_providers: Vec<ProviderOption> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    if let Some(ref m_url) = info.master_url {
        if !m_url.is_empty() && !info.qualities.is_empty() {
            let best_res = info.qualities.first().map(|q| q.label.clone());
            verified_providers.push(ProviderOption {
                id: "zoko".to_string(),
                name: "ZokoAnime".to_string(),
                server_label: "ZokoAnime Direct".to_string(),
                master_url: Some(m_url.clone()),
                qualities: info.qualities.clone(),
                subtitles: all_subtitles.clone(),
                supports_sub: true,
                supports_dub: zoko_dub || dub_avail,
                is_verified: true,
                verified_res: best_res,
            });
            seen_ids.insert("zoko".to_string());
        }
    }

    for (hi_info, mut hi_prov) in hianime_provs {
        if hi_prov.master_url.as_deref().is_some_and(|u| !u.is_empty())
            && seen_ids.insert(hi_prov.id.clone())
        {
            if hi_prov.qualities.is_empty() {
                hi_prov.qualities = hi_info.qualities.clone();
            }
            hi_prov.subtitles = all_subtitles.clone();
            hi_prov.is_verified = true;
            hi_prov.verified_res = hi_prov.qualities.first().map(|q| q.label.clone());
            verified_providers.push(hi_prov);
        }
    }

    if let Some(mut aa_prov) = allanime_prov {
        if aa_prov.master_url.as_deref().is_some_and(|u| !u.is_empty())
            && seen_ids.insert(aa_prov.id.clone())
        {
            aa_prov.subtitles = all_subtitles.clone();
            aa_prov.is_verified = true;
            verified_providers.push(aa_prov);
        }
    }

    info.providers = filter_verified_providers(verified_providers);
    info.dub_available = dub_avail;
    info
}

async fn check_mal_dub_available(client: &reqwest::Client, mal_id: i64, episode: u32) -> bool {
    let url = format!("https://zokoanime.video/stream/mal/{}/{}/dub", mal_id, episode);
    if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(3)).send().await {
        if let Ok(text) = resp.text().await {
            return text.contains("window.__P=\"");
        }
    }
    false
}

/// Probe all working HiAnime server embeds for the exact episode & audio mode (`sub`/`dub`).
async fn fetch_all_hianime_providers(
    client: &reqwest::Client,
    query: &str,
    episode: u32,
    is_dub: bool,
) -> Vec<(StreamInfo, ProviderOption)> {
    let Ok(search_resp) = client
        .get(format!("https://hianime.at/search?keyword={}", urlencoding::encode(query)))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(search_html) = search_resp.text().await else {
        return Vec::new();
    };
    let Some(anime_id) = extract_anime_id(&search_html) else {
        return Vec::new();
    };

    let Ok(ep_resp) = client
        .get(format!("https://hianime.at/api/theme/episode/list/{}", anime_id))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(ep_json) = ep_resp.json::<serde_json::Value>().await else {
        return Vec::new();
    };
    let ep_html = ep_json.get("html").and_then(|v| v.as_str()).unwrap_or("");
    let Some(ep_id) = extract_episode_id_strict(ep_html, episode) else {
        return Vec::new();
    };

    let Ok(srv_resp) = client
        .get(format!("https://hianime.at/api/theme/episode/servers?episodeId={}", ep_id))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(srv_json) = srv_resp.json::<serde_json::Value>().await else {
        return Vec::new();
    };
    let srv_html = srv_json.get("html").and_then(|v| v.as_str()).unwrap_or("");
    let has_dub = srv_html.contains("data-type=\"dub\"");
    let has_sub = srv_html.contains("data-type=\"sub\"");

    let mode_attr = if is_dub { "data-type=\"dub\"" } else { "data-type=\"sub\"" };
    let mut embeds: Vec<(String, String, String)> = Vec::new();
    for block in srv_html.split("server-item") {
        if !block.contains(mode_attr) {
            continue;
        }
        if let Some(hash_idx) = block.find("data-hash=\"") {
            let after = &block[hash_idx + 11..];
            if let Some(end_idx) = after.find('"') {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&after[..end_idx]) {
                    if let Ok(url) = String::from_utf8(bytes) {
                        if url.starts_with("http://") || url.starts_with("https://") {
                            let label = if block.contains("ZokoAnime") {
                                ("zoko", "ZokoAnime")
                            } else if block.contains("HD-1") {
                                ("hd1", "HiAnime HD-1")
                            } else if block.contains("HD-2") {
                                ("hd2", "HiAnime HD-2")
                            } else {
                                ("hianime", "HiAnime")
                            };
                            embeds.push((label.0.to_string(), label.1.to_string(), url));
                        }
                    }
                }
            }
        }
    }

    let mut results = Vec::new();
    for (pid, pname, embed_url) in embeds {
        if let Ok(resp) = client.get(&embed_url).timeout(Duration::from_secs(3)).send().await {
            if let Ok(html) = resp.text().await {
                if let Ok(mut st_info) = parse_stream_embed(client, &html).await {
                    if st_info.master_url.as_deref().is_some_and(|u| !u.is_empty())
                        && !st_info.qualities.is_empty()
                    {
                        st_info.dub_available = st_info.dub_available || has_dub;
                        let best_res = st_info.qualities.first().map(|q| q.label.clone());
                        let prov = ProviderOption {
                            id: pid,
                            name: pname.clone(),
                            server_label: pname,
                            master_url: st_info.master_url.clone(),
                            qualities: st_info.qualities.clone(),
                            subtitles: st_info.subtitles.clone(),
                            supports_sub: has_sub,
                            supports_dub: has_dub,
                            is_verified: true,
                            verified_res: best_res,
                        };
                        results.push((st_info, prov));
                    }
                }
            }
        }
    }
    results
}

fn decode_allanime_source_url(raw: &str) -> String {
    let Some(hex_body) = raw.strip_prefix("--") else {
        return raw.to_string();
    };
    let mut out = String::with_capacity(hex_body.len() / 2);
    let bytes = hex_body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if let Ok(pair) = std::str::from_utf8(&bytes[i..i + 2]) {
            if let Ok(v) = u8::from_str_radix(pair, 16) {
                out.push((v ^ 56) as char);
            }
        }
        i += 2;
    }
    if out.starts_with('/') {
        format!("https://allanime.day{out}")
    } else {
        out
    }
}

/// Probe AllAnime GraphQL directly to verify episode availability and resolve direct HLS/MP4 URLs.
async fn probe_allanime_provider(
    client: &reqwest::Client,
    query: &str,
    episode: u32,
    is_dub: bool,
) -> Option<ProviderOption> {
    let search_gql = r#"query($search: SearchInput) { shows(search: $search, limit: 5, page: 1, translationType: sub, countryOrigin: ALL) { edges { _id name availableEpisodesDetail } } }"#;
    let body = serde_json::json!({
        "query": search_gql,
        "variables": { "search": { "query": query } }
    });
    let resp = client
        .post("https://api.allanime.day/api")
        .header("Referer", "https://allmanga.to/")
        .json(&body)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    let edges = json.pointer("/data/shows/edges")?.as_array()?;
    let ep_str = episode.to_string();
    let mode_key = if is_dub { "dub" } else { "sub" };

    let show = edges.iter().find(|e| {
        e.pointer(&format!("/availableEpisodesDetail/{mode_key}"))
            .and_then(|v| v.as_array())
            .is_some_and(|arr| arr.iter().any(|x| x.as_str() == Some(ep_str.as_str())))
    })?;
    let show_id = show.get("_id")?.as_str()?;
    let has_sub = show
        .pointer("/availableEpisodesDetail/sub")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| arr.iter().any(|x| x.as_str() == Some(ep_str.as_str())));
    let has_dub = show
        .pointer("/availableEpisodesDetail/dub")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| arr.iter().any(|x| x.as_str() == Some(ep_str.as_str())));

    let ep_gql = r#"query($showId: String!, $translationType: VaildTranslationTypeEnumType!, $episodeString: String!) { episode(showId: $showId, translationType: $translationType, episodeString: $episodeString) { sourceUrls } }"#;
    let ep_body = serde_json::json!({
        "query": ep_gql,
        "variables": {
            "showId": show_id,
            "translationType": mode_key,
            "episodeString": ep_str
        }
    });
    let ep_resp = client
        .post("https://api.allanime.day/api")
        .header("Referer", "https://allmanga.to/")
        .json(&ep_body)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let ep_json: serde_json::Value = ep_resp.json().await.ok()?;
    let sources = ep_json.pointer("/data/episode/sourceUrls")?.as_array()?;

    for src in sources {
        let raw_u = src.get("sourceUrl").and_then(|v| v.as_str()).unwrap_or("");
        let decoded = decode_allanime_source_url(raw_u);
        if decoded.contains("clock.json") || decoded.ends_with(".m3u8") || decoded.ends_with(".mp4") {
            let final_url = if decoded.contains("clock.json") {
                let Ok(clk_resp) = client.get(&decoded).timeout(Duration::from_secs(3)).send().await else {
                    continue;
                };
                let Ok(clk_json) = clk_resp.json::<serde_json::Value>().await else {
                    continue;
                };
                let Some(link) = clk_json.pointer("/links/0/link").and_then(|v| v.as_str()) else {
                    continue;
                };
                link.to_string()
            } else {
                decoded
            };
            if final_url.starts_with("http://") || final_url.starts_with("https://") {
                let quals = if final_url.contains(".m3u8") {
                    if let Ok(m_resp) = client.get(&final_url).timeout(Duration::from_secs(3)).send().await {
                        if let Ok(m_txt) = m_resp.text().await {
                            let parsed = parse_m3u8_qualities(&final_url, &m_txt);
                            if !parsed.is_empty() {
                                parsed
                            } else {
                                vec![QualityOption::simple("1080p", final_url.clone())]
                            }
                        } else {
                            vec![QualityOption::simple("1080p", final_url.clone())]
                        }
                    } else {
                        vec![QualityOption::simple("1080p", final_url.clone())]
                    }
                } else {
                    vec![QualityOption::simple("1080p", final_url.clone())]
                };
                let best_res = quals.first().map(|q| q.label.clone());
                return Some(ProviderOption {
                    id: "allanime".to_string(),
                    name: "AllAnime Direct".to_string(),
                    server_label: "AllAnime".to_string(),
                    master_url: Some(final_url),
                    qualities: quals,
                    subtitles: Vec::new(),
                    supports_sub: has_sub,
                    supports_dub: has_dub,
                    is_verified: true,
                    verified_res: best_res,
                });
            }
        }
    }
    None
}

#[allow(dead_code)]
async fn resolve_primary_stream(
    client: &reqwest::Client,
    anilist_id: i64,
    mal_id_hint: Option<i64>,
    query_title: &str,
    episode: u32,
    is_dub: bool,
) -> Option<StreamInfo> {
    let mal_id = match mal_id_hint {
        Some(mid) => Some(mid),
        None => fetch_mal_id(client, anilist_id).await,
    };
    if let Some(mal_id) = mal_id {
        let (mal_res, dub_avail) = tokio::join!(
            fetch_stream_for_mal(client, mal_id, episode, is_dub),
            check_mal_dub_available(client, mal_id, episode)
        );
        if let Ok(mut info) = mal_res {
            if !info.subtitles.is_empty() || !info.qualities.is_empty() {
                info.dub_available = info.dub_available || dub_avail || is_dub;
                return Some(info);
            }
        }
    }
    let all = fetch_all_hianime_providers(client, query_title, episode, is_dub).await;
    all.into_iter().next().map(|(i, _)| i)
}

#[derive(Debug, Deserialize)]
struct AniZipMappingData {
    mappings: Option<AniZipMappingDetails>,
    #[serde(default)]
    titles: HashMap<String, String>,
    #[serde(default)]
    episodes: HashMap<String, AniZipEpDetails>,
}

#[derive(Debug, Deserialize)]
struct AniZipMappingDetails {
    imdb_id: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniZipEpDetails {
    season_number: Option<u32>,
    episode_number: Option<u32>,
    absolute_episode_number: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct StremioSubtitlesPayload {
    #[serde(default)]
    subtitles: Vec<StremioSubtitleItem>,
}

#[derive(Debug, Deserialize)]
struct StremioSubtitleItem {
    url: Option<String>,
    lang: Option<String>,
    #[serde(rename = "subtitleFileName")]
    subtitle_file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenSubtitlesItem {
    #[serde(rename = "SubLanguageID")]
    sub_language_id: Option<String>,
    #[serde(rename = "SeriesEpisode")]
    series_episode: Option<String>,
    #[serde(rename = "SubFileName")]
    sub_file_name: Option<String>,
    #[serde(rename = "SubDownloadLink")]
    sub_download_link: Option<String>,
}

/// Matches an episode number against SeriesEpisode tag or common anime fansub filename conventions.
pub fn matches_episode(series_ep: &str, filename: &str, target_ep: u32) -> bool {
    if let Ok(ep) = series_ep.trim().parse::<u32>() {
        if ep == target_ep {
            return true;
        }
    }

    let lower = filename.to_lowercase();
    let target_str = target_ep.to_string();
    let target_pad = format!("{:02}", target_ep);

    let patterns = [
        format!("e{}", target_str),
        format!("e{}", target_pad),
        format!("ep{}", target_str),
        format!("ep{}", target_pad),
        format!("ep.{}", target_str),
        format!("ep.{}", target_pad),
        format!("ep {}", target_str),
        format!("ep {}", target_pad),
        format!("episode {}", target_str),
        format!("episode {}", target_pad),
        format!("layer {}", target_str),
        format!("layer {}", target_pad),
        format!("layer{}", target_pad),
        format!(" - {}", target_str),
        format!(" - {}", target_pad),
        format!("-{}", target_pad),
        format!(" {}", target_pad),
        format!("_{}_", target_pad),
        format!("_{}", target_pad),
    ];

    for p in &patterns {
        if let Some(pos) = lower.find(p) {
            let end = pos + p.len();
            if end >= lower.len() || !lower.as_bytes()[end].is_ascii_digit() {
                return true;
            }
        }
    }

    false
}

/// Fetch community multi-language subtitle tracks for an anime episode via AniZip mapping,
/// Stremio OpenSubtitles v3 addon, and direct OpenSubtitles REST search across English and Romaji titles.
pub async fn fetch_external_subtitles(
    client: &reqwest::Client,
    anilist_id: i64,
    query_title: &str,
    episode: u32,
) -> Vec<SubtitleOption> {
    let anizip_url = format!("https://api.ani.zip/mappings?anilist_id={}", anilist_id);
    let Ok(resp) = client.get(&anizip_url).timeout(Duration::from_secs(4)).send().await else {
        return Vec::new();
    };
    let Ok(parsed) = resp.json::<AniZipMappingData>().await else {
        return Vec::new();
    };

    let mut options = Vec::new();
    let mut seen = HashSet::new();

    // 1. Stremio OpenSubtitles v3 addon via IMDb ID (if available)
    let mut mapped_season = 1u32;
    let mut mapped_ep = episode;
    let mut mapped_abs_ep = episode;
    if let Some(ep_info) = parsed.episodes.get(&episode.to_string()) {
        mapped_season = ep_info.season_number.unwrap_or(1);
        mapped_ep = ep_info.episode_number.unwrap_or(episode);
        mapped_abs_ep = ep_info.absolute_episode_number.unwrap_or(mapped_ep);
    }

    if let Some(imdb_id) = parsed.mappings.as_ref().and_then(|m| m.imdb_id.clone()).filter(|id| !id.is_empty()) {
        let is_movie = parsed
            .mappings
            .as_ref()
            .and_then(|m| m.media_type.as_deref())
            .map(|t| t.eq_ignore_ascii_case("movie"))
            .unwrap_or(false);

        let query_url = if is_movie {
            format!("https://opensubtitles-v3.strem.io/subtitles/movie/{}.json", imdb_id)
        } else {
            format!("https://opensubtitles-v3.strem.io/subtitles/series/{}:{}:{}.json", imdb_id, mapped_season, mapped_ep)
        };

        if let Ok(sub_resp) = client.get(&query_url).timeout(Duration::from_secs(4)).send().await {
            if let Ok(sub_data) = sub_resp.json::<StremioSubtitlesPayload>().await {
                for item in sub_data.subtitles {
                    let Some(url) = item.url.filter(|u| !u.is_empty()) else {
                        continue;
                    };
                    let lang = item.lang.as_deref().unwrap_or("eng");
                    let label = item.subtitle_file_name.as_deref().unwrap_or(lang);
                    let (code, name) = resolve_lang_meta(label, lang);

                    if seen.insert(code.clone()) {
                        options.push(SubtitleOption {
                            code,
                            name: name.clone(),
                            stream_label: format!("{} [Auto-Synced]", name),
                            url,
                        });
                    }
                }
            }
        }
    }

    // 2. OpenSubtitles REST search API for Arabic, Russian, Indonesian, and wider community fansubs
    let mut search_titles = Vec::new();
    if !query_title.is_empty() {
        search_titles.push(query_title.to_string());
    }
    if let Some(en) = parsed.titles.get("en").filter(|s| !s.is_empty()) {
        if !search_titles.contains(en) {
            search_titles.push(en.clone());
        }
    }
    if let Some(xjat) = parsed.titles.get("x-jat").filter(|s| !s.is_empty()) {
        if !search_titles.contains(xjat) {
            search_titles.push(xjat.clone());
        }
    }

    let mut query_urls = Vec::new();
    for title in &search_titles {
        for lang_filter in ["sublanguageid-ara", "sublanguageid-rus", "sublanguageid-ind", ""] {
            let query_path = if lang_filter.is_empty() {
                format!("https://rest.opensubtitles.org/search/query-{}", urlencoding::encode(title))
            } else {
                format!("https://rest.opensubtitles.org/search/{}/query-{}", lang_filter, urlencoding::encode(title))
            };
            query_urls.push(query_path);
        }
    }

    let search_futs = query_urls.into_iter().map(|query_path| {
        let client = client.clone();
        async move {
            let req = client
                .get(&query_path)
                .header("User-Agent", "TemporaryUserAgent")
                .timeout(Duration::from_secs(3));

            if let Ok(resp) = req.send().await {
                if let Ok(items) = resp.json::<Vec<OpenSubtitlesItem>>().await {
                    return items;
                }
            }
            Vec::new()
        }
    });

    let ql = query_title.to_ascii_lowercase();
    let is_split_sequel = mapped_season >= 2
        || mapped_abs_ep > episode
        || ql.contains("cour 2")
        || ql.contains("part 2")
        || ql.contains("2nd season");

    for items in futures_util::future::join_all(search_futs).await {
        for it in items {
            let sep = it.series_episode.as_deref().unwrap_or("");
            let fn_str = it.sub_file_name.as_deref().unwrap_or("");
            let Some(dl_url) = it.sub_download_link.filter(|u| !u.is_empty()) else {
                continue;
            };

            let matched = if is_split_sequel && mapped_abs_ep != episode {
                matches_episode(sep, fn_str, mapped_abs_ep)
                    || (matches_episode(sep, fn_str, mapped_ep)
                        && (fn_str.to_ascii_lowercase().contains("cour 2")
                            || fn_str.to_ascii_lowercase().contains("part 2")
                            || fn_str.to_ascii_lowercase().contains("s02")))
            } else {
                matches_episode(sep, fn_str, episode)
            };

            if matched {
                let lang_id = it.sub_language_id.as_deref().unwrap_or("eng");
                let (code, name) = resolve_lang_meta(fn_str, lang_id);

                if seen.insert(code.clone()) {
                    options.push(SubtitleOption {
                        code,
                        name: name.clone(),
                        stream_label: format!("{} [Auto-Synced]", name),
                        url: dl_url,
                    });
                }
            }
        }
    }

    prioritize_subtitles(&mut options);
    options
}

/// Download and cache a remote subtitle URL to ~/.cache/solus/subs/ and return the local path.
/// If the subtitle is already local or download fails, returns the input URL.
#[allow(dead_code)]
pub async fn ensure_cached_subtitle(
    client: &reqwest::Client,
    sub_url: &str,
    anime_id: i64,
    episode: u32,
    code: &str,
) -> String {
    if !sub_url.starts_with("http://") && !sub_url.starts_with("https://") {
        return sub_url.to_string();
    }

    let cache_dir = crate::config::Config::cache_dir()
        .map(|dir| dir.join("subs"))
        .unwrap_or_else(|_| std::env::temp_dir().join("solus-subs"));

    if std::fs::create_dir_all(&cache_dir).is_err() {
        return sub_url.to_string();
    }

    let clean_code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let ext = if sub_url.contains(".vtt") { "vtt" } else { "srt" };
    let filename = format!("{}_{}_{}.{}", anime_id, episode, if clean_code.is_empty() { "sub" } else { &clean_code }, ext);
    let file_path = cache_dir.join(&filename);

    if file_path.is_file() {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if metadata.len() > 0 {
                return file_path.to_string_lossy().into_owned();
            }
        }
    }

    if let Ok(resp) = client.get(sub_url).timeout(Duration::from_secs(6)).send().await {
        if let Ok(bytes) = resp.bytes().await {
            if !bytes.is_empty()
                && std::fs::write(&file_path, &bytes).is_ok() {
                    return file_path.to_string_lossy().into_owned();
                }
        }
    }

    sub_url.to_string()
}

/// Backwards compatibility wrapper for subtitle detection.
#[allow(dead_code)]
pub async fn fetch_available_subtitles(
    anilist_id: i64,
    query_title: &str,
    episode: u32,
) -> Vec<SubtitleOption> {
    fetch_stream_info(anilist_id, query_title, episode, false).await.subtitles
}

/// Query AniList GraphQL for MyAnimeList ID (idMal)
async fn fetch_mal_id(client: &reqwest::Client, anilist_id: i64) -> Option<i64> {
    let query = r#"query($id: Int) { Media(id: $id) { idMal } }"#;
    let body = serde_json::json!({
        "query": query,
        "variables": { "id": anilist_id }
    });

    let resp = client
        .post("https://graphql.anilist.co")
        .json(&body)
        .send()
        .await
        .ok()?;

    let json: serde_json::Value = resp.json().await.ok()?;
    json.pointer("/data/Media/idMal")?.as_i64()
}

/// Fetch and parse stream info using MAL ID directly on ZokoAnime embed
async fn fetch_stream_for_mal(
    client: &reqwest::Client,
    mal_id: i64,
    episode: u32,
    is_dub: bool,
) -> Result<StreamInfo, ()> {
    let mode_str = if is_dub { "dub" } else { "sub" };
    let url = format!("https://zokoanime.video/stream/mal/{}/{}/{}", mal_id, episode, mode_str);
    let resp = client.get(&url).send().await.map_err(|_| ())?;
    let html = resp.text().await.map_err(|_| ())?;
    parse_stream_embed(client, &html).await
}

#[allow(dead_code)]
/// Fallback: search HiAnime, find episode ID and embed
async fn fetch_stream_via_hianime(
    client: &reqwest::Client,
    query: &str,
    episode: u32,
    is_dub: bool,
) -> Result<StreamInfo, ()> {
    let all = fetch_all_hianime_providers(client, query, episode, is_dub).await;
    all.into_iter().next().map(|(i, _)| i).ok_or(())
}

async fn parse_stream_embed(client: &reqwest::Client, html: &str) -> Result<StreamInfo, ()> {
    let marker = "window.__P=\"";
    let start = html.find(marker).ok_or(())? + marker.len();
    let rest = &html[start..];
    let end = rest.find('"').ok_or(())?;
    let b64_blob = &rest[..end];

    let json_str = deobfuscate_blob(b64_blob).ok_or(())?;
    let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|_| ())?;

    let master_url = parsed.get("src").and_then(|v| v.as_str()).map(|s| s.to_string());
    let subs = parsed.get("subtitles").and_then(|v| v.as_array());
    let subtitles = subs.map(|arr| map_subtitles(arr)).unwrap_or_else(|| vec![SubtitleOption::default_english()]);

    let mut qualities = Vec::new();
    let mut audio_tracks = Vec::new();
    let mut ladder = None;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if let Some(ref m_url) = master_url {
        if let Ok(resp) = client.get(m_url).header("Referer", "https://zokoanime.video/").send().await {
            if let Ok(text) = resp.text().await {
                let parsed_ladder = parse_m3u8_ladder(m_url, &text, 0, 0, false, now_unix);
                qualities = parse_m3u8_qualities(m_url, &text);
                audio_tracks = parse_m3u8_audio_tracks(&text);
                ladder = Some(parsed_ladder);
            }
        }
        if qualities.is_empty() {
            qualities.push(QualityOption::simple("1080p", m_url.clone()));
        }
    }

    let mut providers = default_providers();
    if let Some(ref m_url) = master_url {
        if let Some(p0) = providers.get_mut(0) {
            p0.master_url = Some(m_url.clone());
        }
    }

    let dub_available = audio_tracks.iter().any(|t| !t.code.starts_with("jpn"));

    Ok(StreamInfo {
        master_url,
        qualities,
        subtitles,
        audio_tracks,
        providers,
        dub_available,
        ladder,
    })
}

fn deobfuscate_blob(blob_b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .ok()?;

    let mut out = Vec::with_capacity(bytes.len());
    for (i, b) in bytes.iter().enumerate() {
        out.push(b ^ XOR_KEY[i % XOR_KEY.len()]);
    }

    String::from_utf8(out).ok()
}

pub fn resolve_lang_meta(label: &str, lang: &str) -> (String, String) {
    let l_lower = label.to_lowercase();
    let lang_lower = if lang.is_empty() {
        l_lower.clone()
    } else {
        lang.to_lowercase()
    };

    // Specific regional checks first
    if l_lower.contains("latin") || l_lower.contains("latam") || l_lower.contains("es-419") || lang_lower == "es-419" {
        return ("spa-419".to_string(), "Spanish (Latin America)".to_string());
    }
    if l_lower.contains("espana") || l_lower.contains("españa") || l_lower.contains("castilian") || l_lower.contains("castellano") || l_lower.contains("spain") || lang_lower == "es-es" {
        return ("spa-es".to_string(), "Spanish (Castilian/Spain)".to_string());
    }
    if l_lower.contains("brasil") || l_lower.contains("brazil") || lang_lower == "pt-br" || lang_lower == "pob" {
        return ("por-br".to_string(), "Portuguese (Brazil)".to_string());
    }
    if l_lower.contains("portugal") || lang_lower == "pt-pt" {
        return ("por-pt".to_string(), "Portuguese (Portugal)".to_string());
    }

    // General language checks
    if l_lower.contains("arab") || l_lower.contains("العربية") || lang_lower == "ar" || lang_lower == "ara" {
        ("ara".to_string(), "Arabic (العربية)".to_string())
    } else if l_lower.contains("russ") || l_lower.contains("русский") || lang_lower == "ru" || lang_lower == "rus" {
        ("rus".to_string(), "Russian (Русский)".to_string())
    } else if l_lower.contains("hind") || l_lower.contains("हिन्दी") || lang_lower == "hi" || lang_lower == "hin" {
        ("hin".to_string(), "Hindi (हिन्दी)".to_string())
    } else if l_lower.contains("indo") || l_lower.contains("bahasa indonesia") || lang_lower == "id" || lang_lower == "ind" {
        ("ind".to_string(), "Indonesian (Bahasa Indonesia)".to_string())
    } else if l_lower.contains("urdu") || l_lower.contains("اردو") || lang_lower == "ur" || lang_lower == "urd" {
        ("urd".to_string(), "Urdu (اردو)".to_string())
    } else if l_lower.contains("engl") || lang_lower == "en" || lang_lower == "eng" {
        ("eng".to_string(), "English".to_string())
    } else if l_lower.contains("span") || l_lower.contains("espanol") || l_lower.contains("español") || lang_lower == "es" || lang_lower == "spa" {
        ("spa".to_string(), "Spanish (Español)".to_string())
    } else if l_lower.contains("fren") || l_lower.contains("francais") || l_lower.contains("français") || lang_lower == "fr" || lang_lower == "fra" || lang_lower == "fre" {
        ("fra".to_string(), "French (Français)".to_string())
    } else if l_lower.contains("germ") || l_lower.contains("deutsch") || lang_lower == "de" || lang_lower == "ger" || lang_lower == "deu" {
        ("ger".to_string(), "German (Deutsch)".to_string())
    } else if l_lower.contains("ital") || lang_lower == "it" || lang_lower == "ita" {
        ("ita".to_string(), "Italian (Italiano)".to_string())
    } else if l_lower.contains("port") || lang_lower == "pt" || lang_lower == "por" {
        ("por".to_string(), "Portuguese (Português)".to_string())
    } else if l_lower.contains("turk") || l_lower.contains("türk") || lang_lower == "tr" || lang_lower == "tur" {
        ("tur".to_string(), "Turkish (Türkçe)".to_string())
    } else if l_lower.contains("viet") || l_lower.contains("việt") || lang_lower == "vi" || lang_lower == "vie" {
        ("vie".to_string(), "Vietnamese (Tiếng Việt)".to_string())
    } else if l_lower.contains("japan") || l_lower.contains("日本語") || lang_lower == "ja" || lang_lower == "jpn" {
        ("jpn".to_string(), "Japanese (日本語)".to_string())
    } else if l_lower.contains("traditional") || lang_lower == "zht" {
        ("zho-tw".to_string(), "Chinese (Traditional / 繁體中文)".to_string())
    } else if l_lower.contains("chin") || l_lower.contains("mandarin") || l_lower.contains("中文") || lang_lower == "zh" || lang_lower == "zho" || lang_lower == "chi" {
        ("zho".to_string(), "Chinese (Simplified / 简体中文)".to_string())
    } else if l_lower.contains("kore") || l_lower.contains("한국어") || lang_lower == "ko" || lang_lower == "kor" {
        ("kor".to_string(), "Korean (한국어)".to_string())
    } else if l_lower.contains("poli") || l_lower.contains("polsk") || lang_lower == "pl" || lang_lower == "pol" {
        ("pol".to_string(), "Polish (Polski)".to_string())
    } else if l_lower.contains("thai") || l_lower.contains("ไทย") || lang_lower == "th" || lang_lower == "tha" {
        ("tha".to_string(), "Thai (ไทย)".to_string())
    } else if l_lower.contains("dutc") || l_lower.contains("neder") || lang_lower == "nl" || lang_lower == "nld" || lang_lower == "dut" {
        ("nld".to_string(), "Dutch (Nederlands)".to_string())
    } else if l_lower.contains("swed") || l_lower.contains("svensk") || lang_lower == "sv" || lang_lower == "swe" {
        ("swe".to_string(), "Swedish (Svenska)".to_string())
    } else if l_lower.contains("gree") || l_lower.contains("ελλην") || lang_lower == "el" || lang_lower == "ell" || lang_lower == "gre" {
        ("ell".to_string(), "Greek (Ελληνικά)".to_string())
    } else if l_lower.contains("hebr") || l_lower.contains("עברי") || lang_lower == "he" || lang_lower == "heb" {
        ("heb".to_string(), "Hebrew (עברית)".to_string())
    } else if l_lower.contains("pers") || l_lower.contains("fars") || l_lower.contains("فارسی") || lang_lower == "fa" || lang_lower == "fas" || lang_lower == "per" {
        ("fas".to_string(), "Persian (فارسی)".to_string())
    } else if l_lower.contains("taga") || l_lower.contains("fili") || lang_lower == "tl" || lang_lower == "tgl" || lang_lower == "fil" {
        ("fil".to_string(), "Tagalog".to_string())
    } else if l_lower.contains("ukra") || l_lower.contains("україн") || lang_lower == "uk" || lang_lower == "ukr" {
        ("ukr".to_string(), "Ukrainian (Українська)".to_string())
    } else if l_lower.contains("czec") || l_lower.contains("češ") || lang_lower == "cs" || lang_lower == "ces" || lang_lower == "cze" {
        ("ces".to_string(), "Czech (Čeština)".to_string())
    } else if l_lower.contains("hung") || l_lower.contains("magy") || lang_lower == "hu" || lang_lower == "hun" {
        ("hun".to_string(), "Hungarian (Magyar)".to_string())
    } else if l_lower.contains("roma") || l_lower.contains("româ") || lang_lower == "ro" || lang_lower == "ron" || lang_lower == "rum" {
        ("ron".to_string(), "Romanian (Română)".to_string())
    } else if l_lower.contains("dani") || l_lower.contains("dansk") || lang_lower == "da" || lang_lower == "dan" {
        ("dan".to_string(), "Danish (Dansk)".to_string())
    } else if l_lower.contains("finn") || l_lower.contains("suom") || lang_lower == "fi" || lang_lower == "fin" {
        ("fin".to_string(), "Finnish (Suomi)".to_string())
    } else if l_lower.contains("norw") || l_lower.contains("norsk") || lang_lower == "no" || lang_lower == "nor" {
        ("nor".to_string(), "Norwegian (Norsk)".to_string())
    } else if l_lower.contains("mala") || l_lower.contains("melayu") || lang_lower == "ms" || lang_lower == "msa" || lang_lower == "may" {
        ("msa".to_string(), "Malay (Bahasa Melayu)".to_string())
    } else if l_lower.contains("serb") || lang_lower == "srp" || lang_lower == "scc" {
        ("srp".to_string(), "Serbian (Srpski)".to_string())
    } else if l_lower.contains("bulg") || lang_lower == "bul" {
        ("bul".to_string(), "Bulgarian (Български)".to_string())
    } else if l_lower.contains("croat") || lang_lower == "hrv" || lang_lower == "scr" {
        ("hrv".to_string(), "Croatian (Hrvatski)".to_string())
    } else if l_lower.contains("tami") || lang_lower == "tam" {
        ("tam".to_string(), "Tamil (தமிழ்)".to_string())
    } else if l_lower.contains("beng") || lang_lower == "ben" {
        ("ben".to_string(), "Bengali (বাংলা)".to_string())
    } else if l_lower.contains("malayalam") || lang_lower == "mal" {
        ("mal".to_string(), "Malayalam (മലയാളം)".to_string())
    } else if l_lower.contains("alba") || lang_lower == "alb" || lang_lower == "sqi" {
        ("sqi".to_string(), "Albanian (Shqip)".to_string())
    } else if l_lower.contains("slov") || lang_lower == "slv" || lang_lower == "slo" {
        ("slv".to_string(), "Slovenian (Slovenščina)".to_string())
    } else {
        // Fallback: Never drop a valid subtitle track!
        let code = if !lang_lower.is_empty() {
            lang_lower
        } else {
            let filtered: String = l_lower.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
            if filtered.is_empty() { "sub".to_string() } else { filtered }
        };
        let display_name = if !label.is_empty() {
            label.to_string()
        } else if !lang.is_empty() {
            lang.to_string()
        } else {
            "Subtitle Track".to_string()
        };
        (code, display_name)
    }
}

fn map_subtitles(subs: &[serde_json::Value]) -> Vec<SubtitleOption> {
    let mut options = Vec::new();
    let mut seen_codes = HashSet::new();

    for s in subs {
        let label = s.get("label").and_then(|v| v.as_str()).unwrap_or("").trim();
        let lang = s.get("lang").and_then(|v| v.as_str()).unwrap_or("").trim();
        let src = s.get("src").and_then(|v| v.as_str()).unwrap_or("").trim();
        if src.is_empty() {
            continue;
        }

        let (code, name) = resolve_lang_meta(label, lang);

        if seen_codes.insert(code.clone()) {
            options.push(SubtitleOption {
                code,
                name: name.clone(),
                stream_label: if !label.is_empty() { label.to_string() } else { name },
                url: src.to_string(),
            });
        }
    }

    // Ensure English is first if present
    if let Some(pos) = options.iter().position(|o| o.code.starts_with("eng")) {
        let eng = options.remove(pos);
        options.insert(0, eng);
    } else {
        options.insert(0, SubtitleOption::default_english());
    }

    prioritize_subtitles(&mut options);

    options
}

fn extract_anime_id(html: &str) -> Option<String> {
    // Look for href="...-[0-9]+"
    for part in html.split("film-name") {
        if let Some(href_idx) = part.find("href=\"") {
            let after = &part[href_idx + 6..];
            if let Some(end_idx) = after.find('"') {
                let url = &after[..end_idx];
                let slug = url.split('/').next_back().unwrap_or("");
                if let Some(last_dash) = slug.rfind('-') {
                    let id_part = &slug[last_dash + 1..];
                    if id_part.chars().all(|c| c.is_ascii_digit()) && !id_part.is_empty() {
                        return Some(id_part.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Strict episode extraction — returns `None` if the requested episode does NOT exist on the provider.
fn extract_episode_id_strict(html: &str, ep_target: u32) -> Option<String> {
    let target_str = format!("data-number=\"{}\"", ep_target);
    for item in html.split("<a ") {
        if item.contains(&target_str) {
            if let Some(id_idx) = item.find("data-id=\"") {
                let after = &item[id_idx + 9..];
                if let Some(end_idx) = after.find('"') {
                    let id_str = &after[..end_idx];
                    if id_str.chars().all(|c| c.is_ascii_digit()) && !id_str.is_empty() {
                        return Some(id_str.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
fn extract_episode_id(html: &str, ep_target: u32) -> Option<String> {
    extract_episode_id_strict(html, ep_target)
}

#[allow(dead_code)]
fn extract_zoko_embed_url(html: &str, is_dub: bool) -> Option<String> {
    let mode_attr = if is_dub { "data-type=\"dub\"" } else { "data-type=\"sub\"" };
    for server_block in html.split("server-item") {
        if server_block.contains("ZokoAnime") && server_block.contains(mode_attr) {
            if let Some(hash_idx) = server_block.find("data-hash=\"") {
                let after = &server_block[hash_idx + 11..];
                if let Some(end_idx) = after.find('"') {
                    let hash_b64 = &after[..end_idx];
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(hash_b64) {
                        if let Ok(url) = String::from_utf8(bytes) {
                            return Some(url);
                        }
                    }
                }
            }
        }
    }
    None
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_lang_meta_arabic() {
        let (code, name) = resolve_lang_meta("Arabic", "");
        assert_eq!(code, "ara");
        assert_eq!(name, "Arabic (العربية)");
    }

    #[test]
    fn test_resolve_lang_meta_spanish_variants() {
        let (code1, name1) = resolve_lang_meta("Spanish (- Espanol)", "");
        assert_eq!(code1, "spa");
        assert_eq!(name1, "Spanish (Español)");

        let (code2, name2) = resolve_lang_meta("Spanish (- Espanol (Latin American))", "");
        assert_eq!(code2, "spa-419");
        assert_eq!(name2, "Spanish (Latin America)");

        let (code3, name3) = resolve_lang_meta("Spanish (- Espanol(Espana))", "");
        assert_eq!(code3, "spa-es");
        assert_eq!(name3, "Spanish (Castilian/Spain)");
    }

    #[test]
    fn test_resolve_lang_meta_community_codes() {
        let (c_ar, n_ar) = resolve_lang_meta("ara", "");
        assert_eq!(c_ar, "ara");
        assert_eq!(n_ar, "Arabic (العربية)");

        let (c_pt, n_pt) = resolve_lang_meta("pob", "");
        assert_eq!(c_pt, "por-br");
        assert_eq!(n_pt, "Portuguese (Brazil)");

        let (c_ja, n_ja) = resolve_lang_meta("jpn", "");
        assert_eq!(c_ja, "jpn");
        assert_eq!(n_ja, "Japanese (日本語)");

        let (c_ru, n_ru) = resolve_lang_meta("rus", "");
        assert_eq!(c_ru, "rus");
        assert_eq!(n_ru, "Russian (Русский)");
    }

    #[test]
    fn test_map_subtitles_ensures_english_first() {
        let json = serde_json::json!([
            {"label": "Arabic", "src": "https://example.com/ar.vtt"},
            {"label": "English", "src": "https://example.com/en.vtt"},
            {"label": "French (- Francais(France))", "src": "https://example.com/fr.vtt"}
        ]);
        let options = map_subtitles(json.as_array().unwrap());
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].code, "eng");
        assert_eq!(options[0].lang_keyword(), "English");
        assert_eq!(options[1].code, "ara");
        assert_eq!(options[1].lang_keyword(), "Arabic");
        assert_eq!(options[2].code, "fra");
        assert_eq!(options[2].lang_keyword(), "French");
    }

    #[test]
    fn test_lang_keywords_for_ani_cli() {
        let opt_ara = SubtitleOption {
            code: "ara".to_string(),
            name: "Arabic (العربية)".to_string(),
            stream_label: "Arabic".to_string(),
            url: "https://example.com/ar.vtt".to_string(),
        };
        assert_eq!(opt_ara.lang_keyword(), "Arabic");

        let opt_spa = SubtitleOption {
            code: "spa-419".to_string(),
            name: "Spanish (Latin America)".to_string(),
            stream_label: "Spanish (- Latin American)".to_string(),
            url: "https://example.com/es.vtt".to_string(),
        };
        assert_eq!(opt_spa.lang_keyword(), "Spanish");
    }

    #[test]
    fn test_extract_episode_id_order_independent() {
        let html1 = r##"<a title="Episode 1" class="ssl-item ep-item" data-number="1" data-id="3086" href="#">"##;
        assert_eq!(extract_episode_id(html1, 1), Some("3086".to_string()));

        let html2 = r##"<a data-id="9999" data-number="2" class="ssl-item" href="#">"##;
        assert_eq!(extract_episode_id(html2, 2), Some("9999".to_string()));
        // Missing episodes MUST return None (no fallback to wrong episode!)
        assert_eq!(extract_episode_id_strict(html2, 99), None);
    }

    #[test]
    fn test_filter_verified_providers_removes_unavailable_sources() {
        let mut raw = default_providers();
        raw[0].master_url = Some("https://cdn.example.com/master.m3u8".into());
        raw[0].is_verified = true;
        // raw[1..4] have is_verified = false or master_url = None
        let filtered = filter_verified_providers(raw);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "zoko");
    }

    #[test]
    fn test_parse_m3u8_qualities_only_actual_resolutions() {
        let m3u8_jjk = "#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080\n1080/index.m3u8\n";
        let quals = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_jjk);
        assert_eq!(quals.len(), 1);
        assert_eq!(quals[0].label, "1080p");
        assert_eq!(quals[0].url, "https://cdn.example.com/stream/1080/index.m3u8");

        let m3u8_multi = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360\n360/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080\n1080/index.m3u8\n";
        let quals_multi = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_multi);
        assert_eq!(quals_multi.len(), 3);
        assert_eq!(quals_multi[0].label, "1080p");
        assert_eq!(quals_multi[1].label, "720p");
        assert_eq!(quals_multi[2].label, "360p");

        let m3u8_older = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=1056x800\n800/index.m3u8\n";
        let quals_older = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_older);
        assert_eq!(quals_older.len(), 1);
        assert_eq!(quals_older[0].label, "800p");
    }

    #[test]
    fn test_prioritize_subtitles_orders_core_languages_first() {
        let mut subs = vec![
            SubtitleOption { code: "spa".to_string(), name: "Spanish".to_string(), stream_label: "Spanish".to_string(), url: "spa.vtt".to_string() },
            SubtitleOption { code: "rus".to_string(), name: "Russian".to_string(), stream_label: "Russian".to_string(), url: "rus.srt".to_string() },
            SubtitleOption { code: "fre".to_string(), name: "French".to_string(), stream_label: "French".to_string(), url: "fre.vtt".to_string() },
            SubtitleOption { code: "eng".to_string(), name: "English".to_string(), stream_label: "English".to_string(), url: "eng.vtt".to_string() },
            SubtitleOption { code: "ind".to_string(), name: "Indonesian".to_string(), stream_label: "Indonesian".to_string(), url: "ind.srt".to_string() },
            SubtitleOption { code: "ara".to_string(), name: "Arabic".to_string(), stream_label: "Arabic".to_string(), url: "ara.vtt".to_string() },
        ];

        prioritize_subtitles(&mut subs);

        let codes: Vec<&str> = subs.iter().map(|s| s.code.as_str()).collect();
        assert_eq!(codes, vec!["eng", "ara", "rus", "ind", "spa", "fre"]);
    }

    #[test]
    fn test_matches_episode() {
        assert!(matches_episode("0", "[Para Exo] Serial Experiments Lain 01 [BD].ass", 1));
        assert!(!matches_episode("0", "[Para Exo] Serial Experiments Lain 02 [BD].ass", 1));
        assert!(matches_episode("0", "Serial.Experiments.Lain.Layer01.AR.srt", 1));
        assert!(matches_episode("0", "Welcome To The NHK - 01.ass", 1));
        assert!(!matches_episode("0", "Welcome To The NHK - 11.ass", 1));
        assert!(matches_episode("1", "random_filename.srt", 1));
        assert!(!matches_episode("2", "random_filename.srt", 1));
    }

    #[tokio::test]
    async fn test_fetch_external_subtitles_finds_arabic_for_lain() {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let subs = fetch_external_subtitles(&client, 339, "Serial Experiments Lain", 1).await;
        let has_ara = subs.iter().any(|s| s.code.starts_with("ara"));
        assert!(has_ara, "Expected Arabic subtitle for Serial Experiments Lain Ep 1");
    }

    #[tokio::test]
    async fn test_fetch_external_subtitles_finds_languages_for_nhk() {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let subs = fetch_external_subtitles(&client, 1210, "Welcome to the N.H.K", 1).await;
        let has_ara = subs.iter().any(|s| s.code.starts_with("ara"));
        let has_rus = subs.iter().any(|s| s.code.starts_with("rus"));
        assert!(has_ara || has_rus, "Expected Arabic or Russian subtitle for Welcome to the N.H.K Ep 1");
    }

    #[test]
    fn test_parse_m3u8_audio_tracks_and_prioritization() {
        let manifest = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="Spanish (Latin America)",LANGUAGE="es-419",DEFAULT=NO,AUTOSELECT=YES,URI="audio/es-419/index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="English",LANGUAGE="en",DEFAULT=NO,AUTOSELECT=YES,URI="audio/eng/index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="French (France)",LANGUAGE="fr",DEFAULT=NO,AUTOSELECT=YES,URI="audio/fra/index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="Japanese",LANGUAGE="ja",DEFAULT=YES,AUTOSELECT=YES,URI="audio/jpn/index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080,AUDIO="aud"
1080/index.m3u8
"#;
        let tracks = parse_m3u8_audio_tracks(manifest);
        assert_eq!(tracks.len(), 4);
        assert_eq!(tracks[0].code, "jpn");
        assert!(tracks[0].is_default);
        assert_eq!(tracks[1].code, "eng");
        assert_eq!(tracks[2].code, "spa-419");
        assert_eq!(tracks[3].code, "fra");
    }

    #[test]
    fn test_parse_m3u8_ladder_and_two_step_downshift() {
        let manifest = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="Japanese",LANGUAGE="ja",DEFAULT=YES,URI="audio/jpn/index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="English",LANGUAGE="en",DEFAULT=NO,URI="audio/eng/index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=6400000,AVERAGE-BANDWIDTH=5600000,RESOLUTION=1920x1080,FRAME-RATE=60.000,CODECS="avc1.64002a",AUDIO="aud"
1080_60/index.m3u8?exp=2000000000
#EXT-X-STREAM-INF:BANDWIDTH=3400000,RESOLUTION=1920x1080,FRAME-RATE=23.976,CODECS="vp09.00.41.08",AUDIO="aud"
1080_24/index.m3u8?exp=2000000000
#EXT-X-STREAM-INF:BANDWIDTH=1800000,RESOLUTION=1280x720,FRAME-RATE=23.976,CODECS="av01.0.05M.08",AUDIO="aud"
720/index.m3u8?exp=2000000000
#EXT-X-STREAM-INF:BANDWIDTH=850000,RESOLUTION=854x480,FRAME-RATE=24.000,CODECS="avc1.4d401f",AUDIO="aud"
480/index.m3u8?exp=2000000000
"#;
        let ladder = parse_m3u8_ladder(
            "https://cdn.example.com/v1/master.m3u8",
            manifest,
            154587,
            12,
            false,
            1_900_000_000,
        );
        assert!(ladder.is_fresh(1_900_000_100));
        assert!(!ladder.is_fresh(1_900_000_700)); // > 600s TTL
        assert!(!ladder.is_fresh(1_999_999_950)); // < 120s before exp=2000000000
        assert_eq!(ladder.video_variants.len(), 4);
        assert_eq!(ladder.audio_variants.len(), 2);

        // Normal selects highest 1080p/60fps
        let best_full = ladder.select_best_variant(None, crate::config::NetworkMode::Normal).unwrap();
        assert_eq!(best_full.height, 1080);
        assert_eq!(best_full.fps_x10, 600);
        assert_eq!(best_full.tbr_kbps, 5600);

        // CongestedWifi clamps to <=720p and <=30fps
        let best_wifi = ladder.select_best_variant(None, crate::config::NetworkMode::CongestedWifi).unwrap();
        assert_eq!(best_wifi.height, 720);

        // Step 1 downshift from 1080p60 drops to 1080p24 first (same height, lower FPS & bitrate)
        let step1 = ladder.select_lower_tier(1080, 600, 5600).unwrap();
        assert_eq!(step1.height, 1080);
        assert_eq!(step1.fps_x10, 240);
        assert_eq!(step1.tbr_kbps, 3400);

        // Step 2 downshift from 1080p24 drops to 720p
        let step2 = ladder.select_lower_tier(1080, 240, 3400).unwrap();
        assert_eq!(step2.height, 720);
        assert_eq!(step2.tbr_kbps, 1800);

        // Step 3 drops to 480p
        let step3 = ladder.select_lower_tier(720, 240, 1800).unwrap();
        assert_eq!(step3.height, 480);

        // Bottom rung returns None
        assert!(ladder.select_lower_tier(480, 240, 850).is_none());

        // Audio URI resolution respects dub vs sub
        assert_eq!(
            ladder.audio_uri_for(step2, false).as_deref(),
            Some("https://cdn.example.com/v1/audio/jpn/index.m3u8")
        );
        assert_eq!(
            ladder.audio_uri_for(step2, true).as_deref(),
            Some("https://cdn.example.com/v1/audio/eng/index.m3u8")
        );
    }
}
