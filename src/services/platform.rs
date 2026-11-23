//! Cross-platform hardware, `libmpv` capability, power, and thermal intelligence.
//!
//! Probes macOS, Windows, and Linux once per process (`OnceLock`) so Solus selects
//! zero-copy hardware decoders, gates `vo=gpu-next` (`libplacebo`) and `--glsl-shader-opts`
//! (`//!PARAM`) flags by actual binary support, and exposes live power/thermal snapshots for
//! the Control Hub Inspector and System Doctor.

use crate::api::player::find_binary;
use std::process::Command;
use std::sync::OnceLock;

/// Cross-platform thermal pressure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[allow(dead_code)]
pub enum ThermalLevel {
    #[default]
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[allow(dead_code)]
impl ThermalLevel {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Nominal => 0,
            Self::Fair => 1,
            Self::Serious => 2,
            Self::Critical => 3,
        }
    }

    pub const fn short_badge(self) -> &'static str {
        match self {
            Self::Nominal => "COOL",
            Self::Fair => "WARM",
            Self::Serious => "HOT",
            Self::Critical => "CRIT",
        }
    }

    pub fn from_celsius(temp_c: f32) -> Self {
        if temp_c >= 90.0 {
            Self::Critical
        } else if temp_c >= 82.0 {
            Self::Serious
        } else if temp_c >= 70.0 {
            Self::Fair
        } else {
            Self::Nominal
        }
    }
}

/// Primary GPU vendor identified on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum GpuVendor {
    Apple,
    Nvidia,
    Amd,
    Intel,
    #[default]
    Unknown,
}

/// Static hardware & OS decoder capability snapshot detected once per session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct DeviceProfile {
    pub os_label: String,
    pub display_server: String,
    pub gpu_vendor: GpuVendor,
    pub gpu_label: String,
    pub ram_gb: Option<u32>,
    pub hw_av1_decode: bool,
    pub hw_vp9_decode: bool,
    pub hw_hevc_decode: bool,
    pub recommended_hwdec: &'static str,
    pub recommended_gpu_api: &'static str,
    pub is_laptop_or_handheld: bool,
}

#[allow(dead_code)]
impl DeviceProfile {
    pub fn decode_summary(&self) -> String {
        let mut caps = Vec::new();
        if self.hw_av1_decode {
            caps.push("AV1");
        }
        if self.hw_vp9_decode {
            caps.push("VP9");
        }
        if self.hw_hevc_decode {
            caps.push("HEVC");
        }
        caps.push("H264");
        format!("HW {} ({})", caps.join("+"), self.recommended_hwdec)
    }

    pub fn inspector_header(&self) -> String {
        match self.ram_gb {
            Some(gb) => format!(
                "{} · {} ({}GB) · {}",
                self.os_label, self.gpu_label, gb, self.display_server
            ),
            None => format!("{} · {} · {}", self.os_label, self.gpu_label, self.display_server),
        }
    }
}

/// Probed software capabilities of the installed `mpv` / `libmpv` player binary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct PlayerCapabilities {
    pub binary_name: String,
    pub version_str: String,
    pub has_gpu_next: bool,
    pub has_shader_opts: bool,
    pub has_perceptual_gamut: bool,
    pub has_hdr_contrast_recovery: bool,
    pub has_gpu_shader_cache_dir: bool,
    pub has_vulkan: bool,
    pub has_d3d11: bool,
    pub has_opengl: bool,
}

#[allow(dead_code)]
impl PlayerCapabilities {
    pub fn conservative_fallback() -> Self {
        Self {
            binary_name: "mpv".to_string(),
            version_str: "unknown".to_string(),
            has_gpu_next: false,
            has_shader_opts: false,
            has_perceptual_gamut: false,
            has_hdr_contrast_recovery: false,
            has_gpu_shader_cache_dir: false,
            has_vulkan: false,
            has_d3d11: cfg!(target_os = "windows"),
            has_opengl: true,
        }
    }

    pub const fn effective_vo(&self) -> &'static str {
        if self.has_gpu_next {
            "gpu-next"
        } else {
            "gpu"
        }
    }

    pub fn effective_gpu_api(&self, dev: &DeviceProfile) -> &'static str {
        match dev.recommended_gpu_api {
            "d3d11" if self.has_d3d11 => "d3d11",
            "vulkan" if self.has_vulkan => "vulkan",
            _ if self.has_vulkan => "vulkan",
            _ if self.has_d3d11 => "d3d11",
            _ => "auto",
        }
    }

    pub fn inspector_summary(&self) -> String {
        let vo = self.effective_vo();
        let param_flag = if self.has_shader_opts { "Live" } else { "Static" };
        let plc_flag = if self.has_perceptual_gamut && self.has_hdr_contrast_recovery {
            "libplacebo Full"
        } else if self.has_gpu_next {
            "libplacebo"
        } else {
            "Legacy GL"
        };
        format!(
            "{} {} · vo={} · //!PARAM: {} · {}",
            self.binary_name, self.version_str, vo, param_flag, plc_flag
        )
    }
}

/// Live cross-platform power and thermal snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[allow(dead_code)]
pub struct PowerThermalSnapshot {
    pub on_battery: bool,
    pub battery_pct: Option<u8>,
    pub battery_saver: bool,
    pub temp_celsius: Option<f32>,
    pub thermal_level: ThermalLevel,
}

#[allow(dead_code)]
impl PowerThermalSnapshot {
    pub fn effective_thermal_tier(&self) -> u8 {
        let mut tier = self.thermal_level.as_u8();
        if self.battery_saver || (self.on_battery && self.battery_pct.is_some_and(|p| p < 25)) {
            tier = tier.max(2);
        } else if self.on_battery {
            tier = tier.max(1);
        }
        tier.min(3)
    }

    pub fn status_badge(&self) -> String {
        let pwr = match (self.on_battery, self.battery_pct, self.battery_saver) {
            (true, Some(pct), true) => format!("BATT {pct}% SAVER"),
            (true, Some(pct), false) => format!("BATT {pct}%"),
            (true, None, _) => "BATTERY".to_string(),
            (false, Some(pct), _) => format!("AC {pct}%"),
            (false, None, _) => "AC".to_string(),
        };
        match self.temp_celsius {
            Some(c) => format!("{pwr} · {c:.0}C {}", self.thermal_level.short_badge()),
            None => format!("{pwr} · {}", self.thermal_level.short_badge()),
        }
    }
}

static DEVICE_PROFILE_CACHE: OnceLock<DeviceProfile> = OnceLock::new();
static PLAYER_CAPS_CACHE: OnceLock<PlayerCapabilities> = OnceLock::new();

/// Cached hardware & OS capability profile (probed once per session).
pub fn detect_device_profile() -> &'static DeviceProfile {
    DEVICE_PROFILE_CACHE.get_or_init(probe_device_profile_uncached)
}

/// Cached installed `mpv` / `libmpv` binary capabilities.
pub fn detect_player_capabilities() -> &'static PlayerCapabilities {
    PLAYER_CAPS_CACHE.get_or_init(probe_player_capabilities_uncached)
}

fn mem_bytes_to_gb(mem_bytes: Option<u64>) -> Option<u32> {
    mem_bytes
        .filter(|&b| b > 0)
        .map(|b| ((b as f64) / (1024.0 * 1024.0 * 1024.0)).round().max(1.0) as u32)
}

fn parse_apple_m_generation(lower: &str) -> Option<u32> {
    let idx = lower.find("apple m")?;
    let rest = &lower[idx + "apple m".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

pub fn parse_macos_chip_profile(
    brand: &str,
    mem_bytes: Option<u64>,
    has_battery: bool,
) -> DeviceProfile {
    let trimmed = brand.trim();
    let lower = trimmed.to_ascii_lowercase();
    let apple_gen = parse_apple_m_generation(&lower);
    let is_apple = apple_gen.is_some() || lower.contains("apple");
    let hw_av1 = apple_gen.is_some_and(|g| g >= 3);

    let gpu_label = if trimmed.is_empty() {
        "Apple Graphics".to_string()
    } else if is_apple {
        trimmed.to_string()
    } else {
        format!("Intel Mac ({trimmed})")
    };

    DeviceProfile {
        os_label: "macOS".to_string(),
        display_server: "Metal".to_string(),
        gpu_vendor: if is_apple {
            GpuVendor::Apple
        } else {
            GpuVendor::Intel
        },
        gpu_label,
        ram_gb: mem_bytes_to_gb(mem_bytes),
        hw_av1_decode: hw_av1,
        hw_vp9_decode: true,
        hw_hevc_decode: true,
        recommended_hwdec: "videotoolbox",
        recommended_gpu_api: "vulkan",
        is_laptop_or_handheld: has_battery,
    }
}

#[allow(dead_code)]
fn extract_first_model_number(haystack: &str, prefix: &str) -> Option<u32> {
    let idx = haystack.find(prefix)?;
    let rest = &haystack[idx + prefix.len()..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok()
}

#[allow(dead_code)]
fn classify_gpu_av1_support(lower: &str, vendor: GpuVendor) -> bool {
    match vendor {
        GpuVendor::Apple => parse_apple_m_generation(lower).is_some_and(|g| g >= 3),
        GpuVendor::Nvidia => {
            extract_first_model_number(lower, "rtx ").is_some_and(|n| n >= 3000)
                || lower.contains("ada")
                || lower.contains("blackwell")
        }
        GpuVendor::Amd => {
            if let Some(rx_num) = extract_first_model_number(lower, "rx ") {
                return rx_num >= 6600 || ((700..1000).contains(&rx_num) && rx_num != 700);
            }
            ["680m", "740m", "760m", "780m", "880m", "890m"]
                .iter()
                .any(|apu| lower.contains(apu))
                || lower.contains("rdna3")
                || lower.contains("rdna 3")
                || lower.contains("rdna4")
        }
        GpuVendor::Intel => {
            lower.contains("arc")
                || lower.contains("iris xe")
                || lower.contains("core ultra")
                || extract_first_model_number(lower, "uhd graphics ").is_some_and(|n| n >= 730)
        }
        GpuVendor::Unknown => false,
    }
}

#[allow(dead_code)]
fn infer_gpu_vendor(lower: &str, drm_vendor_hex: Option<&str>) -> GpuVendor {
    if let Some(hex) = drm_vendor_hex.map(|s| s.trim().to_ascii_lowercase()) {
        if hex == "0x10de" {
            return GpuVendor::Nvidia;
        } else if hex == "0x1002" {
            return GpuVendor::Amd;
        } else if hex == "0x8086" {
            return GpuVendor::Intel;
        }
    }
    if lower.contains("nvidia") || lower.contains("geforce") || lower.contains("rtx") || lower.contains("gtx") {
        GpuVendor::Nvidia
    } else if lower.contains("amd") || lower.contains("radeon") || lower.contains("rx ") {
        GpuVendor::Amd
    } else if lower.contains("intel") || lower.contains("iris") || lower.contains("uhd") || lower.contains("arc") {
        GpuVendor::Intel
    } else if lower.contains("apple") {
        GpuVendor::Apple
    } else {
        GpuVendor::Unknown
    }
}

#[allow(dead_code)]
pub fn parse_windows_gpu_profile(
    gpu_name: &str,
    mem_bytes: Option<u64>,
    has_battery: bool,
) -> DeviceProfile {
    let trimmed = gpu_name.trim();
    let lower = trimmed.to_ascii_lowercase();
    let vendor = infer_gpu_vendor(&lower, None);
    let hw_av1 = classify_gpu_av1_support(&lower, vendor);
    let hw_vp9 = !lower.contains("basic display") && vendor != GpuVendor::Unknown;
    let hw_hevc = hw_vp9;

    DeviceProfile {
        os_label: "Windows".to_string(),
        display_server: "D3D11".to_string(),
        gpu_vendor: vendor,
        gpu_label: if trimmed.is_empty() {
            "Windows GPU".to_string()
        } else {
            trimmed.to_string()
        },
        ram_gb: mem_bytes_to_gb(mem_bytes),
        hw_av1_decode: hw_av1,
        hw_vp9_decode: hw_vp9,
        hw_hevc_decode: hw_hevc,
        recommended_hwdec: "d3d11va",
        recommended_gpu_api: "d3d11",
        is_laptop_or_handheld: has_battery,
    }
}

#[allow(dead_code)]
pub fn parse_linux_profile(
    vainfo_out: Option<&str>,
    drm_vendor_hex: Option<&str>,
    gpu_name: Option<&str>,
    is_wayland: bool,
    mem_bytes: Option<u64>,
    has_battery: bool,
) -> DeviceProfile {
    let gpu_raw = gpu_name.unwrap_or("").trim();
    let lower = gpu_raw.to_ascii_lowercase();
    let vendor = infer_gpu_vendor(&lower, drm_vendor_hex);

    let (hw_av1, hw_vp9, hw_hevc) = if let Some(va) = vainfo_out {
        let has_va_profile = |needle: &str| {
            va.lines()
                .any(|l| l.contains(needle) && l.contains("VAEntrypointVLD"))
        };
        (
            has_va_profile("VAProfileAV1") || classify_gpu_av1_support(&lower, vendor),
            has_va_profile("VAProfileVP9") || vendor != GpuVendor::Unknown,
            has_va_profile("VAProfileHEVC") || vendor != GpuVendor::Unknown,
        )
    } else {
        (
            classify_gpu_av1_support(&lower, vendor),
            vendor != GpuVendor::Unknown,
            vendor != GpuVendor::Unknown,
        )
    };

    let (hwdec, gpu_api) = match vendor {
        GpuVendor::Nvidia => ("nvdec", "vulkan"),
        GpuVendor::Amd | GpuVendor::Intel => ("vaapi", "vulkan"),
        _ => ("auto-safe", "auto"),
    };

    let default_gpu_label = match vendor {
        GpuVendor::Nvidia => "NVIDIA GPU",
        GpuVendor::Amd => "AMD Radeon GPU",
        GpuVendor::Intel => "Intel Graphics",
        _ => "Linux GPU",
    };

    DeviceProfile {
        os_label: "Linux".to_string(),
        display_server: if is_wayland {
            "Wayland".to_string()
        } else {
            "X11".to_string()
        },
        gpu_vendor: vendor,
        gpu_label: if gpu_raw.is_empty() {
            default_gpu_label.to_string()
        } else {
            gpu_raw.to_string()
        },
        ram_gb: mem_bytes_to_gb(mem_bytes),
        hw_av1_decode: hw_av1,
        hw_vp9_decode: hw_vp9,
        hw_hevc_decode: hw_hevc,
        recommended_hwdec: hwdec,
        recommended_gpu_api: gpu_api,
        is_laptop_or_handheld: has_battery,
    }
}

pub fn parse_mpv_capabilities(
    binary_name: &str,
    version_out: &str,
    vo_help: &str,
    gpu_api_help: &str,
    list_options: &str,
) -> PlayerCapabilities {
    let version_str = version_out
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("mpv ")
                .and_then(|rest| rest.split_whitespace().next())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    let has_opt = |flag: &str| list_options.contains(flag);
    let has_word = |hay: &str, token: &str| {
        hay.lines()
            .any(|line| line.split_whitespace().any(|w| w.trim_end_matches(':') == token))
    };

    PlayerCapabilities {
        binary_name: binary_name.to_string(),
        version_str,
        has_gpu_next: has_word(vo_help, "gpu-next"),
        has_shader_opts: has_opt("--glsl-shader-opts"),
        has_perceptual_gamut: has_opt("--gamut-mapping-mode"),
        has_hdr_contrast_recovery: has_opt("--hdr-contrast-recovery"),
        has_gpu_shader_cache_dir: has_opt("--gpu-shader-cache-dir"),
        has_vulkan: has_word(gpu_api_help, "vulkan"),
        has_d3d11: has_word(gpu_api_help, "d3d11"),
        has_opengl: has_word(gpu_api_help, "opengl"),
    }
}

pub fn parse_macos_pmset_batt(out: &str) -> (bool, Option<u8>) {
    let lower = out.to_ascii_lowercase();
    let on_battery = lower.contains("'battery power'");
    let pct = out.lines().find_map(|line| {
        if !line.contains('%') {
            return None;
        }
        line.split(|c: char| !c.is_ascii_digit() && c != '%')
            .find_map(|tok| tok.strip_suffix('%').and_then(|d| d.parse::<u8>().ok()))
            .map(|p| p.min(100))
    });
    (on_battery, pct)
}

pub fn parse_macos_pmset_therm(out: &str) -> ThermalLevel {
    let lower = out.to_ascii_lowercase();
    if lower.contains("no thermal warning") {
        return ThermalLevel::Nominal;
    }
    if lower.contains("critical") {
        return ThermalLevel::Critical;
    }
    if lower.contains("serious") || lower.contains("heavy") {
        return ThermalLevel::Serious;
    }
    if lower.contains("cpu_speed_limit") {
        for line in lower.lines() {
            if line.contains("cpu_speed_limit") {
                if let Some(val) = line
                    .split('=')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    if val <= 65 {
                        return ThermalLevel::Critical;
                    } else if val <= 85 {
                        return ThermalLevel::Serious;
                    } else if val < 100 {
                        return ThermalLevel::Fair;
                    }
                }
            }
        }
    }
    if lower.contains("fair") || lower.contains("moderate") {
        return ThermalLevel::Fair;
    }
    ThermalLevel::Nominal
}

fn run_cmd_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    let mut txt = String::from_utf8_lossy(&out.stdout).into_owned();
    if txt.trim().is_empty() && !out.stderr.is_empty() {
        txt = String::from_utf8_lossy(&out.stderr).into_owned();
    }
    Some(txt)
}

fn probe_device_profile_uncached() -> DeviceProfile {
    #[cfg(target_os = "macos")]
    {
        let brand = run_cmd_stdout("sysctl", &["-n", "machdep.cpu.brand_string"]).unwrap_or_default();
        let mem_bytes = run_cmd_stdout("sysctl", &["-n", "hw.memsize"])
            .and_then(|s| s.trim().parse::<u64>().ok());
        let batt = run_cmd_stdout("pmset", &["-g", "batt"]).unwrap_or_default();
        let has_battery = batt.to_ascii_lowercase().contains("internalbattery");
        parse_macos_chip_profile(&brand, mem_bytes, has_battery)
    }

    #[cfg(target_os = "windows")]
    {
        let reg_out = run_cmd_stdout(
            "reg",
            &[
                "query",
                r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000",
                "/v",
                "DriverDesc",
            ],
        )
        .unwrap_or_default();
        let gpu_name = reg_out
            .lines()
            .find_map(|l| {
                let idx = l.find("REG_SZ")?;
                Some(l[idx + "REG_SZ".len()..].trim().to_string())
            })
            .unwrap_or_default();
        parse_windows_gpu_profile(&gpu_name, None, false)
    }

    #[cfg(target_os = "linux")]
    {
        let vainfo_out = run_cmd_stdout("vainfo", &[]);
        let drm_vendor = (0..4).find_map(|i| {
            std::fs::read_to_string(format!("/sys/class/drm/card{i}/device/vendor")).ok()
        });
        let gpu_name = run_cmd_stdout("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                run_cmd_stdout("lspci", &[]).and_then(|out| {
                    out.lines()
                        .find(|l| {
                            l.contains("VGA compatible controller")
                                || l.contains("3D controller")
                                || l.contains("Display controller")
                        })
                        .and_then(|l| l.split(": ").nth(1))
                        .map(|s| s.trim().to_string())
                })
            });
        let mem_bytes = std::fs::read_to_string("/proc/meminfo").ok().and_then(|txt| {
            txt.lines().find_map(|l| {
                let kb = l
                    .strip_prefix("MemTotal:")?
                    .trim()
                    .strip_suffix("kB")?
                    .trim()
                    .parse::<u64>()
                    .ok()?;
                Some(kb * 1024)
            })
        });
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let has_battery = std::fs::read_dir("/sys/class/power_supply")
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_ascii_uppercase()
                    .starts_with("BAT")
            });
        parse_linux_profile(
            vainfo_out.as_deref(),
            drm_vendor.as_deref(),
            gpu_name.as_deref(),
            is_wayland,
            mem_bytes,
            has_battery,
        )
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        DeviceProfile {
            os_label: std::env::consts::OS.to_string(),
            display_server: "Unknown".to_string(),
            gpu_vendor: GpuVendor::Unknown,
            gpu_label: "Generic GPU".to_string(),
            ram_gb: None,
            hw_av1_decode: false,
            hw_vp9_decode: false,
            hw_hevc_decode: false,
            recommended_hwdec: "auto-safe",
            recommended_gpu_api: "auto",
            is_laptop_or_handheld: false,
        }
    }
}

fn probe_player_capabilities_uncached() -> PlayerCapabilities {
    let bin_path = find_binary("mpv")
        .or_else(|| find_binary("mpvnet"))
        .or_else(|| find_binary("mpv.net"));
    let Some(path) = bin_path else {
        return PlayerCapabilities::conservative_fallback();
    };
    let bin = path.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mpv".to_string());

    let ver = run_cmd_stdout(&bin, &["--version"]).unwrap_or_default();
    let vo_help = run_cmd_stdout(&bin, &["--vo=help"]).unwrap_or_default();
    let api_help = run_cmd_stdout(&bin, &["--vo=gpu-next", "--gpu-api=help"])
        .filter(|s| !s.is_empty())
        .or_else(|| run_cmd_stdout(&bin, &["--gpu-api=help"]))
        .unwrap_or_default();
    let list_opts = run_cmd_stdout(&bin, &["--list-options"]).unwrap_or_default();

    parse_mpv_capabilities(&name, &ver, &vo_help, &api_help, &list_opts)
}

/// Synchronously sample current host power & thermal snapshot cleanly.
#[allow(dead_code)]
pub fn sample_power_thermal() -> PowerThermalSnapshot {
    #[cfg(target_os = "macos")]
    {
        let batt_out = run_cmd_stdout("pmset", &["-g", "batt"]).unwrap_or_default();
        let therm_out = run_cmd_stdout("pmset", &["-g", "therm"]).unwrap_or_default();
        let (on_battery, battery_pct) = parse_macos_pmset_batt(&batt_out);
        let thermal_level = parse_macos_pmset_therm(&therm_out);
        PowerThermalSnapshot {
            on_battery,
            battery_pct,
            battery_saver: false,
            temp_celsius: None,
            thermal_level,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        PowerThermalSnapshot::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_macos_silicon_and_capabilities() {
        let m2 = parse_macos_chip_profile("Apple M2", Some(16 * 1024 * 1024 * 1024), true);
        assert_eq!(m2.gpu_vendor, GpuVendor::Apple);
        assert!(!m2.hw_av1_decode);
        assert!(m2.hw_vp9_decode);
        assert_eq!(m2.recommended_hwdec, "videotoolbox");
        assert_eq!(m2.ram_gb, Some(16));

        let m3 = parse_macos_chip_profile("Apple M3 Pro", Some(18 * 1024 * 1024 * 1024), true);
        assert!(m3.hw_av1_decode);

        let caps = parse_mpv_capabilities(
            "mpv",
            "mpv 0.39.0 Copyright ...",
            "Valid values for option vo are:\n  gpu\n  gpu-next\n",
            "Available GPU APIs:\n  auto\n  vulkan\n",
            " --glsl-shader-opts\n --gamut-mapping-mode\n --hdr-contrast-recovery\n --gpu-shader-cache-dir\n",
        );
        assert_eq!(caps.effective_vo(), "gpu-next");
        assert!(caps.has_shader_opts);
        assert!(caps.has_perceptual_gamut);
    }

    #[test]
    fn test_parse_legacy_mpv_capabilities_falls_back_to_gpu() {
        let caps = parse_mpv_capabilities(
            "mpv",
            "mpv 0.34.1",
            "  gpu\n",
            "  opengl\n",
            "",
        );
        assert_eq!(caps.effective_vo(), "gpu");
        assert!(!caps.has_shader_opts);
    }

    #[test]
    fn test_parse_macos_pmset_batt_and_therm() {
        let (on_batt, pct) = parse_macos_pmset_batt(
            "Now drawing from 'Battery Power'\n -InternalBattery-0\t48%; discharging; 2:45",
        );
        assert!(on_batt);
        assert_eq!(pct, Some(48));
        assert_eq!(
            parse_macos_pmset_therm("Note: No thermal warning level has been recorded"),
            ThermalLevel::Nominal
        );
        assert_eq!(
            parse_macos_pmset_therm("CPU_Speed_Limit = 72"),
            ThermalLevel::Serious
        );
    }

    #[test]
    fn test_live_host_probe_succeeds() {
        let dev = detect_device_profile();
        assert!(!dev.os_label.is_empty());
        assert!(!dev.decode_summary().is_empty());
        let caps = detect_player_capabilities();
        assert!(!caps.inspector_summary().is_empty());
    }
}
