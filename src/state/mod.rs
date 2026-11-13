//! Application state — the single source of truth for the TUI.

use crate::db::cache::Anime;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use std::collections::{HashMap, HashSet};

/// Which screen is currently active.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// Home screen: category rows + featured banner
    Home,
    /// Detail screen: full info + episode list for a selected anime
    Detail,
    /// Playback query picker overlay shown on top of Detail
    PlaybackQuery,
    /// Playback options overlay shown on top of Detail
    PlaybackOptions,
    /// Playback screen: log stream + controls
    Playback,
    /// Search overlay (shown on top of Home or Detail)
    Search,
    /// Help overlay
    Help,
    /// Settings overlay
    Settings,
    /// Dependency / onboarding overlay
    Setup,
    /// AniList OAuth login overlay
    AniListLogin,
    /// Tailored Algorithm: 10-question questionnaire wizard
    TailoredWizard,
    /// Tailored Algorithm: Interactive Persona dashboard & taste constellation diagram
    TailoredSummary,
    /// Tailored Algorithm: Scored anime results shelf
    TailoredResults,
    /// Personal Anime Wrapped & Taste Analytics Dashboard
    TasteAnalytics,
    /// Mobile LAN remote companion QR code modal
    RemoteModal,
    /// AirPlay / DLNA TV device scanner & cast target picker
    CastModal,
    /// Active TV streaming command deck
    CastDeck,
}

/// Which category row the cursor is on (Home screen).
#[derive(Debug, Clone, PartialEq)]
pub enum CategoryRow {
    ContinueWatching,
    Watchlist,
    Recommended,
    Trending,
    Popular,
    TopRated,
    Seasonal,
}

/// Which section is currently focused on the Detail screen.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum DetailFocus {
    Episodes,
    Related,
}

/// Detail screen episode browser layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpisodeViewMode {
    List,
    #[default]
    Split,
}

#[derive(Clone)]
struct DetailSnapshot {
    anime: Anime,
    in_watchlist: bool,
    watched_episodes: HashSet<u32>,
    detail_recommendations: Vec<Anime>,
    detail_recommendation_reasons: HashMap<i64, String>,
    detail_focus: DetailFocus,
    detail_related_cursor: usize,
    detail_related_offset: usize,
    detail_origin_title: Option<String>,
    last_played: Option<String>,
    last_played_anime_id: Option<i64>,
}

/// The full application state passed to every render call.
pub struct AppState {
    /// Currently active screen
    pub screen:           Screen,

    /// Base screen to render under overlays
    pub overlay_base:     Screen,

    /// Home screen: which row the cursor is on
    pub active_row:       CategoryRow,

    /// Home screen: horizontal card index per row
    pub row_offsets:      std::collections::HashMap<String, usize>,

    /// Detail screen: the anime being viewed
    pub selected_anime:   Option<Anime>,

    /// Detail screen: episode list (1..=N generated from anime.episodes)
    pub episode_list:     Vec<u32>,

    /// Detail screen: which episode is highlighted
    pub selected_episode: Option<u32>,

    /// Detail screen: episode list scroll offset
    pub episode_offset:   usize,

    /// Detail screen: whether episodes are shown in vertical List or Split-Deck view
    pub episode_view_mode: EpisodeViewMode,

    /// Detail screen: localized episode titles mapped by episode number
    pub episode_titles: HashMap<u32, String>,

    /// Detail screen: localized episode synopses mapped by episode number
    pub episode_synopses: HashMap<u32, String>,

    /// Detail screen: 16:9 thumbnail URLs mapped by episode number
    pub episode_thumbnail_urls: HashMap<u32, String>,

    /// Detail screen: 16:9 episode thumbnail stateful protocols for Kitty/Halfblock
    pub episode_thumbnails: HashMap<u32, Box<dyn StatefulProtocol>>,

    /// Detail screen: per-episode fallback 16:9 thumbnail protocols (prevents Kitty ID collisions while real thumbnails download or for unaired episodes)
    pub episode_fallback_thumbnails: HashMap<u32, Box<dyn StatefulProtocol>>,

    /// In-flight episode thumbnail downloads
    pub loading_episode_thumbnails: HashSet<u32>,

    /// Whether episode metadata is actively loading from AniList
    pub loading_episode_metadata: bool,

    /// Show's 16:9 banner image protocol (Kitty / Halfblock)
    pub anime_banners: HashMap<i64, Box<dyn StatefulProtocol>>,

    /// Show's 16:9 banner image URL
    #[allow(dead_code)]
    pub anime_banner_urls: HashMap<i64, String>,

    /// In-flight banner downloads
    pub loading_banners: HashSet<i64>,

    /// Search overlay: current input text
    pub search_query:     String,

    /// Search overlay: results list
    pub search_results:   Vec<Anime>,

    /// Search overlay: which result is highlighted
    pub search_cursor:    usize,

    /// Settings overlay: which preference row is focused
    pub settings_cursor: usize,

    /// Settings overlay: fuzzy search query for instant filtering
    pub settings_query: String,

    /// Playback query picker: candidate search queries to send to ani-cli
    pub playback_queries: Vec<String>,

    /// Playback query picker: selected query index
    pub playback_query_cursor: usize,

    /// Playback options: available video stream resolutions for the inspected episode
    pub playback_qualities: Vec<crate::api::subtitles::QualityOption>,

    /// Playback options: currently selected quality index
    pub playback_quality_cursor: usize,

    /// Playback options: available subtitle languages for the currently inspected anime episode
    pub playback_sub_languages: Vec<crate::api::subtitles::SubtitleOption>,

    /// Playback options: selected subtitle language index
    pub playback_sub_cursor: usize,

    /// Playback options: available streaming providers / server mirrors
    pub playback_providers: Vec<crate::api::subtitles::ProviderOption>,

    /// Playback options: whether English dub audio is verified available for this episode
    pub dub_available: Option<bool>,

    /// Playback options: selected streaming provider index
    pub playback_provider_cursor: usize,

    /// Playback options: available media playback engines (mpv, iina, vlc)
    pub playback_players: Vec<crate::config::Player>,

    /// Playback options: selected playback engine index
    pub playback_player_cursor: usize,

    /// Playback options: focused column index (0 = Player, 1 = Provider, 2 = Quality, 3 = Subtitles)
    pub playback_focus_column: usize,

    /// Playback options: which section is focused (false = Quality, true = Subtitles)
    pub playback_sub_focused: bool,

    /// Playback options: whether stream resolutions and subtitles are actively being fetched
    pub is_loading_subs: bool,

    /// Playback options: direct master m3u8 URL if resolved
    pub pending_master_url: Option<String>,

    /// Playback options: pending query chosen before launch
    pub pending_playback_query: Option<String>,

    /// Playback options: pending sub/dub mode for the next launch
    pub pending_dub:       bool,

    /// Playback: oneshot sender to stop the background player task
    pub player_stop:      Option<tokio::sync::oneshot::Sender<()>>,

    /// Playback: title + episode currently playing (for display)
    pub now_playing:      Option<String>,

    /// Playback: log lines from ani-cli stdout/stderr
    pub playback_logs:    Vec<String>,

    /// Detail: whether the selected anime is in the watchlist
    pub in_watchlist:     bool,

    /// Cover image: ratatui-image stateful protocol (Kitty / Sixel / Halfblock)
    pub cover_state:      Option<Box<dyn StatefulProtocol>>,

    /// Multi-cover cache for cards
    pub cover_cache:      HashMap<i64, Box<dyn StatefulProtocol>>,

    /// Separate banner protocol cache to prevent Kitty graphics collision
    pub banner_cache:     HashMap<i64, Box<dyn StatefulProtocol>>,

    /// Separate detail/search protocol cache (retains portrait orientation)
    pub detail_cache:     HashMap<i64, Box<dyn StatefulProtocol>>,

    /// In-flight cover downloads
    pub loading_covers:   HashSet<i64>,

    /// Cover image: which anime ID the current cover_state belongs to
    pub cover_anime_id:   Option<i64>,

    /// Cover image: last anime ID whose image failed to load
    pub cover_failed_anime_id: Option<i64>,

    /// Terminal image picker (initialized once at startup)
    pub picker:           Option<Picker>,

    /// DynamicImage for the currently active show's 16:9 card / banner artwork (used to generate per-episode fallback thumbnails)
    pub current_card_img: Option<image::DynamicImage>,

    /// Home banner: watched episode count for the selected anime
    pub banner_progress:  Option<(i64, usize)>,

    /// Detail screen: "More Like This" recommendations
    pub detail_recommendations: Vec<Anime>,

    /// Detail screen: reason labels keyed by anime ID
    pub detail_recommendation_reasons: HashMap<i64, String>,

    /// Detail screen: which area receives h/l and Enter
    pub detail_focus: DetailFocus,

    /// Detail screen: selected "More Like This" card
    pub detail_related_cursor: usize,

    /// Detail screen: horizontal scroll offset for "More Like This"
    pub detail_related_offset: usize,

    /// Detail screen: breadcrumb title when opened from a related recommendation
    pub detail_origin_title: Option<String>,

    /// Detail navigation history when drilling into related anime
    detail_history: Vec<DetailSnapshot>,

    /// Runtime dependency status
    pub has_ani_cli: bool,
    pub has_mpv: bool,
    pub has_iina: bool,
    pub has_vlc: bool,

    /// Toast notification: (message, expiry unix timestamp)
    pub toast:            Option<(String, i64)>,

    /// Whether the app is loading home data (shows spinner)
    pub is_loading:       bool,

    /// Whether the app should quit on next tick
    pub should_quit:      bool,

    /// Detail screen: set of watched episode numbers for the current anime
    pub watched_episodes: HashSet<u32>,

    /// Last launched playback label for the current session
    pub last_played:      Option<String>,

    /// Which anime the last launched playback belongs to
    pub last_played_anime_id: Option<i64>,

    /// Active theme index for multi-palette switcher
    pub theme_index: usize,

    /// AniList authentication configuration and state
    pub auth: crate::config::AuthConfig,

    /// Login overlay input buffer for access token
    pub login_token_input: String,

    /// Login overlay temporary feedback / error message
    pub login_status: Option<String>,

    /// Account sync active tab (0: Local JSON archive, 1: Username sync, 2: Backup export)
    pub account_sync_tab: usize,

    /// Auto-detected JSON export files found in ~/Downloads
    pub detected_export_files: Vec<std::path::PathBuf>,

    /// Cursor index of selected detected export file
    pub detected_file_cursor: usize,

    /// Statistics from the most recent library import
    pub import_stats: Option<crate::services::importer::ImportStats>,

    /// Tailored Wizard active step (0..9)
    pub tailored_step: usize,

    /// Cursor within the currently open question
    pub tailored_cursor: usize,

    /// Answers to the tailored questionnaire
    pub tailored_answers: crate::services::tailored::TailoredAnswers,

    /// Cursor on the TailoredSummary dashboard (0..9 answer tiles, or 10 for Suggest button)
    pub tailored_summary_cursor: usize,

    /// If editing a single question from the dashboard, which question is being modified
    pub tailored_editing_step: Option<usize>,

    /// Results returned by the tailored recommendation engine
    pub tailored_results: Vec<crate::services::tailored::TailoredMatch>,

    /// Active cursor on the TailoredResults screen
    pub tailored_results_cursor: usize,

    /// Whether tailored recommendations are currently being fetched/calculated
    pub is_generating_tailored: bool,

    /// Answers to the 6 follow-up refinement questions
    pub tailored_refinement: Option<crate::services::tailored::TailoredRefinement>,

    /// Whether refinement questionnaire modal is open over results
    pub is_refining_tailored: bool,

    /// Refinement questionnaire step (0..=5: 0-4 are multi-select, 5 is Keystone Anchor)
    pub tailored_refine_step: usize,

    /// Refinement cursor within current question
    pub tailored_refine_cursor: usize,

    /// Multi-select checkboxes for refinement steps 0..=4
    pub refine_selections: [HashSet<usize>; 5],

    /// Search input query for Step 6 Keystone Anchor Anime
    pub refine_anchor_query: String,

    /// Live candidate anime search results for Step 6 Keystone Anchor
    pub refine_anchor_results: Vec<crate::db::cache::Anime>,

    /// Cursor for selecting among Keystone Anchor candidates
    pub refine_anchor_cursor: usize,

    /// Selected Keystone Anchor Anime
    pub refine_anchor_selected: Option<crate::db::cache::Anime>,

    /// Whether an anchor anime search is currently in-flight
    pub is_searching_anchor: bool,

    /// Live playback second and duration progress per episode for the inspected anime
    pub anime_progress: HashMap<u32, (f64, f64)>,

    /// Filler classifications per episode (manga_canon, mixed, filler) for the inspected anime
    pub anime_fillers: HashMap<u32, String>,

    /// Downloaded offline episode file paths mapped by episode number
    pub offline_episodes: HashMap<u32, String>,

    /// Whether the offline episode download modal is open
    pub is_download_modal_open: bool,

    /// Download modal selected mode (0 = Current Episode, 1 = Range, 2 = All Unwatched)
    pub download_modal_mode: usize,

    /// Download modal episode range text input (e.g. "1-12")
    pub download_range_input: String,

    /// Live active background download status: (anime_id, episode, percentage, speed_str)
    pub active_download: Option<(i64, u32, f32, String)>,

    /// Cached MyAnimeList ID mapping for AniSkip lookup
    pub anime_mal_ids: HashMap<i64, i64>,

    /// Dynamically extracted cover-adaptive ambilight theme palette for the active anime
    pub ambilight_theme: Option<crate::ui::theme::ThemePalette>,

    /// Whether the dynamic cover-adaptive ambilight theme is enabled
    pub ambilight_enabled: bool,

    /// Real-time Anime4K GLSL shader mode (Off, Balanced 2x, Ultra 4K)
    pub anime4k_mode: crate::config::Anime4KMode,

    /// Personal Anime Wrapped & viewing telemetry data
    pub analytics_data: Option<crate::db::user::UserAnalytics>,

    /// Metadata for an available application update if one was detected
    pub available_update: Option<crate::services::updater::UpdateInfo>,

    /// Whether an update check is actively in-flight
    pub is_checking_update: bool,

    /// Whether a self-update download is actively in progress
    pub is_updating: bool,

    /// Live update download progress: (percentage, speed_str)
    pub update_progress: Option<(f32, String)>,

    /// Mobile LAN remote control state & rendered ASCII QR code
    pub remote_state: std::sync::Arc<tokio::sync::RwLock<crate::services::remote::RemotePlaybackState>>,
    pub remote_qr: Option<String>,
    pub remote_url: String,

    /// TV Casting state
    pub cast_devices: Vec<crate::services::cast::CastDevice>,
    pub cast_selected_idx: usize,
    pub active_cast_device: Option<crate::services::cast::CastDevice>,
    pub is_casting: bool,
    pub is_scanning_cast: bool,
    pub cast_playback_pos: f64,
    pub cast_playback_dur: f64,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            screen:           Screen::Home,
            overlay_base:     Screen::Home,
            active_row:       CategoryRow::Trending,
            row_offsets:      std::collections::HashMap::new(),
            selected_anime:   None,
            episode_list:     Vec::new(),
            selected_episode: None,
            episode_offset:   0,
            episode_view_mode: EpisodeViewMode::Split,
            episode_titles:   HashMap::new(),
            episode_synopses: HashMap::new(),
            episode_thumbnail_urls: HashMap::new(),
            episode_thumbnails: HashMap::new(),
            episode_fallback_thumbnails: HashMap::new(),
            loading_episode_thumbnails: HashSet::new(),
            loading_episode_metadata: false,
            anime_banners:    HashMap::new(),
            anime_banner_urls: HashMap::new(),
            loading_banners:  HashSet::new(),
            search_query:     String::new(),
            search_results:   Vec::new(),
            search_cursor:    0,
            settings_cursor:  0,
            settings_query:   String::new(),
            playback_queries: Vec::new(),
            playback_query_cursor: 0,
            playback_qualities: Vec::new(),
            playback_quality_cursor: 0,
            playback_sub_languages: vec![crate::api::subtitles::SubtitleOption::default_english()],
            playback_sub_cursor: 0,
            playback_players: vec![crate::config::Player::Mpv, crate::config::Player::Iina, crate::config::Player::Vlc],
            playback_player_cursor: 0,
            playback_providers: crate::api::subtitles::default_providers(),
            dub_available: None,
            playback_provider_cursor: 0,
            playback_focus_column: 0,
            playback_sub_focused: false,
            is_loading_subs: false,
            pending_master_url: None,
            pending_playback_query: None,
            pending_dub: false,
            player_stop:      None,
            now_playing:      None,
            playback_logs:    Vec::new(),
            in_watchlist:     false,
            cover_state:      None,
            cover_cache:      HashMap::new(),
            banner_cache:     HashMap::new(),
            detail_cache:     HashMap::new(),
            loading_covers:   HashSet::new(),
            cover_anime_id:   None,
            cover_failed_anime_id: None,
            picker:           None,
            current_card_img: None,
            banner_progress:  None,
            detail_recommendations: Vec::new(),
            detail_recommendation_reasons: HashMap::new(),
            detail_focus:      DetailFocus::Episodes,
            detail_related_cursor: 0,
            detail_related_offset: 0,
            detail_origin_title: None,
            detail_history:     Vec::new(),
            has_ani_cli:      true,
            has_mpv:          true,
            has_iina:         false,
            has_vlc:          true,
            toast:            None,
            is_loading:       true,
            should_quit:      false,
            watched_episodes: HashSet::new(),
            last_played:      None,
            last_played_anime_id: None,
            theme_index:      0,
            auth:             crate::config::AuthConfig::load(),
            login_token_input: String::new(),
            login_status:     None,
            account_sync_tab: 0,
            detected_export_files: Vec::new(),
            detected_file_cursor: 0,
            import_stats:     None,
            tailored_step:    0,
            tailored_cursor:  0,
            tailored_answers: crate::services::tailored::TailoredAnswers::default(),
            tailored_summary_cursor: 0,
            tailored_editing_step: None,
            tailored_results: Vec::new(),
            tailored_results_cursor: 0,
            is_generating_tailored: false,
            tailored_refinement: None,
            is_refining_tailored: false,
            tailored_refine_step: 0,
            tailored_refine_cursor: 0,
            refine_selections: [
                HashSet::new(),
                HashSet::new(),
                HashSet::new(),
                HashSet::new(),
                HashSet::new(),
            ],
            refine_anchor_query: String::new(),
            refine_anchor_results: Vec::new(),
            refine_anchor_cursor: 0,
            refine_anchor_selected: None,
            is_searching_anchor: false,
            anime_progress:   HashMap::new(),
            anime_fillers:    HashMap::new(),
            offline_episodes: HashMap::new(),
            is_download_modal_open: false,
            download_modal_mode: 0,
            download_range_input: String::new(),
            active_download:  None,
            anime_mal_ids:    HashMap::new(),
            ambilight_theme:  None,
            ambilight_enabled: true,
            anime4k_mode:     crate::config::Anime4KMode::Off,
            analytics_data:   None,
            available_update: None,
            is_checking_update: false,
            is_updating:      false,
            update_progress:  None,
            remote_state:     std::sync::Arc::new(tokio::sync::RwLock::new(crate::services::remote::RemotePlaybackState::default())),
            remote_qr:        None,
            remote_url:       format!("http://{}:8484", crate::services::remote::get_local_ip()),
            cast_devices:     Vec::new(),
            cast_selected_idx: 0,
            active_cast_device: None,
            is_casting:       false,
            is_scanning_cast: false,
            cast_playback_pos: 0.0,
            cast_playback_dur: 0.0,
        }
    }

    pub fn theme(&self) -> crate::ui::theme::ThemePalette {
        if self.ambilight_enabled && self.screen == Screen::Detail {
            if let Some(amb) = self.ambilight_theme {
                return amb;
            }
        }
        *crate::ui::theme::get_theme(self.theme_index)
    }

    /// Toggle Anime4K upscaling preset: Off -> Balanced 2x -> Ultra 4K -> Off
    pub fn toggle_anime4k(&mut self) {
        self.anime4k_mode = match self.anime4k_mode {
            crate::config::Anime4KMode::Off => crate::config::Anime4KMode::Balanced,
            crate::config::Anime4KMode::Balanced => crate::config::Anime4KMode::Ultra,
            crate::config::Anime4KMode::Ultra => crate::config::Anime4KMode::Off,
        };
    }

    /// Open Personal Anime Wrapped & Taste Analytics Dashboard
    pub fn open_taste_analytics(&mut self) {
        if self.screen != Screen::TasteAnalytics {
            self.overlay_base = self.current_base_screen();
            self.screen = Screen::TasteAnalytics;
        }
    }

    /// Show a toast notification (auto-dismissed after 4 seconds).
    pub fn show_toast(&mut self, msg: impl Into<String>, now: i64) {
        self.toast = Some((msg.into(), now + 4));
    }

    /// Check and clear expired toasts. Returns Some(msg) if a toast is active.
    pub fn active_toast(&mut self, now: i64) -> Option<&str> {
        // Check expiry first, then clear (two separate borrows to satisfy NLL)
        let expired = matches!(&self.toast, Some((_, expiry)) if now >= *expiry);
        if expired {
            self.toast = None;
        }
        match &self.toast {
            Some((msg, _)) => Some(msg.as_str()),
            None           => None,
        }
    }

    /// True if this terminal supports real images (Kitty / Sixel / Iterm2).
    pub fn has_image_support(&self) -> bool {
        use ratatui_image::picker::ProtocolType;
        self.picker
            .as_ref()
            .map(|p| p.protocol_type != ProtocolType::Halfblocks)
            .unwrap_or(false)
    }

    /// Stop any currently running player.
    pub fn stop_player(&mut self) {
        if let Some(stop_tx) = self.player_stop.take() {
            let _ = stop_tx.send(());
        }
        self.now_playing = None;
    }

    /// Navigate back: Playback → Detail → Home
    pub fn go_back(&mut self) {
        self.screen = match self.screen {
            Screen::Playback => Screen::Detail,
            Screen::PlaybackQuery => Screen::Detail,
            Screen::PlaybackOptions => Screen::Detail,
            Screen::Detail   => {
                if self.restore_previous_detail() {
                    Screen::Detail
                } else {
                    Screen::Home
                }
            }
            Screen::Search | Screen::Help | Screen::Settings | Screen::Setup | Screen::AniListLogin | Screen::TailoredWizard | Screen::TasteAnalytics | Screen::RemoteModal | Screen::CastModal => {
                if self.overlay_base == self.screen {
                    Screen::Home
                } else {
                    self.overlay_base.clone()
                }
            }
            Screen::CastDeck => Screen::Detail,
            Screen::TailoredSummary => {
                if self.tailored_editing_step.is_some() {
                    self.tailored_editing_step = None;
                    Screen::TailoredSummary
                } else {
                    Screen::Home
                }
            }
            Screen::TailoredResults => Screen::TailoredSummary,
            Screen::Home     => {
                self.should_quit = true;
                Screen::Home
            }
        };
    }

    /// Open the detail screen for a given anime.
    pub fn open_detail(&mut self, anime: Anime) {
        let total = anime.episodes.unwrap_or(0) as u32;
        let aired = anime.aired_episodes();
        let display_total = total.max(aired).max(1);
        let reuse_cover = self.cover_anime_id == Some(anime.id) && self.cover_state.is_some();
        self.episode_list     = (1..=display_total).collect();
        self.selected_episode = Some(1);
        self.episode_offset   = 0;
        self.cover_anime_id   = Some(anime.id);
        if !reuse_cover {
            self.cover_state  = None;            // reset so stale image isn't shown
        }
        self.watched_episodes = HashSet::new();  // will be populated by the caller
        self.episode_titles.clear();
        self.episode_synopses.clear();
        self.episode_thumbnail_urls.clear();
        self.episode_thumbnails.clear();
        self.episode_fallback_thumbnails.clear();
        self.loading_episode_thumbnails.clear();
        self.current_card_img = None;
        self.loading_episode_metadata = true;
        self.anime_progress.clear();
        self.anime_fillers.clear();
        self.offline_episodes.clear();
        self.is_download_modal_open = false;
        self.download_modal_mode = 0;
        self.download_range_input.clear();
        self.detail_recommendations = Vec::new();
        self.detail_recommendation_reasons = HashMap::new();
        self.detail_focus = DetailFocus::Episodes;
        self.detail_related_cursor = 0;
        self.detail_related_offset = 0;
        self.detail_origin_title = None;
        self.selected_anime   = Some(anime);
        self.screen           = Screen::Detail;
    }

    /// Open the search overlay.
    pub fn open_search(&mut self) {
        self.search_query   = String::new();
        self.search_results = Vec::new();
        self.search_cursor  = 0;
        self.overlay_base   = self.current_base_screen();
        self.screen         = Screen::Search;
    }

    /// Open the help overlay.
    pub fn open_help(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.screen = Screen::Help;
    }

    /// Open the settings overlay.
    pub fn open_settings(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.settings_cursor = 0;
        self.settings_query.clear();
        self.screen = Screen::Settings;
    }

    /// Open the dependency / setup overlay.
    pub fn open_setup(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.screen = Screen::Setup;
    }

    /// Open the AniList login and library sync overlay.
    pub fn open_login(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.login_token_input = String::new();
        self.login_status = None;
        self.account_sync_tab = 0;
        self.detected_export_files = crate::services::importer::detect_export_files();
        self.detected_file_cursor = 0;
        self.import_stats = None;
        self.screen = Screen::AniListLogin;
    }

    /// Open the Tailored Algorithm 10-question wizard.
    pub fn open_tailored_wizard(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.tailored_step = 0;
        self.tailored_cursor = self.tailored_answers.choices[0];
        self.tailored_editing_step = None;
        self.screen = Screen::TailoredWizard;
    }

    /// Open the Tailored Algorithm interactive Persona dashboard.
    pub fn open_tailored_summary(&mut self) {
        self.tailored_summary_cursor = 0;
        self.tailored_editing_step = None;
        self.screen = Screen::TailoredSummary;
    }

    /// Open the Tailored Algorithm results shelf with recommendations.
    pub fn open_tailored_results(&mut self, results: Vec<crate::services::tailored::TailoredMatch>) {
        self.tailored_results = results;
        self.tailored_results_cursor = 0;
        self.is_generating_tailored = false;
        self.is_refining_tailored = false;
        self.screen = Screen::TailoredResults;
    }

    /// Open the 6-question Crucible adaptive refinement overlay over results shelf.
    pub fn open_tailored_refine(&mut self) {
        self.is_refining_tailored = true;
        self.tailored_refine_step = 0;
        self.tailored_refine_cursor = 0;
        self.refine_selections = [
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
        ];
        self.refine_anchor_query.clear();
        self.refine_anchor_results.clear();
        self.refine_anchor_cursor = 0;
        self.refine_anchor_selected = None;
        self.is_searching_anchor = false;
    }

    /// Close the adaptive refinement overlay.
    pub fn close_tailored_refine(&mut self) {
        self.is_refining_tailored = false;
        self.tailored_refine_step = 0;
        self.tailored_refine_cursor = 0;
        self.is_searching_anchor = false;
    }

    /// Open the playback query picker for the current anime.
    pub fn open_playback_query_picker(&mut self, anime: &Anime) {
        self.playback_queries = anime.playback_queries();
        self.playback_query_cursor = 0;
        self.pending_playback_query = None;
        self.screen = Screen::PlaybackQuery;
    }

    /// Open the playback options overlay after a query has been chosen.
    pub fn open_playback_options(&mut self, query: String, quality_idx: usize, dub: bool, player: crate::config::Player) {
        self.pending_playback_query = Some(query);
        self.playback_qualities.clear();
        self.playback_quality_cursor = quality_idx;
        self.pending_dub = dub;
        self.playback_sub_cursor = 0;
        self.playback_players = vec![crate::config::Player::Mpv, crate::config::Player::Iina, crate::config::Player::Vlc];
        self.playback_player_cursor = self.playback_players.iter().position(|p| *p == player).unwrap_or(0);
        self.playback_providers = crate::api::subtitles::default_providers();
        self.dub_available = None;
        self.playback_provider_cursor = 0;
        self.playback_focus_column = 0;
        self.playback_sub_focused = false;
        self.is_loading_subs = true;
        self.pending_master_url = None;
        self.playback_sub_languages = vec![crate::api::subtitles::SubtitleOption::default_english()];
        self.screen = Screen::PlaybackOptions;
    }

    /// Return the currently selected media player.
    pub fn selected_player(&self) -> crate::config::Player {
        self.playback_players.get(self.playback_player_cursor).copied().unwrap_or_default()
    }

    /// Dynamically synchronize active quality and subtitle options with the highlighted provider.
    pub fn sync_active_provider(&mut self) {
        if let Some(prov) = self.playback_providers.get(self.playback_provider_cursor) {
            let curr_quality_label = self
                .playback_qualities
                .get(self.playback_quality_cursor)
                .map(|q| q.label.clone());
            let curr_sub_code = self
                .playback_sub_languages
                .get(self.playback_sub_cursor)
                .map(|s| s.code.clone());

            if !prov.qualities.is_empty() {
                self.playback_qualities = prov.qualities.clone();
                if let Some(ref lbl) = curr_quality_label {
                    self.playback_quality_cursor = self
                        .playback_qualities
                        .iter()
                        .position(|q| q.label.starts_with(lbl) || lbl.starts_with(&q.label))
                        .unwrap_or(0);
                } else if self.playback_quality_cursor >= self.playback_qualities.len() {
                    self.playback_quality_cursor = 0;
                }
            }

            if !prov.subtitles.is_empty() {
                self.playback_sub_languages = prov.subtitles.clone();
                if let Some(ref code) = curr_sub_code {
                    self.playback_sub_cursor = self
                        .playback_sub_languages
                        .iter()
                        .position(|s| s.code == *code)
                        .unwrap_or(0);
                } else if self.playback_sub_cursor >= self.playback_sub_languages.len() {
                    self.playback_sub_cursor = 0;
                }
            }
        }
    }

    /// Update watched progress and set the selected episode to the next unwatched.
    pub fn set_watched_episodes(&mut self, watched: HashSet<u32>) {
        self.watched_episodes = watched;
        let next = self.next_unwatched_episode();
        self.selected_episode = Some(next);
        let pills_per_row = 10usize;
        self.episode_offset = next.saturating_sub(1) as usize / pills_per_row * pills_per_row;
    }

    /// Return the next unwatched episode that has already aired,
    /// defaulting to the latest aired episode when all available episodes are watched.
    pub fn next_unwatched_episode(&self) -> u32 {
        let max_aired = self
            .selected_anime
            .as_ref()
            .map(|a| a.aired_episodes())
            .unwrap_or(u32::MAX);

        self.episode_list
            .iter()
            .copied()
            .find(|ep| *ep <= max_aired && !self.watched_episodes.contains(ep))
            .or_else(|| {
                self.episode_list
                    .iter()
                    .copied()
                    .rfind(|ep| *ep <= max_aired)
            })
            .or_else(|| self.episode_list.first().copied())
            .unwrap_or(1)
    }

    /// Push a log line to the playback log buffer (capped at 200 lines).
    pub fn push_log(&mut self, line: String) {
        if self.playback_logs.len() >= 200 {
            self.playback_logs.remove(0);
        }
        self.playback_logs.push(line);
    }

    /// Check playback logs for known ani-cli error messages.
    /// Returns a user-facing error message if the logs indicate a failure.
    pub fn playback_error_message(&self) -> Option<&str> {
        for log in self.playback_logs.iter().rev() {
            if log.contains("Episode is released, but no valid sources") {
                return Some("No valid sources. The provider may be down — try again later.");
            }
            if log.contains("Failed to spawn ani-cli") {
                return Some("Playback failed. Check that ani-cli and a player are installed.");
            }
            if log.contains("Program ") && log.contains("not found") {
                return Some("Missing player or dependency. Open ! for setup help.");
            }
            if log.contains("No results found") {
                return Some("Anime title could not be found on streaming sources.");
            }
        }
        None
    }

    /// Get or default the card offset for a named row.
    pub fn row_offset(&self, row: &str) -> usize {
        *self.row_offsets.get(row).unwrap_or(&0)
    }

    /// Scroll a row right (card index + 1), clamped to max_cards - 1.
    pub fn scroll_row_right(&mut self, row: &str, max_cards: usize) {
        let entry = self.row_offsets.entry(row.to_string()).or_insert(0);
        if *entry + 1 < max_cards {
            *entry += 1;
        }
    }

    /// Scroll a row left (card index - 1), clamped at 0.
    pub fn scroll_row_left(&mut self, row: &str) {
        let entry = self.row_offsets.entry(row.to_string()).or_insert(0);
        *entry = entry.saturating_sub(1);
    }

    /// Return the base screen that should remain visible under overlays.
    pub fn current_base_screen(&self) -> Screen {
        match self.screen {
            Screen::Detail | Screen::Playback | Screen::PlaybackQuery | Screen::PlaybackOptions => {
                Screen::Detail
            }
            Screen::Search | Screen::Help | Screen::Settings | Screen::Setup | Screen::AniListLogin | Screen::TailoredWizard | Screen::TailoredSummary | Screen::TailoredResults | Screen::TasteAnalytics | Screen::RemoteModal | Screen::CastModal => self.overlay_base.clone(),
            Screen::CastDeck => Screen::Detail,
            Screen::Home => Screen::Home,
        }
    }

    /// Update the cached dependency status.
    pub fn set_dependencies(&mut self, ani_cli: bool, mpv: bool, iina: bool, vlc: bool) {
        self.has_ani_cli = ani_cli;
        self.has_mpv = mpv;
        self.has_iina = iina;
        self.has_vlc = vlc;
    }

    /// True when playback can run with the currently installed tools.
    pub fn has_any_player(&self) -> bool {
        self.has_mpv || self.has_iina || self.has_vlc
    }

    /// Save the current detail screen so a related drill-down can return to it.
    #[allow(dead_code)]
    pub fn push_detail_snapshot(&mut self) {
        let Some(anime) = self.selected_anime.clone() else {
            return;
        };

        self.detail_history.push(DetailSnapshot {
            anime,
            in_watchlist: self.in_watchlist,
            watched_episodes: self.watched_episodes.clone(),
            detail_recommendations: self.detail_recommendations.clone(),
            detail_recommendation_reasons: self.detail_recommendation_reasons.clone(),
            detail_focus: self.detail_focus.clone(),
            detail_related_cursor: self.detail_related_cursor,
            detail_related_offset: self.detail_related_offset,
            detail_origin_title: self.detail_origin_title.clone(),
            last_played: self.last_played.clone(),
            last_played_anime_id: self.last_played_anime_id,
        });
    }

    fn restore_previous_detail(&mut self) -> bool {
        let Some(snapshot) = self.detail_history.pop() else {
            return false;
        };

        let total = snapshot.anime.episodes.unwrap_or(0) as u32;
        self.episode_list = (1..=total.max(1)).collect();
        self.selected_anime = Some(snapshot.anime);
        self.in_watchlist = snapshot.in_watchlist;
        self.detail_recommendations = snapshot.detail_recommendations;
        self.detail_recommendation_reasons = snapshot.detail_recommendation_reasons;
        self.detail_focus = snapshot.detail_focus;
        self.detail_related_cursor = snapshot.detail_related_cursor;
        self.detail_related_offset = snapshot.detail_related_offset;
        self.detail_origin_title = snapshot.detail_origin_title;
        self.last_played = snapshot.last_played;
        self.last_played_anime_id = snapshot.last_played_anime_id;
        self.set_watched_episodes(snapshot.watched_episodes);
        true
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_anime(id: i64) -> Anime {
        Anime {
            id,
            title_english: Some("Test".into()),
            title_romaji:  "Test".into(),
            title_native:  None,
            description:   None,
            episodes:      Some(12),
            status:        None,
            season:        None,
            season_year:   None,
            score:         Some(80),
            format:        None,
            genres:        "[]".into(),
            cover_url:     None,
            cover_blob:    None,
            has_dub:       0,
            updated_at:    0,
            next_airing_episode: None,
        }
    }

    #[test]
    fn test_initial_screen_is_home() {
        let state = AppState::new();
        assert_eq!(state.screen, Screen::Home);
    }

    #[test]
    fn test_go_back_from_detail_goes_home() {
        let mut state  = AppState::new();
        state.screen   = Screen::Detail;
        state.go_back();
        assert_eq!(state.screen, Screen::Home);
    }

    #[test]
    fn test_go_back_from_playback_goes_detail() {
        let mut state  = AppState::new();
        state.screen   = Screen::Playback;
        state.go_back();
        assert_eq!(state.screen, Screen::Detail);
    }

    #[test]
    fn test_go_back_from_home_sets_quit() {
        let mut state = AppState::new();
        state.go_back();
        assert!(state.should_quit);
    }

    #[test]
    fn test_go_back_from_search_goes_home() {
        let mut state = AppState::new();
        state.screen  = Screen::Search;
        state.go_back();
        assert_eq!(state.screen, Screen::Home);
    }

    #[test]
    fn test_open_detail_sets_screen_and_anime() {
        let mut state = AppState::new();
        state.open_detail(dummy_anime(42));
        assert_eq!(state.screen, Screen::Detail);
        assert_eq!(state.selected_anime.as_ref().unwrap().id, 42);
        assert_eq!(state.selected_episode, Some(1));
    }

    #[test]
    fn test_set_watched_episodes_selects_next_unwatched() {
        let mut state = AppState::new();
        state.open_detail(dummy_anime(42));
        state.set_watched_episodes([1, 2, 4].into_iter().collect());
        assert_eq!(state.selected_episode, Some(3));
    }

    #[test]
    fn test_next_unwatched_falls_back_to_last_episode_when_complete() {
        let mut state = AppState::new();
        state.open_detail(dummy_anime(42));
        state.set_watched_episodes((1..=12).collect());
        assert_eq!(state.selected_episode, Some(12));
    }

    #[test]
    fn test_open_search_clears_previous_state() {
        let mut state         = AppState::new();
        state.search_query    = "naruto".to_string();
        state.search_cursor   = 5;
        state.open_search();
        assert_eq!(state.search_query,  "");
        assert_eq!(state.search_cursor, 0);
        assert_eq!(state.screen,        Screen::Search);
    }

    #[test]
    fn test_push_log_caps_at_200() {
        let mut state = AppState::new();
        for i in 0..250 {
            state.push_log(format!("line {}", i));
        }
        assert_eq!(state.playback_logs.len(), 200);
        // Oldest lines removed, newest at end
        assert_eq!(state.playback_logs.last().unwrap(), "line 249");
    }

    #[test]
    fn test_row_offset_defaults_to_zero() {
        let state = AppState::new();
        assert_eq!(state.row_offset("trending"), 0);
    }

    #[test]
    fn test_scroll_row_right() {
        let mut state = AppState::new();
        state.scroll_row_right("trending", 10);
        assert_eq!(state.row_offset("trending"), 1);
    }

    #[test]
    fn test_scroll_row_right_clamps_at_max() {
        let mut state = AppState::new();
        state.scroll_row_right("trending", 2);
        state.scroll_row_right("trending", 2);
        state.scroll_row_right("trending", 2); // should not exceed 1
        assert_eq!(state.row_offset("trending"), 1);
    }

    #[test]
    fn test_playback_error_message_detects_no_sources() {
        let mut state = AppState::new();
        state.push_log("sharepoint Links Fetched".into());
        state.push_log("hianime Links Fetched".into());
        state.push_log("Specified quality not found, defaulting to best".into());
        state.push_log("Episode is released, but no valid sources!".into());

        let msg = state.playback_error_message();
        assert_eq!(msg, Some("No valid sources. The provider may be down — try again later."));
    }

    #[test]
    fn test_sync_active_provider_updates_qualities_and_subtitles() {
        let mut state = AppState::new();
        state.playback_providers = crate::api::subtitles::default_providers();
        assert_eq!(state.playback_providers.len(), 4);

        // Initially on Provider 0 (ZokoAnime)
        state.playback_provider_cursor = 0;
        state.sync_active_provider();
        assert_eq!(state.playback_qualities[0].label, "1080p");

        // Move to Provider 2 (AnimePahe: uses BD tags like "1080p BD")
        state.playback_provider_cursor = 2;
        state.sync_active_provider();
        assert_eq!(state.playback_qualities[0].label, "1080p BD");
        // Quality cursor should have preserved match on 1080p
        assert_eq!(state.playback_quality_cursor, 0);

        // Move back to Provider 0
        state.playback_provider_cursor = 0;
        state.sync_active_provider();
        assert_eq!(state.playback_qualities[0].label, "1080p");
    }

    #[test]
    fn test_playback_error_message_detects_missing_deps() {
        let mut state = AppState::new();
        state.push_log("Failed to spawn ani-cli: No such file or directory".into());

        let msg = state.playback_error_message();
        assert_eq!(msg, Some("Playback failed. Check that ani-cli and a player are installed."));
    }

    #[test]
    fn test_playback_error_message_none_when_ok() {
        let mut state = AppState::new();
        state.push_log("sharepoint Links Fetched".into());
        state.push_log("sharepoint Links Fetched".into());
        state.push_log("inappropriate ioctl for device".into());

        assert!(state.playback_error_message().is_none());
    }

    #[test]
    fn test_open_taste_analytics_and_go_back() {
        let mut state = AppState::new();
        state.screen = Screen::Home;

        state.open_taste_analytics();
        assert_eq!(state.screen, Screen::TasteAnalytics);

        // Calling open_taste_analytics again while already on the screen must not overwrite overlay_base
        state.open_taste_analytics();
        assert_eq!(state.screen, Screen::TasteAnalytics);

        // Go back must return to Home
        state.go_back();
        assert_eq!(state.screen, Screen::Home);
    }

    #[test]
    fn test_playback_player_selection() {
        let mut state = AppState::new();
        assert_eq!(state.selected_player(), crate::config::Player::Mpv);
        state.playback_player_cursor = 1;
        assert_eq!(state.selected_player(), crate::config::Player::Iina);
        state.playback_player_cursor = 2;
        assert_eq!(state.selected_player(), crate::config::Player::Vlc);
    }

    #[test]
    fn test_taste_analytics_corrupted_overlay_base_escapes_to_home() {
        let mut state = AppState::new();
        state.screen = Screen::TasteAnalytics;
        state.overlay_base = Screen::TasteAnalytics;

        state.go_back();
        assert_eq!(state.screen, Screen::Home);
    }
}
