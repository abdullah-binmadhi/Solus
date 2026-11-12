//! AnimeFillerList integration.
//! Scrapes and parses canon / filler / mixed classifications.

use std::collections::HashMap;
use std::time::Duration;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillerType {
    MangaCanon,
    AnimeCanon,
    Mixed,
    Filler,
}

#[allow(dead_code)]
impl FillerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FillerType::MangaCanon => "manga_canon",
            FillerType::AnimeCanon => "anime_canon",
            FillerType::Mixed => "mixed",
            FillerType::Filler => "filler",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        if lower.contains("manga") {
            Some(FillerType::MangaCanon)
        } else if lower.contains("anime_canon") || lower.contains("anime canon") {
            Some(FillerType::AnimeCanon)
        } else if lower.contains("mixed") {
            Some(FillerType::Mixed)
        } else if lower.contains("filler") {
            Some(FillerType::Filler)
        } else {
            None
        }
    }

    pub fn badge_label(&self) -> &'static str {
        match self {
            FillerType::MangaCanon => "Canon",
            FillerType::AnimeCanon => "Canon",
            FillerType::Mixed => "Mixed",
            FillerType::Filler => "Filler",
        }
    }
}

pub fn slugify_title(title: &str) -> String {
    let mut clean = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            clean.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if (c.is_whitespace() || c == '-' || c == '_')
            && !prev_dash && !clean.is_empty() {
                clean.push('-');
                prev_dash = true;
            }
    }
    clean.trim_end_matches('-').to_string()
}

/// Fetch filler episode classifications for a show title from AnimeFillerList.
pub async fn fetch_filler_episodes(title: &str) -> HashMap<u32, String> {
    let slug = slugify_title(title);
    if slug.is_empty() {
        return HashMap::new();
    }

    let url = format!("https://www.animefillerlist.com/shows/{}", slug);
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return HashMap::new(),
    };

    let html = match resp.text().await {
        Ok(t) => t,
        _ => return HashMap::new(),
    };

    parse_filler_html(&html)
}

pub fn parse_filler_html(html: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for row in html.split("<tr ") {
        if let Some(end_tr) = row.find("</tr>") {
            let row_content = &row[..end_tr];
            let class_str = if let Some(cls_idx) = row_content.find("class=\"") {
                let after = &row_content[cls_idx + 7..];
                if let Some(end_cls) = after.find('"') {
                    &after[..end_cls]
                } else {
                    ""
                }
            } else {
                ""
            };

            if let Some(num_idx) = row_content.find("class=\"Number\">") {
                let after = &row_content[num_idx + 15..];
                if let Some(end_num) = after.find('<') {
                    if let Ok(ep) = after[..end_num].trim().parse::<u32>() {
                        if class_str.contains("manga_canon") {
                            map.insert(ep, "manga_canon".to_string());
                        } else if class_str.contains("mixed") {
                            map.insert(ep, "mixed".to_string());
                        } else if class_str.contains("filler") {
                            map.insert(ep, "filler".to_string());
                        } else if class_str.contains("anime_canon") {
                            map.insert(ep, "anime_canon".to_string());
                        }
                    }
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_title() {
        assert_eq!(slugify_title("Naruto: Shippuden"), "naruto-shippuden");
        assert_eq!(slugify_title("One Piece"), "one-piece");
        assert_eq!(slugify_title("Bleach: Thousand-Year Blood War"), "bleach-thousand-year-blood-war");
    }

    #[test]
    fn test_parse_filler_html() {
        let sample = r#"
            <tr class="manga_canon odd"><td class="Number">1</td><td class="Title">Pilot</td><td class="Type">Manga Canon</td></tr>
            <tr class="mixed_canon/filler even"><td class="Number">7</td><td class="Title">Fight</td><td class="Type">Mixed Canon/Filler</td></tr>
            <tr class="filler odd"><td class="Number">26</td><td class="Title">Recap</td><td class="Type">Filler</td></tr>
        "#;
        let map = parse_filler_html(sample);
        assert_eq!(map.get(&1), Some(&"manga_canon".to_string()));
        assert_eq!(map.get(&7), Some(&"mixed".to_string()));
        assert_eq!(map.get(&26), Some(&"filler".to_string()));
    }
}
