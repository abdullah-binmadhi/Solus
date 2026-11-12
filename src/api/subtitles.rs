//! Subtitle and video stream detection module.
//! Dynamically fetches verified stream resolutions, master playlists, and subtitle tracks for a specific anime episode.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const XOR_KEY: [u8; 14] = [111, 116, 97, 107, 117, 45, 101, 109, 98, 101, 100, 45, 118, 49]; // "otaku-embed-v1"
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityOption {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleOption {
    pub code: String,
    pub name: String,
    pub stream_label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub id: String,
    pub name: String,
    pub server_label: String,
    pub master_url: Option<String>,
}

pub fn default_providers() -> Vec<ProviderOption> {
    vec![
        ProviderOption {
            id: "zoko".to_string(),
            name: "ZokoAnime".to_string(),
            server_label: "ZokoAnime [Direct 1080p]".to_string(),
            master_url: None,
        },
        ProviderOption {
            id: "hd1".to_string(),
            name: "HD-1".to_string(),
            server_label: "HD-1 [MegaPlay Cloud]".to_string(),
            master_url: None,
        },
        ProviderOption {
            id: "pahe".to_string(),
            name: "AnimePahe".to_string(),
            server_label: "AnimePahe [ani-cli]".to_string(),
            master_url: None,
        },
        ProviderOption {
            id: "gogo".to_string(),
            name: "GogoAnime".to_string(),
            server_label: "GogoAnime [ani-cli]".to_string(),
            master_url: None,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub master_url: Option<String>,
    pub qualities: Vec<QualityOption>,
    pub subtitles: Vec<SubtitleOption>,
    pub providers: Vec<ProviderOption>,
}

impl StreamInfo {
    pub fn fallback() -> Self {
        Self {
            master_url: None,
            qualities: vec![
                QualityOption { label: "1080p".to_string(), url: String::new() },
                QualityOption { label: "720p".to_string(), url: String::new() },
            ],
            subtitles: vec![SubtitleOption::default_english()],
            providers: default_providers(),
        }
    }
}

impl SubtitleOption {
    pub fn default_english() -> Self {
        Self {
            code: "eng".to_string(),
            name: "English".to_string(),
            stream_label: "English".to_string(),
            url: String::new(),
        }
    }

    /// Extract a clean keyword suitable for ani-cli's ANI_CLI_SUB_LANG matching
    pub fn lang_keyword(&self) -> &str {
        if self.code.starts_with("ara") {
            "Arabic"
        } else if self.code.starts_with("rus") {
            "Russian"
        } else if self.code.starts_with("hin") {
            "Hindi"
        } else if self.code.starts_with("ind") {
            "Indonesian"
        } else if self.code.starts_with("urd") {
            "Urdu"
        } else if self.code.starts_with("spa") {
            "Spanish"
        } else if self.code.starts_with("fra") {
            "French"
        } else if self.code.starts_with("ger") || self.code.starts_with("deu") {
            "German"
        } else if self.code.starts_with("ita") {
            "Italian"
        } else if self.code.starts_with("por") {
            "Portuguese"
        } else if self.code.starts_with("tur") {
            "Turkish"
        } else if self.code.starts_with("vie") {
            "Vietnamese"
        } else if self.code.starts_with("jpn") {
            "Japanese"
        } else if self.code.starts_with("zho") || self.code.starts_with("chi") {
            "Chinese"
        } else if self.code.starts_with("kor") {
            "Korean"
        } else if self.code.starts_with("pol") {
            "Polish"
        } else if self.code.starts_with("tha") {
            "Thai"
        } else if self.code.starts_with("nld") || self.code.starts_with("dut") {
            "Dutch"
        } else if self.code.starts_with("swe") {
            "Swedish"
        } else if self.code.starts_with("ell") || self.code.starts_with("gre") {
            "Greek"
        } else if self.code.starts_with("heb") {
            "Hebrew"
        } else if self.code.starts_with("fas") || self.code.starts_with("per") {
            "Persian"
        } else if self.code.starts_with("fil") || self.code.starts_with("tgl") {
            "Tagalog"
        } else if self.code.starts_with("ukr") {
            "Ukrainian"
        } else if self.code.starts_with("ces") || self.code.starts_with("cze") {
            "Czech"
        } else if self.code.starts_with("hun") {
            "Hungarian"
        } else if self.code.starts_with("ron") || self.code.starts_with("rum") {
            "Romanian"
        } else if self.code.starts_with("dan") {
            "Danish"
        } else if self.code.starts_with("fin") {
            "Finnish"
        } else if self.code.starts_with("nor") {
            "Norwegian"
        } else if self.code.starts_with("msa") || self.code.starts_with("may") {
            "Malay"
        } else if self.code.starts_with("eng") {
            "English"
        } else {
            // First alphabetic word from stream_label
            self.stream_label
                .split(|c: char| !c.is_alphabetic())
                .next()
                .filter(|w| !w.is_empty())
                .unwrap_or("English")
        }
    }
}

/// Prioritize core languages at the top of the subtitle track list:
/// 1. English
/// 2. Arabic
/// 3. Russian
/// 4. Indonesian (Bahasa Indonesia)
/// 5. Remaining languages in original order
pub fn prioritize_subtitles(subtitles: &mut Vec<SubtitleOption>) {
    let lang_priority = |code: &str| -> usize {
        if code.starts_with("eng") {
            0
        } else if code.starts_with("ara") {
            1
        } else if code.starts_with("rus") {
            2
        } else if code.starts_with("ind") {
            3
        } else {
            4
        }
    };

    subtitles.sort_by_key(|s| lang_priority(&s.code));
}

/// Parse actual available video resolutions from an HLS master playlist.
pub fn parse_m3u8_qualities(master_url: &str, m3u8_content: &str) -> Vec<QualityOption> {
    let mut raw_qualities: Vec<(u32, QualityOption)> = Vec::new();
    let base_url = match master_url.rfind('/') {
        Some(idx) => &master_url[..idx],
        None => master_url,
    };

    let lines: Vec<&str> = m3u8_content.lines().map(|l| l.trim()).collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let height = line
                .split(',')
                .find(|part| part.contains("RESOLUTION="))
                .and_then(|res| res.split('=').nth(1))
                .and_then(|dim| dim.split('x').nth(1))
                .and_then(|h| h.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().ok());

            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with('#') || lines[j].is_empty()) {
                j += 1;
            }

            if j < lines.len() {
                let stream_path = lines[j];
                let full_url = if stream_path.starts_with("http://") || stream_path.starts_with("https://") {
                    stream_path.to_string()
                } else {
                    format!("{}/{}", base_url, stream_path)
                };

                let h = height.unwrap_or(1080);
                raw_qualities.push((h, QualityOption {
                    label: format!("{}p", h),
                    url: full_url,
                }));
            }
        }
        i += 1;
    }

    raw_qualities.sort_by(|a, b| b.0.cmp(&a.0));

    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (_, opt) in raw_qualities {
        if seen.insert(opt.label.clone()) {
            result.push(opt);
        }
    }

    if result.is_empty() && !master_url.is_empty() {
        result.push(QualityOption {
            label: "1080p".to_string(),
            url: master_url.to_string(),
        });
    }

    result
}

/// Dynamically fetch verified stream info (qualities, subtitles, and direct master URL)
/// for a specific anime episode. Automatically queries and merges community multi-language
/// subtitle tracks (Arabic, Spanish, French, German, Italian, etc.) if not already present in the stream.
pub async fn fetch_stream_info(
    anilist_id: i64,
    query_title: &str,
    episode: u32,
    is_dub: bool,
) -> StreamInfo {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    // Run stream resolution and external multi-language subtitle fetching in parallel!
    let (stream_res, ext_subs) = tokio::join!(
        resolve_primary_stream(&client, anilist_id, query_title, episode, is_dub),
        fetch_external_subtitles(&client, anilist_id, query_title, episode)
    );

    let mut info = stream_res.unwrap_or_else(StreamInfo::fallback);

    // Merge external community subtitles (OpenSubtitles) into the stream's subtitle list
    let mut existing_codes: HashSet<String> = info.subtitles.iter().map(|s| s.code.clone()).collect();
    for ext in ext_subs {
        if !existing_codes.contains(&ext.code) {
            existing_codes.insert(ext.code.clone());
            info.subtitles.push(ext);
        }
    }

    // Prioritize English, Arabic, Russian, and Bahasa Indonesia at the top of the track list
    prioritize_subtitles(&mut info.subtitles);

    info
}

async fn resolve_primary_stream(
    client: &reqwest::Client,
    anilist_id: i64,
    query_title: &str,
    episode: u32,
    is_dub: bool,
) -> Option<StreamInfo> {
    // 1. First attempt: resolve MAL ID via AniList GraphQL
    if let Some(mal_id) = fetch_mal_id(client, anilist_id).await {
        if let Ok(info) = fetch_stream_for_mal(client, mal_id, episode, is_dub).await {
            if !info.subtitles.is_empty() || !info.qualities.is_empty() {
                return Some(info);
            }
        }
    }

    // 2. Fallback attempt: resolve via HiAnime direct search
    if let Ok(info) = fetch_stream_via_hianime(client, query_title, episode, is_dub).await {
        if !info.subtitles.is_empty() || !info.qualities.is_empty() {
            return Some(info);
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct AniZipMappingData {
    mappings: Option<AniZipMappingDetails>,
    #[serde(default)]
    titles: HashMap<String, String>,
    #[serde(default)]
    episodes: HashMap<String, AniZipEpDetails>,
}

#[derive(Debug, Deserialize)]
struct AniZipMappingDetails {
    imdb_id: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniZipEpDetails {
    season_number: Option<u32>,
    episode_number: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct StremioSubtitlesPayload {
    #[serde(default)]
    subtitles: Vec<StremioSubtitleItem>,
}

#[derive(Debug, Deserialize)]
struct StremioSubtitleItem {
    url: Option<String>,
    lang: Option<String>,
    #[serde(rename = "subtitleFileName")]
    subtitle_file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenSubtitlesItem {
    #[serde(rename = "SubLanguageID")]
    sub_language_id: Option<String>,
    #[serde(rename = "SeriesEpisode")]
    series_episode: Option<String>,
    #[serde(rename = "SubFileName")]
    sub_file_name: Option<String>,
    #[serde(rename = "SubDownloadLink")]
    sub_download_link: Option<String>,
}

/// Matches an episode number against SeriesEpisode tag or common anime fansub filename conventions.
pub fn matches_episode(series_ep: &str, filename: &str, target_ep: u32) -> bool {
    if let Ok(ep) = series_ep.trim().parse::<u32>() {
        if ep == target_ep {
            return true;
        }
    }

    let lower = filename.to_lowercase();
    let target_str = target_ep.to_string();
    let target_pad = format!("{:02}", target_ep);

    let patterns = [
        format!("e{}", target_str),
        format!("e{}", target_pad),
        format!("ep{}", target_str),
        format!("ep{}", target_pad),
        format!("ep.{}", target_str),
        format!("ep.{}", target_pad),
        format!("ep {}", target_str),
        format!("ep {}", target_pad),
        format!("episode {}", target_str),
        format!("episode {}", target_pad),
        format!("layer {}", target_str),
        format!("layer {}", target_pad),
        format!("layer{}", target_pad),
        format!(" - {}", target_str),
        format!(" - {}", target_pad),
        format!("-{}", target_pad),
        format!(" {}", target_pad),
        format!("_{}_", target_pad),
        format!("_{}", target_pad),
    ];

    for p in &patterns {
        if let Some(pos) = lower.find(p) {
            let end = pos + p.len();
            if end >= lower.len() || !lower.as_bytes()[end].is_ascii_digit() {
                return true;
            }
        }
    }

    false
}

/// Fetch community multi-language subtitle tracks for an anime episode via AniZip mapping,
/// Stremio OpenSubtitles v3 addon, and direct OpenSubtitles REST search across English and Romaji titles.
pub async fn fetch_external_subtitles(
    client: &reqwest::Client,
    anilist_id: i64,
    query_title: &str,
    episode: u32,
) -> Vec<SubtitleOption> {
    let anizip_url = format!("https://api.ani.zip/mappings?anilist_id={}", anilist_id);
    let Ok(resp) = client.get(&anizip_url).timeout(Duration::from_secs(4)).send().await else {
        return Vec::new();
    };
    let Ok(parsed) = resp.json::<AniZipMappingData>().await else {
        return Vec::new();
    };

    let mut options = Vec::new();
    let mut seen = HashSet::new();

    // 1. Stremio OpenSubtitles v3 addon via IMDb ID (if available)
    if let Some(imdb_id) = parsed.mappings.as_ref().and_then(|m| m.imdb_id.clone()).filter(|id| !id.is_empty()) {
        let is_movie = parsed
            .mappings
            .as_ref()
            .and_then(|m| m.media_type.as_deref())
            .map(|t| t.eq_ignore_ascii_case("movie"))
            .unwrap_or(false);

        let query_url = if is_movie {
            format!("https://opensubtitles-v3.strem.io/subtitles/movie/{}.json", imdb_id)
        } else {
            let ep_key = episode.to_string();
            let (season, ep_num) = if let Some(ep_info) = parsed.episodes.get(&ep_key) {
                (ep_info.season_number.unwrap_or(1), ep_info.episode_number.unwrap_or(episode))
            } else {
                (1, episode)
            };
            format!("https://opensubtitles-v3.strem.io/subtitles/series/{}:{}:{}.json", imdb_id, season, ep_num)
        };

        if let Ok(sub_resp) = client.get(&query_url).timeout(Duration::from_secs(4)).send().await {
            if let Ok(sub_data) = sub_resp.json::<StremioSubtitlesPayload>().await {
                for item in sub_data.subtitles {
                    let Some(url) = item.url.filter(|u| !u.is_empty()) else {
                        continue;
                    };
                    let lang = item.lang.as_deref().unwrap_or("eng");
                    let label = item.subtitle_file_name.as_deref().unwrap_or(lang);
                    let (code, name) = resolve_lang_meta(label, lang);

                    if seen.insert(code.clone()) {
                        options.push(SubtitleOption {
                            code,
                            name: name.clone(),
                            stream_label: format!("{} [Auto-Synced]", name),
                            url,
                        });
                    }
                }
            }
        }
    }

    // 2. OpenSubtitles REST search API for Arabic, Russian, Indonesian, and wider community fansubs
    let mut search_titles = Vec::new();
    if !query_title.is_empty() {
        search_titles.push(query_title.to_string());
    }
    if let Some(en) = parsed.titles.get("en").filter(|s| !s.is_empty()) {
        if !search_titles.contains(en) {
            search_titles.push(en.clone());
        }
    }
    if let Some(xjat) = parsed.titles.get("x-jat").filter(|s| !s.is_empty()) {
        if !search_titles.contains(xjat) {
            search_titles.push(xjat.clone());
        }
    }

    for title in search_titles {
        for lang_filter in ["sublanguageid-ara", "sublanguageid-rus", "sublanguageid-ind", ""] {
            let query_path = if lang_filter.is_empty() {
                format!("https://rest.opensubtitles.org/search/query-{}", urlencoding::encode(&title))
            } else {
                format!("https://rest.opensubtitles.org/search/{}/query-{}", lang_filter, urlencoding::encode(&title))
            };

            let req = client
                .get(&query_path)
                .header("User-Agent", "TemporaryUserAgent")
                .timeout(Duration::from_secs(3));

            if let Ok(resp) = req.send().await {
                if let Ok(items) = resp.json::<Vec<OpenSubtitlesItem>>().await {
                    for it in items {
                        let sep = it.series_episode.as_deref().unwrap_or("");
                        let fn_str = it.sub_file_name.as_deref().unwrap_or("");
                        let Some(dl_url) = it.sub_download_link.filter(|u| !u.is_empty()) else {
                            continue;
                        };

                        if matches_episode(sep, fn_str, episode) {
                            let lang_id = it.sub_language_id.as_deref().unwrap_or("eng");
                            let (code, name) = resolve_lang_meta(fn_str, lang_id);

                            if seen.insert(code.clone()) {
                                options.push(SubtitleOption {
                                    code,
                                    name: name.clone(),
                                    stream_label: format!("{} [Auto-Synced]", name),
                                    url: dl_url,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    prioritize_subtitles(&mut options);
    options
}

/// Download and cache a remote subtitle URL to ~/.cache/solus/subs/ and return the local path.
/// If the subtitle is already local or download fails, returns the input URL.
#[allow(dead_code)]
pub async fn ensure_cached_subtitle(
    client: &reqwest::Client,
    sub_url: &str,
    anime_id: i64,
    episode: u32,
    code: &str,
) -> String {
    if !sub_url.starts_with("http://") && !sub_url.starts_with("https://") {
        return sub_url.to_string();
    }

    let cache_dir = directories::ProjectDirs::from("", "", "ani-tui")
        .map(|dirs| dirs.cache_dir().join("subs"))
        .unwrap_or_else(|| std::env::temp_dir().join("solus-subs"));

    if std::fs::create_dir_all(&cache_dir).is_err() {
        return sub_url.to_string();
    }

    let clean_code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let ext = if sub_url.contains(".vtt") { "vtt" } else { "srt" };
    let filename = format!("{}_{}_{}.{}", anime_id, episode, if clean_code.is_empty() { "sub" } else { &clean_code }, ext);
    let file_path = cache_dir.join(&filename);

    if file_path.is_file() {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if metadata.len() > 0 {
                return file_path.to_string_lossy().into_owned();
            }
        }
    }

    if let Ok(resp) = client.get(sub_url).timeout(Duration::from_secs(6)).send().await {
        if let Ok(bytes) = resp.bytes().await {
            if !bytes.is_empty() {
                if std::fs::write(&file_path, &bytes).is_ok() {
                    return file_path.to_string_lossy().into_owned();
                }
            }
        }
    }

    sub_url.to_string()
}

/// Backwards compatibility wrapper for subtitle detection.
#[allow(dead_code)]
pub async fn fetch_available_subtitles(
    anilist_id: i64,
    query_title: &str,
    episode: u32,
) -> Vec<SubtitleOption> {
    fetch_stream_info(anilist_id, query_title, episode, false).await.subtitles
}

/// Query AniList GraphQL for MyAnimeList ID (idMal)
async fn fetch_mal_id(client: &reqwest::Client, anilist_id: i64) -> Option<i64> {
    let query = r#"query($id: Int) { Media(id: $id) { idMal } }"#;
    let body = serde_json::json!({
        "query": query,
        "variables": { "id": anilist_id }
    });

    let resp = client
        .post("https://graphql.anilist.co")
        .json(&body)
        .send()
        .await
        .ok()?;

    let json: serde_json::Value = resp.json().await.ok()?;
    json.pointer("/data/Media/idMal")?.as_i64()
}

/// Fetch and parse stream info using MAL ID directly on ZokoAnime embed
async fn fetch_stream_for_mal(
    client: &reqwest::Client,
    mal_id: i64,
    episode: u32,
    is_dub: bool,
) -> Result<StreamInfo, ()> {
    let mode_str = if is_dub { "dub" } else { "sub" };
    let url = format!("https://zokoanime.video/stream/mal/{}/{}/{}", mal_id, episode, mode_str);
    let resp = client.get(&url).send().await.map_err(|_| ())?;
    let html = resp.text().await.map_err(|_| ())?;
    parse_stream_embed(client, &html).await
}

/// Fallback: search HiAnime, find episode ID and embed
async fn fetch_stream_via_hianime(
    client: &reqwest::Client,
    query: &str,
    episode: u32,
    is_dub: bool,
) -> Result<StreamInfo, ()> {
    // 1. Search anime slug
    let search_url = format!("https://hianime.at/search?keyword={}", urlencoding::encode(query));
    let resp = client.get(&search_url).send().await.map_err(|_| ())?;
    let search_html = resp.text().await.map_err(|_| ())?;

    let anime_id = extract_anime_id(&search_html).ok_or(())?;

    // 2. Fetch episode list
    let ep_url = format!("https://hianime.at/api/theme/episode/list/{}", anime_id);
    let ep_resp = client.get(&ep_url).send().await.map_err(|_| ())?;
    let ep_json: serde_json::Value = ep_resp.json().await.map_err(|_| ())?;
    let ep_html = ep_json.get("html").and_then(|v| v.as_str()).unwrap_or("");

    let ep_id = extract_episode_id(ep_html, episode).ok_or(())?;

    // 3. Fetch servers
    let srv_url = format!("https://hianime.at/api/theme/episode/servers?episodeId={}", ep_id);
    let srv_resp = client.get(&srv_url).send().await.map_err(|_| ())?;
    let srv_json: serde_json::Value = srv_resp.json().await.map_err(|_| ())?;
    let srv_html = srv_json.get("html").and_then(|v| v.as_str()).unwrap_or("");

    let embed_url = extract_zoko_embed_url(srv_html, is_dub).ok_or(())?;

    // 4. Fetch embed HTML
    let embed_resp = client.get(&embed_url).send().await.map_err(|_| ())?;
    let embed_html = embed_resp.text().await.map_err(|_| ())?;
    parse_stream_embed(client, &embed_html).await
}

async fn parse_stream_embed(client: &reqwest::Client, html: &str) -> Result<StreamInfo, ()> {
    let marker = "window.__P=\"";
    let start = html.find(marker).ok_or(())? + marker.len();
    let rest = &html[start..];
    let end = rest.find('"').ok_or(())?;
    let b64_blob = &rest[..end];

    let json_str = deobfuscate_blob(b64_blob).ok_or(())?;
    let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|_| ())?;

    let master_url = parsed.get("src").and_then(|v| v.as_str()).map(|s| s.to_string());
    let subs = parsed.get("subtitles").and_then(|v| v.as_array());
    let subtitles = subs.map(|arr| map_subtitles(arr)).unwrap_or_else(|| vec![SubtitleOption::default_english()]);

    let mut qualities = Vec::new();
    if let Some(ref m_url) = master_url {
        if let Ok(resp) = client.get(m_url).header("Referer", "https://zokoanime.video/").send().await {
            if let Ok(text) = resp.text().await {
                qualities = parse_m3u8_qualities(m_url, &text);
            }
        }
        if qualities.is_empty() {
            qualities.push(QualityOption {
                label: "1080p".to_string(),
                url: m_url.clone(),
            });
        }
    }

    let mut providers = default_providers();
    if let Some(ref m_url) = master_url {
        if let Some(p0) = providers.get_mut(0) {
            p0.master_url = Some(m_url.clone());
        }
    }

    Ok(StreamInfo {
        master_url,
        qualities,
        subtitles,
        providers,
    })
}

fn deobfuscate_blob(blob_b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .ok()?;

    let mut out = Vec::with_capacity(bytes.len());
    for (i, b) in bytes.iter().enumerate() {
        out.push(b ^ XOR_KEY[i % XOR_KEY.len()]);
    }

    String::from_utf8(out).ok()
}

fn resolve_lang_meta(label: &str, lang: &str) -> (String, String) {
    let l_lower = label.to_lowercase();
    let lang_lower = if lang.is_empty() {
        l_lower.clone()
    } else {
        lang.to_lowercase()
    };

    // Specific regional checks first
    if l_lower.contains("latin") || l_lower.contains("latam") || l_lower.contains("es-419") || lang_lower == "es-419" {
        return ("spa-419".to_string(), "Spanish (Latin America)".to_string());
    }
    if l_lower.contains("espana") || l_lower.contains("españa") || l_lower.contains("castilian") || l_lower.contains("castellano") || l_lower.contains("spain") || lang_lower == "es-es" {
        return ("spa-es".to_string(), "Spanish (Castilian/Spain)".to_string());
    }
    if l_lower.contains("brasil") || l_lower.contains("brazil") || lang_lower == "pt-br" || lang_lower == "pob" {
        return ("por-br".to_string(), "Portuguese (Brazil)".to_string());
    }
    if l_lower.contains("portugal") || lang_lower == "pt-pt" {
        return ("por-pt".to_string(), "Portuguese (Portugal)".to_string());
    }

    // General language checks
    if l_lower.contains("arab") || l_lower.contains("العربية") || lang_lower == "ar" || lang_lower == "ara" {
        ("ara".to_string(), "Arabic (العربية)".to_string())
    } else if l_lower.contains("russ") || l_lower.contains("русский") || lang_lower == "ru" || lang_lower == "rus" {
        ("rus".to_string(), "Russian (Русский)".to_string())
    } else if l_lower.contains("hind") || l_lower.contains("हिन्दी") || lang_lower == "hi" || lang_lower == "hin" {
        ("hin".to_string(), "Hindi (हिन्दी)".to_string())
    } else if l_lower.contains("indo") || l_lower.contains("bahasa indonesia") || lang_lower == "id" || lang_lower == "ind" {
        ("ind".to_string(), "Indonesian (Bahasa Indonesia)".to_string())
    } else if l_lower.contains("urdu") || l_lower.contains("اردو") || lang_lower == "ur" || lang_lower == "urd" {
        ("urd".to_string(), "Urdu (اردو)".to_string())
    } else if l_lower.contains("engl") || lang_lower == "en" || lang_lower == "eng" {
        ("eng".to_string(), "English".to_string())
    } else if l_lower.contains("span") || l_lower.contains("espanol") || l_lower.contains("español") || lang_lower == "es" || lang_lower == "spa" {
        ("spa".to_string(), "Spanish (Español)".to_string())
    } else if l_lower.contains("fren") || l_lower.contains("francais") || l_lower.contains("français") || lang_lower == "fr" || lang_lower == "fra" || lang_lower == "fre" {
        ("fra".to_string(), "French (Français)".to_string())
    } else if l_lower.contains("germ") || l_lower.contains("deutsch") || lang_lower == "de" || lang_lower == "ger" || lang_lower == "deu" {
        ("ger".to_string(), "German (Deutsch)".to_string())
    } else if l_lower.contains("ital") || lang_lower == "it" || lang_lower == "ita" {
        ("ita".to_string(), "Italian (Italiano)".to_string())
    } else if l_lower.contains("port") || lang_lower == "pt" || lang_lower == "por" {
        ("por".to_string(), "Portuguese (Português)".to_string())
    } else if l_lower.contains("turk") || l_lower.contains("türk") || lang_lower == "tr" || lang_lower == "tur" {
        ("tur".to_string(), "Turkish (Türkçe)".to_string())
    } else if l_lower.contains("viet") || l_lower.contains("việt") || lang_lower == "vi" || lang_lower == "vie" {
        ("vie".to_string(), "Vietnamese (Tiếng Việt)".to_string())
    } else if l_lower.contains("japan") || l_lower.contains("日本語") || lang_lower == "ja" || lang_lower == "jpn" {
        ("jpn".to_string(), "Japanese (日本語)".to_string())
    } else if l_lower.contains("traditional") || lang_lower == "zht" {
        ("zho-tw".to_string(), "Chinese (Traditional / 繁體中文)".to_string())
    } else if l_lower.contains("chin") || l_lower.contains("mandarin") || l_lower.contains("中文") || lang_lower == "zh" || lang_lower == "zho" || lang_lower == "chi" {
        ("zho".to_string(), "Chinese (Simplified / 简体中文)".to_string())
    } else if l_lower.contains("kore") || l_lower.contains("한국어") || lang_lower == "ko" || lang_lower == "kor" {
        ("kor".to_string(), "Korean (한국어)".to_string())
    } else if l_lower.contains("poli") || l_lower.contains("polsk") || lang_lower == "pl" || lang_lower == "pol" {
        ("pol".to_string(), "Polish (Polski)".to_string())
    } else if l_lower.contains("thai") || l_lower.contains("ไทย") || lang_lower == "th" || lang_lower == "tha" {
        ("tha".to_string(), "Thai (ไทย)".to_string())
    } else if l_lower.contains("dutc") || l_lower.contains("neder") || lang_lower == "nl" || lang_lower == "nld" || lang_lower == "dut" {
        ("nld".to_string(), "Dutch (Nederlands)".to_string())
    } else if l_lower.contains("swed") || l_lower.contains("svensk") || lang_lower == "sv" || lang_lower == "swe" {
        ("swe".to_string(), "Swedish (Svenska)".to_string())
    } else if l_lower.contains("gree") || l_lower.contains("ελλην") || lang_lower == "el" || lang_lower == "ell" || lang_lower == "gre" {
        ("ell".to_string(), "Greek (Ελληνικά)".to_string())
    } else if l_lower.contains("hebr") || l_lower.contains("עברי") || lang_lower == "he" || lang_lower == "heb" {
        ("heb".to_string(), "Hebrew (עברית)".to_string())
    } else if l_lower.contains("pers") || l_lower.contains("fars") || l_lower.contains("فارسی") || lang_lower == "fa" || lang_lower == "fas" || lang_lower == "per" {
        ("fas".to_string(), "Persian (فارسی)".to_string())
    } else if l_lower.contains("taga") || l_lower.contains("fili") || lang_lower == "tl" || lang_lower == "tgl" || lang_lower == "fil" {
        ("fil".to_string(), "Tagalog".to_string())
    } else if l_lower.contains("ukra") || l_lower.contains("україн") || lang_lower == "uk" || lang_lower == "ukr" {
        ("ukr".to_string(), "Ukrainian (Українська)".to_string())
    } else if l_lower.contains("czec") || l_lower.contains("češ") || lang_lower == "cs" || lang_lower == "ces" || lang_lower == "cze" {
        ("ces".to_string(), "Czech (Čeština)".to_string())
    } else if l_lower.contains("hung") || l_lower.contains("magy") || lang_lower == "hu" || lang_lower == "hun" {
        ("hun".to_string(), "Hungarian (Magyar)".to_string())
    } else if l_lower.contains("roma") || l_lower.contains("româ") || lang_lower == "ro" || lang_lower == "ron" || lang_lower == "rum" {
        ("ron".to_string(), "Romanian (Română)".to_string())
    } else if l_lower.contains("dani") || l_lower.contains("dansk") || lang_lower == "da" || lang_lower == "dan" {
        ("dan".to_string(), "Danish (Dansk)".to_string())
    } else if l_lower.contains("finn") || l_lower.contains("suom") || lang_lower == "fi" || lang_lower == "fin" {
        ("fin".to_string(), "Finnish (Suomi)".to_string())
    } else if l_lower.contains("norw") || l_lower.contains("norsk") || lang_lower == "no" || lang_lower == "nor" {
        ("nor".to_string(), "Norwegian (Norsk)".to_string())
    } else if l_lower.contains("mala") || l_lower.contains("melayu") || lang_lower == "ms" || lang_lower == "msa" || lang_lower == "may" {
        ("msa".to_string(), "Malay (Bahasa Melayu)".to_string())
    } else if l_lower.contains("serb") || lang_lower == "srp" || lang_lower == "scc" {
        ("srp".to_string(), "Serbian (Srpski)".to_string())
    } else if l_lower.contains("bulg") || lang_lower == "bul" {
        ("bul".to_string(), "Bulgarian (Български)".to_string())
    } else if l_lower.contains("croat") || lang_lower == "hrv" || lang_lower == "scr" {
        ("hrv".to_string(), "Croatian (Hrvatski)".to_string())
    } else if l_lower.contains("tami") || lang_lower == "tam" {
        ("tam".to_string(), "Tamil (தமிழ்)".to_string())
    } else if l_lower.contains("beng") || lang_lower == "ben" {
        ("ben".to_string(), "Bengali (বাংলা)".to_string())
    } else if l_lower.contains("malayalam") || lang_lower == "mal" {
        ("mal".to_string(), "Malayalam (മലയാളം)".to_string())
    } else if l_lower.contains("alba") || lang_lower == "alb" || lang_lower == "sqi" {
        ("sqi".to_string(), "Albanian (Shqip)".to_string())
    } else if l_lower.contains("slov") || lang_lower == "slv" || lang_lower == "slo" {
        ("slv".to_string(), "Slovenian (Slovenščina)".to_string())
    } else {
        // Fallback: Never drop a valid subtitle track!
        let code = if !lang_lower.is_empty() {
            lang_lower
        } else {
            let filtered: String = l_lower.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
            if filtered.is_empty() { "sub".to_string() } else { filtered }
        };
        let display_name = if !label.is_empty() {
            label.to_string()
        } else if !lang.is_empty() {
            lang.to_string()
        } else {
            "Subtitle Track".to_string()
        };
        (code, display_name)
    }
}

fn map_subtitles(subs: &[serde_json::Value]) -> Vec<SubtitleOption> {
    let mut options = Vec::new();
    let mut seen_codes = HashSet::new();

    for s in subs {
        let label = s.get("label").and_then(|v| v.as_str()).unwrap_or("").trim();
        let lang = s.get("lang").and_then(|v| v.as_str()).unwrap_or("").trim();
        let src = s.get("src").and_then(|v| v.as_str()).unwrap_or("").trim();
        if src.is_empty() {
            continue;
        }

        let (code, name) = resolve_lang_meta(label, lang);

        if seen_codes.insert(code.clone()) {
            options.push(SubtitleOption {
                code,
                name: name.clone(),
                stream_label: if !label.is_empty() { label.to_string() } else { name },
                url: src.to_string(),
            });
        }
    }

    // Ensure English is first if present
    if let Some(pos) = options.iter().position(|o| o.code.starts_with("eng")) {
        let eng = options.remove(pos);
        options.insert(0, eng);
    } else {
        options.insert(0, SubtitleOption::default_english());
    }

    prioritize_subtitles(&mut options);

    options
}

fn extract_anime_id(html: &str) -> Option<String> {
    // Look for href="...-[0-9]+"
    for part in html.split("film-name") {
        if let Some(href_idx) = part.find("href=\"") {
            let after = &part[href_idx + 6..];
            if let Some(end_idx) = after.find('"') {
                let url = &after[..end_idx];
                let slug = url.split('/').last().unwrap_or("");
                if let Some(last_dash) = slug.rfind('-') {
                    let id_part = &slug[last_dash + 1..];
                    if id_part.chars().all(|c| c.is_ascii_digit()) && !id_part.is_empty() {
                        return Some(id_part.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_episode_id(html: &str, ep_target: u32) -> Option<String> {
    let target_str = format!("data-number=\"{}\"", ep_target);

    // 1. Check each anchor tag in the episode list
    for item in html.split("<a ") {
        if item.contains(&target_str) {
            if let Some(id_idx) = item.find("data-id=\"") {
                let after = &item[id_idx + 9..];
                if let Some(end_idx) = after.find('"') {
                    let id_str = &after[..end_idx];
                    if id_str.chars().all(|c| c.is_ascii_digit()) && !id_str.is_empty() {
                        return Some(id_str.to_string());
                    }
                }
            }
        }
    }

    // 2. Fallback: window after data-number
    if let Some(idx) = html.find(&target_str) {
        let snippet = &html[idx..html.len().min(idx + 150)];
        if let Some(id_idx) = snippet.find("data-id=\"") {
            let after = &snippet[id_idx + 9..];
            if let Some(end_idx) = after.find('"') {
                return Some(after[..end_idx].to_string());
            }
        }
    }

    // 3. Last fallback: any data-id
    if let Some(id_idx) = html.find("data-id=\"") {
        let after = &html[id_idx + 9..];
        if let Some(end_idx) = after.find('"') {
            return Some(after[..end_idx].to_string());
        }
    }
    None
}

fn extract_zoko_embed_url(html: &str, is_dub: bool) -> Option<String> {
    let mode_attr = if is_dub { "data-type=\"dub\"" } else { "data-type=\"sub\"" };
    for server_block in html.split("server-item") {
        if server_block.contains("ZokoAnime") && server_block.contains(mode_attr) {
            if let Some(hash_idx) = server_block.find("data-hash=\"") {
                let after = &server_block[hash_idx + 11..];
                if let Some(end_idx) = after.find('"') {
                    let hash_b64 = &after[..end_idx];
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(hash_b64) {
                        if let Ok(url) = String::from_utf8(bytes) {
                            return Some(url);
                        }
                    }
                }
            }
        }
    }
    None
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_lang_meta_arabic() {
        let (code, name) = resolve_lang_meta("Arabic", "");
        assert_eq!(code, "ara");
        assert_eq!(name, "Arabic (العربية)");
    }

    #[test]
    fn test_resolve_lang_meta_spanish_variants() {
        let (code1, name1) = resolve_lang_meta("Spanish (- Espanol)", "");
        assert_eq!(code1, "spa");
        assert_eq!(name1, "Spanish (Español)");

        let (code2, name2) = resolve_lang_meta("Spanish (- Espanol (Latin American))", "");
        assert_eq!(code2, "spa-419");
        assert_eq!(name2, "Spanish (Latin America)");

        let (code3, name3) = resolve_lang_meta("Spanish (- Espanol(Espana))", "");
        assert_eq!(code3, "spa-es");
        assert_eq!(name3, "Spanish (Castilian/Spain)");
    }

    #[test]
    fn test_resolve_lang_meta_community_codes() {
        let (c_ar, n_ar) = resolve_lang_meta("ara", "");
        assert_eq!(c_ar, "ara");
        assert_eq!(n_ar, "Arabic (العربية)");

        let (c_pt, n_pt) = resolve_lang_meta("pob", "");
        assert_eq!(c_pt, "por-br");
        assert_eq!(n_pt, "Portuguese (Brazil)");

        let (c_ja, n_ja) = resolve_lang_meta("jpn", "");
        assert_eq!(c_ja, "jpn");
        assert_eq!(n_ja, "Japanese (日本語)");

        let (c_ru, n_ru) = resolve_lang_meta("rus", "");
        assert_eq!(c_ru, "rus");
        assert_eq!(n_ru, "Russian (Русский)");
    }

    #[test]
    fn test_map_subtitles_ensures_english_first() {
        let json = serde_json::json!([
            {"label": "Arabic", "src": "https://example.com/ar.vtt"},
            {"label": "English", "src": "https://example.com/en.vtt"},
            {"label": "French (- Francais(France))", "src": "https://example.com/fr.vtt"}
        ]);
        let options = map_subtitles(json.as_array().unwrap());
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].code, "eng");
        assert_eq!(options[0].lang_keyword(), "English");
        assert_eq!(options[1].code, "ara");
        assert_eq!(options[1].lang_keyword(), "Arabic");
        assert_eq!(options[2].code, "fra");
        assert_eq!(options[2].lang_keyword(), "French");
    }

    #[test]
    fn test_lang_keywords_for_ani_cli() {
        let opt_ara = SubtitleOption {
            code: "ara".to_string(),
            name: "Arabic (العربية)".to_string(),
            stream_label: "Arabic".to_string(),
            url: "https://example.com/ar.vtt".to_string(),
        };
        assert_eq!(opt_ara.lang_keyword(), "Arabic");

        let opt_spa = SubtitleOption {
            code: "spa-419".to_string(),
            name: "Spanish (Latin America)".to_string(),
            stream_label: "Spanish (- Latin American)".to_string(),
            url: "https://example.com/es.vtt".to_string(),
        };
        assert_eq!(opt_spa.lang_keyword(), "Spanish");
    }

    #[test]
    fn test_extract_episode_id_order_independent() {
        let html1 = r##"<a title="Episode 1" class="ssl-item ep-item" data-number="1" data-id="3086" href="#">"##;
        assert_eq!(extract_episode_id(html1, 1), Some("3086".to_string()));

        let html2 = r##"<a data-id="9999" data-number="2" class="ssl-item" href="#">"##;
        assert_eq!(extract_episode_id(html2, 2), Some("9999".to_string()));
    }

    #[test]
    fn test_parse_m3u8_qualities_only_actual_resolutions() {
        let m3u8_jjk = "#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080\n1080/index.m3u8\n";
        let quals = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_jjk);
        assert_eq!(quals.len(), 1);
        assert_eq!(quals[0].label, "1080p");
        assert_eq!(quals[0].url, "https://cdn.example.com/stream/1080/index.m3u8");

        let m3u8_multi = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360\n360/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080\n1080/index.m3u8\n";
        let quals_multi = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_multi);
        assert_eq!(quals_multi.len(), 3);
        assert_eq!(quals_multi[0].label, "1080p");
        assert_eq!(quals_multi[1].label, "720p");
        assert_eq!(quals_multi[2].label, "360p");

        let m3u8_older = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=1056x800\n800/index.m3u8\n";
        let quals_older = parse_m3u8_qualities("https://cdn.example.com/stream/master.m3u8", m3u8_older);
        assert_eq!(quals_older.len(), 1);
        assert_eq!(quals_older[0].label, "800p");
    }

    #[test]
    fn test_prioritize_subtitles_orders_core_languages_first() {
        let mut subs = vec![
            SubtitleOption { code: "spa".to_string(), name: "Spanish".to_string(), stream_label: "Spanish".to_string(), url: "spa.vtt".to_string() },
            SubtitleOption { code: "rus".to_string(), name: "Russian".to_string(), stream_label: "Russian".to_string(), url: "rus.srt".to_string() },
            SubtitleOption { code: "fre".to_string(), name: "French".to_string(), stream_label: "French".to_string(), url: "fre.vtt".to_string() },
            SubtitleOption { code: "eng".to_string(), name: "English".to_string(), stream_label: "English".to_string(), url: "eng.vtt".to_string() },
            SubtitleOption { code: "ind".to_string(), name: "Indonesian".to_string(), stream_label: "Indonesian".to_string(), url: "ind.srt".to_string() },
            SubtitleOption { code: "ara".to_string(), name: "Arabic".to_string(), stream_label: "Arabic".to_string(), url: "ara.vtt".to_string() },
        ];

        prioritize_subtitles(&mut subs);

        let codes: Vec<&str> = subs.iter().map(|s| s.code.as_str()).collect();
        assert_eq!(codes, vec!["eng", "ara", "rus", "ind", "spa", "fre"]);
    }

    #[test]
    fn test_matches_episode() {
        assert!(matches_episode("0", "[Para Exo] Serial Experiments Lain 01 [BD].ass", 1));
        assert!(!matches_episode("0", "[Para Exo] Serial Experiments Lain 02 [BD].ass", 1));
        assert!(matches_episode("0", "Serial.Experiments.Lain.Layer01.AR.srt", 1));
        assert!(matches_episode("0", "Welcome To The NHK - 01.ass", 1));
        assert!(!matches_episode("0", "Welcome To The NHK - 11.ass", 1));
        assert!(matches_episode("1", "random_filename.srt", 1));
        assert!(!matches_episode("2", "random_filename.srt", 1));
    }

    #[tokio::test]
    async fn test_fetch_external_subtitles_finds_arabic_for_lain() {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let subs = fetch_external_subtitles(&client, 339, "Serial Experiments Lain", 1).await;
        let has_ara = subs.iter().any(|s| s.code.starts_with("ara"));
        assert!(has_ara, "Expected Arabic subtitle for Serial Experiments Lain Ep 1");
    }

    #[tokio::test]
    async fn test_fetch_external_subtitles_finds_languages_for_nhk() {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let subs = fetch_external_subtitles(&client, 1210, "Welcome to the N.H.K", 1).await;
        let has_ara = subs.iter().any(|s| s.code.starts_with("ara"));
        let has_rus = subs.iter().any(|s| s.code.starts_with("rus"));
        assert!(has_ara || has_rus, "Expected Arabic or Russian subtitle for Welcome to the N.H.K Ep 1");
    }
}
