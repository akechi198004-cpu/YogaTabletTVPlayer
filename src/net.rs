//! HTTP 客户端、测速、M3U 下载与播放前广告预检。

use std::io::Read;
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, RANGE, REFERER};
use reqwest::redirect::Policy;
use thiserror::Error;
use url::Url;

use crate::adguard;
use crate::models::{PreflightOutcome, SpeedInfo};

/// 默认 UA。部分公共源会拒绝 VLC 默认 UA，预检和导入统一使用浏览器 UA。
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win32; x86) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36 TouchIPTV/2.0";

const PREFLIGHT_BUDGET: Duration = Duration::from_secs(10);
const PREFLIGHT_REQUEST_CAP: Duration = Duration::from_secs(6);
const PREFLIGHT_VARIANT_CAP: Duration = Duration::from_secs(3);
const MAX_MANIFEST_BYTES: usize = 512 * 1024;
const MAX_VARIANT_BYTES: usize = 256 * 1024;
const MAX_PLAYLIST_BYTES: usize = 6 * 1024 * 1024;

/// 创建一个只走直连的 HTTP 客户端。禁用系统代理，避免预检与 VLC 的实际
/// 网络路径不一致。
pub fn build_client(user_agent: &str) -> Result<Client, NetError> {
    let ua = if user_agent.trim().is_empty() {
        DEFAULT_USER_AGENT
    } else {
        user_agent.trim()
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(ACCEPT, "*/*".parse().unwrap());
    headers.insert(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.6".parse().unwrap());

    Ok(Client::builder()
        .user_agent(ua)
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(20))
        .redirect(Policy::limited(10))
        .gzip(true)
        .no_proxy()
        .build()?)
}

/// 下载一个小文本资源（M3U、HTML 等）。
pub fn fetch_text(client: &Client, url: &str, max_bytes: usize) -> Result<String, NetError> {
    let response = client.get(url).timeout(Duration::from_secs(20)).send()?;
    let status = response.status();
    if !status.is_success() {
        return Err(NetError::Status(status.as_u16()));
    }
    let mut body = Vec::new();
    response.take(max_bytes as u64).read_to_end(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// 单频道测速：连接 + 首批字节耗时，并尝试从 HLS 清单里读取分辨率。
pub fn test_speed(client: &Client, url: &str, referer: &str) -> Result<SpeedInfo, String> {
    if url.trim().is_empty() {
        return Err("空地址".to_owned());
    }

    let started = Instant::now();
    let mut request = client
        .get(url)
        .header(RANGE, "bytes=0-65535")
        .timeout(Duration::from_secs(8));
    if !referer.trim().is_empty() {
        request = request.header(REFERER, referer.trim());
    }
    let mut response = request
        .send()
        .map_err(|error| format!("请求失败：{error}"))?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 206 {
        return Err(format!("HTTP {}", status.as_u16()));
    }

    let mut buffer = [0u8; 8192];
    let read = response
        .read(&mut buffer)
        .map_err(|error| format!("读取失败：{error}"))?;
    if read == 0 {
        return Err("服务器没有返回内容".to_owned());
    }

    let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    let text = String::from_utf8_lossy(&buffer[..read]);
    let height = parse_best_resolution(&text).unwrap_or(0);

    Ok(SpeedInfo {
        latency_ms,
        height,
        resolution: if height > 0 {
            format!("{height}P")
        } else {
            String::new()
        },
    })
}

/// 播放前预检。网络失败不阻止播放，只返回提示；命中已知广告特征才阻止。
pub fn preflight(client: &Client, url: &str, ad_guard: bool, referer: &str) -> PreflightOutcome {
    if !ad_guard {
        return PreflightOutcome::default();
    }

    if let Some(signature) = adguard::find_signature(url) {
        return PreflightOutcome {
            blocked: Some(signature),
            ..Default::default()
        };
    }

    let deadline = Instant::now() + PREFLIGHT_BUDGET;
    let mut initial_request = client
        .get(url)
        .header(RANGE, "bytes=0-524287")
        .timeout(request_timeout(deadline, PREFLIGHT_REQUEST_CAP));
    if !referer.trim().is_empty() {
        initial_request = initial_request.header(REFERER, referer.trim());
    }
    let response = match initial_request.send() {
        Ok(response) => response,
        Err(error) => {
            return PreflightOutcome {
                warning: Some(format!("预检请求失败，直接尝试播放：{error}")),
                final_url: Some(url.to_owned()),
                ..Default::default()
            };
        }
    };

    let status = response.status();
    if !status.is_success() && status.as_u16() != 206 {
        return PreflightOutcome {
            warning: Some(format!("预检返回 HTTP {}，直接尝试播放", status.as_u16())),
            final_url: Some(response.url().to_string()),
            ..Default::default()
        };
    }

    let final_url = response.url().to_string();
    if let Some(signature) = adguard::find_signature(&final_url) {
        return PreflightOutcome {
            blocked: Some(signature),
            final_url: Some(final_url),
            ..Default::default()
        };
    }

    let is_hls = is_hls_content_type(&response);
    let Some(manifest) = read_manifest_prefix(response, is_hls, MAX_MANIFEST_BYTES) else {
        return PreflightOutcome {
            final_url: Some(final_url),
            ..Default::default()
        };
    };

    if let Some(signature) = adguard::find_manifest_signature(&manifest, &final_url) {
        return PreflightOutcome {
            blocked: Some(signature),
            final_url: Some(final_url),
            ..Default::default()
        };
    }

    // 主播放列表还要继续检查前两个子清单。很多广告是放在子清单的
    // segment 路径里的，只看主清单会漏掉。
    let variants = extract_variant_urls(&manifest, &final_url);
    for variant in variants.into_iter().take(2) {
        if let Some(signature) = adguard::find_signature(&variant) {
            return PreflightOutcome {
                blocked: Some(signature),
                final_url: Some(final_url),
                ..Default::default()
            };
        }
        if Instant::now() >= deadline {
            break;
        }

        let mut variant_request = client
            .get(&variant)
            .header(RANGE, "bytes=0-262143")
            .timeout(request_timeout(deadline, PREFLIGHT_VARIANT_CAP));
        if !referer.trim().is_empty() {
            variant_request = variant_request.header(REFERER, referer.trim());
        }
        let Ok(response) = variant_request.send() else {
            continue;
        };
        if !response.status().is_success() && response.status().as_u16() != 206 {
            continue;
        }
        let variant_url = response.url().to_string();
        if let Some(signature) = adguard::find_signature(&variant_url) {
            return PreflightOutcome {
                blocked: Some(signature),
                final_url: Some(final_url),
                ..Default::default()
            };
        }
        let Some(variant_manifest) = read_manifest_prefix(response, true, MAX_VARIANT_BYTES) else {
            continue;
        };
        if let Some(signature) = adguard::find_manifest_signature(&variant_manifest, &variant_url) {
            return PreflightOutcome {
                blocked: Some(signature),
                final_url: Some(final_url),
                ..Default::default()
            };
        }
    }

    PreflightOutcome {
        final_url: Some(final_url),
        ..Default::default()
    }
}

/// 读取前缀判断是否为 HLS 清单。非清单（裸 TS/FLV）只读 4KB，避免为了
/// 预检提前下载大量视频数据。
fn read_manifest_prefix(
    mut response: Response,
    content_type_hls: bool,
    max_bytes: usize,
) -> Option<String> {
    let mut body = Vec::with_capacity(4096);
    let mut buffer = [0u8; 4096];
    let read = response.read(&mut buffer).ok()?;
    if read == 0 {
        return None;
    }
    body.extend_from_slice(&buffer[..read]);

    let looks_like_manifest = content_type_hls || looks_like_m3u(&body);
    if !looks_like_manifest {
        return None;
    }

    if body.len() < max_bytes {
        let limit = (max_bytes - body.len()) as u64;
        let mut rest = Vec::new();
        let _ = response.take(limit).read_to_end(&mut rest);
        body.extend_from_slice(&rest);
    }

    Some(String::from_utf8_lossy(&body).into_owned())
}

fn looks_like_m3u(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("#EXTM3U")
}

fn is_hls_content_type(response: &Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("mpegurl")
                || value.contains("m3u8")
                || value.contains("vnd.apple.mpegurl")
        })
        .unwrap_or(false)
}

fn request_timeout(deadline: Instant, cap: Duration) -> Duration {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Duration::from_millis(1)
    } else {
        left.min(cap)
    }
}

/// 从 HLS 主播放列表里提取前几个子清单地址。
pub fn extract_variant_urls(manifest: &str, base_url: &str) -> Vec<String> {
    let Ok(base) = Url::parse(base_url) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut expect_variant = false;

    for raw_line in manifest.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("#EXT-X-STREAM-INF") {
            expect_variant = true;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if expect_variant {
            expect_variant = false;
            if let Ok(url) = base.join(line) {
                let url = url.to_string();
                if !result.contains(&url) {
                    result.push(url);
                }
            }
        }
    }

    result
}

/// 从 HLS 文本里取最大的 `RESOLUTION=宽x高` 高度。
pub fn parse_best_resolution(text: &str) -> Option<u32> {
    let upper = text.to_ascii_uppercase();
    let mut best = 0u32;
    let mut search_from = 0;

    while let Some(relative) = upper[search_from..].find("RESOLUTION=") {
        let start = search_from + relative + "RESOLUTION=".len();
        let rest = &upper[start..];
        let Some(x_position) = rest.find('X') else {
            search_from = start;
            continue;
        };
        let width = &rest[..x_position];
        if width.is_empty() || !width.chars().all(|ch| ch.is_ascii_digit()) {
            search_from = start;
            continue;
        }
        let height_text: String = rest[x_position + 1..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if let Ok(height) = height_text.parse::<u32>() {
            best = best.max(height);
        }
        search_from = start;
    }

    if best > 0 {
        Some(best)
    } else {
        None
    }
}

/// 下载并解析自定义 M3U。返回原始文本与频道列表。
pub fn download_m3u(
    client: &Client,
    url: &str,
) -> Result<(String, Vec<crate::models::Channel>), String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("请输入以 http(s):// 开头的 M3U 地址".to_owned());
    }
    let text = fetch_text(client, url, MAX_PLAYLIST_BYTES)
        .map_err(|error| format!("下载失败：{error}"))?;
    if text.trim().is_empty() {
        return Err("下载失败：服务器返回空内容".to_owned());
    }
    let channels = crate::m3u::parse(&text, "自定义源");
    if channels.is_empty() {
        return Err("该地址不是有效的 M3U 播放列表".to_owned());
    }
    Ok((text, channels))
}

#[derive(Debug, Error)]
pub enum NetError {
    #[error("HTTP 请求失败：{0}")]
    Request(#[from] reqwest::Error),
    #[error("HTTP 状态码 {0}")]
    Status(u16),
    #[error("读取响应失败：{0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_best_resolution() {
        let text = "#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1920x1080\n\
                    #EXT-X-STREAM-INF:RESOLUTION=720x576\n";
        assert_eq!(parse_best_resolution(text), Some(1080));
        assert_eq!(parse_best_resolution("no resolution here"), None);
    }

    #[test]
    fn extracts_variant_urls() {
        let manifest = "#EXTM3U\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n\
            low/index.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1920x1080\n\
            http://cdn.example.com/high/index.m3u8\n";
        let urls = extract_variant_urls(manifest, "http://example.com/live/master.m3u8");
        assert_eq!(
            urls,
            vec![
                "http://example.com/live/low/index.m3u8".to_owned(),
                "http://cdn.example.com/high/index.m3u8".to_owned()
            ]
        );
    }
}

#[cfg(test)]
mod preflight_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// 启动一个只处理固定数量请求的极简 HTTP 服务器。
    fn serve(responses: Vec<(&'static str, &'static str)>) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("local addr");
        let expected = responses.len();
        let handle = thread::spawn(move || {
            for _ in 0..expected {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let body = responses
                    .iter()
                    .find(|(candidate, _)| *candidate == path)
                    .map(|(_, body)| *body)
                    .unwrap_or("");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn preflight_blocks_ad_manifest() {
        let (base, server) = serve(vec![(
            "/ad.m3u8",
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\nmkt/segment_1.ts\n",
        )]);
        let client = build_client("").expect("client");
        let outcome = preflight(&client, &format!("{base}/ad.m3u8"), true, "");
        assert_eq!(outcome.blocked.as_deref(), Some("ad segment path"));
        server.join().expect("server");
    }

    #[test]
    fn preflight_checks_master_and_variant() {
        let (base, server) = serve(vec![
            (
                "/master.m3u8",
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nvariant.m3u8\n",
            ),
            ("/variant.m3u8", "#EXTM3U\n#EXTINF:2,\nseg_0.ts\n"),
        ]);
        let client = build_client("").expect("client");
        let outcome = preflight(&client, &format!("{base}/master.m3u8"), true, "");
        assert!(outcome.blocked.is_none());
        assert!(outcome
            .final_url
            .as_deref()
            .unwrap_or("")
            .ends_with("/master.m3u8"));
        server.join().expect("server");
    }
}
