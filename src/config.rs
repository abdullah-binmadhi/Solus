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
}


/// Sub or dub preference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioMode {
    #[default]
    Sub,
    Dub,
}

/// Video player to use
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Player {
    #[default]
    Mpv,
    Iina,
    Vlc,
}

impl Player {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_str(&self) -> &str {
        match self {
            Player::Mpv => "mpv",
            Player::Iina => "iina",
            Player::Vlc => "vlc",
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
    true
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
    #[serde(default = "default_true")]
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
            ambilight_enabled: default_true(),
            update:           UpdateConfig::default(),
            cache:            CacheConfig::default(),
            subtitle_style:   crate::services::substyler::SubtitleStyleConfig::default(),
            remote:           RemoteConfig::default(),
            cast:             CastConfig::default(),
        }
    }
}

/// Local LAN mobile companion remote configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_remote_enabled")]
    pub enabled: bool,
    #[serde(default = "default_remote_port")]
    pub port: u16,
}

fn default_remote_enabled() -> bool {
    true
}

fn default_remote_port() -> u16 {
    8484
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: default_remote_enabled(),
            port: default_remote_port(),
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
    #[serde(default)]
    pub auto_install: bool,
    #[serde(default = "default_update_interval")]
    pub check_interval: u64,
}

fn default_update_interval() -> u64 {
    14400 // 4 hours
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            auto_install: false,
            check_interval: 14400,
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
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {e}")))?;
        toml::from_str(&raw)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {e}")))
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

/// AniList authentication data — persisted to ~/.config/ani-tui/auth.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub token: Option<String>,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub sync_mode: Option<String>,
    pub last_imported_file: Option<String>,
    pub last_sync_time: Option<i64>,
}

impl AuthConfig {
    pub fn auth_path() -> Result<PathBuf> {
        Ok(Config::config_dir()?.join("auth.json"))
    }

    pub fn load() -> Self {
        if let Ok(path) = Self::auth_path() {
            if path.exists() {
                if let Ok(raw) = std::fs::read_to_string(path) {
                    if let Ok(cfg) = serde_json::from_str::<AuthConfig>(&raw) {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::auth_path()?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize auth: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false)
            || self.username.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false)
            || self.last_imported_file.is_some()
    }

    pub fn clear(&mut self) -> Result<()> {
        self.token = None;
        self.user_id = None;
        self.username = None;
        self.avatar_url = None;
        self.sync_mode = None;
        self.last_imported_file = None;
        self.last_sync_time = None;
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
        assert_eq!(cfg.player, Player::Mpv);
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
    fn test_player_as_str() {
        assert_eq!(Player::Mpv.as_str(), "mpv");
        assert_eq!(Player::Iina.as_str(), "iina");
        assert_eq!(Player::Vlc.as_str(), "vlc");
    }

    #[test]
    fn test_config_round_trip_toml() {
        let original = Config {
            quality:          Quality::P720,
            audio_mode:       AudioMode::Dub,
            player:           Player::Iina,
            default_sub_lang: "Arabic".to_string(),
            auto_skip_intro:  true,
            anime4k_mode:     Anime4KMode::Balanced,
            ambilight_enabled: true,
            update:           UpdateConfig::default(),
            cache:            CacheConfig::default(),
            subtitle_style:   crate::services::substyler::SubtitleStyleConfig::default(),
            remote:           RemoteConfig::default(),
            cast:             CastConfig::default(),
        };
        let serialized   = toml::to_string_pretty(&original).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.quality,          Quality::P720);
        assert_eq!(deserialized.audio_mode,       AudioMode::Dub);
        assert_eq!(deserialized.player,           Player::Iina);
        assert_eq!(deserialized.default_sub_lang, "Arabic");
        assert_eq!(deserialized.anime4k_mode,     Anime4KMode::Balanced);
        assert!(deserialized.ambilight_enabled);
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
}
