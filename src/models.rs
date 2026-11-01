//! 直播频道与播放状态的公共数据结构。

use std::process::Child;
use std::time::Instant;

use crate::embedded::EmbeddedPlayer;

/// 一次测速的结果状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LatencyState {
    /// 还没测过。
    #[default]
    Unknown,
    /// 测速成功，单位毫秒。
    Ok(u32),
    /// 测速失败或超时。
    Failed,
}

impl LatencyState {
    pub fn millis(self) -> Option<u32> {
        match self {
            Self::Ok(ms) => Some(ms),
            _ => None,
        }
    }
}

/// 一个直播电视台。一个台可以有多条备用线路。
#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub name: String,
    pub id: String,
    pub group: String,
    pub source: String,
    pub resolution: String,
    pub height: u32,
    pub latency: LatencyState,
    pub favorite: bool,
    /// 首选线路，始终与 `lines[0]` 保持一致。
    pub url: String,
    pub lines: Vec<String>,
}

impl Channel {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        let name = name.into();
        let url = url.into();
        let mut lines = Vec::new();
        if !url.is_empty() {
            lines.push(url.clone());
        }
        Self {
            id: name.clone(),
            name,
            url,
            lines,
            ..Default::default()
        }
    }

    /// 取指定线路；越界时夹到合法范围。
    pub fn line(&self, index: usize) -> &str {
        if self.lines.is_empty() {
            return &self.url;
        }
        let index = index.min(self.lines.len() - 1);
        &self.lines[index]
    }

    pub fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    /// 收藏与合并自定义源时使用的稳定身份。
    /// 与 Android 版保持一致的规则：优先 tvg-id，去掉空格、间隔号、下划线、
    /// 连字符和括号，再转小写。
    pub fn key(&self) -> String {
        let raw = if self.id.chars().count() > 3 {
            self.id.as_str()
        } else {
            self.name.as_str()
        };
        raw.chars()
            .filter_map(|ch| {
                if matches!(ch, ' ' | '\u{00b7}' | '_' | '-' | '(' | ')' | '[' | ']') {
                    None
                } else {
                    Some(ch.to_lowercase().collect::<String>())
                }
            })
            .collect()
    }
}

/// 单频道测速结果。
#[derive(Clone, Debug, Default)]
pub struct SpeedInfo {
    pub latency_ms: u32,
    pub height: u32,
    pub resolution: String,
}

/// 后台预检结果。网络失败不会阻止播放，只会带一条提示。
#[derive(Clone, Debug, Default)]
pub struct PreflightOutcome {
    /// 命中的广告特征；命中时会阻止本次播放。
    pub blocked: Option<String>,
    /// 预检过程中的非致命提示。
    pub warning: Option<String>,
    /// 重定向后的最终地址，可直接交给 VLC。
    pub final_url: Option<String>,
}

/// 当前播放源：内置 mpv 子窗口，或外部 VLC 进程。
pub enum PlaybackSource {
    External(Child),
    Embedded(EmbeddedPlayer),
}

impl PlaybackSource {
    pub fn is_embedded(&self) -> bool {
        matches!(self, Self::Embedded(_))
    }
}

/// 正在播放的会话。两种后端共用频道/线路/启动时间等界面状态。
pub struct PlayingState {
    /// 启动本次播放时的序号，用于丢弃迟到的内置播放器事件。
    pub token: u64,
    pub channel_key: String,
    pub channel_name: String,
    pub line_index: usize,
    /// 本次播放已经尝试过的线路，外部 VLC 异常早退时用于自动顺延。
    pub tried_lines: std::collections::HashSet<usize>,
    /// 实际交给播放器的地址，内置失败时可用于外部 VLC 兼容播放。
    pub url: String,
    pub source: PlaybackSource,
    /// 仅内置播放器使用：当前是否处于暂停状态。
    pub paused: bool,
    /// 外部 VLC 是否由“改用 VLC 播放”按钮启动。
    /// 这种会话异常退出时不再自动顺延线路，避免又切回内置播放器。
    pub external_fallback: bool,
    pub started_at: Instant,
}

impl PlayingState {
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

/// 批量测速的界面状态。
pub struct BatchState {
    pub id: u64,
    pub total: usize,
    pub done: usize,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefers_long_id_and_strips_separators() {
        let mut channel = Channel::new("CCTV-1 综合", "http://example.com/1.m3u8");
        channel.id = "ch_1_CCTV_1_综合".to_owned();
        assert_eq!(channel.key(), "ch1cctv1综合");

        // 短 id 不可靠，退回频道名。
        channel.id = "x".to_owned();
        assert_eq!(channel.key(), "cctv1综合");
    }

    #[test]
    fn line_and_line_count_are_clamped() {
        let mut channel = Channel::new("A", "http://a");
        channel.lines.push("http://b".to_owned());
        assert_eq!(channel.line(0), "http://a");
        assert_eq!(channel.line(99), "http://b");
        assert_eq!(channel.line_count(), 2);
    }
}
