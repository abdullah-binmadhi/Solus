//! AniSkip public API integration.
//! Queries opening, ending, and recap skip timestamps.

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SkipInterval {
    #[serde(rename = "startTime")]
    pub start_time: f64,
    #[serde(rename = "endTime")]
    pub end_time: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SkipResult {
    pub interval: SkipInterval,
    #[serde(rename = "skipType")]
    pub skip_type: String, // "op", "ed", "mixed-op", "mixed-ed", "recap"
}

#[derive(Debug, Deserialize)]
struct AniSkipResponse {
    #[serde(default)]
    found: bool,
    #[serde(default)]
    results: Vec<SkipResult>,
}

/// Fetch intro / outro skip timestamps for a given MAL anime ID and episode number.
pub async fn fetch_skip_times(mal_id: i64, episode: u32) -> Vec<SkipResult> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_default();

    let url = format!(
        "https://api.aniskip.com/v2/skip-times/{}/{}?types=op&types=ed&episodeLength=0",
        mal_id, episode
    );

    if let Ok(resp) = client.get(&url).send().await {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<AniSkipResponse>().await {
                if data.found {
                    return data.results;
                }
            }
        }
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_aniskip_payload() {
        let json = r#"{
            "found": true,
            "results": [
                {
                    "interval": { "startTime": 47.5, "endTime": 137.5 },
                    "skipType": "op"
                },
                {
                    "interval": { "startTime": 1342.0, "endTime": 1430.0 },
                    "skipType": "ed"
                }
            ]
        }"#;

        let res: AniSkipResponse = serde_json::from_str(json).unwrap();
        assert!(res.found);
        assert_eq!(res.results.len(), 2);
        assert_eq!(res.results[0].skip_type, "op");
        assert!((res.results[0].interval.start_time - 47.5).abs() < 1e-4);
        assert!((res.results[0].interval.end_time - 137.5).abs() < 1e-4);
    }
}
