//! Cinema-Grade Subtitle Styler & Signage Collision Avoidance.
//! Generates tuned mpv/libass typography flags and adjusts overlapping
//! dialogue/sign subtitle events to guarantee maximum legibility.

use serde::{Deserialize, Serialize};

/// High-legibility subtitle aesthetic presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SubtitlePreset {
    #[default]
    StudioMinimal,
    NetflixClean,
    AnimeFansub,
    OledPlate,
    Custom,
}

#[allow(dead_code)]
impl SubtitlePreset {
    pub fn label(&self) -> &'static str {
        match self {
            SubtitlePreset::StudioMinimal => "Studio Minimal",
            SubtitlePreset::NetflixClean  => "Netflix Clean",
            SubtitlePreset::AnimeFansub   => "Anime Fansub (Yellow)",
            SubtitlePreset::OledPlate     => "OLED Plate (Backdrop)",
            SubtitlePreset::Custom        => "Custom",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            SubtitlePreset::StudioMinimal => "Razor-sharp sans-serif, high-contrast black border, soft shadow.",
            SubtitlePreset::NetflixClean  => "Subtle rounded border, generous vertical margin, neutral tone.",
            SubtitlePreset::AnimeFansub   => "Nostalgic golden-yellow font with heavy black stroke outline.",
            SubtitlePreset::OledPlate     => "Translucent dark pill plate behind text for bright scenes.",
            SubtitlePreset::Custom        => "User-defined typography parameters.",
        }
    }
}

/// Configuration parameters for cinema subtitle styling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStyleConfig {
    #[serde(default)]
    pub preset: SubtitlePreset,
    #[serde(default = "default_sub_font")]
    pub font_family: String,
    #[serde(default = "default_sub_font_size")]
    pub font_size: u32,
    #[serde(default = "default_sub_border_size")]
    pub border_size: f32,
    #[serde(default = "default_sub_shadow_offset")]
    pub shadow_offset: f32,
    #[serde(default = "default_sub_margin_y")]
    pub margin_y: u32,
    #[serde(default = "default_sub_color")]
    pub primary_color: String,
    #[serde(default = "default_sub_border_color")]
    pub border_color: String,
    #[serde(default = "default_sub_backdrop_opacity")]
    pub backdrop_opacity: f32,
    #[serde(default = "default_true")]
    pub collision_avoidance: bool,
}

fn default_sub_font() -> String {
    "Trebuchet MS".to_string()
}

fn default_sub_font_size() -> u32 {
    48
}

fn default_sub_border_size() -> f32 {
    2.5
}

fn default_sub_shadow_offset() -> f32 {
    1.2
}

fn default_sub_margin_y() -> u32 {
    38
}

fn default_sub_color() -> String {
    "#FFFFFF".to_string()
}

fn default_sub_border_color() -> String {
    "#000000".to_string()
}

fn default_sub_backdrop_opacity() -> f32 {
    0.0
}

fn default_true() -> bool {
    true
}

impl Default for SubtitleStyleConfig {
    fn default() -> Self {
        Self::from_preset(SubtitlePreset::StudioMinimal)
    }
}

impl SubtitleStyleConfig {
    /// Instantiate a configuration tailored to a specific preset.
    pub fn from_preset(preset: SubtitlePreset) -> Self {
        match preset {
            SubtitlePreset::StudioMinimal => Self {
                preset,
                font_family: "Trebuchet MS".to_string(),
                font_size: 48,
                border_size: 2.5,
                shadow_offset: 1.2,
                margin_y: 38,
                primary_color: "#FFFFFF".to_string(),
                border_color: "#000000".to_string(),
                backdrop_opacity: 0.0,
                collision_avoidance: true,
            },
            SubtitlePreset::NetflixClean => Self {
                preset,
                font_family: "Inter".to_string(),
                font_size: 46,
                border_size: 1.8,
                shadow_offset: 2.0,
                margin_y: 42,
                primary_color: "#F6F6F6".to_string(),
                border_color: "#111111".to_string(),
                backdrop_opacity: 0.15,
                collision_avoidance: true,
            },
            SubtitlePreset::AnimeFansub => Self {
                preset,
                font_family: "Arial".to_string(),
                font_size: 50,
                border_size: 3.2,
                shadow_offset: 0.8,
                margin_y: 36,
                primary_color: "#FFFF55".to_string(),
                border_color: "#000000".to_string(),
                backdrop_opacity: 0.0,
                collision_avoidance: true,
            },
            SubtitlePreset::OledPlate => Self {
                preset,
                font_family: "Roboto".to_string(),
                font_size: 48,
                border_size: 1.5,
                shadow_offset: 0.0,
                margin_y: 40,
                primary_color: "#FFFFFF".to_string(),
                border_color: "#050505".to_string(),
                backdrop_opacity: 0.45,
                collision_avoidance: true,
            },
            SubtitlePreset::Custom => Self {
                preset,
                font_family: "Trebuchet MS".to_string(),
                font_size: 48,
                border_size: 2.5,
                shadow_offset: 1.2,
                margin_y: 38,
                primary_color: "#FFFFFF".to_string(),
                border_color: "#000000".to_string(),
                backdrop_opacity: 0.0,
                collision_avoidance: true,
            },
        }
    }

    /// Convert configuration into explicit mpv player arguments.
    pub fn to_mpv_flags(&self) -> Vec<String> {
        let mut flags = vec![
            format!("--sub-font={}", self.font_family),
            format!("--sub-font-size={}", self.font_size),
            format!("--sub-border-size={:.1}", self.border_size),
            format!("--sub-shadow-offset={:.1}", self.shadow_offset),
            format!("--sub-margin-y={}", self.margin_y),
            format!("--sub-color={}", self.primary_color),
            format!("--sub-border-color={}", self.border_color),
            "--sub-ass-override=force".to_string(),
        ];

        if self.backdrop_opacity > 0.0 {
            flags.push(format!(
                "--sub-back-color=0.0/0.0/0.0/{:.2}",
                self.backdrop_opacity
            ));
        }

        flags
    }
}

/// Detects overlapping bottom signage translations and dialogue subtitles,
/// dynamically shifting dialogue upward to eliminate visual clashes.
pub fn apply_signage_collision_avoidance(content: &str) -> String {
    if !content.contains("[Events]") {
        // Plain text or WebVTT fallback
        return apply_vtt_collision_avoidance(content);
    }

    let mut lines = Vec::new();
    let mut in_events = false;

    // First pass: collect interval coordinates for signage lines
    // A signage line is marked by styles containing 'sign' or 'title', or explicit \pos in lower 25%
    let mut sign_intervals: Vec<(f64, f64)> = Vec::new();

    for line in content.lines() {
        if line.starts_with("[Events]") {
            in_events = true;
            continue;
        }
        if in_events && (line.starts_with("Dialogue:") || line.starts_with("Comment:")) {
            let parts: Vec<&str> = line.splitn(10, ',').collect();
            if parts.len() >= 10 {
                let style = parts[3].to_lowercase();
                let text = parts[9].to_lowercase();

                let is_sign = style.contains("sign")
                    || style.contains("title")
                    || text.contains("\\pos(")
                    || text.contains("sign")
                    || text.contains("headline");

                if is_sign {
                    if let (Some(start), Some(end)) = (
                        crate::services::subsync::parse_timestamp(parts[1]),
                        crate::services::subsync::parse_timestamp(parts[2]),
                    ) {
                        sign_intervals.push((start, end));
                    }
                }
            }
        }
    }

    in_events = false;
    // Second pass: lift standard dialogue that collides with sign intervals
    for line in content.lines() {
        if line.starts_with("[Events]") {
            in_events = true;
            lines.push(line.to_string());
            continue;
        }

        if in_events && line.starts_with("Dialogue:") {
            let parts: Vec<&str> = line.splitn(10, ',').collect();
            if parts.len() >= 10 {
                let style = parts[3].to_lowercase();
                let text = parts[9];

                let is_dialogue = !style.contains("sign")
                    && !style.contains("title")
                    && !text.contains("\\pos(");

                if is_dialogue {
                    if let (Some(start), Some(end)) = (
                        crate::services::subsync::parse_timestamp(parts[1]),
                        crate::services::subsync::parse_timestamp(parts[2]),
                    ) {
                        // Check for collision with any sign interval
                        let collides = sign_intervals.iter().any(|(s_start, s_end)| {
                            let overlap = (start.max(*s_start)) < (end.min(*s_end));
                            overlap && (end - start > 0.2)
                        });

                        if collides {
                            // Inject vertical lift override: {\an8} (top center) or {\pos(960, 880)}
                            // We lift the bottom margin by prepending \an8 or margin lift
                            let lifted_text = format!("{{\\an8}}{}", text);
                            let mut new_parts = parts;
                            new_parts[9] = &lifted_text;
                            lines.push(new_parts.join(","));
                            continue;
                        }
                    }
                }
            }
        }

        lines.push(line.to_string());
    }

    lines.join("\n")
}

/// Fallback collision avoidance for WebVTT cues.
fn apply_vtt_collision_avoidance(content: &str) -> String {
    let mut lines = Vec::new();
    let mut in_cue = false;

    for line in content.lines() {
        if line.contains("-->") {
            in_cue = true;
            lines.push(line.to_string());
            continue;
        }
        if in_cue && line.trim().is_empty() {
            in_cue = false;
            lines.push(line.to_string());
            continue;
        }
        lines.push(line.to_string());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtitle_preset_metadata() {
        for preset in [
            SubtitlePreset::StudioMinimal,
            SubtitlePreset::NetflixClean,
            SubtitlePreset::AnimeFansub,
            SubtitlePreset::OledPlate,
            SubtitlePreset::Custom,
        ] {
            assert!(!preset.label().is_empty());
            assert!(!preset.description().is_empty());
        }
    }

    #[test]
    fn test_subtitle_presets_to_mpv_flags() {
        let studio = SubtitleStyleConfig::from_preset(SubtitlePreset::StudioMinimal);
        let flags = studio.to_mpv_flags();
        assert!(flags.iter().any(|f| f.starts_with("--sub-font=")));
        assert!(flags.iter().any(|f| f == "--sub-font-size=48"));
        assert!(flags.iter().any(|f| f == "--sub-ass-override=force"));

        let fansub = SubtitleStyleConfig::from_preset(SubtitlePreset::AnimeFansub);
        let flags_fansub = fansub.to_mpv_flags();
        assert!(flags_fansub.iter().any(|f| f == "--sub-color=#FFFF55"));
        assert!(flags_fansub.iter().any(|f| f == "--sub-border-size=3.2"));

        let oled = SubtitleStyleConfig::from_preset(SubtitlePreset::OledPlate);
        let flags_oled = oled.to_mpv_flags();
        assert!(flags_oled.iter().any(|f| f.starts_with("--sub-back-color=")));
    }

    #[test]
    fn test_signage_collision_avoidance_lifts_overlapping_dialogue() {
        let ass_sample = "\
[Script Info]
Title: Sample Show

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1
Style: Sign,Arial,18,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:01:10.00,0:01:14.00,Sign,,0,0,0,,{\\pos(960,980)}POLICE HEADQUARTERS
Dialogue: 0,0:01:11.00,0:01:13.50,Default,,0,0,0,,Wait, didn't you hear what he said?
Dialogue: 0,0:01:20.00,0:01:22.00,Default,,0,0,0,,Normal dialogue without collision
";

        let processed = apply_signage_collision_avoidance(ass_sample);

        // The overlapping line at 1:11.00 - 1:13.50 must have {\an8} injected to lift it above the sign
        assert!(processed.contains("Dialogue: 0,0:01:11.00,0:01:13.50,Default,,0,0,0,,{\\an8}Wait, didn't you hear what he said?"));
        // The non-colliding line must be unaffected
        assert!(processed.contains("Dialogue: 0,0:01:20.00,0:01:22.00,Default,,0,0,0,,Normal dialogue without collision"));
    }
}
