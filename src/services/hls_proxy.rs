//! Local HTTP/1.1 HLS segment-unwrapper proxy for CDNs that disguise MPEG-TS segments
//! behind fake PNG/GIF/JPEG image headers (e.g., ZokoAnime `seg_00000.ts.jpg` with `\x89PNG...IEND...`).
//!
//! Uses only `tokio::net::TcpListener`, `reqwest::Client`, and `base64` — zero extra crate deps.

use base64::Engine;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const PROXY_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Detect fake image header (PNG/GIF/JPEG/RIFF) hiding an MPEG-TS stream (`0x47` sync triplet
/// every 188 bytes) and return the byte offset where valid MPEG-TS packets begin.
pub fn detect_disguised_image_prefix(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 400 {
        return None;
    }

    // Already clean MPEG-TS or ISO BMFF / fMP4
    if bytes[0] == 0x47 {
        return None;
    }
    if bytes.len() >= 12 && (&bytes[4..8] == b"ftyp" || &bytes[4..8] == b"moof" || &bytes[4..8] == b"styp") {
        return None;
    }

    let is_disguised_image = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || (bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP");

    if !is_disguised_image {
        return None;
    }

    // Fast path for PNG: check immediately after IEND + 4-byte CRC
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if let Some(iend_pos) = bytes.windows(4).position(|w| w == b"IEND") {
            let after_crc = iend_pos + 8;
            if after_crc + 376 < bytes.len()
                && bytes[after_crc] == 0x47
                && bytes[after_crc + 188] == 0x47
                && bytes[after_crc + 376] == 0x47
            {
                return Some(after_crc);
            }
        }
    }

    // General scan inside first 64 KiB for verified 0x47 MPEG-TS sync triplet
    let scan_end = bytes.len().saturating_sub(377).min(65536);
    for i in 1..=scan_end {
        if bytes[i] == 0x47 && bytes[i + 188] == 0x47 && bytes[i + 376] == 0x47 {
            return Some(i);
        }
    }

    None
}

/// Strip disguised leading image bytes if a verified MPEG-TS sync triplet follows.
pub fn strip_disguised_image_prefix(bytes: &[u8]) -> &[u8] {
    match detect_disguised_image_prefix(bytes) {
        Some(off) if off < bytes.len() => &bytes[off..],
        _ => bytes,
    }
}

fn encode_param(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
}

fn decode_param(s: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()?;
    String::from_utf8(bytes).ok()
}

fn resolve_uri(playlist_url: &str, raw_uri: &str) -> String {
    let trimmed = raw_uri.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.starts_with("//") {
        let scheme = if playlist_url.starts_with("http://") { "http:" } else { "https:" };
        return format!("{scheme}{trimmed}");
    }
    if trimmed.starts_with('/') {
        if let Some(scheme_end) = playlist_url.find("://") {
            let after_scheme = &playlist_url[scheme_end + 3..];
            let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
            let origin = &playlist_url[..scheme_end + 3 + host_end];
            return format!("{origin}{trimmed}");
        }
    }
    let clean_playlist = playlist_url.split('?').next().unwrap_or(playlist_url);
    let base = clean_playlist
        .rfind('/')
        .map(|i| &clean_playlist[..=i])
        .unwrap_or(clean_playlist);
    format!("{base}{trimmed}")
}

fn is_m3u8_uri(uri: &str) -> bool {
    uri.split('?')
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase()
        .ends_with(".m3u8")
}

fn build_proxy_target(proxy_base: &str, kind: &str, upstream_url: &str, referer: &str) -> String {
    let u_enc = encode_param(upstream_url);
    let r_enc = encode_param(referer);
    format!("{proxy_base}/{kind}?u={u_enc}&r={r_enc}")
}

/// Rewrite all playlist/segment/key URIs in an M3U8 manifest to route through the local proxy.
pub fn rewrite_m3u8_for_proxy(
    playlist_url: &str,
    m3u8_text: &str,
    proxy_base: &str,
    referer: &str,
) -> String {
    let mut out = String::with_capacity(m3u8_text.len() * 2);
    for raw_line in m3u8_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if let Some(rewritten) = rewrite_tag_uri(line, playlist_url, proxy_base, referer) {
                out.push_str(&rewritten);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        } else {
            let abs_uri = resolve_uri(playlist_url, line);
            let kind = if is_m3u8_uri(&abs_uri) { "m3u8" } else { "seg.ts" };
            out.push_str(&build_proxy_target(proxy_base, kind, &abs_uri, referer));
            out.push('\n');
        }
    }
    out
}

fn rewrite_tag_uri(
    line: &str,
    playlist_url: &str,
    proxy_base: &str,
    referer: &str,
) -> Option<String> {
    let uri_marker = "URI=\"";
    let idx = line.find(uri_marker)?;
    let val_start = idx + uri_marker.len();
    let rel_end = line[val_start..].find('"')?;
    let val_end = val_start + rel_end;
    let raw_uri = &line[val_start..val_end];
    if raw_uri.is_empty() {
        return None;
    }
    let abs_uri = resolve_uri(playlist_url, raw_uri);
    let kind = if is_m3u8_uri(&abs_uri) {
        "m3u8"
    } else if line.starts_with("#EXT-X-KEY:") || line.starts_with("#EXT-X-MAP:") {
        "raw"
    } else {
        "seg.ts"
    };
    let proxied = build_proxy_target(proxy_base, kind, &abs_uri, referer);
    Some(format!("{}{}{}", &line[..val_start], proxied, &line[val_end..]))
}

/// Running local loopback HTTP/1.1 HLS proxy instance.
#[derive(Debug)]
pub struct HlsProxyServer {
    pub addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl HlsProxyServer {
    pub async fn start(force_ipv4: bool) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let mut builder = reqwest::Client::builder()
            .user_agent(PROXY_UA)
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(25))
            .pool_max_idle_per_host(8);
        if force_ipv4 {
            builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        }
        let client = Arc::new(builder.build().unwrap_or_default());
        let proxy_base = format!("http://{addr}");
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((stream, _)) = accept_res else { break; };
                        let client = client.clone();
                        let base = proxy_base.clone();
                        tokio::spawn(async move {
                            let _ = handle_client(stream, &client, &base).await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn wrap_playlist_url(&self, upstream_url: &str, referer: &str) -> String {
        build_proxy_target(&self.base_url(), "m3u8", upstream_url, referer)
    }
}

impl Drop for HlsProxyServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn parse_query_params(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

fn should_retry_http_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
}

async fn fetch_upstream_with_retry(
    client: &reqwest::Client,
    upstream_url: &str,
    referer: &str,
) -> std::result::Result<Vec<u8>, (String, String)> {
    const BACKOFFS_MS: [u64; 3] = [0, 150, 400];
    let mut last_err = ("502 Bad Gateway".to_string(), "Upstream request failed".to_string());

    for (attempt, delay_ms) in BACKOFFS_MS.iter().enumerate() {
        if *delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
        }
        let resp = match client
            .get(upstream_url)
            .header("Referer", referer)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = ("502 Bad Gateway".to_string(), format!("Upstream error: {e}"));
                if attempt + 1 < BACKOFFS_MS.len() {
                    continue;
                }
                break;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            last_err = (
                "502 Bad Gateway".to_string(),
                format!("Upstream HTTP {}", status.as_u16()),
            );
            if should_retry_http_status(status) && attempt + 1 < BACKOFFS_MS.len() {
                continue;
            }
            break;
        }

        match resp.bytes().await {
            Ok(b) => return Ok(b.to_vec()),
            Err(_) => {
                last_err = ("502 Bad Gateway".to_string(), "Upstream read failed".to_string());
                if attempt + 1 < BACKOFFS_MS.len() {
                    continue;
                }
                break;
            }
        }
    }

    Err(last_err)
}

async fn handle_client(
    mut stream: TcpStream,
    client: &reqwest::Client,
    proxy_base: &str,
) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let req_str = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_str.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if method != "GET" && method != "HEAD" {
        return write_http_response(&mut stream, "405 Method Not Allowed", "text/plain", b"Method Not Allowed").await;
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params = parse_query_params(query);
    let Some(upstream_url) = params.get("u").and_then(|v| decode_param(v)) else {
        return write_http_response(&mut stream, "400 Bad Request", "text/plain", b"Missing upstream URL").await;
    };
    let referer = params
        .get("r")
        .and_then(|v| decode_param(v))
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "https://zokoanime.video/".to_string());

    if !upstream_url.starts_with("http://") && !upstream_url.starts_with("https://") {
        return write_http_response(&mut stream, "400 Bad Request", "text/plain", b"Invalid URL").await;
    }

    let bytes = match fetch_upstream_with_retry(client, &upstream_url, &referer).await {
        Ok(b) => b,
        Err((status, msg)) => {
            return write_http_response(&mut stream, &status, "text/plain", msg.as_bytes()).await;
        }
    };

    if path == "/m3u8" {
        let text = String::from_utf8_lossy(&bytes);
        let rewritten = rewrite_m3u8_for_proxy(&upstream_url, &text, proxy_base, &referer);
        return write_http_response(
            &mut stream,
            "200 OK",
            "application/vnd.apple.mpegurl",
            rewritten.as_bytes(),
        )
        .await;
    }

    if path == "/seg.ts" {
        let stripped = strip_disguised_image_prefix(&bytes);
        return write_http_response(&mut stream, "200 OK", "video/mp2t", stripped).await;
    }

    write_http_response(&mut stream, "200 OK", "application/octet-stream", &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_disguised_png_mpegts_header() {
        // Fake 1x1 PNG header ending in IEND + 4-byte CRC (67 bytes), padded to 252 bytes like Zoko Ep 1
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\rIDATx\xdacd\xf8\xcfP\x0f\x00\x03\x86\x01\x80Z4}k\x00\x00\x00\x00IEND\xaeB`\x82");
        let ts_start = pkt.len();
        for _ in 0..3 {
            pkt.push(0x47);
            pkt.extend_from_slice(&[0x11; 187]);
        }

        assert_eq!(detect_disguised_image_prefix(&pkt), Some(ts_start));
        let stripped = strip_disguised_image_prefix(&pkt);
        assert_eq!(stripped.len(), 188 * 3);
        assert_eq!(stripped[0], 0x47);
        assert_eq!(stripped[188], 0x47);
        assert_eq!(stripped[376], 0x47);
    }

    #[test]
    fn test_raw_mpegts_passes_through_untouched() {
        let mut raw = Vec::new();
        for _ in 0..3 {
            raw.push(0x47);
            raw.extend_from_slice(&[0x40; 187]);
        }
        assert_eq!(detect_disguised_image_prefix(&raw), None);
        assert_eq!(strip_disguised_image_prefix(&raw), raw.as_slice());
    }

    #[test]
    fn test_rewrite_m3u8_rewrites_variants_and_segments() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5300000,RESOLUTION=1920x1080\n1080/index.m3u8\n";
        let out_master = rewrite_m3u8_for_proxy(
            "https://hls.example.com/p/123/master.m3u8?v=abc",
            master,
            "http://127.0.0.1:45555",
            "https://zokoanime.video/",
        );
        assert!(out_master.contains("http://127.0.0.1:45555/m3u8?u="));

        let variant = "#EXTM3U\n#EXTINF:4.0,\nseg_00000.ts.jpg\n";
        let out_var = rewrite_m3u8_for_proxy(
            "https://hls.example.com/p/123/1080/index.m3u8",
            variant,
            "http://127.0.0.1:45555",
            "https://zokoanime.video/",
        );
        assert!(out_var.contains("http://127.0.0.1:45555/seg.ts?u="));
        assert!(!out_var.ends_with(".ts.jpg\n"));
    }
}
