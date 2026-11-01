//! M3U / M3U8 / TXT 直播源解析。
//!
//! 解析规则从 Android 版的 `M3uParser.java` 一比一移植，保证同一份自定义源
//! 在 Windows 版与 Android 版里落到相同的分类。

use std::collections::HashMap;

use crate::models::Channel;

/// 内置数据里出现的已知分类。顺序决定顶部胶囊的排列。
pub const KNOWN_GROUPS: &[&str] = &[
    "央视", "卫视", "苏州", "香港", "台湾", "日本", "国际", "纪录", "少儿", "测试",
];

/// 顶部胶囊的优先顺序（与 Android 版 MainActivity.GROUP_ORDER 一致）。
pub const GROUP_ORDER: &[&str] = &[
    "央视", "卫视", "苏州", "香港", "台湾", "日本", "国际", "纪录", "少儿", "体育", "影视", "新闻",
    "其他",
];

/// 根据频道名与原始分组推断分类。
pub fn classify_group(name: &str, group: &str) -> String {
    let group = group.trim();
    for known in KNOWN_GROUPS {
        if group == *known {
            return (*known).to_owned();
        }
    }

    let text = format!("{name} {group}").to_lowercase();
    if contains_any(&text, &["cctv", "央视", "中央"]) {
        return "央视".to_owned();
    }
    if contains_any(
        &text,
        &[
            "卫视",
            "湖南",
            "浙江",
            "江苏",
            "北京",
            "东方",
            "深圳",
            "广东",
            "山东",
            "安徽",
            "湖北",
            "四川",
            "天津",
            "河南",
            "江西",
            "辽宁",
            "河北",
            "黑龙江",
            "吉林",
            "重庆",
            "三沙",
            "东南",
            "厦门",
            "宁夏",
            "山西",
        ],
    ) {
        return "卫视".to_owned();
    }
    if text.contains("苏州") {
        return "苏州".to_owned();
    }
    if contains_any(&text, &["tvb", "翡翠", "黄金华剧", "美亚", "凤凰", "香港"]) {
        return "香港".to_owned();
    }
    if contains_any(&text, &["大立", "人间卫视", "tvbs", "台湾"]) {
        return "台湾".to_owned();
    }
    if contains_any(&text, &["nhk", "weathernews", "mbs", "日本"]) {
        return "日本".to_owned();
    }
    if contains_any(
        &text,
        &[
            "cnn",
            "bbc news",
            "fox",
            "cna",
            "nbc",
            "dw",
            "cnbc",
            "sky news",
            "scripps",
            "brics",
            "reuters",
            "usa today",
            "半岛",
            "国际",
        ],
    ) {
        return "国际".to_owned();
    }
    if contains_any(
        &text,
        &[
            "earth",
            "nature",
            "documentary",
            "纪录",
            "cgtn documentary",
            "wonder",
            "hit",
        ],
    ) {
        return "纪录".to_owned();
    }
    if contains_any(&text, &["少儿", "卡通", "animation", "cartoon", "cctv-14"]) {
        return "少儿".to_owned();
    }
    if contains_any(
        &text,
        &[
            "体育",
            "sport",
            "espn",
            "nba",
            "五星体育",
            "奥林匹克",
            "高尔夫",
            "cctv-5",
            "cctv-16",
        ],
    ) {
        return "体育".to_owned();
    }
    if contains_any(
        &text,
        &[
            "电影",
            "电视剧",
            "影视",
            "movie",
            "cinema",
            "剧场",
            "影院",
            "chc",
            "怀旧",
            "cctv-6",
            "cctv-8",
        ],
    ) {
        return "影视".to_owned();
    }
    if contains_any(
        &text,
        &[
            "新闻",
            "news",
            "财经",
            "business",
            "社会与法",
            "cctv-13",
            "cctv-2",
        ],
    ) {
        return "新闻".to_owned();
    }

    if group.is_empty() {
        "其他".to_owned()
    } else {
        group.to_owned()
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// 解析 M3U/M3U8 文本。
pub fn parse(text: &str, source_name: &str) -> Vec<Channel> {
    let text = text.trim_start_matches('\u{feff}');
    let mut result = Vec::new();

    let mut pending_name: Option<String> = None;
    let mut pending_id = String::new();
    let mut pending_group = String::new();
    let mut pending_resolution: Option<String> = None;
    let mut txt_group = String::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line.to_ascii_uppercase().starts_with("#EXTINF") {
            let comma = line.rfind(',').unwrap_or(line.len());
            let rest = if comma < line.len() {
                line[comma + 1..].trim()
            } else {
                ""
            };

            let mut resolution = attr(line, "resolution");
            if resolution.is_empty() {
                if let Some(found) = bracket_resolution(rest) {
                    resolution = found;
                }
            }

            let cleaned = remove_bracketed(rest);
            let cleaned = if cleaned.is_empty() {
                attr(line, "tvg-name")
            } else {
                cleaned
            };
            pending_name = Some(if cleaned.is_empty() {
                "未命名频道".to_owned()
            } else {
                cleaned
            });
            pending_id = attr(line, "tvg-id");
            pending_group = attr(line, "group-title");
            pending_resolution = if resolution.is_empty() {
                None
            } else {
                Some(resolution.to_uppercase())
            };
            continue;
        }

        if let Some(group) = line.strip_prefix("#EXTGRP:") {
            pending_group = group.trim().to_owned();
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        if let Some(group) = line.strip_suffix(",#genre#") {
            txt_group = group.trim().to_owned();
            continue;
        }

        let is_url = starts_with_http(line);

        if pending_name.is_none() && line.contains(',') && !is_url {
            if let Some((name, url)) = line.split_once(',') {
                let name = name.trim();
                let url = url.trim();
                if starts_with_http(url) {
                    let mut channel = Channel::new(name, url);
                    channel.group = classify_group(name, &txt_group);
                    channel.source = source_name.to_owned();
                    result.push(channel);
                }
            }
            continue;
        }

        if is_url {
            let name = pending_name
                .take()
                .unwrap_or_else(|| format!("频道 {}", result.len() + 1));
            let id = if pending_id.is_empty() {
                name.clone()
            } else {
                pending_id.clone()
            };
            let group_source = if pending_group.is_empty() {
                &txt_group
            } else {
                &pending_group
            };
            let mut channel = Channel::new(name.clone(), line);
            channel.id = id;
            channel.group = classify_group(&name, group_source);
            channel.source = source_name.to_owned();
            if let Some(resolution) = pending_resolution.take() {
                channel.height = resolution
                    .chars()
                    .filter(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .unwrap_or(0);
                channel.resolution = resolution;
            }
            result.push(channel);

            pending_name = None;
            pending_id.clear();
            pending_group.clear();
            pending_resolution = None;
        }
    }

    result
}

/// 合并重复频道，并把镜像地址累积到 `lines`。
pub fn merge(existing: Vec<Channel>, incoming: Vec<Channel>) -> Vec<Channel> {
    let mut order: Vec<Channel> = Vec::with_capacity(existing.len() + incoming.len());
    let mut index: HashMap<String, usize> = HashMap::new();

    for item in existing.into_iter().chain(incoming.into_iter()) {
        if item.url.is_empty() && item.lines.is_empty() {
            continue;
        }
        let mut item = item;
        if item.lines.is_empty() && !item.url.is_empty() {
            item.lines.push(item.url.clone());
        }
        let key = item.key();
        if let Some(&position) = index.get(&key) {
            let previous = &mut order[position];
            for line in &item.lines {
                if !line.is_empty() && !previous.lines.contains(line) {
                    previous.lines.push(line.clone());
                }
            }
            if previous.url.is_empty() {
                previous.url = item.url;
            }
            if previous.resolution.is_empty() && !item.resolution.is_empty() {
                previous.resolution = item.resolution;
                previous.height = item.height;
            }
            if !previous.source.is_empty() && !item.source.is_empty() {
                if !previous.source.contains(&item.source) {
                    previous.source.push_str(", ");
                    previous.source.push_str(&item.source);
                }
            } else if previous.source.is_empty() {
                previous.source = item.source;
            }
            if item.latency.millis().is_some() {
                let previous_ms = previous.latency.millis();
                if previous_ms.is_none() || item.latency.millis() < previous_ms {
                    previous.latency = item.latency;
                }
            }
        } else {
            index.insert(key, order.len());
            order.push(item);
        }
    }

    order
}

fn starts_with_http(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// 读取 `key="value"` 形式的扩展属性，大小写不敏感。
fn attr(line: &str, key: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let bytes = line.as_bytes();
    let mut search_from = 0;

    while let Some(relative) = lower[search_from..].find(&key_lower) {
        let start = search_from + relative;
        let after = start + key_lower.len();

        // 避免把 `x-tvg-id` 当成 `tvg-id`。
        if start > 0 {
            let previous = bytes[start - 1];
            if previous.is_ascii_alphanumeric() || previous == b'-' || previous == b'_' {
                search_from = after;
                continue;
            }
        }

        let mut cursor = after;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && (bytes[cursor] == b'"' || bytes[cursor] == b'\'') {
                let quote = bytes[cursor];
                cursor += 1;
                if let Some(end) = line[cursor..].find(quote as char) {
                    return line[cursor..cursor + end].to_owned();
                }
            } else {
                let end = line[cursor..]
                    .find(|ch: char| ch == ',' || ch.is_ascii_whitespace())
                    .map(|offset| cursor + offset)
                    .unwrap_or(bytes.len());
                return line[cursor..end].to_owned();
            }
        }

        search_from = after;
    }

    String::new()
}

/// 去掉名字里的 `[720P]`、`[高清]` 等方括号说明。
fn remove_bracketed(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 从名字里的 `[720P]` 提取分辨率。
fn bracket_resolution(value: &str) -> Option<String> {
    let start = value.find('[')?;
    let end = value[start + 1..].find(']')? + start + 1;
    let content = value[start + 1..end].trim();
    let digits: String = content
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &content[digits.len()..];
    if rest.starts_with('p') || rest.starts_with('P') {
        return Some(format!("{digits}P"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extinf_attributes_and_brackets() {
        let text = "#EXTM3U\n\
            #EXTINF:-1 tvg-id=\"cctv1\" tvg-name=\"CCTV-1\" group-title=\"央视\",CCTV-1 综合 [720P]\n\
            http://example.com/cctv1.m3u8\n";
        let channels = parse(text, "测试源");
        assert_eq!(channels.len(), 1);
        let channel = &channels[0];
        assert_eq!(channel.name, "CCTV-1 综合");
        assert_eq!(channel.id, "cctv1");
        assert_eq!(channel.group, "央视");
        assert_eq!(channel.resolution, "720P");
        assert_eq!(channel.height, 720);
        assert_eq!(channel.lines[0], "http://example.com/cctv1.m3u8");
    }

    #[test]
    fn parses_txt_genre_and_name_comma_url() {
        let text =
            "央视,#genre#\nCCTV-1,http://example.com/1.m3u8\nCCTV-2,http://example.com/2.m3u8\n";
        let channels = parse(text, "TXT");
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "CCTV-1");
        assert_eq!(channels[0].group, "央视");
        assert_eq!(channels[1].name, "CCTV-2");
    }

    #[test]
    fn merges_duplicate_channels_and_keeps_lines() {
        let first = parse("#EXTINF:-1 tvg-id=\"cctv1\",CCTV-1\nhttp://a/1.m3u8\n", "A");
        let second = parse(
            "#EXTINF:-1 tvg-id=\"cctv1\",CCTV-1 综合\nhttp://b/1.m3u8\n",
            "B",
        );
        let merged = merge(first, second);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].lines.len(), 2);
        assert!(merged[0].source.contains("A"));
        assert!(merged[0].source.contains("B"));
    }

    #[test]
    fn attr_is_case_insensitive_and_ignores_prefix() {
        let line = "#EXTINF:-1 X-TVG-ID=\"bad\" TVG-ID=\"good\" GROUP-TITLE='央视',名称";
        assert_eq!(attr(line, "tvg-id"), "good");
        assert_eq!(attr(line, "group-title"), "央视");
    }

    #[test]
    fn classifies_common_groups() {
        assert_eq!(classify_group("CCTV-1 综合", ""), "央视");
        assert_eq!(classify_group("湖南卫视", ""), "卫视");
        assert_eq!(classify_group("TVB 翡翠台", ""), "香港");
        assert_eq!(classify_group("NHK World", ""), "日本");
        assert_eq!(classify_group("自定义频道", "我的分类"), "我的分类");
    }

    #[test]
    fn strips_brackets_from_names() {
        assert_eq!(remove_bracketed("CCTV-1 综合 [720P]"), "CCTV-1 综合");
        assert_eq!(
            bracket_resolution("CCTV-1 [1080P 高清]").as_deref(),
            Some("1080P")
        );
    }
}
