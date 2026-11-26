//! Linux Wayland compositor Picture-in-Picture (PiP) controller.
//!
//! macOS, Windows, and X11 honor `mpv`'s `ontop`, `border=no`, `window-scale`, and `geometry`
//! properties directly. Under native Wayland, `xdg-shell` forbids clients from positioning
//! themselves or pinning above other windows, and tiling compositors (Hyprland, Sway) tile new
//! windows by default. This module bridges Solus's PiP state to Hyprland (`hyprctl`) and Sway
//! (`swaymsg`) at runtime without modifying any user config files.

use serde::Deserialize;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Wayland `app_id` assigned to Solus-managed `mpv` windows on Linux.
pub const SOLUS_WAYLAND_APP_ID: &str = "solus-mpv";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipAnchor {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PipCornerPreset {
    pub label: &'static str,
    pub scale: f64,
    pub anchor: PipAnchor,
    pub margin: i32,
    pub mpv_geometry: &'static str,
}

/// Unified 6-preset PiP table shared across `mpv` flags, IPC commands, and Wayland compositors.
pub const PIP_PRESETS: [PipCornerPreset; 6] = [
    PipCornerPreset {
        label: "Bottom-Right 30%",
        scale: 0.30,
        anchor: PipAnchor::BottomRight,
        margin: 24,
        mpv_geometry: "30%x30%-24-24",
    },
    PipCornerPreset {
        label: "Bottom-Left 30%",
        scale: 0.30,
        anchor: PipAnchor::BottomLeft,
        margin: 24,
        mpv_geometry: "30%x30%+24-24",
    },
    PipCornerPreset {
        label: "Top-Right 30%",
        scale: 0.30,
        anchor: PipAnchor::TopRight,
        margin: 24,
        mpv_geometry: "30%x30%-24+24",
    },
    PipCornerPreset {
        label: "Top-Left 30%",
        scale: 0.30,
        anchor: PipAnchor::TopLeft,
        margin: 24,
        mpv_geometry: "30%x30%+24+24",
    },
    PipCornerPreset {
        label: "Compact 24%",
        scale: 0.24,
        anchor: PipAnchor::BottomRight,
        margin: 20,
        mpv_geometry: "24%x24%-20-20",
    },
    PipCornerPreset {
        label: "Theater 38%",
        scale: 0.38,
        anchor: PipAnchor::BottomRight,
        margin: 28,
        mpv_geometry: "38%x38%-28-28",
    },
];

pub fn pip_preset(idx: usize) -> &'static PipCornerPreset {
    &PIP_PRESETS[idx % PIP_PRESETS.len()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorGeometry {
    pub x: i32,
    pub y: i32,
    pub phys_width: i32,
    pub phys_height: i32,
    pub scale: f64,
    pub transform: i32,
    /// `[left, top, right, bottom]` reserved panel insets in logical pixels.
    pub reserved: [i32; 4],
}

/// Compute logical 16:9 floating PiP coordinates respecting HiDPI scaling, rotation, and panels.
pub fn compute_pip_rect(mon: MonitorGeometry, preset: &PipCornerPreset) -> PipRect {
    let scale = if mon.scale.is_finite() && mon.scale > 0.1 {
        mon.scale
    } else {
        1.0
    };
    let (raw_w, raw_h) = if mon.transform.rem_euclid(2) == 1 {
        (mon.phys_height, mon.phys_width)
    } else {
        (mon.phys_width, mon.phys_height)
    };
    let logical_w = ((raw_w as f64) / scale).round() as i32;
    let logical_h = ((raw_h as f64) / scale).round() as i32;

    let [res_left, res_top, res_right, res_bottom] = mon.reserved;
    let usable_x = mon.x + res_left.max(0);
    let usable_y = mon.y + res_top.max(0);
    let usable_w = (logical_w - res_left.max(0) - res_right.max(0)).max(320);
    let usable_h = (logical_h - res_top.max(0) - res_bottom.max(0)).max(180);

    let mut pip_w = ((usable_w as f64) * preset.scale).round() as i32;
    let mut pip_h = ((pip_w as f64) * 9.0 / 16.0).round() as i32;

    // On portrait/tall monitors, cap PiP height to 65% of usable height while keeping 16:9.
    let max_h = ((usable_h as f64) * 0.65).round() as i32;
    if pip_h > max_h && max_h >= 120 {
        pip_h = max_h;
        pip_w = ((pip_h as f64) * 16.0 / 9.0).round() as i32;
    }

    pip_w = pip_w.clamp(240, usable_w.saturating_sub(16).max(240));
    pip_h = pip_h.clamp(135, usable_h.saturating_sub(16).max(135));

    let margin = preset.margin.min((usable_w - pip_w) / 2).min((usable_h - pip_h) / 2).max(0);
    let (x, y) = match preset.anchor {
        PipAnchor::BottomRight => (
            usable_x + usable_w - pip_w - margin,
            usable_y + usable_h - pip_h - margin,
        ),
        PipAnchor::BottomLeft => (
            usable_x + margin,
            usable_y + usable_h - pip_h - margin,
        ),
        PipAnchor::TopRight => (
            usable_x + usable_w - pip_w - margin,
            usable_y + margin,
        ),
        PipAnchor::TopLeft => (usable_x + margin, usable_y + margin),
    };

    PipRect {
        x,
        y,
        width: pip_w,
        height: pip_h,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxPipBackend {
    Hyprland,
    Sway,
    UnsupportedWayland(String),
    X11OrNative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipApplyOutcome {
    NotNeeded,
    Applied,
    WindowNotFound,
    UnsupportedCompositor(String),
    Failed(String),
}

pub fn classify_linux_pip_backend(
    has_hypr_sig: bool,
    has_hyprctl: bool,
    has_swaysock: bool,
    has_swaymsg: bool,
    has_wayland_display: bool,
    desktop_name: &str,
) -> LinuxPipBackend {
    if has_hypr_sig && has_hyprctl {
        LinuxPipBackend::Hyprland
    } else if has_swaysock && has_swaymsg {
        LinuxPipBackend::Sway
    } else if has_wayland_display {
        let label = if desktop_name.trim().is_empty() {
            "Wayland".to_string()
        } else {
            desktop_name.trim().to_string()
        };
        LinuxPipBackend::UnsupportedWayland(label)
    } else {
        LinuxPipBackend::X11OrNative
    }
}

pub fn detect_linux_pip_backend() -> LinuxPipBackend {
    #[cfg(not(target_os = "linux"))]
    {
        LinuxPipBackend::X11OrNative
    }
    #[cfg(target_os = "linux")]
    {
        let has_hypr_sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
            .is_some_and(|v| !v.is_empty());
        let has_swaysock = std::env::var_os("SWAYSOCK").is_some_and(|v| !v.is_empty());
        let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let has_hyprctl = has_hypr_sig && crate::api::player::find_binary("hyprctl").is_some();
        let has_swaymsg = has_swaysock && crate::api::player::find_binary("swaymsg").is_some();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        classify_linux_pip_backend(
            has_hypr_sig,
            has_hyprctl,
            has_swaysock,
            has_swaymsg,
            has_wayland,
            &desktop,
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HyprClient {
    pub address: String,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub fullscreen: i32,
    #[serde(default)]
    pub monitor: i64,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub pid: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HyprMonitor {
    pub id: i64,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub transform: i32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub reserved: [i32; 4],
}

fn default_scale() -> f64 {
    1.0
}

impl From<&HyprMonitor> for MonitorGeometry {
    fn from(m: &HyprMonitor) -> Self {
        Self {
            x: m.x,
            y: m.y,
            phys_width: m.width,
            phys_height: m.height,
            scale: m.scale,
            transform: m.transform,
            reserved: m.reserved,
        }
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn collect_pid_and_descendants(root_pid: u32) -> HashSet<u32> {
    let mut pids = HashSet::new();
    pids.insert(root_pid);
    #[cfg(target_os = "linux")]
    {
        let mut queue = vec![root_pid];
        while let Some(pid) = queue.pop() {
            let task_dir = format!("/proc/{pid}/task");
            let Ok(entries) = std::fs::read_dir(task_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let children_path = entry.path().join("children");
                let Ok(txt) = std::fs::read_to_string(children_path) else {
                    continue;
                };
                for tok in txt.split_whitespace() {
                    if let Ok(child) = tok.parse::<u32>() {
                        if pids.insert(child) {
                            queue.push(child);
                        }
                    }
                }
            }
        }
    }
    pids
}

pub fn find_hypr_target_client<'a>(
    clients: &'a [HyprClient],
    pids: &HashSet<u32>,
) -> Option<&'a HyprClient> {
    clients
        .iter()
        .find(|c| c.mapped && !c.address.is_empty() && pids.contains(&c.pid))
        .or_else(|| {
            clients.iter().find(|c| {
                c.mapped
                    && !c.address.is_empty()
                    && c.class.eq_ignore_ascii_case(SOLUS_WAYLAND_APP_ID)
            })
        })
}

pub fn resolve_hypr_monitor_geometry(
    monitors: &[HyprMonitor],
    monitor_id: i64,
) -> Option<MonitorGeometry> {
    monitors
        .iter()
        .find(|m| m.id == monitor_id)
        .or_else(|| monitors.iter().find(|m| m.focused))
        .or_else(|| monitors.first())
        .map(MonitorGeometry::from)
}

/// Build the Lua script for `hyprctl eval` on Hyprland >= 0.57.
pub fn build_hypr_lua_script(
    client: &HyprClient,
    enabled: bool,
    rect: Option<PipRect>,
) -> Option<String> {
    let addr = format!("address:{}", client.address);
    let mut stmts: Vec<String> = Vec::new();
    if enabled {
        let r = rect?;
        if client.fullscreen != 0 {
            stmts.push(format!(
                "hl.dispatch(hl.dsp.window.fullscreen({{ window = '{addr}', internal = 0, client = 0 }}))"
            ));
        }
        if !client.floating {
            stmts.push(format!(
                "hl.dispatch(hl.dsp.window.float({{ window = '{addr}', action = 'enable' }}))"
            ));
        }
        stmts.push(format!(
            "hl.dispatch(hl.dsp.window.resize({{ window = '{addr}', x = {}, y = {}, relative = false }}))",
            r.width, r.height
        ));
        stmts.push(format!(
            "hl.dispatch(hl.dsp.window.move({{ window = '{addr}', x = {}, y = {}, relative = false }}))",
            r.x, r.y
        ));
        if !client.pinned {
            stmts.push(format!(
                "hl.dispatch(hl.dsp.window.pin({{ window = '{addr}', action = 'enable' }}))"
            ));
        }
    } else {
        if client.pinned {
            stmts.push(format!(
                "hl.dispatch(hl.dsp.window.pin({{ window = '{addr}', action = 'disable' }}))"
            ));
        }
        if client.floating {
            stmts.push(format!(
                "hl.dispatch(hl.dsp.window.float({{ window = '{addr}', action = 'disable' }}))"
            ));
        }
    }

    if stmts.is_empty() {
        None
    } else {
        Some(stmts.join("; "))
    }
}

/// Build legacy `hyprctl --batch` command string for Hyprland < 0.57.
pub fn build_hypr_legacy_batch(
    client: &HyprClient,
    enabled: bool,
    rect: Option<PipRect>,
) -> Option<String> {
    let addr = format!("address:{}", client.address);
    let mut cmds: Vec<String> = Vec::new();
    if enabled {
        let r = rect?;
        if !client.floating {
            cmds.push(format!("dispatch setfloating {addr}"));
        }
        cmds.push(format!(
            "dispatch resizewindowpixel exact {} {},{addr}",
            r.width, r.height
        ));
        cmds.push(format!(
            "dispatch movewindowpixel exact {} {},{addr}",
            r.x, r.y
        ));
        if !client.pinned {
            cmds.push(format!("dispatch pin {addr}"));
        }
    } else {
        if client.pinned {
            cmds.push(format!("dispatch pin {addr}"));
        }
        if client.floating {
            cmds.push(format!("dispatch settiled {addr}"));
        }
    }

    if cmds.is_empty() {
        None
    } else {
        Some(cmds.join("; "))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwayNode {
    pub id: i64,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub nodes: Vec<SwayNode>,
    #[serde(default)]
    pub floating_nodes: Vec<SwayNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwayOutputRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwayOutput {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub focused: bool,
    pub rect: SwayOutputRect,
}

pub fn find_sway_target_con_id(node: &SwayNode, pids: &HashSet<u32>) -> Option<i64> {
    if node.pid.is_some_and(|p| pids.contains(&p)) {
        return Some(node.id);
    }
    if node
        .app_id
        .as_deref()
        .is_some_and(|a| a.eq_ignore_ascii_case(SOLUS_WAYLAND_APP_ID))
    {
        return Some(node.id);
    }
    for child in node.floating_nodes.iter().chain(node.nodes.iter()) {
        if let Some(id) = find_sway_target_con_id(child, pids) {
            return Some(id);
        }
    }
    None
}

pub fn resolve_sway_monitor_geometry(outputs: &[SwayOutput]) -> Option<MonitorGeometry> {
    outputs
        .iter()
        .find(|o| o.active && o.focused)
        .or_else(|| outputs.iter().find(|o| o.active))
        .or_else(|| outputs.first())
        .map(|o| MonitorGeometry {
            x: o.rect.x,
            y: o.rect.y,
            phys_width: o.rect.width,
            phys_height: o.rect.height,
            scale: 1.0, // Sway's output rect is already in logical pixels
            transform: 0,
            reserved: [0, 0, 0, 0],
        })
}

pub fn build_sway_pip_command(con_id: i64, enabled: bool, rect: Option<PipRect>) -> Option<String> {
    if enabled {
        let r = rect?;
        Some(format!(
            "[con_id={con_id}] fullscreen disable, floating enable, sticky enable, resize set {} px {} px, move position {} px {} px",
            r.width, r.height, r.x, r.y
        ))
    } else {
        Some(format!(
            "[con_id={con_id}] sticky disable, floating disable"
        ))
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
static HYPR_USES_LUA_EVAL: OnceLock<bool> = OnceLock::new();

#[cfg(target_os = "linux")]
async fn hypr_supports_lua_eval() -> bool {
    if let Some(&cached) = HYPR_USES_LUA_EVAL.get() {
        return cached;
    }
    let supported = match tokio::process::Command::new("hyprctl")
        .args(["eval", "return 1"])
        .output()
        .await
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            out.status.success() && stdout.trim() == "ok"
        }
        Err(_) => false,
    };
    let _ = HYPR_USES_LUA_EVAL.set(supported);
    supported
}

#[cfg(target_os = "linux")]
async fn hyprctl_json<T: for<'de> Deserialize<'de>>(subcmd: &str) -> Result<T, String> {
    let out = tokio::process::Command::new("hyprctl")
        .args(["-j", subcmd])
        .output()
        .await
        .map_err(|e| format!("Failed to invoke hyprctl -j {subcmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "hyprctl -j {subcmd} exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Failed to parse hyprctl -j {subcmd}: {e}"))
}

#[cfg(target_os = "linux")]
async fn apply_hyprland_pip(
    enabled: bool,
    corner_idx: usize,
    player_pid: Option<u32>,
    wait_for_map: bool,
) -> PipApplyOutcome {
    let max_attempts = if wait_for_map { 28 } else { 3 };
    let mut target_client: Option<HyprClient> = None;

    for attempt in 0..max_attempts {
        let pids = player_pid
            .map(collect_pid_and_descendants)
            .unwrap_or_default();
        if let Ok(clients) = hyprctl_json::<Vec<HyprClient>>("clients").await {
            if let Some(found) = find_hypr_target_client(&clients, &pids) {
                target_client = Some(found.clone());
                break;
            }
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        }
    }

    let Some(client) = target_client else {
        return PipApplyOutcome::WindowNotFound;
    };

    let rect = if enabled {
        let monitors = match hyprctl_json::<Vec<HyprMonitor>>("monitors").await {
            Ok(m) => m,
            Err(e) => return PipApplyOutcome::Failed(e),
        };
        let Some(mon) = resolve_hypr_monitor_geometry(&monitors, client.monitor) else {
            return PipApplyOutcome::Failed("No active Hyprland monitor found".to_string());
        };
        Some(compute_pip_rect(mon, pip_preset(corner_idx)))
    } else {
        None
    };

    if hypr_supports_lua_eval().await {
        let Some(script) = build_hypr_lua_script(&client, enabled, rect) else {
            return PipApplyOutcome::Applied;
        };
        let out = tokio::process::Command::new("hyprctl")
            .args(["eval", &script])
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if stdout.trim_start().starts_with("error:") {
                    PipApplyOutcome::Failed(stdout.trim().to_string())
                } else {
                    PipApplyOutcome::Applied
                }
            }
            Ok(o) => PipApplyOutcome::Failed(format!(
                "hyprctl eval failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => PipApplyOutcome::Failed(format!("Failed to execute hyprctl eval: {e}")),
        }
    } else {
        let Some(batch) = build_hypr_legacy_batch(&client, enabled, rect) else {
            return PipApplyOutcome::Applied;
        };
        let out = tokio::process::Command::new("hyprctl")
            .args(["--batch", &batch])
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => PipApplyOutcome::Applied,
            Ok(o) => PipApplyOutcome::Failed(format!(
                "hyprctl --batch failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => PipApplyOutcome::Failed(format!("Failed to execute hyprctl --batch: {e}")),
        }
    }
}

#[cfg(target_os = "linux")]
async fn swaymsg_json<T: for<'de> Deserialize<'de>>(msg_type: &str) -> Result<T, String> {
    let out = tokio::process::Command::new("swaymsg")
        .args(["-t", msg_type])
        .output()
        .await
        .map_err(|e| format!("Failed to invoke swaymsg -t {msg_type}: {e}"))?;
    if !out.status.success() {
        return Err(format!("swaymsg -t {msg_type} exited with {}", out.status));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("Failed to parse swaymsg -t {msg_type}: {e}"))
}

#[cfg(target_os = "linux")]
async fn apply_sway_pip(
    enabled: bool,
    corner_idx: usize,
    player_pid: Option<u32>,
    wait_for_map: bool,
) -> PipApplyOutcome {
    let max_attempts = if wait_for_map { 28 } else { 3 };
    let mut target_con_id: Option<i64> = None;

    for attempt in 0..max_attempts {
        let pids = player_pid
            .map(collect_pid_and_descendants)
            .unwrap_or_default();
        if let Ok(tree) = swaymsg_json::<SwayNode>("get_tree").await {
            if let Some(id) = find_sway_target_con_id(&tree, &pids) {
                target_con_id = Some(id);
                break;
            }
        }
        if attempt + 1 < max_attempts {
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        }
    }

    let Some(con_id) = target_con_id else {
        return PipApplyOutcome::WindowNotFound;
    };

    let rect = if enabled {
        let outputs = match swaymsg_json::<Vec<SwayOutput>>("get_outputs").await {
            Ok(o) => o,
            Err(e) => return PipApplyOutcome::Failed(e),
        };
        let Some(mon) = resolve_sway_monitor_geometry(&outputs) else {
            return PipApplyOutcome::Failed("No active Sway output found".to_string());
        };
        Some(compute_pip_rect(mon, pip_preset(corner_idx)))
    } else {
        None
    };

    let Some(cmd) = build_sway_pip_command(con_id, enabled, rect) else {
        return PipApplyOutcome::Applied;
    };

    match tokio::process::Command::new("swaymsg")
        .arg(&cmd)
        .output()
        .await
    {
        Ok(o) if o.status.success() => PipApplyOutcome::Applied,
        Ok(o) => PipApplyOutcome::Failed(format!(
            "swaymsg failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => PipApplyOutcome::Failed(format!("Failed to execute swaymsg: {e}")),
    }
}

/// Apply or remove Picture-in-Picture window state on Linux Wayland compositors.
/// Returns `PipApplyOutcome::NotNeeded` immediately on macOS, Windows, and X11.
pub async fn apply_linux_pip(
    enabled: bool,
    corner_idx: usize,
    player_pid: Option<u32>,
    wait_for_map: bool,
) -> PipApplyOutcome {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (enabled, corner_idx, player_pid, wait_for_map);
        PipApplyOutcome::NotNeeded
    }
    #[cfg(target_os = "linux")]
    {
        match detect_linux_pip_backend() {
            LinuxPipBackend::Hyprland => {
                apply_hyprland_pip(enabled, corner_idx, player_pid, wait_for_map).await
            }
            LinuxPipBackend::Sway => {
                apply_sway_pip(enabled, corner_idx, player_pid, wait_for_map).await
            }
            LinuxPipBackend::UnsupportedWayland(de) => {
                PipApplyOutcome::UnsupportedCompositor(de)
            }
            LinuxPipBackend::X11OrNative => PipApplyOutcome::NotNeeded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_pip_rect_hidpi_with_reserved_topbar() {
        let mon = MonitorGeometry {
            x: 0,
            y: 0,
            phys_width: 2560,
            phys_height: 1600,
            scale: 1.6,
            transform: 0,
            reserved: [0, 40, 0, 0],
        };
        // Logical = 1600x1000, usable = (0, 40, 1600, 960)
        let br = compute_pip_rect(mon, pip_preset(0));
        assert_eq!(
            br,
            PipRect {
                x: 1096,
                y: 706,
                width: 480,
                height: 270
            }
        );

        let tr = compute_pip_rect(mon, pip_preset(2));
        assert_eq!(
            tr,
            PipRect {
                x: 1096,
                y: 64,
                width: 480,
                height: 270
            }
        );

        let bl = compute_pip_rect(mon, pip_preset(1));
        assert_eq!(
            bl,
            PipRect {
                x: 24,
                y: 706,
                width: 480,
                height: 270
            }
        );

        let tl = compute_pip_rect(mon, pip_preset(3));
        assert_eq!(
            tl,
            PipRect {
                x: 24,
                y: 64,
                width: 480,
                height: 270
            }
        );
    }

    #[test]
    fn test_compute_pip_rect_multi_monitor_and_rotated() {
        let mon = MonitorGeometry {
            x: 1920,
            y: 120,
            phys_width: 1920,
            phys_height: 1080,
            scale: 1.0,
            transform: 1, // 90 deg rotated -> logical 1080x1920
            reserved: [0, 0, 0, 0],
        };
        let compact = compute_pip_rect(mon, pip_preset(4));
        assert_eq!(compact.width, 259);
        assert_eq!(compact.height, 146);
        assert_eq!(compact.x, 1920 + 1080 - 259 - 20);
        assert_eq!(compact.y, 120 + 1920 - 146 - 20);
    }

    #[test]
    fn test_hyprland_lua_and_legacy_scripts_respect_existing_state() {
        let tiled = HyprClient {
            address: "0x1234".to_string(),
            mapped: true,
            floating: false,
            pinned: false,
            fullscreen: 0,
            monitor: 0,
            class: "solus-mpv".to_string(),
            pid: 4242,
        };
        let rect = Some(PipRect {
            x: 1096,
            y: 706,
            width: 480,
            height: 270,
        });

        let lua_on = build_hypr_lua_script(&tiled, true, rect).unwrap();
        assert!(lua_on.contains("hl.dsp.window.float({ window = 'address:0x1234', action = 'enable' })"));
        assert!(lua_on.contains("hl.dsp.window.resize({ window = 'address:0x1234', x = 480, y = 270, relative = false })"));
        assert!(lua_on.contains("hl.dsp.window.move({ window = 'address:0x1234', x = 1096, y = 706, relative = false })"));
        assert!(lua_on.contains("hl.dsp.window.pin({ window = 'address:0x1234', action = 'enable' })"));

        let legacy_on = build_hypr_legacy_batch(&tiled, true, rect).unwrap();
        assert!(legacy_on.contains("dispatch setfloating address:0x1234"));
        assert!(legacy_on.contains("dispatch resizewindowpixel exact 480 270,address:0x1234"));
        assert!(legacy_on.contains("dispatch movewindowpixel exact 1096 706,address:0x1234"));
        assert!(legacy_on.contains("dispatch pin address:0x1234"));

        // Already floating + pinned: never double-toggles pin or float
        let already_pip = HyprClient {
            floating: true,
            pinned: true,
            ..tiled.clone()
        };
        let lua_move = build_hypr_lua_script(&already_pip, true, rect).unwrap();
        assert!(!lua_move.contains("window.float"));
        assert!(!lua_move.contains("window.pin"));

        let legacy_move = build_hypr_legacy_batch(&already_pip, true, rect).unwrap();
        assert!(!legacy_move.contains("setfloating"));
        assert!(!legacy_move.contains("dispatch pin"));

        let lua_off = build_hypr_lua_script(&already_pip, false, None).unwrap();
        assert!(lua_off.contains("window.pin({ window = 'address:0x1234', action = 'disable' })"));
        assert!(lua_off.contains("window.float({ window = 'address:0x1234', action = 'disable' })"));

        assert!(build_hypr_lua_script(&tiled, false, None).is_none());
    }

    #[test]
    fn test_hypr_and_sway_json_lookup_and_backend_classification() {
        let clients: Vec<HyprClient> = serde_json::from_str(
            r#"[
                {"address":"0xaaa","mapped":true,"floating":false,"pinned":false,"fullscreen":0,"monitor":0,"class":"kitty","pid":100},
                {"address":"0xbbb","mapped":true,"floating":false,"pinned":false,"fullscreen":0,"monitor":1,"class":"solus-mpv","pid":200}
            ]"#,
        )
        .unwrap();
        let mut pids = HashSet::new();
        pids.insert(200);
        assert_eq!(find_hypr_target_client(&clients, &pids).unwrap().address, "0xbbb");

        let empty_pids = HashSet::new();
        assert_eq!(find_hypr_target_client(&clients, &empty_pids).unwrap().address, "0xbbb");

        let tree: SwayNode = serde_json::from_str(
            r#"{"id":1,"nodes":[{"id":12,"pid":555,"app_id":"solus-mpv","nodes":[],"floating_nodes":[]}],"floating_nodes":[]}"#,
        )
        .unwrap();
        let mut sway_pids = HashSet::new();
        sway_pids.insert(555);
        assert_eq!(find_sway_target_con_id(&tree, &sway_pids), Some(12));
        let cmd = build_sway_pip_command(
            12,
            true,
            Some(PipRect {
                x: 100,
                y: 200,
                width: 480,
                height: 270,
            }),
        )
        .unwrap();
        assert!(cmd.contains("[con_id=12]"));
        assert!(cmd.contains("sticky enable"));

        assert_eq!(
            classify_linux_pip_backend(true, true, false, false, true, "Hyprland"),
            LinuxPipBackend::Hyprland
        );
        assert_eq!(
            classify_linux_pip_backend(false, false, true, true, true, "sway"),
            LinuxPipBackend::Sway
        );
        assert_eq!(
            classify_linux_pip_backend(false, false, false, false, true, "GNOME"),
            LinuxPipBackend::UnsupportedWayland("GNOME".to_string())
        );
        assert_eq!(
            classify_linux_pip_backend(false, false, false, false, false, "i3"),
            LinuxPipBackend::X11OrNative
        );
    }
}
