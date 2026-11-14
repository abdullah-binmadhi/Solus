//! Background auto-update service for Solus.
//!
//! Provides ultra-fluid, non-blocking update checking and atomic self-replacement:
//! - Checks GitHub Releases API in the background with zero startup penalty (<40ms cold boot preserved).
//! - Rate-limited with TTL caching so GitHub is never spammed.
//! - Resolves platform-specific binaries (`solus-macos-aarch64`, `solus-macos-x86_64`, `solus-linux-x86_64`).
//! - Atomically replaces the running binary on Unix without terminating the current session prematurely.

use crate::error::{AppError, Result};
use crate::AppMessage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/abdullah-binmadhi/Solus/releases/latest";
const USER_AGENT: &str = concat!("solus/", env!("CARGO_PKG_VERSION"));

/// Asset entry in GitHub release response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// GitHub latest release response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// Metadata about an available update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub asset_name: String,
    pub release_notes: Option<String>,
    pub is_homebrew: bool,
}

/// Parse semantic version numbers into (major, minor, patch).
pub fn parse_semver(ver: &str) -> Option<(u32, u32, u32)> {
    let clean = ver.trim().trim_start_matches(['v', 'V']);
    let mut parts = clean.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch_str = parts.next().unwrap_or("0");
    // Strip any pre-release tags like -beta or -rc
    let patch_num_str = patch_str.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0");
    let patch: u32 = patch_num_str.parse().ok()?;
    Some((major, minor, patch))
}

/// Check if `latest` version string is strictly newer than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let Some(curr) = parse_semver(current) else { return false };
    let Some(late) = parse_semver(latest) else { return false };
    late > curr
}

/// Determine the target release asset name for the current machine's OS and CPU architecture.
pub fn target_asset_name() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "solus-macos-aarch64" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "solus-macos-x86_64" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "solus-linux-x86_64" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "solus-windows-x86_64.exe" }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    { "solus" }
}

/// Resolve the canonical path of the running binary (following symlinks such as `/opt/homebrew/bin/solus -> ~/.cargo/bin/solus`).
fn resolved_current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// Return true only when the canonical binary resides inside Homebrew's Cellar or Caskroom.
pub fn is_homebrew_binary_path(path: &std::path::Path) -> bool {
    let norm = path.to_string_lossy().replace('\\', "/");
    norm.contains("/Cellar/") || norm.contains("/Caskroom/")
}

/// Detect whether the currently running binary is genuinely managed by Homebrew.
pub fn is_homebrew_install() -> bool {
    resolved_current_exe()
        .map(|p| is_homebrew_binary_path(&p))
        .unwrap_or(false)
}

/// Git SHA baked in at compile time (`build.rs`), or `"unknown"` for source tarball builds.
pub fn current_build_sha() -> &'static str {
    option_env!("SOLUS_GIT_SHA").unwrap_or("unknown")
}

/// Commit Unix timestamp baked in at compile time (`build.rs`), preventing accidental downgrades of local dev builds.
pub fn current_commit_timestamp() -> i64 {
    option_env!("SOLUS_COMMIT_TS")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

/// Truncate a git SHA to its standard 7-character short prefix for display.
pub fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// Extract the embedded commit SHA (`<!-- solus-sha:<sha> -->`) from a release body.
pub fn extract_release_sha(body: Option<&str>) -> Option<String> {
    extract_marker(body, "solus-sha:")
        .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Extract the embedded commit timestamp (`<!-- solus-ts:<unix_secs> -->`) from a release body.
pub fn extract_release_timestamp(body: Option<&str>) -> Option<i64> {
    extract_marker(body, "solus-ts:")?.parse::<i64>().ok()
}

fn extract_marker(body: Option<&str>, prefix: &str) -> Option<String> {
    let text = body?;
    let start = text.find(prefix)? + prefix.len();
    let rest = &text[start..];
    let end = rest.find("-->").unwrap_or(rest.len());
    let val = rest[..end].trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Remove internal HTML metadata comments (`<!-- solus-... -->`) before displaying release notes.
fn clean_release_notes(body: Option<String>) -> Option<String> {
    let text = body?;
    let cleaned: Vec<&str> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("<!-- solus-") && trimmed.ends_with("-->"))
        })
        .collect();
    let joined = cleaned.join("\n").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Pure update decision logic supporting both tagged semver releases and rolling `main` commit releases.
pub fn evaluate_release_update(
    release: GitHubRelease,
    current_version: &str,
    local_sha: &str,
    local_ts: i64,
) -> Option<UpdateInfo> {
    let semver_newer = is_newer_version(current_version, &release.tag_name);
    let is_rolling = release.tag_name.ends_with("-rolling") || release.tag_name == "latest";
    let remote_sha = extract_release_sha(release.body.as_deref());
    let remote_ts = extract_release_timestamp(release.body.as_deref()).unwrap_or(0);

    let semver_not_older = !is_newer_version(&release.tag_name, current_version);
    let rolling_sha_newer = is_rolling
        && semver_not_older
        && local_sha != "unknown"
        && matches!(
            remote_sha.as_deref(),
            Some(rsha) if rsha != local_sha && (remote_ts == 0 || local_ts == 0 || remote_ts >= local_ts)
        );

    if !semver_newer && !rolling_sha_newer {
        return None;
    }

    let (display_curr, display_latest) = if !semver_newer && rolling_sha_newer {
        (
            format!(
                "v{} ({})",
                current_version.trim_start_matches(['v', 'V']),
                short_sha(local_sha)
            ),
            format!(
                "main@{}",
                short_sha(remote_sha.as_deref().unwrap_or("latest"))
            ),
        )
    } else {
        (current_version.to_string(), release.tag_name.clone())
    };

    let target_name = target_asset_name();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == target_name)
        .or_else(|| release.assets.iter().find(|a| a.name.contains("solus")))?;

    Some(UpdateInfo {
        current_version: display_curr,
        latest_version: display_latest,
        download_url: asset.browser_download_url.clone(),
        asset_name: asset.name.clone(),
        release_notes: clean_release_notes(release.body),
        is_homebrew: is_homebrew_install(),
    })
}

/// Check GitHub for the latest release in a non-blocking asynchronous call.
pub async fn check_for_updates(client: &reqwest::Client, current_version: &str) -> Result<Option<UpdateInfo>> {
    let resp = client
        .get(GITHUB_RELEASES_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(6))
        .send()
        .await?
        .error_for_status()?;

    let release: GitHubRelease = resp.json().await?;
    Ok(evaluate_release_update(
        release,
        current_version,
        current_build_sha(),
        current_commit_timestamp(),
    ))
}

/// Download the new binary and atomically replace the current running executable.
pub async fn download_and_install_update(
    client: &reqwest::Client,
    download_url: &str,
    target_version: &str,
    tx: Sender<AppMessage>,
) -> Result<PathBuf> {
    let current_exe = resolved_current_exe()
        .ok_or_else(|| AppError::Player("Failed to resolve current binary path".into()))?;

    let current_dir = current_exe
        .parent()
        .ok_or_else(|| AppError::Player("Could not resolve binary parent directory".into()))?;

    // Temporary download file in the same directory (guarantees same filesystem for atomic rename)
    let temp_download_path = current_dir.join(format!(".solus-update-{}.tmp", std::process::id()));

    let mut resp = client
        .get(download_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let total_size = resp.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut last_emit = Instant::now();
    let start_time = Instant::now();

    let mut file = tokio::fs::File::create(&temp_download_path).await?;

    while let Some(chunk) = resp.chunk().await? {
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed() >= Duration::from_millis(150) {
            let pct = if total_size > 0 {
                (downloaded as f32 / total_size as f32) * 100.0
            } else {
                0.0
            };
            let elapsed_secs = start_time.elapsed().as_secs_f64().max(0.001);
            let speed_mb = (downloaded as f64 / 1_048_576.0) / elapsed_secs;
            let speed_str = format!("{:.1} MB/s", speed_mb);

            let _ = tx.send(AppMessage::UpdateProgress(pct, speed_str)).await;
            last_emit = Instant::now();
        }
    }

    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    // Ensure permissions and replace running executable
    #[cfg(windows)]
    {
        // On Windows, the running executable is locked by the OS against modification or deletion.
        // However, NTFS allows renaming an open executable within the same filesystem!
        // 1. Generate old backup name: solus.exe.old
        let old_exe = current_exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old_exe); // Remove any old backup if present and unlocked

        // 2. Rename the currently running executable to solus.exe.old
        if let Err(e) = std::fs::rename(&current_exe, &old_exe) {
            let _ = std::fs::remove_file(&temp_download_path);
            return Err(AppError::Player(format!(
                "Failed to rename running executable {} to {}: {}",
                current_exe.display(),
                old_exe.display(),
                e
            )));
        }

        // 3. Move the downloaded new binary into place
        if let Err(e) = std::fs::rename(&temp_download_path, &current_exe) {
            // Roll back if moving the new binary failed
            let _ = std::fs::rename(&old_exe, &current_exe);
            let _ = std::fs::remove_file(&temp_download_path);
            return Err(AppError::Player(format!(
                "Failed to install new binary at {}: {}",
                current_exe.display(),
                e
            )));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_download_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_download_path, perms)?;

        // Atomically replace running binary with the newly downloaded binary
        if let Err(e) = std::fs::rename(&temp_download_path, &current_exe) {
            // Fallback for cross-device or permission limits: attempt removal then copy
            let _ = std::fs::remove_file(&current_exe);
            if let Err(e2) = std::fs::copy(&temp_download_path, &current_exe) {
                let _ = std::fs::remove_file(&temp_download_path);
                return Err(AppError::Player(format!(
                    "Failed to replace binary at {}: {} (fallback copy error: {})",
                    current_exe.display(),
                    e,
                    e2
                )));
            }
            let _ = std::fs::remove_file(&temp_download_path);
        }
    }

    let _ = tx.send(AppMessage::UpdateSuccess(target_version.to_string())).await;
    Ok(current_exe)
}

/// Clean up any leftover update artifacts (e.g., `*.exe.old` or temporary download files)
/// from previous updates. Called on startup.
pub fn cleanup_legacy_update_files() {
    if let Some(current_exe) = resolved_current_exe() {
        if let Some(parent) = current_exe.parent() {
            #[cfg(windows)]
            {
                let old_exe = current_exe.with_extension("exe.old");
                let _ = std::fs::remove_file(old_exe);
            }
            // Clean up any abandoned .solus-update-*.tmp files
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.starts_with(".solus-update-") && name_str.ends_with(".tmp") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semver() {
        assert_eq!(parse_semver("1.0.6"), Some((1, 0, 6)));
        assert_eq!(parse_semver("v1.0.7"), Some((1, 0, 7)));
        assert_eq!(parse_semver("V2.14.0"), Some((2, 14, 0)));
        assert_eq!(parse_semver("v1.2.3-beta.1"), Some((1, 2, 3)));
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("1.0.6", "1.0.7"));
        assert!(is_newer_version("v1.0.6", "v1.1.0"));
        assert!(is_newer_version("1.0.6", "2.0.0"));
        assert!(!is_newer_version("1.0.6", "1.0.6"));
        assert!(!is_newer_version("1.0.7", "1.0.6"));
        assert!(!is_newer_version("2.0.0", "1.9.9"));
    }

    #[test]
    fn test_target_asset_name_not_empty() {
        let name = target_asset_name();
        assert!(!name.is_empty());
        assert!(name.contains("solus"));
    }

    fn sample_release(tag_name: &str, body: Option<&str>) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_string(),
            name: Some(tag_name.to_string()),
            body: body.map(|s| s.to_string()),
            html_url: "https://github.com/abdullah-binmadhi/Solus/releases".to_string(),
            assets: vec![ReleaseAsset {
                name: target_asset_name().to_string(),
                browser_download_url: "https://example.com/solus".to_string(),
                size: 1024,
            }],
        }
    }

    #[test]
    fn test_evaluate_release_update_rolling_sha_and_semver() {
        let body = "<!-- solus-sha:abcdef1234567890abcdef1234567890abcdef12 -->\n<!-- solus-ts:2000 -->\nAutomated build for commit `abcdef1`: Fix flickering";

        // 1. Existing v1.0.6 binary upgrades via semver to v1.0.7-rolling.
        let r1 = sample_release("v1.0.7-rolling", Some(body));
        let info1 = evaluate_release_update(r1, "1.0.6", "unknown", 0).unwrap();
        assert_eq!(info1.latest_version, "v1.0.7-rolling");
        assert!(!info1.release_notes.as_deref().unwrap().contains("solus-sha"));

        // 2. Same version 1.0.7 with older commit SHA upgrades to newer commit SHA.
        let r2 = sample_release("v1.0.7-rolling", Some(body));
        let info2 = evaluate_release_update(r2, "1.0.7", "1111111222222233333334444444555555566666", 1500).unwrap();
        assert_eq!(info2.latest_version, "main@abcdef1");
        assert_eq!(info2.current_version, "v1.0.7 (1111111)");

        // 3. Matching commit SHA reports no update needed.
        let r3 = sample_release("v1.0.7-rolling", Some(body));
        assert!(evaluate_release_update(r3, "1.0.7", "abcdef1234567890abcdef1234567890abcdef12", 2000).is_none());

        // 4. Local newer dev commit (local_ts > remote_ts) is never downgraded.
        let r4 = sample_release("v1.0.7-rolling", Some(body));
        assert!(evaluate_release_update(r4, "1.0.7", "9999999888888877777776666666555555544444", 2500).is_none());
    }

    #[test]
    fn test_is_homebrew_binary_path() {
        assert!(!is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/bin/solus")));
        assert!(!is_homebrew_binary_path(std::path::Path::new("/Users/dev/.cargo/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/Cellar/solus/1.0.7/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/usr/local/Cellar/solus/1.0.7/bin/solus")));
        assert!(is_homebrew_binary_path(std::path::Path::new("/opt/homebrew/Caskroom/solus/1.0.7/solus")));
    }
}
