//! mpv IPC socket monitor.
//! Connects over Unix domain socket to track live playback progress (time-pos, duration),
//! perform automated OP/ED skip seeking, and trigger 85% completion scrobbling.

use crate::api::aniskip::SkipResult;
use crate::db;
use crate::AppMessage;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::Sender;

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;

#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(not(any(unix, windows)))]
type IpcStream = tokio::io::DuplexStream;

async fn connect_ipc(sock_path: &str) -> Option<IpcStream> {
    #[cfg(unix)]
    {
        if Path::new(sock_path).exists() {
            tokio::net::UnixStream::connect(sock_path).await.ok()
        } else {
            None
        }
    }
    #[cfg(windows)]
    {
        tokio::net::windows::named_pipe::ClientOptions::new()
            .open(sock_path)
            .ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = sock_path;
        None
    }
}

/// Configuration for launching an mpv IPC monitoring session.
pub struct IpcMonitorConfig {
    pub anime_id:        i64,
    pub episode:         u32,
    pub sock_path:       String,
    pub pool:            SqlitePool,
    pub tx:              Sender<AppMessage>,
    pub auth_token:      Option<String>,
    pub skip_intervals:  Vec<SkipResult>,
    pub auto_skip_intro: bool,
}

/// Helper to parse property change data from mpv JSON-RPC.
pub fn parse_property_change(line: &str) -> Option<(String, f64)> {
    let parsed: Value = serde_json::from_str(line).ok()?;
    if parsed.get("event")?.as_str()? == "property-change" {
        let name = parsed.get("name")?.as_str()?.to_string();
        let data = parsed.get("data")?.as_f64()?;
        return Some((name, data));
    }
    None
}

/// Check if a given playback position falls inside any un-skipped interval.
pub fn check_skip_interval<'a>(
    pos: f64,
    intervals: &'a [SkipResult],
    skipped_types: &HashSet<String>,
) -> Option<&'a SkipResult> {
    for item in intervals {
        let key = format!("{}:{:.1}", item.skip_type, item.interval.start_time);
        if skipped_types.contains(&key) {
            continue;
        }
        // If within the skip window (allow 0.5s pre-trigger, and until 1.0s before end)
        if pos >= item.interval.start_time - 0.5 && pos < item.interval.end_time - 1.0 {
            return Some(item);
        }
    }
    None
}

/// Calculate whether the completion percentage has crossed the 85% threshold.
pub fn is_completion_threshold_reached(pos: f64, duration: f64) -> bool {
    if duration < 60.0 {
        return false;
    }
    (pos / duration) >= 0.85
}

/// Spawns the IPC background monitor task.
pub fn spawn_mpv_ipc_monitor(config: IpcMonitorConfig) {
    tokio::spawn(async move {
        run_mpv_ipc_monitor(config).await;
    });
}

async fn run_mpv_ipc_monitor(config: IpcMonitorConfig) {
    let IpcMonitorConfig {
        anime_id,
        episode,
        sock_path,
        pool,
        tx,
        auth_token,
        skip_intervals,
        auto_skip_intro,
    } = config;

    // Retry connecting to the socket / named pipe for up to 5 seconds
    let mut stream = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(s) = connect_ipc(&sock_path).await {
            stream = Some(s);
            break;
        }
    }

    let stream = match stream {
        Some(s) => s,
        None => {
            // Clean up socket file if left behind
            #[cfg(unix)]
            let _ = std::fs::remove_file(&sock_path);
            return;
        }
    };

    let (reader_half, mut writer_half) = tokio::io::split(stream);

    // Subscribe to time-pos and duration property changes
    let observe_cmds = [
        r#"{"command": ["observe_property", 1, "time-pos"]}"#,
        r#"{"command": ["observe_property", 2, "duration"]}"#,
    ];
    for cmd in observe_cmds {
        let _ = writer_half.write_all(format!("{}\n", cmd).as_bytes()).await;
    }
    let _ = writer_half.flush().await;

    let mut reader = BufReader::new(reader_half).lines();
    let mut current_pos = 0.0;
    let mut current_duration = 0.0;
    let mut last_saved_pos = 0.0;
    let mut last_save_time = tokio::time::Instant::now();
    let mut scrobbled = false;
    let mut skipped_intervals: HashSet<String> = HashSet::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if let Some((name, val)) = parse_property_change(&line) {
            match name.as_str() {
                "duration" => {
                    current_duration = val;
                }
                "time-pos" => {
                    current_pos = val;

                    // 1. Auto-skip intro / outro
                    if auto_skip_intro {
                        if let Some(to_skip) = check_skip_interval(current_pos, &skip_intervals, &skipped_intervals) {
                            let key = format!("{}:{:.1}", to_skip.skip_type, to_skip.interval.start_time);
                            skipped_intervals.insert(key);

                            let seek_cmd = format!(
                                "{{\"command\": [\"seek\", {}, \"absolute\"]}}\n",
                                to_skip.interval.end_time
                            );
                            let osd_label = match to_skip.skip_type.as_str() {
                                "ed" | "mixed-ed" => "Outro",
                                _ => "Intro",
                            };
                            let osd_cmd = format!(
                                "{{\"command\": [\"show-text\", \"󰄬 Solus: Auto-skipped {}\", 3000]}}\n",
                                osd_label
                            );

                            let _ = writer_half.write_all(seek_cmd.as_bytes()).await;
                            let _ = writer_half.write_all(osd_cmd.as_bytes()).await;
                            let _ = writer_half.flush().await;

                            let _ = tx
                                .send(AppMessage::PlaybackLog(format!(
                                    "󰄬 Auto-skipped {} at {:.1}s -> {:.1}s",
                                    osd_label, current_pos, to_skip.interval.end_time
                                )))
                                .await;
                        }
                    }

                    // 2. Auto-scrobble once 85% completion threshold is reached
                    if !scrobbled && is_completion_threshold_reached(current_pos, current_duration) {
                        scrobbled = true;
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;

                        let pool_clone = pool.clone();
                        let token_clone = auth_token.clone();
                        tokio::spawn(async move {
                            let _ = db::user::record_watched(&pool_clone, anime_id, episode as i64, now).await;
                            if let Some(token) = token_clone {
                                let client = crate::api::anilist::AniListClient::with_token(Some(token));
                                let _ = client.save_progress(anime_id, episode).await;
                            }
                        });

                        let _ = tx.send(AppMessage::EpisodeAutoScrobbled(anime_id, episode)).await;
                        let _ = tx
                            .send(AppMessage::PlaybackLog(format!(
                                "󰄬 Episode {} completed (85% reached) — auto-scrobbled",
                                episode
                            )))
                            .await;
                    }

                    // 3. Throttle progress updates to UI and database
                    let now_instant = tokio::time::Instant::now();
                    if (current_pos - last_saved_pos).abs() >= 3.0 || now_instant.duration_since(last_save_time).as_secs() >= 4 {
                        last_saved_pos = current_pos;
                        last_save_time = now_instant;
                        let now_unix = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;

                        let _ = db::user::save_playback_progress(
                            &pool,
                            anime_id,
                            episode,
                            current_pos,
                            current_duration,
                            now_unix,
                        )
                        .await;

                        let _ = tx
                            .send(AppMessage::PlaybackProgressUpdated(
                                anime_id,
                                episode,
                                current_pos,
                                current_duration,
                            ))
                            .await;
                    }
                }
                _ => {}
            }
        }
    }

    // On player exit, persist the final position
    if current_pos > 0.0 {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let _ = db::user::save_playback_progress(
            &pool,
            anime_id,
            episode,
            current_pos,
            current_duration,
            now_unix,
        )
        .await;
        let _ = tx
            .send(AppMessage::PlaybackProgressUpdated(
                anime_id,
                episode,
                current_pos,
                current_duration,
            ))
            .await;
    }

    // Clean up socket file
    #[cfg(unix)]
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::aniskip::SkipInterval;

    #[test]
    fn test_parse_property_change() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":142.5}"#;
        let res = parse_property_change(line);
        assert_eq!(res, Some(("time-pos".to_string(), 142.5)));

        let dur_line = r#"{"event":"property-change","id":2,"name":"duration","data":1420.0}"#;
        let res2 = parse_property_change(dur_line);
        assert_eq!(res2, Some(("duration".to_string(), 1420.0)));

        let other = r#"{"request_id":0,"error":"success"}"#;
        assert_eq!(parse_property_change(other), None);
    }

    #[test]
    fn test_check_skip_interval() {
        let intervals = vec![
            SkipResult {
                interval: SkipInterval { start_time: 50.0, end_time: 140.0 },
                skip_type: "op".to_string(),
            },
            SkipResult {
                interval: SkipInterval { start_time: 1350.0, end_time: 1440.0 },
                skip_type: "ed".to_string(),
            },
        ];
        let mut skipped = HashSet::new();

        // Outside intervals
        assert!(check_skip_interval(20.0, &intervals, &skipped).is_none());

        // Inside OP interval
        let hit = check_skip_interval(50.5, &intervals, &skipped);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().skip_type, "op");

        // Mark OP as skipped
        skipped.insert("op:50.0".to_string());
        assert!(check_skip_interval(50.5, &intervals, &skipped).is_none());

        // Inside ED interval
        let hit_ed = check_skip_interval(1360.0, &intervals, &skipped);
        assert!(hit_ed.is_some());
        assert_eq!(hit_ed.unwrap().skip_type, "ed");
    }

    #[test]
    fn test_is_completion_threshold_reached() {
        assert!(!is_completion_threshold_reached(500.0, 1420.0)); // ~35%
        assert!(!is_completion_threshold_reached(1200.0, 1420.0)); // 84.5%
        assert!(is_completion_threshold_reached(1207.0, 1420.0));  // 85.0%
        assert!(is_completion_threshold_reached(1400.0, 1420.0));  // 98.5%
        assert!(!is_completion_threshold_reached(10.0, 20.0));    // < 60s video ignored
    }
}
