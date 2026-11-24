mod api;
mod config;
mod db;
mod error;
mod services;
mod state;
mod ui;

pub use ui::components::cover::{
    build_stateful_protocol, hd_resize_mode, scale_to_frame_contain, scale_to_frame_cover,
};

use anyhow::Context;
use crossterm::{
    event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use state::{AppState, Screen};
use std::{io, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

const QUALITY_CHOICES: [config::Quality; 5] = [
    config::Quality::Best,
    config::Quality::P1080,
    config::Quality::P720,
    config::Quality::P480,
    config::Quality::P360,
];

// ─── Message channel ──────────────────────────────────────────────────────────

/// Messages sent from background tokio tasks to the UI event loop.
enum AppMessage {
    /// Home screen data loaded / refreshed (boxed to shrink AppMessage enum size)
    HomeData(Box<ui::home::HomeData>),
    /// A log line from ani-cli stdout or stderr
    PlaybackLog(String),
    /// Playback process exited
    PlaybackDone {
        gen:          u64,
        exited_ok:    bool,
        natural_eof:  bool,
        user_quit:    bool,
        elapsed_secs: u64,
    },
    /// Live subtitle delay changed inside external libmpv player (e.g. Snap-to-Voice / z / x)
    PlayerSubDelayChanged {
        anime_id: i64,
        delay_ms: i32,
    },
    /// AniList search results
    SearchResults(Vec<db::cache::Anime>),
    /// Cover image downloaded and decoded (anime_id, image)
    CoverReady(i64, image::DynamicImage),
    /// Cover image could not be downloaded or decoded (anime_id)
    CoverFailed(i64),
    /// Off-thread pre-scaled & pre-encoded Home card/banner protocols ready for instant rendering
    PreloadCoverReady {
        gen: Option<u64>,
        anime_id: i64,
        card_proto: Box<dyn ratatui_image::protocol::StatefulProtocol>,
        banner_proto: Box<dyn ratatui_image::protocol::StatefulProtocol>,
        raw_img: image::DynamicImage,
    },
    /// Home preload item failed to download or decode
    PreloadCoverFailed {
        gen: Option<u64>,
        anime_id: i64,
    },
    /// Non-blocking startup dependency detection finished
    DependenciesReady {
        ani_cli: bool,
        mpv: bool,
        mpvnet: bool,
        celluloid: bool,
        prompt_setup: bool,
    },
    /// Watchlist changed — send fresh grouped list, provenance sources, and statuses to update home row immediately
    WatchlistUpdated(
        Vec<db::cache::Anime>,
        std::collections::HashMap<i64, db::user::WatchlistSource>,
        std::collections::HashMap<i64, config::WatchStatus>,
    ),
    /// Live Watchlist status / database writeback completed (`+` / `x` popup or direct delete)
    WatchlistWritebackCompleted {
        anime_id: i64,
        removed_completely: bool,
        added_to_wl: bool,
        outcome_summary: String,
        wl: Vec<db::cache::Anime>,
        sources: std::collections::HashMap<i64, db::user::WatchlistSource>,
        statuses: std::collections::HashMap<i64, config::WatchStatus>,
        cw: Vec<db::cache::Anime>,
    },
    /// Dual-tracker sync / OAuth / username sync completed cleanly
    TrackerSyncCompleted {
        provider: config::TrackerProvider,
        account: config::TrackerAccount,
        report: services::tracker_merge::MergeReport,
        home_data: Box<ui::home::HomeData>,
        summary: String,
    },
    /// Dual-tracker sync or OAuth failed
    TrackerSyncFailed(config::TrackerProvider, String),
    /// Local SQLite merge audit refreshed for `[2] Merge & Conflicts`
    MergeAuditReady(services::tracker_merge::MergeReport),
    /// Cross-tracker parity push (`[P]`) completed
    ParityPushCompleted {
        summary: String,
        report: services::tracker_merge::MergeReport,
        home_data: Box<ui::home::HomeData>,
    },
    /// Official MAL `.xml` / `.xml.gz` archive import completed
    MalXmlImportCompleted(std::result::Result<(services::tracker_merge::MergeReport, String, usize, Box<ui::home::HomeData>), String>),
    /// Background scrobble feedback toast
    ScrobbleSyncToast(String),
    /// Continue Watching changed — send fresh list to update home row immediately
    ContinueWatchingUpdated(Vec<db::cache::Anime>),
    /// Home banner watched-episode count for the active anime
    BannerProgress(i64, usize),
    /// Dynamic stream info (qualities, subtitles, master_url) detected for an anime episode
    StreamInfoDetected(i64, u32, bool, u64, api::subtitles::StreamInfo),
    /// Airing status and next episode refreshed from AniList
    AiringStatusUpdated(i64, Option<String>, Option<i64>, Option<i64>),
    /// Local JSON archive import completed (stats or error message)
    ArchiveImportCompleted(std::result::Result<services::importer::ImportStats, String>),
    /// Public username sync completed (viewer, stats or error message)
    PublicSyncCompleted(std::result::Result<(api::anilist::ViewerInfo, services::importer::ImportStats), String>),
    /// Backup exported to disk (exported count or error message)
    BackupExportCompleted(std::result::Result<usize, String>),
    /// Tailored recommendations scored candidate pool generated
    TailoredResultsReady(Vec<services::tailored::TailoredMatch>),
    /// AniList community peer recommendations loaded for `[m]` More Like This anchor
    TunerPeersReady(i64, Vec<db::cache::Anime>),
    /// Weekly release calendar data fetched for `week_offset`
    CalendarWeekReady {
        week_offset: i32,
        tz_offset_secs: i64,
        tracked_ids: std::collections::HashSet<i64>,
        watched_map: std::collections::HashMap<i64, std::collections::HashSet<u32>>,
        releases: Vec<api::anilist::CalendarRelease>,
    },
    /// Weekly release calendar fetch failed for `week_offset`
    CalendarWeekFailed(i32, String),
    /// Episode titles, streaming thumbnails, synopses and confidence metadata loaded
    EpisodeMetadataReady(i64, u64, std::collections::HashMap<u32, state::EpisodeMediaMetadata>),
    /// Episode thumbnail image downloaded and decoded (anime_id, episode_num, metadata_generation, source_url, image)
    EpisodeThumbnailReady(i64, u32, u64, String, image::DynamicImage),
    /// Show's 16:9 banner image downloaded and decoded (anime_id, image)
    AnimeBannerReady(i64, image::DynamicImage),
    /// Live playback progress updated via mpv IPC (anime_id, episode, pos_secs, duration_secs)
    PlaybackProgressUpdated(i64, u32, f64, f64),
    /// Episode reached 85% completion threshold and was auto-scrobbled
    EpisodeAutoScrobbled(i64, u32),
    /// Filler classifications loaded from AnimeFillerList (anime_id, map of ep -> filler_type)
    AnimeFillersReady(i64, std::collections::HashMap<u32, String>),
    /// Playback progress loaded from database for an anime (anime_id, map of ep -> (pos, dur))
    #[allow(dead_code)]
    AnimeProgressReady(i64, std::collections::HashMap<u32, (f64, f64)>),
    /// Offline downloaded episodes loaded from database (anime_id, map of ep -> file_path)
    #[allow(dead_code)]
    AnimeOfflineReady(i64, std::collections::HashMap<u32, String>),
    /// Background download progress update (anime_id, episode, percentage, speed_str)
    DownloadProgress(i64, u32, f32, String),
    /// Background episode download finished (anime_id, episode, file_path)
    DownloadComplete(i64, u32, String),
    /// Background episode download failed (anime_id, episode, error)
    DownloadFailed(i64, u32, String),
    /// MyAnimeList ID resolved for an anime (anime_id, mal_id)
    AnimeMalIdReady(i64, i64),
    /// Viewing telemetry and taste analytics loaded from local SQLite
    AnalyticsDataReady(db::user::UserAnalytics),
    /// Background update check completed (Option<UpdateInfo>)
    UpdateCheckResult(Option<services::updater::UpdateInfo>),
    /// Background update download progress (percentage, speed_str)
    UpdateProgress(f32, String),
    /// Background update successfully applied to disk (new_version)
    UpdateSuccess(String),
    /// Background update failed (error message)
    UpdateFailed(String),
    /// Remote touch companion requested switching to an episode
    RemotePlayEpisode(u32),
    /// Remote touch companion requested switching between separate /sub and /dub streams
    RemoteSwitchAudioStream(bool),
    /// Live mpv IPC audio track list update
    AudioTracksUpdated(Vec<services::remote::RemoteAudioTrack>),
    /// Remote touch companion toggled or selected Anime4K shader mode
    RemoteAnime4K(config::Anime4KMode),
    /// Background `N+1` queue prefetch completed
    QueuePrefetchReady {
        uid:          u64,
        spec_version: u64,
        prefetch:     Option<services::queue::ResolvedPrefetch>,
    },
    /// Active playback entered final ~90s window — warm next queued item Just-In-Time
    NearEofPrefetch(i64, u32),
    /// Mobile companion tapped Skip Next in Queue drawer
    RemoteQueueSkip,
    /// Mobile companion tapped a specific queued item index to play now
    RemoteQueuePlayIndex(usize),
    /// Mobile companion toggled Queue Auto-Advance
    RemoteQueueToggleAuto,
    /// Mobile companion changed live subtitle delay (milliseconds)
    RemoteSubDelayChanged(i32),
    /// Mobile companion changed subtitle track selection (or disabled subtitles)
    RemoteSubtitleSelectionChanged(api::subtitles::SubtitleSelection),
    /// Real-time mpv buffer & stall telemetry sample
    PlaybackBufferTelemetry(services::ipc::BufferTelemetryUpdate),
    /// Pre-resolved HLS variant ladder cached for instant Auto-ABR hot-switching
    StreamLadderResolved {
        anime_id: i64,
        episode:  u32,
        is_dub:   bool,
        ladder:   api::subtitles::ResolvedStreamLadder,
    },
    /// On-demand stream ladder re-fetch completed for Auto-ABR retry
    AbrRefreshReady {
        anime_id: i64,
        episode:  u32,
        is_dub:   bool,
        info:     api::subtitles::StreamInfo,
    },
    /// Mobile companion changed the active adaptive network profile
    RemoteNetworkModeChanged(String),
    /// Mobile companion toggled Auto-ABR downshift
    RemoteAutoAbrToggled(bool),
    /// Mobile companion updated theme, character art mode, or portrait slot
    RemoteThemeChanged {
        theme_id:  Option<String>,
        art_mode:  Option<String>,
        art_index: Option<usize>,
    },
    /// Mobile companion WebSocket active connection count updated
    RemoteClientsChanged(usize),
    /// User pressed n/N or clicked NEXT EPISODE inside libmpv player
    NextEpisodeRequested,
    /// User pressed [y] or clicked AUTO-SYNC VOICE inside libmpv player
    AutoSyncSubRequested {
        time_pos_secs: f64,
    },
    /// Background Speech-VAD audio cross-correlation finished
    AutoSyncSubCompleted {
        anime_id:    i64,
        provider_id: String,
        result:      services::subsync::SpeechVadSyncResult,
    },
    /// Async episode or audio stream switch resolution completed
    StreamSwitchReady {
        switch_gen:       u64,
        anime_id:         i64,
        episode:          u32,
        is_dub:           bool,
        resume_pos:       Option<f64>,
        prev_provider_id: Option<String>,
        stream_info:      api::subtitles::StreamInfo,
    },
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[cfg(windows)]
fn detect_windows_terminal_font_size() -> (u16, u16) {
    #[repr(C)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct COORD {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    struct CONSOLE_FONT_INFOEX {
        cb_size: u32,
        n_font: u32,
        dw_font_size: COORD,
        font_family: u32,
        font_weight: u32,
        face_name: [u16; 32],
    }
    extern "system" {
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
        fn GetClientRect(hwnd: *mut std::ffi::c_void, lp_rect: *mut RECT) -> i32;
        fn GetStdHandle(n_std_handle: u32) -> *mut std::ffi::c_void;
        fn GetCurrentConsoleFontEx(
            h_console_output: *mut std::ffi::c_void,
            b_maximum_window: i32,
            lp_console_current_font_ex: *mut CONSOLE_FONT_INFOEX,
        ) -> i32;
    }

    // 1. Calculate from Console Window Client Rect and terminal size
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        if cols > 0 && rows > 0 {
            unsafe {
                let hwnd = GetConsoleWindow();
                if !hwnd.is_null() {
                    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    if GetClientRect(hwnd, &mut rect) != 0 {
                        let w = (rect.right - rect.left).max(0) as u32;
                        let h = (rect.bottom - rect.top).max(0) as u32;
                        let fw = (w / cols as u32) as u16;
                        let fh = (h / rows as u32) as u16;
                        if (6..=36).contains(&fw) && (12..=72).contains(&fh) {
                            return (fw, fh);
                        }
                    }
                }
            }
        }
    }

    // 2. Query console font info
    unsafe {
        let h_stdout = GetStdHandle(0xfffffff5); // STD_OUTPUT_HANDLE = -11
        if !h_stdout.is_null() && h_stdout as isize != -1 {
            let mut cfi: CONSOLE_FONT_INFOEX = std::mem::zeroed();
            cfi.cb_size = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
            if GetCurrentConsoleFontEx(h_stdout, 0, &mut cfi) != 0 {
                let fw = cfi.dw_font_size.x.max(0) as u16;
                let fh = cfi.dw_font_size.y.max(0) as u16;
                if (6..=36).contains(&fw) && (12..=72).contains(&fh) {
                    return (fw, fh);
                }
            }
        }
    }

    // 3. Fallback based on known terminal emulators on Windows
    if std::env::var("WEZTERM_EXECUTABLE").is_ok()
        || std::env::var("TERM_PROGRAM").map(|t| t.contains("WezTerm")).unwrap_or(false)
    {
        return (14, 28);
    }
    if std::env::var("WT_SESSION").is_ok() {
        return (12, 24);
    }

    (11, 22)
}

fn parse_cli_profile_arg(args: &[String]) -> Option<String> {
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--profile" || arg == "-p" {
            if let Some(val) = args.get(i + 1) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        } else if let Some(val) = arg.strip_prefix("--profile=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        i += 1;
    }
    None
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Config + DB ───────────────────────────────────────────────────────────
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let cli_profile = parse_cli_profile_arg(&cli_args);
    let mut cfg = config::Config::load().context("Failed to load config")?;
    let db_path = config::Config::db_path().context("Failed to resolve DB path")?;
    let pool    = db::init(db_path.to_str().unwrap_or(":memory:"))
        .await
        .context("Failed to initialise database")?;

    let boot_now = unix_now();
    if let Some(requested) = cli_profile {
        let target_id = if let Some(existing_id) =
            db::user::resolve_profile_id_by_name_or_id(&pool, &requested).await
        {
            existing_id
        } else {
            db::user::create_profile(&pool, &requested, "◈", 0, "sub", boot_now)
                .await
                .map(|p| p.id)
                .unwrap_or_else(|_| "default".to_string())
        };
        let _ = db::user::set_active_profile_id(&pool, &target_id, boot_now).await;
        cfg.active_profile = target_id;
        let _ = cfg.save();
    } else if !cfg.active_profile.trim().is_empty() && cfg.active_profile != "default" {
        if let Some(pid) =
            db::user::resolve_profile_id_by_name_or_id(&pool, &cfg.active_profile).await
        {
            let _ = db::user::set_active_profile_id(&pool, &pid, boot_now).await;
        }
    }

    let active_profile = db::user::get_active_profile(&pool).await.unwrap_or(
        db::user::UserProfile {
            id: "default".to_string(),
            display_name: "Default".to_string(),
            avatar_icon: "◈".to_string(),
            theme_index: 0,
            audio_mode: "sub".to_string(),
            created_at: boot_now,
            last_active_at: boot_now,
        },
    );
    cfg.active_profile = active_profile.id.clone();
    cfg.audio_mode = if active_profile.audio_mode.eq_ignore_ascii_case("dub") {
        config::AudioMode::Dub
    } else {
        config::AudioMode::Sub
    };

    // ── Terminal setup ────────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend      = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Drain any leftover input events from shell launch (e.g. Enter pressed when typing `solus`)
    // or terminal emulator query replies (e.g. WezTerm/ConPTY DA1 queries on Windows).
    while event::poll(Duration::from_millis(50)).unwrap_or(false) {
        let _ = event::read();
    }

    // ── Message channel (background tasks → UI loop) ──────────────────────────
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AppMessage>(256);

    // Clean up any leftover update artifacts from previous update sessions
    services::updater::cleanup_legacy_update_files();

    // ── App state ─────────────────────────────────────────────────────────────
    let mut state     = AppState::new();
    state.active_profile_id = active_profile.id.clone();
    state.active_profile_name = active_profile.display_name.clone();
    state.active_profile_icon = active_profile.avatar_icon.clone();
    state.theme_index = (active_profile.theme_index.max(0) as usize) % ui::theme::THEMES.len();
    state.auth = config::AuthConfig::load_for_profile(&active_profile.id);
    state.anime4k_mode = cfg.anime4k_mode;
    state.performance_mode = cfg.performance_mode;
    state.deband_mode = cfg.deband_mode;
    state.motion_mode = cfg.motion_mode;
    state.ambilight_enabled = cfg.ambilight_enabled;
    state.network_mode = cfg.network_mode;
    state.force_ipv4 = cfg.force_ipv4;
    state.auto_abr_downshift = cfg.auto_abr_downshift;
    state.auto_fullscreen = cfg.auto_fullscreen;
    state.auto_pip = cfg.auto_pip;
    set_global_network_flags(cfg.network_mode, cfg.force_ipv4);
    state.playback_player_cursor = state.playback_players.iter().position(|p| *p == cfg.player).unwrap_or(0);
    if let Some(pb_settings) = db::user::get_profile_playback_settings(&pool, &state.active_profile_id).await {
        state.subtitle_delay_ms = pb_settings.subtitle_delay_ms.clamp(-30_000, 30_000);
        state.preferred_subtitle_code = pb_settings.subtitle_code;
    }
    state.queue = services::queue::load_active_queue(&state.active_profile_id, boot_now as u64);
    {
        let mut st = state.remote_state.write().await;
        st.active_profile = state.active_profile_name.clone();
        st.profile_icon = state.active_profile_icon.clone();
        st.sub_delay = state.subtitle_delay_secs();
        st.queue_auto_advance = state.queue.auto_advance;
        st.queue_loop = state.queue.loop_mode;
        st.queue = build_remote_queue_snapshot(&state.queue, boot_now as u64);
        st.network_mode = cfg.network_mode.as_str().to_string();
        st.network_tag = cfg.network_mode.short_tag().to_string();
        st.auto_abr = cfg.auto_abr_downshift;
        st.force_ipv4 = cfg.force_ipv4;
        st.remote_theme = cfg.remote.theme_id.clone();
        st.remote_art_mode = cfg.remote.art_mode.clone();
        st.remote_art_index = cfg.remote.art_index.unwrap_or_else(|| {
            services::remote::find_remote_theme(&cfg.remote.theme_id).default_art_idx
        });
    }

    // ── Instant Stale-While-Revalidate startup: load cached catalog (<15ms) ────
    let cached_home = services::sync::load_cached_home_data(&pool)
        .await
        .unwrap_or_else(|_| ui::home::HomeData::empty());
    let has_cached = !cached_home.is_empty();
    let mut home_data = cached_home;
    // Detect runtime dependencies (ani-cli / mpv / vlc / iina) asynchronously so Windows PATH scans never block boot.
    spawn_dependency_check(&tx, true);

    // ── Local LAN Mobile Remote Companion ─────────────────────────────────────
    let remote_shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if cfg.remote.enabled {
        #[cfg(unix)]
        let default_sock = "/tmp/solus-mpv.sock".to_string();
        #[cfg(windows)]
        let default_sock = r"\\.\pipe\solus-mpv".to_string();
        #[cfg(not(any(unix, windows)))]
        let default_sock = "solus-mpv.sock".to_string();

        services::remote::spawn_remote_server(
            cfg.remote.port,
            state.remote_state.clone(),
            default_sock,
            remote_shutdown.clone(),
            Some(tx.clone()),
        );
    }

    // ── Image picker (after alternate screen, before event loop) ──────────────
    // guess_protocol() probes the terminal — must happen after EnterAlternateScreen.
    #[cfg(unix)]
    let mut picker = ratatui_image::picker::Picker::from_termios()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::new((10, 20)));
    #[cfg(windows)]
    let mut picker = ratatui_image::picker::Picker::new(detect_windows_terminal_font_size());
    #[cfg(not(any(unix, windows)))]
    let mut picker = ratatui_image::picker::Picker::new((10, 20));

    picker.guess_protocol();
    let in_multiplexer = std::env::var_os("ZELLIJ").is_some()
        || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
        || std::env::var_os("TMUX").is_some();
    if !in_multiplexer {
        if std::env::var("KITTY_WINDOW_ID").is_ok()
            || std::env::var("TERM").map(|t| t.contains("kitty")).unwrap_or(false)
        {
            picker.protocol_type = ratatui_image::picker::ProtocolType::Kitty;
        }
        if std::env::var("WEZTERM_EXECUTABLE").is_ok()
            || std::env::var("TERM_PROGRAM").map(|t| t.contains("WezTerm")).unwrap_or(false)
        {
            picker.protocol_type = ratatui_image::picker::ProtocolType::Iterm2;
        }
    }
    state.picker = Some(picker);
    if has_cached {
        start_home_preload(&home_data, &mut state, &pool, &tx);
    }

    // ── Startup: kick off background sync ─────────────────────────────────────
    {
        let pool2 = pool.clone();
        let cfg2  = cfg.clone();
        let tx2   = tx.clone();
        let auth2 = state.auth.clone();
        tokio::spawn(async move {
            let client = api::anilist::AniListClient::with_token(auth2.anilist_token());
            let mal_cid = cfg2.trackers.resolved_mal_client_id();
            let now    = unix_now();
            let result = services::sync::sync_all(
                &pool2,
                &client,
                Some(&auth2),
                mal_cid.as_deref(),
                cfg2.cache.trending_ttl,
                cfg2.cache.stable_ttl,
                now,
            )
            .await;
            match result {
                Ok(data) => { let _ = tx2.send(AppMessage::HomeData(Box::new(data))).await; }
                Err(e)   => {
                    eprintln!("Sync error: {e}");
                    let _ = tx2.send(AppMessage::HomeData(Box::new(ui::home::HomeData::empty()))).await;
                }
            }
        });
    }

    // ── Startup: non-blocking background auto-update check ────────────────────
    if cfg.update.check_on_startup {
        spawn_update_check(&pool, &tx, cfg.update.auto_install, cfg.update.check_interval, true);
    }

    // ── Startup: one-time Post-Update "What's New & Fixed" welcome popup ─────
    if let Some((ver_label, notes)) = services::updater::check_or_take_post_update_welcome() {
        state.open_post_update_welcome(ver_label, notes);
    }

    let mut needs_redraw = true;
    let mut last_screen = state.screen.clone();
    let mut last_update_poll = std::time::Instant::now();

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        // Drain background messages
        while let Ok(msg) = rx.try_recv() {
            needs_redraw = true;
            match msg {
                AppMessage::HomeData(data) => {
                    let had_home_data = !home_data.is_empty();
                    home_data        = *data;
                    if state.is_loading || !had_home_data {
                        start_home_preload(&home_data, &mut state, &pool, &tx);
                    } else {
                        refresh_home_cover(&mut state, &home_data, &pool, &tx);
                        prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                    }
                }
                AppMessage::PlaybackLog(line) => {
                    let trimmed = line.trim();
                    if trimmed == "__SOLUS_AUDIO_CYCLE__" {
                        cycle_active_playback_audio(&mut state, &pool, &cfg, &tx).await;
                    } else if trimmed == "__SOLUS_NEXT_EPISODE__" {
                        if let Some(ref anime) = state.selected_anime.clone() {
                            if state.is_switching_episode
                                || state
                                    .last_next_ep_at
                                    .is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
                            {
                                continue;
                            }
                            let cur_ep = state.selected_episode.unwrap_or(1);
                            let next_ep = cur_ep + 1;
                            let max_aired = anime.aired_episodes();
                            if max_aired == 0 || next_ep <= max_aired {
                                launch_remote_stream_switch(
                                    &mut state,
                                    &pool,
                                    &cfg,
                                    &tx,
                                    next_ep,
                                    None,
                                    None,
                                )
                                .await;
                            } else {
                                state.show_toast(
                                    format!("Reached latest aired Episode ({})", max_aired),
                                    unix_now(),
                                );
                            }
                        }
                    } else {
                        state.push_log(line);
                    }
                }
                AppMessage::NextEpisodeRequested => {
                    if let Some(ref anime) = state.selected_anime.clone() {
                        if state.is_switching_episode
                            || state
                                .last_next_ep_at
                                .is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
                        {
                            continue;
                        }
                        let cur_ep = state.selected_episode.unwrap_or(1);
                        let next_ep = cur_ep + 1;
                        let max_aired = anime.aired_episodes();
                        if max_aired == 0 || next_ep <= max_aired {
                            launch_remote_stream_switch(
                                &mut state,
                                &pool,
                                &cfg,
                                &tx,
                                next_ep,
                                None,
                                None,
                            )
                            .await;
                        } else {
                            state.show_toast(
                                format!("Reached latest aired Episode ({})", max_aired),
                                unix_now(),
                            );
                        }
                    }
                }
                AppMessage::AutoSyncSubRequested { time_pos_secs } => {
                    if state.sub_sync_in_progress {
                        continue;
                    }
                    let Some(stream_url) = state.active_stream_url_for_sync.clone() else {
                        services::ipc::send_sub_sync_result(
                            false,
                            None,
                            0,
                            "No active stream URL for Speech-VAD sync",
                        );
                        continue;
                    };
                    let Some(sub_path) = state.active_sub_path.clone() else {
                        services::ipc::send_sub_sync_result(
                            false,
                            None,
                            0,
                            "Voice-Sync requires an active subtitle track",
                        );
                        continue;
                    };
                    let anime_id = state.selected_anime.as_ref().map(|a| a.id).unwrap_or(0);
                    let episode_num = state
                        .last_play_options
                        .as_ref()
                        .map(|o| o.episode)
                        .unwrap_or(1);
                    let sub_code = state
                        .last_play_options
                        .as_ref()
                        .and_then(|o| o.sub_lang.clone())
                        .unwrap_or_else(|| "sub".to_string());
                    let provider_id = state.active_provider_id.clone().unwrap_or_default();
                    let referer = state.active_stream_referer.clone();
                    let ref_sync = state.active_ref_sync_source.clone();
                    let is_dub_audio = state
                        .last_play_options
                        .as_ref()
                        .map(|o| o.dub)
                        .unwrap_or(state.pending_dub);
                    let target_lang = if is_dub_audio { Some("eng") } else { Some("jpn") };
                    let force_ipv4 = cfg.force_ipv4;
                    state.sub_sync_in_progress = true;
                    state.show_toast("◈ Running Reference-Audio & Speech-VAD v4 alignment...", unix_now());
                    let tx_sync = tx.clone();
                    tokio::spawn(async move {
                        let client = api::subtitles::build_http_client(force_ipv4, 8);
                        let prov_hint = if provider_id.is_empty() { None } else { Some(provider_id.as_str()) };
                        let local_sub = if sub_path.starts_with("http://") || sub_path.starts_with("https://") {
                            let (cached_path, _) = services::subsync::ensure_synchronized_subtitle_for_provider(
                                &client,
                                &sub_path,
                                None,
                                None,
                                anime_id,
                                episode_num,
                                &sub_code,
                                prov_hint,
                            )
                            .await;
                            cached_path
                        } else {
                            sub_path
                        };
                        let (ref_url, ref_ref, ref_sub) = match ref_sync {
                            Some((u, r, s)) => (Some(u), r, s),
                            None => (None, None, None),
                        };
                        let result = services::subsync::sync_subtitle_with_reference_or_vad_for_lang(
                            &client,
                            &stream_url,
                            referer.as_deref(),
                            &local_sub,
                            time_pos_secs,
                            ref_url.as_deref(),
                            ref_ref.as_deref(),
                            ref_sub.as_deref(),
                            target_lang,
                        )
                        .await;
                        let _ = tx_sync
                            .send(AppMessage::AutoSyncSubCompleted {
                                anime_id,
                                provider_id,
                                result,
                            })
                            .await;
                    });
                }
                AppMessage::AutoSyncSubCompleted { anime_id, provider_id, result } => {
                    state.sub_sync_in_progress = false;
                    if result.applied {
                        let delay_ms = (result.offset_seconds * 1000.0).round() as i32;
                        let clamped_ms = delay_ms.clamp(-150_000, 150_000);
                        let delay_secs = clamped_ms as f64 / 1000.0;
                        if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                            state.subtitle_delay_ms = clamped_ms;
                            if let Some(ref mut opts) = state.last_play_options {
                                opts.sub_delay_ms = clamped_ms;
                            }
                        }
                        let safe_to_persist = clamped_ms.abs() <= 25_000
                            || result.reason.contains("Reference-Audio")
                            || result.z_score >= 3.6;
                        if safe_to_persist {
                            let pool2 = pool.clone();
                            let pid = state.active_profile_id.clone();
                            let prov_id = provider_id.clone();
                            let now = unix_now();
                            tokio::spawn(async move {
                                let _ = db::user::save_anime_provider_subtitle_delay_ms(
                                    &pool2,
                                    &pid,
                                    anime_id,
                                    &prov_id,
                                    clamped_ms,
                                    now,
                                )
                                .await;
                            });
                        }
                        let r_state = state.remote_state.clone();
                        tokio::spawn(async move {
                            {
                                let mut st = r_state.write().await;
                                st.sub_delay = delay_secs;
                            }
                            services::remote::broadcast_state(&r_state).await;
                        });
                        let msg = format!(
                            "Voice-Locked {:+.2}s ({}% confidence)",
                            delay_secs, result.confidence_pct
                        );
                        services::ipc::send_sub_sync_result(
                            true,
                            Some(delay_secs),
                            result.confidence_pct,
                            &msg,
                        );
                        state.show_toast(format!("◈ {}", msg), unix_now());
                    } else {
                        services::ipc::send_sub_sync_result(
                            false,
                            None,
                            result.confidence_pct,
                            &result.reason,
                        );
                        state.show_toast(format!("◈ {}", result.reason), unix_now());
                    }
                }
                AppMessage::StreamSwitchReady {
                    switch_gen,
                    anime_id,
                    episode,
                    is_dub,
                    resume_pos,
                    prev_provider_id,
                    mut stream_info,
                } => {
                    if switch_gen != state.switch_generation
                        || state.selected_anime.as_ref().is_none_or(|a| a.id != anime_id)
                    {
                        continue;
                    }
                    state.is_switching_episode = false;

                    let Some(anime) = state.selected_anime.clone() else {
                        continue;
                    };
                    let title = anime.display_title().to_string();
                    let season_year = anime.season_year;
                    let prev = state.last_play_options.clone();
                    let query = prev
                        .as_ref()
                        .map(|o| o.title.clone())
                        .unwrap_or_else(|| anime.playback_query());
                    let player = prev
                        .as_ref()
                        .map(|o| o.player.clone())
                        .unwrap_or_else(|| state.selected_player().as_str().to_string());
                    let quality = prev
                        .as_ref()
                        .map(|o| o.quality.clone())
                        .unwrap_or_else(|| cfg.quality.as_str().to_string());
                    let pref_sub_lang = if prev.as_ref().is_some_and(|o| o.subtitles_disabled) {
                        "none".to_string()
                    } else if !state.preferred_subtitle_code.trim().is_empty() {
                        state.preferred_subtitle_code.clone()
                    } else {
                        prev.as_ref()
                            .and_then(|o| o.sub_lang.clone())
                            .unwrap_or_else(|| {
                                if is_dub {
                                    "none".to_string()
                                } else {
                                    cfg.default_sub_lang.clone()
                                }
                            })
                    };
                    let was_direct_stream = prev
                        .as_ref()
                        .map(|o| o.stream_url.is_some())
                        .unwrap_or(true);

                    if let Some(ref ladder) = stream_info.ladder {
                        state
                            .stream_ladder_cache
                            .insert((anime.id, episode, is_dub), ladder.clone());
                    }
                    api::subtitles::SubtitleOption::ensure_none_option_first(
                        &mut stream_info.subtitles,
                    );
                    state.playback_qualities = stream_info.qualities.clone();
                    state.playback_sub_languages = stream_info.subtitles.clone();
                    state.playback_audio_tracks = stream_info.audio_tracks.clone();
                    state.playback_providers = stream_info.providers.clone();
                    state.dub_available = Some(stream_info.dub_available || is_dub);
                    state.pending_master_url = stream_info.master_url.clone();

                    // Restore matching provider (or first verified fallback) + its Referer header
                    state.playback_provider_cursor = prev_provider_id
                        .as_deref()
                        .and_then(|pid| state.playback_providers.iter().position(|p| p.id == pid))
                        .unwrap_or(0);
                    let chosen_prov = state
                        .playback_providers
                        .get(state.playback_provider_cursor)
                        .cloned();
                    let prov_referer = chosen_prov.as_ref().and_then(|p| p.referer.clone());

                    let mut stream_url = None;
                    let mut chosen_audio_urls = Vec::new();
                    let mut chosen_has_demuxed = false;
                    let mut needs_hls_proxy = chosen_prov.as_ref().is_some_and(|p| p.needs_hls_proxy);
                    if was_direct_stream {
                        let q_pool = chosen_prov
                            .as_ref()
                            .filter(|p| !p.qualities.is_empty())
                            .map(|p| p.qualities.as_slice())
                            .unwrap_or(stream_info.qualities.as_slice());
                        let chosen_q = q_pool
                            .iter()
                            .find(|q| q.label.eq_ignore_ascii_case(&quality))
                            .or_else(|| q_pool.first());
                        if let Some(q) = chosen_q {
                            chosen_audio_urls = q.audio_urls.clone();
                            chosen_has_demuxed = q.has_demuxed_audio;
                            needs_hls_proxy = needs_hls_proxy || q.needs_hls_proxy;
                        }
                        stream_url = chosen_q
                            .map(|q| q.url.clone())
                            .filter(|u| !u.is_empty())
                            .or_else(|| {
                                chosen_prov
                                    .as_ref()
                                    .and_then(|p| p.master_url.clone())
                                    .or_else(|| stream_info.master_url.clone())
                            });
                    }

                    let chosen_pid = chosen_prov.as_ref().map(|p| p.id.clone()).unwrap_or_default();
                    state.active_provider_id = (!chosen_pid.is_empty()).then_some(chosen_pid.clone());
                    if let Some(saved_prov_delay) = db::user::get_anime_provider_subtitle_delay_ms(
                        &pool,
                        &state.active_profile_id,
                        anime.id,
                        &chosen_pid,
                    )
                    .await
                    {
                        state.subtitle_delay_ms = saved_prov_delay;
                    } else if !chosen_pid.is_empty() {
                        state.subtitle_delay_ms = 0;
                    }

                    if stream_url.is_none() && was_direct_stream {
                        state.show_toast(
                            format!("No playable source verified for Episode {}", episode),
                            unix_now(),
                        );
                        continue;
                    }

                    let (chosen_idx, used_eng_fallback) =
                        api::subtitles::SubtitleOption::select_index_with_english_fallback(
                            &state.playback_sub_languages,
                            &pref_sub_lang,
                        );
                    state.playback_sub_cursor = chosen_idx;
                    let chosen_sub = state.playback_sub_languages.get(chosen_idx).cloned();
                    let subtitles_disabled = chosen_sub.as_ref().is_some_and(|s| s.is_none());

                    if used_eng_fallback {
                        state.show_toast(
                            format!(
                                "❖ Subtitle fallback: English (preferred unavailable on Ep {})",
                                episode
                            ),
                            unix_now(),
                        );
                    }

                    let sub_lang = if subtitles_disabled {
                        Some("none".to_string())
                    } else {
                        chosen_sub
                            .as_ref()
                            .map(|s| s.lang_keyword().to_string())
                            .or(Some(pref_sub_lang))
                    };
                    let sub_url = if subtitles_disabled {
                        None
                    } else {
                        chosen_sub
                            .as_ref()
                            .map(|s| s.url.clone())
                            .filter(|u| !u.is_empty())
                    };

                    let caps = services::platform::detect_player_capabilities();
                    let glsl_shaders = services::shaders::build_shader_arg_for_profile(
                        state.anime4k_mode,
                        cfg.performance_mode,
                        season_year,
                        caps,
                    );

                    let mut opts = api::player::PlayOptions {
                        title: query,
                        episode,
                        quality,
                        dub: is_dub,
                        player,
                        sub_lang,
                        sub_url,
                        stream_url,
                        ipc_socket: None,
                        start_pos: resume_pos,
                        glsl_shaders,
                        sub_style: Some(cfg.subtitle_style.clone()),
                        sub_delay_ms: state.subtitle_delay_ms,
                        subtitles_disabled,
                        loading_image: None,
                        network_mode: state.network_mode,
                        force_ipv4: cfg.force_ipv4,
                        auto_fullscreen: cfg.auto_fullscreen,
                        auto_pip: cfg.auto_pip,
                        shader_mode: state.anime4k_mode,
                        performance_mode: cfg.performance_mode,
                        deband_mode: cfg.deband_mode,
                        motion_mode: cfg.motion_mode,
                        referer: prov_referer,
                        audio_urls: chosen_audio_urls,
                        has_demuxed_audio: chosen_has_demuxed,
                        embedded_sub: chosen_prov
                            .as_ref()
                            .map(|p| p.embedded_sub)
                            .unwrap_or_default(),
                        hardsub_shield: state.hardsub_shield_mode,
                        hardsub_band: state.hardsub_band_height,
                    };
                    wrap_opts_with_hls_proxy_if_needed(&mut state, &cfg, &mut opts, needs_hls_proxy).await;
                    start_playback(&mut state, opts, title, episode, &tx, &pool, &cfg).await;
                }
                AppMessage::PlayerSubDelayChanged { anime_id, delay_ms } => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                        && (state.subtitle_delay_ms - delay_ms).abs() >= 10
                    {
                        state.subtitle_delay_ms = delay_ms;
                        if let Some(ref mut opts) = state.last_play_options {
                            opts.sub_delay_ms = delay_ms;
                        }
                        let pool2 = pool.clone();
                        let pid = state.active_profile_id.clone();
                        let prov_id = state.active_provider_id.clone().unwrap_or_default();
                        let now = unix_now();
                        tokio::spawn(async move {
                            let _ = db::user::save_anime_provider_subtitle_delay_ms(
                                &pool2,
                                &pid,
                                anime_id,
                                &prov_id,
                                delay_ms,
                                now,
                            )
                            .await;
                        });
                        let delay_secs = state.subtitle_delay_secs();
                        let r_state = state.remote_state.clone();
                        tokio::spawn(async move {
                            {
                                let mut st = r_state.write().await;
                                st.sub_delay = delay_secs;
                            }
                            services::remote::broadcast_state(&r_state).await;
                        });
                    }
                }
                AppMessage::PlaybackDone {
                    gen,
                    exited_ok,
                    natural_eof,
                    user_quit,
                    elapsed_secs,
                } => {
                    // Ignore completion events from superseded players (episode switches)
                    if gen != state.playback_gen {
                        continue;
                    }
                    let active_snapshot = state.queue.active_item.clone();
                    state.now_playing = None;
                    state.player_stop = None;
                    state.sub_sync_in_progress = false;
                    state.active_stream_url_for_sync = None;
                    state.active_stream_referer = None;
                    state.active_sub_path = None;
                    state.active_ref_sync_source = None;
                    state.live_playback.clear();
                    state.invalidate_detail_image_protocols();
                    services::ipc::set_playback_paused_for_cache(false);
                    services::ipc::set_active_ipc_sender(None);
                    services::ipc::set_active_socket_path(None);
                    let remote_state_done = state.remote_state.clone();
                    tokio::spawn(async move {
                        {
                            let mut st = remote_state_done.write().await;
                            st.is_playing = false;
                            st.paused_for_cache = false;
                            st.buffering_pct = 0;
                            st.buffer_secs = 0.0;
                            st.cache_mb = 0.0;
                            st.cache_speed_kbps = 0.0;
                        }
                        services::remote::broadcast_state(&remote_state_done).await;
                    });
                    #[cfg(unix)]
                    let _ = std::fs::remove_file("/tmp/solus-mpv.sock");

                    // Self-healing retry once on verified backup mirror (e.g. AnimeHeaven Mirror 2/3)
                    // if a direct HTTP MP4 failed immediately on startup.
                    if !exited_ok && !user_quit && !natural_eof && elapsed_secs < 12 {
                        if let Some(mut retry_opts) = state.last_play_options.clone() {
                            if let Some(ref cur_url) = retry_opts.stream_url {
                                if let Some(cur_idx) = state
                                    .playback_qualities
                                    .iter()
                                    .position(|q| q.url == *cur_url)
                                {
                                    if let Some(next_mirror) = state
                                        .playback_qualities
                                        .get(cur_idx + 1)
                                        .filter(|q| q.label.contains("Mirror") && !q.url.is_empty())
                                    {
                                        retry_opts.stream_url = Some(next_mirror.url.clone());
                                        retry_opts.quality = next_mirror.label.clone();
                                        state.show_toast(
                                            format!("◈ Switching to backup CDN ({})...", next_mirror.label),
                                            unix_now(),
                                        );
                                        let retry_title = retry_opts.title.clone();
                                        let retry_ep = retry_opts.episode;
                                        start_playback(&mut state, retry_opts, retry_title, retry_ep, &tx, &pool, &cfg).await;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Self-healing retry once if a prefetched CDN URL failed quickly on launch
                    if !exited_ok && !user_quit && elapsed_secs < 20 {
                        if let Some(mut active) = active_snapshot
                            .as_ref()
                            .filter(|a| a.used_prefetched && !a.retried_once)
                            .cloned()
                        {
                            active.item.bump_spec();
                            state.show_toast(
                                format!(
                                    "❖ Cached link expired — re-resolving {} Ep {}...",
                                    active.item.title, active.item.episode
                                ),
                                unix_now(),
                            );
                            play_queue_item(&mut state, active.item, false, true, &pool, &cfg, &tx).await;
                            continue;
                        }
                    }

                    // Auto-advance next queued item ONLY on true natural EOF or >=90% completion.
                    // Never jump to the next episode if the player crashed or was closed early.
                    let active_completion_reached = active_snapshot
                        .as_ref()
                        .is_some_and(|a| {
                            a.completion_reached
                                || a.progress_ratio >= services::queue::QUEUE_COMPLETION_THRESHOLD
                        });
                    let active_ep = active_snapshot
                        .as_ref()
                        .map(|a| a.item.episode)
                        .unwrap_or(0);
                    let had_ipc_duration = state
                        .selected_anime
                        .as_ref()
                        .and_then(|_| state.anime_progress.get(&active_ep))
                        .is_some_and(|(_, dur)| *dur >= 60.0);

                    let natural_finish = natural_eof
                        || (active_completion_reached && !user_quit)
                        || (!had_ipc_duration && exited_ok && !user_quit && elapsed_secs >= 900);

                    if natural_finish {
                        state.queue.mark_active_completed();
                        if !state.queue.manual_stop && state.queue.auto_advance {
                            if let Some(next_item) = state.queue.pop_next_for_advance() {
                                if state.queue_cursor >= state.queue.len() {
                                    state.queue_cursor = state.queue.len().saturating_sub(1);
                                }
                                let label = format!("{} · Ep {}", next_item.title, next_item.episode);
                                state.show_toast(format!("◈ Up Next: {}", label), unix_now());
                                play_queue_item(&mut state, next_item, false, false, &pool, &cfg, &tx).await;
                                continue;
                            }
                        }
                        state.queue.active_item = None;
                    } else if let Some(restored) = state.queue.restore_unfinished_active_to_front() {
                        state.queue_cursor = 0;
                        let pct = active_snapshot
                            .as_ref()
                            .map(|a| (a.progress_ratio * 100.0).round() as u32)
                            .unwrap_or(0);
                        state.show_toast(
                            format!(
                                "◈ Player closed early ({}%) — {} Ep {} kept at #1 in queue to resume",
                                pct, restored.title, restored.episode
                            ),
                            unix_now(),
                        );
                    } else {
                        state.queue.active_item = None;
                    }

                    services::queue::save_active_queue(&state.active_profile_id, &state.queue);
                    sync_remote_queue_snapshot(&state);
                    if state.screen == Screen::Playback {
                        state.screen = Screen::Detail;
                    }
                    if let Some(err_msg) = state.playback_error_message() {
                        state.show_toast(err_msg.to_string(), unix_now());
                    }
                }
                AppMessage::SearchResults(results) => {
                    for anime in &results {
                        if !state.cover_cache.contains_key(&anime.id) && !state.loading_covers.contains(&anime.id) {
                            state.loading_covers.insert(anime.id);
                            trigger_cover_download(anime.clone(), pool.clone(), tx.clone());
                        }
                    }
                    state.search_results = results;
                    state.search_cursor  = 0;
                    if let Some(first) = state.search_results.first() {
                        state.cover_anime_id = Some(first.id);
                    }
                }
                AppMessage::CoverReady(anime_id, img) => {
                    state.loading_covers.remove(&anime_id);
                    state.cover_raw_cache.entry(anime_id).or_insert_with(|| img.clone());
                    let font_size = state.picker.as_ref().map(|p| p.font_size).unwrap_or((12, 24));
                    let card_img = prepare_card_cover_with_font(img.clone(), font_size);
                    let portrait_img = prepare_portrait_cover_with_font(img.clone(), font_size);
                    if state.picker.is_some() {
                        insert_cover_protocols_if_absent(&mut state, anime_id, card_img.clone(), portrait_img);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        let base_theme = ui::theme::get_theme(state.theme_index);
                        state.ambilight_theme = Some(ui::theme::extract_ambilight_palette(&card_img, base_theme));
                        if state.current_card_img.is_none() {
                            state.current_card_img = Some(prepare_16_9_thumbnail_cover(img, font_size));
                        }
                    }
                    if state.cover_anime_id == Some(anime_id) {
                        state.cover_failed_anime_id = None;
                    }
                }
                AppMessage::CoverFailed(anime_id) => {
                    state.loading_covers.remove(&anime_id);
                    if state.cover_anime_id == Some(anime_id) {
                        state.cover_failed_anime_id = Some(anime_id);
                    }
                    if state.cover_anime_id == Some(anime_id)
                        && matches!(state.screen, Screen::Detail | Screen::Home)
                    {
                        state.show_toast("Cover image unavailable", unix_now());
                    }
                }
                AppMessage::PreloadCoverReady { gen, anime_id, card_proto, banner_proto, raw_img } => {
                    state.loading_covers.remove(&anime_id);
                    state.cover_cache.entry(anime_id).or_insert(card_proto);
                    state.banner_cache.entry(anime_id).or_insert(banner_proto);
                    state.cover_raw_cache.entry(anime_id).or_insert(raw_img);
                    if state.cover_anime_id == Some(anime_id) {
                        state.cover_failed_anime_id = None;
                    }
                    if let Some(g) = gen {
                        if state.advance_preload(g) {
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                        }
                    }
                }
                AppMessage::PreloadCoverFailed { gen, anime_id } => {
                    state.loading_covers.remove(&anime_id);
                    if let Some(g) = gen {
                        if state.advance_preload(g) {
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                        }
                    }
                }
                AppMessage::DependenciesReady { ani_cli, mpv, mpvnet, celluloid, prompt_setup } => {
                    state.set_dependencies(ani_cli, mpv, mpvnet, celluloid);
                    if prompt_setup && !state.has_any_player() && state.screen != Screen::Setup {
                        state.open_setup();
                    }
                }
                AppMessage::WatchlistUpdated(list, sources, statuses) => {
                    home_data.watchlist = list;
                    home_data.watchlist_sources = sources;
                    home_data.watchlist_statuses = statuses;
                    let max_idx = home_data.watchlist.len().saturating_sub(1);
                    let offset  = state.row_offset("watchlist").min(max_idx);
                    state.row_offsets.insert("watchlist".to_string(), offset);
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                }
                AppMessage::WatchlistWritebackCompleted {
                    anime_id,
                    removed_completely,
                    added_to_wl,
                    outcome_summary,
                    wl,
                    sources,
                    statuses,
                    cw,
                } => {
                    home_data.watchlist = wl;
                    home_data.watchlist_sources = sources;
                    home_data.watchlist_statuses = statuses;
                    home_data.continue_watching = cw;
                    let max_idx = home_data.watchlist.len().saturating_sub(1);
                    let offset = state.row_offset("watchlist").min(max_idx);
                    state.row_offsets.insert("watchlist".to_string(), offset);
                    let cw_max = home_data.continue_watching.len().saturating_sub(1);
                    let cw_off = state.row_offset("continue_watching").min(cw_max);
                    state.row_offsets.insert("continue_watching".to_string(), cw_off);
                    if state.active_row == state::CategoryRow::Watchlist && home_data.watchlist.is_empty() {
                        state.active_row = next_non_empty_row(&state.active_row, &home_data, 1);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.in_watchlist = if added_to_wl { true } else { !removed_completely };
                    }
                    if added_to_wl {
                        state.calendar_tracked_ids.insert(anime_id);
                    } else if removed_completely && !home_data.continue_watching.iter().any(|a| a.id == anime_id) {
                        state.calendar_tracked_ids.remove(&anime_id);
                    }
                    state.show_toast(outcome_summary, unix_now());
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                }
                AppMessage::TrackerSyncCompleted {
                    provider,
                    account,
                    report,
                    home_data: new_home,
                    summary,
                } => {
                    state.oauth_pending = None;
                    state.oauth_cancel_tx = None;
                    *state.auth.account_mut(provider) = account;
                    let _ = state.auth.save();
                    state.merge_report = Some(report);
                    home_data = *new_home;
                    state.login_status = Some(summary.clone());
                    state.show_toast(summary, unix_now());
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                }
                AppMessage::TrackerSyncFailed(_provider, err) => {
                    state.oauth_pending = None;
                    state.oauth_cancel_tx = None;
                    state.login_status = Some(format!("! Sync error: {err}"));
                }
                AppMessage::MergeAuditReady(report) => {
                    state.merge_report = Some(report);
                }
                AppMessage::ParityPushCompleted {
                    summary,
                    report,
                    home_data: new_home,
                } => {
                    state.merge_report = Some(report);
                    home_data = *new_home;
                    state.login_status = Some(summary.clone());
                    state.show_toast(summary, unix_now());
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                }
                AppMessage::MalXmlImportCompleted(res) => {
                    match res {
                        Ok((report, filename, count, new_home)) => {
                            let now = unix_now();
                            state.auth.mal.sync_mode = Some("MAL XML Import".to_string());
                            state.auth.mal.last_imported_file = Some(filename);
                            state.auth.mal.last_sync_time = Some(now);
                            let _ = state.auth.save();
                            state.merge_report = Some(report);
                            home_data = *new_home;
                            let msg = format!("✓ Imported {count} MAL entries (0 duplicates)");
                            state.login_status = Some(msg.clone());
                            state.show_toast(msg, now);
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                            prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                        }
                        Err(err) => {
                            state.login_status = Some(format!("! MAL XML import error: {err}"));
                        }
                    }
                }
                AppMessage::ScrobbleSyncToast(msg) => {
                    state.show_toast(msg, unix_now());
                }
                AppMessage::ContinueWatchingUpdated(list) => {
                    home_data.continue_watching = list;
                    let max_idx = home_data.continue_watching.len().saturating_sub(1);
                    let offset  = state.row_offset("continue_watching").min(max_idx);
                    state.row_offsets.insert("continue_watching".to_string(), offset);
                    if state.active_row == state::CategoryRow::ContinueWatching && home_data.continue_watching.is_empty() {
                        state.active_row = next_non_empty_row(&state.active_row, &home_data, 1);
                    }
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                }
                AppMessage::BannerProgress(anime_id, watched) => {
                    if state.cover_anime_id == Some(anime_id) {
                        state.banner_progress = Some((anime_id, watched));
                    }
                }
                AppMessage::StreamInfoDetected(anime_id, ep, is_dub, req_gen, mut info) => {
                    if info.dub_available {
                        let pool_dub = pool.clone();
                        tokio::spawn(async move {
                            let _ = db::cache::mark_anime_has_dub(&pool_dub, anime_id).await;
                        });
                        if let Some(ref mut a) = state.selected_anime {
                            if a.id == anime_id {
                                a.has_dub = 1;
                            }
                        }
                    }
                    if req_gen == state.playback_request_generation
                        && state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                        && state.selected_episode == Some(ep)
                        && state.pending_dub == is_dub
                    {
                        api::subtitles::SubtitleOption::ensure_none_option_first(&mut info.subtitles);
                        state.playback_qualities = info.qualities;
                        state.playback_sub_languages = info.subtitles;
                        state.playback_audio_tracks = info.audio_tracks;
                        state.playback_providers = info.providers;
                        state.dub_available = Some(info.dub_available);
                        state.pending_master_url = info.master_url;

                        let edit_item = match state.playback_options_purpose {
                            state::PlaybackOptionsPurpose::EditQueueItem { uid } => {
                                state.queue.items.iter().find(|i| i.uid == uid).cloned()
                            }
                            _ => None,
                        };
                        let pref_str = edit_item
                            .as_ref()
                            .map(|i| i.quality.as_str())
                            .unwrap_or_else(|| cfg.quality.as_str());
                        state.playback_quality_cursor = state
                            .playback_qualities
                            .iter()
                            .position(|q| q.label.eq_ignore_ascii_case(pref_str))
                            .unwrap_or(0);

                        let pref_sub = if let Some(ref item) = edit_item {
                            item.sub_lang.clone()
                        } else if !state.preferred_subtitle_code.trim().is_empty() {
                            state.preferred_subtitle_code.clone()
                        } else if is_dub {
                            "none".to_string()
                        } else {
                            cfg.default_sub_lang.clone()
                        };
                        api::subtitles::sort_providers_for_sub_lang(&mut state.playback_providers, &pref_sub);
                        let (sub_idx, _) =
                            api::subtitles::SubtitleOption::select_index_with_english_fallback(
                                &state.playback_sub_languages,
                                &pref_sub,
                            );
                        state.playback_sub_cursor = sub_idx;

                        state.sync_active_provider();
                        let (post_sub_idx, _) =
                            api::subtitles::SubtitleOption::select_index_with_english_fallback(
                                &state.playback_sub_languages,
                                &pref_sub,
                            );
                        state.playback_sub_cursor = post_sub_idx;
                        sync_provider_delay_for_cursor(&mut state, &pool).await;
                        if let Some(ref ladder) = info.ladder {
                            state
                                .stream_ladder_cache
                                .insert((anime_id, ep, is_dub), ladder.clone());
                        }
                        state.is_loading_subs = false;

                        let r_state = state.remote_state.clone();
                        let r_subs: Vec<services::remote::RemoteSubtitleTrack> = state
                            .playback_sub_languages
                            .iter()
                            .enumerate()
                            .map(|(idx, s)| services::remote::RemoteSubtitleTrack {
                                id: (idx + 1) as u32,
                                code: s.code.clone(),
                                label: s.name.clone(),
                                is_active: idx == state.playback_sub_cursor,
                                url: if s.url.is_empty() { None } else { Some(s.url.clone()) },
                            })
                            .collect();
                        let r_audio = build_initial_remote_audio_tracks(
                            &state.playback_audio_tracks,
                            state.dub_available.unwrap_or(false) || is_dub,
                            is_dub,
                        );
                        let r_audio_lang = r_audio
                            .iter()
                            .find(|t| t.is_active)
                            .map(|t| t.label.clone())
                            .unwrap_or_else(|| {
                                if is_dub {
                                    "English (Dub)".to_string()
                                } else {
                                    "Japanese (Original / 日本語)".to_string()
                                }
                            });
                        tokio::spawn(async move {
                            {
                                let mut st = r_state.write().await;
                                if !r_subs.is_empty() {
                                    st.subtitles = r_subs;
                                }
                                st.audio_tracks = r_audio;
                                st.audio_lang = r_audio_lang;
                                st.audio_mode = if is_dub { "dub" } else { "sub" }.to_string();
                            }
                            services::remote::broadcast_state(&r_state).await;
                        });
                    }
                }
                AppMessage::AiringStatusUpdated(anime_id, status, episodes, next_airing) => {
                    if let Some(ref mut anime) = state.selected_anime {
                        if anime.id == anime_id {
                            if let Some(s) = status {
                                anime.status = Some(s);
                            }
                            if let Some(eps) = episodes {
                                anime.episodes = Some(eps);
                            }
                            anime.next_airing_episode = next_airing;

                            let total = anime.episodes.unwrap_or(0) as u32;
                            let aired = anime.aired_episodes();
                            let display_total = total.max(aired).max(1);
                            state.episode_list = (1..=display_total).collect();
                            if state.selected_episode.is_some_and(|ep| ep > aired && aired > 0) {
                                state.selected_episode = Some(state.next_unwatched_episode());
                            }
                        }
                    }
                }
                AppMessage::ArchiveImportCompleted(res) => {
                    match res {
                        Ok(stats) => {
                            let now = unix_now();
                            state.auth.anilist.sync_mode = Some("Local JSON Archive".to_string());
                            state.auth.anilist.last_sync_time = Some(now);
                            let _ = state.auth.save();
                            state.login_status = Some(format!(
                                "✓ Ingested {} anime, {} episodes",
                                stats.total_anime, stats.episodes_logged
                            ));
                            state.show_toast(format!("❖ Ingested {} anime, {} episodes", stats.total_anime, stats.episodes_logged), now);
                            state.import_stats = Some(stats);

                            if let Ok(cw) = services::sync::load_continue_watching(&pool).await {
                                home_data.continue_watching = cw;
                            }
                            if let Ok((wl, sources)) = services::sync::load_watchlist_with_sources(&pool).await {
                                home_data.watchlist = wl;
                                home_data.watchlist_sources = sources;
                            }
                            if let Ok(statuses) = db::user::get_watchlist_statuses(&pool).await {
                                home_data.watchlist_statuses = statuses;
                            }
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                            prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                        }
                        Err(err) => {
                            state.login_status = Some(format!("! Import error: {}", err));
                        }
                    }
                }
                AppMessage::PublicSyncCompleted(res) => {
                    match res {
                        Ok((viewer, stats)) => {
                            let now = unix_now();
                            state.auth.anilist.username = Some(viewer.name.clone());
                            state.auth.anilist.user_id = Some(viewer.id);
                            state.auth.anilist.avatar_url = viewer.avatar_url.clone();
                            state.auth.anilist.sync_mode = Some("Public Username".to_string());
                            state.auth.anilist.last_sync_time = Some(now);
                            let _ = state.auth.save();
                            state.login_status = Some(format!("✓ Synced AniList @{}: {} anime", viewer.name, stats.total_anime));
                            state.show_toast(format!("❖ Synced @{}: {} anime", viewer.name, stats.total_anime), now);
                            state.import_stats = Some(stats);

                            if let Ok(cw) = services::sync::load_continue_watching(&pool).await {
                                home_data.continue_watching = cw;
                            }
                            if let Ok((wl, sources)) = services::sync::load_watchlist_with_sources(&pool).await {
                                home_data.watchlist = wl;
                                home_data.watchlist_sources = sources;
                            }
                            if let Ok(statuses) = db::user::get_watchlist_statuses(&pool).await {
                                home_data.watchlist_statuses = statuses;
                            }
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                            prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                        }
                        Err(err) => {
                            state.login_status = Some(format!("! Sync error: {}", err));
                        }
                    }
                }
                AppMessage::BackupExportCompleted(res) => {
                    match res {
                        Ok(count) => {
                            let dest_str = config::Config::config_dir()
                                .map(|d| d.join("solus_backup.json").display().to_string())
                                .unwrap_or_else(|_| "solus_backup.json".to_string());
                            state.login_status = Some(format!("❖ Backup generated: {} titles exported to {}", count, dest_str));
                            state.show_toast(format!("❖ Backup created: {} anime", count), unix_now());
                        }
                        Err(err) => {
                            state.login_status = Some(format!("Export error: {}", err));
                        }
                    }
                }
                AppMessage::TailoredResultsReady(pool_matches) => {
                    state.open_tailored_results(pool_matches);
                    for m in &state.tailored_results {
                        if !state.cover_cache.contains_key(&m.anime.id) && !state.loading_covers.contains(&m.anime.id) {
                            state.loading_covers.insert(m.anime.id);
                            trigger_cover_download(m.anime.clone(), pool.clone(), tx.clone());
                        }
                    }
                    if let Some(first) = state.tailored_results.first() {
                        state.cover_anime_id = Some(first.anime.id);
                    }
                }
                AppMessage::TunerPeersReady(anchor_id, peers) => {
                    let is_same_anchor = state
                        .tuner_config
                        .anchor_anime
                        .as_ref()
                        .is_some_and(|(id, _, _)| *id == anchor_id);
                    if is_same_anchor {
                        let existing_ids: std::collections::HashSet<i64> =
                            state.tailored_pool.iter().map(|m| m.anime.id).collect();
                        for p in peers {
                            state.tuner_config.anchor_peer_ids.insert(p.id);
                            if !existing_ids.contains(&p.id) {
                                let m = services::tailored::score_anime(&p, &state.tailored_answers)
                                    .unwrap_or_else(|| services::tailored::TailoredMatch {
                                        anime: p.clone(),
                                        match_percentage: 82,
                                        reason_tags: p.genre_list().into_iter().take(2).collect(),
                                    });
                                state.tailored_pool.push(m);
                            }
                        }
                        state.recompute_tailored_results();
                        for m in state.tailored_results.iter().take(8) {
                            if !state.cover_cache.contains_key(&m.anime.id) && !state.loading_covers.contains(&m.anime.id) {
                                state.loading_covers.insert(m.anime.id);
                                trigger_cover_download(m.anime.clone(), pool.clone(), tx.clone());
                            }
                        }
                        state.show_toast("Updated with AniList community similar picks", unix_now());
                    }
                }
                AppMessage::CalendarWeekReady {
                    week_offset,
                    tz_offset_secs,
                    tracked_ids,
                    watched_map,
                    releases,
                } => {
                    state.calendar_tz_offset_secs = tz_offset_secs;
                    state.calendar_tracked_ids = tracked_ids;
                    state.calendar_watched_map = watched_map;
                    state.calendar_cache.insert(week_offset, releases);
                    if state.calendar_week_offset == week_offset {
                        state.is_loading_calendar = false;
                        if state.screen == Screen::Calendar {
                            prefetch_calendar_covers(&mut state, &pool, &tx);
                        }
                    }
                }
                AppMessage::CalendarWeekFailed(week_offset, err) => {
                    if state.calendar_week_offset == week_offset {
                        state.is_loading_calendar = false;
                        if state.screen == Screen::Calendar {
                            state.show_toast(format!("Calendar refresh failed: {}", err), unix_now());
                        }
                    }
                }
                AppMessage::EpisodeMetadataReady(anime_id, meta_gen, meta) => {
                    if meta_gen == state.episode_metadata_generation
                        && state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                    {
                        state.loading_episode_metadata = false;
                        for (ep, item) in meta {
                            let existing_generic = state
                                .episode_titles
                                .get(&ep)
                                .is_none_or(|t| t.starts_with("Episode "));
                            if !item.title.is_empty()
                                && (existing_generic || !item.title.starts_with("Episode "))
                            {
                                state.episode_titles.insert(ep, item.title.clone());
                            }
                            if let Some(ref syn) = item.synopsis {
                                state.episode_synopses.insert(ep, syn.clone());
                            }
                            if let Some(ref new_url) = item.thumbnail_url {
                                let prev_meta = state.episode_media_meta.get(&ep);
                                let prev_src = prev_meta.map(|m| m.thumbnail_source);
                                let prev_url = state.episode_thumbnail_urls.get(&ep).cloned();
                                let should_replace_url = prev_url.is_none()
                                    || prev_src.is_none_or(|src| item.thumbnail_source >= src);
                                if should_replace_url {
                                    if prev_url.as_ref() != Some(new_url) {
                                        state.episode_thumbnails.remove(&ep);
                                        state.episode_raw_thumbnails.remove(&ep);
                                        state.episode_thumbnail_states.remove(&ep);
                                        state.loading_episode_thumbnails.remove(&ep);
                                    }
                                    state.episode_thumbnail_urls.insert(ep, new_url.clone());
                                }
                            }
                            state.episode_media_meta.insert(ep, item);
                        }
                        preload_nearby_episode_thumbnails(&mut state, &tx);
                    }
                }
                AppMessage::EpisodeThumbnailReady(anime_id, ep, meta_gen, source_url, img) => {
                    if meta_gen == state.episode_metadata_generation
                        && state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                    {
                        state.loading_episode_thumbnails.remove(&ep);
                        if state.episode_thumbnail_urls.get(&ep) == Some(&source_url) {
                            state.episode_raw_thumbnails.insert(ep, img.clone());
                            state
                                .episode_thumbnail_states
                                .insert(ep, state::EpisodeThumbnailState::Ready);
                            if let Some(picker) = state.picker {
                                let prepared = if let Some((tw, th, fw, fh)) =
                                    state.thumbnail_geometry_key
                                {
                                    ui::components::cover::scale_to_frame_cover(
                                        img,
                                        tw,
                                        th,
                                        (fw, fh),
                                        0.20,
                                    )
                                } else {
                                    prepare_16_9_thumbnail_cover(img, picker.font_size)
                                };
                                let proto = build_stateful_protocol(picker, prepared);
                                state.episode_thumbnails.insert(ep, proto);
                            }
                        }
                    }
                }
                AppMessage::AnimeBannerReady(anime_id, img) => {
                    state.loading_banners.remove(&anime_id);
                    let font_size = state.picker.as_ref().map(|p| p.font_size).unwrap_or((12, 24));
                    let card_banner = prepare_card_cover_with_font(img.clone(), font_size);
                    if let Some(picker) = state.picker {
                        let proto = build_stateful_protocol(picker, card_banner.clone());
                        state.anime_banners.insert(anime_id, proto);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                        && state.current_card_img.is_none()
                    {
                        let thumb_banner = prepare_16_9_thumbnail_cover(img, font_size);
                        state.current_card_img = Some(thumb_banner);
                    }
                }
                AppMessage::PlaybackProgressUpdated(anime_id, ep, pos, dur) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.anime_progress.insert(ep, (pos, dur));
                    }
                    if state.queue.record_active_progress(anime_id, ep, pos, dur) {
                        services::queue::save_active_queue(&state.active_profile_id, &state.queue);
                        sync_remote_queue_snapshot(&state);
                    }
                    state.live_playback.last_time_pos = pos;
                    let remote_state_clone = state.remote_state.clone();
                    let title = state.selected_anime.as_ref().map(|a| a.display_title().to_string()).unwrap_or_else(|| "Anime Stream".to_string());
                    let ep_title = state.episode_titles.get(&ep).cloned().unwrap_or_else(|| format!("Episode {}", ep));
                    tokio::spawn(async move {
                        {
                            let mut st = remote_state_clone.write().await;
                            st.is_playing = true;
                            st.title = title;
                            st.episode = ep;
                            st.episode_title = ep_title;
                            st.time_pos = pos;
                            st.duration = dur;
                        }
                        services::remote::broadcast_state(&remote_state_clone).await;
                    });
                }
                AppMessage::EpisodeAutoScrobbled(anime_id, ep) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.watched_episodes.insert(ep);
                    }
                    state.show_toast(format!("󰄬 Episode {} completed — auto-scrobbled", ep), unix_now());
                    let pool_clone = pool.clone();
                    let tx_cw = tx.clone();
                    tokio::spawn(async move {
                        if let Ok(cw) = services::sync::load_continue_watching(&pool_clone).await {
                            let _ = tx_cw.send(AppMessage::ContinueWatchingUpdated(cw)).await;
                        }
                    });
                }
                AppMessage::AnimeFillersReady(anime_id, fillers) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.anime_fillers = fillers;
                    }
                }
                AppMessage::AnimeProgressReady(anime_id, progress) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.anime_progress = progress;
                    }
                }
                AppMessage::AnimeOfflineReady(anime_id, offline) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.offline_episodes = offline;
                    }
                }
                AppMessage::DownloadProgress(anime_id, ep, pct, spd) => {
                    state.active_download = Some((anime_id, ep, pct, spd));
                }
                AppMessage::DownloadComplete(anime_id, ep, path) => {
                    state.active_download = None;
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.offline_episodes.insert(ep, path);
                    }
                    state.show_toast(format!("󰄬 Episode {} downloaded offline!", ep), unix_now());
                }
                AppMessage::DownloadFailed(_anime_id, ep, err) => {
                    state.active_download = None;
                    state.show_toast(format!("Download failed for Ep {}: {}", ep, err), unix_now());
                }
                AppMessage::AnimeMalIdReady(anime_id, mal_id) => {
                    state.anime_mal_ids.insert(anime_id, mal_id);
                }
                AppMessage::AnalyticsDataReady(analytics) => {
                    let show_count = analytics.top_shows.len();
                    if show_count > 0 {
                        state.analytics_podium_idx = state.analytics_podium_idx.min(show_count - 1);
                    } else {
                        state.analytics_podium_idx = 0;
                    }
                    state.analytics_list_max_scroll = None;
                    let cover_ids: Vec<i64> = analytics
                        .top_shows
                        .iter()
                        .take(12)
                        .map(|s| s.id)
                        .collect();
                    state.analytics_data = Some(analytics);
                    for id in cover_ids {
                        trigger_cover_fetch_by_id(&mut state, id, &pool, &tx);
                    }
                }
                AppMessage::UpdateCheckResult(update_opt) => {
                    let was_checking = state.is_checking_update;
                    state.is_checking_update = false;
                    if let Some(info) = update_opt {
                        let new_ver = info.latest_version.clone();
                        let is_brew = info.is_homebrew;
                        if is_brew {
                            state.available_update = Some(info);
                            state.show_toast(format!("❖ Solus {} available: brew upgrade solus", new_ver), unix_now());
                        } else if cfg.update.auto_install && !was_checking {
                            state.is_updating = true;
                            state.available_update = Some(info);
                            state.show_toast(format!("❖ Updating Solus to {} in background...", new_ver), unix_now());
                        } else if state.screen != Screen::Playback {
                            // Automatically open the update modal popup so user doesn't miss it!
                            state.open_update_modal(info);
                        } else {
                            state.available_update = Some(info);
                            state.show_toast(format!("❖ Solus {} is available! Press [U] to update.", new_ver), unix_now());
                        }
                    } else if was_checking {
                        state.show_toast(format!("Solus is up to date (v{})", env!("CARGO_PKG_VERSION")), unix_now());
                    }
                }
                AppMessage::UpdateProgress(pct, spd) => {
                    state.is_updating = true;
                    state.update_progress = Some((pct, spd.clone()));
                    if let Some(state::UpdateModalPhase::Downloading { info, .. }) = state.update_modal_phase.clone() {
                        state.update_modal_phase = Some(state::UpdateModalPhase::Downloading {
                            info: info.or_else(|| state.available_update.clone()),
                            progress_pct: pct,
                            speed: spd,
                        });
                    }
                }
                AppMessage::UpdateSuccess(version) => {
                    state.is_updating = false;
                    state.update_progress = None;
                    let simple_notes = state
                        .available_update
                        .as_ref()
                        .map(|u| {
                            services::updater::record_pending_post_update_welcome(u);
                            if u.simple_notes.is_empty() {
                                services::updater::default_current_release_notes()
                            } else {
                                u.simple_notes.clone()
                            }
                        })
                        .unwrap_or_else(services::updater::default_current_release_notes);
                    state.available_update = None;
                    state.update_modal_phase = Some(state::UpdateModalPhase::RestartRequired {
                        new_version: version.clone(),
                        simple_notes,
                    });
                    if state.screen != Screen::UpdateModal && state.screen != Screen::Playback {
                        state.overlay_base = state.current_base_screen();
                        state.screen = Screen::UpdateModal;
                    }
                    state.show_toast(format!("󰄬 Solus updated to {}! Press Enter to exit.", version), unix_now());
                }
                AppMessage::UpdateFailed(err) => {
                    state.is_updating = false;
                    state.update_progress = None;
                    state.update_modal_phase = Some(state::UpdateModalPhase::Failed {
                        error: err.clone(),
                    });
                    state.show_toast(format!("Update failed: {}", err), unix_now());
                }
                AppMessage::RemotePlayEpisode(ep) => {
                    launch_remote_stream_switch(&mut state, &pool, &cfg, &tx, ep, None, None).await;
                }
                AppMessage::RemoteSwitchAudioStream(target_dub) => {
                    let ep = state.selected_episode.unwrap_or(1);
                    let current_pos = {
                        let st = state.remote_state.read().await;
                        st.time_pos
                    };
                    let resume_pos = if current_pos > 2.0 {
                        Some(current_pos)
                    } else {
                        None
                    };
                    launch_remote_stream_switch(
                        &mut state,
                        &pool,
                        &cfg,
                        &tx,
                        ep,
                        Some(target_dub),
                        resume_pos,
                    )
                    .await;
                }
                AppMessage::AudioTracksUpdated(mpv_tracks) => {
                    let is_dub_stream = state
                        .last_play_options
                        .as_ref()
                        .is_some_and(|o| o.dub);
                    let resolved_tracks = if mpv_tracks.len() == 1
                        && (state.dub_available == Some(true) || is_dub_stream)
                    {
                        let live_aid = mpv_tracks[0].mpv_aid.or(Some(1));
                        vec![
                            services::remote::RemoteAudioTrack {
                                id: 1,
                                code: "jpn".to_string(),
                                label: "Japanese (Original / 日本語)".to_string(),
                                is_active: !is_dub_stream,
                                is_dub: false,
                                mpv_aid: if !is_dub_stream { live_aid } else { None },
                            },
                            services::remote::RemoteAudioTrack {
                                id: 2,
                                code: "eng".to_string(),
                                label: "English (Dub)".to_string(),
                                is_active: is_dub_stream,
                                is_dub: true,
                                mpv_aid: if is_dub_stream { live_aid } else { None },
                            },
                        ]
                    } else {
                        mpv_tracks
                    };

                    if !resolved_tracks.is_empty() {
                        let active = resolved_tracks
                            .iter()
                            .find(|t| t.is_active)
                            .or_else(|| resolved_tracks.first())
                            .cloned();
                        let r_state = state.remote_state.clone();
                        tokio::spawn(async move {
                            {
                                let mut st = r_state.write().await;
                                if let Some(ref act) = active {
                                    st.audio_lang = act.label.clone();
                                    st.audio_mode =
                                        if act.is_dub { "dub" } else { "sub" }.to_string();
                                }
                                st.audio_tracks = resolved_tracks;
                            }
                            services::remote::broadcast_state(&r_state).await;
                        });
                    }
                }
                AppMessage::RemoteAnime4K(mode) => {
                    state.anime4k_mode = mode;
                    cfg.anime4k_mode = mode;
                    let _ = cfg.save();
                    state.show_toast(format!("❖ Anime4K: {}", mode.as_str()), unix_now());
                }
                AppMessage::QueuePrefetchReady { uid, spec_version, prefetch } => {
                    if state.queue.apply_prefetch_result(uid, spec_version, prefetch) {
                        services::queue::save_active_queue(&state.active_profile_id, &state.queue);
                    }
                    sync_remote_queue_snapshot(&state);
                }
                AppMessage::NearEofPrefetch(_anime_id, _ep) => {
                    if state.now_playing.is_some() {
                        warm_next_queue_item_now(&mut state, &cfg, &tx);
                    }
                }
                AppMessage::RemoteQueueSkip => {
                    if let Some(next_item) = state.queue.pop_next_for_advance() {
                        state.queue.manual_stop = false;
                        state.show_toast(
                            format!("◈ Remote Skip -> {} · Ep {}", next_item.title, next_item.episode),
                            unix_now(),
                        );
                        play_queue_item(&mut state, next_item, false, false, &pool, &cfg, &tx).await;
                    } else {
                        state.show_toast("Queue is empty", unix_now());
                    }
                }
                AppMessage::RemoteQueuePlayIndex(idx) => {
                    if let Some(item) = state.queue.remove_at(idx) {
                        state.queue.manual_stop = false;
                        if state.queue_cursor >= state.queue.len() {
                            state.queue_cursor = state.queue.len().saturating_sub(1);
                        }
                        state.show_toast(
                            format!("◈ Remote Playing: {} · Ep {}", item.title, item.episode),
                            unix_now(),
                        );
                        play_queue_item(&mut state, item, false, false, &pool, &cfg, &tx).await;
                    }
                }
                AppMessage::RemoteQueueToggleAuto => {
                    state.queue.auto_advance = !state.queue.auto_advance;
                    services::queue::save_active_queue(&state.active_profile_id, &state.queue);
                    sync_remote_queue_snapshot(&state);
                    state.show_toast(
                        format!(
                            "◈ Queue Auto-Advance: {}",
                            if state.queue.auto_advance { "ON" } else { "OFF" }
                        ),
                        unix_now(),
                    );
                }
                AppMessage::RemoteSubDelayChanged(delay_ms) => {
                    let clamped = delay_ms.clamp(-30_000, 30_000);
                    state.subtitle_delay_ms = clamped;
                    if let Some(ref mut opts) = state.last_play_options {
                        opts.sub_delay_ms = clamped;
                    }
                    persist_profile_playback_settings_fields(&state, &pool).await;
                    state.show_toast(
                        format!("◈ Subtitle delay: {}", state.format_subtitle_delay()),
                        unix_now(),
                    );
                }
                AppMessage::RemoteSubtitleSelectionChanged(selection) => {
                    let code = match selection {
                        api::subtitles::SubtitleSelection::None => "none".to_string(),
                        api::subtitles::SubtitleSelection::Track { code, .. } => code,
                    };
                    state.preferred_subtitle_code = code.clone();
                    if let Some(idx) = state.playback_sub_languages.iter().position(|s| {
                        s.code.eq_ignore_ascii_case(&code)
                            || s.lang_keyword().eq_ignore_ascii_case(&code)
                    }) {
                        state.playback_sub_cursor = idx;
                    } else if code == "none" && !state.playback_sub_languages.is_empty() {
                        state.playback_sub_cursor = 0;
                    }
                    if let Some(ref mut opts) = state.last_play_options {
                        opts.subtitles_disabled = code == "none";
                        opts.sub_lang = Some(code.clone());
                        if opts.subtitles_disabled {
                            opts.sub_url = None;
                        }
                    }
                    persist_profile_playback_settings_fields(&state, &pool).await;
                }
                AppMessage::PlaybackBufferTelemetry(upd) => {
                    let newly_stalled =
                        upd.stall_edge || (!state.live_playback.paused_for_cache && upd.paused_for_cache);
                    if newly_stalled {
                        state.live_playback.record_stall(std::time::Instant::now());
                    }
                    state.live_playback.paused_for_cache = upd.paused_for_cache;
                    state.live_playback.buffering_pct = upd.buffering_pct;
                    state.live_playback.cache_duration_secs = upd.cache_duration_secs;
                    state.live_playback.cache_fw_bytes = upd.cache_fw_bytes;
                    state.live_playback.cache_speed_bps = upd.cache_speed_bps;
                    sync_remote_telemetry_snapshot(&state);
                    if upd.paused_for_cache {
                        maybe_trigger_auto_abr_downshift(&mut state, &cfg, &tx).await;
                    }
                }
                AppMessage::StreamLadderResolved {
                    anime_id,
                    episode,
                    is_dub,
                    ladder,
                } => {
                    state
                        .stream_ladder_cache
                        .insert((anime_id, episode, is_dub), ladder);
                }
                AppMessage::AbrRefreshReady {
                    anime_id,
                    episode,
                    is_dub,
                    mut info,
                } => {
                    state.live_playback.abr_refresh_in_flight = false;
                    if let Some(ladder) = info.ladder.take() {
                        state
                            .stream_ladder_cache
                            .insert((anime_id, episode, is_dub), ladder);
                    }
                    if state.live_playback.paused_for_cache
                        && state.live_playback.anime_id == Some(anime_id)
                        && state.live_playback.episode == episode
                    {
                        maybe_trigger_auto_abr_downshift(&mut state, &cfg, &tx).await;
                    }
                }
                AppMessage::RemoteNetworkModeChanged(mode_raw) => {
                    let mode = config::NetworkMode::from_str_lossy(&mode_raw);
                    apply_network_mode(&mut state, &mut cfg, mode, &tx).await;
                }
                AppMessage::RemoteAutoAbrToggled(enabled) => {
                    cfg.auto_abr_downshift = enabled;
                    state.auto_abr_downshift = enabled;
                    let _ = cfg.save();
                    sync_remote_telemetry_snapshot(&state);
                    state.show_toast(
                        format!(
                            "❖ Remote Auto-ABR: {}",
                            if enabled { "Enabled" } else { "Disabled" }
                        ),
                        unix_now(),
                    );
                }
                AppMessage::RemoteThemeChanged {
                    theme_id,
                    art_mode,
                    art_index,
                } => {
                    if let Some(tid) = theme_id {
                        cfg.remote.theme_id = tid;
                    }
                    if let Some(am) = art_mode {
                        cfg.remote.art_mode = am;
                    }
                    if let Some(idx) = art_index {
                        cfg.remote.art_index = Some(idx);
                    }
                    let _ = cfg.save();
                }
                AppMessage::RemoteClientsChanged(count) => {
                    state.remote_ws_clients = count;
                }
            }
        }

        // Resolve active toast before entering draw (avoids mutable borrow conflict)
        let now       = unix_now();
        let toast_msg = state.active_toast(now).map(|s| s.to_string());
        if toast_msg.is_some() {
            needs_redraw = true;
        }

        if state.screen != last_screen {
            let returning_to_detail = state.screen == Screen::Detail;
            let _ = terminal.clear();
            if returning_to_detail {
                state.invalidate_detail_image_protocols();
            }
            last_screen = state.screen.clone();
            needs_redraw = true;
        }

        if needs_redraw {
            // Wrap full frame draw in synchronized update (DEC Mode 2026) so Kitty
            // and compatible terminals swap graphics placeholders atomically without flicker.
            let _ = execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
            let draw_res = terminal
                .draw(|frame| {
                let base_screen = state.current_base_screen();
                match state.screen {
                    Screen::Home => {
                        if state.is_loading {
                            render_loading(frame, &state);
                        } else {
                            ui::home::render(frame, &mut state, &home_data);
                        }
                    }
                    Screen::Detail => {
                        ui::detail::render(frame, &mut state);
                        if state.is_batch_queue_modal_open {
                            ui::queue::render_batch_queue_modal(frame, &state, &cfg);
                        }
                    }
                    Screen::PlaybackQuery => {
                        ui::detail::render_with_image_policy(frame, &mut state, false);
                        ui::play_query::render_overlay(frame, &state);
                    }
                    Screen::PlaybackOptions => {
                        ui::detail::render_with_image_policy(frame, &mut state, false);
                        ui::play_options::render_overlay(frame, &state);
                    }
                    Screen::Playback => ui::playback::render(frame, &state),
                    Screen::Search   => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::search::render_overlay(frame, &mut state);
                    }
                    Screen::Help => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::help::render_overlay(frame, &state);
                    }
                    Screen::Settings => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::settings::render_overlay(frame, &mut state, &cfg);
                    }
                    Screen::Setup => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::setup::render_overlay(frame, &state, &cfg);
                    }
                    Screen::AniListLogin => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::login::render_overlay(frame, &state, &cfg);
                    }
                    Screen::TailoredWizard => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::tailored::render_wizard_modal(frame, &state);
                    }
                    Screen::TailoredSummary => {
                        ui::tailored::render_taste_dashboard(frame, &state);
                    }
                    Screen::TailoredResults => {
                        ui::tailored::render_tailored_results(frame, &mut state);
                    }
                    Screen::TasteAnalytics => {
                        ui::analytics::render(frame, &mut state);
                    }
                    Screen::RemoteModal => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::remote::render_remote_modal(frame, &state);
                    }
                    Screen::CastModal => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::cast::render_cast_modal(frame, &state);
                    }
                    Screen::CastDeck => {
                        ui::cast::render_cast_deck(frame, &state);
                    }
                    Screen::UpdateModal => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::update_modal::render_overlay(frame, &state);
                    }
                    Screen::Calendar => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::calendar::render_overlay(frame, &mut state);
                    }
                    Screen::ProfileModal => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::profile::render_overlay(frame, &state);
                    }
                    Screen::QueueModal => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::queue::render_overlay(frame, &state);
                    }
                }
                if state.watchlist_target_prompt.is_some() {
                    ui::home::render_watchlist_target_modal(frame, &state);
                }
                if state.exit_confirm_open {
                    ui::home::render_exit_confirm_modal(frame, &state);
                }
                // Toast notification renders on top of everything
                if let Some(ref msg) = toast_msg {
                    ui::help::render_toast(frame, msg, &state.theme());
                }
            })
            .map(|_| ());
            let _ = execute!(terminal.backend_mut(), EndSynchronizedUpdate);
            draw_res?;
            needs_redraw = false;
        }

        // Poll input with short timeout so background messages stay responsive
        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) => {
                    // On Windows, Crossterm emits both KeyEventKind::Press and KeyEventKind::Release.
                    // Only process Press events so keys never double-type.
                    if key.kind == event::KeyEventKind::Press {
                        needs_redraw = true;
                        handle_key(key, &mut state, &mut home_data, &pool, &mut cfg, &tx).await;
                    }
                }
                Event::Mouse(mouse) => {
                    if handle_mouse(mouse, &mut state, &mut home_data, &pool, &mut cfg, &tx).await {
                        needs_redraw = true;
                    }
                }
                Event::Paste(text) => {
                    if state.screen == Screen::QueueModal && state.playlist_transfer_mode.is_some() {
                        let pasted = text
                            .trim()
                            .replace('\r', "")
                            .replace('\n', " ");
                        if !pasted.is_empty() {
                            state.playlist_transfer_choice = 1;
                            state.playlist_transfer_input = pasted;
                            state.playlist_transfer_error = None;
                            let path = services::queue::clean_user_path(&state.playlist_transfer_input);
                            state.playlist_import_candidates =
                                services::queue::scan_portable_playlists_in_dir(&path);
                            state.playlist_import_cursor = 0;
                            needs_redraw = true;
                        }
                    }
                }
                Event::Resize(_, _) => {
                    needs_redraw = true;
                }
                _ => {}
            }
        }

        if state.should_quit {
            break;
        }

        if cfg.update.check_on_startup
            && !state.is_updating
            && !state.is_loading
            && state.available_update.is_none()
            && !matches!(state.update_modal_phase, Some(state::UpdateModalPhase::RestartRequired { .. }))
            && last_update_poll.elapsed() >= Duration::from_secs(180)
        {
            last_update_poll = std::time::Instant::now();
            spawn_update_check(&pool, &tx, cfg.update.auto_install, cfg.update.check_interval, false);
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    state.stop_player();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
    )?;
    terminal.show_cursor()?;
    Ok(())
}

// ─── Input handler ────────────────────────────────────────────────────────────

async fn handle_key(
    key:       event::KeyEvent,
    state:     &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool:      &sqlx::SqlitePool,
    cfg:       &mut config::Config,
    tx:        &tokio::sync::mpsc::Sender<AppMessage>,
) {
    // Ctrl+C works everywhere
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return;
    }

    // Ctrl+Q toggles the Watch Session Queue & Saved Playlists Hub anywhere
    if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if state.screen == Screen::QueueModal {
            state.go_back();
        } else if !state.is_loading {
            open_queue_hub(state, pool).await;
        }
        return;
    }

    if state.exit_confirm_open {
        handle_exit_confirm_modal(key, state);
        return;
    }

    if state.watchlist_target_prompt.is_some() {
        handle_watchlist_target_modal(key, state, home_data, pool, cfg, tx).await;
        return;
    }

    if state.is_loading && state.screen == Screen::Home {
        match key.code {
            KeyCode::Esc => {
                state.skip_preload();
                refresh_home_cover(state, home_data, pool, tx);
            }
            KeyCode::Char('q') => {
                state.open_exit_confirm();
            }
            _ => {}
        }
        return;
    }

    match state.screen {
        Screen::Home     => handle_home(key, state, home_data, pool, cfg, tx).await,
        Screen::Detail   => handle_detail(key, state, pool, cfg, tx).await,
        Screen::PlaybackQuery => handle_playback_query(key, state, pool, cfg, tx).await,
        Screen::PlaybackOptions => handle_playback_options(key, state, pool, cfg, tx).await,
        Screen::Playback => handle_playback(key, state, pool, cfg, tx).await,
        Screen::Search   => handle_search(key, state, pool, tx).await,
        Screen::Help     => handle_help(key, state, pool, cfg),
        Screen::Settings => handle_settings(key, state, pool, cfg, tx).await,
        Screen::Setup    => handle_setup(key, state, cfg),
        Screen::AniListLogin => handle_login(key, state, home_data, pool, cfg, tx).await,
        Screen::TailoredWizard => handle_tailored_wizard(key, state),
        Screen::TailoredSummary => handle_tailored_summary(key, state, pool, tx).await,
        Screen::TailoredResults => handle_tailored_results(key, state, pool, cfg, tx).await,
        Screen::TasteAnalytics => handle_analytics(key, state, pool, tx).await,
        Screen::RemoteModal => handle_remote_modal(key, state).await,
        Screen::CastModal => handle_cast_modal(key, state, tx).await,
        Screen::CastDeck => handle_cast_deck(key, state).await,
        Screen::UpdateModal => handle_update_modal(key, state, tx).await,
        Screen::Calendar => handle_calendar(key, state, home_data, pool, cfg, tx).await,
        Screen::ProfileModal => handle_profile_modal(key, state, home_data, pool, cfg, tx).await,
        Screen::QueueModal => handle_queue_modal(key, state, pool, cfg, tx).await,
    }
}

fn handle_exit_confirm_modal(key: event::KeyEvent, state: &mut AppState) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.close_exit_confirm();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.exit_confirm_open = false;
            state.should_quit = true;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.exit_confirm_cursor = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.exit_confirm_cursor = 1;
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Tab | KeyCode::BackTab => {
            state.exit_confirm_cursor = 1 - (state.exit_confirm_cursor % 2);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if state.exit_confirm_cursor == 1 {
                state.exit_confirm_open = false;
                state.should_quit = true;
            } else {
                state.close_exit_confirm();
            }
        }
        _ => {}
    }
}

async fn handle_update_modal(
    key: event::KeyEvent,
    state: &mut AppState,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(ref phase) = state.update_modal_phase.clone() else {
        state.close_update_modal();
        return;
    };

    match phase {
        state::UpdateModalPhase::Available { info } => {
            match key.code {
                KeyCode::Enter | KeyCode::Char('u') | KeyCode::Char('U') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if !info.is_homebrew && !state.is_updating {
                        state.is_updating = true;
                        services::updater::record_pending_post_update_welcome(info);
                        state.update_modal_phase = Some(state::UpdateModalPhase::Downloading {
                            info: Some(info.clone()),
                            progress_pct: 0.0,
                            speed: "Connecting...".to_string(),
                        });
                        let client = reqwest::Client::new();
                        let dl_url = info.download_url.clone();
                        let target_ver = info.latest_version.clone();
                        let tx_dl = tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = services::updater::download_and_install_update(
                                &client,
                                &dl_url,
                                &target_ver,
                                tx_dl.clone(),
                            )
                            .await
                            {
                                let _ = tx_dl.send(AppMessage::UpdateFailed(e.to_string())).await;
                            }
                        });
                    }
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    if !info.is_homebrew && !state.is_updating {
                        state.is_updating = true;
                        services::updater::record_pending_post_update_welcome(info);
                        let client = reqwest::Client::new();
                        let dl_url = info.download_url.clone();
                        let target_ver = info.latest_version.clone();
                        let tx_dl = tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = services::updater::download_and_install_update(
                                &client,
                                &dl_url,
                                &target_ver,
                                tx_dl.clone(),
                            )
                            .await
                            {
                                let _ = tx_dl.send(AppMessage::UpdateFailed(e.to_string())).await;
                            }
                        });
                        state.close_update_modal();
                        state.show_toast(
                            format!("❖ Downloading Solus {} in background...", info.latest_version),
                            unix_now(),
                        );
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    state.close_update_modal();
                }
                _ => {}
            }
        }
        state::UpdateModalPhase::Downloading { .. } => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('b') | KeyCode::Char('B')) {
                // Allow closing modal to background while download continues
                state.close_update_modal();
                state.show_toast("❖ Update download continuing quietly in background...", unix_now());
            }
        }
        state::UpdateModalPhase::RestartRequired { .. } => {
            match key.code {
                KeyCode::Enter | KeyCode::Char('q') => {
                    // Gracefully exit Solus so user can relaunch into the new version
                    state.should_quit = true;
                }
                KeyCode::Esc => {
                    state.close_update_modal();
                }
                _ => {}
            }
        }
        state::UpdateModalPhase::PostUpdateWelcome { .. } => {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q')) {
                state.close_update_modal();
            }
        }
        state::UpdateModalPhase::Failed { .. } => {
            match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                    state.close_update_modal();
                }
                _ => {}
            }
        }
    }
}

async fn handle_mouse(
    mouse:     event::MouseEvent,
    state:     &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool:      &sqlx::SqlitePool,
    cfg:       &mut config::Config,
    tx:        &tokio::sync::mpsc::Sender<AppMessage>,
) -> bool {
    let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
    let term_area = ratatui::layout::Rect::new(0, 0, term_w, term_h);

    // Right-click always acts as Back / Esc (or closes exit confirmation modal)
    if matches!(mouse.kind, event::MouseEventKind::Down(event::MouseButton::Right)) {
        if state.exit_confirm_open {
            state.close_exit_confirm();
        } else {
            state.go_back();
        }
        return true;
    }

    if state.exit_confirm_open {
        if matches!(mouse.kind, event::MouseEventKind::Down(event::MouseButton::Left)) {
            let (mx, my) = (mouse.column, mouse.row);
            let modal = ui::home::exit_confirm_modal_rect(term_area);
            if mx < modal.x || mx >= modal.right() || my < modal.y || my >= modal.bottom() {
                state.close_exit_confirm();
            } else {
                let mid_x = modal.x + modal.width / 2;
                if mx < mid_x {
                    state.close_exit_confirm();
                } else {
                    state.exit_confirm_open = false;
                    state.should_quit = true;
                }
            }
            return true;
        }
        return false;
    }

    match mouse.kind {
        event::MouseEventKind::ScrollDown => {
            match state.screen {
                Screen::Home => {
                    state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
                    refresh_home_cover(state, home_data, pool, tx);
                    true
                }
                Screen::Detail => {
                    let ep = state.selected_episode.unwrap_or(1);
                    let max_ep = state.selected_anime.as_ref().and_then(|a| a.episodes).map(|e| e as u32).unwrap_or(1);
                    if ep < max_ep {
                        state.selected_episode = Some(ep + 1);
                        true
                    } else {
                        false
                    }
                }
                Screen::PlaybackOptions => {
                    match state.playback_focus_column {
                        0 => {
                            if state.playback_player_cursor + 1 < state.playback_players.len() {
                                state.playback_player_cursor += 1;
                                true
                            } else { false }
                        }
                        1 => {
                            if state.playback_provider_cursor + 1 < state.playback_providers.len() {
                                state.playback_provider_cursor += 1;
                                state.sync_active_provider();
                                true
                            } else { false }
                        }
                        2 => {
                            if state.playback_quality_cursor + 1 < state.playback_qualities.len() {
                                state.playback_quality_cursor += 1;
                                true
                            } else { false }
                        }
                        _ => {
                            if state.playback_sub_cursor + 1 < state.playback_sub_languages.len() {
                                state.playback_sub_cursor += 1;
                                true
                            } else { false }
                        }
                    }
                }
                Screen::Search if !state.search_results.is_empty() && state.search_cursor + 1 < state.search_results.len() => {
                    state.search_cursor += 1;
                    if let Some(a) = state.search_results.get(state.search_cursor) {
                        state.cover_anime_id = Some(a.id);
                    }
                    true
                }
                Screen::TasteAnalytics => {
                    state.scroll_analytics_list(2);
                    true
                }
                Screen::Settings => {
                    let len = ui::settings::get_filtered_items(cfg, state).len();
                    if len > 0 && state.settings_cursor + 1 < len {
                        state.settings_cursor += 1;
                    }
                    true
                }
                _ => false,
            }
        }
        event::MouseEventKind::ScrollUp => {
            match state.screen {
                Screen::Home => {
                    state.active_row = next_non_empty_row(&state.active_row, home_data, -1);
                    refresh_home_cover(state, home_data, pool, tx);
                    true
                }
                Screen::Detail => {
                    let ep = state.selected_episode.unwrap_or(1);
                    if ep > 1 {
                        state.selected_episode = Some(ep - 1);
                        true
                    } else { false }
                }
                Screen::PlaybackOptions => {
                    match state.playback_focus_column {
                        0 => {
                            state.playback_player_cursor = state.playback_player_cursor.saturating_sub(1);
                            true
                        }
                        1 => {
                            state.playback_provider_cursor = state.playback_provider_cursor.saturating_sub(1);
                            state.sync_active_provider();
                            true
                        }
                        2 => {
                            state.playback_quality_cursor = state.playback_quality_cursor.saturating_sub(1);
                            true
                        }
                        _ => {
                            state.playback_sub_cursor = state.playback_sub_cursor.saturating_sub(1);
                            true
                        }
                    }
                }
                Screen::Search if state.search_cursor > 0 => {
                    state.search_cursor -= 1;
                    if let Some(a) = state.search_results.get(state.search_cursor) {
                        state.cover_anime_id = Some(a.id);
                    }
                    true
                }
                Screen::TasteAnalytics => {
                    state.scroll_analytics_list(-2);
                    true
                }
                Screen::Settings => {
                    state.settings_cursor = state.settings_cursor.saturating_sub(1);
                    true
                }
                _ => false,
            }
        }
        event::MouseEventKind::Down(event::MouseButton::Left) => {
            let (mx, my) = (mouse.column, mouse.row);
            match state.screen {
                Screen::Home => {
                    let hero_start = ui::home::TOP_BAR_HEIGHT;
                    let rows_start = hero_start + ui::home::HERO_HEIGHT;

                    // Top Navigation Bar shelf pills
                    if my < hero_start {
                        if let Some(target_row) = ui::home::top_bar_shelf_at(home_data, term_w, mx) {
                            state.active_row = target_row;
                            refresh_home_cover(state, home_data, pool, tx);
                            return true;
                        }
                    }

                    // Featured Hero Stage
                    if my >= hero_start && my < rows_start {
                        let banner_anime = active_anime(state, home_data).or_else(|| home_data.featured.clone());
                        if let Some(anime) = banner_anime {
                            open_detail_from_anime(state, anime, pool, tx).await;
                            return true;
                        }
                    }

                    // Bottom Command Dock (2-row responsive footer)
                    if let Some(action) = ui::home::footer_action_at(state, term_w, term_h, mx, my) {
                        use ui::home::HomeFooterAction::*;
                        let code = match action {
                            Search => KeyCode::Char('/'),
                            Queue => KeyCode::Char('Q'),
                            Calendar => KeyCode::Char('c'),
                            Wizard => KeyCode::Char('w'),
                            Analytics => KeyCode::Char('a'),
                            Remote => KeyCode::Char('m'),
                            Profile => KeyCode::Char('P'),
                            Theme => KeyCode::Char('T'),
                            Login => KeyCode::Char('L'),
                            StopPlayback => KeyCode::Char('X'),
                            UpdateNow => KeyCode::Char('U'),
                            OpenDetail => KeyCode::Enter,
                            Resume => KeyCode::Char('r'),
                            Watchlist => KeyCode::Char('+'),
                            Remove => KeyCode::Char('x'),
                            Refresh => KeyCode::Char('R'),
                            Settings => KeyCode::Char('s'),
                            Help => KeyCode::Char('?'),
                            Quit => KeyCode::Char('q'),
                        };
                        handle_home(event::KeyEvent::from(code), state, home_data, pool, cfg, tx).await;
                        return true;
                    }

                    // Smart Bento Tiers & Cards
                    if my >= rows_start && my < term_h.saturating_sub(ui::home::FOOTER_HEIGHT) {
                        let rel_y = my - rows_start;
                        let card_pitch = ui::home::CARD_WIDTH + ui::home::CARD_GAP;
                        let tiers = ui::home::build_shelf_tiers(home_data, term_w);

                        if !tiers.is_empty() {
                            let avail_h = term_h.saturating_sub(rows_start + ui::home::FOOTER_HEIGHT);
                            let active_tier_idx = tiers
                                .iter()
                                .position(|tier| match tier {
                                    ui::home::ShelfTier::BentoSplit => matches!(
                                        state.active_row,
                                        state::CategoryRow::ContinueWatching | state::CategoryRow::Watchlist
                                    ),
                                    ui::home::ShelfTier::Single(r) => *r == state.active_row,
                                })
                                .unwrap_or(0);
                            let (start_idx, heights) =
                                ui::home::compute_shelf_layout(&tiers, avail_h, active_tier_idx);

                            let mut acc_y = 0u16;
                            let mut tier_top_rel_y = 0u16;
                            let mut tier_height = 0u16;
                            let clicked_tier_idx = heights.iter().position(|&h| {
                                let in_band = rel_y >= acc_y && rel_y < acc_y + h;
                                if in_band {
                                    tier_top_rel_y = acc_y;
                                    tier_height = h;
                                }
                                acc_y += h;
                                in_band
                            });

                            if let Some(clicked_tier_idx) = clicked_tier_idx {
                                let actual_tier_idx = start_idx + clicked_tier_idx;
                                let tier_rect = ratatui::layout::Rect {
                                    x: 0,
                                    y: rows_start + tier_top_rel_y,
                                    width: term_w,
                                    height: tier_height,
                                };
                                if let Some(tier) = tiers.get(actual_tier_idx) {
                                    match tier {
                                        ui::home::ShelfTier::BentoSplit => {
                                            let left_w = ui::home::bento_left_width(home_data.continue_watching.len());
                                            if mx < left_w {
                                                let inner_x = mx.saturating_sub(2);
                                                let col = (inner_x / card_pitch) as usize;
                                                let offset = state.row_offset("continue_watching");
                                                let vis_n = home_data.continue_watching.len().min(2);
                                                let start = ui::home::row_view_start(offset, home_data.continue_watching.len(), vis_n);
                                                let target_idx = start + col;
                                                if let Some(anime) = home_data.continue_watching.get(target_idx).cloned() {
                                                    state.active_row = state::CategoryRow::ContinueWatching;
                                                    state.row_offsets.insert("continue_watching".to_string(), target_idx);
                                                    open_detail_from_anime(state, anime, pool, tx).await;
                                                    return true;
                                                }
                                            } else if mx > left_w {
                                                if let Some(target_idx) = ui::home::watchlist_card_index_at(tier_rect, true, state, home_data, mx, my) {
                                                    if let Some(anime) = home_data.watchlist.get(target_idx).cloned() {
                                                        state.active_row = state::CategoryRow::Watchlist;
                                                        state.row_offsets.insert("watchlist".to_string(), target_idx);
                                                        open_detail_from_anime(state, anime, pool, tx).await;
                                                        return true;
                                                    }
                                                }
                                            }
                                        }
                                        ui::home::ShelfTier::Single(cat_row) => {
                                            if *cat_row == state::CategoryRow::Watchlist {
                                                if let Some(target_idx) = ui::home::watchlist_card_index_at(tier_rect, false, state, home_data, mx, my) {
                                                    if let Some(anime) = home_data.watchlist.get(target_idx).cloned() {
                                                        state.active_row = state::CategoryRow::Watchlist;
                                                        state.row_offsets.insert("watchlist".to_string(), target_idx);
                                                        open_detail_from_anime(state, anime, pool, tx).await;
                                                        return true;
                                                    }
                                                }
                                            } else {
                                                let (key, items): (&str, &[db::cache::Anime]) = match cat_row {
                                                    state::CategoryRow::ContinueWatching => ("continue_watching", &home_data.continue_watching),
                                                    state::CategoryRow::Watchlist        => ("watchlist", &home_data.watchlist),
                                                    state::CategoryRow::Trending         => ("trending", &home_data.trending),
                                                    state::CategoryRow::Popular          => ("popular", &home_data.popular),
                                                    state::CategoryRow::TopRated         => ("top_rated", &home_data.top_rated),
                                                    state::CategoryRow::Seasonal         => ("seasonal", &home_data.seasonal),
                                                };
                                                let (vis_n, row_card_w) = ui::home::row_card_metrics_for(cat_row, term_w);
                                                let row_pitch = row_card_w + ui::home::CARD_GAP;
                                                let col = (mx / row_pitch) as usize;
                                                let offset = state.row_offset(key);
                                                let start = ui::home::row_view_start(offset, items.len(), vis_n);
                                                let target_idx = start + col;
                                                if let Some(anime) = items.get(target_idx).cloned() {
                                                    state.active_row = cat_row.clone();
                                                    state.row_offsets.insert(key.to_string(), target_idx);
                                                    open_detail_from_anime(state, anime, pool, tx).await;
                                                    return true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    false
                }
                Screen::Detail => {
                    // Context Top Bar: Back pill click
                    if my == 0 && mx < 14 {
                        state.go_back();
                        return true;
                    }
                    // Bottom 2-Row Command Dock
                    if let Some(action) = ui::detail::footer_action_at(state, term_w, term_h, mx, my) {
                        use ui::detail::DetailFooterAction::*;
                        let code = match action {
                            Play => KeyCode::Enter,
                            Enqueue => KeyCode::Char('e'),
                            BatchQueue => KeyCode::Char('E'),
                            Options => KeyCode::Char('p'),
                            Download => KeyCode::Char('D'),
                            QueueHub => KeyCode::Char('Q'),
                            Remote => KeyCode::Char('m'),
                            Cast => KeyCode::Char('c'),
                            ToggleView => KeyCode::Char('v'),
                            MarkWatched => KeyCode::Char('w'),
                            Watchlist => KeyCode::Char('+'),
                            NextEp => KeyCode::Char('n'),
                            Drop => KeyCode::Backspace,
                            Theme => KeyCode::Char('T'),
                            Search => KeyCode::Char('/'),
                            Help => KeyCode::Char('?'),
                            Back => KeyCode::Esc,
                        };
                        handle_detail(event::KeyEvent::from(code), state, pool, cfg, tx).await;
                        return true;
                    }
                    // Episodes (above Detail Command Dock)
                    let content_bottom = term_h.saturating_sub(ui::detail::DETAIL_FOOTER_HEIGHT);
                    match state.episode_view_mode {
                        state::EpisodeViewMode::List => {
                            if my >= 16 && my < content_bottom {
                                let inner_h = content_bottom.saturating_sub(16);
                                let card_h = if inner_h >= 21 { 7 } else if inner_h >= 14 { 6 } else { 5 };
                                let visible_cards = (inner_h / card_h).max(1) as usize;
                                let card_idx = ((my - 16) / card_h) as usize;
                                if card_idx < visible_cards {
                                    let ep_idx = state.episode_offset + card_idx;
                                    if ep_idx < state.episode_list.len() {
                                        let target_ep = state.episode_list[ep_idx];
                                        if state.selected_episode == Some(target_ep) {
                                            begin_playback_flow(state, pool, cfg, false, tx).await;
                                        } else {
                                            state.selected_episode = Some(target_ep);
                                        }
                                        return true;
                                    }
                                }
                            }
                        }
                        state::EpisodeViewMode::Split => {
                            if mx < 44 {
                                // Left Queue
                                if my >= 17 && my < content_bottom {
                                    let rel_line = (my - 17) as usize;
                                    let visible_rows = content_bottom.saturating_sub(19) as usize;
                                    if rel_line < visible_rows {
                                        let ep_idx = state.episode_offset + rel_line;
                                        if ep_idx < state.episode_list.len() {
                                            let target_ep = state.episode_list[ep_idx];
                                            if state.selected_episode == Some(target_ep) {
                                                begin_playback_flow(state, pool, cfg, false, tx).await;
                                            } else {
                                                state.selected_episode = Some(target_ep);
                                            }
                                            return true;
                                        }
                                    }
                                }
                            } else if my >= 16 && my < content_bottom {
                                // Right Inspector Theater
                                begin_playback_flow(state, pool, cfg, false, tx).await;
                                return true;
                            }
                        }
                    }
                    false
                }
                Screen::PlaybackOptions => {
                    let layout = ui::play_options::compute_layout(term_area);
                    let pos = ratatui::layout::Position { x: mx, y: my };
                    if !layout.modal_rect.contains(pos) {
                        state.go_back();
                        return true;
                    }
                    if layout.player_chip_rect.contains(pos) {
                        state.cycle_playback_player();
                        state.show_toast(
                            format!("Player: {}", state.selected_player().as_str()),
                            unix_now(),
                        );
                        return true;
                    }
                    if layout.audio_chip_rect.contains(pos) {
                        if !state.pending_dub && state.dub_available == Some(false) {
                            state.show_toast(
                                "English Dub is not available for this anime (Sub only)",
                                unix_now(),
                            );
                        } else {
                            state.pending_dub = !state.pending_dub;
                            state.is_loading_subs = true;
                            if let Some(ref anime) = state.selected_anime {
                                let anime_c = anime.clone();
                                let ep = state.selected_episode.unwrap_or(1);
                                let dub = state.pending_dub;
                                let req_gen = state.bump_playback_request_generation();
                                trigger_stream_detection(&anime_c, ep, dub, req_gen, tx);
                            }
                        }
                        return true;
                    }
                    if layout.anime4k_chip_rect.contains(pos) {
                        state.toggle_anime4k();
                        state.show_toast(
                            format!("Anime4K: {}", state.anime4k_mode.as_str()),
                            unix_now(),
                        );
                        return true;
                    }
                    if layout.network_chip_rect.contains(pos) {
                        cycle_network_mode(state, cfg, true, tx).await;
                        return true;
                    }
                    if layout.pip_chip_rect.contains(pos) {
                        toggle_pip_mode_action(state, cfg);
                        return true;
                    }
                    if layout.corner_chip_rect.contains(pos) {
                        cycle_pip_corner_action(state, cfg);
                        return true;
                    }
                    if layout.autofs_chip_rect.contains(pos) {
                        toggle_auto_fullscreen_action(state, cfg);
                        return true;
                    }
                    if layout.hud_chip_rect.contains(pos) {
                        toggle_info_hud_action(state, false);
                        return true;
                    }
                    if layout.launch_btn_rect.contains(pos) {
                        let enter_key = event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
                        handle_playback_options(enter_key, state, pool, cfg, tx).await;
                        return true;
                    }
                    if layout.back_btn_rect.contains(pos) {
                        state.go_back();
                        return true;
                    }

                    let clicked_col = [
                        (0usize, layout.player_rect),
                        (1usize, layout.provider_rect),
                        (2usize, layout.quality_rect),
                        (3usize, layout.subtitle_rect),
                    ]
                    .into_iter()
                    .find(|(_, rect)| rect.contains(pos));

                    if let Some((col_idx, rect)) = clicked_col {
                        state.playback_focus_column = col_idx;
                        state.playback_sub_focused = col_idx == 3;
                        if my > rect.y && my + 1 < rect.bottom() {
                            let rel_item_row = (my - (rect.y + 1)) as usize;
                            match col_idx {
                                0 if rel_item_row < state.playback_players.len() => {
                                    state.playback_player_cursor = rel_item_row;
                                }
                                1 if rel_item_row < state.playback_providers.len() => {
                                    state.playback_provider_cursor = rel_item_row;
                                    state.sync_active_provider();
                                }
                                2 if rel_item_row < state.playback_qualities.len() => {
                                    state.playback_quality_cursor = rel_item_row;
                                }
                                3 if rel_item_row < state.playback_sub_languages.len() => {
                                    state.playback_sub_cursor = rel_item_row;
                                }
                                _ => {}
                            }
                        }
                        return true;
                    }
                    false
                }
                Screen::Search => {
                    if my >= 4 {
                        let item_idx = (my - 4) as usize;
                        if let Some(anime) = state.search_results.get(item_idx) {
                            open_detail_from_anime(state, anime.clone(), pool, tx).await;
                            return true;
                        }
                    }
                    false
                }
                _ => {
                    if my == 0 || mx < 5 {
                        state.go_back();
                        return true;
                    }
                    false
                }
            }
        }
        _ => false,
    }
}

// ── Home screen ───────────────────────────────────────────────────────────────

/// Cycle UI theme cleanly across Home, Detail, and Overlays while disabling silent Ambilight override.
fn cycle_active_theme(
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &mut config::Config,
) {
    state.theme_index = (state.theme_index + 1) % ui::theme::THEMES.len();
    if state.ambilight_enabled {
        state.ambilight_enabled = false;
        cfg.ambilight_enabled = false;
        let _ = cfg.save();
    }
    let theme_name = state.theme().name;
    let pool2 = pool.clone();
    let pid = state.active_profile_id.clone();
    let pname = state.active_profile_name.clone();
    let picon = state.active_profile_icon.clone();
    let tidx = state.theme_index;
    let audio = if cfg.audio_mode == config::AudioMode::Dub { "dub" } else { "sub" };
    tokio::spawn(async move {
        let _ = db::user::update_profile(&pool2, &pid, &pname, &picon, tidx, audio).await;
    });
    state.show_toast(format!("◈ Theme: {}", theme_name), unix_now());
}

async fn handle_home(
    key:       event::KeyEvent,
    state:     &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool:      &sqlx::SqlitePool,
    cfg:       &mut config::Config,
    tx:        &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => state.open_exit_confirm(),

        // Row navigation — skip rows with no content so the cursor never
        // lands on an invisible row where Enter would do nothing.
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
            state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
            refresh_home_cover(state, home_data, pool, tx);
        }
        KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
            state.active_row = next_non_empty_row(&state.active_row, home_data, -1);
            refresh_home_cover(state, home_data, pool, tx);
        }

        // Direct shelf jump shortcuts (1..=6 matching the Top Bar shelf pills)
        KeyCode::Char(c @ '1'..='6') => {
            let target = match c {
                '1' => state::CategoryRow::ContinueWatching,
                '2' => state::CategoryRow::Watchlist,
                '3' => state::CategoryRow::Trending,
                '4' => state::CategoryRow::Popular,
                '5' => state::CategoryRow::TopRated,
                _   => state::CategoryRow::Seasonal,
            };
            if row_len(&target, home_data) > 0 {
                state.active_row = target;
                refresh_home_cover(state, home_data, pool, tx);
            }
        }

        // Card navigation within row
        KeyCode::Char('l') | KeyCode::Right => {
            let (key, max) = active_row_key_max(state, home_data);
            state.scroll_row_right(&key, max);
            refresh_home_cover(state, home_data, pool, tx);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            let (key, _) = active_row_key_max(state, home_data);
            state.scroll_row_left(&key);
            refresh_home_cover(state, home_data, pool, tx);
        }

        // Open detail for highlighted card
        KeyCode::Enter | KeyCode::Char('d') => {
            if let Some(anime) = active_anime(state, home_data) {
                open_detail_from_anime(state, anime, pool, tx).await;
            }
        }

        // Quick resume from Home: open detail, jump to the next unwatched episode,
        // then continue through the normal playback flow.
        KeyCode::Char('r') => {
            if let Some(anime) = active_anime(state, home_data) {
                open_detail_from_anime(state, anime, pool, tx).await;
                begin_playback_flow(state, pool, cfg, false, tx).await;
            }
        }

        KeyCode::Char('w') | KeyCode::Char('W') => state.open_tailored_wizard(),
        KeyCode::Char('m') | KeyCode::Char('M') => {
            state.overlay_base = Screen::Home;
            if state.remote_qr.is_none() {
                state.remote_qr = services::remote::generate_terminal_qr(&state.remote_url);
            }
            state.screen = Screen::RemoteModal;
        }
        KeyCode::Char('Q') => {
            open_queue_hub(state, pool).await;
        }
        KeyCode::Char('N') => {
            cycle_network_mode(state, cfg, true, tx).await;
        }
        KeyCode::Char('e') => {
            if let Some(anime) = active_anime(state, home_data) {
                let next_ep = home_data
                    .resume_next
                    .get(&anime.id)
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                let default_dub = cfg.audio_mode == config::AudioMode::Dub;
                let item = build_queue_item_for_anime(&anime, next_ep, default_dub, cfg, state);
                let label = format!("{} · Ep {}", item.title, item.episode);
                let outcome = state.queue.enqueue_back(item);
                state.queue_cursor = state.queue.len().saturating_sub(1);
                persist_and_warm_queue(state, cfg, tx);
                let verb = if outcome.replaced { "Refreshed in queue" } else { "Queued" };
                state.show_toast(format!("◆ {}: {} (#{})", verb, label, outcome.position), unix_now());
            }
        }
        KeyCode::Char('/') => state.open_search(),
        KeyCode::Char('c') | KeyCode::Char('C') => {
            open_and_fetch_calendar(state, pool, tx).await;
        }
        KeyCode::Char('?') => state.open_help(),
        KeyCode::Char('s') => state.open_settings(),
        KeyCode::Char('a') | KeyCode::Char('A') => {
            trigger_analytics_fetch(state, pool.clone(), tx.clone());
        }
        KeyCode::Char('L') => {
            state.open_login();
            spawn_merge_audit(pool, &state.auth, tx);
        }
        KeyCode::Char('P') => {
            let summaries = db::user::list_profile_summaries(pool).await.unwrap_or_default();
            state.open_profile_modal(summaries);
        }
        KeyCode::Char('t') | KeyCode::Char('T') => {
            cycle_active_theme(state, pool, cfg);
        }
        KeyCode::Char('U') => {
            if let Some(ref info) = state.available_update.clone() {
                state.open_update_modal(info.clone());
            } else if !state.is_checking_update {
                state.is_checking_update = true;
                state.show_toast("Checking GitHub for updates...", unix_now());
                let tx_check = tx.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    let current_ver = env!("CARGO_PKG_VERSION");
                    let res = services::updater::check_for_updates(&client, current_ver).await.unwrap_or(None);
                    let _ = tx_check.send(AppMessage::UpdateCheckResult(res)).await;
                });
            }
        }
        // Remove highlighted anime from Continue Watching or Watchlist, or stop active player
        KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
            if state.active_row == state::CategoryRow::ContinueWatching {
                if let Some(anime) = active_anime(state, home_data) {
                    let now = unix_now();
                    let anime_id = anime.id;
                    let title = anime.display_title().to_string();

                    // Instantly update in-memory UI
                    home_data.continue_watching.retain(|a| a.id != anime_id);
                    home_data.progress_labels.remove(&anime_id);
                    home_data.resume_next.remove(&anime_id);
                    let max_idx = home_data.continue_watching.len().saturating_sub(1);
                    let offset = state.row_offset("continue_watching").min(max_idx);
                    state.row_offsets.insert("continue_watching".to_string(), offset);

                    if home_data.continue_watching.is_empty() {
                        state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
                    }
                    refresh_home_cover(state, home_data, pool, tx);
                    state.show_toast(format!("󰅖 Removed \"{}\" from Continue Watching", title), now);

                    // Persist to database asynchronously and broadcast
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    let mut auth_clone = state.auth.clone();
                    let trackers_cfg = cfg.trackers.clone();
                    tokio::spawn(async move {
                        let _ = db::user::remove_continue_watching(&pool2, anime_id).await;
                        if let Some(token) = auth_clone.anilist_token() {
                            let client = api::anilist::AniListClient::with_token(Some(token));
                            let _ = client.delete_media_list_entry(auth_clone.anilist.user_id, anime_id).await;
                            let _ = db::user::delete_tracker_entry(&pool2, anime_id, config::TrackerProvider::AniList).await;
                        }
                        if auth_clone.mal.has_write_token() {
                            let cid = trackers_cfg.resolved_mal_client_id();
                            let _ = services::scrobble::maybe_refresh_mal_token(&mut auth_clone, cid.as_deref(), now).await;
                            if let Some(tok) = auth_clone.mal_token() {
                                if let Some(mid) = anime.mal_id.or(db::cache::get_mal_id_for_anime(&pool2, anime_id).await.ok().flatten()) {
                                    let _ = api::mal::MalClient::with_token(cid.clone(), Some(tok)).delete_list_entry(mid).await;
                                    let _ = db::user::delete_tracker_entry(&pool2, anime_id, config::TrackerProvider::Mal).await;
                                }
                            }
                        }
                        if let Ok(cw) = services::sync::load_continue_watching(&pool2).await {
                            let _ = tx2.send(AppMessage::ContinueWatchingUpdated(cw)).await;
                        }
                    });
                }
            } else if state.active_row == state::CategoryRow::Watchlist {
                if let Some(anime) = active_anime(state, home_data) {
                    trigger_watchlist_remove_flow(anime, false, Some(home_data), state, pool, cfg, tx).await;
                }
            } else if state.now_playing.is_some() {
                state.queue.manual_stop = true;
                state.stop_player();
                state.show_toast("Playback stopped", unix_now());
            }
        }

        KeyCode::Char('X') => {
            if state.now_playing.is_some() {
                state.queue.manual_stop = true;
                state.stop_player();
                state.show_toast("Playback stopped", unix_now());
            }
        }
        KeyCode::Char('!') => {
            refresh_dependency_status(state);
            state.open_setup();
        }

        // Open Bento Status & Tracker List Picker for the highlighted card from Home
        KeyCode::Char('+') => {
            if let Some(anime) = active_anime(state, home_data) {
                trigger_watchlist_status_picker_flow(anime, false, Some(home_data), state, pool).await;
            }
        }

        // Refresh home data (re-sync respecting TTLs)
        KeyCode::Char('R') => {
            state.is_loading = true;
            let pool2 = pool.clone();
            let cfg2  = cfg.clone();
            let tx2   = tx.clone();
            let auth2 = state.auth.clone();
            tokio::spawn(async move {
                let client = api::anilist::AniListClient::with_token(auth2.anilist_token());
                let mal_cid = cfg2.trackers.resolved_mal_client_id();
                let now    = unix_now();
                let result = services::sync::sync_all(
                    &pool2,
                    &client,
                    Some(&auth2),
                    mal_cid.as_deref(),
                    cfg2.cache.trending_ttl,
                    cfg2.cache.stable_ttl,
                    now,
                )
                .await;
                match result {
                    Ok(data) => { let _ = tx2.send(AppMessage::HomeData(Box::new(data))).await; }
                    Err(e)   => {
                        eprintln!("Refresh error: {e}");
                        let _ = tx2.send(AppMessage::HomeData(Box::new(ui::home::HomeData::empty()))).await;
                    }
                }
            });
        }

        _ => {}
    }
}

/// Row key string + item count for the active row.
fn active_row_key_max(state: &AppState, data: &ui::home::HomeData) -> (String, usize) {
    use state::CategoryRow::*;
    match state.active_row {
        ContinueWatching => ("continue_watching".to_string(), data.continue_watching.len()),
        Watchlist        => ("watchlist".to_string(),         data.watchlist.len()),
        Trending         => ("trending".to_string(),          data.trending.len()),
        Popular          => ("popular".to_string(),           data.popular.len()),
        TopRated         => ("top_rated".to_string(),         data.top_rated.len()),
        Seasonal         => ("seasonal".to_string(),          data.seasonal.len()),
    }
}

/// Currently highlighted anime card.
fn active_anime(state: &AppState, data: &ui::home::HomeData) -> Option<db::cache::Anime> {
    use state::CategoryRow::*;
    let (key, _) = active_row_key_max(state, data);
    let offset   = state.row_offset(&key);
    let list     = match state.active_row {
        ContinueWatching => &data.continue_watching,
        Watchlist        => &data.watchlist,
        Trending         => &data.trending,
        Popular          => &data.popular,
        TopRated         => &data.top_rated,
        Seasonal         => &data.seasonal,
    };
    list.get(offset).cloned()
}

async fn open_detail_from_anime(
    state: &mut AppState,
    anime: db::cache::Anime,
    pool:  &sqlx::SqlitePool,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let in_wl = db::user::is_in_watchlist(pool, anime.id).await.unwrap_or(false);
    let watched = db::user::get_watched_episodes(pool, anime.id).await.unwrap_or_default();
    state.in_watchlist = in_wl;
    if state.cover_anime_id != Some(anime.id) || state.cover_state.is_none() {
        state.cover_failed_anime_id = None;
        ensure_cover_fetch(state, &anime, pool, tx);
    }
    let anime_id = anime.id;
    let title_romaji = anime.title_romaji.clone();
    let title_english = anime.title_english.clone();
    let should_check_airing = anime.status.as_deref() == Some("RELEASING") || anime.next_airing_episode.is_none();
    state.open_detail(anime);
    let cached_blob = match state.selected_anime.as_ref().and_then(|a| a.cover_blob.clone()) {
        Some(b) => Some(b),
        None => db::cache::get_cover_blob(pool, anime_id).await.ok().flatten(),
    };
    if let Some(blob) = cached_blob {
        if let Ok(dyn_img) = image::load_from_memory(&blob) {
            state.cover_raw_cache.entry(anime_id).or_insert_with(|| dyn_img.clone());
            let font_size = state.picker.as_ref().map(|p| p.font_size).unwrap_or((12, 24));
            let card_img = prepare_card_cover_with_font(dyn_img.clone(), font_size);
            let portrait_img = prepare_portrait_cover_with_font(dyn_img.clone(), font_size);
            let thumb_img = prepare_16_9_thumbnail_cover(dyn_img, font_size);
            if let Some(picker) = state.picker {
                state
                    .detail_cache
                    .entry(anime_id)
                    .or_insert_with(|| build_stateful_protocol(picker, portrait_img));
            }
            let base_theme = ui::theme::get_theme(state.theme_index);
            state.ambilight_theme = Some(ui::theme::extract_ambilight_palette(&card_img, base_theme));
            state.current_card_img = Some(thumb_img);
        }
    }
    state.set_watched_episodes(watched.into_iter().map(|e| e as u32).collect());
    let progress = db::user::get_all_progress_for_anime(pool, anime_id).await.unwrap_or_default();
    state.anime_progress = progress;
    let offline = db::user::get_offline_episodes(pool, anime_id).await.unwrap_or_default();
    state.offline_episodes = offline;
    if let Some(saved_delay) =
        db::user::get_anime_subtitle_delay_ms(pool, &state.active_profile_id, anime_id).await
    {
        state.subtitle_delay_ms = saved_delay;
    } else if let Some(pb) =
        db::user::get_profile_playback_settings(pool, &state.active_profile_id).await
    {
        state.subtitle_delay_ms = pb.subtitle_delay_ms.clamp(-30_000, 30_000);
    }
    let fillers = db::user::get_anime_fillers(pool, anime_id).await.unwrap_or_default();
    state.anime_fillers = fillers;

    if state.anime_fillers.is_empty() {
        trigger_filler_fetch(anime_id, title_english.clone(), title_romaji.clone(), pool.clone(), tx.clone());
    }
    if !state.anime_mal_ids.contains_key(&anime_id) {
        trigger_mal_id_fetch(anime_id, tx.clone());
    }

    if should_check_airing {
        trigger_airing_check(anime_id, pool.clone(), tx.clone());
    }
    trigger_episode_metadata_fetch(
        anime_id,
        state.episode_metadata_generation,
        title_romaji,
        title_english,
        tx.clone(),
    );
}

/// Query AnimeFillerList in the background to scrape filler classifications.
fn trigger_filler_fetch(
    anime_id: i64,
    title_english: Option<String>,
    title_romaji: String,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let title = title_english.as_deref().unwrap_or(&title_romaji);
        let mut map = api::filler::fetch_filler_episodes(title).await;
        if map.is_empty() && title_english.is_some() {
            map = api::filler::fetch_filler_episodes(&title_romaji).await;
        }
        if !map.is_empty() {
            let _ = db::user::save_anime_fillers(&pool, anime_id, &map).await;
            let _ = tx.send(AppMessage::AnimeFillersReady(anime_id, map)).await;
        }
    });
}

/// Resolve MyAnimeList ID in the background for AniSkip lookup.
fn trigger_mal_id_fetch(
    anime_id: i64,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        if let Some(mal_id) = api::anilist::resolve_mal_id(&client, anime_id).await {
            let _ = tx.send(AppMessage::AnimeMalIdReady(anime_id, mal_id)).await;
        }
    });
}

/// Query AniList in the background to refresh airing status and next episode number.
fn trigger_airing_check(
    anime_id: i64,
    pool:     sqlx::SqlitePool,
    tx:       tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::new();
        if let Ok((status, episodes, next_airing)) = client.fetch_airing_status(anime_id).await {
            let _ = db::cache::update_airing_status(&pool, anime_id, status.clone(), episodes, next_airing).await;
            let _ = tx.send(AppMessage::AiringStatusUpdated(anime_id, status, episodes, next_airing)).await;
        }
    });
}

async fn begin_playback_flow(
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
    force_resume: bool,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    open_episode_options_flow(
        state,
        pool,
        cfg,
        force_resume,
        state::PlaybackOptionsPurpose::LaunchNow,
        tx,
    )
    .await;
}

async fn open_episode_options_flow(
    state:   &mut AppState,
    pool:    &sqlx::SqlitePool,
    cfg:     &config::Config,
    force_resume: bool,
    purpose: state::PlaybackOptionsPurpose,
    tx:      &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(anime) = state.selected_anime.clone() else {
        return;
    };

    if force_resume {
        state.selected_episode = Some(state.next_unwatched_episode());
    }

    let ep = state.selected_episode.unwrap_or(1);
    let max_aired = anime.aired_episodes();
    if ep > max_aired && max_aired > 0 {
        state.show_toast(
            format!("Episode {} is locked — not yet released/aired", ep),
            unix_now(),
        );
        return;
    }

    let default_audio = db::user::get_audio_mode(pool, anime.id)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| cfg.audio_mode.clone());
    let use_dub = default_audio == config::AudioMode::Dub;
    let ep = state.selected_episode.unwrap_or(1);

    let default_player = state.selected_player();
    let initial_query = db::user::get_playback_query(pool, anime.id)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| anime.playback_query());
    state.open_playback_options(
        initial_query,
        quality_index(&cfg.quality),
        use_dub,
        default_player,
    );
    state.playback_options_purpose = purpose;
    trigger_stream_detection(&anime, ep, use_dub, state.playback_request_generation, tx);
}

fn trigger_stream_detection(
    anime: &db::cache::Anime,
    episode: u32,
    is_dub: bool,
    request_generation: u64,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let anime_id = anime.id;
    let identity = anime.search_identity();
    let net_mode = current_network_mode();
    let force_v4 = current_force_ipv4();
    let tx = tx.clone();
    tokio::spawn(async move {
        let info = api::subtitles::fetch_stream_info_with_identity(
            &identity,
            episode,
            is_dub,
            force_v4,
            net_mode.network_timeout_secs(),
        )
        .await;
        let _ = tx
            .send(AppMessage::StreamInfoDetected(
                anime_id,
                episode,
                is_dub,
                request_generation,
                info,
            ))
            .await;
    });
}

/// Ensure the Home banner cover follows the currently highlighted anime.
/// Shared HTTP/2 connection-pooled client for concurrent cover downloads.
fn cover_http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_max_idle_per_host(12)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

static ACTIVE_NETWORK_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ACTIVE_FORCE_IPV4: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn current_network_mode() -> config::NetworkMode {
    match ACTIVE_NETWORK_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => config::NetworkMode::CongestedWifi,
        2 => config::NetworkMode::UltraSaver,
        _ => config::NetworkMode::Normal,
    }
}

fn current_force_ipv4() -> bool {
    ACTIVE_FORCE_IPV4.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_global_network_flags(mode: config::NetworkMode, force_ipv4: bool) {
    let idx = match mode {
        config::NetworkMode::Normal        => 0,
        config::NetworkMode::CongestedWifi => 1,
        config::NetworkMode::UltraSaver    => 2,
    };
    ACTIVE_NETWORK_MODE.store(idx, std::sync::atomic::Ordering::Relaxed);
    ACTIVE_FORCE_IPV4.store(force_ipv4, std::sync::atomic::Ordering::Relaxed);
}

fn compact_anilist_cover_url(url: &str, compact: bool) -> String {
    if compact && url.contains("/large/") {
        url.replacen("/large/", "/medium/", 1)
    } else {
        url.to_string()
    }
}

/// Bounded concurrency semaphore so startup preloading saturates keep-alive connections without socket/API spikes.
fn cover_download_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(12))
}

fn congested_cover_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(3))
}

fn ultra_cover_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(1))
}

fn active_cover_semaphore(mode: config::NetworkMode) -> &'static tokio::sync::Semaphore {
    match mode {
        config::NetworkMode::Normal        => cover_download_semaphore(),
        config::NetworkMode::CongestedWifi => congested_cover_semaphore(),
        config::NetworkMode::UltraSaver    => ultra_cover_semaphore(),
    }
}

async fn wait_while_paused_for_cache() {
    for _ in 0..48 {
        if !services::ipc::is_playback_paused_for_cache() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn sync_remote_telemetry_snapshot(state: &AppState) {
    let remote_state = state.remote_state.clone();
    let net_mode = state.network_mode.as_str().to_string();
    let net_tag = state.network_mode.short_tag().to_string();
    let auto_abr = state.auto_abr_downshift;
    let force_ipv4 = state.force_ipv4;
    let cur_quality = if state.live_playback.current_height > 0 {
        format!("{}p", state.live_playback.current_height)
    } else {
        String::new()
    };
    let buffer_secs = state.live_playback.cache_duration_secs;
    let cache_mb = state.live_playback.cache_mb();
    let cache_speed_kbps = state.live_playback.cache_speed_kbps();
    let paused_for_cache = state.live_playback.paused_for_cache;
    let buffering_pct = state.live_playback.buffering_pct;
    let stall_count = state.live_playback.stall_count;

    tokio::spawn(async move {
        {
            let mut st = remote_state.write().await;
            st.network_mode = net_mode;
            st.network_tag = net_tag;
            st.auto_abr = auto_abr;
            st.force_ipv4 = force_ipv4;
            if !cur_quality.is_empty() {
                st.current_quality = cur_quality;
            }
            st.buffer_secs = buffer_secs;
            st.cache_mb = cache_mb;
            st.cache_speed_kbps = cache_speed_kbps;
            st.paused_for_cache = paused_for_cache;
            st.buffering_pct = buffering_pct;
            st.stall_count = stall_count;
        }
        services::remote::broadcast_state(&remote_state).await;
    });
}

fn sync_remote_theme_snapshot(state: &AppState, cfg: &config::Config) {
    let remote_state = state.remote_state.clone();
    let theme_id = cfg.remote.theme_id.clone();
    let art_mode = cfg.remote.art_mode.clone();
    let art_index = cfg
        .remote
        .art_index
        .unwrap_or_else(|| services::remote::find_remote_theme(&theme_id).default_art_idx);
    tokio::spawn(async move {
        {
            let mut st = remote_state.write().await;
            st.remote_theme = theme_id;
            st.remote_art_mode = art_mode;
            st.remote_art_index = art_index;
        }
        services::remote::broadcast_state(&remote_state).await;
    });
}

async fn apply_network_mode(
    state: &mut AppState,
    cfg: &mut config::Config,
    mode: config::NetworkMode,
    _tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    cfg.network_mode = mode;
    state.network_mode = mode;
    set_global_network_flags(mode, cfg.force_ipv4);
    let _ = cfg.save();
    sync_remote_telemetry_snapshot(state);
    if state.now_playing.is_some() {
        let _ = services::ipc::send_set_network_mode(mode);
    }
    state.show_toast(
        format!("◈ Network Mode: {} [{}]", mode.label(), mode.short_tag()),
        unix_now(),
    );
}

async fn cycle_network_mode(
    state: &mut AppState,
    cfg: &mut config::Config,
    forward: bool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let next = if forward {
        state.network_mode.next()
    } else {
        state.network_mode.prev()
    };
    apply_network_mode(state, cfg, next, tx).await;
}

async fn maybe_trigger_auto_abr_downshift(
    state: &mut AppState,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if !cfg.auto_abr_downshift {
        return;
    }
    if !state.live_playback.player.to_ascii_lowercase().contains("mpv") {
        return;
    }
    let Some(anime_id) = state.live_playback.anime_id else {
        return;
    };
    let now = std::time::Instant::now();
    if !state.live_playback.can_abr_downshift(now) {
        return;
    }

    let multi_stall = state.live_playback.recent_stall_times.len() >= 2;
    let slow_throughput = state.live_playback.current_tbr_kbps > 0
        && state.live_playback.cache_speed_bps > 0.0
        && state.live_playback.cache_speed_kbps()
            < (state.live_playback.current_tbr_kbps as f64) * 0.60;
    if !multi_stall && !slow_throughput {
        return;
    }

    let ep = state.live_playback.episode;
    let is_dub = state.live_playback.is_dub;
    let unix = unix_now();

    if let Some(ladder) = state.stream_ladder_cache.get(&(anime_id, ep, is_dub)).cloned() {
        if ladder.is_fresh(unix) {
            if let Some(variant) = ladder
                .select_lower_tier(
                    state.live_playback.current_height,
                    state.live_playback.current_fps_x10,
                    state.live_playback.current_tbr_kbps,
                )
                .cloned()
            {
                execute_abr_hot_switch(state, variant, &ladder, "Auto-ABR");
            } else if state.network_mode == config::NetworkMode::Normal
                && multi_stall
                && !state.live_playback.prompted_congested_mode
            {
                state.live_playback.prompted_congested_mode = true;
                state.show_toast(
                    "Repeated stalls at lowest tier — press [N] for Congested Wi-Fi mode",
                    unix_now(),
                );
            }
            return;
        }
    }

    if !state.live_playback.abr_refresh_in_flight {
        if let Some(ref anime) = state.selected_anime.clone() {
            state.live_playback.abr_refresh_in_flight = true;
            let title = state
                .last_play_options
                .as_ref()
                .map(|o| o.title.clone())
                .unwrap_or_else(|| anime.playback_query());
            let mal_id = anime.mal_id;
            let net_mode = state.network_mode;
            let force_v4 = cfg.force_ipv4;
            let tx2 = tx.clone();
            tokio::spawn(async move {
                let info = api::subtitles::fetch_stream_info_with_network(
                    anime_id,
                    mal_id,
                    &title,
                    ep,
                    is_dub,
                    force_v4,
                    net_mode.network_timeout_secs(),
                )
                .await;
                let _ = tx2
                    .send(AppMessage::AbrRefreshReady {
                        anime_id,
                        episode: ep,
                        is_dub,
                        info,
                    })
                    .await;
            });
        }
    }
}

async fn manual_abr_step_down(
    state: &mut AppState,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(anime_id) = state.live_playback.anime_id else {
        state.show_toast("Start mpv playback first to step down stream tier", unix_now());
        return;
    };
    if !state.live_playback.player.to_ascii_lowercase().contains("mpv") {
        state.show_toast("Live tier hot-switch requires mpv player", unix_now());
        return;
    }
    let ep = state.live_playback.episode;
    let is_dub = state.live_playback.is_dub;
    let unix = unix_now();
    if let Some(ladder) = state.stream_ladder_cache.get(&(anime_id, ep, is_dub)).cloned() {
        if ladder.is_fresh(unix) {
            if let Some(variant) = ladder
                .select_lower_tier(
                    state.live_playback.current_height,
                    state.live_playback.current_fps_x10,
                    state.live_playback.current_tbr_kbps,
                )
                .cloned()
            {
                execute_abr_hot_switch(state, variant, &ladder, "Manual ABR");
            } else {
                state.show_toast("Already at lowest available stream tier", unix_now());
            }
            return;
        }
    }
    if let Some(ref anime) = state.selected_anime.clone() {
        state.live_playback.abr_refresh_in_flight = true;
        let title = anime.playback_query();
        let mal_id = anime.mal_id;
        let net_mode = state.network_mode;
        let force_v4 = cfg.force_ipv4;
        let tx2 = tx.clone();
        tokio::spawn(async move {
            let info = api::subtitles::fetch_stream_info_with_network(
                anime_id,
                mal_id,
                &title,
                ep,
                is_dub,
                force_v4,
                net_mode.network_timeout_secs(),
            )
            .await;
            let _ = tx2
                .send(AppMessage::AbrRefreshReady {
                    anime_id,
                    episode: ep,
                    is_dub,
                    info,
                })
                .await;
        });
        state.show_toast("Refreshing stream ladder for downshift...", unix_now());
    }
}

fn execute_abr_hot_switch(
    state: &mut AppState,
    variant: api::subtitles::ResolvedVideoVariant,
    ladder: &api::subtitles::ResolvedStreamLadder,
    origin: &str,
) {
    let anime_title = state
        .selected_anime
        .as_ref()
        .map(|a| a.display_title().to_string())
        .unwrap_or_else(|| "Solus".to_string());
    let osd_label = format!("{origin}: {}p (~{} kbps)", variant.height, variant.tbr_kbps);
    let req = services::ipc::HotSwitchRequest {
        new_url: variant.url.clone(),
        resume_pos: state.live_playback.last_time_pos,
        audio_file: ladder.audio_uri_for(&variant, state.live_playback.is_dub),
        sub_url: state.last_play_options.as_ref().and_then(|o| o.sub_url.clone()),
        subtitles_disabled: state
            .last_play_options
            .as_ref()
            .is_some_and(|o| o.subtitles_disabled),
        sub_delay_ms: state.subtitle_delay_ms,
        media_title: format!("{} - Episode {}", anime_title, state.live_playback.episode),
        osd_label: osd_label.clone(),
        referer: state.last_play_options.as_ref().and_then(|o| o.referer.clone()),
    };
    if services::ipc::send_hot_switch_stream(&req) {
        state.live_playback.current_height = variant.height;
        state.live_playback.current_fps_x10 = variant.fps_x10;
        state.live_playback.current_tbr_kbps = variant.tbr_kbps;
        state.live_playback.current_url = variant.url;
        state.live_playback.last_abr_switch_at = Some(std::time::Instant::now());
        state.live_playback.abr_downshift_count =
            state.live_playback.abr_downshift_count.saturating_add(1);
        state.live_playback.last_abr_note = Some(osd_label.clone());
        sync_remote_telemetry_snapshot(state);
        state.show_toast(format!("◈ {osd_label}"), unix_now());
    }
}

/// Spawn a background cover fetch from SQLite/HTTP by `anime_id` when raw artwork is missing.
fn trigger_cover_fetch_by_id(
    state: &mut AppState,
    anime_id: i64,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.cover_raw_cache.contains_key(&anime_id) || !state.loading_covers.insert(anime_id) {
        return;
    }
    let pool_c = pool.clone();
    let tx_c = tx.clone();
    tokio::spawn(async move {
        if let Ok(Some(anime)) = db::cache::get_anime(&pool_c, anime_id).await {
            match download_cover_image(&anime, &pool_c).await {
                Some(img) => { let _ = tx_c.send(AppMessage::CoverReady(anime_id, img)).await; }
                None => { let _ = tx_c.send(AppMessage::CoverFailed(anime_id)).await; }
            }
        } else {
            let _ = tx_c.send(AppMessage::CoverFailed(anime_id)).await;
        }
    });
}

/// Standard inner image cell dimensions (`16 x 11` portrait) for pre-encoding card & hero covers off-thread.
fn home_card_preencode_area() -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(
        0,
        0,
        ui::home::CARD_WIDTH.saturating_sub(2),
        ui::home::CARD_FRAME_HEIGHT.saturating_sub(2),
    )
}

/// Return true when Home card and banner protocols are cached for `anime_id`.
fn has_home_cached_covers(state: &AppState, anime_id: i64) -> bool {
    state.cover_cache.contains_key(&anime_id) && state.banner_cache.contains_key(&anime_id)
}

/// Return true when card, banner, and detail image protocols are already cached for `anime_id`.
fn has_all_cached_covers(state: &AppState, anime_id: i64) -> bool {
    has_home_cached_covers(state, anime_id) && state.detail_cache.contains_key(&anime_id)
}

/// Insert card, banner, and detail protocols only when absent so active Kitty image IDs stay stable.
fn insert_cover_protocols_if_absent(
    state: &mut AppState,
    anime_id: i64,
    card_img: image::DynamicImage,
    portrait_img: image::DynamicImage,
) {
    let Some(picker) = state.picker else {
        return;
    };
    state
        .cover_cache
        .entry(anime_id)
        .or_insert_with(|| build_stateful_protocol(picker, card_img.clone()));
    state
        .banner_cache
        .entry(anime_id)
        .or_insert_with(|| build_stateful_protocol(picker, card_img));
    state
        .detail_cache
        .entry(anime_id)
        .or_insert_with(|| build_stateful_protocol(picker, portrait_img));
}

/// Fetch cover artwork only when missing from caches and not already in flight.
fn ensure_cover_fetch(
    state: &mut AppState,
    anime: &db::cache::Anime,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if !has_all_cached_covers(state, anime.id) && state.loading_covers.insert(anime.id) {
        trigger_cover_download(anime.clone(), pool.clone(), tx.clone());
    }
}

/// Collect every unique anime across all Home shelves in visual top-to-bottom priority order.
fn collect_unique_home_anime(data: &ui::home::HomeData) -> Vec<db::cache::Anime> {
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();

    if let Some(ref featured) = data.featured {
        if seen.insert(featured.id) {
            ordered.push(featured.clone());
        }
    }

    for shelf in [
        &data.continue_watching,
        &data.watchlist,
        &data.trending,
        &data.popular,
        &data.top_rated,
        &data.seasonal,
    ] {
        for anime in shelf {
            if seen.insert(anime.id) {
                ordered.push(anime.clone());
            }
        }
    }

    ordered
}

/// Spawn parallel preloading for every uncached Home cover while gating behind the loading screen.
fn start_home_preload(
    data: &ui::home::HomeData,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(picker) = state.picker else {
        state.is_loading = false;
        return;
    };

    let pending: Vec<db::cache::Anime> = collect_unique_home_anime(data)
        .into_iter()
        .filter(|anime| !has_home_cached_covers(state, anime.id) && state.loading_covers.insert(anime.id))
        .collect();

    let gen = state.begin_preload(pending.len());
    if pending.is_empty() {
        refresh_home_cover(state, data, pool, tx);
        return;
    }

    for anime in pending {
        trigger_home_cover_preload(anime, pool.clone(), tx.clone(), picker, Some(gen));
    }
}

fn refresh_home_cover(
    state: &mut AppState,
    data: &ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    tx:   &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(anime) = active_anime(state, data).or_else(|| data.featured.clone()) else {
        return;
    };

    if state.cover_anime_id == Some(anime.id) {
        if state.banner_progress.is_none() {
            trigger_banner_progress(anime.id, pool.clone(), tx.clone());
        }
        return;
    }

    let anime_id = anime.id;
    state.cover_anime_id = Some(anime_id);
    state.cover_state    = None;
    state.cover_failed_anime_id = None;
    state.banner_progress = None;
    if !has_home_cached_covers(state, anime_id) && state.loading_covers.insert(anime_id) {
        if let Some(picker) = state.picker {
            trigger_home_cover_preload(anime, pool.clone(), tx.clone(), picker, None);
        } else {
            trigger_cover_download(anime, pool.clone(), tx.clone());
        }
    }
    trigger_banner_progress(anime_id, pool.clone(), tx.clone());
}

fn prefetch_all_home_covers(
    data: &ui::home::HomeData,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    for anime in collect_unique_home_anime(data) {
        if !has_home_cached_covers(state, anime.id) && state.loading_covers.insert(anime.id) {
            if let Some(picker) = state.picker {
                trigger_home_cover_preload(anime, pool.clone(), tx.clone(), picker, None);
            } else {
                trigger_cover_download(anime, pool.clone(), tx.clone());
            }
        }
    }
}

/// Spawn a non-blocking background update check (and auto-install when enabled).
fn spawn_update_check(
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
    auto_install: bool,
    check_interval: u64,
    respect_interval: bool,
) {
    let tx_update = tx.clone();
    let pool_update = pool.clone();
    tokio::spawn(async move {
        let now = unix_now();
        let should_check = if respect_interval && check_interval > 0 {
            match db::sync::last_synced(&pool_update, "update_check").await {
                Ok(Some(last)) => (now - last) >= check_interval as i64,
                _ => true,
            }
        } else {
            true
        };

        if !should_check {
            return;
        }

        let client = reqwest::Client::new();
        let current_ver = env!("CARGO_PKG_VERSION");
        if let Ok(update_opt) = services::updater::check_for_updates(&client, current_ver).await {
            let _ = db::sync::mark_synced(&pool_update, "update_check", now).await;
            if let Some(ref info) = update_opt {
                if auto_install && !info.is_homebrew {
                    services::updater::record_pending_post_update_welcome(info);
                    let download_url = info.download_url.clone();
                    let target_ver = info.latest_version.clone();
                    let tx_dl = tx_update.clone();
                    tokio::spawn(async move {
                        if let Err(e) = services::updater::download_and_install_update(
                            &client,
                            &download_url,
                            &target_ver,
                            tx_dl.clone(),
                        )
                        .await
                        {
                            let _ = tx_dl.send(AppMessage::UpdateFailed(e.to_string())).await;
                        }
                    });
                }
            }
            let _ = tx_update.send(AppMessage::UpdateCheckResult(update_opt)).await;
        }
    });
}

/// Detect runtime player & CLI dependencies in a blocking worker without slowing startup UI render.
fn spawn_dependency_check(
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
    prompt_setup: bool,
) {
    let tx_dep = tx.clone();
    tokio::spawn(async move {
        let deps = tokio::task::spawn_blocking(api::player::detect_dependencies)
            .await
            .unwrap_or_default();
        let _ = tx_dep
            .send(AppMessage::DependenciesReady {
                ani_cli: deps.has_ani_cli,
                mpv: deps.has_mpv,
                mpvnet: deps.has_mpvnet,
                celluloid: deps.has_celluloid,
                prompt_setup,
            })
            .await;
    });
}

fn trigger_banner_progress(
    anime_id: i64,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let watched = db::user::get_watched_episodes(&pool, anime_id)
            .await
            .map(|eps| eps.len())
            .unwrap_or(0);
        let _ = tx.send(AppMessage::BannerProgress(anime_id, watched)).await;
    });
}

fn refresh_dependency_status(state: &mut AppState) {
    let deps = api::player::detect_dependencies();
    state.set_dependencies(deps.has_ani_cli, deps.has_mpv, deps.has_mpvnet, deps.has_celluloid);
}

fn quality_label(idx: usize) -> &'static str {
    match QUALITY_CHOICES.get(idx) {
        Some(quality) => quality.as_str(),
        None => config::Quality::P1080.as_str(),
    }
}

fn quality_index(quality: &config::Quality) -> usize {
    QUALITY_CHOICES
        .iter()
        .position(|candidate| candidate == quality)
        .unwrap_or(1)
}

/// Item count for a given category row.
fn row_len(row: &state::CategoryRow, data: &ui::home::HomeData) -> usize {
    use state::CategoryRow::*;
    match row {
        ContinueWatching => data.continue_watching.len(),
        Watchlist        => data.watchlist.len(),
        Trending         => data.trending.len(),
        Popular          => data.popular.len(),
        TopRated         => data.top_rated.len(),
        Seasonal         => data.seasonal.len(),
    }
}

/// Walk the row order in `direction` (+1 = down, -1 = up) and return the first
/// row that has at least one item.  Returns `current` unchanged if no such row
/// exists in that direction (i.e., already at boundary or all rows are empty).
fn next_non_empty_row(
    current:   &state::CategoryRow,
    data:      &ui::home::HomeData,
    direction: i32,
) -> state::CategoryRow {
    use state::CategoryRow::*;
    let order: &[state::CategoryRow] = &[
        ContinueWatching, Watchlist, Trending, Popular, TopRated, Seasonal,
    ];
    let pos = order.iter().position(|r| r == current).unwrap_or(2) as i32;
    let mut i = pos + direction;
    while i >= 0 && i < order.len() as i32 {
        let candidate = &order[i as usize];
        if row_len(candidate, data) > 0 {
            return candidate.clone();
        }
        i += direction;
    }
    current.clone()
}

// ── Detail screen ─────────────────────────────────────────────────────────────

async fn handle_detail(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &mut config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.is_download_modal_open {
        handle_download_modal(key, state, pool, cfg, tx).await;
        return;
    }

    if state.is_batch_queue_modal_open {
        handle_batch_queue_modal(key, state, cfg, tx).await;
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.go_back();
            if state.screen == Screen::Detail {
                if let Some(anime) = state.selected_anime.clone() {
                    let in_wl = db::user::is_in_watchlist(pool, anime.id).await.unwrap_or(false);
                    let watched = db::user::get_watched_episodes(pool, anime.id).await.unwrap_or_default();
                    state.in_watchlist = in_wl;
                    state.set_watched_episodes(watched.into_iter().map(|e| e as u32).collect());
                    if state.cover_anime_id != Some(anime.id) || state.cover_state.is_none() {
                        state.cover_anime_id = Some(anime.id);
                        state.cover_state = None;
                        state.cover_failed_anime_id = None;
                        ensure_cover_fetch(state, &anime, pool, tx);
                    }
                }
            }
        }
        KeyCode::Char('/')                => state.open_search(),
        KeyCode::Char('?')               => state.open_help(),
        KeyCode::Char('s')               => state.open_settings(),
        KeyCode::Char('t') | KeyCode::Char('T') => cycle_active_theme(state, pool, cfg),
        KeyCode::Char('m') | KeyCode::Char('M') => {
            state.overlay_base = Screen::Detail;
            if state.remote_qr.is_none() {
                state.remote_qr = services::remote::generate_terminal_qr(&state.remote_url);
            }
            state.screen = Screen::RemoteModal;
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.overlay_base = Screen::Detail;
            if state.cast_devices.is_empty() {
                state.cast_devices = services::cast::scan_network(500).await;
            }
            state.screen = Screen::CastModal;
        }
        KeyCode::Char('!')               => {
            refresh_dependency_status(state);
            state.open_setup();
        }

        // Toggle episode view layout: Option 1 (List) ↔ Option 3 (Split Deck)
        KeyCode::Char('v') | KeyCode::Char('V') | KeyCode::Tab => {
            state.episode_view_mode = match state.episode_view_mode {
                state::EpisodeViewMode::List => state::EpisodeViewMode::Split,
                state::EpisodeViewMode::Split => state::EpisodeViewMode::List,
            };
        }

        // Episode navigation
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('l') | KeyCode::Right => {
            if let Some(ep) = state.selected_episode {
                let max = state.episode_list.last().copied().unwrap_or(1);
                if ep < max {
                    state.selected_episode = Some(ep + 1);
                    preload_nearby_episode_thumbnails(state, tx);
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up | KeyCode::Char('h') | KeyCode::Left => {
            if let Some(ep) = state.selected_episode {
                if ep > 1 {
                    state.selected_episode = Some(ep - 1);
                    preload_nearby_episode_thumbnails(state, tx);
                }
            }
        }
        KeyCode::Char('d') | KeyCode::PageDown => {
            if let Some(ep) = state.selected_episode {
                let max = state.episode_list.last().copied().unwrap_or(1);
                let target = (ep + 4).min(max);
                state.selected_episode = Some(target);
                preload_nearby_episode_thumbnails(state, tx);
            }
        }
        KeyCode::Char('u') | KeyCode::PageUp => {
            if let Some(ep) = state.selected_episode {
                let target = ep.saturating_sub(4).max(1);
                state.selected_episode = Some(target);
                preload_nearby_episode_thumbnails(state, tx);
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            state.selected_episode = Some(1);
            state.episode_offset = 0;
            preload_nearby_episode_thumbnails(state, tx);
        }
        KeyCode::Char('G') | KeyCode::End => {
            let max_aired = state
                .selected_anime
                .as_ref()
                .map(|a| a.aired_episodes())
                .unwrap_or(1);
            state.selected_episode = Some(max_aired);
            preload_nearby_episode_thumbnails(state, tx);
        }

        // Play options overlay for the highlighted episode
        KeyCode::Char('p') | KeyCode::Char('P') => {
            begin_playback_flow(state, pool, cfg, true, tx).await;
        }

        // Open offline download modal
        KeyCode::Char('D') => {
            state.is_download_modal_open = true;
            state.download_modal_mode = 0;
            state.download_range_input.clear();
        }

        KeyCode::Char('Q') => {
            open_queue_hub(state, pool).await;
        }
        KeyCode::Char('N') => {
            cycle_network_mode(state, cfg, true, tx).await;
        }

        KeyCode::Char('a') | KeyCode::Char('A') => {
            if state.now_playing.is_some() {
                cycle_active_playback_audio(state, pool, cfg, tx).await;
            }
        }
        KeyCode::Char('[') | KeyCode::Char('-') if state.now_playing.is_some() => {
            adjust_subtitle_delay(state, pool, Some(-100)).await;
        }
        KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') if state.now_playing.is_some() => {
            adjust_subtitle_delay(state, pool, Some(100)).await;
        }
        KeyCode::Char('0') if state.now_playing.is_some() => {
            adjust_subtitle_delay(state, pool, None).await;
        }

        KeyCode::Char('e') => {
            open_episode_options_flow(
                state,
                pool,
                cfg,
                false,
                state::PlaybackOptionsPurpose::EnqueueSingle,
                tx,
            )
            .await;
        }

        KeyCode::Char('E') => {
            if let Some(ref anime) = state.selected_anime {
                let default_audio = db::user::get_audio_mode(pool, anime.id)
                    .await
                    .unwrap_or(None)
                    .unwrap_or_else(|| cfg.audio_mode.clone());
                let dub = default_audio == config::AudioMode::Dub;
                state.open_batch_queue_modal_with_prefs(
                    dub,
                    cfg.quality.as_str(),
                    &cfg.default_sub_lang,
                );
            }
        }

        // Install available update if present
        KeyCode::Char('U') => {
            if let Some(ref info) = state.available_update.clone() {
                state.open_update_modal(info.clone());
            } else if !state.is_checking_update {
                state.is_checking_update = true;
                state.show_toast("Checking GitHub for updates...", unix_now());
                let tx_check = tx.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    let current_ver = env!("CARGO_PKG_VERSION");
                    let res = services::updater::check_for_updates(&client, current_ver).await.unwrap_or(None);
                    let _ = tx_check.send(AppMessage::UpdateCheckResult(res)).await;
                });
            }
        }

        // Toggle Watched status for the highlighted episode
        KeyCode::Char('w') | KeyCode::Char('W') => {
            if let (Some(anime_clone), Some(ep)) = (state.selected_anime.clone(), state.selected_episode) {
                let anime_id = anime_clone.id;
                let is_watched = state.watched_episodes.contains(&ep);
                let pool2 = pool.clone();
                let tx2 = tx.clone();
                let now = unix_now();
                if is_watched {
                    state.watched_episodes.remove(&ep);
                    state.show_toast(format!("Episode {} marked unwatched", ep), now);
                    tokio::spawn(async move {
                        let _ = db::user::remove_watched_episode(&pool2, anime_id, ep as i64).await;
                    });
                } else {
                    state.watched_episodes.insert(ep);
                    state.show_toast(format!("󰄬 Episode {} marked watched", ep), now);
                    let mut auth_clone = state.auth.clone();
                    let trackers_cfg = cfg.trackers.clone();
                    tokio::spawn(async move {
                        let _ = db::user::record_watched(&pool2, anime_id, ep as i64, now).await;
                        if auth_clone.is_authenticated() {
                            let outcome = services::scrobble::sync_episode_watched_all(
                                &pool2,
                                &anime_clone,
                                ep,
                                &mut auth_clone,
                                &trackers_cfg,
                                now,
                            )
                            .await;
                            if outcome.anilist_synced || outcome.mal_synced || !outcome.warnings.is_empty() {
                                let _ = tx2.send(AppMessage::ScrobbleSyncToast(outcome.summary)).await;
                            }
                        }
                    });
                }
            }
        }

        // Play highlighted episode
        KeyCode::Enter => {
            begin_playback_flow(state, pool, cfg, false, tx).await;
        }

        KeyCode::Char('n') => {
            if state.detail_focus == state::DetailFocus::Episodes {
                let current = state.selected_episode.unwrap_or(1);
                let max_aired = state
                    .selected_anime
                    .as_ref()
                    .map(|a| a.aired_episodes())
                    .unwrap_or(u32::MAX);
                if current < max_aired {
                    state.selected_episode = Some(current + 1);
                    begin_playback_flow(state, pool, cfg, false, tx).await;
                } else {
                    state.show_toast(
                        format!("Episode {} is the latest released episode", current),
                        unix_now(),
                    );
                }
            }
        }

        KeyCode::Char('x') | KeyCode::Char('X') => {
            if state.now_playing.is_some() {
                state.queue.manual_stop = true;
                state.stop_player();
                state.show_toast("Playback stopped", unix_now());
            }
        }

        KeyCode::Delete | KeyCode::Backspace => {
            if let Some(anime) = state.selected_anime.clone() {
                let anime_id = anime.id;
                let title = anime.display_title().to_string();
                let now = unix_now();
                state.watched_episodes.clear();
                state.in_watchlist = false;
                state.show_toast(format!("󰅖 Removed \"{}\" from library", title), now);

                let pool2 = pool.clone();
                let tx2 = tx.clone();
                let mut auth_clone = state.auth.clone();
                let trackers_cfg = cfg.trackers.clone();
                tokio::spawn(async move {
                    let _ = db::user::remove_continue_watching(&pool2, anime_id).await;
                    let _ = services::scrobble::remove_watchlist_from_target(
                        &pool2,
                        &anime,
                        config::WatchlistTarget::Both,
                        &mut auth_clone,
                        &trackers_cfg,
                        now,
                    )
                    .await;
                    if let Ok(cw) = services::sync::load_continue_watching(&pool2).await {
                        let _ = tx2.send(AppMessage::ContinueWatchingUpdated(cw)).await;
                    }
                    broadcast_watchlist(&pool2, &tx2).await;
                });
            }
        }

        // Watchlist / Status label picker — updates status or removes across connected trackers
        KeyCode::Char('+') => {
            if let Some(anime) = state.selected_anime.clone() {
                trigger_watchlist_status_picker_flow(anime, true, None, state, pool).await;
            }
        }

        _ => {}
    }
}

async fn handle_download_modal(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc => {
            state.is_download_modal_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.download_modal_mode = (state.download_modal_mode + 1).min(2);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.download_modal_mode = state.download_modal_mode.saturating_sub(1);
        }
        KeyCode::Backspace => {
            if state.download_modal_mode == 1 {
                state.download_range_input.pop();
            }
        }
        KeyCode::Char(c) if state.download_modal_mode == 1 && (c.is_ascii_digit() || c == '-') => {
            state.download_range_input.push(c);
        }
        KeyCode::Enter => {
            let Some(anime) = state.selected_anime.clone() else {
                state.is_download_modal_open = false;
                return;
            };
            let max_aired = anime.aired_episodes();
            let episodes = match state.download_modal_mode {
                0 => vec![state.selected_episode.unwrap_or(1)],
                1 => services::downloader::parse_episode_range(&state.download_range_input, max_aired, &[]),
                2 => (1..=max_aired).filter(|ep| !state.watched_episodes.contains(ep)).collect(),
                _ => vec![],
            };
            state.is_download_modal_open = false;
            if episodes.is_empty() {
                state.show_toast("No episodes matched download selection", unix_now());
            } else {
                state.show_toast(format!("󰇚 Queued {} episode(s) for offline download", episodes.len()), unix_now());
                queue_episodes_for_download(
                    anime,
                    episodes,
                    cfg.default_sub_lang.clone(),
                    state.network_mode,
                    cfg.force_ipv4,
                    pool.clone(),
                    tx.clone(),
                );
            }
        }
        _ => {}
    }
}

fn queue_episodes_for_download(
    anime: db::cache::Anime,
    episodes: Vec<u32>,
    preferred_sub: String,
    network_mode: config::NetworkMode,
    force_ipv4: bool,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let anime_title = anime.display_title().to_string();
        let query = anime.playback_query();
        let mut tasks = Vec::new();

        for ep in episodes {
            let stream_info = api::subtitles::fetch_stream_info_with_network(
                anime.id,
                anime.mal_id,
                &query,
                ep,
                false,
                force_ipv4,
                network_mode.network_timeout_secs(),
            )
            .await;
            let stream_url = stream_info
                .qualities
                .first()
                .map(|q| q.url.clone())
                .or(stream_info.master_url);

            if let Some(s_url) = stream_url {
                let sub_url = stream_info
                    .subtitles
                    .iter()
                    .find(|s| s.code.eq_ignore_ascii_case(&preferred_sub) || s.stream_label.contains(&preferred_sub))
                    .or_else(|| stream_info.subtitles.first())
                    .map(|s| s.url.clone());

                tasks.push(services::downloader::DownloadTask {
                    anime_id: anime.id,
                    anime_title: anime_title.clone(),
                    episode: ep,
                    stream_url: s_url,
                    sub_url,
                    force_ipv4,
                });
            }
        }

        if !tasks.is_empty() {
            services::downloader::spawn_batch_downloader(tasks, pool, tx);
        }
    });
}

// ── Playback query overlay ─────────────────────────────────────────────────────

async fn handle_playback_query(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.go_back(),
        KeyCode::Down | KeyCode::Char('j') => {
            if state.playback_query_cursor + 1 < state.playback_queries.len() {
                state.playback_query_cursor += 1;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.playback_query_cursor = state.playback_query_cursor.saturating_sub(1);
        }
        KeyCode::Enter => {
            if let Some(anime) = state.selected_anime.clone() {
                let query = state
                    .playback_queries
                    .get(state.playback_query_cursor)
                    .cloned()
                    .unwrap_or_else(|| anime.playback_query());
                let _ = db::user::set_playback_query(pool, anime.id, &query, unix_now()).await;
                let preferred_audio = db::user::get_audio_mode(pool, anime.id)
                    .await
                    .unwrap_or(None)
                    .unwrap_or_else(|| cfg.audio_mode.clone());
                let is_dub = preferred_audio == config::AudioMode::Dub;
                let default_player = state.selected_player();
                let purpose = state.playback_options_purpose;
                state.open_playback_options(
                    query,
                    quality_index(&cfg.quality),
                    is_dub,
                    default_player,
                );
                state.playback_options_purpose = purpose;
                let ep = state.selected_episode.unwrap_or(1);
                trigger_stream_detection(&anime, ep, is_dub, state.playback_request_generation, tx);
            }
        }
        _ => {}
    }
}

async fn persist_profile_playback_settings_fields(state: &AppState, pool: &sqlx::SqlitePool) {
    let quality = state
        .playback_qualities
        .get(state.playback_quality_cursor)
        .map(|q| q.label.clone())
        .unwrap_or_else(|| "1080p".to_string());
    let settings = db::user::ProfilePlaybackSettings {
        profile_id: state.active_profile_id.clone(),
        dub: state.pending_dub,
        subtitle_code: state.preferred_subtitle_code.clone(),
        subtitle_delay_ms: state.subtitle_delay_ms.clamp(-120_000, 120_000),
        quality,
        preferred_player: state.selected_player().as_str().to_string(),
    };
    let _ = db::user::save_profile_playback_settings(pool, &settings, unix_now()).await;
}

async fn sync_provider_delay_for_cursor(state: &mut AppState, pool: &sqlx::SqlitePool) {
    let Some(anime_id) = state.selected_anime.as_ref().map(|a| a.id) else {
        return;
    };
    let Some(prov_id) = state
        .playback_providers
        .get(state.playback_provider_cursor)
        .map(|p| p.id.clone())
    else {
        return;
    };
    state.active_provider_id = Some(prov_id.clone());
    if let Some(saved_ms) = db::user::get_anime_provider_subtitle_delay_ms(
        pool,
        &state.active_profile_id,
        anime_id,
        &prov_id,
    )
    .await
    {
        state.subtitle_delay_ms = saved_ms;
    } else {
        state.subtitle_delay_ms = 0;
    }
}

async fn adjust_subtitle_delay(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    delta_ms: Option<i32>,
) {
    let target_ms = match delta_ms {
        Some(d) => state.subtitle_delay_ms.saturating_add(d).clamp(-120_000, 120_000),
        None => 0,
    };
    if state.now_playing.is_some() {
        let sock = services::ipc::get_active_socket_path()
            .unwrap_or_else(|| "/tmp/solus-mpv.sock".to_string());
        let delay_secs = target_ms as f64 / 1000.0;
        let cmd = serde_json::json!({ "command": ["set_property", "sub-delay", delay_secs] });
        if !services::remote::send_mpv_json_command(&sock, cmd).await {
            state.show_toast("Subtitle delay requires active mpv IPC connection", unix_now());
            return;
        }
    }
    state.subtitle_delay_ms = target_ms;
    if let Some(ref mut opts) = state.last_play_options {
        opts.sub_delay_ms = target_ms;
    }
    if let Some(ref anime) = state.selected_anime {
        let prov_id = state
            .playback_providers
            .get(state.playback_provider_cursor)
            .map(|p| p.id.as_str())
            .or(state.active_provider_id.as_deref())
            .unwrap_or("");
        let _ = db::user::save_anime_provider_subtitle_delay_ms(
            pool,
            &state.active_profile_id,
            anime.id,
            prov_id,
            target_ms,
            unix_now(),
        )
        .await;
    }
    persist_profile_playback_settings_fields(state, pool).await;
    let delay_secs = state.subtitle_delay_secs();
    let r_state = state.remote_state.clone();
    tokio::spawn(async move {
        {
            let mut st = r_state.write().await;
            st.sub_delay = delay_secs;
        }
        services::remote::broadcast_state(&r_state).await;
    });
    state.show_toast(
        format!("Subtitle delay: {}", state.format_subtitle_delay()),
        unix_now(),
    );
}

async fn handle_playback_options(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &mut config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.go_back(),
        KeyCode::Tab => {
            state.playback_focus_column = (state.playback_focus_column + 1) % 4;
            state.playback_sub_focused = state.playback_focus_column == 3;
        }
        KeyCode::BackTab => {
            state.playback_focus_column = if state.playback_focus_column == 0 { 3 } else { state.playback_focus_column - 1 };
            state.playback_sub_focused = state.playback_focus_column == 3;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.playback_focus_column = state.playback_focus_column.saturating_sub(1);
            state.playback_sub_focused = state.playback_focus_column == 3;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if state.playback_focus_column < 3 {
                state.playback_focus_column += 1;
                state.playback_sub_focused = state.playback_focus_column == 3;
            }
        }
        KeyCode::Char('1') => {
            state.playback_focus_column = 0;
            state.playback_sub_focused = false;
        }
        KeyCode::Char('2') => {
            state.playback_focus_column = 1;
            state.playback_sub_focused = false;
        }
        KeyCode::Char('3') => {
            state.playback_focus_column = 2;
            state.playback_sub_focused = false;
        }
        KeyCode::Char('4') => {
            state.playback_focus_column = 3;
            state.playback_sub_focused = true;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match state.playback_focus_column {
                0 => {
                    if state.playback_player_cursor + 1 < state.playback_players.len() {
                        state.playback_player_cursor += 1;
                    }
                }
                1 => {
                    if state.playback_provider_cursor + 1 < state.playback_providers.len() {
                        state.playback_provider_cursor += 1;
                        state.sync_active_provider();
                        sync_provider_delay_for_cursor(state, pool).await;
                    }
                }
                2 => {
                    if state.playback_quality_cursor + 1 < state.playback_qualities.len() {
                        state.playback_quality_cursor += 1;
                    }
                }
                _ => {
                    if state.playback_sub_cursor + 1 < state.playback_sub_languages.len() {
                        state.playback_sub_cursor += 1;
                    }
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match state.playback_focus_column {
                0 => {
                    state.playback_player_cursor = state.playback_player_cursor.saturating_sub(1);
                }
                1 => {
                    state.playback_provider_cursor = state.playback_provider_cursor.saturating_sub(1);
                    state.sync_active_provider();
                    sync_provider_delay_for_cursor(state, pool).await;
                }
                2 => {
                    state.playback_quality_cursor = state.playback_quality_cursor.saturating_sub(1);
                }
                _ => {
                    state.playback_sub_cursor = state.playback_sub_cursor.saturating_sub(1);
                }
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') | KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.cycle_playback_player();
            state.show_toast(format!("Player: {}", state.selected_player().as_str()), unix_now());
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if !state.pending_dub && state.dub_available == Some(false) {
                state.show_toast("English Dub is not available for this anime (Sub only)", unix_now());
                return;
            }
            state.pending_dub = !state.pending_dub;
            state.is_loading_subs = true;
            if let Some(ref anime) = state.selected_anime {
                let anime_c = anime.clone();
                let ep = state.selected_episode.unwrap_or(1);
                let dub = state.pending_dub;
                let req_gen = state.bump_playback_request_generation();
                trigger_stream_detection(&anime_c, ep, dub, req_gen, tx);
            }
        }
        KeyCode::Char('[') | KeyCode::Char('-') => {
            adjust_subtitle_delay(state, pool, Some(-100)).await;
        }
        KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') => {
            adjust_subtitle_delay(state, pool, Some(100)).await;
        }
        KeyCode::Char('0') => {
            adjust_subtitle_delay(state, pool, None).await;
        }
        KeyCode::Char('u') | KeyCode::Char('U') => {
            state.toggle_anime4k();
            state.show_toast(format!("Anime4K: {}", state.anime4k_mode.as_str()), unix_now());
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            cycle_network_mode(state, cfg, true, tx).await;
        }
        KeyCode::Char('w') => {
            toggle_pip_mode_action(state, cfg);
        }
        KeyCode::Char('W') => {
            cycle_pip_corner_action(state, cfg);
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            toggle_auto_fullscreen_action(state, cfg);
        }
        KeyCode::Char('i') => {
            toggle_info_hud_action(state, false);
        }
        KeyCode::Char('I') => {
            toggle_info_hud_action(state, true);
        }
        KeyCode::Char('b') => {
            state.cycle_hardsub_shield_mode();
            state.show_toast(
                format!("◈ Hardsub Eradicator: {}", state.hardsub_shield_mode.label()),
                unix_now(),
            );
        }
        KeyCode::Char('B') => {
            state.cycle_hardsub_band_height();
            state.show_toast(
                format!("◈ Hardsub Band Height: {}", state.hardsub_band_height.label()),
                unix_now(),
            );
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            let force_enqueue = matches!(key.code, KeyCode::Char('e'));
            if let Some(anime) = state.selected_anime.clone() {
                let ep    = state.selected_episode.unwrap_or(1);
                let title = anime.display_title().to_string();
                let query = state
                    .pending_playback_query
                    .clone()
                    .unwrap_or_else(|| anime.playback_query());
                let audio = if state.pending_dub {
                    config::AudioMode::Dub
                } else {
                    config::AudioMode::Sub
                };

                let chosen_provider = state.playback_providers.get(state.playback_provider_cursor);
                if state.pending_dub {
                    if let Some(prov) = chosen_provider {
                        if !prov.supports_dub {
                            state.show_toast(format!("{} does not support English Dub. Please switch to Sub.", prov.name), unix_now());
                            return;
                        }
                    }
                }

                let _ = db::user::set_audio_mode(pool, anime.id, audio.clone(), unix_now()).await;

                let chosen_provider = state.playback_providers.get(state.playback_provider_cursor);
                let chosen_sub = state.playback_sub_languages.get(state.playback_sub_cursor);
                let subtitles_disabled = chosen_sub.is_some_and(|s| s.is_none());
                let sub_lang = if subtitles_disabled {
                    Some("none".to_string())
                } else {
                    chosen_sub.map(|s| s.lang_keyword().to_string())
                };
                let sub_url = if subtitles_disabled {
                    None
                } else {
                    chosen_sub.map(|s| s.url.clone()).filter(|u| !u.is_empty())
                };

                let chosen_qual = state.playback_qualities.get(state.playback_quality_cursor);
                let quality_label = chosen_qual
                    .map(|q| q.label.clone())
                    .unwrap_or_else(|| quality_label(state.playback_quality_cursor).to_string());
                let mut stream_url = chosen_qual.map(|q| q.url.clone()).filter(|u| !u.is_empty());

                // If user selected AnimePahe or GogoAnime (ani-cli provider scrapers), stream_url is None
                // so ani-cli executes with its native multi-source engine.
                // If ZokoAnime or HD-1 is selected, it uses direct verified master HLS stream.
                if let Some(prov) = chosen_provider {
                    if prov.id == "pahe" || prov.id == "gogo" {
                        stream_url = None;
                    } else if let Some(ref m_url) = prov.master_url {
                        if stream_url.is_none() {
                            stream_url = Some(m_url.clone());
                        }
                    }
                }

                let chosen_player = state.selected_player();
                if chosen_player == config::Player::AutoBest && !state.has_any_player() {
                    state.show_toast("No libmpv player found (mpv / mpv.net / Celluloid). Press ! for setup.", unix_now());
                    return;
                }
                if chosen_player == config::Player::Mpv && !state.has_mpv {
                    state.show_toast("mpv is not installed. Press ! to check setup or select Auto-Best.", unix_now());
                    return;
                }
                if chosen_player == config::Player::MpvNet && (!cfg!(windows) || !state.has_mpvnet) {
                    state.show_toast("mpv.net requires Windows with mpvnet installed. Select Auto-Best or mpv.", unix_now());
                    return;
                }
                if chosen_player == config::Player::Celluloid && (!cfg!(target_os = "linux") || !state.has_celluloid) {
                    state.show_toast("Celluloid requires Linux with celluloid installed. Select Auto-Best or mpv.", unix_now());
                    return;
                }

                let effective_purpose = if force_enqueue {
                    state::PlaybackOptionsPurpose::EnqueueSingle
                } else {
                    state.playback_options_purpose
                };

                let chosen_sub_str = if subtitles_disabled {
                    "none".to_string()
                } else {
                    sub_lang
                        .clone()
                        .or_else(|| chosen_sub.map(|s| s.name.clone()))
                        .unwrap_or_else(|| cfg.default_sub_lang.clone())
                };
                let is_dub = audio == config::AudioMode::Dub;
                state.preferred_subtitle_code = if subtitles_disabled {
                    "none".to_string()
                } else if is_dub || !chosen_sub_str.eq_ignore_ascii_case(&cfg.default_sub_lang) {
                    chosen_sub_str.clone()
                } else {
                    String::new()
                };
                persist_profile_playback_settings_fields(state, pool).await;

                match effective_purpose {
                    state::PlaybackOptionsPurpose::EnqueueSingle => {
                        let mut item = build_queue_item_for_anime(&anime, ep, is_dub, cfg, state);
                        item.query = query;
                        item.quality = quality_label;
                        item.sub_lang = chosen_sub_str.clone();
                        item.player = chosen_player.as_str().to_string();
                        let mode_tag = if is_dub { "DUB" } else { "SUB" };
                        let outcome = state.queue.enqueue_back(item);
                        state.queue_cursor = state.queue.len().saturating_sub(1);
                        state.playback_options_purpose = state::PlaybackOptionsPurpose::LaunchNow;
                        persist_and_warm_queue(state, cfg, tx);
                        state.go_back();
                        state.show_toast(
                            format!(
                                "◆ Queued {} · Ep {} [{} · {}] (#{})",
                                title, ep, mode_tag, chosen_sub_str, outcome.position
                            ),
                            unix_now(),
                        );
                    }
                    state::PlaybackOptionsPurpose::EditQueueItem { uid } => {
                        let mode_tag = if is_dub { "DUB" } else { "SUB" };
                        let updated = state.queue.update_item_by_uid(uid, |item| {
                            item.query = query.clone();
                            item.dub = is_dub;
                            item.quality = quality_label.clone();
                            item.sub_lang = chosen_sub_str.clone();
                            item.player = chosen_player.as_str().to_string();
                        });
                        state.playback_options_purpose = state::PlaybackOptionsPurpose::LaunchNow;
                        persist_and_warm_queue(state, cfg, tx);
                        state.screen = Screen::QueueModal;
                        if updated {
                            state.show_toast(
                                format!(
                                    "◆ Updated Queue: Ep {} [{} · {} · {}]",
                                    ep, mode_tag, quality_label, chosen_sub_str
                                ),
                                unix_now(),
                            );
                        }
                    }
                    state::PlaybackOptionsPurpose::LaunchNow => {
                        let caps = services::platform::detect_player_capabilities();
                        let glsl_shaders = services::shaders::build_shader_arg_for_profile(
                            state.anime4k_mode,
                            cfg.performance_mode,
                            anime.season_year,
                            caps,
                        );
                        let chosen_prov = state
                            .playback_providers
                            .get(state.playback_provider_cursor)
                            .cloned();
                        let chosen_pid = chosen_prov.as_ref().map(|p| p.id.clone()).unwrap_or_default();
                        state.active_provider_id = (!chosen_pid.is_empty()).then_some(chosen_pid.clone());
                        if let Some(saved_prov_delay) = db::user::get_anime_provider_subtitle_delay_ms(
                            pool,
                            &state.active_profile_id,
                            anime.id,
                            &chosen_pid,
                        )
                        .await
                        {
                            state.subtitle_delay_ms = saved_prov_delay;
                        } else if !chosen_pid.is_empty() {
                            state.subtitle_delay_ms = 0;
                        }
                        let chosen_q_info = state.playback_qualities.get(state.playback_quality_cursor).cloned();
                        let needs_proxy = chosen_q_info.as_ref().is_some_and(|q| q.needs_hls_proxy)
                            || chosen_prov.as_ref().is_some_and(|p| p.needs_hls_proxy);
                        let audio_urls = chosen_q_info
                            .as_ref()
                            .map(|q| q.audio_urls.clone())
                            .unwrap_or_default();
                        let has_demuxed_audio = chosen_q_info
                            .as_ref()
                            .is_some_and(|q| q.has_demuxed_audio);
                        let mut opts = api::player::PlayOptions {
                            title:   query,
                            episode: ep,
                            quality: quality_label,
                            dub:     is_dub,
                            player:  chosen_player.as_str().to_string(),
                            sub_lang,
                            sub_url,
                            stream_url,
                            ipc_socket: None,
                            start_pos:  None,
                            glsl_shaders,
                            sub_style:  Some(cfg.subtitle_style.clone()),
                            sub_delay_ms: state.subtitle_delay_ms,
                            subtitles_disabled,
                            loading_image: None,
                            network_mode: state.network_mode,
                            force_ipv4: cfg.force_ipv4,
                            auto_fullscreen: cfg.auto_fullscreen,
                            auto_pip: cfg.auto_pip,
                            shader_mode: state.anime4k_mode,
                            performance_mode: cfg.performance_mode,
                            deband_mode: cfg.deband_mode,
                            motion_mode: cfg.motion_mode,
                            embedded_sub: chosen_prov
                                .as_ref()
                                .map(|p| p.embedded_sub)
                                .unwrap_or_default(),
                            hardsub_shield: state.hardsub_shield_mode,
                            hardsub_band: state.hardsub_band_height,
                            referer: chosen_prov.and_then(|p| p.referer),
                            audio_urls,
                            has_demuxed_audio,
                        };
                        wrap_opts_with_hls_proxy_if_needed(state, cfg, &mut opts, needs_proxy).await;
                        start_playback(state, opts, title, ep, tx, pool, cfg).await;
                    }
                }
            }
        }
        _ => {}
    }
}

// ── Search overlay ────────────────────────────────────────────────────────────

async fn handle_search(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc => {
            // Clear stale search state so it doesn't bleed through on next open
            state.search_query.clear();
            state.search_results.clear();
            state.search_cursor = 0;
            state.go_back();
        }

        KeyCode::Backspace => {
            state.search_query.pop();
            if state.search_query.is_empty() {
                state.search_results.clear();
                state.search_cursor = 0;
            } else {
                let query = state.search_query.clone();
                let pool2 = pool.clone();
                let tx2   = tx.clone();
                tokio::spawn(async move {
                    search_and_send(&pool2, &query, &tx2).await;
                });
            }
        }

        // Cursor movement uses arrows so letter keys still work for typing.
        KeyCode::Down => {
            if state.search_cursor + 1 < state.search_results.len() {
                state.search_cursor += 1;
                if let Some(anime) = state.search_results.get(state.search_cursor).cloned() {
                    state.cover_anime_id = Some(anime.id);
                    state.cover_state = None;
                    ensure_cover_fetch(state, &anime, pool, tx);
                }
            }
        }
        KeyCode::Up => {
            if state.search_cursor > 0 {
                state.search_cursor -= 1;
                if let Some(anime) = state.search_results.get(state.search_cursor).cloned() {
                    state.cover_anime_id = Some(anime.id);
                    state.cover_state = None;
                    ensure_cover_fetch(state, &anime, pool, tx);
                }
            }
        }

        KeyCode::Enter => {
            if let Some(anime) = state.search_results.get(state.search_cursor).cloned() {
                state.screen = Screen::Home; // close search first
                open_detail_from_anime(state, anime, pool, tx).await;
            }
        }

        // Append character to search query (after all specific char patterns)
        KeyCode::Char(c) => {
            state.search_query.push(c);
            let query = state.search_query.clone();
            let pool2 = pool.clone();
            let tx2   = tx.clone();
            tokio::spawn(async move {
                search_and_send(&pool2, &query, &tx2).await;
            });
        }

        _ => {}
    }
}

async fn handle_settings(
    key: event::KeyEvent,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &mut config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let filtered = ui::settings::get_filtered_items(cfg, state);
    let selected_idx = state.settings_cursor.min(filtered.len().saturating_sub(1));

    match key.code {
        KeyCode::Esc => {
            if !state.settings_query.is_empty() {
                state.settings_query.clear();
                state.settings_cursor = 0;
            } else {
                state.go_back();
            }
        }
        KeyCode::Backspace => {
            state.settings_query.pop();
            state.settings_cursor = 0;
        }
        KeyCode::Down => {
            if !filtered.is_empty() {
                state.settings_cursor = (selected_idx + 1).min(filtered.len() - 1);
            }
        }
        KeyCode::Up => {
            state.settings_cursor = selected_idx.saturating_sub(1);
        }
        KeyCode::Char('n') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
            if !filtered.is_empty() {
                state.settings_cursor = (selected_idx + 1).min(filtered.len() - 1);
            }
        }
        KeyCode::Char('p') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
            state.settings_cursor = selected_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
            if let Some(item) = filtered.get(selected_idx).cloned() {
                execute_hub_action(&item, cfg, state, pool, 1, tx).await;
            }
        }
        KeyCode::Left => {
            if let Some(item) = filtered.get(selected_idx).cloned() {
                execute_hub_action(&item, cfg, state, pool, -1, tx).await;
            }
        }
        KeyCode::Right => {
            if let Some(item) = filtered.get(selected_idx).cloned() {
                execute_hub_action(&item, cfg, state, pool, 1, tx).await;
            }
        }
        KeyCode::Char(' ') => {
            if state.settings_query.is_empty() {
                if let Some(item) = filtered.get(selected_idx).cloned() {
                    execute_hub_action(&item, cfg, state, pool, 1, tx).await;
                }
            } else {
                state.settings_query.push(' ');
                state.settings_cursor = 0;
            }
        }
        KeyCode::Tab => {
            let n = ui::settings::SETTINGS_CATEGORIES.len();
            state.settings_category = (state.settings_category + 1) % n;
            state.settings_cursor = 0;
            state.settings_scroll_offset = 0;
        }
        KeyCode::BackTab => {
            let n = ui::settings::SETTINGS_CATEGORIES.len();
            state.settings_category = (state.settings_category + n - 1) % n;
            state.settings_cursor = 0;
            state.settings_scroll_offset = 0;
        }
        KeyCode::Char(d @ '0'..='5') if state.settings_query.is_empty() => {
            state.settings_category = (d as u8 - b'0') as usize;
            state.settings_cursor = 0;
            state.settings_scroll_offset = 0;
        }
        KeyCode::Char('N') if state.settings_query.is_empty() => {
            cycle_network_mode(state, cfg, true, tx).await;
        }
        KeyCode::Char(c) => {
            state.settings_query.push(c);
            state.settings_cursor = 0;
        }
        _ => {}
    }
}

async fn execute_hub_action(
    item: &ui::settings::HubItem,
    cfg: &mut config::Config,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    delta: i32,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    use ui::settings::HubActionId::*;
    use services::substyler::{SubtitlePreset, SubtitleStyleConfig};

    match item.id {
        NetworkMode => {
            cycle_network_mode(state, cfg, delta >= 0, tx).await;
        }
        AutoAbrDownshift => {
            cfg.auto_abr_downshift = !cfg.auto_abr_downshift;
            state.auto_abr_downshift = cfg.auto_abr_downshift;
            let _ = cfg.save();
            sync_remote_telemetry_snapshot(state);
            let label = if cfg.auto_abr_downshift { "Enabled" } else { "Disabled" };
            state.show_toast(format!("◈ Auto-ABR Downshift: {label}"), unix_now());
        }
        ForceIpv4 => {
            cfg.force_ipv4 = !cfg.force_ipv4;
            state.force_ipv4 = cfg.force_ipv4;
            set_global_network_flags(state.network_mode, cfg.force_ipv4);
            let _ = cfg.save();
            sync_remote_telemetry_snapshot(state);
            let label = if cfg.force_ipv4 { "Forced" } else { "Auto (IPv4/IPv6)" };
            state.show_toast(format!("◈ Force IPv4 Transport: {label}"), unix_now());
        }
        ManualAbrStepDown => {
            manual_abr_step_down(state, cfg, tx).await;
        }
        RemoteTheme => {
            let next = services::remote::cycle_remote_theme_id(&cfg.remote.theme_id, delta >= 0);
            cfg.remote.theme_id = next.id.to_string();
            cfg.remote.art_index = Some(next.default_art_idx);
            let _ = cfg.save();
            sync_remote_theme_snapshot(state, cfg);
            state.show_toast(
                format!("❖ Remote Theme: {} ({})", next.name, next.genre),
                unix_now(),
            );
        }
        RemoteArtMode => {
            if delta < 0 {
                let base_idx = cfg.remote.art_index.unwrap_or_else(|| {
                    services::remote::find_remote_theme(&cfg.remote.theme_id).default_art_idx
                });
                let next_idx = (base_idx + 1) % services::loading_art::Y2K_ART_COUNT;
                cfg.remote.art_index = Some(next_idx);
                let (_bytes, tag) = services::loading_art::raw_y2k_art(next_idx);
                state.show_toast(
                    format!("❖ Remote Portrait #{:02}: {}", next_idx + 1, tag),
                    unix_now(),
                );
            } else {
                cfg.remote.art_mode =
                    services::remote::cycle_remote_art_mode(&cfg.remote.art_mode).to_string();
                state.show_toast(
                    format!("❖ Remote Art Mode: {}", cfg.remote.art_mode.to_ascii_uppercase()),
                    unix_now(),
                );
            }
            let _ = cfg.save();
            sync_remote_theme_snapshot(state, cfg);
        }
        SubPreset => {
            let presets = [
                SubtitlePreset::StudioMinimal,
                SubtitlePreset::NetflixClean,
                SubtitlePreset::AnimeFansub,
                SubtitlePreset::OledPlate,
            ];
            let cur = presets.iter().position(|p| p == &cfg.subtitle_style.preset).unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(presets.len() as i32) as usize;
            cfg.subtitle_style = SubtitleStyleConfig::from_preset(presets[next]);
            let _ = cfg.save();
            state.show_toast(format!("❖ Subtitle Preset: {}", presets[next].label()), unix_now());
        }
        SubCollisionGuard => {
            cfg.subtitle_style.collision_avoidance = !cfg.subtitle_style.collision_avoidance;
            let _ = cfg.save();
            let msg = if cfg.subtitle_style.collision_avoidance {
                "◈ Signage Collision Guard: Enabled"
            } else {
                "◈ Signage Collision Guard: Disabled"
            };
            state.show_toast(msg, unix_now());
        }
        CastScan => {
            state.overlay_base = state.current_base_screen();
            state.screen = Screen::CastModal;
            state.is_scanning_cast = true;
            let devs = services::cast::scan_network(500).await;
            state.cast_devices = devs;
            state.is_scanning_cast = false;
        }
        RemoteQrModal => {
            state.overlay_base = state.current_base_screen();
            if state.remote_qr.is_none() {
                state.remote_qr = services::remote::generate_terminal_qr(&state.remote_url);
            }
            state.screen = Screen::RemoteModal;
        }
        RemoteOpenBrowser => {
            let url = state.remote_url.clone();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn();
            state.show_toast(format!("Opened {}", url), unix_now());
        }
        RemoteToggleServer => {
            cfg.remote.enabled = !cfg.remote.enabled;
            let _ = cfg.save();
            let msg = if cfg.remote.enabled {
                "● Mobile LAN Remote: Enabled (Port 8484)"
            } else {
                "● Mobile LAN Remote: Disabled"
            };
            state.show_toast(msg, unix_now());
        }
        PlaybackEngine => {
            cfg.player = if delta < 0 { cfg.player.prev() } else { cfg.player.next() };
            if let Some(pos) = state.playback_players.iter().position(|p| *p == cfg.player) {
                state.playback_player_cursor = pos;
            }
            let _ = cfg.save();
            state.show_toast(
                format!("❖ Playback Engine: {} ({})", cfg.player.label(), cfg.player.parity_badge()),
                unix_now(),
            );
        }
        PerformanceMode => {
            cfg.performance_mode = if delta < 0 {
                cfg.performance_mode.prev()
            } else {
                cfg.performance_mode.next()
            };
            state.performance_mode = cfg.performance_mode;
            let _ = cfg.save();
            if let Some(sock) = services::ipc::get_active_socket_path() {
                let (shadow, deblock, detail, cas, grain) = match cfg.performance_mode {
                    config::PerformanceMode::Eco => ("0.025", "0.18", "0.06", "0.18", "0.004"),
                    config::PerformanceMode::Reference4K => ("0.045", "0.32", "0.14", "0.32", "0.008"),
                    config::PerformanceMode::Balanced | config::PerformanceMode::Auto => {
                        ("0.035", "0.30", "0.10", "0.26", "0.006")
                    }
                };
                let val = format!(
                    "shadow_lift={shadow},deblock_amt={deblock},detail_lift={detail},cas_amt={cas},grain_amt={grain}"
                );
                let label = format!("❖ Perf Tier: {}", cfg.performance_mode.as_str());
                tokio::spawn(async move {
                    let _ = services::remote::send_mpv_command(
                        &sock,
                        &["set_property", "glsl-shader-opts", &val],
                    )
                    .await;
                    let _ = services::remote::send_mpv_command(&sock, &["show-text", &label, "2000"]).await;
                });
            }
            state.show_toast(format!("❖ Mastering Tier: {}", cfg.performance_mode.as_str()), unix_now());
        }
        DebandMode => {
            cfg.deband_mode = cfg.deband_mode.next();
            state.deband_mode = cfg.deband_mode;
            let _ = cfg.save();
            if let Some(sock) = services::ipc::get_active_socket_path() {
                let mode = cfg.deband_mode;
                tokio::spawn(async move {
                    match mode {
                        config::DebandMode::Off => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband", "no"]).await;
                        }
                        config::DebandMode::Auto => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband", "yes"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband-iterations", "1"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband-threshold", "32"]).await;
                        }
                        config::DebandMode::Strong => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband", "yes"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband-iterations", "2"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "deband-threshold", "44"]).await;
                        }
                    }
                    let msg = format!("❖ Debanding: {}", mode.as_str());
                    let _ = services::remote::send_mpv_command(&sock, &["show-text", &msg, "2000"]).await;
                });
            }
            state.show_toast(format!("❖ Deband: {}", cfg.deband_mode.as_str()), unix_now());
        }
        MotionMode => {
            cfg.motion_mode = cfg.motion_mode.next();
            state.motion_mode = cfg.motion_mode;
            let _ = cfg.save();
            if let Some(sock) = services::ipc::get_active_socket_path() {
                let mode = cfg.motion_mode;
                tokio::spawn(async move {
                    match mode {
                        config::MotionMode::Off => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "interpolation", "no"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "video-sync", "audio"]).await;
                        }
                        config::MotionMode::Cadence => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "video-sync", "display-resample"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "tscale", "oversample"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "interpolation", "yes"]).await;
                        }
                        config::MotionMode::Smooth => {
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "video-sync", "display-resample"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "tscale", "mitchell"]).await;
                            let _ = services::remote::send_mpv_command(&sock, &["set_property", "interpolation", "yes"]).await;
                        }
                    }
                    let msg = format!("❖ Motion Cadence: {}", mode.as_str());
                    let _ = services::remote::send_mpv_command(&sock, &["show-text", &msg, "2000"]).await;
                });
            }
            state.show_toast(format!("❖ Motion: {}", cfg.motion_mode.as_str()), unix_now());
        }
        Anime4kMode => {
            let modes = [
                config::Anime4KMode::Off,
                config::Anime4KMode::Balanced,
                config::Anime4KMode::Ultra,
            ];
            let cur = modes.iter().position(|m| m == &cfg.anime4k_mode).unwrap_or(0) as i32;
            let next = (cur + delta).rem_euclid(modes.len() as i32) as usize;
            cfg.anime4k_mode = modes[next];
            state.anime4k_mode = modes[next];
            let _ = cfg.save();
            state.show_toast(format!("❖ Anime4K: {}", modes[next].as_str()), unix_now());
        }
        AutoFullscreen => {
            toggle_auto_fullscreen_action(state, cfg);
        }
        PipMode => {
            toggle_pip_mode_action(state, cfg);
        }
        PipCorner => {
            if delta < 0 {
                let n = services::ipc::PIP_CORNER_LABELS.len();
                state.pip_corner_idx = (state.pip_corner_idx + n - 1) % n;
            }
            cycle_pip_corner_action(state, cfg);
        }
        ToggleInfoHud => {
            toggle_info_hud_action(state, false);
        }
        AmbilightGlow => {
            cfg.ambilight_enabled = !cfg.ambilight_enabled;
            state.ambilight_enabled = cfg.ambilight_enabled;
            let _ = cfg.save();
            let msg = if cfg.ambilight_enabled {
                "❖ Ambilight: Enabled"
            } else {
                "❖ Ambilight: Disabled"
            };
            state.show_toast(msg, unix_now());
        }
        AutoSkipIntro => {
            cfg.auto_skip_intro = !cfg.auto_skip_intro;
            let _ = cfg.save();
            let msg = if cfg.auto_skip_intro {
                "󰄬 Auto-Skip Intro: Enabled"
            } else {
                "󰄬 Auto-Skip Intro: Disabled"
            };
            state.show_toast(msg, unix_now());
        }
        CheckUpdates => {
            if let Some(ref info) = state.available_update.clone() {
                state.open_update_modal(info.clone());
            } else if !state.is_checking_update {
                state.is_checking_update = true;
                state.show_toast("Checking GitHub for updates...", unix_now());
                let tx_check = tx.clone();
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    let current_ver = env!("CARGO_PKG_VERSION");
                    let res = services::updater::check_for_updates(&client, current_ver).await.unwrap_or(None);
                    let _ = tx_check.send(AppMessage::UpdateCheckResult(res)).await;
                });
            }
        }
        WhatsNewNotes => {
            let (ver_label, notes) = if let Some(ref u) = state.available_update {
                let n = if u.simple_notes.is_empty() {
                    services::updater::default_current_release_notes()
                } else {
                    u.simple_notes.clone()
                };
                (u.latest_version.clone(), n)
            } else {
                (
                    services::updater::current_display_version_label(),
                    services::updater::default_current_release_notes(),
                )
            };
            state.open_post_update_welcome(ver_label, notes);
        }
        AutoUpdateMode => {
            let next = if !cfg.update.check_on_startup {
                0
            } else if cfg.update.auto_install {
                2
            } else {
                1
            };
            match next {
                0 => {
                    cfg.update.check_on_startup = true;
                    cfg.update.auto_install = false;
                    state.show_toast("❖ Auto-Update: Check Only", unix_now());
                }
                1 => {
                    cfg.update.check_on_startup = true;
                    cfg.update.auto_install = true;
                    state.show_toast("❖ Auto-Update: Auto-Install", unix_now());
                }
                _ => {
                    cfg.update.check_on_startup = false;
                    cfg.update.auto_install = false;
                    state.show_toast("❖ Auto-Update: Off", unix_now());
                }
            }
            let _ = cfg.save();
        }
        HealthDoctor => {
            refresh_dependency_status(state);
            state.open_setup();
        }
        OpenCalendar => {
            open_and_fetch_calendar(state, pool, tx).await;
        }
        AniListSync => {
            state.open_login();
            spawn_merge_audit(pool, &state.auth, tx);
        }
    }
}

fn handle_help(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &mut config::Config,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter => state.go_back(),
        KeyCode::Char('s') => state.open_settings(),
        KeyCode::Char('!') => state.open_setup(),
        KeyCode::Char('t') | KeyCode::Char('T') => {
            cycle_active_theme(state, pool, cfg);
        }
        _ => {}
    }
}

fn handle_setup(
    key: event::KeyEvent,
    state: &mut AppState,
    cfg: &mut config::Config,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => state.go_back(),
        KeyCode::Char('r') => {
            refresh_dependency_status(state);
            state.show_toast("Dependency check refreshed", unix_now());
        }
        KeyCode::Char('p')
        | KeyCode::Right
        | KeyCode::Char('l')
        | KeyCode::Left
        | KeyCode::Char('h') => {
            let players = ui::setup::supported_players();
            let cur = players.iter().position(|&p| p == cfg.player).unwrap_or(0) as isize;
            let step = if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                -1
            } else {
                1
            };
            let next_idx = (cur + step).rem_euclid(players.len() as isize) as usize;
            let next = players[next_idx];
            cfg.player = next;
            if let Some(pos) = state.playback_players.iter().position(|p| *p == next) {
                state.playback_player_cursor = pos;
            }
            let _ = cfg.save();
            let msg = if ui::setup::player_installed(next, state) {
                format!("Player switched to {}", next.as_str())
            } else {
                format!("Player set to {} (binary missing in PATH)", next.as_str())
            };
            state.show_toast(msg, unix_now());
        }
        KeyCode::Char('u') => {
            state.toggle_anime4k();
            cfg.anime4k_mode = state.anime4k_mode;
            let _ = cfg.save();
            state.show_toast(format!("Anime4K: {}", state.anime4k_mode.as_str()), unix_now());
        }
        KeyCode::Char('s') => state.open_settings(),
        _ => {}
    }
}

async fn broadcast_watchlist(pool: &sqlx::SqlitePool, tx: &tokio::sync::mpsc::Sender<AppMessage>) {
    if let Ok((wl, sources)) = services::sync::load_watchlist_with_sources(pool).await {
        let statuses = db::user::get_watchlist_statuses(pool).await.unwrap_or_default();
        let _ = tx.send(AppMessage::WatchlistUpdated(wl, sources, statuses)).await;
    }
}

fn spawn_merge_audit(
    pool: &sqlx::SqlitePool,
    auth: &config::AuthConfig,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = auth.merge_policy;
    tokio::spawn(async move {
        if let Ok(report) = services::tracker_merge::build_merge_audit(&pool2, policy).await {
            let _ = tx2.send(AppMessage::MergeAuditReady(report)).await;
        }
    });
}

async fn trigger_watchlist_status_picker_flow(
    anime: db::cache::Anime,
    from_detail: bool,
    home_data_opt: Option<&ui::home::HomeData>,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
) {
    let _ = db::cache::upsert_anime(pool, &anime).await;
    let current_status = home_data_opt
        .and_then(|hd| hd.watchlist_statuses.get(&anime.id).copied())
        .or(db::user::get_anime_watch_status(pool, anime.id).await);
    let in_wl = db::user::is_in_watchlist(pool, anime.id).await.unwrap_or(false);
    let current_source = if in_wl || current_status.is_some() {
        home_data_opt
            .and_then(|hd| hd.watchlist_sources.get(&anime.id).copied())
            .or_else(|| None)
            .or(db::user::get_anime_watchlist_source(pool, anime.id).await.ok())
    } else {
        None
    };
    state.open_watchlist_status_picker(
        anime,
        state::WatchlistPromptMode::Add,
        current_status,
        current_source,
        from_detail,
    );
}

async fn trigger_watchlist_remove_flow(
    anime: db::cache::Anime,
    from_detail: bool,
    home_data_opt: Option<&mut ui::home::HomeData>,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let source = home_data_opt
        .as_ref()
        .and_then(|hd| hd.watchlist_sources.get(&anime.id).copied())
        .unwrap_or(
            db::user::get_anime_watchlist_source(pool, anime.id)
                .await
                .unwrap_or(db::user::WatchlistSource::Local),
        );
    let current_status = home_data_opt
        .as_ref()
        .and_then(|hd| hd.watchlist_statuses.get(&anime.id).copied())
        .or(db::user::get_anime_watch_status(pool, anime.id).await);

    if source == db::user::WatchlistSource::Both {
        state.open_watchlist_status_picker(
            anime,
            state::WatchlistPromptMode::RemoveShared,
            current_status,
            Some(source),
            from_detail,
        );
        return;
    }

    let target = match source {
        db::user::WatchlistSource::AniList => config::WatchlistTarget::AniList,
        db::user::WatchlistSource::Mal => config::WatchlistTarget::Mal,
        db::user::WatchlistSource::Both => config::WatchlistTarget::Both,
        db::user::WatchlistSource::Local => config::WatchlistTarget::Local,
    };

    let now = unix_now();
    let title = anime.display_title().to_string();
    if let Some(hd) = home_data_opt {
        hd.watchlist.retain(|a| a.id != anime.id);
        hd.watchlist_sources.remove(&anime.id);
        hd.watchlist_statuses.remove(&anime.id);
        let max_idx = hd.watchlist.len().saturating_sub(1);
        let offset = state.row_offset("watchlist").min(max_idx);
        state.row_offsets.insert("watchlist".to_string(), offset);
        if hd.watchlist.is_empty() && state.active_row == state::CategoryRow::Watchlist {
            state.active_row = next_non_empty_row(&state.active_row, hd, 1);
        }
        refresh_home_cover(state, hd, pool, tx);
    }
    if from_detail {
        state.in_watchlist = false;
    }
    state.show_toast(
        format!("Removing \"{}\" ({})...", title, source.divider_label()),
        now,
    );

    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let mut auth_clone = state.auth.clone();
    let trackers_cfg = cfg.trackers.clone();
    tokio::spawn(async move {
        let outcome = services::scrobble::remove_watchlist_from_target(
            &pool2,
            &anime,
            target,
            &mut auth_clone,
            &trackers_cfg,
            now,
        )
        .await
        .unwrap_or_default();
        let (wl, sources) = services::sync::load_watchlist_with_sources(&pool2)
            .await
            .unwrap_or_default();
        let statuses = db::user::get_watchlist_statuses(&pool2).await.unwrap_or_default();
        let cw = services::sync::load_continue_watching(&pool2)
            .await
            .unwrap_or_default();
        let still_in_wl = db::user::is_in_watchlist(&pool2, anime.id).await.unwrap_or(false);
        let _ = tx2
            .send(AppMessage::WatchlistWritebackCompleted {
                anime_id: anime.id,
                removed_completely: !still_in_wl,
                added_to_wl: false,
                outcome_summary: outcome.summary,
                wl,
                sources,
                statuses,
                cw,
            })
            .await;
    });
}

async fn commit_watchlist_choice(
    prompt: state::WatchlistTargetPrompt,
    choice: state::WatchlistPickerChoice,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let now = unix_now();
    let anime = prompt.anime;
    let target = prompt.target;
    let target_label = target.label();

    let is_add = matches!(choice, state::WatchlistPickerChoice::SetStatus(_));
    match choice {
        state::WatchlistPickerChoice::SetStatus(status) => {
            if prompt.from_detail {
                state.in_watchlist = true;
            }
            state.show_toast(
                format!(
                    "Labeling \"{}\" · {} ({target_label})...",
                    anime.display_title(),
                    status.full_label()
                ),
                now,
            );
        }
        state::WatchlistPickerChoice::Remove => {
            state.show_toast(
                format!("Removing \"{}\" ({target_label})...", anime.display_title()),
                now,
            );
        }
    }

    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let mut auth_clone = state.auth.clone();
    let trackers_cfg = cfg.trackers.clone();
    tokio::spawn(async move {
        let outcome = match choice {
            state::WatchlistPickerChoice::SetStatus(status) => {
                services::scrobble::set_watchlist_status_on_target(
                    &pool2,
                    &anime,
                    status,
                    target,
                    &mut auth_clone,
                    &trackers_cfg,
                    now,
                )
                .await
                .unwrap_or_default()
            }
            state::WatchlistPickerChoice::Remove => {
                services::scrobble::remove_watchlist_from_target(
                    &pool2,
                    &anime,
                    target,
                    &mut auth_clone,
                    &trackers_cfg,
                    now,
                )
                .await
                .unwrap_or_default()
            }
        };

        let (wl, sources) = services::sync::load_watchlist_with_sources(&pool2)
            .await
            .unwrap_or_default();
        let statuses = db::user::get_watchlist_statuses(&pool2).await.unwrap_or_default();
        let cw = services::sync::load_continue_watching(&pool2)
            .await
            .unwrap_or_default();
        let still_in_wl = db::user::is_in_watchlist(&pool2, anime.id).await.unwrap_or(false);
        let _ = tx2
            .send(AppMessage::WatchlistWritebackCompleted {
                anime_id: anime.id,
                removed_completely: !still_in_wl,
                added_to_wl: is_add,
                outcome_summary: outcome.summary,
                wl,
                sources,
                statuses,
                cw,
            })
            .await;
    });
}

async fn handle_watchlist_target_modal(
    key: event::KeyEvent,
    state: &mut AppState,
    _home_data: &mut ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.watchlist_target_prompt = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut p) = state.watchlist_target_prompt {
                p.cursor = if p.cursor == 0 { 5 } else { p.cursor - 1 };
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut p) = state.watchlist_target_prompt {
                p.cursor = (p.cursor + 1) % 6;
            }
        }
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
            if let Some(ref mut p) = state.watchlist_target_prompt {
                p.cycle_target(true);
            }
        }
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
            if let Some(ref mut p) = state.watchlist_target_prompt {
                p.cycle_target(false);
            }
        }
        KeyCode::Char(c @ '1'..='6') => {
            let idx = (c as usize) - ('1' as usize);
            if let Some(p) = state.watchlist_target_prompt.take() {
                let choice = state::WatchlistTargetPrompt::CHOICES[idx];
                commit_watchlist_choice(p, choice, state, pool, cfg, tx).await;
            }
        }
        KeyCode::Enter => {
            if let Some(p) = state.watchlist_target_prompt.take() {
                let choice = p.selected_choice();
                commit_watchlist_choice(p, choice, state, pool, cfg, tx).await;
            }
        }
        _ => {}
    }
}

fn spawn_provider_oauth(
    provider: config::TrackerProvider,
    client_id: String,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    state.cancel_pending_oauth();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    state.oauth_cancel_tx = Some(cancel_tx);
    state.oauth_pending = Some(provider);

    let nonce = services::oauth::generate_state_nonce();
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = state.auth.merge_policy;
    let existing = state.auth.account(provider).clone();

    match provider {
        config::TrackerProvider::AniList => {
            let auth_url = services::oauth::build_anilist_authorize_url(&client_id);
            let _ = services::oauth::open_browser(&auth_url);
            state.login_status = Some(
                "Opened AniList sign-in in browser. Complete authorization to enable live sync.".into(),
            );
            tokio::spawn(async move {
                let res: std::result::Result<(config::TrackerAccount, services::tracker_merge::MergeReport, ui::home::HomeData, String), String> =
                    async {
                        let oauth = services::oauth::await_anilist_browser_oauth(cancel_rx)
                            .await
                            .map_err(|e| e.to_string())?;
                        let viewer = services::oauth::verify_anilist_token(&oauth.access_token)
                            .await
                            .map_err(|e| e.to_string())?;
                        let now = unix_now();
                        let client = api::anilist::AniListClient::with_token(Some(oauth.access_token.clone()));
                        let entries = client
                            .detailed_user_entries(viewer.user_id, now)
                            .await
                            .map_err(|e| e.to_string())?;
                        let snaps = services::tracker_merge::anilist_records_to_snapshots(&entries);
                        let report = services::tracker_merge::merge_and_reconcile(
                            &pool2,
                            &[config::TrackerProvider::AniList],
                            &snaps,
                            policy,
                            vec![],
                            now,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let home = services::sync::load_cached_home_data(&pool2)
                            .await
                            .map_err(|e| e.to_string())?;
                        let account = config::TrackerAccount {
                            token: Some(oauth.access_token),
                            refresh_token: None,
                            expires_at: None,
                            user_id: Some(viewer.user_id),
                            username: Some(viewer.username.clone()),
                            avatar_url: viewer.avatar_url,
                            sync_mode: Some("OAuth2 Live".into()),
                            last_imported_file: existing.last_imported_file,
                            last_sync_time: Some(now),
                        };
                        let msg = format!("✓ AniList OAuth connected (@{}, {} entries)", viewer.username, entries.len());
                        Ok((account, report, home, msg))
                    }
                    .await;

                match res {
                    Ok((account, report, home, summary)) => {
                        let _ = tx2
                            .send(AppMessage::TrackerSyncCompleted {
                                provider: config::TrackerProvider::AniList,
                                account,
                                report,
                                home_data: Box::new(home),
                                summary,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx2
                            .send(AppMessage::TrackerSyncFailed(config::TrackerProvider::AniList, e))
                            .await;
                    }
                }
            });
        }
        config::TrackerProvider::Mal => {
            let verifier = services::oauth::generate_pkce_verifier();
            let auth_url = services::oauth::build_mal_authorize_url(&client_id, &verifier, &nonce);
            let _ = services::oauth::open_browser(&auth_url);
            state.login_status = Some(
                "Opened MyAnimeList OAuth2 in browser. Authorize Solus to enable live MAL sync.".into(),
            );
            tokio::spawn(async move {
                let res: std::result::Result<(config::TrackerAccount, services::tracker_merge::MergeReport, ui::home::HomeData, String), String> =
                    async {
                        let tokens = services::oauth::await_mal_browser_oauth(
                            client_id.clone(),
                            verifier,
                            nonce,
                            cancel_rx,
                            unix_now,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let now = unix_now();
                        let mc = api::mal::MalClient::with_token(Some(client_id.clone()), Some(tokens.access_token.clone()));
                        let me = mc.fetch_me().await.map_err(|e| e.to_string())?;
                        let mal_entries = mc
                            .fetch_user_animelist("@me")
                            .await
                            .map_err(|e| e.to_string())?;
                        let al_public = api::anilist::AniListClient::new();
                        let (snaps, unresolved) =
                            services::tracker_merge::resolve_mal_entries_to_canonical(
                                &pool2,
                                &al_public,
                                &mal_entries,
                                now,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                        let report = services::tracker_merge::merge_and_reconcile(
                            &pool2,
                            &[config::TrackerProvider::Mal],
                            &snaps,
                            policy,
                            unresolved,
                            now,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        let home = services::sync::load_cached_home_data(&pool2)
                            .await
                            .map_err(|e| e.to_string())?;
                        let account = config::TrackerAccount {
                            token: Some(tokens.access_token),
                            refresh_token: tokens.refresh_token,
                            expires_at: tokens.expires_at,
                            user_id: Some(me.id),
                            username: Some(me.name.clone()),
                            avatar_url: me.picture,
                            sync_mode: Some("OAuth2 Live".into()),
                            last_imported_file: existing.last_imported_file,
                            last_sync_time: Some(now),
                        };
                        let msg = format!("✓ MyAnimeList OAuth connected (@{}, {} entries)", me.name, mal_entries.len());
                        Ok((account, report, home, msg))
                    }
                    .await;

                match res {
                    Ok((account, report, home, summary)) => {
                        let _ = tx2
                            .send(AppMessage::TrackerSyncCompleted {
                                provider: config::TrackerProvider::Mal,
                                account,
                                report,
                                home_data: Box::new(home),
                                summary,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx2
                            .send(AppMessage::TrackerSyncFailed(config::TrackerProvider::Mal, e))
                            .await;
                    }
                }
            });
        }
    }
}

fn spawn_provider_username_sync(
    provider: config::TrackerProvider,
    username: String,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = state.auth.merge_policy;
    let mal_cid = cfg.trackers.resolved_mal_client_id();
    let existing = state.auth.account(provider).clone();

    state.login_status = Some(format!(
        "Pulling public {} list for @{}...",
        provider.display_name(),
        username
    ));

    tokio::spawn(async move {
        let now = unix_now();
        let res: std::result::Result<(config::TrackerAccount, services::tracker_merge::MergeReport, ui::home::HomeData, String), String> =
            match provider {
                config::TrackerProvider::AniList => async {
                    let al = api::anilist::AniListClient::new();
                    let (viewer, entries) = al
                        .detailed_public_user_entries(&username, now)
                        .await
                        .map_err(|e| e.to_string())?;
                    let snaps = services::tracker_merge::anilist_records_to_snapshots(&entries);
                    let report = services::tracker_merge::merge_and_reconcile(
                        &pool2,
                        &[config::TrackerProvider::AniList],
                        &snaps,
                        policy,
                        vec![],
                        now,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let home = services::sync::load_cached_home_data(&pool2)
                        .await
                        .map_err(|e| e.to_string())?;
                    let account = config::TrackerAccount {
                        token: existing.token,
                        refresh_token: existing.refresh_token,
                        expires_at: existing.expires_at,
                        user_id: Some(viewer.id),
                        username: Some(viewer.name.clone()),
                        avatar_url: viewer.avatar_url,
                        sync_mode: Some("Public Username".into()),
                        last_imported_file: existing.last_imported_file,
                        last_sync_time: Some(now),
                    };
                    let msg = format!("✓ Synced AniList @{} ({} entries)", viewer.name, entries.len());
                    Ok((account, report, home, msg))
                }
                .await,
                config::TrackerProvider::Mal => async {
                    let mc = api::mal::MalClient::new(mal_cid.clone(), None);
                    let mal_entries = mc
                        .fetch_user_animelist(&username)
                        .await
                        .map_err(|e| e.to_string())?;
                    let al_public = api::anilist::AniListClient::new();
                    let (snaps, unresolved) =
                        services::tracker_merge::resolve_mal_entries_to_canonical(
                            &pool2,
                            &al_public,
                            &mal_entries,
                            now,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    let report = services::tracker_merge::merge_and_reconcile(
                        &pool2,
                        &[config::TrackerProvider::Mal],
                        &snaps,
                        policy,
                        unresolved,
                        now,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let home = services::sync::load_cached_home_data(&pool2)
                        .await
                        .map_err(|e| e.to_string())?;
                    let account = config::TrackerAccount {
                        token: existing.token,
                        refresh_token: existing.refresh_token,
                        expires_at: existing.expires_at,
                        user_id: existing.user_id,
                        username: Some(username.clone()),
                        avatar_url: existing.avatar_url,
                        sync_mode: Some("Public Username".into()),
                        last_imported_file: existing.last_imported_file,
                        last_sync_time: Some(now),
                    };
                    let msg = format!("✓ Synced MyAnimeList @{} ({} entries, 0 duplicates)", username, mal_entries.len());
                    Ok((account, report, home, msg))
                }
                .await,
            };

        match res {
            Ok((account, report, home, summary)) => {
                let _ = tx2
                    .send(AppMessage::TrackerSyncCompleted {
                        provider,
                        account,
                        report,
                        home_data: Box::new(home),
                        summary,
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx2.send(AppMessage::TrackerSyncFailed(provider, e)).await;
            }
        }
    });
}

async fn complete_mal_authenticated_sync(
    pool: &sqlx::SqlitePool,
    existing: config::TrackerAccount,
    mal_cid: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    sync_mode: &str,
    policy: config::MergePolicy,
    now: i64,
) -> std::result::Result<(config::TrackerAccount, services::tracker_merge::MergeReport, ui::home::HomeData, String), String> {
    let mc = api::mal::MalClient::with_token(mal_cid, Some(access_token.clone()));
    let me = mc.fetch_me().await.map_err(|e| e.to_string())?;
    let mal_entries = mc
        .fetch_user_animelist("@me")
        .await
        .map_err(|e| e.to_string())?;
    let al_public = api::anilist::AniListClient::new();
    let (snaps, unresolved) =
        services::tracker_merge::resolve_mal_entries_to_canonical(pool, &al_public, &mal_entries, now)
            .await
            .map_err(|e| e.to_string())?;
    let report = services::tracker_merge::merge_and_reconcile(
        pool,
        &[config::TrackerProvider::Mal],
        &snaps,
        policy,
        unresolved,
        now,
    )
    .await
    .map_err(|e| e.to_string())?;
    let home = services::sync::load_cached_home_data(pool)
        .await
        .map_err(|e| e.to_string())?;
    let account = config::TrackerAccount {
        token: Some(access_token),
        refresh_token: refresh_token.or(existing.refresh_token),
        expires_at: expires_at.or(existing.expires_at),
        user_id: Some(me.id),
        username: Some(me.name.clone()),
        avatar_url: me.picture,
        sync_mode: Some(sync_mode.to_string()),
        last_imported_file: existing.last_imported_file,
        last_sync_time: Some(now),
    };
    let msg = format!(
        "✓ Connected MAL @{} via {} ({} entries, 2-Way Sync ✓)",
        me.name,
        sync_mode,
        mal_entries.len()
    );
    Ok((account, report, home, msg))
}

fn spawn_mal_password_login(
    username: String,
    password: String,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = state.auth.merge_policy;
    let mal_cid = cfg.trackers.resolved_mal_client_id();
    let existing = state.auth.mal.clone();

    state.login_status = Some(format!(
        "Signing in to MyAnimeList as @{}...",
        username
    ));

    tokio::spawn(async move {
        let now = unix_now();
        let res = async {
            let auth_client = api::mal::MalClient::new(mal_cid.clone(), None);
            let tok = auth_client
                .login_with_password(&username, &password)
                .await
                .map_err(|e| e.to_string())?;
            let expires_at = tok.expires_in.map(|secs| now + secs);
            complete_mal_authenticated_sync(
                &pool2,
                existing,
                mal_cid,
                tok.access_token,
                tok.refresh_token,
                expires_at,
                "Direct Login",
                policy,
                now,
            )
            .await
        }
        .await;

        match res {
            Ok((account, report, home, summary)) => {
                let _ = tx2
                    .send(AppMessage::TrackerSyncCompleted {
                        provider: config::TrackerProvider::Mal,
                        account,
                        report,
                        home_data: Box::new(home),
                        summary,
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx2
                    .send(AppMessage::TrackerSyncFailed(
                        config::TrackerProvider::Mal,
                        e,
                    ))
                    .await;
            }
        }
    });
}

fn spawn_provider_token_sync(
    provider: config::TrackerProvider,
    token: String,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = state.auth.merge_policy;
    let mal_cid = cfg.trackers.resolved_mal_client_id();
    let existing = state.auth.account(provider).clone();

    state.login_status = Some(format!(
        "Verifying {} access token & syncing...",
        provider.display_name()
    ));

    tokio::spawn(async move {
        let now = unix_now();
        let res: std::result::Result<(config::TrackerAccount, services::tracker_merge::MergeReport, ui::home::HomeData, String), String> =
            match provider {
                config::TrackerProvider::AniList => async {
                    let viewer = services::oauth::verify_anilist_token(&token)
                        .await
                        .map_err(|e| e.to_string())?;
                    let client = api::anilist::AniListClient::with_token(Some(token.clone()));
                    let entries = client
                        .detailed_user_entries(viewer.user_id, now)
                        .await
                        .map_err(|e| e.to_string())?;
                    let snaps = services::tracker_merge::anilist_records_to_snapshots(&entries);
                    let report = services::tracker_merge::merge_and_reconcile(
                        &pool2,
                        &[config::TrackerProvider::AniList],
                        &snaps,
                        policy,
                        vec![],
                        now,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let home = services::sync::load_cached_home_data(&pool2)
                        .await
                        .map_err(|e| e.to_string())?;
                    let account = config::TrackerAccount {
                        token: Some(token),
                        refresh_token: None,
                        expires_at: None,
                        user_id: Some(viewer.user_id),
                        username: Some(viewer.username.clone()),
                        avatar_url: viewer.avatar_url,
                        sync_mode: Some("OAuth2 Token".into()),
                        last_imported_file: existing.last_imported_file,
                        last_sync_time: Some(now),
                    };
                    let msg = format!("✓ Connected AniList @{} via Token ({} entries)", viewer.username, entries.len());
                    Ok((account, report, home, msg))
                }
                .await,
                config::TrackerProvider::Mal => {
                    complete_mal_authenticated_sync(
                        &pool2,
                        existing,
                        mal_cid,
                        token,
                        None,
                        None,
                        "OAuth2 Token",
                        policy,
                        now,
                    )
                    .await
                }
            };

        match res {
            Ok((account, report, home, summary)) => {
                let _ = tx2
                    .send(AppMessage::TrackerSyncCompleted {
                        provider,
                        account,
                        report,
                        home_data: Box::new(home),
                        summary,
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx2.send(AppMessage::TrackerSyncFailed(provider, e)).await;
            }
        }
    });
}

fn spawn_reconcile_policy_only(
    pool: &sqlx::SqlitePool,
    auth: &config::AuthConfig,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let policy = auth.merge_policy;
    tokio::spawn(async move {
        let now = unix_now();
        if let Ok(report) =
            services::tracker_merge::merge_and_reconcile(&pool2, &[], &[], policy, vec![], now).await
        {
            if let Ok(home) = services::sync::load_cached_home_data(&pool2).await {
                let _ = tx2
                    .send(AppMessage::ParityPushCompleted {
                        summary: format!("✓ Applied Merge Rule: {}", policy.label()),
                        report,
                        home_data: Box::new(home),
                    })
                    .await;
            }
        }
    });
}

fn spawn_parity_push(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let diffs = state
        .merge_report
        .as_ref()
        .map(|r| r.conflict_diffs.clone())
        .unwrap_or_default();
    if diffs.is_empty() {
        state.login_status = Some("✓ Zero lagging diffs to push.".into());
        return;
    }

    state.login_status = Some(format!("Pushing {} parity update(s) to websites...", diffs.len()));
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let mut auth2 = state.auth.clone();
    let trackers2 = cfg.trackers.clone();
    tokio::spawn(async move {
        let now = unix_now();
        let outcome =
            services::scrobble::push_parity_diffs(&pool2, &diffs, &mut auth2, &trackers2, now).await;
        let report = services::tracker_merge::merge_and_reconcile(
            &pool2,
            &[],
            &[],
            auth2.merge_policy,
            vec![],
            now,
        )
        .await
        .unwrap_or_default();
        let home = services::sync::load_cached_home_data(&pool2)
            .await
            .unwrap_or_else(|_| ui::home::HomeData::empty());
        let _ = tx2
            .send(AppMessage::ParityPushCompleted {
                summary: outcome.summary,
                report,
                home_data: Box::new(home),
            })
            .await;
    });
}

fn spawn_refresh_all_connected(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if !state.auth.has_anilist() && !state.auth.has_mal() {
        state.login_status = Some("Connect AniList or MyAnimeList first.".into());
        return;
    }

    state.login_status = Some("Refreshing connected tracker snapshots...".into());
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let mut auth2 = state.auth.clone();
    let trackers2 = cfg.trackers.clone();
    let cfg_ttl_tr = cfg.cache.trending_ttl;
    let cfg_ttl_st = cfg.cache.stable_ttl;
    tokio::spawn(async move {
        let now = unix_now();
        let mal_cid = trackers2.resolved_mal_client_id();
        let _ = services::scrobble::maybe_refresh_mal_token(&mut auth2, mal_cid.as_deref(), now).await;
        let client = api::anilist::AniListClient::with_token(auth2.anilist_token());
        match services::sync::sync_all(
            &pool2,
            &client,
            Some(&auth2),
            mal_cid.as_deref(),
            cfg_ttl_tr,
            cfg_ttl_st,
            now,
        )
        .await
        {
            Ok(home) => {
                let report =
                    services::tracker_merge::build_merge_audit(&pool2, auth2.merge_policy)
                        .await
                        .unwrap_or_default();
                let _ = tx2
                    .send(AppMessage::ParityPushCompleted {
                        summary: format!(
                            "✓ Refreshed lists: {} unique shows (0 duplicates)",
                            report.unified_unique_count
                        ),
                        report,
                        home_data: Box::new(home),
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx2
                    .send(AppMessage::TrackerSyncFailed(
                        config::TrackerProvider::AniList,
                        e.to_string(),
                    ))
                    .await;
            }
        }
    });
}

async fn handle_login(
    key: event::KeyEvent,
    state: &mut AppState,
    _home_data: &mut ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    cfg: &mut config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    // 1. Waiting on browser OAuth2 loopback callback
    if let Some(pending_provider) = state.oauth_pending {
        match key.code {
            KeyCode::Esc => {
                state.cancel_pending_oauth();
                state.login_status = Some("Browser OAuth2 cancelled.".into());
            }
            KeyCode::Char('t') => {
                state.cancel_pending_oauth();
                state.account_input_mode = state::AccountInputMode::ManualToken(pending_provider);
                state.account_text_input.clear();
                state.login_status = None;
            }
            _ => {}
        }
        return;
    }

    // 2. Typing Mode (explicit isolation so shortcut letters never hijack username/token typing)
    if state.account_input_mode != state::AccountInputMode::None {
        match key.code {
            KeyCode::Esc => {
                state.account_input_mode = state::AccountInputMode::None;
                state.account_text_input.clear();
                state.pending_mal_login_username.clear();
            }
            KeyCode::Backspace => {
                state.account_text_input.pop();
            }
            KeyCode::Char(c) => {
                state.account_text_input.push(c);
            }
            KeyCode::Enter => {
                let mode = state.account_input_mode;
                if mode == state::AccountInputMode::MalPasswordLoginPassword {
                    let raw_password = std::mem::take(&mut state.account_text_input);
                    if raw_password.is_empty() {
                        state.login_status = Some("! Password cannot be empty.".into());
                        return;
                    }
                    let uname = std::mem::take(&mut state.pending_mal_login_username);
                    state.account_input_mode = state::AccountInputMode::None;
                    spawn_mal_password_login(uname, raw_password, state, pool, cfg, tx);
                    return;
                }

                let input = state.account_text_input.trim().to_string();
                if input.is_empty() {
                    state.login_status = Some("! Input cannot be empty.".into());
                    return;
                }
                state.account_text_input.clear();
                match mode {
                    state::AccountInputMode::MalPasswordLoginUsername => {
                        state.pending_mal_login_username = input;
                        state.account_input_mode = state::AccountInputMode::MalPasswordLoginPassword;
                        state.login_status = Some(
                            "Step 2/2: Type your MAL password & press [Enter] (never stored on disk)."
                                .into(),
                        );
                    }
                    state::AccountInputMode::PublicUsername(provider) => {
                        state.account_input_mode = state::AccountInputMode::None;
                        spawn_provider_username_sync(provider, input, state, pool, cfg, tx);
                    }
                    state::AccountInputMode::ManualToken(provider) => {
                        state.account_input_mode = state::AccountInputMode::None;
                        spawn_provider_token_sync(provider, input, state, pool, cfg, tx);
                    }
                    state::AccountInputMode::ClientId(provider) => {
                        state.account_input_mode = state::AccountInputMode::None;
                        match provider {
                            config::TrackerProvider::AniList => {
                                cfg.trackers.anilist_client_id = Some(input);
                            }
                            config::TrackerProvider::Mal => {
                                cfg.trackers.mal_client_id = Some(input);
                            }
                        }
                        if let Err(e) = cfg.save() {
                            state.login_status = Some(format!("! Failed saving Client ID: {e}"));
                        } else {
                            state.login_status = Some(format!(
                                "✓ Saved {} OAuth Client ID! Press [o] to launch Browser OAuth.",
                                provider.display_name()
                            ));
                        }
                    }
                    state::AccountInputMode::MalPasswordLoginPassword
                    | state::AccountInputMode::None => {}
                }
            }
            _ => {}
        }
        return;
    }

    // 3. Navigation Mode across [1] Live Trackers, [2] Merge & Conflicts, [3] Archives & Vault
    match key.code {
        KeyCode::Esc => {
            state.cancel_pending_oauth();
            state.go_back();
        }
        KeyCode::Char('1') => {
            state.account_sync_tab = 0;
            state.login_status = None;
        }
        KeyCode::Char('2') => {
            state.account_sync_tab = 1;
            state.login_status = None;
            spawn_merge_audit(pool, &state.auth, tx);
        }
        KeyCode::Char('3') => {
            state.account_sync_tab = 2;
            state.login_status = None;
            state.detected_export_files = services::importer::detect_export_files();
            if state.detected_file_cursor >= state.detected_export_files.len() {
                state.detected_file_cursor = 0;
            }
        }
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
            if state.account_sync_tab == 0 =>
        {
            state.account_focus_provider = match state.account_focus_provider {
                config::TrackerProvider::AniList => config::TrackerProvider::Mal,
                config::TrackerProvider::Mal => config::TrackerProvider::AniList,
            };
        }
        KeyCode::Char('m') => {
            state.auth.merge_policy = state.auth.merge_policy.next();
            let _ = state.auth.save();
            spawn_reconcile_policy_only(pool, &state.auth, tx);
        }
        KeyCode::Char('a') => {
            state.auth.auto_scrobble_both = !state.auth.auto_scrobble_both;
            let _ = state.auth.save();
            state.login_status = Some(if state.auth.auto_scrobble_both {
                "✓ Auto-scrobble enabled for BOTH AniList + MyAnimeList.".into()
            } else {
                "Auto-scrobble set to Primary Tracker Only.".into()
            });
        }
        KeyCode::Char('r') => {
            spawn_refresh_all_connected(state, pool, cfg, tx);
        }
        _ => match state.account_sync_tab {
            0 => match key.code {
                KeyCode::Char('p') => {
                    state.account_focus_provider = config::TrackerProvider::Mal;
                    state.pending_mal_login_username.clear();
                    state.account_text_input = state.auth.mal.username.clone().unwrap_or_default();
                    state.account_input_mode = state::AccountInputMode::MalPasswordLoginUsername;
                    state.login_status = Some(
                        "Step 1/2: Enter your MyAnimeList username & press [Enter].".into(),
                    );
                }
                KeyCode::Char('o') => {
                    let provider = state.account_focus_provider;
                    let cid_opt = match provider {
                        config::TrackerProvider::AniList => cfg.trackers.resolved_anilist_client_id(),
                        config::TrackerProvider::Mal => cfg.trackers.custom_mal_client_id(),
                    };
                    if let Some(cid) = cid_opt {
                        spawn_provider_oauth(provider, cid, state, pool, tx);
                    } else if provider == config::TrackerProvider::Mal {
                        state.account_input_mode = state::AccountInputMode::ClientId(provider);
                        state.account_text_input.clear();
                        state.login_status = Some(
                            "Tip: Press [p] for instant MAL Username + Password Login (zero Client ID needed!), or enter a custom MAL Web Client ID."
                                .into(),
                        );
                    } else {
                        state.account_input_mode = state::AccountInputMode::ClientId(provider);
                        state.account_text_input.clear();
                        state.login_status = Some(format!(
                            "Set {} Client ID once to enable 1-click browser OAuth (or [t] Token / [u] Username).",
                            provider.display_name()
                        ));
                    }
                }
                KeyCode::Char('u') => {
                    let p = state.account_focus_provider;
                    state.account_text_input = state
                        .auth
                        .account(p)
                        .username
                        .clone()
                        .unwrap_or_default();
                    state.account_input_mode = state::AccountInputMode::PublicUsername(p);
                }
                KeyCode::Char('t') => {
                    let p = state.account_focus_provider;
                    state.account_text_input.clear();
                    state.account_input_mode = state::AccountInputMode::ManualToken(p);
                }
                KeyCode::Char('k') => {
                    let p = state.account_focus_provider;
                    state.account_text_input = match p {
                        config::TrackerProvider::AniList => cfg.trackers.resolved_anilist_client_id().unwrap_or_default(),
                        config::TrackerProvider::Mal => cfg.trackers.custom_mal_client_id().unwrap_or_default(),
                    };
                    state.account_input_mode = state::AccountInputMode::ClientId(p);
                }
                KeyCode::Char('x') => {
                    let p = state.account_focus_provider;
                    if state.auth.account(p).is_connected() {
                        let _ = state.auth.clear_provider(p);
                        let _ = state.auth.save();
                        let _ = db::user::clear_provider_tracker_entries(pool, p).await;
                        spawn_reconcile_policy_only(pool, &state.auth, tx);
                        state.login_status = Some(format!(
                            "Unlinked {} (other tracker & local vault preserved).",
                            p.display_name()
                        ));
                        state.show_toast(format!("Unlinked {}", p.display_name()), unix_now());
                    }
                }
                _ => {}
            },
            1 => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.merge_diff_cursor > 0 {
                        state.merge_diff_cursor -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = state
                        .merge_report
                        .as_ref()
                        .map(|r| r.conflict_diffs.len())
                        .unwrap_or(0);
                    if state.merge_diff_cursor + 1 < len {
                        state.merge_diff_cursor += 1;
                    }
                }
                KeyCode::Char('P') => {
                    spawn_parity_push(state, pool, cfg, tx);
                }
                _ => {}
            },
            _ => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.detected_file_cursor > 0 {
                        state.detected_file_cursor -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.detected_file_cursor + 1 < state.detected_export_files.len() {
                        state.detected_file_cursor += 1;
                    }
                }
                KeyCode::Char('s') => {
                    state.detected_export_files = services::importer::detect_export_files();
                    state.detected_file_cursor = 0;
                    state.login_status = Some(format!(
                        "Scanned Downloads: {} file(s) found.",
                        state.detected_export_files.len()
                    ));
                }
                KeyCode::Char('e') => {
                    state.login_status = Some("Generating Solus backup JSON...".into());
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        let dest = directories::UserDirs::new()
                            .and_then(|u| u.download_dir().map(|d| d.join("solus_backup.json")))
                            .or_else(|| config::Config::config_dir().ok().map(|d| d.join("solus_backup.json")))
                            .unwrap_or_else(|| std::path::PathBuf::from("solus_backup.json"));
                        let res = services::importer::export_library_to_json(&pool2, &dest)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx2.send(AppMessage::BackupExportCompleted(res)).await;
                    });
                }
                KeyCode::Enter => {
                    let Some(path) = state.detected_export_files.get(state.detected_file_cursor).cloned() else {
                        state.login_status = Some("No export file selected.".into());
                        return;
                    };
                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("export")
                        .to_string();
                    let lower = name.to_lowercase();
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    let policy = state.auth.merge_policy;

                    if lower.ends_with(".xml") || lower.ends_with(".xml.gz") {
                        state.login_status = Some(format!("Resolving & importing MAL XML ({name})..."));
                        tokio::spawn(async move {
                            let res: std::result::Result<(services::tracker_merge::MergeReport, String, usize, Box<ui::home::HomeData>), String> =
                                async {
                                    let entries = services::importer::parse_mal_xml_file(&path)
                                        .map_err(|e| e.to_string())?;
                                    let count = entries.len();
                                    let now = unix_now();
                                    let al = api::anilist::AniListClient::new();
                                    let (snaps, unresolved) =
                                        services::tracker_merge::resolve_mal_entries_to_canonical(
                                            &pool2,
                                            &al,
                                            &entries,
                                            now,
                                        )
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    let report = services::tracker_merge::merge_and_reconcile(
                                        &pool2,
                                        &[config::TrackerProvider::Mal],
                                        &snaps,
                                        policy,
                                        unresolved,
                                        now,
                                    )
                                    .await
                                    .map_err(|e| e.to_string())?;
                                    let home = services::sync::load_cached_home_data(&pool2)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    Ok((report, name, count, Box::new(home)))
                                }
                                .await;
                            let _ = tx2.send(AppMessage::MalXmlImportCompleted(res)).await;
                        });
                    } else {
                        state.login_status = Some(format!("Importing JSON archive ({name})..."));
                        tokio::spawn(async move {
                            let res = match std::fs::read_to_string(&path) {
                                Ok(content) => match services::importer::parse_anime_export_json(&content) {
                                    Ok(entries) => {
                                        let now = unix_now();
                                        services::importer::import_entries_to_db(&pool2, &entries, now)
                                            .await
                                            .map_err(|e| e.to_string())
                                    }
                                    Err(e) => Err(e.to_string()),
                                },
                                Err(e) => Err(format!("Failed to read file: {e}")),
                            };
                            let _ = tx2.send(AppMessage::ArchiveImportCompleted(res)).await;
                        });
                    }
                }
                _ => {}
            },
        },
    }
}

// ── Anime Wrapped & Analytics screen ──────────────────────────────────────────

async fn handle_analytics(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a') | KeyCode::Char('A') => {
            state.go_back();
            if state.screen == Screen::TasteAnalytics {
                state.screen = Screen::Home;
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            let next_id = state.analytics_data.as_ref().and_then(|data| {
                if data.top_shows.is_empty() {
                    None
                } else {
                    let len = data.top_shows.len();
                    state.analytics_podium_idx = (state.analytics_podium_idx + len - 1) % len;
                    data.top_shows.get(state.analytics_podium_idx).map(|s| s.id)
                }
            });
            if let Some(id) = next_id {
                trigger_cover_fetch_by_id(state, id, pool, tx);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let next_id = state.analytics_data.as_ref().and_then(|data| {
                if data.top_shows.is_empty() {
                    None
                } else {
                    state.analytics_podium_idx = (state.analytics_podium_idx + 1) % data.top_shows.len();
                    data.top_shows.get(state.analytics_podium_idx).map(|s| s.id)
                }
            });
            if let Some(id) = next_id {
                trigger_cover_fetch_by_id(state, id, pool, tx);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll_analytics_list(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll_analytics_list(-1);
        }
        KeyCode::PageDown | KeyCode::Char('J') => {
            state.scroll_analytics_list(5);
        }
        KeyCode::PageUp | KeyCode::Char('K') => {
            state.scroll_analytics_list(-5);
        }
        KeyCode::Home | KeyCode::Char('g') => {
            state.analytics_list_scroll = 0;
        }
        KeyCode::End | KeyCode::Char('G') => {
            state.analytics_list_scroll = state.analytics_list_max_scroll.unwrap_or(usize::MAX);
        }
        KeyCode::Enter => {
            let target_id = state
                .analytics_data
                .as_ref()
                .and_then(|d| {
                    d.top_shows
                        .get(state.analytics_podium_idx)
                        .or_else(|| d.top_shows.first())
                })
                .map(|s| s.id);
            if let Some(id) = target_id {
                if let Ok(Some(anime)) = db::cache::get_anime(pool, id).await {
                    open_detail_from_anime(state, anime, pool, tx).await;
                }
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            state.show_toast("Refreshing viewing telemetry...", unix_now());
            trigger_analytics_fetch(state, pool.clone(), tx.clone());
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            state.open_settings();
        }
        KeyCode::Char('?') => {
            state.open_help();
        }
        KeyCode::Char('t') | KeyCode::Char('T') => {
            state.theme_index = (state.theme_index + 1) % ui::theme::THEMES.len();
            let theme_name = state.theme().name;
            state.show_toast(format!("Theme: {}", theme_name), unix_now());
        }
        _ => {}
    }
}

async fn apply_profile_switch(
    target_profile: &db::user::UserProfile,
    state: &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    cfg: &mut config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let now = unix_now();
    if db::user::set_active_profile_id(pool, &target_profile.id, now)
        .await
        .is_err()
    {
        return;
    }

    cfg.active_profile = target_profile.id.clone();
    cfg.audio_mode = if target_profile.audio_mode.eq_ignore_ascii_case("dub") {
        config::AudioMode::Dub
    } else {
        config::AudioMode::Sub
    };
    let _ = cfg.save();

    let prev_profile_id = state.active_profile_id.clone();
    services::queue::save_active_queue(&prev_profile_id, &state.queue);

    state.active_profile_id = target_profile.id.clone();
    state.active_profile_name = target_profile.display_name.clone();
    state.active_profile_icon = target_profile.avatar_icon.clone();
    state.theme_index = (target_profile.theme_index.max(0) as usize) % ui::theme::THEMES.len();
    state.auth = config::AuthConfig::load_for_profile(&target_profile.id);
    if let Some(pb_settings) =
        db::user::get_profile_playback_settings(pool, &state.active_profile_id).await
    {
        state.subtitle_delay_ms = pb_settings.subtitle_delay_ms.clamp(-30_000, 30_000);
        state.preferred_subtitle_code = pb_settings.subtitle_code;
    } else {
        state.subtitle_delay_ms = 0;
        state.preferred_subtitle_code.clear();
    }
    state.queue = services::queue::load_active_queue(&target_profile.id, now as u64);
    state.queue_cursor = 0;
    state.playlist_cursor = 0;
    state.saved_playlists.clear();
    state.playlist_preview_items.clear();

    state.analytics_data = None;
    state.merge_report = None;
    state.banner_progress = None;
    state.watched_episodes.clear();
    state.anime_progress.clear();
    state.in_watchlist = false;
    state.row_offsets.insert("continue_watching".to_string(), 0);
    state.row_offsets.insert("watchlist".to_string(), 0);

    let r_state = state.remote_state.clone();
    let p_name = state.active_profile_name.clone();
    let p_icon = state.active_profile_icon.clone();
    let p_sub_delay = state.subtitle_delay_secs();
    let p_queue = build_remote_queue_snapshot(&state.queue, now as u64);
    let p_auto = state.queue.auto_advance;
    let p_loop = state.queue.loop_mode;
    tokio::spawn(async move {
        {
            let mut st = r_state.write().await;
            st.active_profile = p_name;
            st.profile_icon = p_icon;
            st.sub_delay = p_sub_delay;
            st.queue = p_queue;
            st.queue_auto_advance = p_auto;
            st.queue_loop = p_loop;
        }
        services::remote::broadcast_state(&r_state).await;
    });

    if let Ok(new_home) = services::sync::load_cached_home_data(pool).await {
        *home_data = new_home;
        if row_len(&state.active_row, home_data) == 0 {
            state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
        }
        refresh_home_cover(state, home_data, pool, tx);
        prefetch_all_home_covers(home_data, state, pool, tx);
    }

    if state.auth.is_authenticated() {
        let pool2 = pool.clone();
        let cfg2 = cfg.clone();
        let tx2 = tx.clone();
        let auth2 = state.auth.clone();
        tokio::spawn(async move {
            let client = api::anilist::AniListClient::with_token(auth2.anilist_token());
            let mal_cid = cfg2.trackers.resolved_mal_client_id();
            let now_ts = unix_now();
            if let Ok(data) = services::sync::sync_all(
                &pool2,
                &client,
                Some(&auth2),
                mal_cid.as_deref(),
                cfg2.cache.trending_ttl,
                cfg2.cache.stable_ttl,
                now_ts,
            )
            .await
            {
                let _ = tx2.send(AppMessage::HomeData(Box::new(data))).await;
            }
        });
    }
}

async fn handle_profile_modal(
    key: event::KeyEvent,
    state: &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    cfg: &mut config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    use db::user::PROFILE_AVATAR_PRESETS;
    use state::ProfileInputMode;

    if state.profile_input_mode != ProfileInputMode::None {
        match key.code {
            KeyCode::Esc => {
                state.profile_input_mode = ProfileInputMode::None;
                state.profile_name_input.clear();
            }
            KeyCode::Tab => {
                state.profile_avatar_cursor =
                    (state.profile_avatar_cursor + 1) % PROFILE_AVATAR_PRESETS.len();
            }
            KeyCode::Up | KeyCode::Down => {
                state.profile_audio_dub = !state.profile_audio_dub;
            }
            KeyCode::Backspace => {
                state.profile_name_input.pop();
            }
            KeyCode::Char(c) => {
                if !c.is_control() && state.profile_name_input.chars().count() < 24 {
                    state.profile_name_input.push(c);
                }
            }
            KeyCode::Enter => {
                let name = state.profile_name_input.trim().to_string();
                if name.is_empty() {
                    state.show_toast("Profile name cannot be empty", unix_now());
                    return;
                }
                let (icon, _) = PROFILE_AVATAR_PRESETS
                    [state.profile_avatar_cursor % PROFILE_AVATAR_PRESETS.len()];
                let audio_str = if state.profile_audio_dub { "dub" } else { "sub" };
                let now = unix_now();

                match state.profile_input_mode {
                    ProfileInputMode::CreateName => {
                        match db::user::create_profile(
                            pool,
                            &name,
                            icon,
                            state.theme_index,
                            audio_str,
                            now,
                        )
                        .await
                        {
                            Ok(created) => {
                                state.profile_input_mode = ProfileInputMode::None;
                                state.profile_name_input.clear();
                                apply_profile_switch(&created, state, home_data, pool, cfg, tx)
                                    .await;
                                state.screen = Screen::Home;
                                state.show_toast(
                                    format!(
                                        "◈ Created & switched to {} {}",
                                        created.avatar_icon, created.display_name
                                    ),
                                    now,
                                );
                            }
                            Err(e) => {
                                state.show_toast(format!("Profile error: {e}"), now);
                            }
                        }
                    }
                    ProfileInputMode::RenameName => {
                        if let Some(summary) = state.profiles_list.get(state.profile_cursor).cloned() {
                            let target = summary.profile;
                            let _ = db::user::update_profile(
                                pool,
                                &target.id,
                                &name,
                                icon,
                                target.theme_index.max(0) as usize,
                                audio_str,
                            )
                            .await;
                            if target.id == state.active_profile_id {
                                state.active_profile_name = name.clone();
                                state.active_profile_icon = icon.to_string();
                                cfg.audio_mode = if state.profile_audio_dub {
                                    config::AudioMode::Dub
                                } else {
                                    config::AudioMode::Sub
                                };
                                let _ = cfg.save();
                                let r_state = state.remote_state.clone();
                                let p_name = state.active_profile_name.clone();
                                let p_icon = state.active_profile_icon.clone();
                                tokio::spawn(async move {
                                    {
                                        let mut st = r_state.write().await;
                                        st.active_profile = p_name;
                                        st.profile_icon = p_icon;
                                    }
                                    services::remote::broadcast_state(&r_state).await;
                                });
                            }
                            state.profiles_list =
                                db::user::list_profile_summaries(pool).await.unwrap_or_default();
                            state.profile_input_mode = ProfileInputMode::None;
                            state.profile_name_input.clear();
                            state.show_toast(format!("◈ Updated profile: {} {}", icon, name), now);
                        }
                    }
                    ProfileInputMode::None => {}
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('P') => {
            state.go_back();
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Up | KeyCode::Char('k') => {
            if state.profile_cursor > 0 {
                state.profile_cursor -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Down | KeyCode::Char('j') => {
            if state.profile_cursor + 1 < state.profiles_list.len() {
                state.profile_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(summary) = state.profiles_list.get(state.profile_cursor).cloned() {
                let target = summary.profile;
                apply_profile_switch(&target, state, home_data, pool, cfg, tx).await;
                state.screen = Screen::Home;
                state.show_toast(
                    format!(
                        "◈ Switched to profile: {} {}",
                        target.avatar_icon, target.display_name
                    ),
                    unix_now(),
                );
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            state.profile_input_mode = ProfileInputMode::CreateName;
            state.profile_name_input.clear();
            state.profile_avatar_cursor = 1;
            state.profile_audio_dub = false;
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            if let Some(summary) = state.profiles_list.get(state.profile_cursor) {
                state.profile_input_mode = ProfileInputMode::RenameName;
                state.profile_name_input = summary.profile.display_name.clone();
                state.profile_avatar_cursor = PROFILE_AVATAR_PRESETS
                    .iter()
                    .position(|(ic, _)| *ic == summary.profile.avatar_icon)
                    .unwrap_or(0);
                state.profile_audio_dub = summary.profile.audio_mode.eq_ignore_ascii_case("dub");
            }
        }
        KeyCode::Char('i') | KeyCode::Char('I') => {
            if let Some(summary) = state.profiles_list.get(state.profile_cursor).cloned() {
                let target = summary.profile;
                let cur_idx = PROFILE_AVATAR_PRESETS
                    .iter()
                    .position(|(ic, _)| *ic == target.avatar_icon)
                    .unwrap_or(0);
                let (next_icon, _) =
                    PROFILE_AVATAR_PRESETS[(cur_idx + 1) % PROFILE_AVATAR_PRESETS.len()];
                let _ = db::user::update_profile(
                    pool,
                    &target.id,
                    &target.display_name,
                    next_icon,
                    target.theme_index.max(0) as usize,
                    &target.audio_mode,
                )
                .await;
                if target.id == state.active_profile_id {
                    state.active_profile_icon = next_icon.to_string();
                }
                state.profiles_list =
                    db::user::list_profile_summaries(pool).await.unwrap_or_default();
                state.show_toast(
                    format!("Badge: {} {}", next_icon, target.display_name),
                    unix_now(),
                );
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if let Some(summary) = state.profiles_list.get(state.profile_cursor).cloned() {
                let target = summary.profile;
                let next_audio = if target.audio_mode.eq_ignore_ascii_case("dub") {
                    "sub"
                } else {
                    "dub"
                };
                let _ = db::user::update_profile(
                    pool,
                    &target.id,
                    &target.display_name,
                    &target.avatar_icon,
                    target.theme_index.max(0) as usize,
                    next_audio,
                )
                .await;
                if target.id == state.active_profile_id {
                    cfg.audio_mode = if next_audio == "dub" {
                        config::AudioMode::Dub
                    } else {
                        config::AudioMode::Sub
                    };
                    let _ = cfg.save();
                }
                state.profiles_list =
                    db::user::list_profile_summaries(pool).await.unwrap_or_default();
                state.show_toast(
                    format!(
                        "{} audio default: {}",
                        target.display_name,
                        next_audio.to_uppercase()
                    ),
                    unix_now(),
                );
            }
        }
        KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
            if let Some(summary) = state.profiles_list.get(state.profile_cursor).cloned() {
                let pid = summary.profile.id.clone();
                if pid == "default" {
                    state.show_toast("Default profile cannot be deleted", unix_now());
                    return;
                }
                if db::user::delete_profile(pool, &pid).await.unwrap_or(false) {
                    config::AuthConfig::remove_profile_dir(&pid);
                    services::queue::remove_active_queue_file(&pid);
                    if pid == state.active_profile_id {
                        if let Ok(def) = db::user::get_active_profile(pool).await {
                            apply_profile_switch(&def, state, home_data, pool, cfg, tx).await;
                        }
                    }
                    state.profiles_list =
                        db::user::list_profile_summaries(pool).await.unwrap_or_default();
                    if state.profile_cursor >= state.profiles_list.len() {
                        state.profile_cursor = state.profiles_list.len().saturating_sub(1);
                    }
                    state.show_toast(
                        format!("Removed profile: {}", summary.profile.display_name),
                        unix_now(),
                    );
                }
            }
        }
        _ => {}
    }
}

// ─── Watch Session Queue & Saved Playlists Hub ────────────────────────────────

fn build_remote_queue_snapshot(
    queue: &services::queue::WatchQueue,
    now_unix: u64,
) -> Vec<services::remote::RemoteQueueItem> {
    queue
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| services::remote::RemoteQueueItem {
            index: idx,
            title: item.title.clone(),
            episode: item.episode,
            status: item.status(now_unix).remote_badge().to_string(),
            quality: item.quality.clone(),
            dub: item.dub,
        })
        .collect()
}

fn sync_remote_queue_snapshot(state: &AppState) {
    let r_state = state.remote_state.clone();
    let snapshot = build_remote_queue_snapshot(&state.queue, unix_now() as u64);
    let auto_adv = state.queue.auto_advance;
    let loop_m = state.queue.loop_mode;
    tokio::spawn(async move {
        {
            let mut st = r_state.write().await;
            st.queue = snapshot;
            st.queue_auto_advance = auto_adv;
            st.queue_loop = loop_m;
        }
        services::remote::broadcast_state(&r_state).await;
    });
}

fn cycle_quality_str(cur: &str) -> String {
    const QUALS: [&str; 4] = ["1080p", "720p", "480p", "360p"];
    let idx = QUALS
        .iter()
        .position(|q| q.eq_ignore_ascii_case(cur))
        .map(|i| (i + 1) % QUALS.len())
        .unwrap_or(0);
    QUALS[idx].to_string()
}

fn cycle_sub_lang_str(cur: &str) -> String {
    const SUBS: [&str; 7] = [
        "English",
        "Arabic",
        "Spanish",
        "French",
        "German",
        "Portuguese",
        "Indonesian",
    ];
    let idx = SUBS
        .iter()
        .position(|s| cur.to_lowercase().contains(&s.to_lowercase()))
        .map(|i| (i + 1) % SUBS.len())
        .unwrap_or(0);
    SUBS[idx].to_string()
}

/// Persist the session queue file and purge stale cached URLs without eagerly pre-fetching CDN URLs prematurely.
fn persist_and_warm_queue(
    state: &mut AppState,
    _cfg:  &config::Config,
    _tx:   &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let now = unix_now() as u64;
    state.queue.invalidate_stale(now);
    services::queue::save_active_queue(&state.active_profile_id, &state.queue);
    sync_remote_queue_snapshot(state);
}

/// Trigger Just-In-Time background warm-up for the front queued item when playback enters the final 90s.
fn warm_next_queue_item_now(
    state: &mut AppState,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let now = unix_now() as u64;
    state.queue.invalidate_stale(now);
    if let Some(target) = state.queue.next_prefetch_target(now) {
        let spec_ver = target.spec_version;
        state.queue.set_resolving(target.uid, true);
        let a4k = state.anime4k_mode;
        let sub_style = cfg.subtitle_style.clone();
        let net_mode = state.network_mode;
        let force_v4 = cfg.force_ipv4;
        let tx2 = tx.clone();
        tokio::spawn(async move {
            let resolved = services::queue::resolve_item_prefetch(
                &target,
                a4k,
                sub_style,
                net_mode,
                force_v4,
                unix_now() as u64,
            )
            .await;
            let _ = tx2
                .send(AppMessage::QueuePrefetchReady {
                    uid: target.uid,
                    spec_version: spec_ver,
                    prefetch: resolved,
                })
                .await;
        });
    }
}

async fn persist_edited_playlist_items(state: &mut AppState, pool: &sqlx::SqlitePool) {
    let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() else {
        return;
    };
    let drafts: Vec<db::playlists::PlaylistItemDraft> = state
        .playlist_preview_items
        .iter()
        .map(|it| db::playlists::PlaylistItemDraft {
            anime_id: it.anime_id,
            mal_id: it.mal_id,
            title: it.title.clone(),
            query: it.query.clone(),
            episode: it.episode,
            season_year: it.season_year,
            quality: it.quality.clone(),
            dub: it.dub,
            sub_lang: it.sub_lang.clone(),
            player: it.player.clone(),
        })
        .collect();
    let now = unix_now();
    if db::playlists::update_playlist_items(pool, &state.active_profile_id, pl.id, &drafts, now)
        .await
        .unwrap_or(false)
    {
        state.saved_playlists = db::playlists::list_playlists_for_profile(
            pool,
            &state.active_profile_id,
        )
        .await
        .unwrap_or_default();
        if let Some(new_idx) = state.saved_playlists.iter().position(|p| p.id == pl.id) {
            state.playlist_cursor = new_idx;
        }
        refresh_playlist_preview(state, pool).await;
    }
}

async fn refresh_playlist_preview(state: &mut AppState, pool: &sqlx::SqlitePool) {
    if let Some(pl) = state.saved_playlists.get(state.playlist_cursor) {
        state.playlist_preview_items =
            db::playlists::get_playlist_items(pool, pl.id).await.unwrap_or_default();
    } else {
        state.playlist_preview_items.clear();
    }
}

async fn open_queue_hub(state: &mut AppState, pool: &sqlx::SqlitePool) {
    let lists = db::playlists::list_playlists_for_profile(pool, &state.active_profile_id)
        .await
        .unwrap_or_default();
    state.open_queue_modal(lists);
    refresh_playlist_preview(state, pool).await;
}

fn build_queue_item_for_anime(
    anime: &db::cache::Anime,
    ep:    u32,
    dub:   bool,
    cfg:   &config::Config,
    state: &AppState,
) -> services::queue::QueueItem {
    let mal_id = state
        .anime_mal_ids
        .get(&anime.id)
        .copied()
        .or(anime.mal_id);
    services::queue::QueueItem::new(
        anime.id,
        mal_id,
        anime.season_year,
        anime.display_title().to_string(),
        anime.playback_query(),
        ep,
        cfg.quality.as_str().to_string(),
        dub,
        cfg.default_sub_lang.clone(),
        state.selected_player().as_str().to_string(),
    )
}

async fn play_queue_item(
    state:          &mut AppState,
    item:           services::queue::QueueItem,
    _used_prefetch: bool,
    retried_once:   bool,
    pool:           &sqlx::SqlitePool,
    cfg:            &config::Config,
    tx:             &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let need_switch = state
        .selected_anime
        .as_ref()
        .is_none_or(|a| a.id != item.anime_id);
    if need_switch {
        let target_anime = match db::cache::get_anime(pool, item.anime_id).await {
            Ok(Some(a)) => a,
            _ => {
                let mut fallback = db::cache::Anime::default();
                fallback.id = item.anime_id;
                fallback.mal_id = item.mal_id;
                fallback.season_year = item.season_year;
                fallback.title_romaji = item.title.clone();
                fallback.title_english = Some(item.title.clone());
                fallback.episodes = Some(item.episode.max(1) as i64);
                fallback
            }
        };
        open_detail_from_anime(state, target_anime, pool, tx).await;
    }

    state.selected_episode = Some(item.episode);
    state.playback_qualities.clear();
    state.playback_sub_languages.clear();
    state.playback_audio_tracks.clear();
    state.pending_master_url = None;
    state.dub_available = None;

    let now = unix_now() as u64;
    let cached_pre = item
        .prefetch
        .clone()
        .filter(|p| p.is_fresh(now) && p.options.stream_url.as_ref().is_some_and(|u| !u.is_empty()));

    let (resolved_pre, actually_prefetched) = if let Some(cached) = cached_pre {
        (Some(cached), true)
    } else {
        let resolved = services::queue::resolve_item_prefetch(
            &item,
            state.anime4k_mode,
            cfg.subtitle_style.clone(),
            state.network_mode,
            cfg.force_ipv4,
            now,
        )
        .await;
        (resolved, false)
    };

    if let Some(ref pre) = resolved_pre {
        if let Some(ref ladder) = pre.ladder {
            state
                .stream_ladder_cache
                .insert((item.anime_id, item.episode, item.dub), ladder.clone());
        }
        state.playback_sub_languages = pre.subtitles.clone();
        api::subtitles::SubtitleOption::ensure_none_option_first(
            &mut state.playback_sub_languages,
        );
        state.playback_audio_tracks = pre.audio_tracks.clone();
        state.pending_master_url = pre.master_url.clone();
        state.dub_available = Some(pre.dub_available || item.dub);
        if pre.options.subtitles_disabled {
            state.playback_sub_cursor = 0;
        } else if let Some(ref chosen_lang) = pre.options.sub_lang {
            state.playback_sub_cursor = state
                .playback_sub_languages
                .iter()
                .position(|s| {
                    s.lang_keyword().eq_ignore_ascii_case(chosen_lang)
                        || s.name.eq_ignore_ascii_case(chosen_lang)
                })
                .unwrap_or(0);
        }
    }

    let sub_disabled = matches!(
        item.sub_lang.trim().to_ascii_lowercase().as_str(),
        "none" | "off" | "no" | "no subtitles"
    );
    let mut opts = resolved_pre
        .map(|r| r.options)
        .unwrap_or_else(|| api::player::PlayOptions {
            title: item.query.clone(),
            episode: item.episode,
            quality: item.quality.clone(),
            dub: item.dub,
            player: item.player.clone(),
            sub_lang: if sub_disabled {
                Some("none".to_string())
            } else {
                Some(item.sub_lang.clone())
            },
            sub_url: None,
            stream_url: None,
            ipc_socket: None,
            start_pos: None,
            glsl_shaders: None,
            sub_style: Some(cfg.subtitle_style.clone()),
            sub_delay_ms: state.subtitle_delay_ms,
            subtitles_disabled: sub_disabled,
            loading_image: None,
            network_mode: state.network_mode,
            force_ipv4: cfg.force_ipv4,
            auto_fullscreen: cfg.auto_fullscreen,
            auto_pip: cfg.auto_pip,
            shader_mode: state.anime4k_mode,
            performance_mode: cfg.performance_mode,
            deband_mode: cfg.deband_mode,
            motion_mode: cfg.motion_mode,
            referer: None,
            audio_urls: Vec::new(),
            has_demuxed_audio: false,
            embedded_sub: Default::default(),
            hardsub_shield: state.hardsub_shield_mode,
            hardsub_band: state.hardsub_band_height,
        });

    opts.ipc_socket = None;
    opts.loading_image = None;
    opts.network_mode = state.network_mode;
    opts.force_ipv4 = cfg.force_ipv4;
    opts.auto_fullscreen = cfg.auto_fullscreen;
    opts.auto_pip = cfg.auto_pip;
    opts.shader_mode = state.anime4k_mode;
    opts.performance_mode = cfg.performance_mode;
    opts.deband_mode = cfg.deband_mode;
    opts.motion_mode = cfg.motion_mode;
    opts.hardsub_shield = state.hardsub_shield_mode;
    opts.hardsub_band = state.hardsub_band_height;
    opts.sub_style = Some(cfg.subtitle_style.clone());
    opts.sub_delay_ms = state.subtitle_delay_ms;
    if state.anime4k_mode != config::Anime4KMode::Off {
        let caps = services::platform::detect_player_capabilities();
        opts.glsl_shaders = services::shaders::build_shader_arg_for_profile(
            state.anime4k_mode,
            cfg.performance_mode,
            item.season_year,
            caps,
        );
    }

    state.queue.active_item = Some(services::queue::ActiveNowItem {
        item: item.clone(),
        used_prefetched: actually_prefetched,
        retried_once,
        from_queue: true,
        completion_reached: false,
        progress_ratio: 0.0,
    });
    state.queue.manual_stop = false;

    let display_title = item.title.clone();
    let ep = item.episode;
    start_playback(state, opts, display_title, ep, tx, pool, cfg).await;
}

fn toggle_batch_dub(state: &mut AppState) {
    if !state.batch_queue_dub && state.dub_available == Some(false) {
        state.show_toast("English Dub is not available for this anime (Sub only)", unix_now());
        return;
    }
    state.batch_queue_dub = !state.batch_queue_dub;
}

async fn handle_batch_queue_modal(
    key:   event::KeyEvent,
    state: &mut AppState,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(anime) = state.selected_anime.clone() else {
        state.is_batch_queue_modal_open = false;
        return;
    };
    let ep = state.selected_episode.unwrap_or(1);
    let max_aired = anime.aired_episodes().max(1);

    if state.batch_queue_mode == 4 {
        match key.code {
            KeyCode::Esc => {
                state.is_batch_queue_modal_open = false;
                return;
            }
            KeyCode::Tab => {
                toggle_batch_dub(state);
                return;
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                state.batch_queue_quality = cycle_quality_str(&state.batch_queue_quality);
                return;
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                state.batch_queue_sub_lang = cycle_sub_lang_str(&state.batch_queue_sub_lang);
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.batch_queue_mode = 3;
                return;
            }
            KeyCode::Backspace => {
                state.batch_queue_range.pop();
                return;
            }
            KeyCode::Char(c) if c.is_ascii_digit() || c == '-' => {
                if state.batch_queue_range.len() < 12 {
                    state.batch_queue_range.push(c);
                }
                return;
            }
            KeyCode::Enter => {}
            _ => return,
        }
    }

    let exec_mode: Option<usize> = match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.is_batch_queue_modal_open = false;
            return;
        }
        KeyCode::Tab => {
            toggle_batch_dub(state);
            return;
        }
        KeyCode::Char('v') | KeyCode::Char('V') => {
            state.batch_queue_quality = cycle_quality_str(&state.batch_queue_quality);
            return;
        }
        KeyCode::Char('l') | KeyCode::Char('L') => {
            state.batch_queue_sub_lang = cycle_sub_lang_str(&state.batch_queue_sub_lang);
            return;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.batch_queue_mode = (state.batch_queue_mode + 1).min(4);
            return;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.batch_queue_mode = state.batch_queue_mode.saturating_sub(1);
            return;
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            state.batch_queue_mode = 4;
            return;
        }
        KeyCode::Char('1') => Some(0),
        KeyCode::Char('2') => Some(1),
        KeyCode::Char('3') => Some(2),
        KeyCode::Char('4') => Some(3),
        KeyCode::Enter => Some(state.batch_queue_mode),
        _ => None,
    };

    let Some(mode) = exec_mode else {
        return;
    };
    state.is_batch_queue_modal_open = false;

    let dub = state.batch_queue_dub;
    let chosen_quality = state.batch_queue_quality.clone();
    let chosen_sub = state.batch_queue_sub_lang.clone();
    let mal_id = state
        .anime_mal_ids
        .get(&anime.id)
        .copied()
        .or(anime.mal_id);
    let player_str = state.selected_player().as_str().to_string();
    let display_title = anime.display_title().to_string();
    let playback_query = anime.playback_query();
    let make_item = |ep_num: u32| -> services::queue::QueueItem {
        services::queue::QueueItem::new(
            anime.id,
            mal_id,
            anime.season_year,
            display_title.clone(),
            playback_query.clone(),
            ep_num,
            chosen_quality.clone(),
            dub,
            chosen_sub.clone(),
            player_str.clone(),
        )
    };

    match mode {
        0 => {
            let item = make_item(ep);
            state.queue.enqueue_next(item);
            state.queue_cursor = 0;
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!("◆ Queued NEXT: {} · Ep {}", anime.display_title(), ep),
                unix_now(),
            );
        }
        1 => {
            let item = make_item(ep);
            let outcome = state.queue.enqueue_back(item);
            state.queue_cursor = state.queue.len().saturating_sub(1);
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!(
                    "◆ Queued: {} · Ep {} (#{})",
                    anime.display_title(),
                    ep,
                    outcome.position
                ),
                unix_now(),
            );
        }
        2 => {
            let end = (ep + 2).min(max_aired);
            let eps: Vec<u32> = (ep..=end).collect();
            let count = eps.len();
            for e in eps {
                state.queue.enqueue_back(make_item(e));
            }
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!("◆ Queued {} eps ({}..{}) of {}", count, ep, end, anime.display_title()),
                unix_now(),
            );
        }
        3 => {
            let start = ep.min(max_aired);
            let eps: Vec<u32> = (start..=max_aired).collect();
            let count = eps.len();
            for e in eps {
                state.queue.enqueue_back(make_item(e));
            }
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!(
                    "◆ Queued remaining {} eps ({}..{}) of {}",
                    count,
                    start,
                    max_aired,
                    anime.display_title()
                ),
                unix_now(),
            );
        }
        4 => {
            let eps = services::queue::parse_batch_queue_input(&state.batch_queue_range, max_aired);
            if eps.is_empty() {
                state.show_toast(
                    format!("No valid episodes in range \"{}\"", state.batch_queue_range),
                    unix_now(),
                );
                return;
            }
            let count = eps.len();
            for e in eps {
                state.queue.enqueue_back(make_item(e));
            }
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!(
                    "◆ Queued {} episode(s) [{}] of {}",
                    count,
                    state.batch_queue_range,
                    anime.display_title()
                ),
                unix_now(),
            );
        }
        _ => {}
    }
}

fn refresh_playlist_transfer_candidates(state: &mut AppState) {
    if state.playlist_transfer_choice != 1 || state.playlist_transfer_input.trim().is_empty() {
        state.playlist_import_candidates.clear();
        state.playlist_import_cursor = 0;
        return;
    }
    let path = services::queue::clean_user_path(&state.playlist_transfer_input);
    state.playlist_import_candidates = services::queue::scan_portable_playlists_in_dir(&path);
    state.playlist_import_cursor = 0;
}

async fn handle_playlist_transfer_modal(
    key: event::KeyEvent,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
) {
    match key.code {
        KeyCode::Esc => {
            state.playlist_transfer_mode = None;
            state.playlist_transfer_choice = 0;
            state.playlist_transfer_input.clear();
            state.playlist_transfer_error = None;
            state.playlist_import_candidates.clear();
            state.playlist_import_cursor = 0;
        }
        KeyCode::Char('1') => {
            state.playlist_transfer_choice = 0;
            state.playlist_transfer_error = None;
            if matches!(&state.playlist_transfer_mode, Some(services::queue::PlaylistTransferKind::Import)) {
                state.playlist_import_candidates = if let Ok(dir) = services::queue::playlists_export_dir(&state.active_profile_id) {
                    services::queue::scan_portable_playlists_in_dir(&dir)
                } else { Vec::new() };
            }
        }
        KeyCode::Char('2') => {
            state.playlist_transfer_choice = 1;
            state.playlist_transfer_error = None;
            refresh_playlist_transfer_candidates(state);
        }
        KeyCode::Tab => {
            state.playlist_transfer_choice = 1 - state.playlist_transfer_choice;
            state.playlist_transfer_error = None;
            if state.playlist_transfer_choice == 0
                && matches!(&state.playlist_transfer_mode, Some(services::queue::PlaylistTransferKind::Import))
            {
                state.playlist_import_candidates = if let Ok(dir) = services::queue::playlists_export_dir(&state.active_profile_id) {
                    services::queue::scan_portable_playlists_in_dir(&dir)
                } else { Vec::new() };
            } else {
                refresh_playlist_transfer_candidates(state);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.playlist_import_cursor = state.playlist_import_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.playlist_import_cursor + 1 < state.playlist_import_candidates.len() {
                state.playlist_import_cursor += 1;
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') if state.playlist_transfer_choice == 1 => {
            state.playlist_transfer_input = "~/Desktop/".to_string();
            state.playlist_transfer_error = None;
            refresh_playlist_transfer_candidates(state);
        }
        KeyCode::Char(c) if state.playlist_transfer_choice == 1 && !c.is_control() && state.playlist_transfer_input.chars().count() < 4096 => {
            state.playlist_transfer_input.push(c);
            state.playlist_transfer_error = None;
            refresh_playlist_transfer_candidates(state);
        }
        KeyCode::Backspace if state.playlist_transfer_choice == 1 => {
            state.playlist_transfer_input.pop();
            state.playlist_transfer_error = None;
            refresh_playlist_transfer_candidates(state);
        }
        KeyCode::Char('u') if state.playlist_transfer_choice == 1 && key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.playlist_transfer_input.clear();
            state.playlist_transfer_error = None;
            refresh_playlist_transfer_candidates(state);
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if matches!(&state.playlist_transfer_mode, Some(services::queue::PlaylistTransferKind::Import)) {
                let candidates = if state.playlist_transfer_choice == 0 {
                    state.playlist_import_candidates.clone()
                } else {
                    let path = services::queue::clean_user_path(&state.playlist_transfer_input);
                    services::queue::scan_portable_playlists_in_dir(&path)
                };
                let now = unix_now();
                let existing: std::collections::HashSet<String> = state.saved_playlists.iter().map(|p| p.name.to_lowercase()).collect();
                let mut imported = 0usize;
                for (_, pack) in candidates {
                    if existing.contains(&pack.name.to_lowercase()) { continue; }
                    if db::playlists::save_playlist(pool, &state.active_profile_id, &pack.name, pack.loop_default, &pack.items, now).await.is_ok() {
                        imported += 1;
                    }
                }
                state.saved_playlists = db::playlists::list_playlists_for_profile(pool, &state.active_profile_id).await.unwrap_or_default();
                refresh_playlist_preview(state, pool).await;
                state.playlist_transfer_mode = None;
                state.playlist_transfer_error = None;
                state.show_toast(format!("Imported {} playlist pack(s)", imported), now);
            }
        }
        KeyCode::Enter => {
            match state.playlist_transfer_mode.clone() {
                Some(services::queue::PlaylistTransferKind::Export { playlist_id, name, loop_default }) => {
                    let items = db::playlists::get_playlist_items(pool, playlist_id).await.unwrap_or_default();
                    let drafts: Vec<db::playlists::PlaylistItemDraft> = items.iter().map(|item| services::queue::QueueItem::from_saved(item).to_draft()).collect();
                    let result = if state.playlist_transfer_choice == 0 {
                        services::queue::export_playlist_json(&state.active_profile_id, &name, loop_default, &drafts)
                    } else {
                        match services::queue::resolve_export_target(&state.playlist_transfer_input, &name) {
                            Ok(target) => services::queue::export_playlist_json_to_path(&target, &name, loop_default, &drafts),
                            Err(e) => Err(e),
                        }
                    };
                    match result {
                        Ok(path) => {
                            state.playlist_transfer_mode = None;
                            state.playlist_transfer_error = None;
                            state.show_toast(format!("Exported playlist -> {}", path.display()), unix_now());
                        }
                        Err(e) => state.playlist_transfer_error = Some(format!("Export error: {e}")),
                    }
                }
                Some(services::queue::PlaylistTransferKind::Import) => {
                    let candidates = if state.playlist_transfer_choice == 0 {
                        state.playlist_import_candidates.clone()
                    } else {
                        let path = services::queue::clean_user_path(&state.playlist_transfer_input);
                        if path.as_os_str().is_empty() {
                            state.playlist_transfer_error = Some("Enter or paste a JSON file or folder path.".to_string());
                            Vec::new()
                        } else { services::queue::scan_portable_playlists_in_dir(&path) }
                    };
                    let selected = candidates.get(state.playlist_import_cursor).or_else(|| candidates.first()).cloned();
                    let Some((_, pack)) = selected else {
                        if state.playlist_transfer_error.is_none() {
                            state.playlist_transfer_error = Some("No valid playlist packs found at this path.".to_string());
                        }
                        return;
                    };
                    let existing: std::collections::HashSet<String> = state.saved_playlists.iter().map(|p| p.name.to_lowercase()).collect();
                    if existing.contains(&pack.name.to_lowercase()) {
                        state.playlist_transfer_error = Some(format!("A playlist named \"{}\" already exists.", pack.name));
                        return;
                    }
                    let now = unix_now();
                    match db::playlists::save_playlist(pool, &state.active_profile_id, &pack.name, pack.loop_default, &pack.items, now).await {
                        Ok(_) => {
                            state.saved_playlists = db::playlists::list_playlists_for_profile(pool, &state.active_profile_id).await.unwrap_or_default();
                            refresh_playlist_preview(state, pool).await;
                            state.playlist_transfer_mode = None;
                            state.playlist_transfer_error = None;
                            state.show_toast(format!("Imported playlist \"{}\"", pack.name), now);
                        }
                        Err(e) => state.playlist_transfer_error = Some(format!("Import error: {e}")),
                    }
                }
                None => {}
            }
        }
        _ => {}
    }
}

async fn handle_queue_modal(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.playlist_transfer_mode.is_some() {
        handle_playlist_transfer_modal(key, state, pool).await;
        return;
    }

    if state.queue_save_mode {
        match key.code {
            KeyCode::Esc => {
                state.queue_save_mode = false;
                state.queue_save_input.clear();
            }
            KeyCode::Backspace => {
                state.queue_save_input.pop();
            }
            KeyCode::Char(c) => {
                if !c.is_control() && state.queue_save_input.chars().count() < 36 {
                    state.queue_save_input.push(c);
                }
            }
            KeyCode::Enter => {
                let drafts: Vec<db::playlists::PlaylistItemDraft> = state
                    .queue
                    .items
                    .iter()
                    .map(services::queue::QueueItem::to_draft)
                    .collect();
                if drafts.is_empty() {
                    state.queue_save_mode = false;
                    state.show_toast("Queue is empty — nothing to save", unix_now());
                    return;
                }
                let name = state.queue_save_input.trim().to_string();
                let final_name = if name.is_empty() {
                    format!("Watch Mix ({} items)", drafts.len())
                } else {
                    name
                };
                let now = unix_now();
                match db::playlists::save_playlist(
                    pool,
                    &state.active_profile_id,
                    &final_name,
                    state.queue.loop_mode,
                    &drafts,
                    now,
                )
                .await
                {
                    Ok(_) => {
                        state.queue_save_mode = false;
                        state.queue_save_input.clear();
                        state.saved_playlists = db::playlists::list_playlists_for_profile(
                            pool,
                            &state.active_profile_id,
                        )
                        .await
                        .unwrap_or_default();
                        state.playlist_cursor = 0;
                        refresh_playlist_preview(state, pool).await;
                        state.show_toast(format!("❖ Saved playlist: \"{}\"", final_name), now);
                    }
                    Err(e) => {
                        state.show_toast(format!("Playlist save failed: {e}"), now);
                    }
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc if state.queue_focus_pane == 1 && state.playlist_focus_items => {
            state.playlist_focus_items = false;
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            state.playlist_focus_items = false;
            state.go_back();
        }
        KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
            if !(state.queue_focus_pane == 0 && key.code == KeyCode::Char('l'))
                && !(state.queue_focus_pane == 1 && state.playlist_focus_items && key.code == KeyCode::Char('l')) =>
        {
            state.queue_focus_pane = 1 - (state.queue_focus_pane % 2);
            state.playlist_focus_items = false;
            if state.queue_focus_pane == 1 {
                refresh_playlist_preview(state, pool).await;
            }
        }
        KeyCode::Char('e') | KeyCode::Char('E') if state.queue_focus_pane == 1 => {
            if state.playlist_focus_items {
                state.playlist_focus_items = false;
            } else if !state.playlist_preview_items.is_empty() {
                state.playlist_focus_items = true;
                if state.playlist_item_cursor >= state.playlist_preview_items.len() {
                    state.playlist_item_cursor = 0;
                }
            } else {
                state.show_toast("Selected playlist has no items to edit", unix_now());
            }
        }
        KeyCode::Char(' ') => {
            state.queue.auto_advance = !state.queue.auto_advance;
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!(
                    "◆ Queue Auto-Advance: {}",
                    if state.queue.auto_advance { "ON" } else { "OFF" }
                ),
                unix_now(),
            );
        }
        KeyCode::Char('L') => {
            state.queue.loop_mode = !state.queue.loop_mode;
            persist_and_warm_queue(state, cfg, tx);
            state.show_toast(
                format!("◆ Queue Loop: {}", if state.queue.loop_mode { "ON" } else { "OFF" }),
                unix_now(),
            );
        }
        KeyCode::Char('R') => {
            if state.queue.len() > 1 {
                state.queue.shuffle();
                state.queue_cursor = 0;
                persist_and_warm_queue(state, cfg, tx);
                state.show_toast("◆ Shuffled session queue", unix_now());
            }
        }
        KeyCode::Char('C') => {
            if !state.queue.is_empty() {
                state.queue.clear();
                state.queue_cursor = 0;
                persist_and_warm_queue(state, cfg, tx);
                state.show_toast("Cleared session queue", unix_now());
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            if state.queue.is_empty() {
                state.show_toast("Add episodes to the queue before saving a playlist", unix_now());
            } else {
                state.queue_save_mode = true;
                state.queue_save_input.clear();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.queue_focus_pane == 0 {
                if state.queue_cursor + 1 < state.queue.len() {
                    state.queue_cursor += 1;
                }
            } else if state.playlist_focus_items {
                if state.playlist_item_cursor + 1 < state.playlist_preview_items.len() {
                    state.playlist_item_cursor += 1;
                }
            } else if state.playlist_cursor + 1 < state.saved_playlists.len() {
                state.playlist_cursor += 1;
                state.playlist_item_cursor = 0;
                refresh_playlist_preview(state, pool).await;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.queue_focus_pane == 0 {
                state.queue_cursor = state.queue_cursor.saturating_sub(1);
            } else if state.playlist_focus_items {
                state.playlist_item_cursor = state.playlist_item_cursor.saturating_sub(1);
            } else if state.playlist_cursor > 0 {
                state.playlist_cursor -= 1;
                state.playlist_item_cursor = 0;
                refresh_playlist_preview(state, pool).await;
            }
        }
        KeyCode::Char('J') => {
            if state.queue_focus_pane == 0 && !state.queue.is_empty() {
                state.queue_cursor = state.queue.move_down(state.queue_cursor);
                persist_and_warm_queue(state, cfg, tx);
            } else if state.queue_focus_pane == 1
                && state.playlist_focus_items
                && state.playlist_item_cursor + 1 < state.playlist_preview_items.len()
            {
                let idx = state.playlist_item_cursor;
                state.playlist_preview_items.swap(idx, idx + 1);
                state.playlist_item_cursor = idx + 1;
                persist_edited_playlist_items(state, pool).await;
            }
        }
        KeyCode::Char('K') => {
            if state.queue_focus_pane == 0 && !state.queue.is_empty() {
                state.queue_cursor = state.queue.move_up(state.queue_cursor);
                persist_and_warm_queue(state, cfg, tx);
            } else if state.queue_focus_pane == 1
                && state.playlist_focus_items
                && state.playlist_item_cursor > 0
                && state.playlist_item_cursor < state.playlist_preview_items.len()
            {
                let idx = state.playlist_item_cursor;
                state.playlist_preview_items.swap(idx, idx - 1);
                state.playlist_item_cursor = idx - 1;
                persist_edited_playlist_items(state, pool).await;
            }
        }
        KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace => {
            if state.queue_focus_pane == 0 {
                if let Some(removed) = state.queue.remove_at(state.queue_cursor) {
                    if state.queue_cursor >= state.queue.len() {
                        state.queue_cursor = state.queue.len().saturating_sub(1);
                    }
                    persist_and_warm_queue(state, cfg, tx);
                    state.show_toast(
                        format!("Removed {} · Ep {}", removed.title, removed.episode),
                        unix_now(),
                    );
                }
            } else if state.playlist_focus_items {
                if state.playlist_item_cursor < state.playlist_preview_items.len() {
                    let removed = state.playlist_preview_items.remove(state.playlist_item_cursor);
                    if state.playlist_item_cursor >= state.playlist_preview_items.len() {
                        state.playlist_item_cursor =
                            state.playlist_preview_items.len().saturating_sub(1);
                    }
                    persist_edited_playlist_items(state, pool).await;
                    state.show_toast(
                        format!("Removed {} · Ep {} from playlist", removed.title, removed.episode),
                        unix_now(),
                    );
                }
            } else if let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() {
                if db::playlists::delete_playlist(pool, &state.active_profile_id, pl.id)
                    .await
                    .unwrap_or(false)
                {
                    state.saved_playlists = db::playlists::list_playlists_for_profile(
                        pool,
                        &state.active_profile_id,
                    )
                    .await
                    .unwrap_or_default();
                    if state.playlist_cursor >= state.saved_playlists.len() {
                        state.playlist_cursor = state.saved_playlists.len().saturating_sub(1);
                    }
                    refresh_playlist_preview(state, pool).await;
                    state.show_toast(format!("Deleted playlist \"{}\"", pl.name), unix_now());
                }
            }
        }
        KeyCode::Char('v') | KeyCode::Char('V') => {
            if state.queue_focus_pane == 0 {
                if let Some(target) = state.queue.items.get(state.queue_cursor).map(|i| i.uid) {
                    let mut next_q = String::new();
                    state.queue.update_item_by_uid(target, |item| {
                        item.quality = cycle_quality_str(&item.quality);
                        next_q = item.quality.clone();
                    });
                    persist_and_warm_queue(state, cfg, tx);
                    state.show_toast(format!("◆ Queue item quality -> {}", next_q), unix_now());
                }
            } else if state.playlist_focus_items {
                if let Some(it) = state.playlist_preview_items.get_mut(state.playlist_item_cursor) {
                    it.quality = cycle_quality_str(&it.quality);
                    let q = it.quality.clone();
                    persist_edited_playlist_items(state, pool).await;
                    state.show_toast(format!("◆ Playlist item quality -> {}", q), unix_now());
                }
            }
        }
        KeyCode::Char('l') if state.queue_focus_pane == 0 || state.playlist_focus_items => {
            if state.queue_focus_pane == 0 {
                if let Some(target) = state.queue.items.get(state.queue_cursor).map(|i| i.uid) {
                    let mut next_sub = String::new();
                    state.queue.update_item_by_uid(target, |item| {
                        item.sub_lang = cycle_sub_lang_str(&item.sub_lang);
                        next_sub = item.sub_lang.clone();
                    });
                    persist_and_warm_queue(state, cfg, tx);
                    state.show_toast(format!("◆ Queue item subtitle -> {}", next_sub), unix_now());
                }
            } else if let Some(it) = state.playlist_preview_items.get_mut(state.playlist_item_cursor) {
                it.sub_lang = cycle_sub_lang_str(&it.sub_lang);
                let s = it.sub_lang.clone();
                persist_edited_playlist_items(state, pool).await;
                state.show_toast(format!("◆ Playlist item subtitle -> {}", s), unix_now());
            }
        }
        KeyCode::Char('c') if state.queue_focus_pane == 0 => {
            if let Some(item) = state.queue.items.get(state.queue_cursor).cloned() {
                let target_anime = match db::cache::get_anime(pool, item.anime_id).await {
                    Ok(Some(a)) => a,
                    _ => {
                        let mut fallback = db::cache::Anime::default();
                        fallback.id = item.anime_id;
                        fallback.mal_id = item.mal_id;
                        fallback.season_year = item.season_year;
                        fallback.title_romaji = item.title.clone();
                        fallback.title_english = Some(item.title.clone());
                        fallback.episodes = Some(item.episode.max(1) as i64);
                        fallback
                    }
                };
                open_detail_from_anime(state, target_anime.clone(), pool, tx).await;
                state.selected_episode = Some(item.episode);
                let player = config::Player::from_str_lossy(&item.player);
                state.open_playback_options(item.query.clone(), 0, item.dub, player);
                state.playback_options_purpose =
                    state::PlaybackOptionsPurpose::EditQueueItem { uid: item.uid };
                trigger_stream_detection(
                    &target_anime,
                    item.episode,
                    item.dub,
                    state.playback_request_generation,
                    tx,
                );
            }
        }
        KeyCode::Char('x') => {
            if state.queue_focus_pane == 1 {
                if let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() {
                    if db::playlists::delete_playlist(pool, &state.active_profile_id, pl.id)
                        .await
                        .unwrap_or(false)
                    {
                        state.saved_playlists = db::playlists::list_playlists_for_profile(
                            pool,
                            &state.active_profile_id,
                        )
                        .await
                        .unwrap_or_default();
                        if state.playlist_cursor >= state.saved_playlists.len() {
                            state.playlist_cursor = state.saved_playlists.len().saturating_sub(1);
                        }
                        refresh_playlist_preview(state, pool).await;
                        state.show_toast(format!("Deleted playlist \"{}\"", pl.name), unix_now());
                    }
                }
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if state.queue_focus_pane == 0 {
                if let Some(target) = state.queue.items.get(state.queue_cursor).map(|i| i.uid) {
                    let mut mode_str = "SUB";
                    state.queue.update_item_by_uid(target, |item| {
                        item.dub = !item.dub;
                        mode_str = if item.dub { "DUB" } else { "SUB" };
                    });
                    persist_and_warm_queue(state, cfg, tx);
                    state.show_toast(format!("◆ Queue item audio -> {}", mode_str), unix_now());
                }
            } else if state.queue_focus_pane == 1 && state.playlist_focus_items {
                if let Some(it) = state.playlist_preview_items.get_mut(state.playlist_item_cursor) {
                    it.dub = !it.dub;
                    let mode_str = if it.dub { "DUB" } else { "SUB" };
                    persist_edited_playlist_items(state, pool).await;
                    state.show_toast(format!("◆ Playlist item audio -> {}", mode_str), unix_now());
                }
            } else if state.queue_focus_pane == 1 {
                if let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() {
                    let items = db::playlists::get_playlist_items(pool, pl.id)
                        .await
                        .unwrap_or_default();
                    let count = items.len();
                    for saved in &items {
                        state
                            .queue
                            .enqueue_back(services::queue::QueueItem::from_saved(saved));
                    }
                    persist_and_warm_queue(state, cfg, tx);
                    state.show_toast(
                        format!("◆ Appended {} items from \"{}\"", count, pl.name),
                        unix_now(),
                    );
                }
            }
        }
        KeyCode::Char('w') | KeyCode::Char('W') => {
            if state.queue_focus_pane == 1 {
                if let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() {
                    state.playlist_transfer_mode = Some(services::queue::PlaylistTransferKind::Export {
                        playlist_id: pl.id,
                        name: pl.name,
                        loop_default: pl.loop_default,
                    });
                    state.playlist_transfer_choice = 0;
                    state.playlist_transfer_input.clear();
                    state.playlist_transfer_error = None;
                    state.playlist_import_candidates.clear();
                    state.playlist_import_cursor = 0;
                }
            }
        }
        KeyCode::Char('i') | KeyCode::Char('I') => {
            state.playlist_transfer_mode = Some(services::queue::PlaylistTransferKind::Import);
            state.playlist_transfer_choice = 0;
            state.playlist_transfer_input.clear();
            state.playlist_transfer_error = None;
            state.playlist_import_cursor = 0;
            state.playlist_import_candidates = if let Ok(dir) = services::queue::playlists_export_dir(&state.active_profile_id) {
                services::queue::scan_portable_playlists_in_dir(&dir)
            } else { Vec::new() };
        }
        KeyCode::Enter => {
            if state.queue_focus_pane == 0 {
                if let Some(item) = state.queue.remove_at(state.queue_cursor) {
                    if state.queue_cursor >= state.queue.len() {
                        state.queue_cursor = state.queue.len().saturating_sub(1);
                    }
                    state.queue.manual_stop = false;
                    state.show_toast(
                        format!("▶ Playing {} · Ep {}", item.title, item.episode),
                        unix_now(),
                    );
                    play_queue_item(state, item, false, false, pool, cfg, tx).await;
                }
            } else if let Some(pl) = state.saved_playlists.get(state.playlist_cursor).cloned() {
                let items = db::playlists::get_playlist_items(pool, pl.id)
                    .await
                    .unwrap_or_default();
                if items.is_empty() {
                    state.show_toast("Playlist is empty", unix_now());
                    return;
                }
                state.queue.clear();
                state.queue.loop_mode = pl.loop_default;
                state.queue.auto_advance = true;
                for saved in &items {
                    state
                        .queue
                        .enqueue_back(services::queue::QueueItem::from_saved(saved));
                }
                state.queue_cursor = 0;
                if let Some(first) = state.queue.pop_next_for_advance() {
                    state.show_toast(
                        format!("▶ Loaded \"{}\" — {} · Ep {}", pl.name, first.title, first.episode),
                        unix_now(),
                    );
                    play_queue_item(state, first, false, false, pool, cfg, tx).await;
                }
            }
        }
        _ => {}
    }
}

async fn handle_remote_modal(
    key: event::KeyEvent,
    state: &mut AppState,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') | KeyCode::Char('M') => {
            state.go_back();
        }
        KeyCode::Char('o') | KeyCode::Char('O') => {
            let url = state.remote_url.clone();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn();
            state.show_toast(format!("Opened {}", url), unix_now());
        }
        KeyCode::Char(' ') => {
            #[cfg(unix)]
            services::remote::send_mpv_command("/tmp/solus-mpv.sock", &["cycle", "pause"]).await;
            #[cfg(windows)]
            services::remote::send_mpv_command(r"\\.\pipe\solus-mpv", &["cycle", "pause"]).await;
            #[cfg(not(any(unix, windows)))]
            services::remote::send_mpv_command("solus-mpv.sock", &["cycle", "pause"]).await;
        }
        _ => {}
    }
}

async fn handle_cast_modal(
    key: event::KeyEvent,
    state: &mut AppState,
    _tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Char('C') => {
            state.go_back();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !state.cast_devices.is_empty() {
                state.cast_selected_idx = (state.cast_selected_idx + 1).min(state.cast_devices.len() - 1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.cast_selected_idx = state.cast_selected_idx.saturating_sub(1);
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            state.is_scanning_cast = true;
            let devs = services::cast::scan_network(500).await;
            state.cast_devices = devs;
            state.is_scanning_cast = false;
        }
        KeyCode::Enter => {
            if let Some(dev) = state.cast_devices.get(state.cast_selected_idx).cloned() {
                state.active_cast_device = Some(dev.clone());
                state.is_casting = true;
                state.screen = Screen::CastDeck;
                state.show_toast(format!("Casting to {}...", dev.name), unix_now());

                let ep = state.selected_episode.unwrap_or(1);
                let start_pos = state.anime_progress.get(&ep).map(|(p, _)| *p).unwrap_or(0.0);
                if let Some(ref anime) = state.selected_anime {
                    let anime_id = anime.id;
                    let mal_id = anime.mal_id;
                    let title = anime.playback_query();
                    let net_mode = current_network_mode();
                    let force_v4 = current_force_ipv4();
                    tokio::spawn(async move {
                        let stream_info = api::subtitles::fetch_stream_info_with_network(
                            anime_id,
                            mal_id,
                            &title,
                            ep,
                            false,
                            force_v4,
                            net_mode.network_timeout_secs(),
                        )
                        .await;
                        let target_url = stream_info.qualities.first().map(|q| q.url.clone())
                            .or(stream_info.master_url);
                        if let Some(url) = target_url {
                            let _ = services::cast::start_casting(&dev, &url, start_pos).await;
                        }
                    });
                }
            }
        }
        _ => {}
    }
}

async fn handle_cast_deck(
    key: event::KeyEvent,
    state: &mut AppState,
) {
    let Some(ref dev) = state.active_cast_device.clone() else {
        state.screen = Screen::Detail;
        return;
    };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Char('C') => {
            let _ = services::cast::send_tv_command(dev, services::cast::TvAction::Stop).await;
            state.is_casting = false;
            state.active_cast_device = None;
            state.screen = Screen::Detail;
            state.show_toast("TV Casting stopped", unix_now());
        }
        KeyCode::Char(' ') => {
            let _ = services::cast::send_tv_command(dev, services::cast::TvAction::TogglePause(true)).await;
        }
        KeyCode::Char('h') | KeyCode::Left => {
            let new_pos = (state.cast_playback_pos - 10.0).max(0.0);
            state.cast_playback_pos = new_pos;
            let _ = services::cast::send_tv_command(dev, services::cast::TvAction::Seek(new_pos)).await;
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let new_pos = state.cast_playback_pos + 10.0;
            state.cast_playback_pos = new_pos;
            let _ = services::cast::send_tv_command(dev, services::cast::TvAction::Seek(new_pos)).await;
        }
        _ => {}
    }
}

fn trigger_analytics_fetch(
    state: &mut AppState,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    state.open_taste_analytics();
    tokio::spawn(async move {
        if let Ok(data) = db::user::compute_user_analytics(&pool).await {
            let _ = tx.send(AppMessage::AnalyticsDataReady(data)).await;
        }
    });
}

// ── Seasonal & Movie Release Calendar ─────────────────────────────────────────

async fn open_and_fetch_calendar(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let now = unix_now();
    let tz = services::calendar::local_tz_offset_secs(pool).await;
    if let Ok((tracked, watched)) = db::user::get_calendar_tracking_snapshot(pool).await {
        state.calendar_tracked_ids = tracked;
        state.calendar_watched_map = watched;
    }
    state.open_calendar(tz, now);
    if state.calendar_cache.contains_key(&0) {
        state.is_loading_calendar = false;
        prefetch_calendar_covers(state, pool, tx);
    }
    // Always fetch or silently refresh current week so newly aired episodes appear accurately
    spawn_calendar_week_fetch(state, 0, tz, now, pool.clone(), tx.clone());
}

fn spawn_calendar_week_fetch(
    state: &mut AppState,
    week_offset: i32,
    tz_offset_secs: i64,
    now: i64,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    if !state.calendar_cache.contains_key(&week_offset) && state.calendar_week_offset == week_offset {
        state.is_loading_calendar = true;
    }
    let bounds = services::calendar::compute_week_bounds(now, tz_offset_secs, week_offset);
    let auth_token = state.auth.anilist_token();
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::with_token(auth_token);
        match client
            .fetch_weekly_schedule(
                bounds.start_unix,
                bounds.end_unix,
                bounds.query_prev_ymd,
                bounds.query_next_ymd,
                tz_offset_secs,
                now,
            )
            .await
        {
            Ok(releases) => {
                for rel in &releases {
                    let _ = db::cache::upsert_anime(&pool, &rel.anime).await;
                }
                let (tracked_ids, watched_map) = db::user::get_calendar_tracking_snapshot(&pool)
                    .await
                    .unwrap_or_default();
                let _ = tx
                    .send(AppMessage::CalendarWeekReady {
                        week_offset,
                        tz_offset_secs,
                        tracked_ids,
                        watched_map,
                        releases,
                    })
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(AppMessage::CalendarWeekFailed(week_offset, err.to_string()))
                    .await;
            }
        }
    });
}

fn calendar_day_entries(state: &AppState) -> Vec<services::calendar::CalendarDayEntry> {
    let now = if state.calendar_today_unix > 0 {
        state.calendar_today_unix
    } else {
        unix_now()
    };
    let tz = state.calendar_tz_offset_secs;
    let bounds = services::calendar::compute_week_bounds(now, tz, state.calendar_week_offset);
    let releases = state
        .calendar_cache
        .get(&state.calendar_week_offset)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let (mut buckets, _) = services::calendar::build_week_buckets(
        releases,
        state.calendar_filter,
        &state.calendar_tracked_ids,
        &state.calendar_watched_map,
        &bounds,
        tz,
    );
    let idx = state.calendar_day_idx.min(6);
    std::mem::take(&mut buckets[idx])
}

fn focused_calendar_entry(state: &AppState) -> Option<services::calendar::CalendarDayEntry> {
    let entries = calendar_day_entries(state);
    if entries.is_empty() {
        None
    } else {
        let idx = state.calendar_cursor.min(entries.len().saturating_sub(1));
        entries.into_iter().nth(idx)
    }
}

fn prefetch_calendar_covers(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let entries = calendar_day_entries(state);
    if entries.is_empty() {
        return;
    }
    let safe_cursor = state.calendar_cursor.min(entries.len().saturating_sub(1));
    if let Some(focused) = entries.get(safe_cursor) {
        let fid = focused.anime().id;
        state.cover_anime_id = Some(fid);
        if state.cover_failed_anime_id != Some(fid) {
            state.cover_failed_anime_id = None;
        }
        if !state.cover_raw_cache.contains_key(&fid)
            && !state.loading_covers.contains(&fid)
        {
            state.loading_covers.insert(fid);
            trigger_cover_download(focused.anime().clone(), pool.clone(), tx.clone());
        }
    }
    for entry in entries.iter().take(16) {
        let id = entry.anime().id;
        if !state.cover_raw_cache.contains_key(&id)
            && !state.loading_covers.contains(&id)
        {
            state.loading_covers.insert(id);
            trigger_cover_download(entry.anime().clone(), pool.clone(), tx.clone());
        }
    }
}

fn switch_calendar_week(
    state: &mut AppState,
    new_offset: i32,
    new_day_idx: usize,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let clamped_offset = new_offset.clamp(-8, 8);
    state.calendar_week_offset = clamped_offset;
    state.calendar_day_idx = new_day_idx.min(6);
    state.calendar_cursor = 0;

    let now = unix_now();
    state.calendar_today_unix = now;
    let tz = state.calendar_tz_offset_secs;

    if state.calendar_cache.contains_key(&clamped_offset) {
        state.is_loading_calendar = false;
        prefetch_calendar_covers(state, pool, tx);
    } else {
        spawn_calendar_week_fetch(state, clamped_offset, tz, now, pool.clone(), tx.clone());
    }
}

async fn handle_calendar(
    key: event::KeyEvent,
    state: &mut AppState,
    _home_data: &ui::home::HomeData,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Char('C') => {
            state.go_back();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if state.calendar_day_idx > 0 {
                state.calendar_day_idx -= 1;
                state.calendar_cursor = 0;
                prefetch_calendar_covers(state, pool, tx);
            } else if state.calendar_week_offset > -8 {
                switch_calendar_week(state, state.calendar_week_offset - 1, 6, pool, tx);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if state.calendar_day_idx < 6 {
                state.calendar_day_idx += 1;
                state.calendar_cursor = 0;
                prefetch_calendar_covers(state, pool, tx);
            } else if state.calendar_week_offset < 8 {
                switch_calendar_week(state, state.calendar_week_offset + 1, 0, pool, tx);
            }
        }
        KeyCode::Char('H') | KeyCode::Char('[') => {
            if state.calendar_week_offset > -8 {
                switch_calendar_week(
                    state,
                    state.calendar_week_offset - 1,
                    state.calendar_day_idx,
                    pool,
                    tx,
                );
            } else {
                state.show_toast("Reached earliest 8-week archive limit", unix_now());
            }
        }
        KeyCode::Char('L') | KeyCode::Char(']') => {
            if state.calendar_week_offset < 8 {
                switch_calendar_week(
                    state,
                    state.calendar_week_offset + 1,
                    state.calendar_day_idx,
                    pool,
                    tx,
                );
            } else {
                state.show_toast("Reached farthest 8-week horizon limit", unix_now());
            }
        }
        KeyCode::Char('t') | KeyCode::Char('T') => {
            let now = unix_now();
            state.calendar_today_unix = now;
            let today_idx = services::calendar::today_weekday_idx(now, state.calendar_tz_offset_secs);
            switch_calendar_week(state, 0, today_idx, pool, tx);
        }
        KeyCode::Char(d) if ('1'..='7').contains(&d) => {
            let day_idx = (d as u8 - b'1') as usize;
            state.calendar_day_idx = day_idx;
            state.calendar_cursor = 0;
            prefetch_calendar_covers(state, pool, tx);
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            state.calendar_filter = state.calendar_filter.next();
            state.calendar_cursor = 0;
            prefetch_calendar_covers(state, pool, tx);
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            let now = unix_now();
            state.calendar_today_unix = now;
            let offset = state.calendar_week_offset;
            let tz = state.calendar_tz_offset_secs;
            spawn_calendar_week_fetch(state, offset, tz, now, pool.clone(), tx.clone());
            state.show_toast("Refreshing release schedule...", now);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let entries = calendar_day_entries(state);
            if !entries.is_empty() && state.calendar_cursor + 1 < entries.len() {
                state.calendar_cursor += 1;
                prefetch_calendar_covers(state, pool, tx);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.calendar_cursor > 0 {
                state.calendar_cursor -= 1;
                prefetch_calendar_covers(state, pool, tx);
            }
        }
        KeyCode::Char('d') => {
            if let Some(entry) = focused_calendar_entry(state) {
                open_detail_from_anime(state, entry.anime().clone(), pool, tx).await;
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = focused_calendar_entry(state) {
                let now = unix_now();
                let ep = entry.release.episode;
                if entry.release.airing_at <= now {
                    let mut anime = entry.anime().clone();
                    if anime.status.as_deref() == Some("NOT_YET_RELEASED") {
                        anime.status = Some("RELEASING".to_string());
                    }
                    if anime.status.as_deref() == Some("RELEASING") {
                        let min_next = (ep as i64).saturating_add(1);
                        if anime.next_airing_episode.unwrap_or(0) < min_next {
                            anime.next_airing_episode = Some(min_next);
                        }
                    } else if anime.episodes.unwrap_or(0) < ep as i64 {
                        anime.episodes = Some(ep as i64);
                    }
                    open_detail_from_anime(state, anime, pool, tx).await;
                    if !state.episode_list.contains(&ep) {
                        state.episode_list.push(ep);
                        state.episode_list.sort_unstable();
                    }
                    state.selected_episode = Some(ep);
                    begin_playback_flow(state, pool, cfg, false, tx).await;
                } else {
                    open_detail_from_anime(state, entry.anime().clone(), pool, tx).await;
                    state.show_toast(
                        format!(
                            "Episode {} airs {} at {} — showing series overview",
                            ep, entry.day_label, entry.local_hhmm
                        ),
                        now,
                    );
                }
            }
        }
        KeyCode::Char('+') => {
            if let Some(entry) = focused_calendar_entry(state) {
                let anime = entry.anime().clone();
                trigger_watchlist_status_picker_flow(anime, false, None, state, pool).await;
            }
        }
        _ => {}
    }
}

// ── Playback screen ───────────────────────────────────────────────────────────

async fn handle_playback(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &mut config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            state.stop_player();
            state.go_back();
        }
        // Next episode without leaving playback
        KeyCode::Char('n')
            if state.selected_anime.is_some() => {
                let ep  = state.selected_episode.unwrap_or(1);
                let max = state.episode_list.last().copied().unwrap_or(1);
                if ep < max {
                    let next = ep + 1;
                    state.selected_episode = Some(next);
                    begin_playback_flow(state, pool, cfg, false, tx).await;
                }
            }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            cycle_active_playback_audio(state, pool, cfg, tx).await;
        }
        KeyCode::Char('[') | KeyCode::Char('-') => {
            adjust_subtitle_delay(state, pool, Some(-100)).await;
        }
        KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') => {
            adjust_subtitle_delay(state, pool, Some(100)).await;
        }
        KeyCode::Char('0') => {
            adjust_subtitle_delay(state, pool, None).await;
        }
        KeyCode::Char('p') => {
            toggle_pip_mode_action(state, cfg);
        }
        KeyCode::Char('P') => {
            cycle_pip_corner_action(state, cfg);
        }
        KeyCode::Char('i') => {
            toggle_info_hud_action(state, false);
        }
        KeyCode::Char('I') => {
            toggle_info_hud_action(state, true);
        }
        KeyCode::Char('b') => {
            state.cycle_hardsub_shield_mode();
            state.show_toast(
                format!("◈ Hardsub Eradicator: {}", state.hardsub_shield_mode.label()),
                unix_now(),
            );
        }
        KeyCode::Char('B') => {
            state.cycle_hardsub_band_height();
            state.show_toast(
                format!("◈ Hardsub Band Height: {}", state.hardsub_band_height.label()),
                unix_now(),
            );
        }
        _ => {}
    }
}

fn toggle_auto_fullscreen_action(state: &mut AppState, cfg: &mut config::Config) {
    cfg.auto_fullscreen = !cfg.auto_fullscreen;
    if cfg.auto_fullscreen {
        cfg.auto_pip = false;
        state.auto_pip = false;
        state.live_playback.pip_mode = false;
    }
    state.auto_fullscreen = cfg.auto_fullscreen;
    let _ = cfg.save();
    services::ipc::send_set_fullscreen(cfg.auto_fullscreen);
    let msg = if cfg.auto_fullscreen {
        "❖ Auto-Fullscreen on Launch: Enabled"
    } else {
        "❖ Auto-Fullscreen on Launch: Disabled"
    };
    state.show_toast(msg, unix_now());
}

fn toggle_pip_mode_action(state: &mut AppState, cfg: &mut config::Config) {
    let next = !(state.live_playback.pip_mode || cfg.auto_pip);
    cfg.auto_pip = next;
    state.auto_pip = next;
    state.live_playback.pip_mode = next;
    if next {
        cfg.auto_fullscreen = false;
        state.auto_fullscreen = false;
    }
    let _ = cfg.save();
    services::ipc::send_set_pip_mode(next);
    let msg = if next {
        "◈ Picture-in-Picture (PiP): Enabled (Floating Corner)"
    } else {
        "◈ Picture-in-Picture (PiP): Disabled (Standard Window)"
    };
    state.show_toast(msg, unix_now());
}

fn cycle_pip_corner_action(state: &mut AppState, cfg: &mut config::Config) {
    let n = services::ipc::PIP_CORNER_LABELS.len();
    if !(state.live_playback.pip_mode || cfg.auto_pip) {
        cfg.auto_pip = true;
        state.auto_pip = true;
        state.live_playback.pip_mode = true;
        cfg.auto_fullscreen = false;
        state.auto_fullscreen = false;
        let _ = cfg.save();
    } else {
        state.pip_corner_idx = (state.pip_corner_idx + 1) % n;
    }
    services::ipc::send_cycle_pip_corner();
    let label = services::ipc::PIP_CORNER_LABELS[state.pip_corner_idx % n];
    state.show_toast(format!("◈ PiP Corner: {label}"), unix_now());
}

fn toggle_info_hud_action(state: &mut AppState, pin: bool) {
    let sent = if pin {
        services::ipc::send_pin_info_hud()
    } else {
        services::ipc::send_toggle_info_hud()
    };
    let msg = if sent {
        if pin {
            "◈ Toggled Pinned 1Hz Bento Info HUD in mpv"
        } else {
            "◈ Toggled 6s Bento Info HUD in mpv"
        }
    } else {
        "Launch playback with mpv to toggle the Bento Info HUD"
    };
    state.show_toast(msg, unix_now());
}

// ── Tailored Algorithm Screens ────────────────────────────────────────────────

fn handle_tailored_wizard(
    key: event::KeyEvent,
    state: &mut AppState,
) {
    let questions = services::tailored::get_questions();
    let step = state.tailored_step.min(questions.len() - 1);
    let q = &questions[step];
    let is_multi = q.is_multi_select;

    match key.code {
        KeyCode::Esc => state.go_back(),
        KeyCode::Up | KeyCode::Char('k') => {
            state.tailored_cursor = state.tailored_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.tailored_cursor + 1 < q.options.len() {
                state.tailored_cursor += 1;
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
            let idx = (c as usize) - ('1' as usize);
            if idx < q.options.len() {
                if is_multi {
                    if state.tailored_answers.deal_breakers.contains(&idx) {
                        state.tailored_answers.deal_breakers.remove(&idx);
                    } else {
                        state.tailored_answers.deal_breakers.insert(idx);
                    }
                } else {
                    state.tailored_answers.choices[step] = idx;
                    if step + 1 < questions.len() {
                        state.tailored_step += 1;
                        state.tailored_cursor = if state.tailored_step == 9 { 0 } else { state.tailored_answers.choices[state.tailored_step] };
                    } else {
                        state.open_tailored_summary();
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            if is_multi {
                let idx = state.tailored_cursor;
                if state.tailored_answers.deal_breakers.contains(&idx) {
                    state.tailored_answers.deal_breakers.remove(&idx);
                } else {
                    state.tailored_answers.deal_breakers.insert(idx);
                }
            } else {
                state.tailored_answers.choices[step] = state.tailored_cursor;
            }
        }
        KeyCode::Enter => {
            if is_multi {
                state.open_tailored_summary();
            } else {
                state.tailored_answers.choices[step] = state.tailored_cursor;
                if step + 1 < questions.len() {
                    state.tailored_step += 1;
                    state.tailored_cursor = if state.tailored_step == 9 { 0 } else { state.tailored_answers.choices[state.tailored_step] };
                } else {
                    state.open_tailored_summary();
                }
            }
        }
        KeyCode::Backspace | KeyCode::Char('b')
            if state.tailored_step > 0 => {
                state.tailored_step -= 1;
                state.tailored_cursor = state.tailored_answers.choices[state.tailored_step];
            }
        _ => {}
    }
}

async fn handle_tailored_summary(
    key: event::KeyEvent,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let questions = services::tailored::get_questions();

    // If currently quick-editing an answer in submodal
    if let Some(edit_idx) = state.tailored_editing_step {
        let q = &questions[edit_idx.min(questions.len() - 1)];
        let is_multi = q.is_multi_select;

        match key.code {
            KeyCode::Esc => {
                state.tailored_editing_step = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.tailored_cursor = state.tailored_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.tailored_cursor + 1 < q.options.len() {
                    state.tailored_cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                if is_multi {
                    let idx = state.tailored_cursor;
                    if state.tailored_answers.deal_breakers.contains(&idx) {
                        state.tailored_answers.deal_breakers.remove(&idx);
                    } else {
                        state.tailored_answers.deal_breakers.insert(idx);
                    }
                } else {
                    state.tailored_answers.choices[edit_idx] = state.tailored_cursor;
                    state.tailored_editing_step = None;
                }
            }
            KeyCode::Enter => {
                if is_multi {
                    state.tailored_editing_step = None;
                } else {
                    state.tailored_answers.choices[edit_idx] = state.tailored_cursor;
                    state.tailored_editing_step = None;
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = (c as usize) - ('1' as usize);
                if idx < q.options.len() {
                    if is_multi {
                        if state.tailored_answers.deal_breakers.contains(&idx) {
                            state.tailored_answers.deal_breakers.remove(&idx);
                        } else {
                            state.tailored_answers.deal_breakers.insert(idx);
                        }
                    } else {
                        state.tailored_answers.choices[edit_idx] = idx;
                        state.tailored_editing_step = None;
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // Main dashboard navigation
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.go_back(),
        KeyCode::Up | KeyCode::Char('k') => {
            state.tailored_summary_cursor = state.tailored_summary_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.tailored_summary_cursor < 10 {
                state.tailored_summary_cursor += 1;
            }
        }
        KeyCode::Char('s') => {
            trigger_generate_tailored(state, pool, tx);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if state.tailored_summary_cursor == 10 {
                trigger_generate_tailored(state, pool, tx);
            } else {
                let edit_idx = state.tailored_summary_cursor;
                state.tailored_editing_step = Some(edit_idx);
                state.tailored_cursor = if edit_idx == 9 { 0 } else { state.tailored_answers.choices[edit_idx] };
            }
        }
        _ => {}
    }
}

fn trigger_generate_tailored(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    state.is_generating_tailored = true;
    state.show_toast("Attuning recommendations from AniList archives...", unix_now());
    let pool2 = pool.clone();
    let tx2 = tx.clone();
    let answers = state.tailored_answers.clone();
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::new();
        let now = unix_now();
        let matches = services::tailored::generate_tailored_pool_advanced(
            &pool2,
            &client,
            &answers,
            None,
            now,
        ).await;
        let _ = tx2.send(AppMessage::TailoredResultsReady(matches)).await;
    });
}

fn trigger_tuner_peers(
    anchor_id: i64,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::new();
        let now = unix_now();
        let peers = client.fetch_peer_recommendations(anchor_id, now).await.unwrap_or_default();
        for p in &peers {
            let _ = db::cache::upsert_anime(&pool, p).await;
        }
        let _ = tx.send(AppMessage::TunerPeersReady(anchor_id, peers)).await;
    });
}

async fn handle_tailored_results(
    key: event::KeyEvent,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.tuner_open {
        let tag_count = state.tuner_available_tags.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('f') | KeyCode::Char('F') => {
                state.tuner_open = false;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                state.tuner_col = state.tuner_col.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let max_col = if state.tuner_row == 0 {
                    6
                } else {
                    tag_count.saturating_sub(1)
                };
                if state.tuner_col < max_col {
                    state.tuner_col += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.tuner_row = 0;
                state.tuner_col = state.tuner_col.min(6);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if tag_count > 0 {
                    state.tuner_row = 1;
                    state.tuner_col = state.tuner_col.min(tag_count - 1);
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if state.tuner_row == 0 {
                    match state.tuner_col {
                        0..=3 => state.tuner_config.discovery_tier = state.tuner_col,
                        4 => state.tuner_config.hide_sequels = !state.tuner_config.hide_sequels,
                        5 => state.tuner_config.finished_only = !state.tuner_config.finished_only,
                        _ => state.tuner_config.reset(),
                    }
                } else if let Some(tag) = state.tuner_available_tags.get(state.tuner_col).cloned() {
                    if state.tuner_config.boosted_tags.contains(&tag) {
                        state.tuner_config.boosted_tags.remove(&tag);
                        state.tuner_config.muted_tags.insert(tag);
                    } else if state.tuner_config.muted_tags.contains(&tag) {
                        state.tuner_config.muted_tags.remove(&tag);
                    } else {
                        state.tuner_config.boosted_tags.insert(tag);
                    }
                }
                state.recompute_tailored_results();
                if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor) {
                    if !state.cover_cache.contains_key(&focused.anime.id) && !state.loading_covers.contains(&focused.anime.id) {
                        state.loading_covers.insert(focused.anime.id);
                        trigger_cover_download(focused.anime.clone(), pool.clone(), tx.clone());
                    }
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                state.tuner_config.reset();
                state.recompute_tailored_results();
                state.show_toast("Live Vibe Tuner reset", unix_now());
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.screen = Screen::Home;
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            state.toggle_live_tuner();
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if state.tuner_config.has_active_modifiers() {
                state.tuner_config.reset();
                state.recompute_tailored_results();
                state.show_toast("Live Vibe Tuner reset", unix_now());
            }
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor).cloned() {
                let already_anchored = state
                    .tuner_config
                    .anchor_anime
                    .as_ref()
                    .is_some_and(|(id, _, _)| *id == focused.anime.id);
                if already_anchored {
                    state.tuner_config.anchor_anime = None;
                    state.tuner_config.anchor_peer_ids.clear();
                    state.recompute_tailored_results();
                    state.show_toast("Cleared 'More Like This' similarity lock", unix_now());
                } else {
                    let title = focused.anime.display_title().to_string();
                    let genres = focused.anime.genre_list();
                    let anchor_id = focused.anime.id;
                    state.tuner_config.anchor_anime = Some((anchor_id, title.clone(), genres));
                    state.tuner_config.anchor_peer_ids.clear();
                    state.recompute_tailored_results();
                    state.tailored_results_cursor = 0;
                    if let Some(top) = state.tailored_results.first() {
                        state.cover_anime_id = Some(top.anime.id);
                        if !state.cover_cache.contains_key(&top.anime.id) && !state.loading_covers.contains(&top.anime.id) {
                            state.loading_covers.insert(top.anime.id);
                            trigger_cover_download(top.anime.clone(), pool.clone(), tx.clone());
                        }
                    }
                    state.show_toast(format!("Finding shows similar to {}...", title), unix_now());
                    trigger_tuner_peers(anchor_id, pool.clone(), tx.clone());
                }
            }
        }
        KeyCode::Char('x') | KeyCode::Char('X') => {
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor).cloned() {
                let title = focused.anime.short_title();
                state.tuner_config.dismissed_ids.insert(focused.anime.id);
                state.recompute_tailored_results();
                if let Some(next_focused) = state.tailored_results.get(state.tailored_results_cursor) {
                    if !state.cover_cache.contains_key(&next_focused.anime.id) && !state.loading_covers.contains(&next_focused.anime.id) {
                        state.loading_covers.insert(next_focused.anime.id);
                        trigger_cover_download(next_focused.anime.clone(), pool.clone(), tx.clone());
                    }
                }
                state.show_toast(format!("Hidden '{}' · Next best match surfaced", title), unix_now());
            }
        }
        KeyCode::Char('r') => {
            state.screen = Screen::TailoredSummary;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.tailored_results_cursor = state.tailored_results_cursor.saturating_sub(1);
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor) {
                state.cover_anime_id = Some(focused.anime.id);
                if !state.cover_cache.contains_key(&focused.anime.id) && !state.loading_covers.contains(&focused.anime.id) {
                    state.loading_covers.insert(focused.anime.id);
                    trigger_cover_download(focused.anime.clone(), pool.clone(), tx.clone());
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.tailored_results_cursor + 1 < state.tailored_results.len() {
                state.tailored_results_cursor += 1;
                if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor) {
                    state.cover_anime_id = Some(focused.anime.id);
                    if !state.cover_cache.contains_key(&focused.anime.id) && !state.loading_covers.contains(&focused.anime.id) {
                        state.loading_covers.insert(focused.anime.id);
                        trigger_cover_download(focused.anime.clone(), pool.clone(), tx.clone());
                    }
                }
            }
        }
        KeyCode::Enter => {
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor).cloned() {
                open_detail_from_anime(state, focused.anime, pool, tx).await;
                begin_playback_flow(state, pool, cfg, false, tx).await;
            }
        }
        KeyCode::Char('d') => {
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor).cloned() {
                open_detail_from_anime(state, focused.anime, pool, tx).await;
            }
        }
        KeyCode::Char('+') => {
            if let Some(focused) = state.tailored_results.get(state.tailored_results_cursor).cloned() {
                trigger_watchlist_status_picker_flow(focused.anime, false, None, state, pool).await;
            }
        }
        _ => {}
    }
}

fn build_initial_remote_audio_tracks(
    audio_opts: &[api::subtitles::AudioTrackOption],
    dub_available: bool,
    is_dub_active: bool,
) -> Vec<services::remote::RemoteAudioTrack> {
    if audio_opts.len() > 1 {
        let mut tracks: Vec<services::remote::RemoteAudioTrack> = audio_opts
            .iter()
            .enumerate()
            .map(|(idx, opt)| {
                let (label, is_dub) =
                    services::remote::format_audio_track_label(&opt.code, &opt.name);
                services::remote::RemoteAudioTrack {
                    id: (idx + 1) as u32,
                    code: opt.code.clone(),
                    label,
                    is_active: false,
                    is_dub,
                    mpv_aid: Some((idx + 1) as u32),
                }
            })
            .collect();

        let active_idx = if is_dub_active {
            tracks
                .iter()
                .position(|t| t.is_dub && t.code.starts_with("eng"))
                .or_else(|| tracks.iter().position(|t| t.is_dub))
                .unwrap_or(0)
        } else {
            tracks.iter().position(|t| !t.is_dub).unwrap_or(0)
        };
        if let Some(t) = tracks.get_mut(active_idx) {
            t.is_active = true;
        }
        return tracks;
    }

    let mut tracks = vec![services::remote::RemoteAudioTrack {
        id: 1,
        code: "jpn".to_string(),
        label: "Japanese (Original / 日本語)".to_string(),
        is_active: !is_dub_active,
        is_dub: false,
        mpv_aid: if !is_dub_active { Some(1) } else { None },
    }];
    if dub_available || is_dub_active {
        tracks.push(services::remote::RemoteAudioTrack {
            id: 2,
            code: "eng".to_string(),
            label: "English (Dub)".to_string(),
            is_active: is_dub_active,
            is_dub: true,
            mpv_aid: if is_dub_active { Some(1) } else { None },
        });
    }
    tracks
}

/// Toggle or cycle audio during active playback:
/// - Multi-track HLS: cycles embedded `aid` in `mpv` + exact `0s` seek flush (no restart)
/// - Separate Sub/Dub embeds: captures `time_pos` over IPC and hot-swaps stream cleanly
async fn cycle_active_playback_audio(
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.now_playing.is_none() || state.selected_anime.is_none() {
        return;
    }

    let (tracks, current_dub, current_pos) = {
        let st = state.remote_state.read().await;
        (st.audio_tracks.clone(), st.audio_mode == "dub", st.time_pos)
    };

    let aid_tracks: Vec<_> = tracks
        .iter()
        .filter(|t| t.mpv_aid.is_some())
        .cloned()
        .collect();

    // 1. In-stream multi-audio switch via live mpv IPC (zero window restart)
    if aid_tracks.len() > 1 {
        let active_idx = aid_tracks.iter().position(|t| t.is_active).unwrap_or(0);
        let next_track = &aid_tracks[(active_idx + 1) % aid_tracks.len()];
        if let Some(aid) = next_track.mpv_aid {
            let aid_cmd = serde_json::json!({ "command": ["set_property", "aid", aid] }).to_string();
            let seek_cmd = serde_json::json!({ "command": ["seek", 0.0, "relative+exact"] }).to_string();
            let osd_cmd = serde_json::json!({
                "command": ["show-text", format!("✦ Audio: {}", next_track.label), 2500]
            })
            .to_string();
            let sent = services::ipc::try_send_via_active_monitor(&aid_cmd);
            let _ = services::ipc::try_send_via_active_monitor(&seek_cmd);
            let _ = services::ipc::try_send_via_active_monitor(&osd_cmd);
            if sent {
                {
                    let mut st = state.remote_state.write().await;
                    for item in &mut st.audio_tracks {
                        item.is_active = item.id == next_track.id;
                    }
                    st.audio_lang = next_track.label.clone();
                    st.audio_mode = if next_track.is_dub { "dub" } else { "sub" }.to_string();
                }
                services::remote::broadcast_state(&state.remote_state).await;
                state.show_toast(format!("✦ Audio: {}", next_track.label), unix_now());
                return;
            }
        }
    }

    // 2. Separate Sub/Dub embed switch: guard only if verified Sub-only
    let target_dub = !current_dub;
    if target_dub && state.dub_available == Some(false) {
        state.show_toast("English Dub is not available for this episode (Sub only)", unix_now());
        return;
    }

    let ep = state.selected_episode.unwrap_or(1);
    let resume_pos = if current_pos > 2.0 {
        Some(current_pos)
    } else {
        None
    };
    launch_remote_stream_switch(state, pool, cfg, tx, ep, Some(target_dub), resume_pos).await;
}

async fn wrap_opts_with_hls_proxy_if_needed(
    state: &mut AppState,
    cfg: &config::Config,
    opts: &mut api::player::PlayOptions,
    needs_proxy: bool,
) {
    if !needs_proxy {
        return;
    }
    if state.hls_proxy.is_none() {
        if let Ok(srv) = services::hls_proxy::HlsProxyServer::start(cfg.force_ipv4).await {
            state.hls_proxy = Some(std::sync::Arc::new(srv));
        }
    }
    let Some(ref proxy) = state.hls_proxy else {
        return;
    };
    let referer = opts.referer.as_deref().unwrap_or("https://zokoanime.video/");
    if let Some(ref s_url) = opts.stream_url {
        if s_url.starts_with("http://") || s_url.starts_with("https://") {
            opts.stream_url = Some(proxy.wrap_playlist_url(s_url, referer));
        }
    }
    for aurl in &mut opts.audio_urls {
        if aurl.starts_with("http://") || aurl.starts_with("https://") {
            *aurl = proxy.wrap_playlist_url(aurl, referer);
        }
    }
}

/// Non-blocking episode / Sub-Dub stream switch invoked by in-player Next Episode (`n`/`N`/Click) or Mobile Remote.
/// Immediately closes the old player, bumps generations, and fetches the next episode off the UI thread.
async fn launch_remote_stream_switch(
    state: &mut AppState,
    _pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
    ep: u32,
    dub_override: Option<bool>,
    resume_pos: Option<f64>,
) {
    let Some(anime) = state.selected_anime.clone() else {
        return;
    };

    // Bump generations BEFORE stopping old player so superseded PlaybackDone events are dropped cleanly
    state.playback_gen = state.playback_gen.wrapping_add(1);
    state.switch_generation = state.switch_generation.wrapping_add(1);
    let switch_gen = state.switch_generation;
    state.is_switching_episode = true;
    state.last_next_ep_at = Some(std::time::Instant::now());

    let prev_provider_id = state
        .playback_providers
        .get(state.playback_provider_cursor)
        .map(|p| p.id.clone());

    services::remote::quit_active_player().await;
    state.stop_player();
    state.selected_episode = Some(ep);

    let prev = state.last_play_options.clone();
    let is_dub = dub_override.unwrap_or_else(|| {
        prev.as_ref()
            .map(|o| o.dub)
            .unwrap_or(cfg.audio_mode == config::AudioMode::Dub)
    });

    if dub_override.is_some() {
        let mode_label = if is_dub {
            "English (Dub)"
        } else {
            "Japanese (Sub)"
        };
        state.show_toast(format!("❖ Switching audio to {}...", mode_label), unix_now());
    } else {
        state.show_toast(format!("⏭ Loading Episode {}...", ep), unix_now());
    }

    let anime_id = anime.id;
    let identity = anime.search_identity();
    let force_ipv4 = cfg.force_ipv4;
    let timeout_secs = state.network_mode.network_timeout_secs();
    let tx_bg = tx.clone();

    tokio::spawn(async move {
        let stream_info = tokio::time::timeout(
            Duration::from_secs(8),
            api::subtitles::fetch_stream_info_with_identity(
                &identity,
                ep,
                is_dub,
                force_ipv4,
                timeout_secs,
            ),
        )
        .await
        .unwrap_or_else(|_| api::subtitles::StreamInfo::fallback());

        let _ = tx_bg
            .send(AppMessage::StreamSwitchReady {
                switch_gen,
                anime_id,
                episode: ep,
                is_dub,
                resume_pos,
                prev_provider_id,
                stream_info,
            })
            .await;
    });
}

/// Spawn player wrapper with IPC socket, auto-scrobble on 85% completion, auto-skip intro/outro, and exact timestamp resume.
fn resolve_sync_stream_url(
    opts: &api::player::PlayOptions,
    q_pool: &[api::subtitles::QualityOption],
    proxy: Option<&services::hls_proxy::HlsProxyServer>,
) -> Option<String> {
    let referer = opts.referer.as_deref().unwrap_or("https://zokoanime.video/");
    let wrap_if_needed = |raw: &str, force_proxy: bool| -> String {
        let is_hls = raw.contains(".m3u8") || raw.contains("/m3u8");
        if is_hls && !raw.starts_with("http://127.0.0.1:") {
            if let Some(p) = proxy {
                if force_proxy || opts.stream_url.as_deref().is_some_and(|s| s.starts_with("http://127.0.0.1:")) {
                    return p.wrap_playlist_url(raw, referer);
                }
            }
        }
        raw.to_string()
    };

    let preferred_audio = if opts.dub {
        opts.audio_urls.iter().find(|u| {
            let low = u.to_ascii_lowercase();
            low.contains("/en/") || low.contains("english") || low.contains("lang=en")
        })
    } else {
        opts.audio_urls.iter().find(|u| {
            let low = u.to_ascii_lowercase();
            low.contains("/ja/") || low.contains("jpn") || low.contains("japanese") || low.contains("lang=ja")
        })
    }
    .or_else(|| opts.audio_urls.first());

    if let Some(first_audio) = preferred_audio.filter(|u| !u.trim().is_empty()) {
        return Some(wrap_if_needed(first_audio, false));
    }

    if let Some(lowest_q) = q_pool
        .iter()
        .filter(|q| !q.url.trim().is_empty() && !q.label.contains("Mirror"))
        .min_by_key(|q| q.height.unwrap_or(1080))
    {
        if lowest_q.url.contains(".m3u8") || lowest_q.url.contains("/m3u8") {
            return Some(wrap_if_needed(&lowest_q.url, lowest_q.needs_hls_proxy));
        }
    }

    opts.stream_url.clone()
}

fn resolve_clean_ref_sync_source(
    state: &AppState,
    active_pid: Option<&str>,
) -> Option<(String, Option<String>, Option<String>)> {
    let clean = state.playback_providers.iter().find(|p| {
        Some(p.id.as_str()) != active_pid
            && !p.embedded_sub.has_baked_english()
            && !p.qualities.is_empty()
    })?;

    let ref_raw = clean
        .qualities
        .iter()
        .find_map(|q| q.audio_urls.first().filter(|u| !u.trim().is_empty()).cloned())
        .or_else(|| {
            clean
                .qualities
                .iter()
                .filter(|q| !q.url.trim().is_empty() && !q.label.contains("Mirror"))
                .min_by_key(|q| q.height.unwrap_or(1080))
                .map(|q| q.url.clone())
        })?;

    let referer = clean.referer.clone();
    let ref_url = {
        let is_hls = ref_raw.contains(".m3u8") || ref_raw.contains("/m3u8");
        let needs_proxy = clean.needs_hls_proxy
            || clean.qualities.iter().any(|q| q.url == ref_raw && q.needs_hls_proxy);
        if is_hls && needs_proxy && !ref_raw.starts_with("http://127.0.0.1:") {
            if let Some(ref proxy) = state.hls_proxy {
                proxy.wrap_playlist_url(
                    &ref_raw,
                    referer.as_deref().unwrap_or("https://zokoanime.video/"),
                )
            } else {
                ref_raw
            }
        } else {
            ref_raw
        }
    };

    let is_native_synced = |s: &api::subtitles::SubtitleOption| -> bool {
        !s.url.trim().is_empty()
            && !s.stream_label.contains("[Auto-Synced]")
            && !s.url.contains("opensubtitles")
            && !s.url.contains("strem.io")
    };
    let ref_sub = clean
        .subtitles
        .iter()
        .find(|s| {
            is_native_synced(s)
                && (s.code == "eng"
                    || s.code == "en"
                    || s.name.to_ascii_lowercase().contains("english"))
        })
        .or_else(|| clean.subtitles.iter().find(|s| is_native_synced(s)))
        .map(|s| s.url.clone());

    Some((ref_url, referer, ref_sub))
}

async fn start_playback(
    state:   &mut AppState,
    opts:    api::player::PlayOptions,
    title:   String,
    episode: u32,
    tx:      &tokio::sync::mpsc::Sender<AppMessage>,
    pool:    &sqlx::SqlitePool,
    cfg:     &config::Config,
) {
    // Instantly shut down any previous player (incl. detached IINA) and drop its stale IPC stream
    services::remote::quit_active_player().await;
    state.stop_player();
    state.queue.manual_stop = false;
    state.playback_gen = state.playback_gen.wrapping_add(1);
    let my_gen = state.playback_gen;
    state.playback_logs.clear();
    state.now_playing = Some(format!("{} — Episode {}", title, episode));
    state.last_played = state.now_playing.clone();
    state.last_played_anime_id = state.selected_anime.as_ref().map(|anime| anime.id);
    state.screen      = Screen::Detail;
    state.pending_playback_query = None;
    state.invalidate_detail_image_protocols();

    let deps = api::player::detect_dependencies();
    state.set_dependencies(deps.has_ani_cli, deps.has_mpv, deps.has_mpvnet, deps.has_celluloid);
    if opts.stream_url.is_none() && !deps.has_ani_cli {
        state.show_toast("ani-cli is required for Pahe/Gogo streams. Open ! for setup help.", unix_now());
        return;
    }
    if !deps.has_any_player() {
        state.show_toast("No supported libmpv engine found (mpv / mpv.net / Celluloid). Open ! for setup.", unix_now());
        return;
    }

    let mut opts = opts;
    opts.auto_fullscreen = cfg.auto_fullscreen;
    opts.auto_pip = cfg.auto_pip;
    opts.shader_mode = state.anime4k_mode;
    opts.performance_mode = cfg.performance_mode;
    opts.deband_mode = cfg.deband_mode;
    opts.motion_mode = cfg.motion_mode;
    state.last_play_options = Some(opts.clone());
    let anime_id = state.selected_anime.as_ref().map(|a| a.id).unwrap_or(0);
    let season_year = state.selected_anime.as_ref().and_then(|a| a.season_year);
    let mal_for_queue = state
        .anime_mal_ids
        .get(&anime_id)
        .copied()
        .or_else(|| state.selected_anime.as_ref().and_then(|a| a.mal_id));
    if state
        .queue
        .active_item
        .as_ref()
        .is_none_or(|a| a.item.anime_id != anime_id || a.item.episode != episode)
    {
        let matched_front = state
            .queue
            .items
            .front()
            .is_some_and(|head| head.anime_id == anime_id && head.episode == episode);
        let queue_item = if matched_front {
            state.queue.items.pop_front().unwrap()
        } else {
            services::queue::QueueItem::new(
                anime_id,
                mal_for_queue,
                season_year,
                title.clone(),
                opts.title.clone(),
                episode,
                opts.quality.clone(),
                opts.dub,
                opts.sub_lang.clone().unwrap_or_else(|| cfg.default_sub_lang.clone()),
                opts.player.clone(),
            )
        };
        state.queue.active_item = Some(services::queue::ActiveNowItem {
            item: queue_item,
            used_prefetched: false,
            retried_once: false,
            from_queue: matched_front,
            completion_reached: false,
            progress_ratio: 0.0,
        });
    }
    let era = services::shaders::AnimeEra::from_year(season_year);

    // Ensure shaders are Era-Adaptive & capability-aware if Anime4K is enabled
    if state.anime4k_mode != config::Anime4KMode::Off {
        let caps = services::platform::detect_player_capabilities();
        opts.glsl_shaders = services::shaders::build_shader_arg_for_profile(
            state.anime4k_mode,
            cfg.performance_mode,
            season_year,
            caps,
        );
    }

    // 1. Instant 0ms launch if episode was downloaded offline
    let mut is_offline_local = false;
    if let Some(offline_path) = state.offline_episodes.get(&episode) {
        if std::path::Path::new(offline_path).exists() {
            opts.stream_url = Some(offline_path.clone());
            is_offline_local = true;
            state.show_toast(format!("❖ Instant launch from offline vault: Ep {}", episode), unix_now());
        }
    }

    // 2. Exact timestamp resume from saved progress
    if opts.start_pos.is_none() {
        if let Ok(Some((pos, dur))) = db::user::get_playback_progress(pool, anime_id, episode).await {
            if pos > 10.0 && (dur <= 0.0 || pos / dur < services::queue::QUEUE_COMPLETION_THRESHOLD) {
                opts.start_pos = Some(pos);
                state.show_toast(
                    format!("❖ Resuming Ep {} at {:02}:{:02}", episode, (pos as u64) / 60, (pos as u64) % 60),
                    unix_now(),
                );
            }
        }
    }

    // 3. Connect Solus to mpv via IPC socket / named pipe
    #[cfg(unix)]
    let sock_path = format!("/tmp/solus-mpv-{}-{}.sock", anime_id, episode);
    #[cfg(windows)]
    let sock_path = format!(r"\\.\pipe\solus-mpv-{}-{}", anime_id, episode);
    #[cfg(not(any(unix, windows)))]
    let sock_path = format!("solus-mpv-{}-{}.sock", anime_id, episode);

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_file("/tmp/solus-mpv.sock");
        let _ = std::os::unix::fs::symlink(&sock_path, "/tmp/solus-mpv.sock");
    }
    services::ipc::set_playback_paused_for_cache(false);
    let init_h = api::subtitles::parse_height_from_label(&opts.quality).unwrap_or(1080);
    let (init_height, init_fps_x10, init_tbr_kbps) = state
        .stream_ladder_cache
        .get(&(anime_id, episode, opts.dub))
        .and_then(|l| l.select_best_variant(Some(init_h), state.network_mode))
        .map(|v| (v.height, v.fps_x10, v.tbr_kbps))
        .unwrap_or_else(|| {
            (
                init_h,
                240,
                api::subtitles::estimate_tbr_kbps_for_height(init_h),
            )
        });
    state.live_playback.reset_for_session(
        anime_id,
        episode,
        opts.dub,
        is_offline_local,
        opts.player.clone(),
        init_height,
        init_fps_x10,
        init_tbr_kbps,
        opts.stream_url.as_deref().unwrap_or(""),
    );
    state.live_playback.pip_mode = opts.auto_pip;

    services::ipc::set_active_socket_path(Some(sock_path.clone()));
    opts.ipc_socket = Some(sock_path.clone());

    // Initialize remote companion state immediately
    let remote_state_init = state.remote_state.clone();
    let r_title = title.clone();
    let r_ep = episode;
    let r_ep_title = state.episode_titles.get(&episode).cloned().unwrap_or_else(|| format!("Episode {}", episode));
    let r_poster = state.selected_anime.as_ref().and_then(|a| a.cover_url.clone());
    let r_total_eps = state.selected_anime.as_ref().and_then(|a| a.episodes).unwrap_or(24);
    let mut ep_items = Vec::new();
    for e in 1..=r_total_eps {
        let e_num = e as u32;
        let e_t = state.episode_titles.get(&e_num).cloned().unwrap_or_else(|| format!("Episode {}", e_num));
        let is_w = state.watched_episodes.contains(&e_num);
        ep_items.push(services::remote::RemoteEpisodeItem {
            ep: e_num,
            title: e_t,
            is_watched: is_w,
            is_current: e_num == episode,
        });
    }
    let r_subs: Vec<services::remote::RemoteSubtitleTrack> = state
        .playback_sub_languages
        .iter()
        .enumerate()
        .map(|(idx, s)| services::remote::RemoteSubtitleTrack {
            id: (idx + 1) as u32,
            code: s.code.clone(),
            label: s.name.clone(),
            is_active: idx == state.playback_sub_cursor,
            url: if s.url.is_empty() { None } else { Some(s.url.clone()) },
        })
        .collect();
    let r_sub_preset = cfg.subtitle_style.preset.label().to_string();
    let r_anime4k = state.anime4k_mode.as_str().to_string();
    let r_audio_mode = if opts.dub { "dub".to_string() } else { "sub".to_string() };
    let r_audio_tracks = build_initial_remote_audio_tracks(
        &state.playback_audio_tracks,
        state.dub_available.unwrap_or(false) || opts.dub,
        opts.dub,
    );
    let r_audio_lang = r_audio_tracks
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.label.clone())
        .unwrap_or_else(|| {
            if opts.dub {
                "English (Dub)".to_string()
            } else {
                "Japanese (Original / 日本語)".to_string()
            }
        });
    let r_aid = anime_id;
    let r_season_year = season_year;
    let r_era_badge = era.short_badge().to_string();
    let r_profile_name = state.active_profile_name.clone();
    let r_profile_icon = state.active_profile_icon.clone();
    let r_queue_snapshot = build_remote_queue_snapshot(&state.queue, unix_now() as u64);
    let r_queue_auto = state.queue.auto_advance;
    let r_queue_loop = state.queue.loop_mode;
    let r_sub_delay = opts.sub_delay_ms as f64 / 1000.0;
    let r_sub_active = !opts.subtitles_disabled;
    let r_net_mode = state.network_mode.as_str().to_string();
    let r_net_tag = state.network_mode.short_tag().to_string();
    let r_auto_abr = state.auto_abr_downshift;
    let r_force_ipv4 = cfg.force_ipv4;
    let r_quality = format!("{}p", init_height);
    let r_theme_id = cfg.remote.theme_id.clone();
    let r_art_mode = cfg.remote.art_mode.clone();
    let r_art_idx = cfg
        .remote
        .art_index
        .unwrap_or_else(|| services::remote::find_remote_theme(&r_theme_id).default_art_idx);

    tokio::spawn(async move {
        {
            let mut st = remote_state_init.write().await;
            st.is_playing = true;
            st.title = r_title;
            st.anime_id = Some(r_aid);
            st.season_year = r_season_year;
            st.era_badge = r_era_badge;
            st.episode = r_ep;
            st.episode_title = r_ep_title;
            st.poster_url = r_poster;
            st.episodes = ep_items;
            if !r_subs.is_empty() {
                st.subtitles = r_subs;
            }
            st.sub_preset = r_sub_preset;
            st.sub_delay = r_sub_delay;
            st.sub_active = r_sub_active;
            st.anime4k = r_anime4k;
            st.audio_mode = r_audio_mode;
            st.audio_lang = r_audio_lang;
            st.audio_tracks = r_audio_tracks;
            st.active_profile = r_profile_name;
            st.profile_icon = r_profile_icon;
            st.queue = r_queue_snapshot;
            st.queue_auto_advance = r_queue_auto;
            st.queue_loop = r_queue_loop;
            st.network_mode = r_net_mode;
            st.network_tag = r_net_tag;
            st.auto_abr = r_auto_abr;
            st.force_ipv4 = r_force_ipv4;
            st.current_quality = r_quality;
            st.buffer_secs = 0.0;
            st.cache_mb = 0.0;
            st.cache_speed_kbps = 0.0;
            st.paused_for_cache = false;
            st.buffering_pct = 0;
            st.stall_count = 0;
            st.remote_theme = r_theme_id;
            st.remote_art_mode = r_art_mode;
            st.remote_art_index = r_art_idx;
        }
        services::remote::broadcast_state(&remote_state_init).await;
    });

    // 4. Smart Subtitle Sync & Local Caching for ALL remote subtitles (both [Auto-Synced] and native .vtt)
    // so (a) subtle lags like +1.2s are fixed via Onset-Weighted / Voice-Lead alignment, and
    // (b) in-player `[ ⚡ SNAP TO VOICE (y) ]` can parse exact dialogue cues locally.
    let needs_sub_prep = !opts.subtitles_disabled
        && opts
            .sub_url
            .as_ref()
            .is_some_and(|u| u.starts_with("http://") || u.starts_with("https://"));
    if needs_sub_prep {
        if let Some(ref sub_u) = opts.sub_url {
            let code = opts.sub_lang.as_deref().unwrap_or("sub");
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default();

            let ref_sub_url = state
                .playback_sub_languages
                .iter()
                .find(|s| {
                    s.url != *sub_u
                        && !s.stream_label.contains("[Auto-Synced]")
                        && (s.url.starts_with("http://") || s.url.starts_with("https://"))
                })
                .map(|s| s.url.as_str())
                .or_else(|| {
                    state
                        .playback_sub_languages
                        .iter()
                        .find(|s| {
                            s.url != *sub_u
                                && s.code.starts_with("eng")
                                && (s.url.starts_with("http://") || s.url.starts_with("https://"))
                        })
                        .map(|s| s.url.as_str())
                });

            let stream_ref = opts.stream_url.as_deref().or(state.pending_master_url.as_deref());
            let prov_hint = state.active_provider_id.as_deref();

            if let Ok((cached_path, sync_res)) = tokio::time::timeout(
                Duration::from_secs(5),
                services::subsync::ensure_synchronized_subtitle_for_provider(
                    &client,
                    sub_u,
                    ref_sub_url,
                    stream_ref,
                    anime_id,
                    episode,
                    code,
                    prov_hint,
                ),
            )
            .await
            {
                opts.sub_url = Some(cached_path);

                if let Some(res) = sync_res {
                    if res.applied {
                        state.show_toast(
                            format!("❖ Auto-Synced {} sub (offset: {:+.2}s)", code, res.offset_seconds),
                            unix_now(),
                        );
                    }
                }
            }
        }
    }

    // 5. Generate rotating Y2K loading screen for direct libmpv engines (mpv / mpv.net)
    // AniSkip intervals are resolved concurrently by the IPC monitor.
    let mal_id_opt = state.anime_mal_ids.get(&anime_id).copied();
    let (resolved_engine, _) = api::player::resolve_engine_executable(&opts.player);

    if opts.stream_url.is_some() && resolved_engine != config::Player::Celluloid {
        let variant = services::loading_art::next_loading_variant_index();
        opts.loading_image =
            services::loading_art::generate_y2k_loading_screen(&title, episode, variant)
                .map(|p| p.to_string_lossy().into_owned());
    }
    let has_loading_screen = opts.loading_image.is_some();

    let mut child = match api::player::spawn_async(&opts) {
        Ok(c) => c,
        Err(e) => {
            state.screen = Screen::Detail;
            state.show_toast(format!("Playback failed: {e}"), unix_now());
            return;
        }
    };

    state.show_toast(
        format!("Launching Episode {} in {}", episode, resolved_engine.label()),
        unix_now(),
    );

    // 6. Record in continue watching (without marking as completed history yet)
    if let Some(ref anime) = state.selected_anime {
        let pool2 = pool.clone();
        let aid   = anime.id;
        let now   = unix_now();
        tokio::spawn(async move {
            let _ = db::user::touch_continue_watching(&pool2, aid, episode as i64, now).await;
        });
    }

    // 7. Spawn libmpv IPC monitor (100% parity across Auto-Best / mpv / mpv.net / Celluloid)
    let has_mpv_ipc = opts.stream_url.is_some();
    let playback_exit_kind = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
        services::ipc::PLAYBACK_EXIT_UNKNOWN,
    ));
    let max_aired = state
        .selected_anime
        .as_ref()
        .map(|a| a.aired_episodes())
        .unwrap_or(0);
    let next_episode = if max_aired == 0 || episode + 1 <= max_aired {
        Some(episode + 1)
    } else {
        None
    };
    let sub_path_for_ctx = if opts.subtitles_disabled {
        None
    } else {
        opts.sub_url.clone()
    };
    state.active_stream_url_for_sync = resolve_sync_stream_url(
        &opts,
        &state.playback_qualities,
        state.hls_proxy.as_deref(),
    );
    state.active_stream_referer = opts.referer.clone();
    state.active_sub_path = sub_path_for_ctx.clone();
    state.active_ref_sync_source = resolve_clean_ref_sync_source(state, state.active_provider_id.as_deref());
    state.sub_sync_in_progress = false;

    // Non-blocking background Reference-Audio + Speech-VAD auto-sync for cross-provider streams
    if has_mpv_ipc
        && !is_offline_local
        && opts.sub_delay_ms == 0
        && state
            .active_provider_id
            .as_deref()
            .is_some_and(|id| id == "animegg" || id == "animeheaven")
    {
        if let (Some(ref s_url), Some(ref sub_p)) = (&state.active_stream_url_for_sync, &sub_path_for_ctx) {
            if !sub_p.starts_with("http://") && !sub_p.starts_with("https://") {
                let s_url_bg = s_url.clone();
                let ref_bg = opts.referer.clone();
                let sub_p_bg = sub_p.clone();
                let anchor_pos = opts.start_pos.filter(|&p| p >= 185.0).unwrap_or(0.0);
                let prov_id_bg = state.active_provider_id.clone().unwrap_or_default();
                let ref_sync_bg = state.active_ref_sync_source.clone();
                let target_lang_bg = if opts.dub { Some("eng") } else { Some("jpn") };
                let force_ipv4_bg = cfg.force_ipv4;
                let tx_bg = tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    let client = api::subtitles::build_http_client(force_ipv4_bg, 8);
                    let (ref_u, ref_r, ref_s) = match ref_sync_bg {
                        Some((u, r, s)) => (Some(u), r, s),
                        None => (None, None, None),
                    };
                    let result = services::subsync::sync_subtitle_with_reference_or_vad_for_lang(
                        &client,
                        &s_url_bg,
                        ref_bg.as_deref(),
                        &sub_p_bg,
                        anchor_pos,
                        ref_u.as_deref(),
                        ref_r.as_deref(),
                        ref_s.as_deref(),
                        target_lang_bg,
                    )
                    .await;
                    if result.applied && result.offset_seconds.abs() >= 0.45 {
                        let _ = tx_bg
                            .send(AppMessage::AutoSyncSubCompleted {
                                anime_id,
                                provider_id: prov_id_bg,
                                result,
                            })
                            .await;
                    }
                });
            }
        }
    }
    let ipc_cfg = services::ipc::IpcMonitorConfig {
        anime_id,
        mal_id: mal_id_opt,
        episode,
        next_episode,
        sub_path: sub_path_for_ctx,
        sock_path,
        pool: pool.clone(),
        tx: tx.clone(),
        auth: state.auth.clone(),
        trackers: cfg.trackers.clone(),
        skip_intervals: Vec::new(),
        auto_skip_intro: cfg.auto_skip_intro,
        has_loading_screen,
        playback_exit_kind: Some(playback_exit_kind.clone()),
    };
    services::ipc::spawn_mpv_ipc_monitor(ipc_cfg);

    // Pre-warm HLS variant ladder in background for instant Auto-ABR downshift if playback stalls
    let need_ladder_warm = has_mpv_ipc
        && !is_offline_local
        && state.auto_abr_downshift
        && state
            .stream_ladder_cache
            .get(&(anime_id, episode, opts.dub))
            .is_none_or(|l| !l.is_fresh(unix_now()));
    if need_ladder_warm {
        let q_title = opts.title.clone();
        let dub_flag = opts.dub;
        let net_mode = state.network_mode;
        let force_v4 = cfg.force_ipv4;
        let tx_ladder = tx.clone();
        tokio::spawn(async move {
            let info = api::subtitles::fetch_stream_info_with_network(
                anime_id,
                mal_id_opt,
                &q_title,
                episode,
                dub_flag,
                force_v4,
                net_mode.network_timeout_secs(),
            )
            .await;
            if let Some(ladder) = info.ladder {
                let _ = tx_ladder
                    .send(AppMessage::StreamLadderResolved {
                        anime_id,
                        episode,
                        is_dub: dub_flag,
                        ladder,
                    })
                    .await;
            }
        });
    }

    // Queue state stays saved immediately; actual N+1 stream warm-up happens Just-In-Time ~90s before EOF.
    persist_and_warm_queue(state, cfg, tx);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    state.player_stop = Some(stop_tx);
    let launch_started = tokio::time::Instant::now();

    if let Some(out) = stdout {
        let tx2 = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx2.send(AppMessage::PlaybackLog(line)).await;
            }
        });
    }

    if let Some(err) = stderr {
        let tx3 = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx3.send(AppMessage::PlaybackLog(line)).await;
            }
        });
    }

    let tx4 = tx.clone();
    tokio::spawn(async move {
        tokio::select! {
            status = child.wait() => {
                if has_mpv_ipc
                    && playback_exit_kind.load(std::sync::atomic::Ordering::Relaxed)
                        == services::ipc::PLAYBACK_EXIT_UNKNOWN
                {
                    // Allow IPC reader task a brief flush so final `end-file` reason settles
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
                let kind = playback_exit_kind.load(std::sync::atomic::Ordering::Relaxed);
                let elapsed_secs = launch_started.elapsed().as_secs();
                let exited_ok = matches!(&status, Ok(st) if st.success());
                let msg = match status {
                    Ok(st) if !st.success() => format!("Player exited with error: {}", st),
                    Ok(st) => format!("Player exited: {}", st),
                    Err(e) => format!("Player wait error: {}", e),
                };
                let _ = tx4.send(AppMessage::PlaybackLog(msg)).await;
                let _ = tx4
                    .send(AppMessage::PlaybackDone {
                        gen: my_gen,
                        exited_ok,
                        natural_eof: kind == services::ipc::PLAYBACK_EXIT_NATURAL_EOF,
                        user_quit: kind == services::ipc::PLAYBACK_EXIT_USER_QUIT,
                        elapsed_secs,
                    })
                    .await;
            }
            _ = stop_rx       => {
                playback_exit_kind.store(
                    services::ipc::PLAYBACK_EXIT_USER_QUIT,
                    std::sync::atomic::Ordering::Relaxed,
                );
                let elapsed_secs = launch_started.elapsed().as_secs();
                let kill_res = child.kill().await;
                let msg = format!("manual stop — kill result: {:?}", kill_res);
                let _ = tx4.send(AppMessage::PlaybackLog(msg)).await;
                let _ = tx4
                    .send(AppMessage::PlaybackDone {
                        gen: my_gen,
                        exited_ok: false,
                        natural_eof: false,
                        user_quit: true,
                        elapsed_secs,
                    })
                    .await;
            }
        }
    });
}

// ─── Cover image download ─────────────────────────────────────────────────────

pub fn prepare_card_cover(img: image::DynamicImage) -> image::DynamicImage {
    prepare_card_cover_with_font(img, (12, 24))
}

pub fn prepare_card_cover_with_font(img: image::DynamicImage, font_size: (u16, u16)) -> image::DynamicImage {
    if img.width() == 0 || img.height() == 0 {
        return img;
    }
    if img.width() <= img.height() {
        // Portrait / square poster: glove-fit 16x11 frame edge-to-edge (~0.727 matches AniList ~0.708)
        scale_to_frame_cover(img, 16, 11, font_size, 0.20)
    } else {
        // Landscape artwork: zoom out inside 16x11 over blurred ambient backdrop
        scale_to_frame_contain(img, 16, 11, font_size)
    }
}

pub fn prepare_portrait_cover(img: image::DynamicImage) -> image::DynamicImage {
    scale_to_frame_cover(img, 16, 12, (12, 24), 0.15)
}

pub fn prepare_portrait_cover_with_font(img: image::DynamicImage, font_size: (u16, u16)) -> image::DynamicImage {
    scale_to_frame_cover(img, 16, 12, font_size, 0.15)
}

pub fn prepare_thumbnail_cover_with_font(
    img: image::DynamicImage,
    target_cols: u16,
    target_rows: u16,
    font_size: (u16, u16),
) -> image::DynamicImage {
    scale_to_frame_cover(img, target_cols, target_rows, font_size, 0.20)
}

pub fn prepare_16_9_thumbnail_cover(
    img: image::DynamicImage,
    font_size: (u16, u16),
) -> image::DynamicImage {
    let font_w = (font_size.0 as f32).max(1.0);
    let font_h = (font_size.1 as f32).max(1.0);
    let target_rows: u16 = 9;
    let target_cols: u16 = ((16.0 * font_h) / font_w).round() as u16;
    scale_to_frame_cover(img, target_cols, target_rows, font_size, 0.20)
}

/// Preload episode thumbnails for the first 16 episodes plus a window around the currently selected episode.
fn preload_nearby_episode_thumbnails(
    state: &mut AppState,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let Some(ref anime) = state.selected_anime else {
        return;
    };
    let anime_id = anime.id;
    let meta_gen = state.episode_metadata_generation;
    let selected = state.selected_episode.unwrap_or(1);
    let win_start = selected.saturating_sub(3).max(1);
    let win_end = selected + 6;

    let targets: Vec<u32> = if state.network_mode == config::NetworkMode::UltraSaver {
        vec![selected, selected + 1]
    } else {
        let mut list: Vec<u32> = (win_start..=win_end).collect();
        for ep in 1..=16 {
            if !list.contains(&ep) {
                list.push(ep);
            }
        }
        list
    };

    for ep in targets {
        if let Some(url) = state.episode_thumbnail_urls.get(&ep).cloned() {
            if !state.episode_thumbnails.contains_key(&ep)
                && !state.loading_episode_thumbnails.contains(&ep)
            {
                state.loading_episode_thumbnails.insert(ep);
                state
                    .episode_thumbnail_states
                    .insert(ep, state::EpisodeThumbnailState::Loading);
                trigger_episode_thumbnail_download(anime_id, ep, meta_gen, url, tx.clone());
            }
        }
    }
}

/// Fetch raw cover image bytes from memory, SQLite `cover_blob`, or the shared pooled HTTP client.
async fn fetch_cover_bytes(
    anime: &db::cache::Anime,
    pool: &sqlx::SqlitePool,
) -> Option<Vec<u8>> {
    if let Some(ref blob) = anime.cover_blob {
        return Some(blob.clone());
    }
    if let Ok(Some(blob)) = db::cache::get_cover_blob(pool, anime.id).await {
        return Some(blob);
    }
    let raw_url = anime.cover_url.as_ref()?;
    wait_while_paused_for_cache().await;
    let mode = current_network_mode();
    let force_v4 = current_force_ipv4();
    let _permit = active_cover_semaphore(mode).acquire().await.ok()?;
    let url = compact_anilist_cover_url(raw_url, mode.prefer_compact_artwork());
    let resp = if force_v4 {
        api::subtitles::build_http_client(true, mode.network_timeout_secs())
            .get(&url)
            .send()
            .await
            .ok()?
    } else {
        cover_http_client().get(&url).send().await.ok()?
    };
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?.to_vec();
    let _ = db::cache::store_cover_blob(pool, anime.id, &bytes).await;
    Some(bytes)
}

/// Spawn a worker task that fetches, full-quality scales, and pre-encodes Home card/banner protocols off the UI thread.
fn trigger_home_cover_preload(
    anime: db::cache::Anime,
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
    picker: ratatui_image::picker::Picker,
    gen: Option<u64>,
) {
    let anime_id = anime.id;
    tokio::spawn(async move {
        let Some(bytes) = fetch_cover_bytes(&anime, &pool).await else {
            let _ = tx.send(AppMessage::PreloadCoverFailed { gen, anime_id }).await;
            return;
        };

        let encoded = tokio::task::spawn_blocking(move || {
            let img = image::load_from_memory(&bytes).ok()?;
            let card_img = prepare_card_cover_with_font(img.clone(), picker.font_size);
            let card_area = home_card_preencode_area();
            let mut card_proto = build_stateful_protocol(picker, card_img.clone());
            let mut banner_proto = build_stateful_protocol(picker, card_img);
            let resize_mode = hd_resize_mode();
            card_proto.resize_encode(&resize_mode, picker.background_color, card_area);
            banner_proto.resize_encode(&resize_mode, picker.background_color, card_area);
            Some((card_proto, banner_proto, img))
        })
        .await
        .ok()
        .flatten();

        match encoded {
            Some((card_proto, banner_proto, raw_img)) => {
                let _ = tx
                    .send(AppMessage::PreloadCoverReady {
                        gen,
                        anime_id,
                        card_proto,
                        banner_proto,
                        raw_img,
                    })
                    .await;
            }
            None => {
                let _ = tx.send(AppMessage::PreloadCoverFailed { gen, anime_id }).await;
            }
        }
    });
}

/// Spawn a background task to fetch (or decode a cached) cover and send CoverReady.
fn trigger_cover_download(
    anime: db::cache::Anime,
    pool:  sqlx::SqlitePool,
    tx:    tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        match download_cover_image(&anime, &pool).await {
            Some(img) => { let _ = tx.send(AppMessage::CoverReady(anime.id, img)).await; }
            None      => { let _ = tx.send(AppMessage::CoverFailed(anime.id)).await; }
        }
    });
}

/// Decode blob from DB or fetch via shared HTTP pool, caching the blob on success.
async fn download_cover_image(
    anime: &db::cache::Anime,
    pool:  &sqlx::SqlitePool,
) -> Option<image::DynamicImage> {
    let bytes = fetch_cover_bytes(anime, pool).await?;
    tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok())
        .await
        .ok()
        .flatten()
}

fn merge_episode_metadata(
    anime_id: i64,
    map: &mut std::collections::HashMap<u32, state::EpisodeMediaMetadata>,
    ep_num: u32,
    title: String,
    synopsis: Option<String>,
    thumbnail_url: Option<String>,
    source: state::ThumbnailSource,
) {
    let entry = map.entry(ep_num).or_insert_with(|| state::EpisodeMediaMetadata {
        anime_id,
        episode: ep_num,
        title: title.clone(),
        synopsis: synopsis.clone(),
        thumbnail_url: thumbnail_url.clone(),
        thumbnail_source: source,
    });
    if entry.title.starts_with("Episode ") && !title.starts_with("Episode ") {
        entry.title = title;
    }
    if entry.synopsis.is_none() && synopsis.is_some() {
        entry.synopsis = synopsis;
    }
    if let Some(url) = thumbnail_url {
        if entry.thumbnail_url.is_none() || source > entry.thumbnail_source {
            entry.thumbnail_url = Some(url);
            entry.thumbnail_source = source;
        }
    }
}

fn trigger_episode_metadata_fetch(
    anime_id: i64,
    metadata_generation: u64,
    title_romaji: String,
    title_english: Option<String>,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut map: std::collections::HashMap<u32, state::EpisodeMediaMetadata> =
            std::collections::HashMap::new();

        // 1. Primary: Fetch from api.ani.zip by exact AniList ID (highest confidence TVDB 16:9 screencaps)
        if let Some(anizip_map) = api::kitsu::fetch_anizip_episodes(&client, anime_id).await {
            for (ep_num, info) in anizip_map {
                merge_episode_metadata(
                    anime_id,
                    &mut map,
                    ep_num,
                    info.title,
                    info.synopsis,
                    info.thumbnail_url,
                    state::ThumbnailSource::AniZip,
                );
            }
        }

        // 2. Fetch from AniList (bannerImage + streamingEpisodes confidence-checked by title)
        if let Ok(meta) = api::anilist::fetch_anime_streaming_and_banner(&client, anime_id).await {
            if let Some(banner_url) = meta.banner_image {
                trigger_anime_banner_download(anime_id, banner_url, tx.clone());
            }

            for (idx, ep) in meta.streaming_episodes.into_iter().enumerate() {
                let raw_title = ep.title.unwrap_or_else(|| format!("Episode {}", idx + 1));
                let parsed =
                    api::anilist::parse_streaming_episode_title(&raw_title, (idx + 1) as u32);
                if parsed.is_special_or_ambiguous {
                    continue;
                }
                let src = if parsed.explicit_number {
                    state::ThumbnailSource::AniListTitle
                } else {
                    state::ThumbnailSource::AniListIndexFallback
                };
                merge_episode_metadata(
                    anime_id,
                    &mut map,
                    parsed.episode,
                    parsed.title,
                    None,
                    ep.thumbnail,
                    src,
                );
            }
        }

        // 3. Kitsu API fallback (higher confidence than AniListIndexFallback)
        let needs_kitsu = map.is_empty()
            || map.values().any(|m| {
                m.thumbnail_url.is_none()
                    || m.synopsis.is_none()
                    || m.thumbnail_source == state::ThumbnailSource::AniListIndexFallback
            });
        if needs_kitsu {
            let primary = title_english.as_deref().unwrap_or(&title_romaji);
            let fallback = if title_english.is_some() {
                Some(title_romaji.as_str())
            } else {
                None
            };

            if let Some(kitsu_map) = api::kitsu::fetch_kitsu_episodes_with_mode(
                &client,
                primary,
                fallback,
                current_network_mode().prefer_compact_artwork(),
            )
            .await
            {
                for (ep_num, k_info) in kitsu_map {
                    merge_episode_metadata(
                        anime_id,
                        &mut map,
                        ep_num,
                        k_info.title,
                        k_info.synopsis,
                        k_info.thumbnail_url,
                        state::ThumbnailSource::Kitsu,
                    );
                }
            }
        }

        let _ = tx
            .send(AppMessage::EpisodeMetadataReady(
                anime_id,
                metadata_generation,
                map,
            ))
            .await;
    });
}

fn trigger_anime_banner_download(
    anime_id: i64,
    url: String,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        if let Some(img) = download_anime_banner(anime_id, &url).await {
            let _ = tx.send(AppMessage::AnimeBannerReady(anime_id, img)).await;
        }
    });
}

async fn download_anime_banner(
    anime_id: i64,
    url: &str,
) -> Option<image::DynamicImage> {
    let cache_dir = config::Config::config_dir()
        .map(|d| d.join("episodes"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/solus_episodes"));
    let _ = std::fs::create_dir_all(&cache_dir);
    let file_path = cache_dir.join(format!("banner_{}.jpg", anime_id));

    if file_path.exists() {
        if let Ok(bytes) = std::fs::read(&file_path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                return Some(img);
            }
        }
    }

    wait_while_paused_for_cache().await;
    let mode = current_network_mode();
    let _permit = active_cover_semaphore(mode).acquire().await.ok()?;
    let client = api::subtitles::build_http_client(current_force_ipv4(), mode.network_timeout_secs());
    let resp = client.get(url).send().await.ok()?;
    let bytes = resp.bytes().await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let _ = std::fs::write(&file_path, &bytes);
    Some(img)
}

pub(crate) fn hash_thumbnail_url(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn thumbnail_cache_filename(anime_id: i64, episode: u32, url: &str) -> String {
    format!("{}_{}_{:016x}.jpg", anime_id, episode, hash_thumbnail_url(url))
}

fn trigger_episode_thumbnail_download(
    anime_id: i64,
    episode: u32,
    metadata_generation: u64,
    url: String,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        if let Some(img) = download_episode_thumbnail(anime_id, episode, &url).await {
            let _ = tx
                .send(AppMessage::EpisodeThumbnailReady(
                    anime_id,
                    episode,
                    metadata_generation,
                    url,
                    img,
                ))
                .await;
        }
    });
}

async fn download_episode_thumbnail(
    anime_id: i64,
    episode: u32,
    url: &str,
) -> Option<image::DynamicImage> {
    let cache_dir = config::Config::config_dir()
        .map(|d| d.join("episodes"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/solus_episodes"));
    let _ = std::fs::create_dir_all(&cache_dir);
    let file_path = cache_dir.join(thumbnail_cache_filename(anime_id, episode, url));

    if file_path.exists() {
        if let Ok(bytes) = std::fs::read(&file_path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                return Some(img);
            }
        }
    }

    wait_while_paused_for_cache().await;
    let mode = current_network_mode();
    let _permit = active_cover_semaphore(mode).acquire().await.ok()?;
    let client = api::subtitles::build_http_client(current_force_ipv4(), mode.network_timeout_secs());
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let _ = std::fs::write(&file_path, &bytes);
    Some(img)
}

// ─── Loading screen ───────────────────────────────────────────────────────────

fn render_loading(frame: &mut ratatui::Frame, state: &AppState) {
    use ratatui::{
        layout::{Alignment, Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::Paragraph,
    };

    let area = frame.area();
    let has_progress = state.preload_total > 0;
    let block_h = if has_progress { 6 } else { 3 };
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(block_h),
            Constraint::Min(0),
        ])
        .split(area);

    let mut lines = vec![Line::from(Span::styled(
        "SOLUS",
        Style::default()
            .fg(Color::Rgb(180, 0, 255))
            .add_modifier(Modifier::BOLD),
    ))];

    if has_progress {
        let total = state.preload_total.max(1);
        let done = state.preload_done.min(total);
        let pct = (done * 100) / total;
        let bar_w: usize = (area.width.saturating_sub(16) as usize).clamp(16, 36);
        let filled = ((done * bar_w) + (total / 2)) / total;
        let filled = filled.min(bar_w);
        let empty = bar_w.saturating_sub(filled);

        lines.push(Line::from(vec![
            Span::styled(
                format!("Loading library artwork  {} / {} ", done, total),
                Style::default().fg(Color::Rgb(210, 210, 230)),
            ),
            Span::styled(
                format!("({}%)", pct),
                Style::default()
                    .fg(Color::Rgb(180, 0, 255))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                "━".repeat(filled),
                Style::default().fg(Color::Rgb(180, 0, 255)),
            ),
            Span::styled(
                "─".repeat(empty),
                Style::default().fg(Color::Rgb(65, 65, 85)),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Esc skip  •  q quit",
            Style::default().fg(Color::Rgb(115, 115, 138)),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Fetching catalog...",
            Style::default().fg(Color::Rgb(140, 140, 165)),
        )));
    }

    let msg = Paragraph::new(lines).alignment(Alignment::Center);

    frame.render_widget(msg, vert[1]);
}

fn render_base_screen(
    frame: &mut ratatui::Frame,
    state: &mut AppState,
    home_data: &ui::home::HomeData,
    base_screen: &Screen,
) {
    match base_screen {
        Screen::Detail if state.selected_anime.is_some() => {
            ui::detail::render_with_image_policy(frame, state, false)
        }
        _ => {
            if state.is_loading {
                render_loading(frame, state);
            } else {
                ui::home::render(frame, state, home_data);
            }
        }
    }
}

// ─── Search helper ────────────────────────────────────────────────────────────

/// Search SQLite cache first (fast), then fall back to AniList network for queries
/// with ≥ 3 characters if local results are sparse. Sends SearchResults messages.
async fn search_and_send(
    pool:  &sqlx::SqlitePool,
    query: &str,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    // Local search — always runs first for instant results
    if let Ok(results) = db::cache::search_cache(pool, query).await {
        let _ = tx.send(AppMessage::SearchResults(results)).await;
    }

    // Network fallback: only when query is long enough and worth the round trip
    if query.len() < 3 {
        return;
    }
    let client = api::anilist::AniListClient::new();
    let now    = unix_now();
    if let Ok(net_results) = client.search(query, now).await {
        // Cache the network results so future local searches benefit
        for anime in &net_results {
            let _ = db::cache::upsert_anime(pool, anime).await;
        }
        // Re-run local search to merge with freshly cached network results
        if let Ok(merged) = db::cache::search_cache(pool, query).await {
            let _ = tx.send(AppMessage::SearchResults(merged)).await;
        }
    }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Current Unix timestamp in seconds.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_card_cover_portrait() {
        // Typical AniList portrait cover dimensions (230x345) -> 16x11 cells @ (12, 24) => 1152x1584
        let img = image::DynamicImage::new_rgb8(230, 345);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 1152);
        assert_eq!(prepared.height(), 1584);
    }

    #[test]
    fn test_prepare_card_cover_contain_preserves_full_portrait_unclipped() {
        use image::GenericImageView;
        // Portrait image with distinct top band (bright red) and bottom band (bright blue)
        let mut rgba = image::RgbaImage::new(200, 300);
        for y in 0..300 {
            for x in 0..200 {
                let px = if y < 60 {
                    image::Rgba([240, 20, 20, 255])
                } else if y >= 240 {
                    image::Rgba([20, 20, 240, 255])
                } else {
                    image::Rgba([20, 220, 20, 255])
                };
                rgba.put_pixel(x, y, px);
            }
        }
        let prepared = prepare_card_cover(image::DynamicImage::ImageRgba8(rgba));
        assert_eq!(prepared.width(), 1152);
        assert_eq!(prepared.height(), 1584);
        // Center column (x=576) must still contain top red band and bottom blue band uncropped
        let top_px = prepared.get_pixel(576, 10);
        let bot_px = prepared.get_pixel(576, 1574);
        assert!(top_px[0] > 180 && top_px[2] < 80, "top band was cropped: {:?}", top_px);
        assert!(bot_px[2] > 180 && bot_px[0] < 80, "bottom band was cropped: {:?}", bot_px);
    }

    #[test]
    fn test_prepare_card_cover_landscape() {
        // Landscape image
        let img = image::DynamicImage::new_rgb8(800, 400);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 1152);
        assert_eq!(prepared.height(), 1584);
    }

    #[test]
    fn test_prepare_card_cover_zero() {
        let img = image::DynamicImage::new_rgb8(0, 0);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 0);
        assert_eq!(prepared.height(), 0);
    }

    #[test]
    fn test_prepare_portrait_cover_dimensions() {
        let img = image::DynamicImage::new_rgb8(230, 345);
        let prepared = prepare_portrait_cover(img);
        assert_eq!(prepared.width(), 1152);
        assert_eq!(prepared.height(), 1728);
    }

    #[test]
    fn test_prepare_16_9_thumbnail_cover_aspect_ratio() {
        let img = image::DynamicImage::new_rgb8(800, 600);
        let prepared = prepare_16_9_thumbnail_cover(img, (12, 24));
        // Ratio must be exactly 16:9
        let aspect = prepared.width() as f64 / prepared.height() as f64;
        let expected = 16.0 / 9.0;
        assert!((aspect - expected).abs() < 0.001);
    }

    #[test]
    fn test_insert_cover_protocols_if_absent_preserves_existing_protocols() {
        let mut state = AppState::new();
        let mut picker = ratatui_image::picker::Picker::new((10, 20));
        picker.protocol_type = ratatui_image::picker::ProtocolType::Kitty;
        state.picker = Some(picker);

        let img1 = image::DynamicImage::new_rgb8(40, 40);
        let img2 = image::DynamicImage::new_rgb8(60, 60);

        insert_cover_protocols_if_absent(&mut state, 42, img1.clone(), img1);
        assert!(has_all_cached_covers(&state, 42));

        let original_ptr = state
            .cover_cache
            .get(&42)
            .map(|p| &**p as *const dyn ratatui_image::protocol::StatefulProtocol as *const ())
            .unwrap();

        // A redundant CoverReady arrival must not replace active Kitty StatefulProtocols.
        insert_cover_protocols_if_absent(&mut state, 42, img2.clone(), img2);
        let after_ptr = state
            .cover_cache
            .get(&42)
            .map(|p| &**p as *const dyn ratatui_image::protocol::StatefulProtocol as *const ())
            .unwrap();
        assert_eq!(original_ptr, after_ptr);
    }

    #[tokio::test]
    async fn test_refresh_home_cover_skips_download_when_already_cached() {
        let pool = db::init(":memory:").await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel::<AppMessage>(8);

        let mut state = AppState::new();
        let mut picker = ratatui_image::picker::Picker::new((10, 20));
        picker.protocol_type = ratatui_image::picker::ProtocolType::Kitty;
        state.picker = Some(picker);

        let anime1 = db::cache::Anime {
            id: 101,
            title_romaji: "Serial Experiments Lain".to_string(),
            ..Default::default()
        };
        let anime2 = db::cache::Anime {
            id: 102,
            title_romaji: "Made in Abyss".to_string(),
            ..Default::default()
        };
        let img = image::DynamicImage::new_rgb8(40, 40);
        insert_cover_protocols_if_absent(&mut state, anime2.id, img.clone(), img);

        let mut data = ui::home::HomeData::empty();
        data.continue_watching.push(anime1);
        data.continue_watching.push(anime2);
        state.active_row = state::CategoryRow::ContinueWatching;
        state.row_offsets.insert("continue_watching".to_string(), 1);

        refresh_home_cover(&mut state, &data, &pool, &tx);

        assert_eq!(state.cover_anime_id, Some(102));
        assert!(!state.loading_covers.contains(&102));
    }

    #[test]
    fn test_collect_unique_home_anime_dedups_and_includes_past_six() {
        let mut data = ui::home::HomeData::empty();
        for id in 1..=10 {
            data.trending.push(db::cache::Anime {
                id,
                title_romaji: format!("Show {}", id),
                ..Default::default()
            });
        }
        // Duplicate an ID in watchlist and add a distinct ID beyond Trending's 6th card.
        data.watchlist.push(data.trending[0].clone());
        data.watchlist.push(db::cache::Anime {
            id: 99,
            title_romaji: "Unique Watchlist".to_string(),
            ..Default::default()
        });
        data.featured = Some(data.trending[0].clone());

        let collected = collect_unique_home_anime(&data);
        let ids: Vec<i64> = collected.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), 11);
        assert!(ids.contains(&10));
        assert!(ids.contains(&99));
        assert_eq!(ids.iter().filter(|&&id| id == 1).count(), 1);
    }

    #[tokio::test]
    async fn test_start_home_preload_preencodes_all_cached_blobs_and_unlocks_home() {
        let pool = db::init(":memory:").await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AppMessage>(32);

        let mut state = AppState::new();
        let mut picker = ratatui_image::picker::Picker::new((12, 24));
        picker.protocol_type = ratatui_image::picker::ProtocolType::Iterm2;
        state.picker = Some(picker);

        // Encode valid PNG bytes for two cards (one beyond index 6 in trending).
        let img = image::DynamicImage::new_rgb8(24, 36);
        let mut png_bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();

        let mut data = ui::home::HomeData::empty();
        for id in 1..=8 {
            data.trending.push(db::cache::Anime {
                id,
                title_romaji: format!("Show {}", id),
                cover_blob: Some(png_bytes.clone()),
                ..Default::default()
            });
        }

        start_home_preload(&data, &mut state, &pool, &tx);
        assert!(state.is_loading);
        assert_eq!(state.preload_total, 8);
        assert_eq!(state.preload_done, 0);

        for _ in 0..8 {
            let msg = tokio::time::timeout(Duration::from_secs(15), rx.recv())
                .await
                .expect("timed out waiting for preload cover")
                .expect("channel closed");
            match msg {
                AppMessage::PreloadCoverReady { gen, anime_id, card_proto, banner_proto, raw_img } => {
                    state.loading_covers.remove(&anime_id);
                    state.cover_cache.entry(anime_id).or_insert(card_proto);
                    state.banner_cache.entry(anime_id).or_insert(banner_proto);
                    state.cover_raw_cache.entry(anime_id).or_insert(raw_img);
                    if let Some(g) = gen {
                        state.advance_preload(g);
                    }
                }
                other => panic!("unexpected message: {:?}", matches!(other, AppMessage::PreloadCoverFailed { .. })),
            }
        }

        assert!(!state.is_loading);
        assert_eq!(state.preload_done, 8);
        for id in 1..=8 {
            assert!(state.cover_cache.contains_key(&id));
            assert!(state.banner_cache.contains_key(&id));
            assert!(state.cover_raw_cache.contains_key(&id));
            // Pre-encoded Iterm2 protocol needs no additional resize at standard card dimensions.
            let card_area = home_card_preencode_area();
            assert!(
                state
                    .cover_cache
                    .get_mut(&id)
                    .unwrap()
                    .needs_resize(&ratatui_image::Resize::Fit(None), card_area)
                    .is_none()
            );
        }
    }

    #[test]
    fn test_parse_cli_profile_arg() {
        assert_eq!(
            parse_cli_profile_arg(&["--profile".into(), "Brother".into()]),
            Some("Brother".to_string())
        );
        assert_eq!(
            parse_cli_profile_arg(&["-p".into(), "alice".into()]),
            Some("alice".to_string())
        );
        assert_eq!(
            parse_cli_profile_arg(&["--profile=Sarah".into()]),
            Some("Sarah".to_string())
        );
        assert_eq!(parse_cli_profile_arg(&[]), None);
    }

    #[test]
    fn test_thumbnail_cache_filename_url_hash_distinct() {
        let f1 = thumbnail_cache_filename(11061, 13, "https://cdn.example.com/ep13-old.jpg");
        let f2 = thumbnail_cache_filename(11061, 13, "https://cdn.example.com/ep13-tvdb.jpg");
        assert!(f1.starts_with("11061_13_"));
        assert!(f2.starts_with("11061_13_"));
        assert!(f1.ends_with(".jpg"));
        assert_ne!(f1, f2);
    }

    #[test]
    fn test_merge_episode_metadata_confidence_prevents_episode_73_overwriting_13() {
        let mut map = std::collections::HashMap::new();
        // Simulates array-index fallback for ep 13 before Kitsu/AniZip arrives.
        merge_episode_metadata(
            11061,
            &mut map,
            13,
            "Episode 13".to_string(),
            None,
            Some("https://anilist.example.com/index-fallback-13.jpg".to_string()),
            state::ThumbnailSource::AniListIndexFallback,
        );
        // Mislabeled entry sitting at streaming_episodes[13] actually claims "Episode 73".
        let parsed_ep73 = api::anilist::parse_streaming_episode_title("Episode 73 - Insanity", 14);
        assert_eq!(parsed_ep73.episode, 73);
        assert!(parsed_ep73.explicit_number);
        merge_episode_metadata(
            11061,
            &mut map,
            parsed_ep73.episode,
            parsed_ep73.title,
            None,
            Some("https://anilist.example.com/ep73.jpg".to_string()),
            state::ThumbnailSource::AniListTitle,
        );
        // Verified Kitsu/AniZip for ep 13 cleanly upgrades the low-confidence AniListIndexFallback.
        merge_episode_metadata(
            11061,
            &mut map,
            13,
            "Letter x From x Gon".to_string(),
            Some("Verified synopsis".to_string()),
            Some("https://tvdb.example.com/ep13.jpg".to_string()),
            state::ThumbnailSource::AniZip,
        );

        let ep13 = map.get(&13).expect("episode 13 entry exists");
        assert_eq!(ep13.title, "Letter x From x Gon");
        assert_eq!(
            ep13.thumbnail_url.as_deref(),
            Some("https://tvdb.example.com/ep13.jpg")
        );
        assert_eq!(ep13.thumbnail_source, state::ThumbnailSource::AniZip);
        assert_eq!(map.get(&73).unwrap().title, "Insanity");
    }
}
