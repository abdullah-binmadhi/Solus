//! AniList GraphQL API client.
//! Fetches trending, popular, top-rated, seasonal, and search results.

use crate::{
    db::cache::Anime,
    error::{AppError, Result},
};
use serde::{Deserialize, Serialize};

const ANILIST_API: &str = "https://graphql.anilist.co";

/// The GraphQL query for fetching a page of anime with full metadata.
const MEDIA_FIELDS: &str = r#"
    id
    title { english romaji native }
    description(asHtml: false)
    episodes
    status
    season
    seasonYear
    averageScore
    format
    genres
    tags { name rank isMediaSpoiler }
    coverImage { large }
    countryOfOrigin
    nextAiringEpisode { episode }
"#;

// ─── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct GraphQLRequest<'a> {
    query:     &'a str,
    variables: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PageResponse {
    data: PageData,
}

#[derive(Debug, Deserialize)]
struct PageData {
    #[serde(rename = "Page")]
    page: PagePayload,
}

#[derive(Debug, Deserialize)]
struct PagePayload {
    media: Vec<RawAnime>,
}

#[derive(Debug, Deserialize)]
struct SingleMediaResponse {
    data: SingleMediaData,
}

#[derive(Debug, Deserialize)]
struct SingleMediaData {
    #[serde(rename = "Media")]
    media: Option<SingleMediaAiring>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingEpisode {
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub url: Option<String>,
    pub site: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimeMediaMeta {
    pub banner_image: Option<String>,
    pub streaming_episodes: Vec<StreamingEpisode>,
}

#[derive(Debug, Deserialize)]
struct StreamingEpisodesResponse {
    data: StreamingEpisodesData,
}

#[derive(Debug, Deserialize)]
struct StreamingEpisodesData {
    #[serde(rename = "Media")]
    media: Option<StreamingEpisodesMedia>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamingEpisodesMedia {
    banner_image: Option<String>,
    streaming_episodes: Option<Vec<StreamingEpisode>>,
}

#[derive(Debug, Deserialize)]
struct RecommendationsResponse {
    data: RecommendationsData,
}

#[derive(Debug, Deserialize)]
struct RecommendationsData {
    #[serde(rename = "Media")]
    media: Option<RecommendationsMedia>,
}

#[derive(Debug, Deserialize)]
struct RecommendationsMedia {
    recommendations: Option<RecommendationsPayload>,
}

#[derive(Debug, Deserialize)]
struct RecommendationsPayload {
    nodes: Vec<RecommendationNode>,
}

#[derive(Debug, Deserialize)]
struct RecommendationNode {
    #[serde(rename = "mediaRecommendation")]
    media_recommendation: Option<RawAnime>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SingleMediaAiring {
    #[allow(dead_code)]
    id: i64,
    status: Option<String>,
    episodes: Option<i64>,
    next_airing_episode: Option<RawNextAiringEpisode>,
}

#[derive(Debug, Deserialize)]
struct RawNextAiringEpisode {
    episode: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawTag {
    pub name: String,
    pub rank: Option<i64>,
    #[serde(rename = "isMediaSpoiler")]
    pub is_media_spoiler: Option<bool>,
}

/// Raw anime as returned by AniList — mapped to our `Anime` model.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAnime {
    id:            i64,
    title:         RawTitle,
    description:   Option<String>,
    episodes:      Option<i64>,
    status:        Option<String>,
    season:        Option<String>,
    season_year:   Option<i64>,
    average_score: Option<i64>,
    format:        Option<String>,
    genres:        Vec<String>,
    #[serde(default)]
    tags:          Option<Vec<RawTag>>,
    cover_image:   Option<RawCoverImage>,
    next_airing_episode: Option<RawNextAiringEpisode>,
}

#[derive(Debug, Deserialize)]
struct RawTitle {
    english: Option<String>,
    romaji:  Option<String>,
    native:  Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCoverImage {
    large: Option<String>,
}

impl RawAnime {
    /// Convert AniList response into our canonical Anime model.
    fn into_anime(self, now: i64) -> Anime {
        let mut combined_genres = self.genres;
        if let Some(tags) = self.tags {
            for t in tags {
                if !t.is_media_spoiler.unwrap_or(false) && t.rank.unwrap_or(0) >= 45 {
                    if !combined_genres.iter().any(|g| g.eq_ignore_ascii_case(&t.name)) {
                        combined_genres.push(t.name);
                    }
                }
            }
        }

        Anime {
            id:            self.id,
            title_english: self.title.english,
            title_romaji:  self.title.romaji.unwrap_or_else(|| "Unknown".to_string()),
            title_native:  self.title.native,
            description:   self.description,
            episodes:      self.episodes,
            status:        self.status,
            season:        self.season,
            season_year:   self.season_year,
            score:         self.average_score,
            format:        self.format,
            genres:        serde_json::to_string(&combined_genres).unwrap_or_else(|_| "[]".to_string()),
            cover_url:     self.cover_image.and_then(|c| c.large),
            cover_blob:    None,
            has_dub:       0, // AniList doesn't expose dub info reliably
            updated_at:    now,
            next_airing_episode: self.next_airing_episode.map(|n| n.episode),
        }
    }
}

// ─── Client ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ViewerInfo {
    pub id: i64,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UserCollection {
    pub watching: Vec<(Anime, u32)>, // (Anime, watched episode progress)
    pub planning: Vec<Anime>,        // Watchlist / Plan to Watch
    pub completed: Vec<Anime>,
    pub dropped: Vec<Anime>,
}

/// AniList API client. Wraps reqwest and converts results to our Anime model.
pub struct AniListClient {
    http: reqwest::Client,
    token: Option<String>,
}

impl AniListClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            token: None,
        }
    }

    pub fn with_token(token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
        }
    }

    /// Low-level GraphQL POST. Returns raw JSON value.
    async fn gql(&self, query: &str, variables: serde_json::Value) -> Result<serde_json::Value> {
        let body = GraphQLRequest { query, variables };
        let mut req = self.http.post(ANILIST_API).json(&body);
        if let Some(ref t) = self.token {
            let t_trimmed = t.trim();
            if !t_trimmed.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", t_trimmed));
            }
        }
        let resp = req
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        Ok(resp)
    }

    /// Fetch authenticated user profile
    pub async fn viewer(&self) -> Result<ViewerInfo> {
        let query = r#"query { Viewer { id name avatar { large } } }"#;
        let raw = self.gql(query, serde_json::json!({})).await?;
        let viewer_data = raw.pointer("/data/Viewer").ok_or_else(|| {
            AppError::Auth("Failed to get Viewer profile from AniList. Please verify your token.".into())
        })?;
        let id = viewer_data.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let name = viewer_data.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
        let avatar_url = viewer_data.pointer("/avatar/large").and_then(|v| v.as_str()).map(|s| s.to_string());
        Ok(ViewerInfo { id, name, avatar_url })
    }

    /// Fetch user anime collection from AniList
    pub async fn user_collection(&self, user_id: i64, now: i64) -> Result<UserCollection> {
        let query = format!(
            r#"query($userId: Int) {{
                MediaListCollection(userId: $userId, type: ANIME) {{
                    lists {{
                        name
                        status
                        entries {{
                            id
                            status
                            progress
                            media {{
                                {}
                            }}
                        }}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );
        let vars = serde_json::json!({ "userId": user_id });
        let raw = self.gql(&query, vars).await?;

        let mut coll = UserCollection::default();
        if let Some(lists) = raw.pointer("/data/MediaListCollection/lists").and_then(|v| v.as_array()) {
            for list in lists {
                let status = list.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(entries) = list.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        let progress = entry.get("progress").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(media_val) = entry.get("media") {
                            if let Ok(raw_anime) = serde_json::from_value::<RawAnime>(media_val.clone()) {
                                let anime = raw_anime.into_anime(now);
                                match status {
                                    "CURRENT" => coll.watching.push((anime, progress)),
                                    "PLANNING" => coll.planning.push(anime),
                                    "COMPLETED" => coll.completed.push(anime),
                                    "DROPPED" | "PAUSED" => coll.dropped.push(anime),
                                    _ => coll.planning.push(anime),
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(coll)
    }

    /// Fetch user anime collection by public AniList username without requiring an OAuth token.
    pub async fn public_user_collection(&self, username: &str, now: i64) -> Result<(ViewerInfo, UserCollection)> {
        let query = format!(
            r#"query($userName: String) {{
                User(name: $userName) {{
                    id
                    name
                    avatar {{ large }}
                }}
                MediaListCollection(userName: $userName, type: ANIME) {{
                    lists {{
                        name
                        status
                        entries {{
                            id
                            status
                            progress
                            media {{
                                {}
                            }}
                        }}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );
        let vars = serde_json::json!({ "userName": username });
        let raw = self.gql(&query, vars).await?;

        let user_val = raw.pointer("/data/User").ok_or_else(|| {
            AppError::Auth(format!("AniList user '{username}' not found. Please verify username spelling."))
        })?;
        let id = user_val.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let name = user_val.get("name").and_then(|v| v.as_str()).unwrap_or(username).to_string();
        let avatar_url = user_val.pointer("/avatar/large").and_then(|v| v.as_str()).map(|s| s.to_string());
        let viewer = ViewerInfo { id, name, avatar_url };

        let mut coll = UserCollection::default();
        if let Some(lists) = raw.pointer("/data/MediaListCollection/lists").and_then(|v| v.as_array()) {
            for list in lists {
                let status = list.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(entries) = list.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        let progress = entry.get("progress").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(media_val) = entry.get("media") {
                            if let Ok(raw_anime) = serde_json::from_value::<RawAnime>(media_val.clone()) {
                                let anime = raw_anime.into_anime(now);
                                match status {
                                    "CURRENT" => coll.watching.push((anime, progress)),
                                    "PLANNING" => coll.planning.push(anime),
                                    "COMPLETED" => coll.completed.push(anime),
                                    "DROPPED" | "PAUSED" => coll.dropped.push(anime),
                                    _ => coll.planning.push(anime),
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok((viewer, coll))
    }

    /// Save episode progress to AniList
    pub async fn save_progress(&self, media_id: i64, progress: u32) -> Result<()> {
        let mutation = r#"mutation($mediaId: Int, $progress: Int) {
            SaveMediaListEntry(mediaId: $mediaId, progress: $progress) {
                id
                progress
                status
            }
        }"#;
        let vars = serde_json::json!({ "mediaId": media_id, "progress": progress });
        let _ = self.gql(mutation, vars).await?;
        Ok(())
    }

    /// Add an anime to the user's AniList Planning (Watchlist)
    pub async fn save_planning_entry(&self, media_id: i64) -> Result<()> {
        let mutation = r#"mutation($mediaId: Int) {
            SaveMediaListEntry(mediaId: $mediaId, status: PLANNING) {
                id
                status
            }
        }"#;
        let vars = serde_json::json!({ "mediaId": media_id });
        let _ = self.gql(mutation, vars).await?;
        Ok(())
    }

    /// Remove an anime entry from the user's AniList MediaList
    pub async fn delete_media_list_entry(&self, user_id: Option<i64>, media_id: i64) -> Result<()> {
        let uid = match user_id {
            Some(id) => id,
            None => self.viewer().await?.id,
        };
        let query = r#"query($userId: Int, $mediaId: Int) {
            MediaList(userId: $userId, mediaId: $mediaId) {
                id
            }
        }"#;
        let vars = serde_json::json!({ "userId": uid, "mediaId": media_id });
        let raw = self.gql(query, vars).await?;
        if let Some(entry_id) = raw.pointer("/data/MediaList/id").and_then(|v| v.as_i64()) {
            let del_mutation = r#"mutation($id: Int) {
                DeleteMediaListEntry(id: $id) {
                    deleted
                }
            }"#;
            let _ = self.gql(del_mutation, serde_json::json!({ "id": entry_id })).await?;
        }
        Ok(())
    }

    /// Fetch current airing status, total episodes, and next airing episode for a specific anime.
    pub async fn fetch_airing_status(
        &self,
        id: i64,
    ) -> Result<(Option<String>, Option<i64>, Option<i64>)> {
        let query = r#"query($id: Int) {
            Media(id: $id, type: ANIME) {
                id
                status
                episodes
                nextAiringEpisode { episode }
            }
        }"#;
        let vars = serde_json::json!({ "id": id });
        let resp = self.gql(query, vars).await?;
        let parsed: SingleMediaResponse = serde_json::from_value(resp)
            .map_err(|e| AppError::Parse(format!("Failed to parse Media response: {e}")))?;
        if let Some(media) = parsed.data.media {
            let next_ep = media.next_airing_episode.map(|n| n.episode);
            Ok((media.status, media.episodes, next_ep))
        } else {
            Ok((None, None, None))
        }
    }

    /// Fetch streaming episode titles and 16:9 thumbnails for a specific anime.
    #[allow(dead_code)]
    pub async fn fetch_streaming_episodes(&self, id: i64) -> Result<Vec<StreamingEpisode>> {
        fetch_streaming_episodes_with_client(&self.http, id).await
    }

    /// Fetch trending anime (updated daily).
    pub async fn trending(&self, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query {{ Page(page: 1, perPage: 30) {{ media(sort: TRENDING_DESC, type: ANIME, isAdult: false) {{ {} }} }} }}"#,
            MEDIA_FIELDS
        );
        self.fetch_page(&query, serde_json::json!({}), now).await
    }

    /// Fetch all-time popular anime.
    pub async fn popular(&self, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query {{ Page(page: 1, perPage: 30) {{ media(sort: POPULARITY_DESC, type: ANIME, isAdult: false) {{ {} }} }} }}"#,
            MEDIA_FIELDS
        );
        self.fetch_page(&query, serde_json::json!({}), now).await
    }

    /// Fetch top-rated anime of all time.
    pub async fn top_rated(&self, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query {{ Page(page: 1, perPage: 30) {{ media(sort: SCORE_DESC, type: ANIME, isAdult: false) {{ {} }} }} }}"#,
            MEDIA_FIELDS
        );
        self.fetch_page(&query, serde_json::json!({}), now).await
    }

    /// Fetch currently airing seasonal anime.
    pub async fn seasonal(&self, season: &str, year: i64, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query($season: MediaSeason, $year: Int) {{
                Page(page: 1, perPage: 30) {{
                    media(season: $season, seasonYear: $year, sort: POPULARITY_DESC, type: ANIME, isAdult: false) {{
                        {}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );
        let vars = serde_json::json!({ "season": season, "year": year });
        self.fetch_page(&query, vars, now).await
    }

    /// Search anime by title.
    pub async fn search(&self, query_str: &str, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query($search: String) {{
                Page(page: 1, perPage: 30) {{
                    media(search: $search, type: ANIME, isAdult: false, sort: SEARCH_MATCH) {{
                        {}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );
        let vars = serde_json::json!({ "search": query_str });
        self.fetch_page(&query, vars, now).await
    }

    /// Helper to identify AniList standard genres
    fn is_genre(s: &str) -> bool {
        matches!(
            s.to_lowercase().as_str(),
            "action" | "adventure" | "comedy" | "drama" | "ecchi" | "fantasy" | "hentai" | "horror"
            | "mahou shoujo" | "mecha" | "music" | "mystery" | "psychological" | "romance" | "sci-fi"
            | "slice of life" | "sports" | "supernatural" | "thriller"
        )
    }

    /// Dynamic tailored attunement search against AniList's 350+ micro-tags and genres.
    pub async fn tailored_search(
        &self,
        tags_in: &[String],
        tags_not_in: &[String],
        min_score: Option<i64>,
        max_popularity: Option<i64>,
        excluded_ids: &[i64],
        now: i64,
    ) -> Result<Vec<Anime>> {
        let mut genres_in = Vec::new();
        let mut actual_tags_in = Vec::new();
        for t in tags_in {
            if Self::is_genre(t) {
                genres_in.push(t.clone());
            } else {
                actual_tags_in.push(t.clone());
            }
        }

        let mut genres_not_in = Vec::new();
        let mut actual_tags_not_in = Vec::new();
        for t in tags_not_in {
            if Self::is_genre(t) {
                genres_not_in.push(t.clone());
            } else {
                actual_tags_not_in.push(t.clone());
            }
        }

        let query = format!(
            r#"query(
                $genresIn: [String],
                $genresNotIn: [String],
                $tagsIn: [String],
                $tagsNotIn: [String],
                $minScore: Int,
                $maxPop: Int,
                $excludedIds: [Int]
            ) {{
                Page(page: 1, perPage: 40) {{
                    media(
                        genre_in: $genresIn,
                        genre_not_in: $genresNotIn,
                        tag_in: $tagsIn,
                        tag_not_in: $tagsNotIn,
                        averageScore_greater: $minScore,
                        popularity_lesser: $maxPop,
                        id_not_in: $excludedIds,
                        type: ANIME,
                        isAdult: false,
                        sort: [SCORE_DESC, POPULARITY_DESC]
                    ) {{
                        {}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );

        let mut vars = serde_json::json!({});
        if !genres_in.is_empty() {
            vars["genresIn"] = serde_json::json!(genres_in);
        }
        if !genres_not_in.is_empty() {
            vars["genresNotIn"] = serde_json::json!(genres_not_in);
        }
        if !actual_tags_in.is_empty() {
            vars["tagsIn"] = serde_json::json!(actual_tags_in);
        }
        if !actual_tags_not_in.is_empty() {
            vars["tagsNotIn"] = serde_json::json!(actual_tags_not_in);
        }
        if let Some(score) = min_score {
            vars["minScore"] = serde_json::json!(score);
        }
        if let Some(pop) = max_popularity {
            vars["maxPop"] = serde_json::json!(pop);
        }
        if !excluded_ids.is_empty() {
            let capped: Vec<i64> = excluded_ids.iter().take(500).copied().collect();
            vars["excludedIds"] = serde_json::json!(capped);
        }

        self.fetch_page(&query, vars, now).await
    }

    /// Fetch direct peer anime recommendations for an anchor anime ID from AniList.
    pub async fn fetch_peer_recommendations(&self, anchor_id: i64, now: i64) -> Result<Vec<Anime>> {
        let query = format!(
            r#"query($id: Int) {{
                Media(id: $id, type: ANIME) {{
                    recommendations(sort: RATING_DESC, page: 1, perPage: 20) {{
                        nodes {{
                            mediaRecommendation {{
                                {}
                            }}
                        }}
                    }}
                }}
            }}"#,
            MEDIA_FIELDS
        );
        let vars = serde_json::json!({ "id": anchor_id });
        let raw = self.gql(&query, vars).await?;
        let parsed: RecommendationsResponse = serde_json::from_value(raw)
            .map_err(|e| AppError::Parse(format!("AniList peer recommendations parse error: {e}")))?;

        let mut results = Vec::new();
        if let Some(media) = parsed.data.media {
            if let Some(recs) = media.recommendations {
                for node in recs.nodes {
                    if let Some(raw_anime) = node.media_recommendation {
                        if let Some(ref fmt) = raw_anime.format {
                            if fmt == "MANGA" || fmt == "NOVEL" || fmt == "ONE_SHOT" {
                                continue;
                            }
                        }
                        results.push(raw_anime.into_anime(now));
                    }
                }
            }
        }
        Ok(results)
    }

    /// Internal: POST query → parse Page response → convert to Vec<Anime>.
    async fn fetch_page(
        &self,
        query:     &str,
        variables: serde_json::Value,
        now:       i64,
    ) -> Result<Vec<Anime>> {
        let raw  = self.gql(query, variables).await?;
        let page: PageResponse = serde_json::from_value(raw)
            .map_err(|e| AppError::Parse(format!("AniList page parse error: {e}")))?;
        let anime = page
            .data
            .page
            .media
            .into_iter()
            .map(|r| r.into_anime(now))
            .collect();
        Ok(anime)
    }
}

/// Fetch anime banner image and streaming episode titles + 16:9 thumbnails from AniList directly.
pub async fn fetch_anime_streaming_and_banner(
    client: &reqwest::Client,
    id: i64,
) -> Result<AnimeMediaMeta> {
    let query = r#"query($id: Int) {
        Media(id: $id, type: ANIME) {
            bannerImage
            streamingEpisodes {
                title
                thumbnail
                url
                site
            }
        }
    }"#;
    let body = serde_json::json!({
        "query": query,
        "variables": { "id": id },
    });
    let resp = client
        .post(ANILIST_API)
        .json(&body)
        .send()
        .await
        .map_err(AppError::Network)?;

    let parsed: StreamingEpisodesResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Failed to parse streamingEpisodes response: {e}")))?;

    if let Some(media) = parsed.data.media {
        Ok(AnimeMediaMeta {
            banner_image: media.banner_image,
            streaming_episodes: media.streaming_episodes.unwrap_or_default(),
        })
    } else {
        Ok(AnimeMediaMeta {
            banner_image: None,
            streaming_episodes: Vec::new(),
        })
    }
}

/// Fetch streaming episode titles and 16:9 thumbnails from AniList directly.
pub async fn fetch_streaming_episodes_with_client(
    client: &reqwest::Client,
    id: i64,
) -> Result<Vec<StreamingEpisode>> {
    Ok(fetch_anime_streaming_and_banner(client, id).await?.streaming_episodes)
}

/// Resolve the MyAnimeList ID for an AniList anime ID.
pub async fn resolve_mal_id(client: &reqwest::Client, anilist_id: i64) -> Option<i64> {
    let query = r#"query($id: Int) { Media(id: $id, type: ANIME) { idMal } }"#;
    let body = serde_json::json!({
        "query": query,
        "variables": { "id": anilist_id },
    });
    let resp = client.post(ANILIST_API).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    data.pointer("/data/Media/idMal")?.as_i64()
}

/// Helper to extract episode number and cleaned title from streaming episode entries.
/// E.g. "Episode 1 - Ryomen Sukuna" -> (1, "Ryomen Sukuna")
/// E.g. "Episode 12 - EAGLE EYE" -> (12, "EAGLE EYE")
pub fn parse_episode_title_and_number(raw_title: &str, fallback_index: u32) -> (u32, String) {
    let trimmed = raw_title.trim();
    let lower = trimmed.to_lowercase();

    // Check for "episode X" or "ep X" or "eX"
    if let Some(rest) = lower.strip_prefix("episode ") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = num_str.parse::<u32>() {
            let after_num = trimmed["episode ".len() + num_str.len()..].trim();
            let clean = after_num
                .strip_prefix('-')
                .or_else(|| after_num.strip_prefix(':'))
                .unwrap_or(after_num)
                .trim();
            let final_title = if clean.is_empty() {
                format!("Episode {}", num)
            } else {
                clean.to_string()
            };
            return (num, final_title);
        }
    } else if let Some(rest) = lower.strip_prefix("ep ") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = num_str.parse::<u32>() {
            let after_num = trimmed["ep ".len() + num_str.len()..].trim();
            let clean = after_num
                .strip_prefix('-')
                .or_else(|| after_num.strip_prefix(':'))
                .unwrap_or(after_num)
                .trim();
            let final_title = if clean.is_empty() {
                format!("Episode {}", num)
            } else {
                clean.to_string()
            };
            return (num, final_title);
        }
    }

    (fallback_index, trimmed.to_string())
}

impl Default for AniListClient {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Pure unit tests (no HTTP) ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_anime_fixture(id: i64, english: Option<&str>, romaji: &str) -> RawAnime {
        RawAnime {
            id,
            title:         RawTitle {
                english: english.map(str::to_string),
                romaji:  Some(romaji.to_string()),
                native:  None,
            },
            description:   Some("A test anime".to_string()),
            episodes:      Some(24),
            status:        Some("FINISHED".to_string()),
            season:        Some("FALL".to_string()),
            season_year:   Some(2023),
            average_score: Some(87),
            format:        Some("TV".to_string()),
            genres:        vec!["Action".to_string(), "Drama".to_string()],
            tags:          None,
            cover_image:   Some(RawCoverImage {
                large: Some("https://example.com/cover.jpg".to_string()),
            }),
            next_airing_episode: None,
        }
    }

    #[test]
    fn test_raw_anime_into_anime_with_english_title() {
        let raw   = raw_anime_fixture(1535, Some("Sword Art Online"), "Sword Art Online");
        let anime = raw.into_anime(1_700_000_000);

        assert_eq!(anime.id, 1535);
        assert_eq!(anime.title_english, Some("Sword Art Online".to_string()));
        assert_eq!(anime.title_romaji,  "Sword Art Online");
        assert_eq!(anime.score,         Some(87));
        assert_eq!(anime.episodes,      Some(24));
    }

    #[test]
    fn test_raw_anime_into_anime_without_english_title() {
        let raw   = raw_anime_fixture(1, None, "Sousou no Frieren");
        let anime = raw.into_anime(0);

        assert!(anime.title_english.is_none());
        assert_eq!(anime.title_romaji, "Sousou no Frieren");
    }

    #[test]
    fn test_raw_anime_genres_serialized_to_json() {
        let raw   = raw_anime_fixture(1, None, "Test");
        let anime = raw.into_anime(0);

        let genres: Vec<String> = serde_json::from_str(&anime.genres).unwrap();
        assert_eq!(genres, vec!["Action", "Drama"]);
    }

    #[test]
    fn test_raw_anime_cover_url_extracted() {
        let raw   = raw_anime_fixture(1, None, "Test");
        let anime = raw.into_anime(0);
        assert_eq!(anime.cover_url, Some("https://example.com/cover.jpg".to_string()));
    }

    #[test]
    fn test_raw_anime_no_cover_image() {
        let mut raw      = raw_anime_fixture(1, None, "Test");
        raw.cover_image  = None;
        let anime        = raw.into_anime(0);
        assert!(anime.cover_url.is_none());
    }

    #[test]
    fn test_raw_anime_empty_romaji_uses_unknown() {
        let raw = RawAnime {
            id:            99,
            title:         RawTitle { english: None, romaji: None, native: None },
            description:   None,
            episodes:      None,
            status:        None,
            season:        None,
            season_year:   None,
            average_score: None,
            format:        None,
            genres:        vec![],
            tags:          None,
            cover_image:   None,
            next_airing_episode: None,
        };
        let anime = raw.into_anime(0);
        assert_eq!(anime.title_romaji, "Unknown");
    }

    #[test]
    fn test_raw_anime_has_dub_defaults_to_zero() {
        let raw   = raw_anime_fixture(1, None, "Test");
        let anime = raw.into_anime(0);
        assert_eq!(anime.has_dub, 0);
    }

    #[test]
    fn test_raw_anime_updated_at_is_set() {
        let raw   = raw_anime_fixture(1, None, "Test");
        let now   = 1_700_123_456i64;
        let anime = raw.into_anime(now);
        assert_eq!(anime.updated_at, now);
    }

    #[test]
    fn test_single_media_response_deserialization() {
        let json_str = r#"{"data":{"Media":{"id":195516,"status":"RELEASING","episodes":12,"nextAiringEpisode":{"episode":7}}}}"#;
        let parsed: SingleMediaResponse = serde_json::from_str(json_str).unwrap();
        assert!(parsed.data.media.is_some());
        let media = parsed.data.media.unwrap();
        assert_eq!(media.id, 195516);
        assert_eq!(media.status.as_deref(), Some("RELEASING"));
        assert_eq!(media.episodes, Some(12));
        assert_eq!(media.next_airing_episode.map(|n| n.episode), Some(7));
    }

    #[test]
    fn test_parse_episode_title_and_number() {
        assert_eq!(
            parse_episode_title_and_number("Episode 1 - Ryomen Sukuna", 1),
            (1, "Ryomen Sukuna".to_string())
        );
        assert_eq!(
            parse_episode_title_and_number("Episode 14 - Kyoto Sister School Exchange Event - Group Battle 0 -", 14),
            (14, "Kyoto Sister School Exchange Event - Group Battle 0 -".to_string())
        );
        assert_eq!(
            parse_episode_title_and_number("Ep 3: Night Training", 3),
            (3, "Night Training".to_string())
        );
        assert_eq!(
            parse_episode_title_and_number("Episode 5", 5),
            (5, "Episode 5".to_string())
        );
        assert_eq!(
            parse_episode_title_and_number("Some Random ONA Title", 42),
            (42, "Some Random ONA Title".to_string())
        );
    }
}
