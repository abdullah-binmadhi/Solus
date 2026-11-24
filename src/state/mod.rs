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
    #[allow(dead_code)]
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
    /// In-app self-updater modal dialog
    UpdateModal,
    /// Seasonal Anime & Movie Release Calendar popup (`[c]`)
    Calendar,
    /// Multi-user "Who's Watching?" profile switcher & manager (`[P]`)
    ProfileModal,
    /// Watch Session Queue & Profile-Scoped Saved Playlists Hub (`[Q]` / `[Ctrl+Q]`)
    QueueModal,
}

/// Active inline text input mode inside the Profile Switcher modal (`[P]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileInputMode {
    #[default]
    None,
    CreateName,
    RenameName,
}

/// Phases of the in-app self-updater modal dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateModalPhase {
    /// Update available: shows version diff, plain-English highlights, and prompts Enter to install.
    Available {
        info: crate::services::updater::UpdateInfo,
    },
    /// Downloading and installing update with live progress bar and plain-English summary.
    Downloading {
        info: Option<crate::services::updater::UpdateInfo>,
        progress_pct: f32,
        speed: String,
    },
    /// Update successfully installed: shows plain-English summary and prompts user to exit and relaunch.
    RestartRequired {
        new_version: String,
        simple_notes: Vec<String>,
    },
    /// First launch after an update (or opened via Control Hub "What's New"):
    /// explains what changed and what was fixed in plain, jargon-free English.
    PostUpdateWelcome {
        version_label: String,
        simple_notes: Vec<String>,
    },
    /// Update failed with an error message.
    Failed {
        error: String,
    },
}

/// Which inline text input mode is active inside `[L]` Connected Accounts & Sync Hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccountInputMode {
    #[default]
    None,
    PublicUsername(crate::config::TrackerProvider),
    ManualToken(crate::config::TrackerProvider),
    ClientId(crate::config::TrackerProvider),
    MalPasswordLoginUsername,
    MalPasswordLoginPassword,
}

/// Mode for the interactive Watchlist Database Target Picker popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchlistPromptMode {
    Add,
    RemoveShared,
}

/// Selectable action row inside the Bento Status & Tracker List Picker popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchlistPickerChoice {
    SetStatus(crate::config::WatchStatus),
    Remove,
}

/// Interactive Status & Watchlist Database Target Picker modal overlay state.
#[derive(Debug, Clone)]
pub struct WatchlistTargetPrompt {
    pub anime: Anime,
    pub mode: WatchlistPromptMode,
    pub cursor: usize,
    pub target: crate::config::WatchlistTarget,
    pub current_status: Option<crate::config::WatchStatus>,
    pub current_source: Option<crate::db::user::WatchlistSource>,
    pub from_detail: bool,
}

impl WatchlistTargetPrompt {
    pub const CHOICES: [WatchlistPickerChoice; 6] = [
        WatchlistPickerChoice::SetStatus(crate::config::WatchStatus::Watching),
        WatchlistPickerChoice::SetStatus(crate::config::WatchStatus::Planning),
        WatchlistPickerChoice::SetStatus(crate::config::WatchStatus::Completed),
        WatchlistPickerChoice::SetStatus(crate::config::WatchStatus::Paused),
        WatchlistPickerChoice::SetStatus(crate::config::WatchStatus::Dropped),
        WatchlistPickerChoice::Remove,
    ];

    pub fn selected_choice(&self) -> WatchlistPickerChoice {
        Self::CHOICES[self.cursor.min(Self::CHOICES.len() - 1)]
    }

    pub fn cycle_target(&mut self, forward: bool) {
        self.target = if forward {
            self.target.next()
        } else {
            self.target.prev()
        };
    }

    #[allow(dead_code)]
    pub fn options_for_mode(&self) -> [crate::config::WatchlistTarget; 4] {
        crate::config::WatchlistTarget::ALL
    }

    #[allow(dead_code)]
    pub fn selected_target(&self) -> crate::config::WatchlistTarget {
        self.target
    }
}

/// Which category row the cursor is on (Home screen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryRow {
    ContinueWatching,
    Watchlist,
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

/// Purpose of the `PlaybackOptions` overlay (play now, pre-configure queue item, or edit existing entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackOptionsPurpose {
    #[default]
    LaunchNow,
    EnqueueSingle,
    EditQueueItem { uid: u64 },
}

/// Confidence source ranking for canonical episode metadata and thumbnails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum ThumbnailSource {
    Fallback = 0,
    AniListIndexFallback = 1,
    Kitsu = 2,
    AniListTitle = 3,
    AniZip = 4,
}

/// Canonical episode metadata & source-ranked thumbnail record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeMediaMetadata {
    pub anime_id: i64,
    pub episode: u32,
    pub title: String,
    pub synopsis: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_source: ThumbnailSource,
}

/// Per-episode thumbnail lifecycle state preventing repeated untrimmed replacement flicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeThumbnailState {
    Empty,
    Fallback,
    Loading,
    Ready,
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

    /// Playback options: available audio / dub tracks parsed from HLS master playlist
    pub playback_audio_tracks: Vec<crate::api::subtitles::AudioTrackOption>,

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

    /// Playback options purpose (LaunchNow, EnqueueSingle, or EditQueueItem)
    pub playback_options_purpose: PlaybackOptionsPurpose,

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

    /// Uncropped source cover images keyed by anime ID for exact per-frame aspect fitting
    pub cover_raw_cache:  HashMap<i64, image::DynamicImage>,

    /// Per-frame exact-aspect cover protocols keyed by `(anime_id, cols, rows)`
    pub fitted_cover_cache: HashMap<(i64, u16, u16), Box<dyn StatefulProtocol>>,

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
    pub has_mpvnet: bool,
    pub has_celluloid: bool,

    /// Toast notification: (message, expiry unix timestamp)
    pub toast:            Option<(String, i64)>,

    /// Whether the app is loading home data (shows spinner / preload progress)
    pub is_loading:       bool,
    /// Total cover items scheduled for the active startup preload batch
    pub preload_total:    usize,
    /// Completed cover items in the active startup preload batch
    pub preload_done:     usize,
    /// Monotonic generation ID to ignore stale preload progress completions
    pub preload_generation: u64,

    /// Whether the app should quit on next tick
    pub should_quit:      bool,

    /// Exit confirmation popup modal state (guards against accidental `q` / `Esc` presses)
    pub exit_confirm_open:   bool,
    pub exit_confirm_cursor: usize,

    /// Non-blocking episode/stream switch concurrency & debounce guards
    pub is_switching_episode: bool,
    pub switch_generation:    u64,
    pub last_next_ep_at:      Option<std::time::Instant>,

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

    /// Account sync active tab (0: Live Trackers, 1: Merge & Conflicts, 2: Archives & Vault)
    pub account_sync_tab: usize,

    /// Focused tracker provider card in Live Trackers view (AniList vs MAL)
    pub account_focus_provider: crate::config::TrackerProvider,

    /// Active inline typing mode inside `[L]` (Navigation vs Typing mode separation)
    pub account_input_mode: AccountInputMode,

    /// Text buffer for the active `[L]` input mode
    pub account_text_input: String,

    /// Ephemeral MAL username held between step 1 (username) and step 2 (password) of `[p]` login
    pub pending_mal_login_username: String,

    /// Currently awaiting browser OAuth loopback callback for this provider
    pub oauth_pending: Option<crate::config::TrackerProvider>,

    /// Watch sender to abort an active loopback OAuth server on Esc
    pub oauth_cancel_tx: Option<tokio::sync::watch::Sender<bool>>,

    /// Cached cross-tracker reconciliation report & conflict diffs
    pub merge_report: Option<crate::services::tracker_merge::MergeReport>,

    /// Selected conflict row in `[2] Merge & Conflicts`
    pub merge_diff_cursor: usize,

    /// Interactive Watchlist Database Target Picker popup when adding/removing tracked items
    pub watchlist_target_prompt: Option<WatchlistTargetPrompt>,

    /// Auto-detected JSON / MAL XML export files found in ~/Downloads
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

    /// Visible filtered/sorted top 30 tailored recommendations
    pub tailored_results: Vec<crate::services::tailored::TailoredMatch>,

    /// Full scored candidate pool (up to 120 items) for instant zero-latency live tuning
    pub tailored_pool: Vec<crate::services::tailored::TailoredMatch>,

    /// Active cursor on the TailoredResults screen
    pub tailored_results_cursor: usize,

    /// Whether tailored recommendations are currently being fetched/calculated
    pub is_generating_tailored: bool,

    /// Whether the Live Vibe Tuner dock is expanded above results (`[f]`)
    pub tuner_open: bool,

    /// Focused row inside the Live Vibe Tuner (`0` = Discovery & Scope, `1` = Dynamic Tag Chips)
    pub tuner_row: usize,

    /// Focused pill column inside the active Live Vibe Tuner row
    pub tuner_col: usize,

    /// Live Vibe Tuner active filters, boosts, exclusions, and `[m]` similarity anchor
    pub tuner_config: crate::services::tailored::LiveTunerConfig,

    /// Top genres/tags extracted from the current candidate pool for one-key boost/hide chips
    pub tuner_available_tags: Vec<String>,

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
    /// GPU mastering & thermal efficiency profile
    pub performance_mode: crate::config::PerformanceMode,
    /// Hardware debanding tier (Auto 1-pass, Off, Strong 2-pass)
    pub deband_mode: crate::config::DebandMode,
    /// 24fps anime camera panning cadence mode
    pub motion_mode: crate::config::MotionMode,

    /// Personal Anime Wrapped & viewing telemetry data
    pub analytics_data: Option<crate::db::user::UserAnalytics>,

    /// Spotlight podium index inside Analytics (#0 = Crowned #1 show, cycled via Left/Right)
    pub analytics_podium_idx: usize,

    /// Scroll offset inside Analytics watchlist/recent log feed
    pub analytics_list_scroll: usize,

    /// Measured maximum scroll offset of the Analytics horizon/recent-logs panel (None until rendered)
    pub analytics_list_max_scroll: Option<usize>,

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

    /// Active phase of the in-app update modal
    pub update_modal_phase: Option<UpdateModalPhase>,

    /// Last launched PlayOptions (preserved so remote episode switching reuses exact player, quality, dub/sub, provider, and subtitle language)
    /// Monotonic playback generation; stale PlaybackDone events are ignored
    pub playback_gen: u64,

    pub last_play_options: Option<crate::api::player::PlayOptions>,

    /// Release Calendar (`[c]`) state
    pub calendar_week_offset: i32,
    pub calendar_day_idx: usize,
    pub calendar_cursor: usize,
    pub calendar_filter: crate::services::calendar::CalendarFilter,
    pub calendar_cache: HashMap<i32, Vec<crate::api::anilist::CalendarRelease>>,
    pub calendar_tracked_ids: HashSet<i64>,
    pub calendar_watched_map: HashMap<i64, HashSet<u32>>,
    pub calendar_tz_offset_secs: i64,
    pub calendar_today_unix: i64,
    pub is_loading_calendar: bool,

    /// Multi-user profile switcher (`[P]`) state
    pub active_profile_id: String,
    pub active_profile_name: String,
    pub active_profile_icon: String,
    pub profiles_list: Vec<crate::db::user::ProfileSummary>,
    pub profile_cursor: usize,
    pub profile_input_mode: ProfileInputMode,
    pub profile_name_input: String,
    pub profile_avatar_cursor: usize,
    pub profile_audio_dub: bool,

    /// Watch Session Queue & Saved Playlists Hub (`[Q]`)
    pub queue:                   crate::services::queue::WatchQueue,
    pub saved_playlists:         Vec<crate::db::playlists::SavedPlaylistSummary>,
    pub playlist_preview_items:  Vec<crate::db::playlists::SavedPlaylistItem>,
    pub queue_focus_pane:        usize,
    pub queue_cursor:            usize,
    pub playlist_cursor:         usize,
    pub playlist_focus_items:    bool,
    pub playlist_item_cursor:    usize,
    pub queue_save_mode:         bool,
    pub queue_save_input:        String,

    /// Unified playlist export/import transfer modal (`[w]` / `[i]`).
    pub playlist_transfer_mode: Option<crate::services::queue::PlaylistTransferKind>,
    pub playlist_transfer_choice: usize,
    pub playlist_transfer_input: String,
    pub playlist_transfer_error: Option<String>,
    pub playlist_import_candidates: Vec<(std::path::PathBuf, crate::services::queue::PortablePlaylist)>,
    pub playlist_import_cursor: usize,

    /// Detail Screen Batch Quick-Queue modal (`[E]`)
    pub is_batch_queue_modal_open: bool,
    pub batch_queue_mode:          usize,
    pub batch_queue_range:         String,
    pub batch_queue_dub:           bool,
    pub batch_queue_quality:       String,
    pub batch_queue_sub_lang:      String,

    /// Playback & metadata request generations + shared subtitle/thumbnail reliability state
    pub playback_request_generation: u64,
    pub episode_metadata_generation: u64,
    pub subtitle_delay_ms:           i32,
    pub preferred_subtitle_code:     String,
    pub episode_media_meta:          HashMap<u32, EpisodeMediaMetadata>,
    pub episode_raw_thumbnails:      HashMap<u32, image::DynamicImage>,
    pub episode_thumbnail_states:    HashMap<u32, EpisodeThumbnailState>,
    pub thumbnail_geometry_key:      Option<(u16, u16, u16, u16)>,
    /// Adaptive Network & Stream Ladder / Auto-ABR state
    pub network_mode:                crate::config::NetworkMode,
    pub force_ipv4:                  bool,
    pub auto_abr_downshift:          bool,
    pub auto_fullscreen:             bool,
    pub auto_pip:                    bool,
    pub pip_corner_idx:              usize,
    pub live_playback:               LivePlaybackTelemetry,
    pub stream_ladder_cache:         HashMap<(i64, u32, bool), crate::api::subtitles::ResolvedStreamLadder>,
    /// Live Speech-VAD subtitle sync & active playback context
    pub hls_proxy:                   Option<std::sync::Arc<crate::services::hls_proxy::HlsProxyServer>>,
    pub sub_sync_in_progress:        bool,
    pub active_stream_url_for_sync:  Option<String>,
    pub active_stream_referer:       Option<String>,
    pub active_sub_path:             Option<String>,
    pub active_provider_id:          Option<String>,
    pub active_ref_sync_source:      Option<(String, Option<String>, Option<String>)>,
    /// Settings Control Hub v2 navigation + Remote live state
    pub settings_category:           usize,
    pub settings_scroll_offset:      usize,
    pub remote_ws_clients:           usize,
}

/// Real-time playback buffer telemetry and Auto-ABR downshift tracking.
#[derive(Debug, Clone, Default)]
pub struct LivePlaybackTelemetry {
    pub anime_id:                Option<i64>,
    pub episode:                 u32,
    pub is_dub:                  bool,
    pub is_offline_local:        bool,
    pub player:                  String,
    pub started_at:              Option<std::time::Instant>,
    pub current_height:          u32,
    pub current_fps_x10:         u32,
    pub current_tbr_kbps:        u32,
    pub current_url:             String,
    pub paused_for_cache:        bool,
    pub buffering_pct:           u8,
    pub cache_duration_secs:     f64,
    pub cache_fw_bytes:          u64,
    pub cache_speed_bps:         f64,
    pub stall_count:             u32,
    pub recent_stall_times:      std::collections::VecDeque<std::time::Instant>,
    pub last_abr_switch_at:      Option<std::time::Instant>,
    pub abr_downshift_count:     u32,
    pub last_abr_note:           Option<String>,
    pub prompted_congested_mode: bool,
    pub abr_refresh_in_flight:   bool,
    pub last_time_pos:           f64,
    pub pip_mode:                bool,
}

impl LivePlaybackTelemetry {
    /// Reset all counters and telemetry for a newly started playback session.
    #[allow(clippy::too_many_arguments)]
    pub fn reset_for_session(
        &mut self,
        anime_id: i64,
        episode: u32,
        is_dub: bool,
        is_offline_local: bool,
        player: impl Into<String>,
        height: u32,
        fps_x10: u32,
        tbr_kbps: u32,
        url: impl Into<String>,
    ) {
        *self = Self {
            anime_id: Some(anime_id),
            episode,
            is_dub,
            is_offline_local,
            player: player.into(),
            started_at: Some(std::time::Instant::now()),
            current_height: height,
            current_fps_x10: fps_x10,
            current_tbr_kbps: tbr_kbps,
            current_url: url.into(),
            ..Self::default()
        };
    }

    /// Clear active session telemetry once playback stops.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Record a cache stall and prune stall timestamps outside the 60-second window.
    pub fn record_stall(&mut self, now: std::time::Instant) {
        self.stall_count = self.stall_count.saturating_add(1);
        self.recent_stall_times.push_back(now);
        while self
            .recent_stall_times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t).as_secs() > 60)
        {
            self.recent_stall_times.pop_front();
        }
    }

    /// Return whether an automatic ABR downshift is permitted right now.
    pub fn can_abr_downshift(&self, now: std::time::Instant) -> bool {
        if self.is_offline_local {
            return false;
        }
        let past_warmup = self
            .started_at
            .is_some_and(|started| now.saturating_duration_since(started).as_secs() >= 6);
        let past_start_pos = self.last_time_pos > 2.0;
        let past_cooldown = self
            .last_abr_switch_at
            .is_none_or(|last| now.saturating_duration_since(last).as_secs() >= 25);
        past_warmup && past_start_pos && past_cooldown
    }

    /// Convert measured cache byte throughput to kilobits per second.
    pub fn cache_speed_kbps(&self) -> f64 {
        (self.cache_speed_bps * 8.0) / 1000.0
    }

    /// Convert forward demuxer cache bytes to megabytes.
    pub fn cache_mb(&self) -> f64 {
        self.cache_fw_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Concise, properly spaced status string for HUD and metadata chips.
    pub fn compact_status_text(&self) -> String {
        if self.paused_for_cache {
            if self.buffering_pct > 0 {
                return format!(
                    "BUF {}% | {:.1}s | Stl:{}",
                    self.buffering_pct, self.cache_duration_secs, self.stall_count
                );
            }
            return format!(
                "BUFFERING | {:.1}s | Stl:{}",
                self.cache_duration_secs, self.stall_count
            );
        }
        let q = if self.current_height > 0 {
            format!("{}p ", self.current_height)
        } else {
            String::new()
        };
        format!(
            "{q}Buf:{:.0}s | {:.1}MB | {:.0}kbps | Stl:{}",
            self.cache_duration_secs,
            self.cache_mb(),
            self.cache_speed_kbps(),
            self.stall_count
        )
    }
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
            playback_audio_tracks: Vec::new(),
            playback_sub_cursor: 0,
            playback_players: Self::host_players(),
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
            playback_options_purpose: PlaybackOptionsPurpose::LaunchNow,
            player_stop:      None,
            now_playing:      None,
            playback_logs:    Vec::new(),
            in_watchlist:     false,
            cover_state:      None,
            cover_cache:      HashMap::new(),
            banner_cache:     HashMap::new(),
            detail_cache:     HashMap::new(),
            cover_raw_cache:  HashMap::new(),
            fitted_cover_cache: HashMap::new(),
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
            has_ani_cli:      false,
            has_mpv:          false,
            has_mpvnet:       false,
            has_celluloid:    false,
            toast:            None,
            is_loading:       true,
            preload_total:    0,
            preload_done:     0,
            preload_generation: 0,
            should_quit:      false,
            exit_confirm_open: false,
            exit_confirm_cursor: 0,
            is_switching_episode: false,
            switch_generation: 0,
            last_next_ep_at: None,
            watched_episodes: HashSet::new(),
            last_played:      None,
            last_played_anime_id: None,
            theme_index:      0,
            auth:             crate::config::AuthConfig::load(),
            login_token_input: String::new(),
            login_status:     None,
            account_sync_tab: 0,
            account_focus_provider: crate::config::TrackerProvider::AniList,
            account_input_mode: AccountInputMode::None,
            account_text_input: String::new(),
            pending_mal_login_username: String::new(),
            oauth_pending: None,
            oauth_cancel_tx: None,
            merge_report: None,
            merge_diff_cursor: 0,
            watchlist_target_prompt: None,
            detected_export_files: Vec::new(),
            detected_file_cursor: 0,
            import_stats:     None,
            tailored_step:    0,
            tailored_cursor:  0,
            tailored_answers: crate::services::tailored::TailoredAnswers::default(),
            tailored_summary_cursor: 0,
            tailored_editing_step: None,
            tailored_results: Vec::new(),
            tailored_pool:    Vec::new(),
            tailored_results_cursor: 0,
            is_generating_tailored: false,
            tuner_open:       false,
            tuner_row:        0,
            tuner_col:        0,
            tuner_config:     crate::services::tailored::LiveTunerConfig::default(),
            tuner_available_tags: Vec::new(),
            anime_progress:   HashMap::new(),
            anime_fillers:    HashMap::new(),
            offline_episodes: HashMap::new(),
            is_download_modal_open: false,
            download_modal_mode: 0,
            download_range_input: String::new(),
            active_download:  None,
            anime_mal_ids:    HashMap::new(),
            ambilight_theme:  None,
            ambilight_enabled: false,
            anime4k_mode:     crate::config::Anime4KMode::Off,
            performance_mode: crate::config::PerformanceMode::Balanced,
            deband_mode:      crate::config::DebandMode::Auto,
            motion_mode:      crate::config::MotionMode::Off,
            analytics_data:   None,
            analytics_podium_idx: 0,
            analytics_list_scroll: 0,
            analytics_list_max_scroll: None,
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
            update_modal_phase: None,
            playback_gen: 0,
            last_play_options: None,
            calendar_week_offset: 0,
            calendar_day_idx: 0,
            calendar_cursor: 0,
            calendar_filter: crate::services::calendar::CalendarFilter::All,
            calendar_cache: HashMap::new(),
            calendar_tracked_ids: HashSet::new(),
            calendar_watched_map: HashMap::new(),
            calendar_tz_offset_secs: 0,
            calendar_today_unix: 0,
            is_loading_calendar: false,
            active_profile_id: "default".to_string(),
            active_profile_name: "Default".to_string(),
            active_profile_icon: "◈".to_string(),
            profiles_list: Vec::new(),
            profile_cursor: 0,
            profile_input_mode: ProfileInputMode::None,
            profile_name_input: String::new(),
            profile_avatar_cursor: 0,
            profile_audio_dub: false,
            queue: crate::services::queue::WatchQueue::default(),
            saved_playlists: Vec::new(),
            playlist_preview_items: Vec::new(),
            queue_focus_pane: 0,
            queue_cursor: 0,
            playlist_cursor: 0,
            playlist_focus_items: false,
            playlist_item_cursor: 0,
            queue_save_mode: false,
            queue_save_input: String::new(),
            playlist_transfer_mode: None,
            playlist_transfer_choice: 0,
            playlist_transfer_input: String::new(),
            playlist_transfer_error: None,
            playlist_import_candidates: Vec::new(),
            playlist_import_cursor: 0,
            is_batch_queue_modal_open: false,
            batch_queue_mode: 0,
            batch_queue_range: String::new(),
            batch_queue_dub: false,
            batch_queue_quality: "1080p".to_string(),
            batch_queue_sub_lang: "English".to_string(),
            playback_request_generation: 0,
            episode_metadata_generation: 0,
            subtitle_delay_ms: 0,
            preferred_subtitle_code: String::new(),
            episode_media_meta: HashMap::new(),
            episode_raw_thumbnails: HashMap::new(),
            episode_thumbnail_states: HashMap::new(),
            thumbnail_geometry_key: None,
            network_mode: crate::config::NetworkMode::Normal,
            force_ipv4: false,
            auto_abr_downshift: true,
            auto_fullscreen: false,
            auto_pip: false,
            pip_corner_idx: 0,
            live_playback: LivePlaybackTelemetry::default(),
            stream_ladder_cache: HashMap::new(),
            hls_proxy: None,
            sub_sync_in_progress: false,
            active_stream_url_for_sync: None,
            active_stream_referer: None,
            active_sub_path: None,
            active_provider_id: None,
            active_ref_sync_source: None,
            settings_category: 0,
            settings_scroll_offset: 0,
            remote_ws_clients: 0,
        }
    }

    pub fn bump_playback_request_generation(&mut self) -> u64 {
        self.playback_request_generation = self.playback_request_generation.saturating_add(1);
        self.playback_request_generation
    }

    pub fn bump_episode_metadata_generation(&mut self) -> u64 {
        self.episode_metadata_generation = self.episode_metadata_generation.saturating_add(1);
        self.episode_metadata_generation
    }

    pub fn subtitle_delay_secs(&self) -> f64 {
        self.subtitle_delay_ms as f64 / 1000.0
    }

    pub fn format_subtitle_delay(&self) -> String {
        if self.subtitle_delay_ms == 0 {
            "0ms (0.0s)".to_string()
        } else {
            format!("{:+}ms ({:+.1}s)", self.subtitle_delay_ms, self.subtitle_delay_secs())
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
        self.analytics_list_scroll = 0;
        self.analytics_list_max_scroll = None;
    }

    /// Adjust the Analytics horizon/recent-logs scroll offset, clamping cleanly when layout bounds are known.
    pub fn scroll_analytics_list(&mut self, delta: isize) {
        let next = if delta >= 0 {
            self.analytics_list_scroll.saturating_add(delta as usize)
        } else {
            self.analytics_list_scroll.saturating_sub(delta.unsigned_abs())
        };
        self.analytics_list_scroll = match self.analytics_list_max_scroll {
            Some(max_s) => next.min(max_s),
            None => next,
        };
    }

    /// Return a cached `StatefulProtocol` scaled cleanly to `cols x rows` from `cover_raw_cache`
    /// (falling back to `detail_cache` and optionally `cover_cache` when raw art isn't ready yet),
    /// preserving Kitty graphic IDs across renders and filling 100% of the target rect.
    pub fn resolve_cover_protocol(
        &mut self,
        anime_id: i64,
        cols: u16,
        rows: u16,
        upper_bias: f32,
        allow_card_fallback: bool,
    ) -> Option<&mut Box<dyn StatefulProtocol>> {
        if cols > 0 && rows > 0 {
            if let Some(picker) = self.picker {
                let key = (anime_id, cols, rows);
                if !self.fitted_cover_cache.contains_key(&key) {
                    if let Some(raw) = self.cover_raw_cache.get(&anime_id) {
                        let fitted = if raw.width() > raw.height() {
                            crate::ui::components::cover::scale_to_frame_contain(
                                raw.clone(),
                                cols,
                                rows,
                                picker.font_size,
                            )
                        } else {
                            crate::ui::components::cover::scale_to_frame_cover(
                                raw.clone(),
                                cols,
                                rows,
                                picker.font_size,
                                upper_bias,
                            )
                        };
                        let proto =
                            crate::ui::components::cover::build_stateful_protocol(picker, fitted);
                        self.fitted_cover_cache.insert(key, proto);
                    }
                }
                if let Some(proto) = self.fitted_cover_cache.get_mut(&key) {
                    return Some(proto);
                }
            }
        }
        if let Some(proto) = self.detail_cache.get_mut(&anime_id) {
            return Some(proto);
        }
        if allow_card_fallback {
            return self.cover_cache.get_mut(&anime_id);
        }
        None
    }

    /// Clear encoded Kitty/terminal `StatefulProtocol` widgets while preserving decoded raw images
    /// in RAM (`cover_raw_cache`, `episode_raw_thumbnails`, `current_card_img`) so returning from
    /// playback or closing modal overlays re-transmits every cover & episode thumbnail cleanly.
    pub fn invalidate_detail_image_protocols(&mut self) {
        self.fitted_cover_cache.clear();
        self.detail_cache.clear();
        self.episode_thumbnails.clear();
        self.episode_fallback_thumbnails.clear();
        self.thumbnail_geometry_key = None;
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

    /// True when an image picker is initialized (Kitty / Sixel / iTerm2 / RGB Halfblocks).
    pub fn has_image_support(&self) -> bool {
        self.picker.is_some()
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
            Screen::Search | Screen::Help | Screen::Settings | Screen::Setup | Screen::AniListLogin | Screen::TailoredWizard | Screen::TasteAnalytics | Screen::RemoteModal | Screen::CastModal | Screen::UpdateModal | Screen::Calendar | Screen::ProfileModal | Screen::QueueModal => {
                if self.screen == Screen::QueueModal {
                    self.queue_save_mode = false;
                }
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

    /// Open the exit confirmation popup defaulting to Cancel so double-taps stay safe.
    pub fn open_exit_confirm(&mut self) {
        self.exit_confirm_open = true;
        self.exit_confirm_cursor = 0;
    }

    /// Dismiss the exit confirmation popup and remain in Solus.
    pub fn close_exit_confirm(&mut self) {
        self.exit_confirm_open = false;
        self.exit_confirm_cursor = 0;
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
        self.episode_media_meta.clear();
        self.episode_raw_thumbnails.clear();
        self.episode_thumbnail_states.clear();
        self.bump_episode_metadata_generation();
        self.bump_playback_request_generation();
        self.current_card_img = None;
        self.loading_episode_metadata = true;
        self.anime_progress.clear();
        self.anime_fillers.clear();
        self.offline_episodes.clear();
        self.is_download_modal_open = false;
        self.download_modal_mode = 0;
        self.download_range_input.clear();
        self.is_batch_queue_modal_open = false;
        self.batch_queue_mode = 0;
        self.batch_queue_range.clear();
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
        self.settings_scroll_offset = 0;
        self.settings_category = 0;
        self.settings_query.clear();
        self.screen = Screen::Settings;
    }

    /// Open the dependency / setup overlay.
    pub fn open_setup(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.screen = Screen::Setup;
    }

    /// Open the Watch Session Queue & Saved Playlists Hub (`[Q]` / `[Ctrl+Q]`).
    pub fn open_queue_modal(&mut self, playlists: Vec<crate::db::playlists::SavedPlaylistSummary>) {
        if self.screen != Screen::QueueModal {
            self.overlay_base = self.current_base_screen();
        }
        self.saved_playlists = playlists;
        self.queue_save_mode = false;
        self.queue_save_input.clear();
        self.playlist_focus_items = false;
        self.playlist_item_cursor = 0;
        if self.queue_cursor >= self.queue.len() {
            self.queue_cursor = self.queue.len().saturating_sub(1);
        }
        if self.playlist_cursor >= self.saved_playlists.len() {
            self.playlist_cursor = self.saved_playlists.len().saturating_sub(1);
        }
        if self.queue.is_empty() && !self.saved_playlists.is_empty() {
            self.queue_focus_pane = 1;
        }
        self.screen = Screen::QueueModal;
    }

    /// Open the Detail batch quick-queue preset / range modal (`[E]`).
    #[allow(dead_code)]
    pub fn open_batch_queue_modal(&mut self, default_dub: bool) {
        let q = self.batch_queue_quality.clone();
        let sub = self.batch_queue_sub_lang.clone();
        self.open_batch_queue_modal_with_prefs(default_dub, &q, &sub);
    }

    /// Open the Detail batch quick-queue modal seeded with active user quality and subtitle defaults.
    pub fn open_batch_queue_modal_with_prefs(
        &mut self,
        default_dub: bool,
        quality:     &str,
        sub_lang:    &str,
    ) {
        let ep = self.selected_episode.unwrap_or(1);
        let max_aired = self
            .selected_anime
            .as_ref()
            .map(|a| a.aired_episodes())
            .unwrap_or(1)
            .max(1);
        let end_sample = (ep + 2).min(max_aired);
        self.is_batch_queue_modal_open = true;
        self.batch_queue_mode = 1; // Default to Append selected episode
        self.batch_queue_range = if ep < end_sample {
            format!("{ep}-{end_sample}")
        } else {
            format!("{ep}")
        };
        self.batch_queue_dub = default_dub;
        self.batch_queue_quality = if quality.trim().is_empty() {
            "1080p".to_string()
        } else {
            quality.to_string()
        };
        self.batch_queue_sub_lang = if sub_lang.trim().is_empty() {
            "English".to_string()
        } else {
            sub_lang.to_string()
        };
    }

    /// Open the in-app update modal dialog.
    pub fn open_update_modal(&mut self, info: crate::services::updater::UpdateInfo) {
        if self.screen != Screen::UpdateModal {
            self.overlay_base = self.current_base_screen();
        }
        self.available_update = Some(info.clone());
        self.update_modal_phase = Some(UpdateModalPhase::Available { info });
        self.screen = Screen::UpdateModal;
    }

    /// Open the Post-Update "What's New & Fixed" plain-English welcome modal.
    pub fn open_post_update_welcome(&mut self, version_label: String, simple_notes: Vec<String>) {
        if self.screen != Screen::UpdateModal {
            self.overlay_base = self.current_base_screen();
        }
        self.update_modal_phase = Some(UpdateModalPhase::PostUpdateWelcome {
            version_label,
            simple_notes,
        });
        self.screen = Screen::UpdateModal;
    }

    /// Close the in-app update modal dialog.
    pub fn close_update_modal(&mut self) {
        if self.screen == Screen::UpdateModal {
            self.screen = self.overlay_base.clone();
            self.update_modal_phase = None;
        }
    }

    /// Open the Connected Trackers & Sync Hub overlay (`[L]`).
    pub fn open_login(&mut self) {
        self.overlay_base = self.current_base_screen();
        self.login_token_input = String::new();
        self.account_text_input = String::new();
        self.pending_mal_login_username = String::new();
        self.account_input_mode = AccountInputMode::None;
        self.login_status = None;
        self.account_sync_tab = 0;
        self.merge_diff_cursor = 0;
        self.detected_export_files = crate::services::importer::detect_export_files();
        self.detected_file_cursor = 0;
        self.import_stats = None;
        self.screen = Screen::AniListLogin;
    }

    /// Cancel any active loopback OAuth server cleanly.
    pub fn cancel_pending_oauth(&mut self) {
        if let Some(tx) = self.oauth_cancel_tx.take() {
            let _ = tx.send(true);
        }
        self.oauth_pending = None;
    }

    /// Open the interactive Bento Status & Watchlist Database Target Picker popup.
    pub fn open_watchlist_status_picker(
        &mut self,
        anime: Anime,
        mode: WatchlistPromptMode,
        current_status: Option<crate::config::WatchStatus>,
        current_source: Option<crate::db::user::WatchlistSource>,
        from_detail: bool,
    ) {
        let has_al = self.auth.has_anilist();
        let has_mal = self.auth.has_mal();
        let default_target = match current_source {
            Some(crate::db::user::WatchlistSource::Both) if has_al && has_mal => {
                crate::config::WatchlistTarget::Both
            }
            Some(crate::db::user::WatchlistSource::AniList) if has_al => {
                crate::config::WatchlistTarget::AniList
            }
            Some(crate::db::user::WatchlistSource::Mal) if has_mal => {
                crate::config::WatchlistTarget::Mal
            }
            _ => match (has_al, has_mal) {
                (true, true) => crate::config::WatchlistTarget::Both,
                (true, false) => crate::config::WatchlistTarget::AniList,
                (false, true) => crate::config::WatchlistTarget::Mal,
                (false, false) => crate::config::WatchlistTarget::Local,
            },
        };
        let default_cursor = match mode {
            WatchlistPromptMode::RemoveShared => 5,
            WatchlistPromptMode::Add => current_status
                .map(|st| st.picker_index())
                .unwrap_or(1),
        };
        self.watchlist_target_prompt = Some(WatchlistTargetPrompt {
            anime,
            mode,
            cursor: default_cursor,
            target: default_target,
            current_status,
            current_source,
            from_detail,
        });
    }

    /// Compatibility wrapper opening the Status & Watchlist Target Picker popup.
    pub fn open_watchlist_prompt(&mut self, anime: Anime, mode: WatchlistPromptMode, from_detail: bool) {
        self.open_watchlist_status_picker(anime, mode, None, None, from_detail);
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

    /// Open the Tailored Algorithm results shelf with the scored candidate pool and live top-30 view.
    pub fn open_tailored_results(&mut self, pool: Vec<crate::services::tailored::TailoredMatch>) {
        self.tuner_available_tags = crate::services::tailored::extract_tuner_tags(&pool, &self.tailored_answers);
        self.tailored_pool = pool;
        self.tuner_config.reset();
        self.tuner_open = false;
        self.tuner_row = 0;
        self.tuner_col = 0;
        self.recompute_tailored_results();
        self.tailored_results_cursor = 0;
        self.is_generating_tailored = false;
        self.screen = Screen::TailoredResults;
    }

    /// Re-filter and re-rank `tailored_results` (up to 30 matches) instantly from `tailored_pool` + `tuner_config`.
    pub fn recompute_tailored_results(&mut self) {
        self.tailored_results = crate::services::tailored::apply_live_tuner(
            &self.tailored_pool,
            &self.tailored_answers,
            &self.tuner_config,
        );
        if self.tailored_results.is_empty() {
            self.tailored_results_cursor = 0;
            self.cover_anime_id = None;
        } else if self.tailored_results_cursor >= self.tailored_results.len() {
            self.tailored_results_cursor = self.tailored_results.len() - 1;
        }
        if let Some(focused) = self.tailored_results.get(self.tailored_results_cursor) {
            self.cover_anime_id = Some(focused.anime.id);
        }
    }

    /// Toggle the compact Live Vibe Tuner bar above the recommendations shelf (`[f]`).
    pub fn toggle_live_tuner(&mut self) {
        self.tuner_open = !self.tuner_open;
        self.tuner_row = 0;
        self.tuner_col = 0;
    }

    /// Open the Seasonal & Movie Release Calendar popup (`[c]`), snapping to Today's day on first open.
    pub fn open_calendar(&mut self, tz_offset_secs: i64, now_unix: i64) {
        if self.screen != Screen::Calendar {
            self.overlay_base = self.current_base_screen();
            self.calendar_tz_offset_secs = tz_offset_secs;
            self.calendar_today_unix = now_unix;
            self.calendar_week_offset = 0;
            self.calendar_day_idx = crate::services::calendar::today_weekday_idx(now_unix, tz_offset_secs);
            self.calendar_cursor = 0;
            self.screen = Screen::Calendar;
        }
    }

    /// Open the Multi-User "Who's Watching?" Profile Switcher modal (`[P]`).
    pub fn open_profile_modal(&mut self, summaries: Vec<crate::db::user::ProfileSummary>) {
        if self.screen != Screen::ProfileModal {
            self.overlay_base = self.current_base_screen();
        }
        self.profile_cursor = summaries.iter().position(|s| s.is_active).unwrap_or(0);
        self.profiles_list = summaries;
        self.profile_input_mode = ProfileInputMode::None;
        self.profile_name_input.clear();
        self.profile_avatar_cursor = 0;
        self.profile_audio_dub = false;
        self.screen = Screen::ProfileModal;
    }

    /// Open the playback query picker for the current anime.
    #[allow(dead_code)]
    pub fn open_playback_query_picker(&mut self, anime: &Anime) {
        self.playback_queries = anime.playback_queries();
        self.playback_query_cursor = 0;
        self.pending_playback_query = None;
        self.screen = Screen::PlaybackQuery;
    }

    /// Returns ALL unified `libmpv` players so users can choose freely across the full roster.
    pub fn host_players() -> Vec<crate::config::Player> {
        crate::config::Player::ALL.to_vec()
    }

    /// Open the playback options overlay after a query has been chosen.
    pub fn open_playback_options(&mut self, query: String, quality_idx: usize, dub: bool, player: crate::config::Player) {
        self.bump_playback_request_generation();
        self.playback_options_purpose = PlaybackOptionsPurpose::LaunchNow;
        self.pending_playback_query = Some(query);
        self.playback_qualities.clear();
        self.playback_quality_cursor = quality_idx;
        self.pending_dub = dub;
        self.playback_sub_cursor = 0;
        self.playback_players = Self::host_players();
        self.playback_player_cursor = self.playback_players.iter().position(|p| *p == player).unwrap_or(0);
        self.playback_providers.clear();
        self.dub_available = None;
        self.playback_provider_cursor = 0;
        self.playback_focus_column = 0;
        self.playback_sub_focused = false;
        self.is_loading_subs = true;
        self.pending_master_url = None;
        self.playback_audio_tracks.clear();
        self.playback_sub_languages = vec![
            crate::api::subtitles::SubtitleOption::none_option(),
            crate::api::subtitles::SubtitleOption::default_english(),
        ];
        self.playback_sub_cursor = if self.preferred_subtitle_code.eq_ignore_ascii_case("none")
            || (dub && self.preferred_subtitle_code.trim().is_empty())
        {
            0
        } else {
            1
        };
        self.screen = Screen::PlaybackOptions;
    }

    /// Return the currently selected media player.
    pub fn selected_player(&self) -> crate::config::Player {
        self.playback_players.get(self.playback_player_cursor).copied().unwrap_or_default()
    }

    /// Cycle through available media players (e.g. mpv -> iina -> vlc -> mpv).
    pub fn cycle_playback_player(&mut self) {
        if self.playback_players.is_empty() {
            return;
        }
        let total = self.playback_players.len();
        self.playback_player_cursor = (self.playback_player_cursor + 1) % total;
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

            if prov.embedded_sub.has_baked_english() {
                self.playback_sub_languages =
                    vec![crate::api::subtitles::SubtitleOption::none_option()];
                self.playback_sub_cursor = 0;
            } else if !prov.subtitles.is_empty() {
                self.playback_sub_languages = prov.subtitles.clone();
                crate::api::subtitles::SubtitleOption::ensure_none_option_first(
                    &mut self.playback_sub_languages,
                );
                if let Some(ref code) = curr_sub_code {
                    self.playback_sub_cursor = self
                        .playback_sub_languages
                        .iter()
                        .position(|s| {
                            s.code.eq_ignore_ascii_case(code)
                                || (code.to_ascii_lowercase().starts_with("ar")
                                    && s.code.to_ascii_lowercase().starts_with("ar"))
                        })
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

    /// Check playback logs for known ani-cli or player error messages.
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
            if log.contains("Player exited with error:") || log.contains("Player wait error:") {
                return Some("Player failed to launch or stream expired — try another provider (HD-1 / Pahe / Gogo).");
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

    /// Start a startup cover preloading batch behind the loading screen.
    pub fn begin_preload(&mut self, total: usize) -> u64 {
        self.preload_generation = self.preload_generation.wrapping_add(1);
        self.preload_total = total;
        self.preload_done = 0;
        self.is_loading = total > 0;
        self.preload_generation
    }

    /// Record completion of one preloaded cover. Returns `true` when this completion unlocks Home.
    pub fn advance_preload(&mut self, gen: u64) -> bool {
        if gen != self.preload_generation {
            return false;
        }
        self.preload_done = (self.preload_done + 1).min(self.preload_total);
        if self.is_loading && self.preload_done >= self.preload_total {
            self.is_loading = false;
            return true;
        }
        false
    }

    /// Exit the startup loading screen immediately while allowing background covers to finish caching.
    pub fn skip_preload(&mut self) {
        self.preload_generation = self.preload_generation.wrapping_add(1);
        self.is_loading = false;
    }

    /// Return the base screen that should remain visible under overlays.
    pub fn current_base_screen(&self) -> Screen {
        match self.screen {
            Screen::Detail | Screen::Playback | Screen::PlaybackQuery | Screen::PlaybackOptions => {
                Screen::Detail
            }
            Screen::Search | Screen::Help | Screen::Settings | Screen::Setup | Screen::AniListLogin | Screen::TailoredWizard | Screen::TailoredSummary | Screen::TailoredResults | Screen::TasteAnalytics | Screen::RemoteModal | Screen::CastModal | Screen::UpdateModal | Screen::Calendar | Screen::ProfileModal | Screen::QueueModal => self.overlay_base.clone(),
            Screen::CastDeck => Screen::Detail,
            Screen::Home => Screen::Home,
        }
    }

    /// Update the cached dependency status.
    pub fn set_dependencies(&mut self, ani_cli: bool, mpv: bool, mpvnet: bool, celluloid: bool) {
        self.has_ani_cli = ani_cli;
        self.has_mpv = mpv;
        self.has_mpvnet = mpvnet;
        self.has_celluloid = celluloid;
    }

    /// True when at least one `libmpv`-compatible engine is installed.
    pub fn has_any_player(&self) -> bool {
        self.has_mpv || self.has_mpvnet || self.has_celluloid
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
            synonyms:      "[]".into(),
            cover_url:     None,
            cover_blob:    None,
            has_dub:       0,
            updated_at:    0,
            next_airing_episode: None,
            mal_id: None,
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
    fn test_open_and_close_exit_confirm_defaults_to_cancel() {
        let mut state = AppState::new();
        assert!(!state.exit_confirm_open);
        state.open_exit_confirm();
        assert!(state.exit_confirm_open);
        assert_eq!(state.exit_confirm_cursor, 0);
        assert!(!state.should_quit);
        state.close_exit_confirm();
        assert!(!state.exit_confirm_open);
        assert_eq!(state.exit_confirm_cursor, 0);
        assert!(!state.should_quit);
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
        assert_eq!(state.playback_providers.len(), 3);

        // Initially on Provider 0 (ZokoAnime)
        state.playback_provider_cursor = 0;
        state.sync_active_provider();
        assert_eq!(state.playback_qualities[0].label, "1080p");

        // Move to Provider 2 (AnimeGG Direct)
        state.playback_provider_cursor = 2;
        state.sync_active_provider();
        assert_eq!(state.playback_qualities[0].label, "1080p");
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
        assert_eq!(state.selected_player(), crate::config::Player::AutoBest);
        state.playback_player_cursor = 1;
        assert_eq!(state.selected_player(), crate::config::Player::Mpv);
        #[cfg(windows)]
        {
            state.playback_player_cursor = 2;
            assert_eq!(state.selected_player(), crate::config::Player::MpvNet);
        }
        #[cfg(target_os = "linux")]
        {
            state.playback_player_cursor = 2;
            assert_eq!(state.selected_player(), crate::config::Player::Celluloid);
        }
    }

    #[test]
    fn test_cycle_playback_player() {
        let mut state = AppState::new();
        let initial = state.selected_player();
        state.cycle_playback_player();
        assert_ne!(state.selected_player(), initial);
        for _ in 1..state.playback_players.len() {
            state.cycle_playback_player();
        }
        assert_eq!(state.selected_player(), initial);
    }

    #[test]
    fn test_taste_analytics_corrupted_overlay_base_escapes_to_home() {
        let mut state = AppState::new();
        state.screen = Screen::TasteAnalytics;
        state.overlay_base = Screen::TasteAnalytics;

        state.go_back();
        assert_eq!(state.screen, Screen::Home);
    }

    #[test]
    fn test_update_modal_flow_and_go_back() {
        let mut state = AppState::new();
        state.screen = Screen::Home;

        let info = crate::services::updater::UpdateInfo {
            current_version: "1.0.6".to_string(),
            latest_version: "1.0.7".to_string(),
            download_url: "https://github.com/example/solus".to_string(),
            asset_name: "solus-windows-x86_64.exe".to_string(),
            release_notes: Some("Big launch speed upgrade".to_string()),
            simple_notes: vec!["[IMPROVED] Faster startup.".to_string()],
            is_homebrew: false,
        };

        state.open_update_modal(info.clone());
        assert_eq!(state.screen, Screen::UpdateModal);
        assert_eq!(state.overlay_base, Screen::Home);
        assert_eq!(state.available_update, Some(info.clone()));
        assert_eq!(
            state.update_modal_phase,
            Some(UpdateModalPhase::Available { info })
        );

        state.close_update_modal();
        assert_eq!(state.screen, Screen::Home);
        assert_eq!(state.update_modal_phase, None);
    }

    #[test]
    fn test_preload_state_progress_and_skip() {
        let mut state = AppState::new();
        let gen1 = state.begin_preload(2);
        assert!(state.is_loading);
        assert_eq!(state.preload_total, 2);
        assert_eq!(state.preload_done, 0);

        // Stale generation completions do not advance progress.
        assert!(!state.advance_preload(gen1 + 99));
        assert_eq!(state.preload_done, 0);

        assert!(!state.advance_preload(gen1));
        assert!(state.is_loading);
        assert_eq!(state.preload_done, 1);

        // Final item unlocks Home.
        assert!(state.advance_preload(gen1));
        assert!(!state.is_loading);
        assert_eq!(state.preload_done, 2);

        // Skipping early clears is_loading and invalidates the active generation gate.
        let gen2 = state.begin_preload(3);
        assert!(state.is_loading);
        state.skip_preload();
        assert!(!state.is_loading);
        assert!(!state.advance_preload(gen2));
    }

    #[test]
    fn test_has_image_support_enables_halfblocks_for_alacritty_and_zellij() {
        let mut state = AppState::new();
        assert!(!state.has_image_support());

        let mut picker = Picker::new((10, 20));
        picker.protocol_type = ratatui_image::picker::ProtocolType::Halfblocks;
        state.picker = Some(picker);
        assert!(state.has_image_support());
    }

    #[test]
    fn test_resolve_cover_protocol_caches_per_card_dimensions_without_right_gap() {
        let mut state = AppState::new();
        let mut picker = Picker::new((12, 24));
        picker.protocol_type = ratatui_image::picker::ProtocolType::Halfblocks;
        state.picker = Some(picker);
        state.cover_raw_cache.insert(
            77,
            image::DynamicImage::ImageRgb8(image::RgbImage::new(400, 580)),
        );

        assert!(state.resolve_cover_protocol(77, 16, 11, 0.20, true).is_some());
        assert!(state.resolve_cover_protocol(77, 19, 11, 0.20, true).is_some());
        assert!(state.fitted_cover_cache.contains_key(&(77, 16, 11)));
        assert!(state.fitted_cover_cache.contains_key(&(77, 19, 11)));
    }

    #[test]
    fn test_playback_request_and_metadata_generation_isolation() {
        assert!(ThumbnailSource::AniZip > ThumbnailSource::AniListTitle);
        assert!(ThumbnailSource::AniListTitle > ThumbnailSource::Kitsu);
        assert!(ThumbnailSource::Kitsu > ThumbnailSource::AniListIndexFallback);
        assert!(ThumbnailSource::AniListIndexFallback > ThumbnailSource::Fallback);

        let mut state = AppState::new();
        let anime = crate::db::cache::Anime {
            id: 11061,
            title_romaji: "Hunter x Hunter (2011)".to_string(),
            episodes: Some(148),
            ..Default::default()
        };

        state.open_detail(anime);
        let gen1 = state.episode_metadata_generation;
        assert!(gen1 > 0);

        state.open_playback_options(
            "Hunter x Hunter (2011)".to_string(),
            0,
            true,
            crate::config::Player::Mpv,
        );
        let pb_gen1 = state.playback_request_generation;
        assert!(pb_gen1 > 0);
        // Optimistically seeded with "No subtitles" first and Dub default at index 0.
        assert!(!state.playback_sub_languages.is_empty());
        assert!(state.playback_sub_languages[0].is_none());
        assert_eq!(state.playback_sub_cursor, 0);

        let pb_gen2 = state.bump_playback_request_generation();
        assert!(pb_gen2 > pb_gen1);

        // Provider sync preserves "No subtitles" ahead of provider tracks.
        state.playback_providers = vec![crate::api::subtitles::ProviderOption {
            id: "zoko".into(),
            name: "Zoko".into(),
            server_label: "Zoko".into(),
            supports_sub: true,
            supports_dub: true,
            is_verified: true,
            verified_res: Some("1080p".into()),
            master_url: None,
            qualities: vec![],
            subtitles: vec![crate::api::subtitles::SubtitleOption {
                name: "English".into(),
                code: "eng".into(),
                url: "https://example.com/en.vtt".into(),
                stream_label: "Zoko".into(),
            }],
            referer: None,
            needs_hls_proxy: false,
            embedded_sub: crate::api::subtitles::EmbeddedSubtitleKind::None,
        }];
        state.playback_provider_cursor = 0;
        state.sync_active_provider();
        assert_eq!(state.playback_sub_languages.len(), 2);
        assert!(state.playback_sub_languages[0].is_none());
        assert_eq!(state.playback_sub_languages[1].code, "eng");

        // Opening another anime bumps episode metadata generation & clears stale raw thumbnails.
        let other = crate::db::cache::Anime {
            id: 21,
            title_romaji: "ONE PIECE".to_string(),
            episodes: Some(1000),
            ..Default::default()
        };
        state.episode_raw_thumbnails.insert(
            13,
            image::DynamicImage::ImageRgb8(image::RgbImage::new(16, 9)),
        );
        state.thumbnail_geometry_key = Some((32, 9, 12, 24));
        state.invalidate_detail_image_protocols();
        assert_eq!(state.episode_raw_thumbnails.len(), 1);
        assert!(state.thumbnail_geometry_key.is_none());

        state.open_detail(other);
        assert!(state.episode_metadata_generation > gen1);
        assert!(state.episode_raw_thumbnails.is_empty());
    }
}
