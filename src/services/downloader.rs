//! Background offline episode batch downloader.
//! Drives yt-dlp or ffmpeg to download and mux episodes into ~/Movies/Solus/{Title}/.

use crate::db;
use crate::AppMessage;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::Sender;

/// Task describing an episode to download offline.
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub anime_id:    i64,
    pub anime_title: String,
    pub episode:     u32,
    pub stream_url:  String,
    pub sub_url:     Option<String>,
}

/// Parse yt-dlp progress output into percentage (0.0..100.0) and download speed string.
pub fn parse_yt_dlp_progress(line: &str) -> Option<(f32, String)> {
    if !line.contains("[download]") {
        return None;
    }
    let perc_idx = line.find('%')?;
    let before_perc = line[..perc_idx].trim_end();
    let num_start = before_perc
        .rfind(|c: char| !c.is_numeric() && c != '.')
        .map(|i| i + 1)
        .unwrap_or(0);
    let percent: f32 = before_perc[num_start..].parse().ok()?;

    let speed = if let Some(at_idx) = line.find(" at ") {
        let after_at = line[at_idx + 4..].trim_start();
        if let Some(space_idx) = after_at.find(" ETA") {
            after_at[..space_idx].trim().to_string()
        } else if let Some(slash_idx) = after_at.find("/s") {
            after_at[..slash_idx + 2].trim().to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Some((percent, speed))
}

/// Sanitize anime title for use in filesystem paths.
pub fn sanitize_filename(title: &str) -> String {
    let mut clean = String::new();
    for c in title.chars() {
        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') {
            clean.push('_');
        } else {
            clean.push(c);
        }
    }
    let trimmed = clean.trim().to_string();
    if trimmed.is_empty() {
        "Anime".to_string()
    } else {
        trimmed
    }
}

/// Get the destination vault directory for downloaded anime: ~/Movies/Solus/{Title}/
pub fn get_vault_dir(title: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let safe_title = sanitize_filename(title);
    home.join("Movies").join("Solus").join(safe_title)
}

/// Parse user range string (e.g. "1", "1-12", "all") into list of episode numbers.
pub fn parse_episode_range(input: &str, max_episodes: u32, unwatched: &[u32]) -> Vec<u32> {
    let trimmed = input.trim().to_lowercase();
    if trimmed == "all" {
        return (1..=max_episodes).collect();
    }
    if trimmed == "unwatched" {
        let mut list = unwatched.to_vec();
        list.sort_unstable();
        return list;
    }

    if let Some((start_s, end_s)) = trimmed.split_once('-') {
        if let (Ok(s), Ok(e)) = (start_s.trim().parse::<u32>(), end_s.trim().parse::<u32>()) {
            if s <= e && s > 0 {
                return (s..=e.min(max_episodes)).collect();
            }
        }
    }

    if let Ok(single) = trimmed.parse::<u32>() {
        if single > 0 && single <= max_episodes {
            return vec![single];
        }
    }

    Vec::new()
}

/// Spawn the background downloader queue for a batch of tasks.
pub fn spawn_batch_downloader(
    tasks: Vec<DownloadTask>,
    pool:  SqlitePool,
    tx:    Sender<AppMessage>,
) {
    tokio::spawn(async move {
        for task in tasks {
            download_single_episode(task, pool.clone(), tx.clone()).await;
        }
    });
}

async fn download_single_episode(
    task: DownloadTask,
    pool: SqlitePool,
    tx:   Sender<AppMessage>,
) {
    let vault = get_vault_dir(&task.anime_title);
    if let Err(e) = tokio::fs::create_dir_all(&vault).await {
        let _ = tx
            .send(AppMessage::DownloadFailed(
                task.anime_id,
                task.episode,
                format!("Failed to create folder: {e}"),
            ))
            .await;
        return;
    }

    let output_mkv = vault.join(format!("Episode_{:02}.mkv", task.episode));
    let output_mkv_str = output_mkv.to_string_lossy().into_owned();

    // 1. Download subtitle if provided
    if let Some(ref sub_url) = task.sub_url {
        if !sub_url.is_empty() {
            let sub_ext = if sub_url.ends_with(".srt") { "srt" } else { "vtt" };
            let sub_file = vault.join(format!("Episode_{:02}.{}", task.episode, sub_ext));
            if let Ok(resp) = reqwest::get(sub_url).await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        let _ = tokio::fs::write(&sub_file, bytes).await;
                    }
                }
            }
        }
    }

    // 2. Drive yt-dlp if available
    let has_ytdlp = crate::api::player::resolve_player("yt-dlp") == "yt-dlp"
        || Path::new("/opt/homebrew/bin/yt-dlp").exists()
        || Path::new("/usr/local/bin/yt-dlp").exists();

    let yt_cmd = if Path::new("/opt/homebrew/bin/yt-dlp").exists() {
        "/opt/homebrew/bin/yt-dlp"
    } else {
        "yt-dlp"
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let success = if has_ytdlp {
        let mut child = match Command::new(yt_cmd)
            .arg("--hls-prefer-native")
            .arg("--no-part")
            .arg("--newline")
            .arg("--progress")
            .arg("--referer")
            .arg("https://zokoanime.video/")
            .arg("-o")
            .arg(&output_mkv_str)
            .arg(&task.stream_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(AppMessage::DownloadFailed(
                        task.anime_id,
                        task.episode,
                        format!("Failed to start yt-dlp: {e}"),
                    ))
                    .await;
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            let tx2 = tx.clone();
            let aid = task.anime_id;
            let ep = task.episode;
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some((pct, spd)) = parse_yt_dlp_progress(&line) {
                        let _ = tx2.send(AppMessage::DownloadProgress(aid, ep, pct, spd)).await;
                    }
                }
            });
        }

        match child.wait().await {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    } else {
        // Fallback to ffmpeg
        let ffmpeg_cmd = if Path::new("/opt/homebrew/bin/ffmpeg").exists() {
            "/opt/homebrew/bin/ffmpeg"
        } else {
            "ffmpeg"
        };

        let mut child = match Command::new(ffmpeg_cmd)
            .arg("-y")
            .arg("-allowed_extensions")
            .arg("ALL")
            .arg("-headers")
            .arg("Referer: https://zokoanime.video/\r\n")
            .arg("-i")
            .arg(&task.stream_url)
            .arg("-c")
            .arg("copy")
            .arg(&output_mkv_str)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(AppMessage::DownloadFailed(
                        task.anime_id,
                        task.episode,
                        format!("Failed to start ffmpeg: {e}"),
                    ))
                    .await;
                return;
            }
        };

        let _ = tx
            .send(AppMessage::DownloadProgress(
                task.anime_id,
                task.episode,
                50.0,
                "muxing...".to_string(),
            ))
            .await;

        match child.wait().await {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    };

    if success && Path::new(&output_mkv_str).exists() {
        let size = std::fs::metadata(&output_mkv_str)
            .map(|m| m.len() as i64)
            .unwrap_or(0);

        let _ = db::user::save_offline_episode(
            &pool,
            task.anime_id,
            task.episode,
            &output_mkv_str,
            size,
            now_unix,
        )
        .await;

        let _ = tx
            .send(AppMessage::DownloadComplete(
                task.anime_id,
                task.episode,
                output_mkv_str,
            ))
            .await;
    } else {
        let _ = tx
            .send(AppMessage::DownloadFailed(
                task.anime_id,
                task.episode,
                "Download process returned non-zero exit code".to_string(),
            ))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yt_dlp_progress() {
        let line1 = "[download]  45.2% of ~120.00MiB at  3.50MiB/s ETA 00:15";
        let res1 = parse_yt_dlp_progress(line1);
        assert!(res1.is_some());
        let (p1, s1) = res1.unwrap();
        assert!((p1 - 45.2).abs() < 1e-3);
        assert_eq!(s1, "3.50MiB/s");

        let line2 = "[download] 100% of 150.00MiB in 00:42";
        let res2 = parse_yt_dlp_progress(line2);
        assert!(res2.is_some());
        let (p2, _) = res2.unwrap();
        assert!((p2 - 100.0).abs() < 1e-3);

        let line3 = "Random log output";
        assert_eq!(parse_yt_dlp_progress(line3), None);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Steins;Gate: 0"), "Steins;Gate_ 0");
        assert_eq!(sanitize_filename("Fate/stay night [UBW]"), "Fate_stay night [UBW]");
        assert_eq!(sanitize_filename("   "), "Anime");
    }

    #[test]
    fn test_parse_episode_range() {
        assert_eq!(parse_episode_range("1-5", 12, &[]), vec![1, 2, 3, 4, 5]);
        assert_eq!(parse_episode_range("4", 12, &[]), vec![4]);
        assert_eq!(parse_episode_range("all", 4, &[]), vec![1, 2, 3, 4]);
        assert_eq!(parse_episode_range("unwatched", 12, &[3, 1, 5]), vec![1, 3, 5]);
        assert_eq!(parse_episode_range("invalid", 12, &[]), Vec::<u32>::new());
    }
}
