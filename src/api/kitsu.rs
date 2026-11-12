//! Kitsu & AniZip API clients for episode metadata, synopses, and 16:9 thumbnail previews.
//! Used to provide rich per-episode titles, TVDB screencap thumbnails, and synopses.

use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitsuEpisodeInfo {
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub synopsis: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AniZipResponse {
    #[serde(default)]
    episodes: HashMap<String, AniZipEpisode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniZipEpisode {
    episode_number: Option<u32>,
    title: Option<AniZipTitle>,
    image: Option<String>,
    overview: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AniZipTitle {
    en: Option<String>,
    #[serde(rename = "x-jat")]
    x_jat: Option<String>,
    ja: Option<String>,
}

/// Fetch episode titles, 16:9 TVDB screencaps, and synopses by exact AniList ID via `api.ani.zip`.
pub async fn fetch_anizip_episodes(
    client: &reqwest::Client,
    anilist_id: i64,
) -> Option<HashMap<u32, KitsuEpisodeInfo>> {
    let resp = client
        .get("https://api.ani.zip/mappings")
        .query(&[("anilist_id", &anilist_id.to_string())])
        .header("Accept", "application/json")
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .ok()?;

    let parsed: AniZipResponse = resp.json().await.ok()?;
    let mut map = HashMap::new();

    for (key, ep) in parsed.episodes {
        let Ok(key_num) = key.parse::<u32>() else {
            continue;
        };
        let ep_num = ep.episode_number.unwrap_or(key_num);
        if ep_num == 0 {
            continue;
        }

        let title = ep
            .title
            .and_then(|t| {
                t.en.filter(|s| !s.trim().is_empty())
                    .or_else(|| t.x_jat.filter(|s| !s.trim().is_empty()))
                    .or_else(|| t.ja.filter(|s| !s.trim().is_empty()))
            })
            .unwrap_or_else(|| format!("Episode {}", ep_num));

        let thumbnail_url = ep.image.filter(|s| !s.trim().is_empty());
        let synopsis = ep
            .overview
            .filter(|s| !s.trim().is_empty())
            .or_else(|| ep.summary.filter(|s| !s.trim().is_empty()))
            .map(|s| s.trim().to_string());

        map.insert(
            ep_num,
            KitsuEpisodeInfo {
                title,
                thumbnail_url,
                synopsis,
            },
        );
    }

    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

#[derive(Debug, Deserialize)]
struct KitsuSearchResponse {
    data: Vec<KitsuAnimeResource>,
}

#[derive(Debug, Deserialize)]
struct KitsuAnimeResource {
    id: String,
}

#[derive(Debug, Deserialize)]
struct KitsuEpisodesResponse {
    data: Vec<KitsuEpisodeResource>,
}

#[derive(Debug, Deserialize)]
struct KitsuEpisodeResource {
    attributes: KitsuEpisodeAttributes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KitsuEpisodeAttributes {
    number: Option<u32>,
    canonical_title: Option<String>,
    thumbnail: Option<KitsuThumbnail>,
    synopsis: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KitsuThumbnail {
    original: Option<String>,
    large: Option<String>,
    medium: Option<String>,
}

/// Fetch episode titles, synopses, and 16:9 thumbnails from Kitsu.
/// Supports both primary and fallback search titles, and paginates through episode lists.
pub async fn fetch_kitsu_episodes(
    client: &reqwest::Client,
    title_primary: &str,
    title_fallback: Option<&str>,
) -> Option<HashMap<u32, KitsuEpisodeInfo>> {
    // 1. Search Kitsu for the anime ID (try primary, then fallback)
    let anime_id = match search_kitsu_id(client, title_primary).await {
        Some(id) => id,
        None => {
            {
                let fallback = title_fallback?;
                search_kitsu_id(client, fallback).await?
            }
        }
    };

    // 2. Fetch episodes for this anime (paginated with limit=20 up to 60 episodes)
    let mut map = HashMap::new();
    let mut offset = 0;
    let limit = 20;

    loop {
        let ep_resp = client
            .get("https://kitsu.io/api/edge/episodes")
            .query(&[
                ("filter[media_id]", anime_id.as_str()),
                ("page[limit]", &limit.to_string()),
                ("page[offset]", &offset.to_string()),
                ("sort", "number"),
            ])
            .header("Accept", "application/vnd.api+json")
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .ok()?;

        let ep_json: KitsuEpisodesResponse = ep_resp.json().await.ok()?;
        let count = ep_json.data.len();
        if count == 0 {
            break;
        }

        for (idx, ep_item) in ep_json.data.into_iter().enumerate() {
            let ep_num = ep_item.attributes.number.unwrap_or((offset + idx as u32) + 1);
            let ep_title = ep_item
                .attributes
                .canonical_title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| format!("Episode {}", ep_num));

            let thumb_url = ep_item.attributes.thumbnail.and_then(|t| {
                t.original.or(t.large).or(t.medium)
            });

            let synopsis = ep_item
                .attributes
                .synopsis
                .or(ep_item.attributes.description)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            map.insert(
                ep_num,
                KitsuEpisodeInfo {
                    title: ep_title,
                    thumbnail_url: thumb_url,
                    synopsis,
                },
            );
        }

        if count < limit as usize || offset >= 60 {
            break;
        }
        offset += limit;
    }

    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

async fn search_kitsu_id(client: &reqwest::Client, title: &str) -> Option<String> {
    let resp = client
        .get("https://kitsu.io/api/edge/anime")
        .query(&[("filter[text]", title), ("page[limit]", "1")])
        .header("Accept", "application/vnd.api+json")
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .ok()?;

    let json: KitsuSearchResponse = resp.json().await.ok()?;
    json.data.first().map(|a| a.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_kitsu_payload() {
        let json = r#"{
            "data": [
                {
                    "id": "123",
                    "type": "episodes",
                    "attributes": {
                        "number": 1,
                        "canonicalTitle": "Let You Down",
                        "synopsis": "David loses everything.",
                        "thumbnail": {
                            "original": "https://media.kitsu.app/episode/1/original.jpg",
                            "large": "https://media.kitsu.app/episode/1/large.jpg"
                        }
                    }
                }
            ]
        }"#;

        let parsed: KitsuEpisodesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 1);
        let ep = &parsed.data[0].attributes;
        assert_eq!(ep.number, Some(1));
        assert_eq!(ep.canonical_title.as_deref(), Some("Let You Down"));
        assert_eq!(ep.synopsis.as_deref(), Some("David loses everything."));
        assert_eq!(
            ep.thumbnail.as_ref().and_then(|t| t.original.as_deref()),
            Some("https://media.kitsu.app/episode/1/original.jpg")
        );
    }

    #[test]
    fn test_deserialize_anizip_payload() {
        let json = r#"{
            "episodes": {
                "1": {
                    "episodeNumber": 1,
                    "title": {
                        "en": "Starting Line",
                        "x-jat": "Start Line"
                    },
                    "image": "https://artworks.thetvdb.com/banners/v4/episode/1/screencap.jpg",
                    "overview": "Ashito begins his journey."
                }
            }
        }"#;
        let parsed: AniZipResponse = serde_json::from_str(json).unwrap();
        let ep1 = parsed.episodes.get("1").unwrap();
        assert_eq!(ep1.episode_number, Some(1));
        assert_eq!(ep1.title.as_ref().and_then(|t| t.en.as_deref()), Some("Starting Line"));
        assert_eq!(ep1.image.as_deref(), Some("https://artworks.thetvdb.com/banners/v4/episode/1/screencap.jpg"));
        assert_eq!(ep1.overview.as_deref(), Some("Ashito begins his journey."));
    }
}
