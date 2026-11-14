mod api;
mod config;
mod db;
mod error;
mod services;
mod state;
mod ui;

use anyhow::Context;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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
    /// ani-cli process exited
    PlaybackDone(u64),
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
        iina: bool,
        vlc: bool,
        prompt_setup: bool,
    },
    /// Watchlist changed — send fresh list to update home row immediately
    WatchlistUpdated(Vec<db::cache::Anime>),
    /// Continue Watching changed — send fresh list to update home row immediately
    ContinueWatchingUpdated(Vec<db::cache::Anime>),
    /// Home banner watched-episode count for the active anime
    BannerProgress(i64, usize),
    /// Dynamic stream info (qualities, subtitles, master_url) detected for an anime episode
    StreamInfoDetected(i64, u32, bool, api::subtitles::StreamInfo),
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
    /// Episode titles, streaming thumbnails and synopses loaded (anime_id, map of ep -> (title, Option<thumbnail_url>, Option<synopsis>))
    EpisodeMetadataReady(i64, std::collections::HashMap<u32, (String, Option<String>, Option<String>)>),
    /// Episode thumbnail image downloaded and decoded (anime_id, episode_num, image)
    EpisodeThumbnailReady(i64, u32, image::DynamicImage),
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
    /// Remote touch companion toggled or selected Anime4K shader mode
    RemoteAnime4K(config::Anime4KMode),
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Config + DB ───────────────────────────────────────────────────────────
    let mut cfg = config::Config::load().context("Failed to load config")?;
    let db_path = config::Config::db_path().context("Failed to resolve DB path")?;
    let pool    = db::init(db_path.to_str().unwrap_or(":memory:"))
        .await
        .context("Failed to initialise database")?;

    // ── Terminal setup ────────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
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
    state.anime4k_mode = cfg.anime4k_mode;
    state.ambilight_enabled = cfg.ambilight_enabled;
    state.playback_player_cursor = state.playback_players.iter().position(|p| *p == cfg.player).unwrap_or(0);

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
            let client = api::anilist::AniListClient::with_token(auth2.token.clone());
            let now    = unix_now();
            let result = services::sync::sync_all(
                &pool2, &client, Some(&auth2),
                cfg2.cache.trending_ttl, cfg2.cache.stable_ttl, now,
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
                    state.push_log(line);
                }
                AppMessage::PlaybackDone(gen) => {
                    // Ignore completion events from superseded players (episode switches)
                    if gen != state.playback_gen {
                        continue;
                    }
                    state.now_playing = None;
                    state.player_stop = None;
                    let remote_state_done = state.remote_state.clone();
                    tokio::spawn(async move {
                        {
                            let mut st = remote_state_done.write().await;
                            st.is_playing = false;
                        }
                        services::remote::broadcast_state(&remote_state_done).await;
                    });
                    #[cfg(unix)]
                    let _ = std::fs::remove_file("/tmp/solus-mpv.sock");
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
                            state.episode_fallback_thumbnails.clear();
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
                AppMessage::PreloadCoverReady { gen, anime_id, card_proto, banner_proto } => {
                    state.loading_covers.remove(&anime_id);
                    state.cover_cache.entry(anime_id).or_insert(card_proto);
                    state.banner_cache.entry(anime_id).or_insert(banner_proto);
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
                AppMessage::DependenciesReady { ani_cli, mpv, iina, vlc, prompt_setup } => {
                    state.set_dependencies(ani_cli, mpv, iina, vlc);
                    if prompt_setup && !state.has_any_player() && state.screen != Screen::Setup {
                        state.open_setup();
                    }
                }
                AppMessage::WatchlistUpdated(list) => {
                    home_data.watchlist = list;
                    let max_idx = home_data.watchlist.len().saturating_sub(1);
                    let offset  = state.row_offset("watchlist").min(max_idx);
                    state.row_offsets.insert("watchlist".to_string(), offset);
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
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
                AppMessage::StreamInfoDetected(anime_id, ep, is_dub, info) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id)
                        && state.selected_episode == Some(ep)
                        && state.pending_dub == is_dub
                    {
                        state.playback_qualities = info.qualities;
                        state.playback_sub_languages = info.subtitles;
                        state.playback_providers = info.providers;
                        state.dub_available = Some(info.dub_available);
                        state.pending_master_url = info.master_url;

                        let pref_str = cfg.quality.as_str();
                        state.playback_quality_cursor = state
                            .playback_qualities
                            .iter()
                            .position(|q| q.label == pref_str)
                            .unwrap_or(0);

                        let pref_sub = &cfg.default_sub_lang;
                        state.playback_sub_cursor = state
                            .playback_sub_languages
                            .iter()
                            .position(|s| {
                                s.name.to_lowercase().contains(&pref_sub.to_lowercase())
                                    || s.code.to_lowercase().starts_with(&pref_sub.to_lowercase())
                                    || s.stream_label.to_lowercase().contains(&pref_sub.to_lowercase())
                            })
                            .unwrap_or(0);

                        state.sync_active_provider();
                        state.is_loading_subs = false;

                        if !state.playback_sub_languages.is_empty() {
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
                            tokio::spawn(async move {
                                {
                                    let mut st = r_state.write().await;
                                    st.subtitles = r_subs;
                                }
                                services::remote::broadcast_state(&r_state).await;
                            });
                        }
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
                            state.auth.sync_mode = Some("Local JSON Archive".to_string());
                            state.auth.last_sync_time = Some(unix_now());
                            let _ = state.auth.save();
                            state.login_status = None;
                            state.show_toast(format!("❖ Ingested {} anime, {} episodes", stats.total_anime, stats.episodes_logged), unix_now());
                            state.import_stats = Some(stats);

                            if let Ok(cw) = services::sync::load_continue_watching(&pool).await {
                                home_data.continue_watching = cw;
                            }
                            if let Ok(wl) = services::sync::load_watchlist(&pool).await {
                                home_data.watchlist = wl;
                            }
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                            prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                        }
                        Err(err) => {
                            state.login_status = Some(format!("Import error: {}", err));
                        }
                    }
                }
                AppMessage::PublicSyncCompleted(res) => {
                    match res {
                        Ok((viewer, stats)) => {
                            state.auth.username = Some(viewer.name.clone());
                            state.auth.user_id = Some(viewer.id);
                            state.auth.avatar_url = viewer.avatar_url.clone();
                            state.auth.sync_mode = Some("Public Over-The-Air Sync".to_string());
                            state.auth.last_sync_time = Some(unix_now());
                            let _ = state.auth.save();
                            state.login_status = None;
                            state.show_toast(format!("❖ Synced @{}: {} anime", viewer.name, stats.total_anime), unix_now());
                            state.import_stats = Some(stats);

                            if let Ok(cw) = services::sync::load_continue_watching(&pool).await {
                                home_data.continue_watching = cw;
                            }
                            if let Ok(wl) = services::sync::load_watchlist(&pool).await {
                                home_data.watchlist = wl;
                            }
                            refresh_home_cover(&mut state, &home_data, &pool, &tx);
                            prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                        }
                        Err(err) => {
                            state.login_status = Some(format!("Sync error: {}", err));
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
                AppMessage::EpisodeMetadataReady(anime_id, meta) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.loading_episode_metadata = false;
                        for (ep, (title, thumb_url, synopsis)) in meta {
                            state.episode_titles.insert(ep, title);
                            if let Some(syn) = synopsis {
                                state.episode_synopses.insert(ep, syn);
                            }
                            if let Some(url) = thumb_url {
                                state.episode_thumbnail_urls.insert(ep, url);
                            }
                        }
                        preload_nearby_episode_thumbnails(&mut state, &tx);
                    }
                }
                AppMessage::EpisodeThumbnailReady(anime_id, ep, img) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.loading_episode_thumbnails.remove(&ep);
                        let font_size = state.picker.as_ref().map(|p| p.font_size).unwrap_or((12, 24));
                        let card_thumb = prepare_16_9_thumbnail_cover(img, font_size);
                        if let Some(ref mut picker) = state.picker {
                            let proto = picker.new_resize_protocol(card_thumb);
                            state.episode_thumbnails.insert(ep, proto);
                        }
                    }
                }
                AppMessage::AnimeBannerReady(anime_id, img) => {
                    state.loading_banners.remove(&anime_id);
                    let font_size = state.picker.as_ref().map(|p| p.font_size).unwrap_or((12, 24));
                    let card_banner = prepare_card_cover_with_font(img.clone(), font_size);
                    if let Some(ref mut picker) = state.picker {
                        let proto = picker.new_resize_protocol(card_banner.clone());
                        state.anime_banners.insert(anime_id, proto);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        let thumb_banner = prepare_16_9_thumbnail_cover(img, font_size);
                        state.current_card_img = Some(thumb_banner);
                        state.episode_fallback_thumbnails.clear();
                    }
                }
                AppMessage::PlaybackProgressUpdated(anime_id, ep, pos, dur) => {
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.anime_progress.insert(ep, (pos, dur));
                    }
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
                    let cover_ids: Vec<i64> = analytics
                        .top_shows
                        .iter()
                        .take(6)
                        .map(|s| s.id)
                        .filter(|id| !state.detail_cache.contains_key(id) && state.loading_covers.insert(*id))
                        .collect();
                    state.analytics_data = Some(analytics);
                    for id in cover_ids {
                        let pool_c = pool.clone();
                        let tx_c = tx.clone();
                        tokio::spawn(async move {
                            if let Ok(Some(anime)) = db::cache::get_anime(&pool_c, id).await {
                                match download_cover_image(&anime, &pool_c).await {
                                    Some(img) => { let _ = tx_c.send(AppMessage::CoverReady(id, img)).await; }
                                    None => { let _ = tx_c.send(AppMessage::CoverFailed(id)).await; }
                                }
                            } else {
                                let _ = tx_c.send(AppMessage::CoverFailed(id)).await;
                            }
                        });
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
                    if matches!(state.update_modal_phase, Some(state::UpdateModalPhase::Downloading { .. })) {
                        state.update_modal_phase = Some(state::UpdateModalPhase::Downloading {
                            progress_pct: pct,
                            speed: spd,
                        });
                    }
                }
                AppMessage::UpdateSuccess(version) => {
                    state.is_updating = false;
                    state.update_progress = None;
                    state.available_update = None;
                    state.update_modal_phase = Some(state::UpdateModalPhase::RestartRequired {
                        new_version: version.clone(),
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
                    if let Some(ref anime) = state.selected_anime.clone() {
                        // Close the current episode immediately, before resolving the next stream
                        services::remote::quit_active_player().await;
                        state.stop_player();
                        state.selected_episode = Some(ep);
                        let title = anime.display_title().to_string();
                        let season_year = anime.season_year;

                        // Reuse exact playback settings from the previous episode (player, dub, quality, provider, sub_lang)
                        let prev = state.last_play_options.clone();
                        let query = prev
                            .as_ref()
                            .map(|o| o.title.clone())
                            .unwrap_or_else(|| anime.playback_query());
                        let is_dub = prev
                            .as_ref()
                            .map(|o| o.dub)
                            .unwrap_or(cfg.audio_mode == config::AudioMode::Dub);
                        let player = prev
                            .as_ref()
                            .map(|o| o.player.clone())
                            .unwrap_or_else(|| state.selected_player().as_str().to_string());
                        let quality = prev
                            .as_ref()
                            .map(|o| o.quality.clone())
                            .unwrap_or_else(|| cfg.quality.as_str().to_string());
                        let pref_sub_lang = prev
                            .as_ref()
                            .and_then(|o| o.sub_lang.clone())
                            .unwrap_or_else(|| cfg.default_sub_lang.clone());
                        let was_direct_stream = prev
                            .as_ref()
                            .map(|o| o.stream_url.is_some())
                            .unwrap_or(true);

                        state.show_toast(format!("❖ Remote: Loading Episode {}...", ep), unix_now());

                        // Resolve fresh stream URL and subtitle track for the requested episode
                        let stream_info = api::subtitles::fetch_stream_info(anime.id, &query, ep, is_dub).await;
                        state.playback_qualities = stream_info.qualities.clone();
                        state.playback_sub_languages = stream_info.subtitles.clone();
                        state.playback_providers = stream_info.providers.clone();
                        state.dub_available = Some(stream_info.dub_available);
                        state.pending_master_url = stream_info.master_url.clone();

                        let mut stream_url = None;
                        if was_direct_stream {
                            stream_url = stream_info
                                .qualities
                                .iter()
                                .find(|q| q.label.eq_ignore_ascii_case(&quality))
                                .or_else(|| stream_info.qualities.first())
                                .map(|q| q.url.clone())
                                .filter(|u| !u.is_empty())
                                .or_else(|| stream_info.master_url.clone());
                        }

                        let chosen_sub = stream_info
                            .subtitles
                            .iter()
                            .find(|s| {
                                s.name.to_lowercase().contains(&pref_sub_lang.to_lowercase())
                                    || s.code.to_lowercase().starts_with(&pref_sub_lang.to_lowercase())
                                    || s.lang_keyword().eq_ignore_ascii_case(&pref_sub_lang)
                            })
                            .or_else(|| stream_info.subtitles.first());

                        let sub_lang = chosen_sub
                            .map(|s| s.lang_keyword().to_string())
                            .or(Some(pref_sub_lang));
                        let sub_url = chosen_sub
                            .map(|s| s.url.clone())
                            .filter(|u| !u.is_empty());

                        let glsl_shaders = services::shaders::build_shader_arg_for_year(
                            state.anime4k_mode,
                            season_year,
                        );

                        let opts = api::player::PlayOptions {
                            title: query,
                            episode: ep,
                            quality,
                            dub: is_dub,
                            player,
                            sub_lang,
                            sub_url,
                            stream_url,
                            ipc_socket: None,
                            start_pos: None,
                            glsl_shaders,
                            sub_style: Some(cfg.subtitle_style.clone()),
                        };
                        start_playback(&mut state, opts, title, ep, &tx, &pool, &cfg).await;
                    }
                }
                AppMessage::RemoteAnime4K(mode) => {
                    state.anime4k_mode = mode;
                    cfg.anime4k_mode = mode;
                    let _ = cfg.save();
                    state.show_toast(format!("❖ Anime4K: {}", mode.as_str()), unix_now());
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
            let _ = terminal.clear();
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
                    Screen::Detail   => ui::detail::render(frame, &mut state),
                    Screen::PlaybackQuery => {
                        ui::detail::render(frame, &mut state);
                        ui::play_query::render_overlay(frame, &state);
                    }
                    Screen::PlaybackOptions => {
                        ui::detail::render(frame, &mut state);
                        ui::play_options::render_overlay(frame, &state);
                    }
                    Screen::Playback => ui::playback::render(frame, &state),
                    Screen::Search   => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::search::render_overlay(frame, &mut state);
                    }
                    Screen::Help => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::help::render_overlay(frame);
                    }
                    Screen::Settings => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::settings::render_overlay(frame, &state, &cfg);
                    }
                    Screen::Setup => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::setup::render_overlay(frame, &state, &cfg);
                    }
                    Screen::AniListLogin => {
                        render_base_screen(frame, &mut state, &home_data, &base_screen);
                        ui::login::render_overlay(frame, &state);
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
                }
                // Toast notification renders on top of everything
                if let Some(ref msg) = toast_msg {
                    ui::help::render_toast(frame, msg);
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

    if state.is_loading && state.screen == Screen::Home {
        match key.code {
            KeyCode::Esc => {
                state.skip_preload();
                refresh_home_cover(state, home_data, pool, tx);
            }
            KeyCode::Char('q') => {
                state.should_quit = true;
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
        Screen::Help     => { state.go_back(); }
        Screen::Settings => handle_settings(key, state, pool, cfg, tx).await,
        Screen::Setup    => handle_setup(key, state).await,
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
                KeyCode::Enter => {
                    if !info.is_homebrew && !state.is_updating {
                        state.is_updating = true;
                        state.update_modal_phase = Some(state::UpdateModalPhase::Downloading {
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
                KeyCode::Esc | KeyCode::Char('q') => {
                    state.close_update_modal();
                }
                _ => {}
            }
        }
        state::UpdateModalPhase::Downloading { .. } => {
            if key.code == KeyCode::Esc {
                // Allow closing modal to background while download continues
                state.close_update_modal();
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

fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup_layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ratatui::layout::Constraint::Percentage(percent_y),
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ratatui::layout::Constraint::Percentage(percent_x),
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
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

    // Right-click always acts as Back / Esc
    if matches!(mouse.kind, event::MouseEventKind::Down(event::MouseButton::Right)) {
        state.go_back();
        return true;
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
                            Calendar => KeyCode::Char('c'),
                            Wizard => KeyCode::Char('w'),
                            Analytics => KeyCode::Char('a'),
                            Remote => KeyCode::Char('m'),
                            Doctor => KeyCode::Char('!'),
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
                            let clicked_tier_idx = heights.iter().position(|&h| {
                                let in_band = rel_y >= acc_y && rel_y < acc_y + h;
                                acc_y += h;
                                in_band
                            });

                            if let Some(clicked_tier_idx) = clicked_tier_idx {
                                let actual_tier_idx = start_idx + clicked_tier_idx;
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
                                                let right_x = left_w + 1;
                                                let wl_usable_w = term_w.saturating_sub(right_x + 4);
                                                let (vis_n, wl_card_w) = ui::home::compute_row_card_metrics(wl_usable_w);
                                                let wl_pitch = wl_card_w + ui::home::CARD_GAP;
                                                let inner_x = mx.saturating_sub(right_x + 2);
                                                let col = (inner_x / wl_pitch) as usize;
                                                let offset = state.row_offset("watchlist");
                                                let start = ui::home::row_view_start(offset, home_data.watchlist.len(), vis_n);
                                                let target_idx = start + col;
                                                if let Some(anime) = home_data.watchlist.get(target_idx).cloned() {
                                                    state.active_row = state::CategoryRow::Watchlist;
                                                    state.row_offsets.insert("watchlist".to_string(), target_idx);
                                                    open_detail_from_anime(state, anime, pool, tx).await;
                                                    return true;
                                                }
                                            }
                                        }
                                        ui::home::ShelfTier::Single(cat_row) => {
                                            let (key, items): (&str, &[db::cache::Anime]) = match cat_row {
                                                state::CategoryRow::ContinueWatching => ("continue_watching", &home_data.continue_watching),
                                                state::CategoryRow::Watchlist        => ("watchlist", &home_data.watchlist),
                                                state::CategoryRow::Trending         => ("trending", &home_data.trending),
                                                state::CategoryRow::Popular          => ("popular", &home_data.popular),
                                                state::CategoryRow::TopRated         => ("top_rated", &home_data.top_rated),
                                                state::CategoryRow::Seasonal         => ("seasonal", &home_data.seasonal),
                                            };
                                            let (vis_n, row_card_w) = ui::home::compute_row_card_metrics(term_w);
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
                    false
                }
                Screen::Detail => {
                    // Hint bar / back
                    if my == 0 {
                        if mx < 8 {
                            state.go_back();
                            return true;
                        } else if mx < 25 {
                            begin_playback_flow(state, pool, cfg, false, tx).await;
                            return true;
                        } else if mx < 45 {
                            state.episode_view_mode = match state.episode_view_mode {
                                state::EpisodeViewMode::List => state::EpisodeViewMode::Split,
                                state::EpisodeViewMode::Split => state::EpisodeViewMode::List,
                            };
                            return true;
                        } else if mx < 60 {
                            let q = state.selected_anime.as_ref().map(|a| a.playback_query()).unwrap_or_default();
                            let ep = state.selected_episode.unwrap_or(1);
                            let dub = cfg.audio_mode == config::AudioMode::Dub;
                            let player = cfg.player;
                            state.open_playback_options(q, 0, dub, player);
                            if let Some(ref anime) = state.selected_anime {
                                trigger_stream_detection(anime, ep, dub, tx);
                            }
                            return true;
                        }
                    }
                    // Episodes
                    match state.episode_view_mode {
                        state::EpisodeViewMode::List => {
                            if my >= 16 {
                                let inner_h = term_h.saturating_sub(17);
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
                                if my >= 17 {
                                    let rel_line = (my - 17) as usize;
                                    let visible_rows = term_h.saturating_sub(19) as usize;
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
                            } else if my >= 16 {
                                // Right Inspector Theater
                                begin_playback_flow(state, pool, cfg, false, tx).await;
                                return true;
                            }
                        }
                    }
                    false
                }
                Screen::PlaybackOptions => {
                    let modal_area = centered_rect(84, 70, term_area);
                    if mx < modal_area.x || mx >= modal_area.right() || my < modal_area.y || my >= modal_area.bottom() {
                        state.go_back();
                        return true;
                    }
                    // Header options: row 2 of modal
                    if my == modal_area.y + 2 {
                        let rel_x = mx.saturating_sub(modal_area.x);
                            if rel_x < 28 {
                                state.cycle_playback_player();
                                state.show_toast(format!("Player: {}", state.selected_player().as_str()), unix_now());
                                return true;
                            } else if rel_x < 55 {
                                if !state.pending_dub && state.dub_available == Some(false) {
                                    state.show_toast("English Dub is not available for this anime (Sub only)", unix_now());
                                } else {
                                    state.pending_dub = !state.pending_dub;
                                    state.is_loading_subs = true;
                                    state.playback_qualities.clear();
                                    state.playback_sub_languages.clear();
                                    state.playback_quality_cursor = 0;
                                    state.playback_sub_cursor = 0;
                                    if let Some(ref anime) = state.selected_anime {
                                        let ep = state.selected_episode.unwrap_or(1);
                                        trigger_stream_detection(anime, ep, state.pending_dub, tx);
                                    }
                                }
                                return true;
                            } else {
                                state.toggle_anime4k();
                                state.show_toast(format!("Anime4K: {}", state.anime4k_mode.as_str()), unix_now());
                                return true;
                            }
                        }
                    // Footer
                    if my >= modal_area.bottom().saturating_sub(3) {
                        let rel_x = mx.saturating_sub(modal_area.x);
                        if rel_x < 16 {
                            let enter_key = event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
                            handle_playback_options(enter_key, state, pool, cfg, tx).await;
                            return true;
                        } else if rel_x >= modal_area.width.saturating_sub(15) {
                            state.go_back();
                            return true;
                        }
                    }
                    // Columns
                    if my >= modal_area.y + 4 && my < modal_area.bottom().saturating_sub(3) {
                        let rel_x = mx.saturating_sub(modal_area.x);
                        let w = modal_area.width;
                        let col0_w = (w * 20) / 100;
                        let col1_w = (w * 24) / 100;
                        let col2_w = (w * 22) / 100;
                        let rel_item_row = (my - (modal_area.y + 5)) as usize;

                        if rel_x < col0_w {
                            state.playback_focus_column = 0;
                            if rel_item_row < state.playback_players.len() {
                                state.playback_player_cursor = rel_item_row;
                                return true;
                            }
                        } else if rel_x < col0_w + col1_w {
                            state.playback_focus_column = 1;
                            if rel_item_row < state.playback_providers.len() {
                                state.playback_provider_cursor = rel_item_row;
                                state.sync_active_provider();
                                return true;
                            }
                        } else if rel_x < col0_w + col1_w + col2_w {
                            state.playback_focus_column = 2;
                            if rel_item_row < state.playback_qualities.len() {
                                state.playback_quality_cursor = rel_item_row;
                                return true;
                            }
                        } else {
                            state.playback_focus_column = 3;
                            if rel_item_row < state.playback_sub_languages.len() {
                                state.playback_sub_cursor = rel_item_row;
                                return true;
                            }
                        }
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

async fn handle_home(
    key:       event::KeyEvent,
    state:     &mut AppState,
    home_data: &mut ui::home::HomeData,
    pool:      &sqlx::SqlitePool,
    cfg:       &config::Config,
    tx:        &tokio::sync::mpsc::Sender<AppMessage>,
) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => state.should_quit = true,

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
        KeyCode::Char('/') => state.open_search(),
        KeyCode::Char('c') | KeyCode::Char('C') => {
            open_and_fetch_calendar(state, pool, tx).await;
        }
        KeyCode::Char('?') => state.open_help(),
        KeyCode::Char('s') => state.open_settings(),
        KeyCode::Char('a') | KeyCode::Char('A') => {
            trigger_analytics_fetch(state, pool.clone(), tx.clone());
        }
        KeyCode::Char('L') => state.open_login(),
        KeyCode::Char('t') | KeyCode::Char('T') => {
            state.theme_index = (state.theme_index + 1) % ui::theme::THEMES.len();
            let theme_name = state.theme().name;
            state.show_toast(format!("Theme: {}", theme_name), unix_now());
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
                    let auth_token = state.auth.token.clone();
                    let user_id = state.auth.user_id;
                    tokio::spawn(async move {
                        let _ = db::user::remove_continue_watching(&pool2, anime_id).await;
                        if let Some(token) = auth_token {
                            let client = api::anilist::AniListClient::with_token(Some(token));
                            let _ = client.delete_media_list_entry(user_id, anime_id).await;
                        }
                        if let Ok(cw) = services::sync::load_continue_watching(&pool2).await {
                            let _ = tx2.send(AppMessage::ContinueWatchingUpdated(cw)).await;
                        }
                    });
                }
            } else if state.active_row == state::CategoryRow::Watchlist {
                if let Some(anime) = active_anime(state, home_data) {
                    let now = unix_now();
                    let anime_id = anime.id;
                    let title = anime.display_title().to_string();

                    home_data.watchlist.retain(|a| a.id != anime_id);
                    let max_idx = home_data.watchlist.len().saturating_sub(1);
                    let offset = state.row_offset("watchlist").min(max_idx);
                    state.row_offsets.insert("watchlist".to_string(), offset);

                    if home_data.watchlist.is_empty() {
                        state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
                    }
                    refresh_home_cover(state, home_data, pool, tx);
                    state.show_toast(format!("󰅖 Removed \"{}\" from Watchlist", title), now);

                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    let auth_token = state.auth.token.clone();
                    let user_id = state.auth.user_id;
                    tokio::spawn(async move {
                        let _ = db::user::remove_from_watchlist(&pool2, anime_id).await;
                        if let Some(token) = auth_token {
                            let client = api::anilist::AniListClient::with_token(Some(token));
                            let _ = client.delete_media_list_entry(user_id, anime_id).await;
                        }
                        if let Ok(wl) = services::sync::load_watchlist(&pool2).await {
                            let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                        }
                    });
                }
            } else if state.now_playing.is_some() {
                state.stop_player();
                state.show_toast("Playback stopped", unix_now());
            }
        }

        KeyCode::Char('X') => {
            if state.now_playing.is_some() {
                state.stop_player();
                state.show_toast("Playback stopped", unix_now());
            }
        }
        KeyCode::Char('!') => {
            refresh_dependency_status(state);
            state.open_setup();
        }

        // Toggle watchlist for the highlighted card directly from Home
        KeyCode::Char('+') => {
            if let Some(anime) = active_anime(state, home_data) {
                let now   = unix_now();
                let anime_id = anime.id;
                let in_wl = db::user::is_in_watchlist(pool, anime_id).await.unwrap_or(false);
                let auth_token = state.auth.token.clone();
                let user_id = state.auth.user_id;

                if in_wl {
                    if db::user::remove_from_watchlist(pool, anime_id).await.is_ok() {
                        state.show_toast("Removed from watchlist", now);
                        let pool2 = pool.clone();
                        let tx2   = tx.clone();
                        tokio::spawn(async move {
                            if let Some(token) = auth_token {
                                let client = api::anilist::AniListClient::with_token(Some(token));
                                let _ = client.delete_media_list_entry(user_id, anime_id).await;
                            }
                            let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                            let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                        });
                    }
                } else if db::user::add_to_watchlist(pool, anime_id, now).await.is_ok() {
                    state.show_toast("Added to watchlist", now);
                    let pool2 = pool.clone();
                    let tx2   = tx.clone();
                    tokio::spawn(async move {
                        if let Some(token) = auth_token {
                            let client = api::anilist::AniListClient::with_token(Some(token));
                            let _ = client.save_planning_entry(anime_id).await;
                        }
                        let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                        let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                    });
                }
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
                let client = api::anilist::AniListClient::with_token(auth2.token.clone());
                let now    = unix_now();
                let result = services::sync::sync_all(
                    &pool2, &client, Some(&auth2),
                    cfg2.cache.trending_ttl, cfg2.cache.stable_ttl, now,
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
    trigger_episode_metadata_fetch(anime_id, title_romaji, title_english, tx.clone());
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
    let Some(anime) = state.selected_anime.clone() else {
        return;
    };

    if force_resume {
        state.selected_episode = Some(state.next_unwatched_episode());
    }

    let ep = state.selected_episode.unwrap_or(1);
    let max_aired = anime.aired_episodes();
    if ep > max_aired {
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
    let use_dub = anime.has_dub() && default_audio == config::AudioMode::Dub;
    let ep = state.selected_episode.unwrap_or(1);

    let default_player = state.selected_player();
    if let Some(saved_query) = db::user::get_playback_query(pool, anime.id).await.unwrap_or(None) {
        state.open_playback_options(
            saved_query,
            quality_index(&cfg.quality),
            use_dub,
            default_player,
        );
        trigger_stream_detection(&anime, ep, use_dub, tx);
    } else if anime.playback_queries().len() > 1 {
        state.open_playback_query_picker(&anime);
    } else {
        state.open_playback_options(
            anime.playback_query(),
            quality_index(&cfg.quality),
            use_dub,
            default_player,
        );
        trigger_stream_detection(&anime, ep, use_dub, tx);
    }
}

fn trigger_stream_detection(
    anime: &db::cache::Anime,
    episode: u32,
    is_dub: bool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let anime_id = anime.id;
    let title = anime.playback_query();
    let tx = tx.clone();
    tokio::spawn(async move {
        let info = api::subtitles::fetch_stream_info(anime_id, &title, episode, is_dub).await;
        let _ = tx.send(AppMessage::StreamInfoDetected(anime_id, episode, is_dub, info)).await;
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

/// Bounded concurrency semaphore so startup preloading saturates keep-alive connections without socket/API spikes.
fn cover_download_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(12))
}

/// Build a `StatefulProtocol` safely across UI and background worker threads without Kitty ID collisions.
fn build_stateful_protocol(
    mut picker: ratatui_image::picker::Picker,
    img: image::DynamicImage,
) -> Box<dyn ratatui_image::protocol::StatefulProtocol> {
    static NEXT_KITTY_IMAGE_ID: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(100_000);
    if picker.protocol_type == ratatui_image::picker::ProtocolType::Kitty {
        let id = NEXT_KITTY_IMAGE_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .max(1);
        let source = ratatui_image::protocol::ImageSource::new(img, picker.font_size);
        Box::new(ratatui_image::protocol::kitty::StatefulKitty::new(
            source,
            id,
            picker.is_tmux,
        ))
    } else {
        picker.new_resize_protocol(img)
    }
}

/// Standard inner image cell dimensions (`20 x 6`) for pre-encoding card & hero covers off-thread.
fn home_card_preencode_area() -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(
        0,
        0,
        ui::home::CARD_WIDTH.saturating_sub(2),
        ui::home::CARD_HEIGHT.saturating_sub(4),
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
            .unwrap_or((false, false, false, false));
        let _ = tx_dep
            .send(AppMessage::DependenciesReady {
                ani_cli: deps.0,
                mpv: deps.1,
                iina: deps.2,
                vlc: deps.3,
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
    let (ani_cli, mpv, iina, vlc) = api::player::detect_dependencies();
    state.set_dependencies(ani_cli, mpv, iina, vlc);
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
    cfg:   &config::Config,
    tx:    &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.is_download_modal_open {
        handle_download_modal(key, state, pool, cfg, tx).await;
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
            if let (Some(ref anime), Some(ep)) = (&state.selected_anime, state.selected_episode) {
                let anime_id = anime.id;
                let is_watched = state.watched_episodes.contains(&ep);
                let pool2 = pool.clone();
                let now = unix_now();
                if is_watched {
                    state.watched_episodes.remove(&ep);
                    state.show_toast(format!("Episode {} marked unwatched", ep), now);
                    tokio::spawn(async move {
                        let _ = sqlx::query("DELETE FROM history WHERE anime_id = ? AND episode = ?")
                            .bind(anime_id)
                            .bind(ep as i64)
                            .execute(&pool2)
                            .await;
                    });
                } else {
                    state.watched_episodes.insert(ep);
                    state.show_toast(format!("󰄬 Episode {} marked watched", ep), now);
                    tokio::spawn(async move {
                        let _ = db::user::record_watched(&pool2, anime_id, ep as i64, now).await;
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
                let auth_token = state.auth.token.clone();
                let user_id = state.auth.user_id;
                tokio::spawn(async move {
                    let _ = db::user::remove_continue_watching(&pool2, anime_id).await;
                    let _ = db::user::remove_from_watchlist(&pool2, anime_id).await;
                    if let Some(token) = auth_token {
                        let client = api::anilist::AniListClient::with_token(Some(token));
                        let _ = client.delete_media_list_entry(user_id, anime_id).await;
                    }
                    if let Ok(cw) = services::sync::load_continue_watching(&pool2).await {
                        let _ = tx2.send(AppMessage::ContinueWatchingUpdated(cw)).await;
                    }
                    if let Ok(wl) = services::sync::load_watchlist(&pool2).await {
                        let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                    }
                });
            }
        }

        // Watchlist toggle — updates home row immediately via WatchlistUpdated
        KeyCode::Char('+') => {
            if let Some(anime) = state.selected_anime.clone() {
                let anime_id = anime.id;
                let now = unix_now();
                let auth_token = state.auth.token.clone();
                let user_id = state.auth.user_id;
                if state.in_watchlist {
                    if db::user::remove_from_watchlist(pool, anime_id).await.is_ok() {
                        state.in_watchlist = false;
                        state.show_toast("Removed from watchlist", now);
                        let pool2 = pool.clone();
                        let tx2   = tx.clone();
                        tokio::spawn(async move {
                            if let Some(token) = auth_token {
                                let client = api::anilist::AniListClient::with_token(Some(token));
                                let _ = client.delete_media_list_entry(user_id, anime_id).await;
                            }
                            let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                            let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                        });
                    }
                } else if db::user::add_to_watchlist(pool, anime_id, now).await.is_ok() {
                    state.in_watchlist = true;
                    state.show_toast("Added to watchlist", now);
                    let pool2 = pool.clone();
                    let tx2   = tx.clone();
                    tokio::spawn(async move {
                        if let Some(token) = auth_token {
                            let client = api::anilist::AniListClient::with_token(Some(token));
                            let _ = client.save_planning_entry(anime_id).await;
                        }
                        let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                        let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                    });
                }
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
    pool: sqlx::SqlitePool,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let anime_title = anime.display_title().to_string();
        let query = anime.playback_query();
        let mut tasks = Vec::new();

        for ep in episodes {
            let stream_info = api::subtitles::fetch_stream_info(anime.id, &query, ep, false).await;
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
                let is_dub = anime.has_dub() && preferred_audio == config::AudioMode::Dub;
                let default_player = state.selected_player();
                state.open_playback_options(
                    query,
                    quality_index(&cfg.quality),
                    is_dub,
                    default_player,
                );
                let ep = state.selected_episode.unwrap_or(1);
                trigger_stream_detection(&anime, ep, is_dub, tx);
            }
        }
        _ => {}
    }
}

async fn handle_playback_options(
    key:   event::KeyEvent,
    state: &mut AppState,
    pool:  &sqlx::SqlitePool,
    cfg:   &config::Config,
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
            state.playback_qualities.clear();
            state.playback_sub_languages.clear();
            state.playback_quality_cursor = 0;
            state.playback_sub_cursor = 0;
            if let Some(ref anime) = state.selected_anime {
                let ep = state.selected_episode.unwrap_or(1);
                trigger_stream_detection(anime, ep, state.pending_dub, tx);
            }
        }
        KeyCode::Char('u') | KeyCode::Char('U') => {
            state.toggle_anime4k();
            state.show_toast(format!("Anime4K: {}", state.anime4k_mode.as_str()), unix_now());
        }
        KeyCode::Enter => {
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
                let sub_lang = chosen_sub.map(|s| s.lang_keyword().to_string());
                let sub_url = chosen_sub.map(|s| s.url.clone()).filter(|u| !u.is_empty());

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
                if chosen_player == config::Player::Iina && !cfg!(target_os = "macos") {
                    state.show_toast("IINA is only supported on macOS. Please select mpv or vlc.", unix_now());
                    return;
                }
                if chosen_player == config::Player::Iina && !state.has_iina {
                    state.show_toast("IINA is not installed. Run 'brew install --cask iina' or select mpv.", unix_now());
                    return;
                }
                if chosen_player == config::Player::Mpv && !state.has_mpv {
                    state.show_toast("mpv is not installed. Press ! to check setup.", unix_now());
                    return;
                }
                if chosen_player == config::Player::Vlc && !state.has_vlc {
                    state.show_toast("VLC is not installed. Press ! to check setup.", unix_now());
                    return;
                }

                let glsl_shaders = services::shaders::build_shader_arg_for_year(
                    state.anime4k_mode,
                    anime.season_year,
                );
                let opts = api::player::PlayOptions {
                    title:   query,
                    episode: ep,
                    quality: quality_label,
                    dub:     audio == config::AudioMode::Dub,
                    player:  chosen_player.as_str().to_string(),
                    sub_lang,
                    sub_url,
                    stream_url,
                    ipc_socket: None,
                    start_pos:  None,
                    glsl_shaders,
                    sub_style:  Some(cfg.subtitle_style.clone()),
                };
                start_playback(state, opts, title, ep, tx, pool, cfg).await;
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
        }
    }
}

async fn handle_setup(
    key: event::KeyEvent,
    state: &mut AppState,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => state.go_back(),
        KeyCode::Char('r') => {
            refresh_dependency_status(state);
            state.show_toast("Dependency check refreshed", unix_now());
        }
        KeyCode::Char('s') => state.open_settings(),
        _ => {}
    }
}

async fn handle_login(
    key:        event::KeyEvent,
    state:      &mut AppState,
    _home_data: &mut ui::home::HomeData,
    pool:       &sqlx::SqlitePool,
    _cfg:       &config::Config,
    tx:         &tokio::sync::mpsc::Sender<AppMessage>,
) {
    // If import stats modal is showing, Enter or Esc dismisses it and returns to Home
    if state.import_stats.is_some() {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            state.import_stats = None;
            state.go_back();
        }
        return;
    }

    match key.code {
        KeyCode::Esc => state.go_back(),

        // Tab switching via 1/2/3 when input is empty
        KeyCode::Char('1') if state.login_token_input.is_empty() => {
            state.account_sync_tab = 0;
            state.login_status = None;
        }
        KeyCode::Char('2') if state.login_token_input.is_empty() => {
            state.account_sync_tab = 1;
            state.login_status = None;
        }
        KeyCode::Char('3') if state.login_token_input.is_empty() => {
            state.account_sync_tab = 2;
            state.login_status = None;
        }

        // Tab 0: Cycle / auto-fill detected files
        KeyCode::Tab if state.account_sync_tab == 0 => {
            if !state.detected_export_files.is_empty() {
                let idx = state.detected_file_cursor % state.detected_export_files.len();
                let path = &state.detected_export_files[idx];
                state.login_token_input = path.display().to_string();
                state.detected_file_cursor = (state.detected_file_cursor + 1) % state.detected_export_files.len();
            }
        }

        // Disconnect account if authenticated and input is empty
        KeyCode::Char('x') if state.login_token_input.is_empty() && state.auth.is_authenticated() => {
            let _ = state.auth.clear();
            state.login_status = Some("Library unlinked. Local database retained.".to_string());
            state.show_toast("Library unlinked", unix_now());
        }

        // Re-sync shortcut if authenticated via username
        KeyCode::Char('r') if state.login_token_input.is_empty() && state.auth.is_authenticated() => {
            if let Some(ref username) = state.auth.username.clone() {
                state.login_status = Some(format!("Re-syncing library for @{}...", username));
                let pool2 = pool.clone();
                let tx2 = tx.clone();
                let u = username.clone();
                tokio::spawn(async move {
                    let client = api::anilist::AniListClient::new();
                    let now = unix_now();
                    let res = match client.public_user_collection(&u, now).await {
                        Ok((viewer, coll)) => {
                            let entries = services::importer::collection_to_entries(coll);
                            match services::importer::import_entries_to_db(&pool2, &entries, now).await {
                                Ok(stats) => Ok((viewer, stats)),
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx2.send(AppMessage::PublicSyncCompleted(res)).await;
                });
            }
        }

        KeyCode::Backspace => {
            state.login_token_input.pop();
        }

        KeyCode::Enter => {
            match state.account_sync_tab {
                // Tab 0: Local JSON archive ingestion
                0 => {
                    let mut input = state.login_token_input.trim().to_string();
                    if input.is_empty() && !state.detected_export_files.is_empty() {
                        input = state.detected_export_files[0].display().to_string();
                    }
                    if input.is_empty() {
                        state.login_status = Some("Please specify a JSON file path or press [Tab] to select a detected file.".to_string());
                        return;
                    }

                    // Strip accidental wrapping quotes from file drag-and-drop
                    let clean_input = input.trim_matches('\'').trim_matches('"').trim();
                    let resolved_path = if let Some(stripped) = clean_input.strip_prefix("~/") {
                        if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
                            home.join(stripped)
                        } else {
                            std::path::PathBuf::from(clean_input)
                        }
                    } else {
                        std::path::PathBuf::from(clean_input)
                    };

                    if !resolved_path.exists() {
                        state.login_status = Some(format!("Archive not found: {}", resolved_path.display()));
                        return;
                    }

                    state.login_status = Some("Parsing and hydrating database from archive...".to_string());
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        let res = match std::fs::read_to_string(&resolved_path) {
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

                // Tab 1: Public Username live sync
                1 => {
                    let username = state.login_token_input.trim().to_string();
                    if username.is_empty() {
                        state.login_status = Some("Please enter an AniList username.".to_string());
                        return;
                    }

                    state.login_status = Some(format!("Connecting to AniList public API for @{}...", username));
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        let client = api::anilist::AniListClient::new();
                        let now = unix_now();
                        let res = match client.public_user_collection(&username, now).await {
                            Ok((viewer, coll)) => {
                                let entries = services::importer::collection_to_entries(coll);
                                match services::importer::import_entries_to_db(&pool2, &entries, now).await {
                                    Ok(stats) => Ok((viewer, stats)),
                                    Err(e) => Err(e.to_string()),
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        };
                        let _ = tx2.send(AppMessage::PublicSyncCompleted(res)).await;
                    });
                }

                // Tab 2: Export backup
                _ => {
                    state.login_status = Some("Generating Solus JSON backup archive...".to_string());
                    let pool2 = pool.clone();
                    let tx2 = tx.clone();
                    tokio::spawn(async move {
                        let dest = match config::Config::config_dir() {
                            Ok(d) => d.join("solus_backup.json"),
                            Err(_) => std::path::PathBuf::from("solus_backup.json"),
                        };
                        let res = services::importer::export_library_to_json(&pool2, &dest)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx2.send(AppMessage::BackupExportCompleted(res)).await;
                    });
                }
            }
        }

        KeyCode::Char(c) => {
            state.login_token_input.push(c);
        }

        _ => {}
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
            if let Some(ref data) = state.analytics_data {
                if !data.top_shows.is_empty() {
                    let len = data.top_shows.len();
                    state.analytics_podium_idx = (state.analytics_podium_idx + len - 1) % len;
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(ref data) = state.analytics_data {
                if !data.top_shows.is_empty() {
                    state.analytics_podium_idx = (state.analytics_podium_idx + 1) % data.top_shows.len();
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.analytics_list_scroll = state.analytics_list_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.analytics_list_scroll = state.analytics_list_scroll.saturating_sub(1);
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
                    let title = anime.playback_query();
                    tokio::spawn(async move {
                        let stream_info = api::subtitles::fetch_stream_info(anime_id, &title, ep, false).await;
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
    let auth_token = state.auth.token.clone();
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
        if !state.detail_cache.contains_key(&fid)
            && !state.cover_cache.contains_key(&fid)
            && !state.loading_covers.contains(&fid)
        {
            state.loading_covers.insert(fid);
            trigger_cover_download(focused.anime().clone(), pool.clone(), tx.clone());
        }
    }
    for entry in entries.iter().take(8) {
        let id = entry.anime().id;
        if !state.detail_cache.contains_key(&id)
            && !state.cover_cache.contains_key(&id)
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
    home_data: &ui::home::HomeData,
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
                let anime_id = entry.anime().id;
                let now = unix_now();
                let auth_token = state.auth.token.clone();
                let user_id = state.auth.user_id;
                let removed = if db::user::is_in_watchlist(pool, anime_id).await.unwrap_or(false) {
                    let _ = db::user::remove_from_watchlist(pool, anime_id).await;
                    if !home_data.continue_watching.iter().any(|a| a.id == anime_id) {
                        state.calendar_tracked_ids.remove(&anime_id);
                    }
                    state.show_toast("Removed from watchlist", now);
                    true
                } else {
                    let _ = db::cache::upsert_anime(pool, entry.anime()).await;
                    let _ = db::user::add_to_watchlist(pool, anime_id, now).await;
                    state.calendar_tracked_ids.insert(anime_id);
                    state.show_toast("Added to watchlist", now);
                    false
                };
                let pool2 = pool.clone();
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    if let Some(token) = auth_token {
                        let client = api::anilist::AniListClient::with_token(Some(token));
                        if removed {
                            let _ = client.delete_media_list_entry(user_id, anime_id).await;
                        } else {
                            let _ = client.save_planning_entry(anime_id).await;
                        }
                    }
                    let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                    let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                });
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
    cfg:   &config::Config,
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
        _ => {}
    }
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
                let anime_id = focused.anime.id;
                let now = unix_now();
                let auth_token = state.auth.token.clone();
                let user_id = state.auth.user_id;
                let removed = if db::user::is_in_watchlist(pool, anime_id).await.unwrap_or(false) {
                    let _ = db::user::remove_from_watchlist(pool, anime_id).await;
                    state.show_toast("Removed from watchlist", now);
                    true
                } else {
                    let _ = db::user::add_to_watchlist(pool, anime_id, now).await;
                    state.show_toast("Added to watchlist", now);
                    false
                };
                let pool2 = pool.clone();
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    if let Some(token) = auth_token {
                        let client = api::anilist::AniListClient::with_token(Some(token));
                        if removed {
                            let _ = client.delete_media_list_entry(user_id, anime_id).await;
                        } else {
                            let _ = client.save_planning_entry(anime_id).await;
                        }
                    }
                    let wl = services::sync::load_watchlist(&pool2).await.unwrap_or_default();
                    let _ = tx2.send(AppMessage::WatchlistUpdated(wl)).await;
                });
            }
        }
        _ => {}
    }
}

/// Spawn player wrapper with IPC socket, auto-scrobble on 85% completion, auto-skip intro/outro, and exact timestamp resume.
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
    state.playback_gen = state.playback_gen.wrapping_add(1);
    let my_gen = state.playback_gen;
    state.playback_logs.clear();
    state.now_playing = Some(format!("{} — Episode {}", title, episode));
    state.last_played = state.now_playing.clone();
    state.last_played_anime_id = state.selected_anime.as_ref().map(|anime| anime.id);
    state.screen      = Screen::Detail;
    state.pending_playback_query = None;

    let (has_ani_cli, has_mpv, has_iina, has_vlc) = api::player::detect_dependencies();
    state.set_dependencies(has_ani_cli, has_mpv, has_iina, has_vlc);
    if opts.stream_url.is_none() && !has_ani_cli {
        state.show_toast("ani-cli is required for Pahe/Gogo streams. Open ! for setup help.", unix_now());
        return;
    }
    if !(has_mpv || has_iina || has_vlc) {
        state.show_toast("No supported player found (mpv/vlc). Open ! for setup help.", unix_now());
        return;
    }

    let mut opts = opts;
    state.last_play_options = Some(opts.clone());
    let anime_id = state.selected_anime.as_ref().map(|a| a.id).unwrap_or(0);
    let season_year = state.selected_anime.as_ref().and_then(|a| a.season_year);
    let era = services::shaders::AnimeEra::from_year(season_year);

    // Ensure shaders are Era-Adaptive if Anime4K is enabled
    if state.anime4k_mode != config::Anime4KMode::Off {
        opts.glsl_shaders = services::shaders::build_shader_arg_for_year(state.anime4k_mode, season_year);
    }

    // 1. Instant 0ms launch if episode was downloaded offline
    if let Some(offline_path) = state.offline_episodes.get(&episode) {
        if std::path::Path::new(offline_path).exists() {
            opts.stream_url = Some(offline_path.clone());
            state.show_toast(format!("❖ Instant launch from offline vault: Ep {}", episode), unix_now());
        }
    }

    // 2. Exact timestamp resume from saved progress
    if opts.start_pos.is_none() {
        if let Ok(Some((pos, dur))) = db::user::get_playback_progress(pool, anime_id, episode).await {
            if pos > 10.0 && (dur <= 0.0 || pos / dur < 0.85) {
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
    let r_aid = anime_id;
    let r_season_year = season_year;
    let r_era_badge = era.short_badge().to_string();

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
            st.anime4k = r_anime4k;
            st.audio_mode = r_audio_mode;
        }
        services::remote::broadcast_state(&remote_state_init).await;
    });

    // 4. Subtitle auto-sync
    if let Some(ref sub_u) = opts.sub_url {
        if sub_u.starts_with("http://") || sub_u.starts_with("https://") {
            let code = opts.sub_lang.as_deref().unwrap_or("sub");
            let client = reqwest::Client::new();

            // Locate verified reference subtitle from the primary stream (e.g. native official English .vtt)
            let ref_sub_url = state
                .playback_sub_languages
                .iter()
                .find(|s| !s.stream_label.contains("[Auto-Synced]") && (s.url.starts_with("http://") || s.url.starts_with("https://")))
                .map(|s| s.url.as_str())
                .or_else(|| {
                    state
                        .playback_sub_languages
                        .iter()
                        .find(|s| s.code.starts_with("eng") && (s.url.starts_with("http://") || s.url.starts_with("https://")))
                        .map(|s| s.url.as_str())
                });

            let stream_ref = opts.stream_url.as_deref().or(state.pending_master_url.as_deref());

            let (cached_path, sync_res) = services::subsync::ensure_synchronized_subtitle(
                &client,
                sub_u,
                ref_sub_url,
                stream_ref,
                anime_id,
                episode,
                code,
            )
            .await;

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

    // 5. Query AniSkip for intro/outro skip times
    let mal_id_opt = state.anime_mal_ids.get(&anime_id).copied();
    let skip_intervals = if let Some(mid) = mal_id_opt {
        api::aniskip::fetch_skip_times(mid, episode).await
    } else {
        let client = reqwest::Client::new();
        if let Some(mid) = api::anilist::resolve_mal_id(&client, anime_id).await {
            state.anime_mal_ids.insert(anime_id, mid);
            api::aniskip::fetch_skip_times(mid, episode).await
        } else {
            Vec::new()
        }
    };

    let mut child = match api::player::spawn_async(&opts) {
        Ok(c) => c,
        Err(e) => {
            state.screen = Screen::Detail;
            state.show_toast(format!("Playback failed: {e}"), unix_now());
            return;
        }
    };

    state.show_toast(format!("Launching Episode {} in external player", episode), unix_now());

    // 6. Record in continue watching (without marking as completed history yet)
    if let Some(ref anime) = state.selected_anime {
        let pool2 = pool.clone();
        let aid   = anime.id;
        let now   = unix_now();
        tokio::spawn(async move {
            let _ = db::user::touch_continue_watching(&pool2, aid, episode as i64, now).await;
        });
    }

    // 7. Spawn mpv IPC monitor for live tracking, auto-scrobble, and auto-skip
    let ipc_cfg = services::ipc::IpcMonitorConfig {
        anime_id,
        episode,
        sock_path,
        pool: pool.clone(),
        tx: tx.clone(),
        auth_token: state.auth.token.clone(),
        skip_intervals,
        auto_skip_intro: cfg.auto_skip_intro,
    };
    services::ipc::spawn_mpv_ipc_monitor(ipc_cfg);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    state.player_stop = Some(stop_tx);

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
                let msg = format!("ani-cli exited: {:?}", status);
                let _ = tx4.send(AppMessage::PlaybackLog(msg)).await;
                let _ = tx4.send(AppMessage::PlaybackDone(my_gen)).await;
            }
            _ = stop_rx       => {
                let kill_res = child.kill().await;
                let msg = format!("manual stop — kill result: {:?}", kill_res);
                let _ = tx4.send(AppMessage::PlaybackLog(msg)).await;
                let _ = tx4.send(AppMessage::PlaybackDone(my_gen)).await;
            }
        }
    });
}

// ─── Cover image download ─────────────────────────────────────────────────────

/// Dynamically scales and center/upper-biased crops an image so its physical pixel aspect ratio
/// exactly matches the character cell area (target_cols x target_rows) for the detected font size.
/// By ensuring the pixel dimensions are an exact integer multiple of (target_cols * font_w, target_rows * font_h),
/// ratatui-image's internal ratio calculation (wratio == hratio) matches identically down to the lowest bit,
/// guaranteeing 100% frame fill with 0% deadspace across all platforms, fonts, and terminal emulators.
pub fn scale_to_frame_cover(
    img: image::DynamicImage,
    target_cols: u16,
    target_rows: u16,
    font_size: (u16, u16),
    upper_bias: f32,
) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || target_cols == 0 || target_rows == 0 {
        return img;
    }

    let font_w = (font_size.0 as u32).max(1);
    let font_h = (font_size.1 as u32).max(1);

    let base_pw = target_cols as u32 * font_w;
    let base_ph = target_rows as u32 * font_h;

    // Scale to high-res crisp multiplier (target_w >= 1000px) so ratatui-image has full fidelity
    let multiplier = (1280 / base_pw).clamp(2, 16);
    let target_w = base_pw * multiplier;
    let target_h = base_ph * multiplier;
    let target_aspect = target_w as f32 / target_h as f32;

    let current_aspect = w as f32 / h as f32;

    if current_aspect < target_aspect {
        // Source is taller/narrower than frame -> scale width to target_w, crop excess height with upper bias
        let scale = target_w as f32 / w as f32;
        let scaled_h = ((h as f32 * scale).round() as u32).max(target_h);
        let scaled = img.resize(target_w, scaled_h, image::imageops::FilterType::Triangle);

        let excess_h = scaled.height().saturating_sub(target_h);
        let top = ((excess_h as f32) * upper_bias).round() as u32;
        scaled.crop_imm(0, top.min(excess_h), target_w, target_h)
    } else {
        // Source is wider than frame -> scale height to target_h, center-crop excess width
        let scale = target_h as f32 / h as f32;
        let scaled_w = ((w as f32 * scale).round() as u32).max(target_w);
        let scaled = img.resize(scaled_w, target_h, image::imageops::FilterType::Triangle);

        let excess_w = scaled.width().saturating_sub(target_w);
        let left = excess_w / 2;
        scaled.crop_imm(left, 0, target_w, target_h)
    }
}

pub fn prepare_card_cover(img: image::DynamicImage) -> image::DynamicImage {
    scale_to_frame_cover(img, 20, 6, (12, 24), 0.22)
}

pub fn prepare_card_cover_with_font(img: image::DynamicImage, font_size: (u16, u16)) -> image::DynamicImage {
    scale_to_frame_cover(img, 20, 6, font_size, 0.22)
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
    let selected = state.selected_episode.unwrap_or(1);
    let win_start = selected.saturating_sub(3).max(1);
    let win_end = selected + 6;

    let mut targets: Vec<u32> = (win_start..=win_end).collect();
    for ep in 1..=16 {
        if !targets.contains(&ep) {
            targets.push(ep);
        }
    }

    for ep in targets {
        if let Some(url) = state.episode_thumbnail_urls.get(&ep).cloned() {
            if !state.episode_thumbnails.contains_key(&ep)
                && !state.loading_episode_thumbnails.contains(&ep)
            {
                state.loading_episode_thumbnails.insert(ep);
                trigger_episode_thumbnail_download(anime_id, ep, url, tx.clone());
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
    let url = anime.cover_url.as_ref()?;
    let _permit = cover_download_semaphore().acquire().await.ok()?;
    let resp = cover_http_client().get(url).send().await.ok()?;
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
            let card_img = prepare_card_cover_with_font(img, picker.font_size);
            let card_area = home_card_preencode_area();
            let mut card_proto = build_stateful_protocol(picker, card_img.clone());
            let mut banner_proto = build_stateful_protocol(picker, card_img);
            card_proto.resize_encode(&ratatui_image::Resize::Fit(None), picker.background_color, card_area);
            banner_proto.resize_encode(&ratatui_image::Resize::Fit(None), picker.background_color, card_area);
            Some((card_proto, banner_proto))
        })
        .await
        .ok()
        .flatten();

        match encoded {
            Some((card_proto, banner_proto)) => {
                let _ = tx
                    .send(AppMessage::PreloadCoverReady {
                        gen,
                        anime_id,
                        card_proto,
                        banner_proto,
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

fn trigger_episode_metadata_fetch(
    anime_id: i64,
    title_romaji: String,
    title_english: Option<String>,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        // ep_num -> (title, Option<thumbnail_url>, Option<synopsis>)
        let mut map: std::collections::HashMap<u32, (String, Option<String>, Option<String>)> = std::collections::HashMap::new();

        // 1. Primary: Fetch from api.ani.zip by exact AniList ID (provides TVDB 16:9 screencaps, titles, and synopses)
        if let Some(anizip_map) = api::kitsu::fetch_anizip_episodes(&client, anime_id).await {
            for (ep_num, info) in anizip_map {
                map.insert(ep_num, (info.title, info.thumbnail_url, info.synopsis));
            }
        }

        // 2. Fetch from AniList (bannerImage + streamingEpisodes fallback)
        if let Ok(meta) = api::anilist::fetch_anime_streaming_and_banner(&client, anime_id).await {
            if let Some(banner_url) = meta.banner_image {
                trigger_anime_banner_download(anime_id, banner_url, tx.clone());
            }

            for (idx, ep) in meta.streaming_episodes.into_iter().enumerate() {
                let raw_title = ep.title.unwrap_or_else(|| format!("Episode {}", idx + 1));
                let (ep_num, cleaned) = api::anilist::parse_episode_title_and_number(&raw_title, (idx + 1) as u32);
                let entry = map.entry(ep_num).or_insert_with(|| (cleaned.clone(), None, None));
                if entry.0.starts_with("Episode ") && !cleaned.starts_with("Episode ") {
                    entry.0 = cleaned;
                }
                if entry.1.is_none() && ep.thumbnail.is_some() {
                    entry.1 = ep.thumbnail;
                }
            }
        }

        // 3. Fallback: Fetch from Kitsu API if any episode is still missing a thumbnail or synopsis
        let needs_kitsu = map.is_empty() || map.values().any(|(_, thumb, syn)| thumb.is_none() || syn.is_none());
        if needs_kitsu {
            let primary = title_english.as_deref().unwrap_or(&title_romaji);
            let fallback = if title_english.is_some() { Some(title_romaji.as_str()) } else { None };

            if let Some(kitsu_map) = api::kitsu::fetch_kitsu_episodes(&client, primary, fallback).await {
                for (ep_num, k_info) in kitsu_map {
                    let entry = map.entry(ep_num).or_insert_with(|| (k_info.title.clone(), None, None));
                    if entry.1.is_none() && k_info.thumbnail_url.is_some() {
                        entry.1 = k_info.thumbnail_url;
                    }
                    if entry.0.starts_with("Episode ") && !k_info.title.starts_with("Episode ") {
                        entry.0 = k_info.title;
                    }
                    if entry.2.is_none() && k_info.synopsis.is_some() {
                        entry.2 = k_info.synopsis;
                    }
                }
            }
        }

        let _ = tx.send(AppMessage::EpisodeMetadataReady(anime_id, map)).await;
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    let bytes = resp.bytes().await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let _ = std::fs::write(&file_path, &bytes);
    Some(img)
}

fn trigger_episode_thumbnail_download(
    anime_id: i64,
    episode: u32,
    url: String,
    tx: tokio::sync::mpsc::Sender<AppMessage>,
) {
    tokio::spawn(async move {
        if let Some(img) = download_episode_thumbnail(anime_id, episode, &url).await {
            let _ = tx.send(AppMessage::EpisodeThumbnailReady(anime_id, episode, img)).await;
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
    let file_path = cache_dir.join(format!("{}_{}.jpg", anime_id, episode));

    if file_path.exists() {
        if let Ok(bytes) = std::fs::read(&file_path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                return Some(img);
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
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
        Screen::Detail if state.selected_anime.is_some() => ui::detail::render(frame, state),
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
        // Typical AniList portrait cover dimensions (230x345)
        let img = image::DynamicImage::new_rgb8(230, 345);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 1200);
        assert_eq!(prepared.height(), 720);
    }

    #[test]
    fn test_prepare_card_cover_landscape() {
        // Landscape image
        let img = image::DynamicImage::new_rgb8(800, 400);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 1200);
        assert_eq!(prepared.height(), 720);
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
            let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for preload cover")
                .expect("channel closed");
            match msg {
                AppMessage::PreloadCoverReady { gen, anime_id, card_proto, banner_proto } => {
                    state.loading_covers.remove(&anime_id);
                    state.cover_cache.entry(anime_id).or_insert(card_proto);
                    state.banner_cache.entry(anime_id).or_insert(banner_proto);
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
}
