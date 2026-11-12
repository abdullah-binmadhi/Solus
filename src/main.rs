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
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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
    /// Home screen data loaded / refreshed
    HomeData(ui::home::HomeData),
    /// A log line from ani-cli stdout or stderr
    PlaybackLog(String),
    /// ani-cli process exited
    PlaybackDone,
    /// AniList search results
    SearchResults(Vec<db::cache::Anime>),
    /// Cover image downloaded and decoded (anime_id, image)
    CoverReady(i64, image::DynamicImage),
    /// Cover image could not be downloaded or decoded (anime_id)
    CoverFailed(i64),
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
    /// Tailored recommendations generated
    TailoredResultsReady(Vec<services::tailored::TailoredMatch>),
    /// Live search results for Keystone Anchor Anime
    AnchorSearchResultsReady(Vec<db::cache::Anime>),
    /// Episode titles, streaming thumbnails and synopses loaded (anime_id, map of ep -> (title, Option<thumbnail_url>, Option<synopsis>))
    EpisodeMetadataReady(i64, std::collections::HashMap<u32, (String, Option<String>, Option<String>)>),
    /// Episode thumbnail image downloaded and decoded (anime_id, episode_num, image)
    EpisodeThumbnailReady(i64, u32, image::DynamicImage),
    /// Show's 16:9 banner image downloaded and decoded (anime_id, image)
    AnimeBannerReady(i64, image::DynamicImage),
}

// ─── Entry point ──────────────────────────────────────────────────────────────

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

    // ── Message channel (background tasks → UI loop) ──────────────────────────
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AppMessage>(64);

    // ── App state ─────────────────────────────────────────────────────────────
    let mut state     = AppState::new();
    let mut home_data = ui::home::HomeData::empty();
    refresh_dependency_status(&mut state);
    if !state.has_ani_cli || !state.has_any_player() {
        state.open_setup();
    }

    // ── Image picker (after alternate screen, before event loop) ──────────────
    // guess_protocol() probes the terminal — must happen after EnterAlternateScreen.
    let mut picker = ratatui_image::picker::Picker::from_termios()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::new((9, 18)));
    picker.guess_protocol();
    if std::env::var("KITTY_WINDOW_ID").is_ok()
        || std::env::var("TERM").map(|t| t.contains("kitty")).unwrap_or(false)
    {
        picker.protocol_type = ratatui_image::picker::ProtocolType::Kitty;
    }
    state.picker = Some(picker);

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
                Ok(data) => { let _ = tx2.send(AppMessage::HomeData(data)).await; }
                Err(e)   => {
                    eprintln!("Sync error: {e}");
                    let _ = tx2.send(AppMessage::HomeData(ui::home::HomeData::empty())).await;
                }
            }
        });
    }

    let mut needs_redraw = true;

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        // Drain background messages
        while let Ok(msg) = rx.try_recv() {
            needs_redraw = true;
            match msg {
                AppMessage::HomeData(data) => {
                    home_data        = data;
                    state.is_loading = false;
                    refresh_home_cover(&mut state, &home_data, &pool, &tx);
                    prefetch_all_home_covers(&home_data, &mut state, &pool, &tx);
                }
                AppMessage::PlaybackLog(line) => {
                    state.push_log(line);
                }
                AppMessage::PlaybackDone => {
                    state.now_playing = None;
                    state.player_stop = None;
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
                    let card_img = prepare_card_cover(img.clone());
                    let portrait_img = prepare_portrait_cover(img);
                    if let Some(ref mut picker) = state.picker {
                        let proto1 = picker.new_resize_protocol(card_img.clone());
                        let proto2 = picker.new_resize_protocol(card_img.clone());
                        let proto3 = picker.new_resize_protocol(portrait_img);
                        state.cover_cache.insert(anime_id, proto1);
                        state.banner_cache.insert(anime_id, proto2);
                        state.detail_cache.insert(anime_id, proto3);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        if state.current_card_img.is_none() {
                            state.current_card_img = Some(card_img);
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
                            state.login_status = Some(format!("❖ Backup generated: {} titles exported to ~/.config/ani-tui/solus_backup.json", count));
                            state.show_toast(format!("❖ Backup created: {} anime", count), unix_now());
                        }
                        Err(err) => {
                            state.login_status = Some(format!("Export error: {}", err));
                        }
                    }
                }
                AppMessage::TailoredResultsReady(results) => {
                    for m in &results {
                        if !state.cover_cache.contains_key(&m.anime.id) && !state.loading_covers.contains(&m.anime.id) {
                            state.loading_covers.insert(m.anime.id);
                            trigger_cover_download(m.anime.clone(), pool.clone(), tx.clone());
                        }
                    }
                    state.open_tailored_results(results);
                    if let Some(first) = state.tailored_results.first() {
                        state.cover_anime_id = Some(first.anime.id);
                    }
                }
                AppMessage::AnchorSearchResultsReady(results) => {
                    state.is_searching_anchor = false;
                    state.refine_anchor_results = results;
                    state.refine_anchor_cursor = 0;
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
                        let card_thumb = prepare_card_cover(img);
                        if let Some(ref mut picker) = state.picker {
                            let proto = picker.new_resize_protocol(card_thumb);
                            state.episode_thumbnails.insert(ep, proto);
                        }
                    }
                }
                AppMessage::AnimeBannerReady(anime_id, img) => {
                    state.loading_banners.remove(&anime_id);
                    let card_banner = prepare_card_cover(img);
                    if let Some(ref mut picker) = state.picker {
                        let proto = picker.new_resize_protocol(card_banner.clone());
                        state.anime_banners.insert(anime_id, proto);
                    }
                    if state.selected_anime.as_ref().is_some_and(|a| a.id == anime_id) {
                        state.current_card_img = Some(card_banner);
                        state.episode_fallback_thumbnails.clear();
                    }
                }
            }
        }

        // Resolve active toast before entering draw (avoids mutable borrow conflict)
        let now       = unix_now();
        let toast_msg = state.active_toast(now).map(|s| s.to_string());
        if toast_msg.is_some() {
            needs_redraw = true;
        }

        if needs_redraw {
            // Draw current screen
            terminal.draw(|frame| {
                let base_screen = state.current_base_screen();
                match state.screen {
                    Screen::Home => {
                        if state.is_loading {
                            render_loading(frame);
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
                }
                // Toast notification renders on top of everything
                if let Some(ref msg) = toast_msg {
                    ui::help::render_toast(frame, msg);
                }
            })?;
            needs_redraw = false;
        }

        // Poll keyboard with short timeout so background messages stay responsive
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                needs_redraw = true;
                handle_key(key, &mut state, &mut home_data, &pool, &mut cfg, &tx).await;
            }
        }

        if state.should_quit {
            break;
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

    match state.screen {
        Screen::Home     => handle_home(key, state, home_data, pool, cfg, tx).await,
        Screen::Detail   => handle_detail(key, state, pool, cfg, tx).await,
        Screen::PlaybackQuery => handle_playback_query(key, state, pool, cfg, tx).await,
        Screen::PlaybackOptions => handle_playback_options(key, state, pool, cfg, tx).await,
        Screen::Playback => handle_playback(key, state, pool, cfg, tx).await,
        Screen::Search   => handle_search(key, state, pool, tx).await,
        Screen::Help     => { state.go_back(); }
        Screen::Settings => handle_settings(key, state, cfg).await,
        Screen::Setup    => handle_setup(key, state).await,
        Screen::AniListLogin => handle_login(key, state, home_data, pool, cfg, tx).await,
        Screen::TailoredWizard => handle_tailored_wizard(key, state),
        Screen::TailoredSummary => handle_tailored_summary(key, state, pool, tx).await,
        Screen::TailoredResults => handle_tailored_results(key, state, pool, cfg, tx).await,
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
        KeyCode::Char('j') | KeyCode::Down => {
            state.active_row = next_non_empty_row(&state.active_row, home_data, 1);
            refresh_home_cover(state, home_data, pool, tx);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.active_row = next_non_empty_row(&state.active_row, home_data, -1);
            refresh_home_cover(state, home_data, pool, tx);
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
        KeyCode::Char('/') => state.open_search(),
        KeyCode::Char('?') => state.open_help(),
        KeyCode::Char('s') => state.open_settings(),
        KeyCode::Char('a') | KeyCode::Char('L') => state.open_login(),
        KeyCode::Char('t') | KeyCode::Char('T') => {
            state.theme_index = (state.theme_index + 1) % ui::theme::THEMES.len();
            let theme_name = state.theme().name;
            state.show_toast(format!("Theme: {}", theme_name), unix_now());
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
                    Ok(data) => { let _ = tx2.send(AppMessage::HomeData(data)).await; }
                    Err(e)   => {
                        eprintln!("Refresh error: {e}");
                        let _ = tx2.send(AppMessage::HomeData(ui::home::HomeData::empty())).await;
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
        Recommended      => ("recommended".to_string(),       data.recommended.len()),
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
        Recommended      => &data.recommended,
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
        trigger_cover_download(anime.clone(), pool.clone(), tx.clone());
    }
    let anime_id = anime.id;
    let title_romaji = anime.title_romaji.clone();
    let title_english = anime.title_english.clone();
    let should_check_airing = anime.status.as_deref() == Some("RELEASING") || anime.next_airing_episode.is_none();
    state.open_detail(anime);
    if let Some(ref blob) = state.selected_anime.as_ref().and_then(|a| a.cover_blob.as_ref()) {
        if let Ok(dyn_img) = image::load_from_memory(blob) {
            let card_img = prepare_card_cover(dyn_img.clone());
            let portrait_img = prepare_portrait_cover(dyn_img);
            if let Some(ref mut picker) = state.picker {
                let proto_portrait = picker.new_resize_protocol(portrait_img);
                state.detail_cache.insert(anime_id, proto_portrait);
            }
            state.current_card_img = Some(card_img);
        }
    }
    state.set_watched_episodes(watched.into_iter().map(|e| e as u32).collect());
    if should_check_airing {
        trigger_airing_check(anime_id, pool.clone(), tx.clone());
    }
    trigger_episode_metadata_fetch(anime_id, title_romaji, title_english, tx.clone());
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

    if let Some(saved_query) = db::user::get_playback_query(pool, anime.id).await.unwrap_or(None) {
        state.open_playback_options(
            saved_query,
            quality_index(&cfg.quality),
            use_dub,
        );
        trigger_stream_detection(&anime, ep, use_dub, tx);
    } else if anime.playback_queries().len() > 1 {
        state.open_playback_query_picker(&anime);
    } else {
        state.open_playback_options(
            anime.playback_query(),
            quality_index(&cfg.quality),
            use_dub,
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
    trigger_cover_download(anime, pool.clone(), tx.clone());
    trigger_banner_progress(anime_id, pool.clone(), tx.clone());
}

fn prefetch_all_home_covers(
    data: &ui::home::HomeData,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let mut items: Vec<db::cache::Anime> = Vec::new();
    items.extend(data.continue_watching.iter().cloned());
    items.extend(data.trending.iter().cloned());
    items.extend(data.popular.iter().cloned());
    items.extend(data.top_rated.iter().cloned());
    items.extend(data.seasonal.iter().cloned());
    items.extend(data.watchlist.iter().cloned());

    for anime in items {
        if !state.cover_cache.contains_key(&anime.id) && !state.loading_covers.contains(&anime.id) {
            state.loading_covers.insert(anime.id);
            trigger_cover_download(anime, pool.clone(), tx.clone());
        }
    }
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
        Recommended      => data.recommended.len(),
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
        ContinueWatching, Watchlist, Recommended, Trending, Popular, TopRated, Seasonal,
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
                        trigger_cover_download(anime, pool.clone(), tx.clone());
                    }
                }
            }
        }
        KeyCode::Char('/')                => state.open_search(),
        KeyCode::Char('?')               => state.open_help(),
        KeyCode::Char('s')               => state.open_settings(),
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
                state.open_playback_options(
                    query,
                    quality_index(&cfg.quality),
                    is_dub,
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
            state.playback_focus_column = (state.playback_focus_column + 1) % 3;
            state.playback_sub_focused = state.playback_focus_column == 2;
        }
        KeyCode::BackTab => {
            state.playback_focus_column = if state.playback_focus_column == 0 { 2 } else { state.playback_focus_column - 1 };
            state.playback_sub_focused = state.playback_focus_column == 2;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.playback_focus_column = state.playback_focus_column.saturating_sub(1);
            state.playback_sub_focused = state.playback_focus_column == 2;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if state.playback_focus_column < 2 {
                state.playback_focus_column += 1;
                state.playback_sub_focused = state.playback_focus_column == 2;
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
            state.playback_sub_focused = true;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match state.playback_focus_column {
                0 => {
                    if state.playback_provider_cursor + 1 < state.playback_providers.len() {
                        state.playback_provider_cursor += 1;
                        state.sync_active_provider();
                    }
                }
                1 => {
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
                    state.playback_provider_cursor = state.playback_provider_cursor.saturating_sub(1);
                    state.sync_active_provider();
                }
                1 => {
                    state.playback_quality_cursor = state.playback_quality_cursor.saturating_sub(1);
                }
                _ => {
                    state.playback_sub_cursor = state.playback_sub_cursor.saturating_sub(1);
                }
            }
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

                let opts = api::player::PlayOptions {
                    title:   query,
                    episode: ep,
                    quality: quality_label,
                    dub:     audio == config::AudioMode::Dub,
                    player:  cfg.player.as_str().to_string(),
                    sub_lang,
                    sub_url,
                    stream_url,
                };
                start_playback(state, opts, title, ep, tx, pool).await;
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
                if let Some(anime) = state.search_results.get(state.search_cursor) {
                    state.cover_anime_id = Some(anime.id);
                    state.cover_state = None;
                    trigger_cover_download(anime.clone(), pool.clone(), tx.clone());
                }
            }
        }
        KeyCode::Up => {
            if state.search_cursor > 0 {
                state.search_cursor -= 1;
                if let Some(anime) = state.search_results.get(state.search_cursor) {
                    state.cover_anime_id = Some(anime.id);
                    state.cover_state = None;
                    trigger_cover_download(anime.clone(), pool.clone(), tx.clone());
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
    cfg: &mut config::Config,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.go_back(),
        KeyCode::Char('j') | KeyCode::Down => {
            state.settings_cursor = (state.settings_cursor + 1).min(3);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.settings_cursor = state.settings_cursor.saturating_sub(1);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            if apply_settings_delta(cfg, state.settings_cursor, -1).is_ok() {
                let _ = cfg.save();
                state.show_toast("Settings saved", unix_now());
            }
            refresh_dependency_status(state);
        }
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
            if apply_settings_delta(cfg, state.settings_cursor, 1).is_ok() {
                let _ = cfg.save();
                state.show_toast("Settings saved", unix_now());
            }
            refresh_dependency_status(state);
        }
        _ => {}
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
                    let resolved_path = if clean_input.starts_with("~/") {
                        if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
                            home.join(&clean_input[2..])
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

fn apply_settings_delta(
    cfg: &mut config::Config,
    row: usize,
    delta: i32,
) -> anyhow::Result<()> {
    match row {
        0 => {
            let values = [config::Player::Mpv, config::Player::Iina, config::Player::Vlc];
            let current = values.iter().position(|value| value == &cfg.player).unwrap_or(0) as i32;
            let next = (current + delta).rem_euclid(values.len() as i32) as usize;
            cfg.player = values[next].clone();
        }
        1 => {
            let values = [
                config::Quality::Best,
                config::Quality::P1080,
                config::Quality::P720,
                config::Quality::P480,
                config::Quality::P360,
            ];
            let current = values.iter().position(|value| value == &cfg.quality).unwrap_or(1) as i32;
            let next = (current + delta).rem_euclid(values.len() as i32) as usize;
            cfg.quality = values[next].clone();
        }
        2 => {
            cfg.audio_mode = match cfg.audio_mode {
                config::AudioMode::Sub => config::AudioMode::Dub,
                config::AudioMode::Dub => config::AudioMode::Sub,
            };
        }
        3 => {
            let values = ui::settings::SUBTITLE_CHOICES;
            let current = values.iter().position(|&val| val.eq_ignore_ascii_case(&cfg.default_sub_lang)).unwrap_or(0) as i32;
            let next = (current + delta).rem_euclid(values.len() as i32) as usize;
            cfg.default_sub_lang = values[next].to_string();
        }
        _ => {}
    }
    Ok(())
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
        KeyCode::Char('n') => {
            if state.selected_anime.is_some() {
                let ep  = state.selected_episode.unwrap_or(1);
                let max = state.episode_list.last().copied().unwrap_or(1);
                if ep < max {
                    let next = ep + 1;
                    state.selected_episode = Some(next);
                    begin_playback_flow(state, pool, cfg, false, tx).await;
                }
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
        KeyCode::Backspace | KeyCode::Char('b') => {
            if state.tailored_step > 0 {
                state.tailored_step -= 1;
                state.tailored_cursor = state.tailored_answers.choices[state.tailored_step];
            }
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
    let refinement = state.tailored_refinement.clone();
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::new();
        let now = unix_now();
        let matches = services::tailored::generate_tailored_matches_advanced(
            &pool2,
            &client,
            &answers,
            refinement.as_ref(),
            now,
        ).await;
        let _ = tx2.send(AppMessage::TailoredResultsReady(matches)).await;
    });
}

fn finalize_refinement_and_generate(
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    let refinement = services::tailored::TailoredRefinement {
        trope_selections: state.refine_selections[0].clone(),
        pacing_selections: state.refine_selections[1].clone(),
        visual_selections: state.refine_selections[2].clone(),
        aftertaste_selections: state.refine_selections[3].clone(),
        rarity_selections: state.refine_selections[4].clone(),
        anchor_anime_id: state.refine_anchor_selected.as_ref().map(|a| a.id),
        anchor_title: state.refine_anchor_selected.as_ref().map(|a| a.display_title().to_string()),
    };
    state.tailored_refinement = Some(refinement);
    state.close_tailored_refine();
    trigger_generate_tailored(state, pool, tx);
}

fn trigger_anchor_search(query: String, tx: tokio::sync::mpsc::Sender<AppMessage>) {
    tokio::spawn(async move {
        let client = api::anilist::AniListClient::new();
        let now = unix_now();
        let results = client.search(&query, now).await.unwrap_or_default();
        let _ = tx.send(AppMessage::AnchorSearchResultsReady(results)).await;
    });
}

async fn handle_tailored_results(
    key: event::KeyEvent,
    state: &mut AppState,
    pool: &sqlx::SqlitePool,
    cfg: &config::Config,
    tx: &tokio::sync::mpsc::Sender<AppMessage>,
) {
    if state.is_refining_tailored {
        let refine_questions = services::tailored::get_refinement_questions(state.tailored_answers.choices[0]);
        let step = state.tailored_refine_step.min(5);

        if step < 5 {
            let q = &refine_questions[step];

            match key.code {
                KeyCode::Esc => {
                    state.close_tailored_refine();
                }
                KeyCode::Char('b') => {
                    if state.tailored_refine_step > 0 {
                        state.tailored_refine_step -= 1;
                        state.tailored_refine_cursor = 0;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.tailored_refine_cursor = state.tailored_refine_cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.tailored_refine_cursor + 1 < q.options.len() {
                        state.tailored_refine_cursor += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    let cursor = state.tailored_refine_cursor;
                    let set = &mut state.refine_selections[step];
                    if set.contains(&cursor) {
                        set.remove(&cursor);
                    } else {
                        set.insert(cursor);
                    }
                }
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < q.options.len() {
                        let set = &mut state.refine_selections[step];
                        if set.contains(&idx) {
                            set.remove(&idx);
                        } else {
                            set.insert(idx);
                        }
                    }
                }
                KeyCode::Enter => {
                    // Advance to next Crucible step
                    state.tailored_refine_step += 1;
                    state.tailored_refine_cursor = 0;
                }
                _ => {}
            }
        } else {
            // Step 6 (index 5): Keystone Anchor Anime Search
            match key.code {
                KeyCode::Esc => {
                    state.close_tailored_refine();
                }
                KeyCode::Char('b') if state.refine_anchor_query.is_empty() => {
                    state.tailored_refine_step = 4;
                    state.tailored_refine_cursor = 0;
                }
                KeyCode::Backspace => {
                    state.refine_anchor_query.pop();
                    if !state.refine_anchor_query.trim().is_empty() {
                        state.is_searching_anchor = true;
                        trigger_anchor_search(state.refine_anchor_query.clone(), tx.clone());
                    } else {
                        state.refine_anchor_results.clear();
                        state.refine_anchor_selected = None;
                        state.is_searching_anchor = false;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !state.refine_anchor_results.is_empty() {
                        state.refine_anchor_cursor = state.refine_anchor_cursor.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !state.refine_anchor_results.is_empty() && state.refine_anchor_cursor + 1 < state.refine_anchor_results.len() {
                        state.refine_anchor_cursor += 1;
                    }
                }
                KeyCode::Char('s') if state.refine_anchor_query.is_empty() => {
                    finalize_refinement_and_generate(state, pool, tx);
                }
                KeyCode::Enter => {
                    if let Some(chosen) = state.refine_anchor_results.get(state.refine_anchor_cursor).cloned() {
                        state.refine_anchor_selected = Some(chosen);
                    }
                    finalize_refinement_and_generate(state, pool, tx);
                }
                KeyCode::Char(c) => {
                    state.refine_anchor_query.push(c);
                    state.is_searching_anchor = true;
                    trigger_anchor_search(state.refine_anchor_query.clone(), tx.clone());
                }
                _ => {}
            }
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.screen = Screen::Home;
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            state.open_tailored_refine();
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

/// Spawn ani-cli, stream logs to channel, record watch history, auto-return on exit.
async fn start_playback(
    state:   &mut AppState,
    opts:    api::player::PlayOptions,
    title:   String,
    episode: u32,
    tx:      &tokio::sync::mpsc::Sender<AppMessage>,
    pool:    &sqlx::SqlitePool,
) {
    state.stop_player();
    state.playback_logs.clear();
    state.now_playing = Some(format!("{} — Episode {}", title, episode));
    state.last_played = state.now_playing.clone();
    state.last_played_anime_id = state.selected_anime.as_ref().map(|anime| anime.id);
    state.screen      = Screen::Detail;
    state.pending_playback_query = None;

    let (has_ani_cli, has_mpv, has_iina, has_vlc) = api::player::detect_dependencies();
    state.set_dependencies(has_ani_cli, has_mpv, has_iina, has_vlc);
    if !has_ani_cli {
        state.show_toast("ani-cli is not installed. Open ! for setup help.", unix_now());
        return;
    }
    if !(has_mpv || has_iina || has_vlc) {
        state.show_toast("No supported player found. Open ! for setup help.", unix_now());
        return;
    }

    let mut opts = opts;
    if let Some(ref sub_u) = opts.sub_url {
        if sub_u.starts_with("http://") || sub_u.starts_with("https://") {
            let anime_id = state.selected_anime.as_ref().map(|a| a.id).unwrap_or(0);
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

    let mut child = match api::player::spawn_async(&opts) {
        Ok(c) => c,
        Err(e) => {
            state.screen = Screen::Detail;
            state.show_toast(format!("Playback failed: {e}"), unix_now());
            return;
        }
    };

    state.show_toast(format!("Launching Episode {} in external player", episode), unix_now());

    // Record watch history as soon as playback starts & sync to AniList if logged in
    if let Some(ref anime) = state.selected_anime {
        let pool2      = pool.clone();
        let anime_id   = anime.id;
        let now        = unix_now();
        let auth_token = state.auth.token.clone();
        state.watched_episodes.insert(episode);
        tokio::spawn(async move {
            let _ = db::user::record_watched(&pool2, anime_id, episode as i64, now).await;
            if let Some(token) = auth_token {
                let client = api::anilist::AniListClient::with_token(Some(token));
                let _ = client.save_progress(anime_id, episode).await;
            }
        });
    }

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
                let _ = tx4.send(AppMessage::PlaybackDone).await;
            }
            _ = stop_rx       => {
                let kill_res = child.kill().await;
                let msg = format!("manual stop — kill result: {:?}", kill_res);
                let _ = tx4.send(AppMessage::PlaybackLog(msg)).await;
                let _ = tx4.send(AppMessage::PlaybackDone).await;
            }
        }
    });
}

// ─── Cover image download ─────────────────────────────────────────────────────

/// Prepare an anime cover image for card and banner rendering:
/// Anime posters from AniList are portrait (e.g. 230x345). Home cards allocate a 20-col x 6-row
/// inner cover chunk. In standard terminal cell geometry (1:2 cell aspect ratio),
/// 20 columns by 6 rows corresponds to an aspect ratio of 20 / (6 * 2) = 1.6667 (5:3).
///
/// To eliminate all horizontal deadspace and ensure the image fills 100% of the
/// card frame from border to border without distortion:
/// 1. Scales the image so its width reaches at least 1280px (ensuring `desired.width` in
///    ratatui-image exceeds card width and prevents premature truncation on high-DPI displays).
/// 2. Scales and crops to target 1280x768 (1.6667 aspect ratio) with an upper-center
///    bias (22% offset from top of excess height) to keep character faces, eyes, and action in focus.
pub fn prepare_card_cover(img: image::DynamicImage) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return img;
    }

    let target_w = 1280u32;
    let target_h = 768u32;
    let target_aspect = target_w as f32 / target_h as f32; // 1.6667

    let current_aspect = w as f32 / h as f32;

    if current_aspect < target_aspect {
        // Taller/narrower than target (standard portrait anime poster)
        let scale = target_w as f32 / w as f32;
        let scaled_h = ((h as f32 * scale).round() as u32).max(target_h);
        let scaled = img.resize(target_w, scaled_h, image::imageops::FilterType::Triangle);

        let excess_h = scaled.height().saturating_sub(target_h);
        let top = ((excess_h as f32) * 0.22).round() as u32;
        scaled.crop_imm(0, top.min(excess_h), target_w, target_h)
    } else {
        // Wider than target
        let scale = target_h as f32 / h as f32;
        let scaled_w = ((w as f32 * scale).round() as u32).max(target_w);
        let scaled = img.resize(scaled_w, target_h, image::imageops::FilterType::Triangle);

        let excess_w = scaled.width().saturating_sub(target_w);
        let left = excess_w / 2;
        scaled.crop_imm(left, 0, target_w, target_h)
    }
}

/// Prepare an anime cover image for portrait poster preview rendering:
/// Tailored results allocate a 20-col x 16-row box (inner: 18 cols x 14 rows).
/// In standard terminal cell geometry (1:2 cell aspect ratio),
/// 18 columns by 14 rows corresponds to an aspect ratio of 18 / (14 * 2) = 0.6428.
///
/// To eliminate all horizontal and vertical deadspace and ensure the image fills 100% of the
/// poster frame from border to border without distortion:
/// 1. Target dimensions: 720x1120 (0.6428 aspect ratio).
/// 2. Scales and crops with a slight upper-center bias (15% offset from top of excess height)
///    to keep character faces, eyes, and action in focus.
pub fn prepare_portrait_cover(img: image::DynamicImage) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return img;
    }

    let target_w = 1200u32;
    let target_h = 1800u32;
    let target_aspect = target_w as f32 / target_h as f32; // ~0.6667 (2:3 standard anime poster)

    let current_aspect = w as f32 / h as f32;

    if current_aspect < target_aspect {
        // Taller than target aspect -> scale to target_w, crop excess height with upper bias
        let scale = target_w as f32 / w as f32;
        let scaled_h = ((h as f32 * scale).round() as u32).max(target_h);
        let scaled = img.resize(target_w, scaled_h, image::imageops::FilterType::Triangle);

        let excess_h = scaled.height().saturating_sub(target_h);
        let top = ((excess_h as f32) * 0.15).round() as u32;
        scaled.crop_imm(0, top.min(excess_h), target_w, target_h)
    } else {
        // Wider than target aspect -> scale to target_h, crop excess width centered
        let scale = target_h as f32 / h as f32;
        let scaled_w = ((w as f32 * scale).round() as u32).max(target_w);
        let scaled = img.resize(scaled_w, target_h, image::imageops::FilterType::Triangle);

        let excess_w = scaled.width().saturating_sub(target_w);
        let left = excess_w / 2;
        scaled.crop_imm(left, 0, target_w, target_h)
    }
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

/// Decode blob from DB or fetch from URL, cache the blob on success.
async fn download_cover_image(
    anime: &db::cache::Anime,
    pool:  &sqlx::SqlitePool,
) -> Option<image::DynamicImage> {
    if let Some(ref blob) = anime.cover_blob {
        return image::load_from_memory(blob).ok();
    }
    let url   = anime.cover_url.as_ref()?;
    let resp  = reqwest::get(url).await.ok()?;
    let bytes = resp.bytes().await.ok()?;
    let img   = image::load_from_memory(&bytes).ok()?;
    let _     = db::cache::store_cover_blob(pool, anime.id, &bytes).await;
    Some(img)
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

fn render_loading(frame: &mut ratatui::Frame) {
    use ratatui::{
        layout::{Alignment, Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::Paragraph,
    };

    let area = frame.area();
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let msg = Paragraph::new(vec![
        Line::from(Span::styled(
            "SOLUS",
            Style::default()
                .fg(Color::Rgb(180, 0, 255))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Fetching content...",
            Style::default().fg(Color::Rgb(120, 120, 140)),
        )),
    ])
    .alignment(Alignment::Center);

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
                render_loading(frame);
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
        assert_eq!(prepared.width(), 1280);
        assert_eq!(prepared.height(), 768);
    }

    #[test]
    fn test_prepare_card_cover_landscape() {
        // Landscape image
        let img = image::DynamicImage::new_rgb8(800, 400);
        let prepared = prepare_card_cover(img);
        assert_eq!(prepared.width(), 1280);
        assert_eq!(prepared.height(), 768);
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
        assert_eq!(prepared.width(), 1200);
        assert_eq!(prepared.height(), 1800);
    }
}
