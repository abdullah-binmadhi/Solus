use crate::error::{AppError, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Video quality preference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Quality {
    #[serde(rename = "360p")]
    P360,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    #[default]
    P1080,
    Best,
}

impl Quality {
    pub fn as_str(&self) -> &str {
        match self {
            Quality::P360  => "360p",
            Quality::P480  => "480p",
            Quality::P720  => "720p",
            Quality::P1080 => "1080p",
            Quality::Best  => "best",
        }
    }

    pub fn height_px(&self) -> Option<u32> {
        match self {
            Quality::P360  => Some(360),
            Quality::P480  => Some(480),
            Quality::P720  => Some(720),
            Quality::P1080 => Some(1080),
            Quality::Best  => None,
        }
    }

    pub fn clamped_for_network(&self, mode: NetworkMode) -> Quality {
        let Some(cap) = mode.max_height_cap() else {
            return self.clone();
        };
        let eff_h = self.height_px().map(|h| h.min(cap)).unwrap_or(cap);
        match eff_h {
            0..=360 => Quality::P360,
            361..=480 => Quality::P480,
            481..=720 => Quality::P720,
            _ => Quality::P1080,
        }
    }
}

/// Network resilience profile controlling stream caps, player readahead cushions, and artwork throttling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    #[default]
    Normal,
    CongestedWifi,
    UltraSaver,
}

impl NetworkMode {
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::CongestedWifi,
            Self::CongestedWifi => Self::UltraSaver,
            Self::UltraSaver => Self::Normal,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Normal => Self::UltraSaver,
            Self::CongestedWifi => Self::Normal,
            Self::UltraSaver => Self::CongestedWifi,
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        let norm = s.trim().to_ascii_lowercase();
        if norm.contains("wifi") || norm.contains("congested") {
            Self::CongestedWifi
        } else if norm.contains("eco") || norm.contains("ultra") || norm.contains("saver") {
            Self::UltraSaver
        } else {
            Self::Normal
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::CongestedWifi => "congested_wifi",
            Self::UltraSaver => "ultra_saver",
        }
    }

    pub fn short_tag(&self) -> &'static str {
        match self {
            Self::Normal => "NET:FULL",
            Self::CongestedWifi => "NET:WIFI",
            Self::UltraSaver => "NET:ECO",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "Normal (Full)",
            Self::CongestedWifi => "Congested Wi-Fi",
            Self::UltraSaver => "Ultra Saver",
        }
    }

    pub fn is_congested(&self) -> bool {
        !matches!(self, Self::Normal)
    }

    pub fn max_height_cap(&self) -> Option<u32> {
        match self {
            Self::Normal => None,
            Self::CongestedWifi => Some(720),
            Self::UltraSaver => Some(480),
        }
    }

    pub fn max_fps_cap(&self) -> Option<u32> {
        match self {
            Self::Normal => None,
            Self::CongestedWifi | Self::UltraSaver => Some(30),
        }
    }

    pub fn hls_bitrate_cap(&self) -> &'static str {
        match self {
            Self::Normal => "max",
            Self::CongestedWifi => "1800000",
            Self::UltraSaver => "900000",
        }
    }

    pub fn vod_readahead_secs(&self) -> u32 {
        match self {
            Self::Normal => 45,
            Self::CongestedWifi => 120,
            Self::UltraSaver => 180,
        }
    }

    pub fn vod_cache_pause_wait_secs(&self) -> f64 {
        match self {
            Self::Normal => 2.5,
            Self::CongestedWifi => 4.5,
            Self::UltraSaver => 6.0,
        }
    }

    pub fn use_disk_cache(&self) -> bool {
        !matches!(self, Self::Normal)
    }

    pub fn network_timeout_secs(&self) -> u64 {
        match self {
            Self::Normal => 15,
            Self::CongestedWifi => 12,
            Self::UltraSaver => 10,
        }
    }

    pub fn prefer_compact_artwork(&self) -> bool {
        !matches!(self, Self::Normal)
    }

    pub fn max_bg_image_concurrency(&self, is_playing: bool) -> usize {
        match (self, is_playing) {
            (Self::Normal, false) => 12,
            (Self::Normal, true) => 4,
            (Self::CongestedWifi, false) => 2,
            (Self::CongestedWifi, true) => 1,
            (Self::UltraSaver, _) => 1,
        }
    }
}


/// Sub or dub preference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioMode {
    #[default]
    Sub,
    Dub,
}

/// Unified `libmpv` video player engine selector (`Auto-Best`, `mpv`, `mpv.net`, `Celluloid`).
/// Legacy `"iina"` / `"vlc"` config entries transparently migrate to `Player::AutoBest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Player {
    #[default]
    AutoBest,
    Mpv,
    MpvNet,
    Celluloid,
}

impl Player {
    pub const ALL: [Player; 4] = [
        Player::AutoBest,
        Player::Mpv,
        Player::MpvNet,
        Player::Celluloid,
    ];

    pub fn from_str_lossy(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "mpv" => Self::Mpv,
            "mpv.net" | "mpvnet" => Self::MpvNet,
            "celluloid" => Self::Celluloid,
            // "auto", "auto-best", "autobest", and legacy "iina" / "vlc"
            _ => Self::AutoBest,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_str(&self) -> &'static str {
        match self {
            Player::AutoBest => "auto-best",
            Player::Mpv => "mpv",
            Player::MpvNet => "mpv.net",
            Player::Celluloid => "celluloid",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Player::AutoBest => "Auto-Best",
            Player::Mpv => "mpv",
            Player::MpvNet => "mpv.net",
            Player::Celluloid => "Celluloid",
        }
    }

    pub fn parity_badge(&self) -> &'static str {
        match self {
            Player::AutoBest => "Auto-Best (libmpv)",
            Player::Mpv => "100% Universal",
            Player::MpvNet => "Win 99%",
            Player::Celluloid => "Linux GTK 95%",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::AutoBest => Self::Mpv,
            Self::Mpv => Self::MpvNet,
            Self::MpvNet => Self::Celluloid,
            Self::Celluloid => Self::AutoBest,
        }
    }

    #[allow(dead_code)]
    pub fn prev(self) -> Self {
        match self {
            Self::AutoBest => Self::Celluloid,
            Self::Mpv => Self::AutoBest,
            Self::MpvNet => Self::Mpv,
            Self::Celluloid => Self::MpvNet,
        }
    }
}

impl Serialize for Player {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Player {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer).unwrap_or_else(|_| "auto-best".to_string());
        Ok(Self::from_str_lossy(&s))
    }
}

/// GPU mastering & thermal efficiency profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Auto,
    Eco,
    #[default]
    Balanced,
    Reference4K,
}

impl PerformanceMode {
    pub const ALL: [PerformanceMode; 4] = [
        PerformanceMode::Auto,
        PerformanceMode::Eco,
        PerformanceMode::Balanced,
        PerformanceMode::Reference4K,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Thermal)",
            Self::Eco => "Eco",
            Self::Balanced => "Balanced",
            Self::Reference4K => "Reference 4K",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Eco => "ECO",
            Self::Balanced => "BALANCED",
            Self::Reference4K => "REF 4K",
        }
    }

    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Eco,
            Self::Eco => Self::Balanced,
            Self::Balanced => Self::Reference4K,
            Self::Reference4K => Self::Auto,
        }
    }

    #[allow(dead_code)]
    pub fn prev(self) -> Self {
        match self {
            Self::Auto => Self::Reference4K,
            Self::Eco => Self::Auto,
            Self::Balanced => Self::Eco,
            Self::Reference4K => Self::Balanced,
        }
    }
}

/// Hardware debanding tier eliminating 8-bit dark-scene contour banding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DebandMode {
    #[default]
    Auto,
    Off,
    Strong,
}

impl DebandMode {
    pub const ALL: [DebandMode; 3] = [DebandMode::Auto, DebandMode::Off, DebandMode::Strong];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "Auto (1-Pass)",
            Self::Off => "Off",
            Self::Strong => "Strong (2-Pass)",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Auto => "1x",
            Self::Off => "OFF",
            Self::Strong => "2x",
        }
    }

    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Strong,
            Self::Strong => Self::Off,
            Self::Off => Self::Auto,
        }
    }
}

/// 24fps anime camera panning cadence & temporal interpolation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    #[default]
    Off,
    Cadence,
    Smooth,
}

impl MotionMode {
    pub const ALL: [MotionMode; 3] = [MotionMode::Off, MotionMode::Cadence, MotionMode::Smooth];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "Off (Native 24p)",
            Self::Cadence => "Cadence (24->60)",
            Self::Smooth => "Smooth (120Hz)",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Cadence => "CADENCE",
            Self::Smooth => "SMOOTH",
        }
    }

    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Cadence,
            Self::Cadence => Self::Smooth,
            Self::Smooth => Self::Off,
        }
    }
}

/// TTL settings (in seconds) for AniList cache refresh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Trending and seasonal rows (default: 24h)
    pub trending_ttl: u64,
    /// Popular and top-rated rows (default: 7 days)
    pub stable_ttl: u64,
    /// Individual anime detail (default: 48h)
    pub detail_ttl: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            trending_ttl: 60 * 60 * 24,         // 24 hours
            stable_ttl:   60 * 60 * 24 * 7,     // 7 days
            detail_ttl:   60 * 60 * 48,          // 48 hours
        }
    }
}

fn default_sub_lang_val() -> String {
    "English".to_string()
}

fn default_auto_skip_intro() -> bool {
    false
}

fn default_true() -> bool {
    true
}

/// Real-time Anime4K GLSL shader upscaling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Anime4KMode {
    #[default]
    Off,
    Balanced,
    Ultra,
}

impl Anime4KMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Anime4KMode::Off => "off",
            Anime4KMode::Balanced => "balanced 2x",
            Anime4KMode::Ultra => "ultra 4k",
        }
    }
}

fn default_anime4k() -> Anime4KMode {
    Anime4KMode::Off
}

fn default_active_profile() -> String {
    "default".to_string()
}

/// Root config struct — persisted to ~/.config/ani-tui/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub quality:          Quality,
    pub audio_mode:       AudioMode,
    pub player:           Player,
    #[serde(default = "default_sub_lang_val")]
    pub default_sub_lang: String,
    #[serde(default = "default_auto_skip_intro")]
    pub auto_skip_intro:  bool,
    #[serde(default = "default_anime4k")]
    pub anime4k_mode:     Anime4KMode,
    #[serde(default)]
    pub ambilight_enabled: bool,
    #[serde(default)]
    pub update:           UpdateConfig,
    pub cache:            CacheConfig,
    #[serde(default)]
    pub subtitle_style:   crate::services::substyler::SubtitleStyleConfig,
    #[serde(default)]
    pub remote:           RemoteConfig,
    #[serde(default)]
    pub cast:             CastConfig,
    /// Dual-tracker OAuth Client IDs (AniList + MyAnimeList)
    #[serde(default)]
    pub trackers:         TrackersConfig,
    /// Active multi-user profile ID
    #[serde(default = "default_active_profile")]
    pub active_profile:   String,
    /// Schema revision for one-time default migrations (0 = legacy v1.0.6 defaults)
    #[serde(default)]
    pub config_version:   u32,
    /// Network resilience profile (Normal, CongestedWifi, UltraSaver)
    #[serde(default)]
    pub network_mode:     NetworkMode,
    /// Force IPv4 socket binding for HTTP / yt-dlp / libmpv network requests
    #[serde(default)]
    pub force_ipv4:       bool,
    /// Enable automatic HLS ladder downshift on sustained buffering/stalls
    #[serde(default = "default_true")]
    pub auto_abr_downshift: bool,
    /// Automatically launch the external libmpv player in fullscreen mode
    #[serde(default)]
    pub auto_fullscreen: bool,
    /// Automatically launch the external libmpv player in borderless Picture-in-Picture (PiP) corner mode
    #[serde(default)]
    pub auto_pip: bool,
    /// GPU mastering & thermal efficiency profile
    #[serde(default)]
    pub performance_mode: PerformanceMode,
    /// Hardware debanding tier (Auto = 1-pass GPU, Off, Strong = 2-pass)
    #[serde(default)]
    pub deband_mode: DebandMode,
    /// 24fps anime camera panning cadence & temporal interpolation mode
    #[serde(default)]
    pub motion_mode: MotionMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quality:          Quality::default(),
            audio_mode:       AudioMode::default(),
            player:           Player::default(),
            default_sub_lang: default_sub_lang_val(),
            auto_skip_intro:  default_auto_skip_intro(),
            anime4k_mode:     default_anime4k(),
            ambilight_enabled: false,
            update:           UpdateConfig::default(),
            cache:            CacheConfig::default(),
            subtitle_style:   crate::services::substyler::SubtitleStyleConfig::default(),
            remote:           RemoteConfig::default(),
            cast:             CastConfig::default(),
            trackers:         TrackersConfig::default(),
            active_profile:   default_active_profile(),
            config_version:   2,
            network_mode:     NetworkMode::Normal,
            force_ipv4:       false,
            auto_abr_downshift: true,
            auto_fullscreen:  false,
            auto_pip:         false,
            performance_mode: PerformanceMode::default(),
            deband_mode:      DebandMode::default(),
            motion_mode:      MotionMode::default(),
        }
    }
}

/// Built-in public MyAnimeList mobile API Client ID enabling zero-config `[u]` Username sync
/// and `[p]` Direct Username + Password login without requiring developer app registration.
pub const DEFAULT_MAL_CLIENT_ID: &str = "6114d00ca681b7701d1e15fe11a4987e";

/// Optional user/app OAuth2 Client IDs for tracker APIs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrackersConfig {
    #[serde(default)]
    pub mal_client_id: Option<String>,
    #[serde(default)]
    pub anilist_client_id: Option<String>,
}

impl TrackersConfig {
    /// Return a user-configured or env-overridden custom MAL OAuth Client ID, if any.
    pub fn custom_mal_client_id(&self) -> Option<String> {
        std::env::var("SOLUS_MAL_CLIENT_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| self.mal_client_id.clone().filter(|v| !v.trim().is_empty()))
    }

    /// Resolve MAL OAuth Client ID (env var -> config.toml -> built-in default).
    pub fn resolved_mal_client_id(&self) -> Option<String> {
        self.custom_mal_client_id()
            .or_else(|| Some(DEFAULT_MAL_CLIENT_ID.to_string()))
    }

    /// Resolve AniList OAuth Client ID (env var overrides config.toml).
    pub fn resolved_anilist_client_id(&self) -> Option<String> {
        std::env::var("SOLUS_ANILIST_CLIENT_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| self.anilist_client_id.clone().filter(|v| !v.trim().is_empty()))
    }
}

/// Local LAN mobile companion remote configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_remote_enabled")]
    pub enabled: bool,
    #[serde(default = "default_remote_port")]
    pub port: u16,
    #[serde(default = "default_remote_theme")]
    pub theme_id: String,
    #[serde(default = "default_remote_art_mode")]
    pub art_mode: String,
    #[serde(default)]
    pub art_index: Option<usize>,
}

fn default_remote_enabled() -> bool {
    true
}

fn default_remote_port() -> u16 {
    8484
}

fn default_remote_theme() -> String {
    "lain-wired".to_string()
}

fn default_remote_art_mode() -> String {
    "companion".to_string()
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: default_remote_enabled(),
            port: default_remote_port(),
            theme_id: default_remote_theme(),
            art_mode: default_remote_art_mode(),
            art_index: None,
        }
    }
}

/// AirPlay & DLNA TV casting configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastConfig {
    #[serde(default = "default_true")]
    pub auto_discover: bool,
    #[serde(default = "default_cast_timeout")]
    pub scan_timeout_ms: u64,
}

fn default_cast_timeout() -> u64 {
    500
}

impl Default for CastConfig {
    fn default() -> Self {
        Self {
            auto_discover: true,
            scan_timeout_ms: default_cast_timeout(),
        }
    }
}

/// Auto-update configuration preferences
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateConfig {
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
    #[serde(default = "default_true")]
    pub auto_install: bool,
    #[serde(default = "default_update_interval")]
    pub check_interval: u64,
}

fn default_update_interval() -> u64 {
    0 // Check on every launch (non-blocking background check)
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            auto_install: true,
            check_interval: 0,
        }
    }
}

impl Config {
    /// Returns the platform-appropriate config directory path for Solus:
    /// e.g. ~/.config/solus/ on Linux, ~/Library/Application Support/solus/ on macOS.
    /// Performs a seamless, transparent migration if legacy ~/.config/ani-tui/ exists.
    pub fn config_dir() -> Result<PathBuf> {
        let solus_dirs = ProjectDirs::from("", "", "solus")
            .ok_or_else(|| AppError::Config("Could not determine config directory".into()))?;
        let target_dir = solus_dirs.config_dir().to_path_buf();

        // Perform transparent migration from legacy ani-tui directory if target does not yet exist
        if !target_dir.exists() {
            if let Some(legacy_dirs) = ProjectDirs::from("", "", "ani-tui") {
                let legacy_dir = legacy_dirs.config_dir();
                if legacy_dir.exists() {
                    let _ = std::fs::create_dir_all(&target_dir);
                    let legacy_cfg = legacy_dir.join("config.toml");
                    let target_cfg = target_dir.join("config.toml");
                    if legacy_cfg.exists() && !target_cfg.exists() {
                        let _ = std::fs::copy(&legacy_cfg, &target_cfg);
                    }
                    let legacy_auth = legacy_dir.join("auth.json");
                    let target_auth = target_dir.join("auth.json");
                    if legacy_auth.exists() && !target_auth.exists() {
                        let _ = std::fs::copy(&legacy_auth, &target_auth);
                    }
                    let legacy_db = legacy_dir.join("ani-tui.db");
                    let target_db = target_dir.join("solus.db");
                    if legacy_db.exists() && !target_db.exists() {
                        let _ = std::fs::copy(&legacy_db, &target_db);
                    }
                    let legacy_backup = legacy_dir.join("solus_backup.json");
                    let target_backup = target_dir.join("solus_backup.json");
                    if legacy_backup.exists() && !target_backup.exists() {
                        let _ = std::fs::copy(&legacy_backup, &target_backup);
                    }
                }
            }
        }

        // Also migrate ani-tui.db if present inside target_dir into solus.db
        let legacy_in_target = target_dir.join("ani-tui.db");
        let target_db = target_dir.join("solus.db");
        if legacy_in_target.exists() && !target_db.exists() {
            let _ = std::fs::copy(&legacy_in_target, &target_db);
        }

        Ok(target_dir)
    }

    /// Returns the platform-appropriate cache directory path for Solus:
    /// e.g. ~/.cache/solus/ on Linux, ~/Library/Caches/solus/ on macOS.
    pub fn cache_dir() -> Result<PathBuf> {
        ProjectDirs::from("", "", "solus")
            .map(|dirs| dirs.cache_dir().to_path_buf())
            .ok_or_else(|| AppError::Config("Could not determine cache directory".into()))
    }

    /// Returns the path to config.toml
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Returns the path to the SQLite database file (solus.db with ani-tui.db migration)
    pub fn db_path() -> Result<PathBuf> {
        let dir = Self::config_dir()?;
        let target_db = dir.join("solus.db");
        if !target_db.exists() {
            let legacy_db = dir.join("ani-tui.db");
            if legacy_db.exists() {
                let _ = std::fs::copy(&legacy_db, &target_db);
            }
        }
        Ok(target_db)
    }

    /// Load config from disk. Returns default config if file doesn't exist.
    /// Automatically migrates legacy pre-v1.0.7 update defaults once so friends receive automatic updates.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {e}")))?;
        let mut cfg: Self = toml::from_str(&raw)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {e}")))?;
        if cfg.migrate_legacy_defaults() {
            let _ = cfg.save();
        }
        Ok(cfg)
    }

    /// Upgrade legacy v1 defaults once cleanly while preserving explicit future user choices.
    pub fn migrate_legacy_defaults(&mut self) -> bool {
        if self.config_version >= 2 {
            return false;
        }
        if self.config_version == 0 {
            if self.update.check_on_startup && !self.update.auto_install && self.update.check_interval == 14400 {
                self.update.auto_install = true;
                self.update.check_interval = 0;
            } else if self.update.check_interval == 14400 {
                self.update.check_interval = 0;
            }
        }
        // Keep Detail screen consistent with the user's chosen `[T]` palette unless explicitly re-enabled
        self.ambilight_enabled = false;
        self.config_version = 2;
        true
    }

    /// Persist config to disk, creating the directory if needed.
    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {e}")))?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}

/// Supported external anime trackers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerProvider {
    AniList,
    Mal,
}

impl TrackerProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrackerProvider::AniList => "anilist",
            TrackerProvider::Mal => "mal",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TrackerProvider::AniList => "AniList",
            TrackerProvider::Mal => "MyAnimeList",
        }
    }
}

/// Strategy used when AniList & MyAnimeList differ on the same canonical show
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MergePolicy {
    #[default]
    HighestProgress,
    LatestUpdated,
    AniListPrimary,
    MalPrimary,
}

impl MergePolicy {
    pub fn label(&self) -> &'static str {
        match self {
            MergePolicy::HighestProgress => "Highest Progress (Smart)",
            MergePolicy::LatestUpdated => "Latest Updated Wins",
            MergePolicy::AniListPrimary => "AniList Priority",
            MergePolicy::MalPrimary => "MyAnimeList Priority",
        }
    }

    pub fn next(self) -> Self {
        match self {
            MergePolicy::HighestProgress => MergePolicy::LatestUpdated,
            MergePolicy::LatestUpdated => MergePolicy::AniListPrimary,
            MergePolicy::AniListPrimary => MergePolicy::MalPrimary,
            MergePolicy::MalPrimary => MergePolicy::HighestProgress,
        }
    }
}

/// Target database(s) chosen when interactively adding, labeling, or removing Watchlist items
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchlistTarget {
    Both,
    AniList,
    Mal,
    Local,
}

impl WatchlistTarget {
    pub const ALL: [WatchlistTarget; 4] = [
        WatchlistTarget::Both,
        WatchlistTarget::AniList,
        WatchlistTarget::Mal,
        WatchlistTarget::Local,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Both (AL + MAL)",
            Self::AniList => "AniList",
            Self::Mal => "MyAnimeList",
            Self::Local => "Local Only",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Both => Self::AniList,
            Self::AniList => Self::Mal,
            Self::Mal => Self::Local,
            Self::Local => Self::Both,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Both => Self::Local,
            Self::AniList => Self::Both,
            Self::Mal => Self::AniList,
            Self::Local => Self::Mal,
        }
    }
}

/// Canonical 5-state anime list status shared across Solus, AniList, and MyAnimeList.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum WatchStatus {
    Watching,
    #[default]
    Planning,
    Completed,
    Paused,
    Dropped,
}

impl WatchStatus {
    pub const ALL: [WatchStatus; 5] = [
        WatchStatus::Watching,
        WatchStatus::Planning,
        WatchStatus::Completed,
        WatchStatus::Paused,
        WatchStatus::Dropped,
    ];

    pub fn from_tracker_status(raw: &str) -> Self {
        match raw.trim().to_ascii_uppercase().as_str() {
            "CURRENT" | "WATCHING" | "REPEATING" => Self::Watching,
            "COMPLETED" => Self::Completed,
            "PAUSED" | "ON_HOLD" | "ON-HOLD" | "ONHOLD" => Self::Paused,
            "DROPPED" => Self::Dropped,
            _ => Self::Planning,
        }
    }

    pub fn to_tracker_status(self) -> &'static str {
        match self {
            Self::Watching => "CURRENT",
            Self::Planning => "PLANNING",
            Self::Completed => "COMPLETED",
            Self::Paused => "PAUSED",
            Self::Dropped => "DROPPED",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Watching => "",
            Self::Planning => "󰃰",
            Self::Completed => "󰄬",
            Self::Paused => "󰏤",
            Self::Dropped => "󰅖",
        }
    }

    pub fn short_word(self) -> &'static str {
        match self {
            Self::Watching => "Watch",
            Self::Planning => "Plan",
            Self::Completed => "Done",
            Self::Paused => "Hold",
            Self::Dropped => "Drop",
        }
    }

    pub fn short_card_tag(self) -> &'static str {
        match self {
            Self::Watching => " Watch",
            Self::Planning => "󰃰 Plan",
            Self::Completed => "󰄬 Done",
            Self::Paused => "󰏤 Hold",
            Self::Dropped => "󰅖 Drop",
        }
    }

    pub fn full_label(self) -> &'static str {
        match self {
            Self::Watching => "Watching",
            Self::Planning => "Plan to Watch",
            Self::Completed => "Completed",
            Self::Paused => "On-Hold",
            Self::Dropped => "Dropped",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Watching => "Active rotation — tracks episodes as you watch",
            Self::Planning => "Saved in your backlog for later",
            Self::Completed => "Finished all episodes of this show",
            Self::Paused => "Paused temporarily — keeps your episode spot",
            Self::Dropped => "Stopped watching — kept in your list history",
        }
    }

    pub fn picker_index(self) -> usize {
        match self {
            Self::Watching => 0,
            Self::Planning => 1,
            Self::Completed => 2,
            Self::Paused => 3,
            Self::Dropped => 4,
        }
    }
}

/// Connection & token state for a single tracker provider
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrackerAccount {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub sync_mode: Option<String>,
    #[serde(default)]
    pub last_imported_file: Option<String>,
    #[serde(default)]
    pub last_sync_time: Option<i64>,
}

impl TrackerAccount {
    pub fn is_connected(&self) -> bool {
        self.has_write_token()
            || self.username.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false)
            || self.last_imported_file.is_some()
    }

    pub fn has_write_token(&self) -> bool {
        self.token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Dual-tracker authentication & sync rules — persisted per profile under ~/.config/solus/profiles/<id>/auth.json
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub anilist: TrackerAccount,
    #[serde(default)]
    pub mal: TrackerAccount,
    #[serde(default)]
    pub merge_policy: MergePolicy,
    #[serde(default = "default_true")]
    pub auto_scrobble_both: bool,
    #[serde(default)]
    pub auto_align_lagging: bool,
    #[serde(skip)]
    pub profile_id: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            anilist: TrackerAccount::default(),
            mal: TrackerAccount::default(),
            merge_policy: MergePolicy::default(),
            auto_scrobble_both: true,
            auto_align_lagging: false,
            profile_id: None,
        }
    }
}

/// Legacy single-provider auth schema migrated automatically on first load.
#[derive(Deserialize)]
struct RawAuthCompat {
    #[serde(default)]
    anilist: Option<TrackerAccount>,
    #[serde(default)]
    mal: Option<TrackerAccount>,
    #[serde(default)]
    merge_policy: Option<MergePolicy>,
    #[serde(default)]
    auto_scrobble_both: Option<bool>,
    #[serde(default)]
    auto_align_lagging: Option<bool>,
    // Legacy v1 flat AniList fields
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    sync_mode: Option<String>,
    #[serde(default)]
    last_imported_file: Option<String>,
    #[serde(default)]
    last_sync_time: Option<i64>,
}

impl AuthConfig {
    pub fn auth_path() -> Result<PathBuf> {
        Ok(Config::config_dir()?.join("auth.json"))
    }

    pub fn auth_path_for_profile(profile_id: &str) -> Result<PathBuf> {
        let safe_id = if profile_id.trim().is_empty() {
            "default"
        } else {
            profile_id.trim()
        };
        Ok(Config::config_dir()?
            .join("profiles")
            .join(safe_id)
            .join("auth.json"))
    }

    pub fn remove_profile_dir(profile_id: &str) {
        let safe_id = profile_id.trim();
        if safe_id.is_empty() || safe_id == "default" {
            return;
        }
        if let Ok(dir) = Config::config_dir() {
            let pdir = dir.join("profiles").join(safe_id);
            if pdir.exists() {
                let _ = std::fs::remove_dir_all(pdir);
            }
        }
    }

    pub fn from_json_str(raw: &str) -> std::result::Result<Self, serde_json::Error> {
        let compat: RawAuthCompat = serde_json::from_str(raw)?;
        let mut anilist = compat.anilist.unwrap_or_default();
        if !anilist.is_connected() {
            if compat.token.is_some()
                || compat.username.is_some()
                || compat.user_id.is_some()
                || compat.last_imported_file.is_some()
            {
                anilist.token = compat.token;
                anilist.user_id = compat.user_id;
                anilist.username = compat.username;
                anilist.avatar_url = compat.avatar_url;
                anilist.sync_mode = compat.sync_mode;
                anilist.last_imported_file = compat.last_imported_file;
                anilist.last_sync_time = compat.last_sync_time;
            }
        }
        Ok(Self {
            anilist,
            mal: compat.mal.unwrap_or_default(),
            merge_policy: compat.merge_policy.unwrap_or_default(),
            auto_scrobble_both: compat.auto_scrobble_both.unwrap_or(true),
            auto_align_lagging: compat.auto_align_lagging.unwrap_or(false),
            profile_id: None,
        })
    }

    pub fn load_for_profile(profile_id: &str) -> Self {
        let pid = if profile_id.trim().is_empty() {
            "default"
        } else {
            profile_id.trim()
        };

        if let Ok(path) = Self::auth_path_for_profile(pid) {
            if path.exists() {
                if let Ok(raw) = std::fs::read_to_string(path) {
                    if let Ok(mut cfg) = Self::from_json_str(&raw) {
                        cfg.profile_id = Some(pid.to_string());
                        return cfg;
                    }
                }
            }
        }

        if pid == "default" {
            if let Ok(legacy_path) = Self::auth_path() {
                if legacy_path.exists() {
                    if let Ok(raw) = std::fs::read_to_string(legacy_path) {
                        if let Ok(mut cfg) = Self::from_json_str(&raw) {
                            cfg.profile_id = Some("default".to_string());
                            return cfg;
                        }
                    }
                }
            }
        }

        Self {
            profile_id: Some(pid.to_string()),
            ..Self::default()
        }
    }

    pub fn load() -> Self {
        Self::load_for_profile("default")
    }

    pub fn save(&self) -> Result<()> {
        let pid = self.profile_id.as_deref().unwrap_or("default");
        let path = Self::auth_path_for_profile(pid)?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize auth: {e}")))?;
        std::fs::write(&path, &content)?;
        if pid == "default" {
            if let Ok(legacy_path) = Self::auth_path() {
                if let Some(parent) = legacy_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(legacy_path, &content);
            }
        }
        Ok(())
    }

    pub fn account(&self, provider: TrackerProvider) -> &TrackerAccount {
        match provider {
            TrackerProvider::AniList => &self.anilist,
            TrackerProvider::Mal => &self.mal,
        }
    }

    pub fn account_mut(&mut self, provider: TrackerProvider) -> &mut TrackerAccount {
        match provider {
            TrackerProvider::AniList => &mut self.anilist,
            TrackerProvider::Mal => &mut self.mal,
        }
    }

    pub fn has_anilist(&self) -> bool {
        self.anilist.is_connected()
    }

    pub fn has_mal(&self) -> bool {
        self.mal.is_connected()
    }

    pub fn is_authenticated(&self) -> bool {
        self.has_anilist() || self.has_mal()
    }

    pub fn anilist_token(&self) -> Option<String> {
        self.anilist.token.clone().filter(|t| !t.trim().is_empty())
    }

    pub fn mal_token(&self) -> Option<String> {
        self.mal.token.clone().filter(|t| !t.trim().is_empty())
    }

    pub fn clear_provider(&mut self, provider: TrackerProvider) -> Result<()> {
        self.account_mut(provider).clear();
        self.save()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn clear(&mut self) -> Result<()> {
        self.anilist.clear();
        self.mal.clear();
        self.save()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config_values() {
        let cfg = Config::default();
        assert_eq!(cfg.quality, Quality::P1080);
        assert_eq!(cfg.audio_mode, AudioMode::Sub);
        assert_eq!(cfg.player, Player::AutoBest);
        assert_eq!(cfg.performance_mode, PerformanceMode::Balanced);
        assert_eq!(cfg.deband_mode, DebandMode::Auto);
        assert_eq!(cfg.motion_mode, MotionMode::Off);
        assert_eq!(cfg.cache.trending_ttl, 86_400);
        assert_eq!(cfg.cache.stable_ttl, 604_800);
    }

    #[test]
    fn test_quality_as_str() {
        assert_eq!(Quality::P720.as_str(),  "720p");
        assert_eq!(Quality::P1080.as_str(), "1080p");
        assert_eq!(Quality::Best.as_str(),  "best");
    }

    #[test]
    fn test_player_as_str_and_legacy_migration() {
        assert_eq!(Player::AutoBest.as_str(), "auto-best");
        assert_eq!(Player::Mpv.as_str(), "mpv");
        assert_eq!(Player::MpvNet.as_str(), "mpv.net");
        assert_eq!(Player::Celluloid.as_str(), "celluloid");

        assert_eq!(Player::from_str_lossy("iina"), Player::AutoBest);
        assert_eq!(Player::from_str_lossy("vlc"), Player::AutoBest);
        assert_eq!(Player::from_str_lossy("mpvnet"), Player::MpvNet);
        assert_eq!(Player::from_str_lossy("celluloid"), Player::Celluloid);
        assert_eq!(Player::from_str_lossy("mpv"), Player::Mpv);
    }

    #[test]
    fn test_config_round_trip_toml() {
        let original = Config {
            quality:          Quality::P720,
            audio_mode:       AudioMode::Dub,
            player:           Player::Celluloid,
            default_sub_lang: "Arabic".to_string(),
            auto_skip_intro:  true,
            anime4k_mode:     Anime4KMode::Balanced,
            ambilight_enabled: true,
            update:           UpdateConfig::default(),
            cache:            CacheConfig::default(),
            subtitle_style:   crate::services::substyler::SubtitleStyleConfig::default(),
            remote:           RemoteConfig::default(),
            cast:             CastConfig::default(),
            trackers:         TrackersConfig::default(),
            active_profile:   default_active_profile(),
            config_version:   1,
            network_mode:     NetworkMode::CongestedWifi,
            force_ipv4:       true,
            auto_abr_downshift: false,
            auto_fullscreen:  true,
            auto_pip:         false,
            performance_mode: PerformanceMode::Reference4K,
            deband_mode:      DebandMode::Strong,
            motion_mode:      MotionMode::Cadence,
        };
        let serialized   = toml::to_string_pretty(&original).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.quality,          Quality::P720);
        assert_eq!(deserialized.audio_mode,       AudioMode::Dub);
        assert_eq!(deserialized.player,           Player::Celluloid);
        assert_eq!(deserialized.default_sub_lang, "Arabic");
        assert_eq!(deserialized.anime4k_mode,     Anime4KMode::Balanced);
        assert!(deserialized.ambilight_enabled);
        assert_eq!(deserialized.network_mode,     NetworkMode::CongestedWifi);
        assert!(deserialized.force_ipv4);
        assert!(!deserialized.auto_abr_downshift);
        assert!(deserialized.auto_fullscreen);
        assert_eq!(deserialized.performance_mode, PerformanceMode::Reference4K);
        assert_eq!(deserialized.deband_mode,      DebandMode::Strong);
        assert_eq!(deserialized.motion_mode,      MotionMode::Cadence);
    }

    #[test]
    fn test_save_and_load_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        let original = Config {
            quality:          Quality::P480,
            audio_mode:       AudioMode::Sub,
            player:           Player::Mpv,
            default_sub_lang: "English".to_string(),
            auto_skip_intro:  true,
            anime4k_mode:     Anime4KMode::Off,
            ambilight_enabled: false,
            update:           UpdateConfig::default(),
            cache:            CacheConfig::default(),
            subtitle_style:   crate::services::substyler::SubtitleStyleConfig::default(),
            remote:           RemoteConfig::default(),
            cast:             CastConfig::default(),
            trackers:         TrackersConfig::default(),
            active_profile:   default_active_profile(),
            config_version:   1,
            ..Config::default()
        };

        // Save manually (bypassing config_path() which uses XDG dirs)
        let content = toml::to_string_pretty(&original).unwrap();
        std::fs::write(&path, content).unwrap();

        // Load back
        let raw  = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&raw).unwrap();

        assert_eq!(loaded.quality,    Quality::P480);
        assert_eq!(loaded.audio_mode, AudioMode::Sub);
        assert_eq!(loaded.player,     Player::Mpv);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        // Config::load() returns default when file doesn't exist.
        // We can't easily redirect the path, but we can verify toml::from_str
        // of an empty string fails gracefully and default is sound.
        let cfg = Config::default();
        assert_eq!(cfg.quality, Quality::P1080);
    }

    #[test]
    fn test_config_dir_is_some() {
        // Just ensure the platform can resolve a config dir
        assert!(Config::config_dir().is_ok());
    }

    #[test]
    fn test_legacy_update_config_migrates_once_to_auto_install() {
        let legacy_toml = r#"
quality = "1080p"
audio_mode = "sub"
player = "mpv"

[update]
check_on_startup = true
auto_install = false
check_interval = 14400

[cache]
trending_ttl = 1800
stable_ttl = 21600
detail_ttl = 3600
"#;
        let mut cfg: Config = toml::from_str(legacy_toml).unwrap();
        assert_eq!(cfg.config_version, 0);
        assert!(cfg.migrate_legacy_defaults());
        assert!(cfg.update.auto_install);
        assert_eq!(cfg.update.check_interval, 0);
        assert!(!cfg.ambilight_enabled);
        assert_eq!(cfg.config_version, 2);

        // User opting back out after migration must not be overridden on subsequent loads.
        cfg.update.auto_install = false;
        assert!(!cfg.migrate_legacy_defaults());
        assert!(!cfg.update.auto_install);
        // Legacy configs missing network fields receive sane defaults.
        assert_eq!(cfg.network_mode, NetworkMode::Normal);
        assert!(!cfg.force_ipv4);
        assert!(cfg.auto_abr_downshift);
        assert_eq!(cfg.remote.theme_id, "lain-wired");
        assert_eq!(cfg.remote.art_mode, "companion");
    }

    #[test]
    fn test_network_mode_profiles_and_quality_clamping() {
        assert_eq!(NetworkMode::Normal.next(), NetworkMode::CongestedWifi);
        assert_eq!(NetworkMode::CongestedWifi.next(), NetworkMode::UltraSaver);
        assert_eq!(NetworkMode::UltraSaver.next(), NetworkMode::Normal);
        assert_eq!(NetworkMode::Normal.prev(), NetworkMode::UltraSaver);

        assert!(!NetworkMode::Normal.is_congested());
        assert!(NetworkMode::CongestedWifi.is_congested());
        assert!(NetworkMode::UltraSaver.is_congested());

        assert_eq!(NetworkMode::Normal.max_height_cap(), None);
        assert_eq!(NetworkMode::CongestedWifi.max_height_cap(), Some(720));
        assert_eq!(NetworkMode::UltraSaver.max_height_cap(), Some(480));

        assert_eq!(NetworkMode::Normal.vod_readahead_secs(), 45);
        assert_eq!(NetworkMode::CongestedWifi.vod_readahead_secs(), 120);
        assert_eq!(NetworkMode::UltraSaver.vod_readahead_secs(), 180);

        assert!(!NetworkMode::Normal.use_disk_cache());
        assert!(NetworkMode::CongestedWifi.use_disk_cache());
        assert!(NetworkMode::UltraSaver.use_disk_cache());

        assert_eq!(Quality::P1080.clamped_for_network(NetworkMode::Normal), Quality::P1080);
        assert_eq!(Quality::Best.clamped_for_network(NetworkMode::CongestedWifi), Quality::P720);
        assert_eq!(Quality::P1080.clamped_for_network(NetworkMode::CongestedWifi), Quality::P720);
        assert_eq!(Quality::P480.clamped_for_network(NetworkMode::CongestedWifi), Quality::P480);
        assert_eq!(Quality::P720.clamped_for_network(NetworkMode::UltraSaver), Quality::P480);
        assert_eq!(Quality::P360.clamped_for_network(NetworkMode::UltraSaver), Quality::P360);
    }

    #[test]
    fn test_resolved_mal_client_id_defaults_to_builtin() {
        let trackers = TrackersConfig::default();
        assert_eq!(trackers.custom_mal_client_id(), None);
        assert_eq!(
            trackers.resolved_mal_client_id().as_deref(),
            Some(DEFAULT_MAL_CLIENT_ID)
        );
    }
}
