//! 已知广告流特征识别。
//!
//! 规则从 Android 版 `AdStreamDetector.java` 移植：
//! - 明确的广告播放列表路径；
//! - `mkt/segment_*.ts`、`byt/segment_*.ts`、`yss/segment_*.ts` 片段；
//! - `appadhw`、`zjstream2` 路径段；
//! - `a=tvzb0006` / `app=tvzb0006` 查询参数；
//! - HLS 清单里实际资源行的同类特征（不扫描标题和注释）。

use url::Url;

const AD_PLAYLIST_PATHS: &[&str] = &[
    "/applive/mkt.m3u8",
    "/applive/byt.m3u8",
    "/applive/yss.m3u8",
];

const URI_ATTRIBUTE_TAGS: &[&str] = &[
    "#EXT-X-KEY:",
    "#EXT-X-SESSION-KEY:",
    "#EXT-X-MAP:",
    "#EXT-X-MEDIA:",
    "#EXT-X-I-FRAME-STREAM-INF:",
    "#EXT-X-RENDITION-REPORT:",
    "#EXT-X-PRELOAD-HINT:",
    "#EXT-X-PART:",
    "#EXT-X-IMAGE-STREAM-INF:",
];

/// 检查一个播放地址是否直接命中已知广告特征。
pub fn find_signature(value: &str) -> Option<String> {
    let url = Url::parse(value.trim()).ok()?;
    find_resource_signature(&url)
}

/// 检查 HLS 清单正文。只扫描实际的 URI 行和 URI 属性，避免把
/// 标题里的“广告”二字误判成广告流。
pub fn find_manifest_signature(manifest: &str, manifest_url: &str) -> Option<String> {
    if manifest.is_empty() {
        return None;
    }
    let base = Url::parse(manifest_url.trim()).ok();

    for raw_line in manifest.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            if !is_uri_attribute_tag(line) {
                continue;
            }
            let Some(colon) = line.find(':') else {
                continue;
            };
            for resource in uri_attributes(&line[colon + 1..]) {
                if let Some(signature) = find_resolved_resource_signature(base.as_ref(), &resource)
                {
                    return Some(signature);
                }
            }
            continue;
        }

        if let Some(signature) = find_resolved_resource_signature(base.as_ref(), line) {
            return Some(signature);
        }
    }

    None
}

fn find_resolved_resource_signature(base: Option<&Url>, resource: &str) -> Option<String> {
    let resource = resource.trim();
    if resource.is_empty() {
        return None;
    }
    let resolved = match base {
        Some(base) => base.join(resource).ok()?,
        None => Url::parse(resource).ok()?,
    };
    find_resource_signature(&resolved)
}

fn find_resource_signature(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }

    let path = url.path().to_ascii_lowercase();
    for ad_path in AD_PLAYLIST_PATHS {
        if path == *ad_path {
            return Some((*ad_path).to_owned());
        }
    }

    if is_ad_segment_path(&path) {
        return Some("ad segment path".to_owned());
    }
    if has_exact_path_segment(&path, "appadhw") || has_exact_path_segment(&path, "zjstream2") {
        return Some("ad marker path".to_owned());
    }

    find_ad_query_signature(url)
}

fn is_ad_segment_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    for pair in segments.windows(2) {
        let directory = pair[0];
        let file = pair[1];
        if matches!(directory, "mkt" | "byt" | "yss")
            && file.starts_with("segment_")
            && file.ends_with(".ts")
            && file.len() > "segment_".len() + ".ts".len()
        {
            return true;
        }
    }
    false
}

fn has_exact_path_segment(path: &str, expected: &str) -> bool {
    path.split('/').any(|segment| segment == expected)
}

fn find_ad_query_signature(url: &Url) -> Option<String> {
    for (key, value) in url.query_pairs() {
        let key = key.as_ref();
        if (key == "a" || key == "app") && value.as_ref() == "tvzb0006" {
            return Some(format!("{key}=tvzb0006"));
        }
    }
    None
}

fn is_uri_attribute_tag(line: &str) -> bool {
    let bytes = line.as_bytes();
    URI_ATTRIBUTE_TAGS.iter().any(|tag| {
        bytes.len() >= tag.len() && bytes[..tag.len()].eq_ignore_ascii_case(tag.as_bytes())
    })
}

/// 从 `URI="..."` / `URI=...` 属性里提取资源地址。
fn uri_attributes(input: &str) -> Vec<String> {
    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let mut result = Vec::new();
    let mut search_from = 0;

    while let Some(relative) = lower[search_from..].find("uri") {
        let start = search_from + relative;
        let after = start + 3;

        let boundary_ok = start == 0
            || matches!(bytes[start - 1], b',' | b' ' | b'\t')
            || bytes[start - 1].is_ascii_whitespace();
        if !boundary_ok {
            search_from = after;
            continue;
        }

        let mut cursor = after;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            search_from = after;
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }

        if bytes[cursor] == b'"' || bytes[cursor] == b'\'' {
            let quote = bytes[cursor];
            cursor += 1;
            let end = input[cursor..]
                .find(quote as char)
                .map(|offset| cursor + offset)
                .unwrap_or(bytes.len());
            result.push(input[cursor..end].to_owned());
            search_from = (end + 1).min(bytes.len());
        } else {
            let end = input[cursor..]
                .find(|ch: char| ch == ',' || ch.is_ascii_whitespace())
                .map(|offset| cursor + offset)
                .unwrap_or(bytes.len());
            result.push(input[cursor..end].to_owned());
            search_from = end;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_playlist_paths() {
        assert_eq!(
            find_signature("http://example.com/applive/mkt.m3u8").as_deref(),
            Some("/applive/mkt.m3u8")
        );
        assert!(find_signature("http://example.com/live/cctv1.m3u8").is_none());
    }

    #[test]
    fn detects_ad_segments_and_markers() {
        assert!(find_signature("http://example.com/foo/mkt/segment_12.ts").is_some());
        assert!(find_signature("http://example.com/foo/byt/segment_abc.ts").is_some());
        assert!(find_signature("http://example.com/foo/appadhw/1.ts").is_some());
        assert!(find_signature("http://example.com/foo/zjstream2/index.m3u8").is_some());
        assert!(find_signature("http://example.com/foo/segment_12.ts").is_none());
    }

    #[test]
    fn detects_query_markers_after_decoding() {
        assert_eq!(
            find_signature("http://example.com/live?app=tvzb0006").as_deref(),
            Some("app=tvzb0006")
        );
        assert_eq!(
            find_signature("http://example.com/live?a=tvzb0006&x=1").as_deref(),
            Some("a=tvzb0006")
        );
        assert!(find_signature("http://example.com/live?a=tvzb0007").is_none());
    }

    #[test]
    fn scans_manifest_resources_not_titles() {
        let manifest = "#EXTM3U\n\
            #EXTINF:-1,广告时段\n\
            http://example.com/live/ok/segment_1.ts\n";
        assert!(find_manifest_signature(manifest, "http://example.com/master.m3u8").is_none());

        let manifest = "#EXTM3U\n\
            #EXTINF:-1,正常频道\n\
            http://example.com/foo/mkt/segment_1.ts\n";
        assert_eq!(
            find_manifest_signature(manifest, "http://example.com/master.m3u8").as_deref(),
            Some("ad segment path")
        );
    }

    #[test]
    fn scans_uri_attributes() {
        let manifest = "#EXTM3U\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"http://example.com/appadhw/key.bin\"\n\
            http://example.com/live/ok/segment_1.ts\n";
        assert_eq!(
            find_manifest_signature(manifest, "http://example.com/master.m3u8").as_deref(),
            Some("ad marker path")
        );
    }

    #[test]
    fn resolves_relative_resources() {
        let manifest = "#EXTM3U\n#EXTINF:-1,测试\nmkt/segment_9.ts\n";
        assert!(find_manifest_signature(manifest, "http://example.com/a/b.m3u8").is_some());
    }
}
