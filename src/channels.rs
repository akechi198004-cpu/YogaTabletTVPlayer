//! 频道仓库：内置频道数据 + 用户导入的自定义 M3U。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use thiserror::Error;

use crate::m3u;
use crate::models::Channel;

static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

const BUNDLED_CHANNELS: &str = include_str!("../assets/channels.json");
pub const CUSTOM_SOURCE_FILE: &str = "custom_source.m3u";

#[derive(Debug, Deserialize)]
struct RawChannel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    source: String,
    #[serde(default)]
    lines: Vec<String>,
}

/// 读取内置频道数据。数据损坏时返回错误，由界面提示，不崩溃。
pub fn load_bundled() -> Result<Vec<Channel>, ChannelError> {
    let raw: Vec<RawChannel> = serde_json::from_str(BUNDLED_CHANNELS)?;
    let mut channels = Vec::with_capacity(raw.len());
    for item in raw {
        let name = item.name.trim();
        if name.is_empty() {
            continue;
        }

        let mut lines = Vec::new();
        for line in item.lines {
            let line = line.trim();
            if !line.is_empty() && !lines.iter().any(|existing| existing == line) {
                lines.push(line.to_owned());
            }
        }
        if lines.is_empty() && !item.url.trim().is_empty() {
            lines.push(item.url.trim().to_owned());
        }
        if lines.is_empty() {
            continue;
        }

        let id = if item.id.trim().is_empty() {
            name.to_owned()
        } else {
            item.id.trim().to_owned()
        };
        let mut channel = Channel::new(name, lines[0].clone());
        channel.id = id;
        channel.group = m3u::classify_group(name, &item.group);
        channel.source = if item.source.trim().is_empty() {
            "内置频道".to_owned()
        } else {
            item.source.trim().to_owned()
        };
        channel.resolution = item.resolution.trim().to_owned();
        channel.height = item.height;
        channel.lines = lines;
        channel.url = channel.lines[0].clone();
        channels.push(channel);
    }
    Ok(channels)
}

/// 自定义源缓存文件放在配置目录，便携模式下跟着 exe 走。
pub fn custom_source_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CUSTOM_SOURCE_FILE)
}

pub fn read_custom_source(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 载入内置频道并合并自定义源。返回的频道尚未套用收藏和测速状态。
pub fn load_all(custom_path: &Path) -> (Vec<Channel>, Option<String>, Option<String>) {
    let (bundled, mut warning) = match load_bundled() {
        Ok(channels) => (channels, None),
        Err(error) => (Vec::new(), Some(format!("内置频道数据载入失败：{error}"))),
    };

    let custom_text = read_custom_source(custom_path);
    let custom = match &custom_text {
        Some(text) => m3u::parse(text, "自定义源"),
        None => Vec::new(),
    };

    if custom_text.is_some() && custom.is_empty() && warning.is_none() {
        warning = Some("自定义源里没有解析到有效频道，已仅使用内置频道".to_owned());
    }

    let merged = m3u::merge(bundled, custom);
    (merged, custom_text, warning)
}

/// 套用收藏、上次测速结果。重新载入频道后保留用户已经测出来的数据。
pub fn apply_runtime_state(
    channels: &mut [Channel],
    favorites: &HashSet<String>,
    previous: &[Channel],
) {
    use std::collections::HashMap;
    let previous: HashMap<String, (crate::models::LatencyState, String, u32)> = previous
        .iter()
        .map(|channel| {
            (
                channel.key(),
                (channel.latency, channel.resolution.clone(), channel.height),
            )
        })
        .collect();

    for channel in channels.iter_mut() {
        let key = channel.key();
        channel.favorite = favorites.contains(&key);
        if let Some((latency, resolution, height)) = previous.get(&key) {
            channel.latency = *latency;
            // 测速过程中发现的实际画质也保留下来。
            if channel.resolution.is_empty() && !resolution.is_empty() {
                channel.resolution = resolution.clone();
                channel.height = *height;
            }
        }
    }
}

/// 原子写入自定义源文本。写入失败时旧文件保持完整。
pub fn save_custom_source(path: &Path, text: &str) -> Result<(), ChannelError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let sequence = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), sequence));

    let result = (|| -> Result<(), ChannelError> {
        use std::io::Write;
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub fn remove_custom_source(path: &Path) -> Result<(), ChannelError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ChannelError::Io(error)),
    }
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("读取或写入频道文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("内置频道 JSON 解析失败：{0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_channels_have_expected_count_and_shape() {
        let channels = load_bundled().expect("bundled channels");
        assert_eq!(channels.len(), 104);
        assert!(channels
            .iter()
            .any(|channel| channel.name.contains("CCTV-1")));
        assert!(channels.iter().all(|channel| !channel.lines.is_empty()));
    }

    #[test]
    fn custom_source_round_trip() {
        let root = std::env::temp_dir().join(format!(
            "touchiptv-channel-test-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let path = custom_source_path(&root);
        let text = "#EXTM3U\n#EXTINF:-1,测试台\nhttp://example.com/test.m3u8\n";
        save_custom_source(&path, text).expect("save");
        assert_eq!(read_custom_source(&path).as_deref(), Some(text));
        let (channels, custom, warning) = load_all(&path);
        assert!(warning.is_none());
        assert!(custom.is_some());
        assert!(channels.iter().any(|channel| channel.name == "测试台"));
        remove_custom_source(&path).expect("remove");
        let _ = fs::remove_dir_all(root);
    }
}
