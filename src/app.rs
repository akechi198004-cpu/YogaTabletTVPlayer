//! 应用状态与业务逻辑。
//!
//! 界面绘制在 `app_ui.rs`；这里只负责频道、配置、播放会话、后台任务和
//! 外部 VLC 进程的生命周期。所有网络操作都在后台线程里完成。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use reqwest::blocking::Client;

use crate::adguard;
use crate::channels;
use crate::config::AppConfig;
use crate::jobs::{self, JobEvent};
use crate::m3u;
use crate::models::{BatchState, Channel, LatencyState, PlayingState, PreflightOutcome, SpeedInfo};
use crate::net;
use crate::player::{self, PlaybackTarget, PlaylistEntry, VlcOptions};
use crate::ui::theme;
use crate::AppPaths;

#[path = "app_ui.rs"]
mod app_ui;

pub const FILTER_ALL: &str = "全部";
pub const FILTER_FAV: &str = "收藏";

const MAX_JOB_EVENTS_PER_FRAME: usize = 64;
const MAX_PLAYLIST_ENTRIES: usize = 500;
const BATCH_WORKERS: usize = 3;

/// 设置窗口的草稿。只有点击“保存”才会写回生效配置。
#[derive(Clone, Debug)]
pub struct SettingsDraft {
    pub vlc_path: String,
    pub source_input: String,
    pub fullscreen: bool,
    pub use_playlist: bool,
    pub network_caching_ms: u32,
    pub disable_hw_decode: bool,
    pub ad_guard: bool,
    pub auto_play_last: bool,
    pub user_agent: String,
    pub referer: String,
}

impl SettingsDraft {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            vlc_path: config.vlc_path.clone(),
            source_input: config.custom_source_url.clone(),
            fullscreen: config.fullscreen,
            use_playlist: config.use_playlist,
            network_caching_ms: config.network_caching_ms,
            disable_hw_decode: config.disable_hw_decode,
            ad_guard: config.ad_guard,
            auto_play_last: config.auto_play_last,
            user_agent: config.user_agent.clone(),
            referer: config.referer.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Save,
    Cancel,
    AutoDetect,
    Import,
    ClearSource,
    ConfirmClear,
    CancelClear,
}

pub(crate) struct PendingPreflight {
    token: u64,
    channel_key: String,
    line_index: usize,
    tried_lines: HashSet<usize>,
}

pub struct TouchIptvApp {
    pub(crate) config: AppConfig,
    pub(crate) config_path: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) custom_source_path: PathBuf,
    pub(crate) config_load_failed: bool,
    pub(crate) config_error: Option<String>,

    pub(crate) channels: Vec<Channel>,
    pub(crate) channel_index: HashMap<String, usize>,
    pub(crate) visible: Vec<usize>,
    pub(crate) chips: Vec<(String, usize)>,
    pub(crate) filter: String,
    pub(crate) query: String,
    pub(crate) sort_latency: bool,
    pub(crate) selected_key: Option<String>,

    pub(crate) playback: Option<PlayingState>,
    pub(crate) pending_preflight: Option<PendingPreflight>,
    pub(crate) last_error: Option<(String, String)>,
    pub(crate) last_preflight_warning: Option<String>,

    pub(crate) notice: String,
    pub(crate) notice_error: bool,
    pub(crate) settings: Option<SettingsDraft>,
    pub(crate) settings_message: Option<(String, bool)>,
    pub(crate) import_message: Option<(String, bool)>,
    pub(crate) importing: bool,
    pub(crate) confirm_clear_source: bool,
    pub(crate) custom_source_present: bool,

    pub(crate) speed_test_key: Option<String>,
    pub(crate) batch: Option<BatchState>,
    pub(crate) job_tx: Sender<JobEvent>,
    pub(crate) job_rx: Receiver<JobEvent>,
    pub(crate) client: Client,

    pub(crate) play_seq: u64,
    pub(crate) batch_seq: u64,
    pub(crate) scroll_to_current: bool,
    pub(crate) pending_autoplay: bool,
    pub(crate) last_playlist_path: Option<PathBuf>,
}

impl TouchIptvApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths) -> Self {
        theme::apply(&cc.egui_ctx);

        let (mut config, config_load_failed, config_error) =
            match AppConfig::load_or_default(&paths.config_file) {
                Ok(config) => (config, false, None),
                Err(error) => (
                    AppConfig::default(),
                    true,
                    Some(format!("配置文件读取失败：{error}")),
                ),
            };

        // 首次运行或路径留空时自动探测 VLC，用户仍然可以在设置里改写。
        if config.vlc_path.trim().is_empty() {
            if let Some(found) = player::find_vlc() {
                config.vlc_path = found;
            }
        }

        let client = match net::build_client(&config.user_agent) {
            Ok(client) => client,
            Err(error) => {
                eprintln!("创建 HTTP 客户端失败：{error}");
                Client::new()
            }
        };

        let custom_source_path = channels::custom_source_path(&paths.config_dir);
        let (mut loaded, custom_text, load_warning) = channels::load_all(&custom_source_path);
        let favorites: HashSet<String> = config.favorites.iter().cloned().collect();
        channels::apply_runtime_state(&mut loaded, &favorites, &[]);

        let (job_tx, job_rx) = mpsc::channel();
        let filter = config.last_filter.clone();

        let mut app = Self {
            config,
            config_path: paths.config_file,
            cache_dir: paths.cache_dir,
            custom_source_path,
            config_load_failed,
            config_error,
            channels: loaded,
            channel_index: HashMap::new(),
            visible: Vec::new(),
            chips: Vec::new(),
            filter,
            query: String::new(),
            sort_latency: false,
            selected_key: None,
            playback: None,
            pending_preflight: None,
            last_error: None,
            last_preflight_warning: None,
            notice: String::new(),
            notice_error: false,
            settings: None,
            settings_message: None,
            import_message: None,
            importing: false,
            confirm_clear_source: false,
            custom_source_present: custom_text.is_some(),
            speed_test_key: None,
            batch: None,
            job_tx,
            job_rx,
            client,
            play_seq: 0,
            batch_seq: 0,
            scroll_to_current: false,
            pending_autoplay: false,
            last_playlist_path: None,
        };

        app.channel_index = app
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.key(), index))
            .collect();
        app.ensure_filter_valid();
        app.rebuild_chips();
        app.rebuild_visible(false);
        app.ensure_selected(true);

        let mut warnings = Vec::new();
        if let Some(error) = &app.config_error {
            warnings.push(error.clone());
        }
        if let Some(warning) = load_warning {
            warnings.push(warning);
        }
        if !app.config.custom_source_url.trim().is_empty() && !app.custom_source_present {
            warnings.push("自定义源缓存不存在，请重新导入".to_owned());
        }
        if app.channels.is_empty() {
            warnings.push("没有可用频道，请检查内置数据或导入自定义源".to_owned());
        }
        if warnings.is_empty() {
            app.notice = "已就绪 · 点击频道开始播放".to_owned();
            app.notice_error = false;
        } else {
            app.notice = warnings.join("；");
            app.notice_error = true;
        }

        app.pending_autoplay = app.config.auto_play_last && app.selected_key.is_some();
        app
    }

    // ------------------------------------------------------------------ 频道

    pub(crate) fn rebuild_chips(&mut self) {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for channel in &self.channels {
            *counts.entry(channel.group.as_str()).or_default() += 1;
        }

        let favorites = self
            .channels
            .iter()
            .filter(|channel| channel.favorite)
            .count();
        let mut chips = vec![
            (FILTER_ALL.to_owned(), self.channels.len()),
            (FILTER_FAV.to_owned(), favorites),
        ];
        let mut seen: HashSet<String> = HashSet::new();

        for group in m3u::GROUP_ORDER {
            let count = counts.get(group).copied().unwrap_or(0);
            if count > 0 {
                chips.push(((*group).to_owned(), count));
                seen.insert((*group).to_owned());
            }
        }
        // 自定义源可能出现 GROUP_ORDER 之外的分类，按首次出现顺序补在末尾。
        for channel in &self.channels {
            let group = channel.group.trim();
            if group.is_empty() || seen.contains(group) {
                continue;
            }
            let count = counts.get(group).copied().unwrap_or(0);
            if count > 0 {
                chips.push((group.to_owned(), count));
                seen.insert(group.to_owned());
            }
        }

        self.chips = chips;
    }

    pub(crate) fn ensure_filter_valid(&mut self) {
        if self.filter == FILTER_ALL || self.filter == FILTER_FAV {
            return;
        }
        if self
            .channels
            .iter()
            .any(|channel| channel.group == self.filter)
        {
            return;
        }
        self.filter = FILTER_ALL.to_owned();
    }

    pub(crate) fn rebuild_visible(&mut self, scroll_to_current: bool) {
        let query = self.query.trim().to_lowercase();
        let all_filter = self.filter == FILTER_ALL;
        let favorite_filter = self.filter == FILTER_FAV;

        let mut visible = Vec::new();
        for (index, channel) in self.channels.iter().enumerate() {
            if favorite_filter && !channel.favorite {
                continue;
            }
            if !favorite_filter && !all_filter && channel.group != self.filter {
                continue;
            }
            if !query.is_empty() {
                let haystack = format!("{} {} {}", channel.name, channel.group, channel.resolution)
                    .to_lowercase();
                if !haystack.contains(&query) {
                    continue;
                }
            }
            visible.push(index);
        }

        if self.sort_latency {
            visible.sort_by_key(|&index| match self.channels[index].latency {
                LatencyState::Ok(ms) => (0u8, ms),
                LatencyState::Unknown => (1, 0),
                LatencyState::Failed => (2, 0),
            });
        }

        self.visible = visible;
        if scroll_to_current {
            self.scroll_to_current = true;
        }
    }

    pub(crate) fn ensure_selected(&mut self, scroll: bool) {
        let exists = self
            .selected_key
            .as_ref()
            .and_then(|key| self.channel_index.get(key))
            .is_some();
        if !exists {
            let mut key = None;
            if self
                .channel_index
                .contains_key(&self.config.last_channel_key)
            {
                key = Some(self.config.last_channel_key.clone());
            }
            if key.is_none() {
                if let Some(channel) = self
                    .channels
                    .iter()
                    .find(|channel| channel.name.contains("CCTV-1"))
                {
                    key = Some(channel.key());
                }
            }
            if key.is_none() {
                key = self.channels.first().map(Channel::key);
            }
            self.selected_key = key;
        }
        if scroll {
            self.scroll_to_current = true;
        }
    }

    pub(crate) fn selected_channel(&self) -> Option<&Channel> {
        self.selected_key
            .as_ref()
            .and_then(|key| self.channel_index.get(key))
            .and_then(|&index| self.channels.get(index))
    }

    pub(crate) fn playing_channel(&self) -> Option<&Channel> {
        self.playback
            .as_ref()
            .and_then(|state| self.channel_index.get(&state.channel_key))
            .and_then(|&index| self.channels.get(index))
    }

    pub(crate) fn reload_channels(&mut self) {
        let (mut loaded, custom_text, warning) = channels::load_all(&self.custom_source_path);
        let favorites: HashSet<String> = self.config.favorites.iter().cloned().collect();
        channels::apply_runtime_state(&mut loaded, &favorites, &self.channels);

        self.channels = loaded;
        self.custom_source_present = custom_text.is_some();
        self.channel_index = self
            .channels
            .iter()
            .enumerate()
            .map(|(index, channel)| (channel.key(), index))
            .collect();

        self.ensure_filter_valid();
        self.rebuild_chips();
        self.rebuild_visible(false);
        self.ensure_selected(true);

        if let Some(warning) = warning {
            self.notice = warning;
            self.notice_error = true;
        } else if self.channels.is_empty() {
            self.notice = "频道列表为空：请检查内置数据或重新导入直播源".to_owned();
            self.notice_error = true;
        } else {
            self.notice = "频道列表已重新载入".to_owned();
            self.notice_error = false;
        }
    }

    pub(crate) fn refresh_channels(&mut self) {
        self.reload_channels();
    }

    pub(crate) fn set_filter(&mut self, filter: String) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        self.config.last_filter = self.filter.clone();
        self.persist_config(false);
        self.rebuild_visible(true);
    }

    pub(crate) fn toggle_sort(&mut self) {
        self.sort_latency = !self.sort_latency;
        self.rebuild_visible(true);
    }

    pub(crate) fn toggle_favorite(&mut self, channel_key: &str) {
        let Some(&index) = self.channel_index.get(channel_key) else {
            return;
        };
        let name = self.channels[index].name.clone();
        let favorite = !self.channels[index].favorite;
        self.channels[index].favorite = favorite;

        let key = self.channels[index].key();
        if favorite {
            if !self.config.favorites.iter().any(|item| item == &key) {
                self.config.favorites.push(key);
            }
        } else {
            self.config.favorites.retain(|item| item != &key);
        }
        self.persist_config(false);
        self.rebuild_chips();
        if self.filter == FILTER_FAV {
            self.rebuild_visible(false);
        }
        self.notice = if favorite {
            format!("已收藏：{name}")
        } else {
            format!("已取消收藏：{name}")
        };
        self.notice_error = false;
    }

    // ------------------------------------------------------------------ 播放

    pub(crate) fn play_channel(&mut self, channel_key: String, ctx: &egui::Context) {
        self.start_play(channel_key, 0, HashSet::new(), ctx);
    }

    fn start_play(
        &mut self,
        channel_key: String,
        line_index: usize,
        tried_lines: HashSet<usize>,
        ctx: &egui::Context,
    ) {
        let Some(&index) = self.channel_index.get(&channel_key) else {
            self.fail_play("频道不存在", "频道列表可能已经刷新，请重新选择。");
            return;
        };
        let channel = self.channels[index].clone();
        if channel.lines.is_empty() {
            self.fail_play("无法播放", "该频道没有可用线路。");
            return;
        }
        let line_index = line_index.min(channel.line_count() - 1);
        if channel.line(line_index).trim().is_empty() {
            self.fail_play("无法播放", "该线路地址为空。");
            return;
        }

        self.kill_playback_process();
        self.play_seq = self.play_seq.wrapping_add(1);
        let token = self.play_seq;
        self.pending_preflight = None;
        self.last_error = None;
        self.last_preflight_warning = None;
        self.selected_key = Some(channel_key.clone());
        self.config.last_channel_key = channel_key.clone();
        self.persist_config(false);
        self.scroll_to_current = true;

        if self.config.ad_guard {
            let url = channel.line(line_index).to_owned();
            self.pending_preflight = Some(PendingPreflight {
                token,
                channel_key: channel_key.clone(),
                line_index,
                tried_lines,
            });
            self.notice = format!(
                "正在检查线路：{}（{}/{}）…",
                channel.name,
                line_index + 1,
                channel.line_count()
            );
            self.notice_error = false;
            jobs::spawn_preflight(
                self.client.clone(),
                url,
                self.config.referer.clone(),
                true,
                token,
                channel_key,
                line_index,
                self.job_tx.clone(),
                ctx.clone(),
            );
        } else {
            self.launch_playback(&channel, line_index, token, None, tried_lines);
        }
    }

    fn launch_playback(
        &mut self,
        channel: &Channel,
        line_index: usize,
        token: u64,
        url_override: Option<String>,
        tried_lines: HashSet<usize>,
    ) {
        if token != self.play_seq {
            return;
        }

        let url = url_override.unwrap_or_else(|| channel.line(line_index).to_owned());
        if url.trim().is_empty() {
            self.fail_play("无法播放", "播放地址为空。");
            return;
        }

        let mut vlc_path = self.config.vlc_path.trim().to_owned();
        if vlc_path.is_empty() {
            if let Some(found) = player::find_vlc() {
                vlc_path = found;
                self.config.vlc_path = vlc_path.clone();
                self.persist_config(false);
            }
        }
        if vlc_path.is_empty() {
            self.fail_play(
                "未找到 VLC",
                "请先安装 VLC，然后在设置里填写 vlc.exe 的完整路径。",
            );
            return;
        }
        if let Err(error) = player::validate_vlc_path(&vlc_path) {
            self.fail_play("无法启动 VLC", error.to_string());
            return;
        }

        let options = VlcOptions {
            vlc_path,
            fullscreen: self.config.fullscreen,
            network_caching_ms: self.config.network_caching_ms,
            disable_hw_decode: self.config.disable_hw_decode,
            user_agent: self.config.user_agent.clone(),
            referer: self.config.referer.clone(),
        };

        let mut warning = self.last_preflight_warning.take();
        let target = if self.config.use_playlist {
            let (entries, skipped) = self.build_playlist_entries(channel, &url);
            let path = self.cache_dir.join(format!("playlist-{token}.m3u"));
            match player::write_playlist(&path, &entries) {
                Ok(()) => {
                    if let Some(old) = self.last_playlist_path.replace(path.clone()) {
                        if old != path {
                            let _ = std::fs::remove_file(old);
                        }
                    }
                    if skipped > 0 {
                        let text = format!("已从 VLC 播放列表跳过 {skipped} 个已知广告源");
                        warning = Some(match warning {
                            Some(existing) => format!("{existing}；{text}"),
                            None => text,
                        });
                    }
                    PlaybackTarget::Playlist { path }
                }
                Err(error) => {
                    let text = format!("写入 VLC 播放列表失败，改为单地址播放：{error}");
                    warning = Some(match warning {
                        Some(existing) => format!("{existing}；{text}"),
                        None => text,
                    });
                    PlaybackTarget::Url {
                        url: url.clone(),
                        title: channel.name.clone(),
                    }
                }
            }
        } else {
            PlaybackTarget::Url {
                url: url.clone(),
                title: channel.name.clone(),
            }
        };

        match player::launch_vlc(&options, &target) {
            Ok(child) => {
                self.playback = Some(PlayingState {
                    channel_key: channel.key(),
                    channel_name: channel.name.clone(),
                    line_index,
                    tried_lines,
                    child,
                    started_at: Instant::now(),
                });
                self.pending_preflight = None;
                self.last_error = None;
                self.last_preflight_warning = warning;
                self.notice = format!(
                    "正在播放：{}（线路 {}/{}）",
                    channel.name,
                    line_index + 1,
                    channel.line_count()
                );
                self.notice_error = false;
                self.scroll_to_current = true;
            }
            Err(error) => {
                if let Some(path) = self.last_playlist_path.take() {
                    let _ = std::fs::remove_file(path);
                }
                self.fail_play("无法启动 VLC", error.to_string());
            }
        }
    }

    fn build_playlist_entries(
        &self,
        selected: &Channel,
        selected_url: &str,
    ) -> (Vec<PlaylistEntry>, usize) {
        let mut entries = vec![PlaylistEntry {
            title: selected.name.clone(),
            url: selected_url.to_owned(),
        }];
        let mut seen = HashSet::new();
        seen.insert(selected_url.to_owned());
        let selected_key = selected.key();
        let mut skipped = 0usize;

        for &index in &self.visible {
            if entries.len() >= MAX_PLAYLIST_ENTRIES {
                break;
            }
            let channel = &self.channels[index];
            if channel.key() == selected_key {
                continue;
            }
            let url = channel.line(0).trim();
            if url.is_empty() || !seen.insert(url.to_owned()) {
                continue;
            }
            if self.config.ad_guard && adguard::find_signature(url).is_some() {
                skipped += 1;
                continue;
            }
            entries.push(PlaylistEntry {
                title: channel.name.clone(),
                url: url.to_owned(),
            });
        }

        (entries, skipped)
    }

    pub(crate) fn stop_playback(&mut self) {
        if self.playback.is_none() && self.pending_preflight.is_none() {
            self.notice = "当前没有正在播放的频道".to_owned();
            self.notice_error = false;
            return;
        }
        self.play_seq = self.play_seq.wrapping_add(1);
        self.pending_preflight = None;
        self.kill_playback_process();
        self.last_error = None;
        self.last_preflight_warning = None;
        self.notice = "已停止播放".to_owned();
        self.notice_error = false;
    }

    pub(crate) fn retry_current(&mut self, ctx: &egui::Context) {
        if self.pending_preflight.is_some() {
            self.notice = "正在检查线路，请稍候…".to_owned();
            self.notice_error = false;
            return;
        }
        let key = self
            .playback
            .as_ref()
            .map(|state| state.channel_key.clone())
            .or_else(|| self.selected_key.clone());
        let line = self
            .playback
            .as_ref()
            .map(|state| state.line_index)
            .unwrap_or(0);
        if let Some(key) = key {
            self.start_play(key, line, HashSet::new(), ctx);
        } else {
            self.notice = "请先选择一个频道".to_owned();
            self.notice_error = false;
        }
    }

    pub(crate) fn step_channel(&mut self, delta: isize, ctx: &egui::Context) {
        if self.visible.is_empty() {
            self.notice = "当前分类下没有频道".to_owned();
            self.notice_error = false;
            return;
        }
        let current_key = self
            .playback
            .as_ref()
            .map(|state| state.channel_key.clone())
            .or_else(|| self.selected_key.clone());
        let position = current_key.and_then(|key| {
            self.visible
                .iter()
                .position(|&index| self.channels[index].key() == key)
        });
        let position = match position {
            Some(position) => position,
            None => {
                if delta > 0 {
                    self.visible.len() - 1
                } else {
                    0
                }
            }
        };
        let next = (position as isize + delta).rem_euclid(self.visible.len() as isize) as usize;
        let key = self.channels[self.visible[next]].key();
        self.start_play(key, 0, HashSet::new(), ctx);
    }

    pub(crate) fn switch_line(&mut self, ctx: &egui::Context) {
        let key = self
            .playback
            .as_ref()
            .map(|state| state.channel_key.clone())
            .or_else(|| self.selected_key.clone());
        let Some(key) = key else {
            self.notice = "请先选择一个频道".to_owned();
            self.notice_error = false;
            return;
        };
        let Some(&index) = self.channel_index.get(&key) else {
            return;
        };
        let count = self.channels[index].line_count();
        if count <= 1 {
            self.notice = "当前频道只有一条线路".to_owned();
            self.notice_error = false;
            return;
        }
        let line = self
            .playback
            .as_ref()
            .filter(|state| state.channel_key == key)
            .map(|state| state.line_index)
            .unwrap_or(0);
        let next = (line + 1) % count;
        self.start_play(key, next, HashSet::new(), ctx);
    }

    fn kill_playback_process(&mut self) {
        if let Some(mut state) = self.playback.take() {
            let _ = state.child.kill();
            let _ = state.child.wait();
        }
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn fail_play(&mut self, title: impl Into<String>, detail: impl Into<String>) {
        let title = title.into();
        let detail = detail.into();
        self.pending_preflight = None;
        self.last_preflight_warning = None;
        self.last_error = Some((title.clone(), detail.clone()));
        self.notice = format!("{title}：{detail}");
        self.notice_error = true;
    }

    fn poll_playback(&mut self, ctx: &egui::Context) {
        let exited = match self.playback.as_mut() {
            Some(state) => match state.child.try_wait() {
                Ok(Some(status)) => Some((
                    status,
                    state.started_at.elapsed(),
                    state.channel_key.clone(),
                    state.channel_name.clone(),
                    state.line_index,
                    state.tried_lines.clone(),
                )),
                _ => None,
            },
            None => None,
        };
        let Some((status, elapsed, key, name, line_index, mut tried)) = exited else {
            return;
        };

        self.playback = None;
        self.last_preflight_warning = None;
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }

        if !status.success() && elapsed < Duration::from_secs(5) {
            if let Some(&index) = self.channel_index.get(&key) {
                let channel = self.channels[index].clone();
                tried.insert(line_index);
                if let Some(next) = (0..channel.line_count()).find(|line| !tried.contains(line)) {
                    self.notice = format!(
                        "线路 {} 不可用，正在尝试线路 {}/{}",
                        line_index + 1,
                        next + 1,
                        channel.line_count()
                    );
                    self.notice_error = false;
                    self.start_play(key, next, tried, ctx);
                    return;
                }
            }
            self.last_error = Some((
                "VLC 已退出".to_owned(),
                "播放器启动后很快退出，可能是流地址不可用或解码失败。可重试或切换线路。".to_owned(),
            ));
            self.notice = format!("VLC 异常退出：{name}");
            self.notice_error = true;
        } else {
            self.notice = format!("VLC 已退出：{name}");
            self.notice_error = false;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ------------------------------------------------------------------ 测速

    pub(crate) fn start_single_speed(&mut self, channel_key: &str, ctx: &egui::Context) {
        if self.batch.is_some() {
            self.notice = "批量测速进行中，请稍候…".to_owned();
            self.notice_error = false;
            return;
        }
        if self.speed_test_key.is_some() {
            self.notice = "测速进行中，请稍候…".to_owned();
            self.notice_error = false;
            return;
        }
        let Some(&index) = self.channel_index.get(channel_key) else {
            return;
        };
        let url = self.channels[index].line(0).to_owned();
        if url.trim().is_empty() {
            self.notice = "该频道没有可测速的地址".to_owned();
            self.notice_error = true;
            return;
        }
        let name = self.channels[index].name.clone();
        self.speed_test_key = Some(channel_key.to_owned());
        self.notice = format!("正在测速：{name} …");
        self.notice_error = false;
        jobs::spawn_speed_test(
            self.client.clone(),
            url,
            self.config.referer.clone(),
            channel_key.to_owned(),
            self.job_tx.clone(),
            ctx.clone(),
        );
    }

    pub(crate) fn start_batch_test(&mut self, ctx: &egui::Context) {
        if self.batch.is_some() {
            self.notice = "批量测速已经进行中".to_owned();
            self.notice_error = false;
            return;
        }
        if self.visible.is_empty() {
            self.notice = "当前分类下没有可测速的频道".to_owned();
            self.notice_error = false;
            return;
        }

        let items: Vec<(String, String)> = self
            .visible
            .iter()
            .filter_map(|&index| {
                let channel = &self.channels[index];
                let url = channel.line(0).trim();
                if url.is_empty() {
                    None
                } else {
                    Some((channel.key(), url.to_owned()))
                }
            })
            .collect();
        if items.is_empty() {
            self.notice = "当前分类下没有可测速的频道".to_owned();
            self.notice_error = false;
            return;
        }

        self.batch_seq = self.batch_seq.wrapping_add(1);
        let batch_id = self.batch_seq;
        let total = items.len();
        let cancel = Arc::new(AtomicBool::new(false));
        self.batch = Some(BatchState {
            id: batch_id,
            total,
            done: 0,
            cancel: cancel.clone(),
        });
        self.notice = format!("开始测速 {total} 个频道（并发 {BATCH_WORKERS} 路）…");
        self.notice_error = false;
        jobs::spawn_batch_test(
            self.client.clone(),
            items,
            self.config.referer.clone(),
            BATCH_WORKERS,
            batch_id,
            cancel,
            self.job_tx.clone(),
            ctx.clone(),
        );
    }

    pub(crate) fn cancel_batch(&mut self) {
        if let Some(batch) = self.batch.take() {
            batch.cancel.store(true, Ordering::Relaxed);
            self.notice = "已停止批量测速".to_owned();
            self.notice_error = false;
        }
    }

    fn apply_speed_result(
        &mut self,
        channel_key: &str,
        result: Result<SpeedInfo, String>,
        single: bool,
    ) {
        let Some(&index) = self.channel_index.get(channel_key) else {
            return;
        };
        let name = self.channels[index].name.clone();

        match result {
            Ok(info) => {
                let channel = &mut self.channels[index];
                channel.latency = LatencyState::Ok(info.latency_ms);
                if info.height > 0 {
                    channel.height = info.height;
                    channel.resolution = info.resolution;
                }
                if single {
                    self.notice = if info.height > 0 {
                        format!(
                            "测速完成：{name} · 延迟 {}ms · 画质 {}",
                            info.latency_ms, channel.resolution
                        )
                    } else {
                        format!("测速完成：{name} · 延迟 {}ms", info.latency_ms)
                    };
                    self.notice_error = false;
                }
            }
            Err(error) => {
                self.channels[index].latency = LatencyState::Failed;
                if single {
                    self.notice = format!("测速失败：{name} · {error}");
                    self.notice_error = true;
                }
            }
        }

        if self.sort_latency || !self.query.trim().is_empty() {
            self.rebuild_visible(false);
        }
    }

    // ------------------------------------------------------------------ 设置与导入

    pub(crate) fn open_settings(&mut self) {
        self.settings = Some(SettingsDraft::from_config(&self.config));
        self.settings_message = None;
        self.import_message = None;
        self.confirm_clear_source = false;
    }

    pub(crate) fn apply_settings_action(&mut self, action: SettingsAction, ctx: &egui::Context) {
        match action {
            SettingsAction::Save => self.save_settings(),
            SettingsAction::Cancel => {
                self.settings = None;
                self.settings_message = None;
                self.import_message = None;
                self.confirm_clear_source = false;
            }
            SettingsAction::AutoDetect => match player::find_vlc() {
                Some(found) => {
                    if let Some(draft) = self.settings.as_mut() {
                        draft.vlc_path = found.clone();
                    }
                    self.settings_message = Some((format!("已找到 VLC：{found}"), false));
                }
                None => {
                    self.settings_message =
                        Some(("未找到 VLC，请手动填写 vlc.exe 完整路径".to_owned(), true));
                }
            },
            SettingsAction::Import => {
                if self.importing {
                    return;
                }
                let Some(draft) = self.settings.as_ref() else {
                    return;
                };
                let url = draft.source_input.trim().to_owned();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    self.settings_message =
                        Some(("请输入以 http(s):// 开头的 M3U 地址".to_owned(), true));
                    return;
                }
                self.importing = true;
                self.import_message = Some(("正在下载并解析直播源…".to_owned(), false));
                self.settings_message = None;
                jobs::spawn_import(self.client.clone(), url, self.job_tx.clone(), ctx.clone());
            }
            SettingsAction::ClearSource => {
                if self.config.custom_source_url.trim().is_empty() && !self.custom_source_present {
                    self.settings_message = Some(("当前没有自定义源".to_owned(), false));
                } else {
                    self.confirm_clear_source = true;
                }
            }
            SettingsAction::ConfirmClear => {
                if let Err(error) = channels::remove_custom_source(&self.custom_source_path) {
                    self.settings_message = Some((format!("删除自定义源失败：{error}"), true));
                    return;
                }
                self.config.custom_source_url.clear();
                self.persist_config(true);
                self.reload_channels();
                self.confirm_clear_source = false;
                if let Some(draft) = self.settings.as_mut() {
                    draft.source_input.clear();
                }
                self.settings_message =
                    Some(("已移除自定义源，恢复内置频道列表".to_owned(), false));
            }
            SettingsAction::CancelClear => {
                self.confirm_clear_source = false;
            }
        }
    }

    fn save_settings(&mut self) {
        let Some(draft) = self.settings.clone() else {
            return;
        };

        let mut vlc_path = draft.vlc_path.trim().trim_matches('"').trim().to_owned();
        if vlc_path.is_empty() {
            match player::find_vlc() {
                Some(found) => vlc_path = found,
                None => {
                    self.settings_message =
                        Some(("未找到 VLC，请手动填写 vlc.exe 完整路径".to_owned(), true));
                    return;
                }
            }
        }
        if let Err(error) = player::validate_vlc_path(&vlc_path) {
            self.settings_message = Some((format!("VLC 路径无效：{error}"), true));
            return;
        }

        self.config.vlc_path = vlc_path;
        self.config.fullscreen = draft.fullscreen;
        self.config.use_playlist = draft.use_playlist;
        self.config.network_caching_ms = draft.network_caching_ms.min(60_000);
        self.config.disable_hw_decode = draft.disable_hw_decode;
        self.config.ad_guard = draft.ad_guard;
        self.config.auto_play_last = draft.auto_play_last;
        self.config.user_agent = draft.user_agent.trim().to_owned();
        self.config.referer = draft.referer.trim().to_owned();

        if let Err(error) = self.config.save(&self.config_path) {
            self.settings_message = Some((format!("保存配置失败：{error}"), true));
            return;
        }

        match net::build_client(&self.config.user_agent) {
            Ok(client) => self.client = client,
            Err(error) => {
                self.settings_message = Some((format!("重建 HTTP 客户端失败：{error}"), true));
                return;
            }
        }

        self.config_load_failed = false;
        self.config_error = None;
        self.settings = None;
        self.settings_message = None;
        self.import_message = None;
        self.confirm_clear_source = false;
        self.notice = "设置已保存".to_owned();
        self.notice_error = false;
    }

    fn handle_import(&mut self, url: String, result: Result<(String, usize), String>) {
        self.importing = false;
        match result {
            Ok((text, count)) => {
                if let Err(error) = channels::save_custom_source(&self.custom_source_path, &text) {
                    self.import_message = Some((format!("写入本地缓存失败：{error}"), true));
                    self.notice = format!("导入失败：{error}");
                    self.notice_error = true;
                    return;
                }

                self.config.custom_source_url = url.clone();
                self.persist_config(true);
                self.reload_channels();
                if let Some(draft) = self.settings.as_mut() {
                    draft.source_input = url;
                }
                let message = format!("已导入 {count} 个频道");
                self.import_message = Some((message.clone(), false));
                self.notice = message;
                self.notice_error = false;
            }
            Err(error) => {
                self.import_message = Some((error.clone(), true));
                self.notice = format!("导入失败：{error}");
                self.notice_error = true;
            }
        }
    }

    // ------------------------------------------------------------------ 配置持久化

    pub(crate) fn persist_config(&mut self, force: bool) {
        if self.config_load_failed && !force {
            return;
        }
        self.config.normalize();
        match self.config.save(&self.config_path) {
            Ok(()) => {
                if force {
                    self.config_load_failed = false;
                    self.config_error = None;
                }
            }
            Err(error) => {
                self.notice = format!("保存配置失败：{error}");
                self.notice_error = true;
            }
        }
    }

    // ------------------------------------------------------------------ 后台事件

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        for _ in 0..MAX_JOB_EVENTS_PER_FRAME {
            match self.job_rx.try_recv() {
                Ok(event) => self.handle_job(event, ctx),
                Err(_) => break,
            }
        }
    }

    fn handle_job(&mut self, event: JobEvent, ctx: &egui::Context) {
        match event {
            JobEvent::Speed {
                channel_key,
                batch_id,
                result,
            } => match batch_id {
                Some(batch_id) => {
                    if self.batch.as_ref().map(|batch| batch.id) != Some(batch_id) {
                        return;
                    }
                    self.apply_speed_result(&channel_key, result, false);
                }
                None => {
                    if self.speed_test_key.as_deref() != Some(channel_key.as_str()) {
                        return;
                    }
                    self.speed_test_key = None;
                    self.apply_speed_result(&channel_key, result, true);
                }
            },
            JobEvent::BatchProgress {
                batch_id,
                done,
                total,
            } => {
                if let Some(batch) = self.batch.as_mut() {
                    if batch.id == batch_id {
                        batch.done = done.min(total);
                        batch.total = total;
                    }
                }
            }
            JobEvent::BatchDone {
                batch_id,
                total,
                cancelled,
            } => {
                if self.batch.as_ref().map(|batch| batch.id) == Some(batch_id) {
                    self.batch = None;
                    self.notice = if cancelled {
                        "已停止批量测速".to_owned()
                    } else {
                        format!("批量测速完成，共 {total} 个频道")
                    };
                    self.notice_error = false;
                    if self.sort_latency {
                        self.rebuild_visible(false);
                    }
                }
            }
            JobEvent::Preflight {
                token,
                channel_key,
                line_index,
                outcome,
            } => self.handle_preflight(token, channel_key, line_index, outcome, ctx),
            JobEvent::Import { url, result } => self.handle_import(url, result),
        }
    }

    fn handle_preflight(
        &mut self,
        token: u64,
        channel_key: String,
        line_index: usize,
        outcome: PreflightOutcome,
        ctx: &egui::Context,
    ) {
        let matches = self.pending_preflight.as_ref().is_some_and(|pending| {
            pending.token == token
                && pending.channel_key == channel_key
                && pending.line_index == line_index
        });
        if !matches || token != self.play_seq {
            return;
        }
        let Some(pending) = self.pending_preflight.take() else {
            return;
        };
        let Some(&index) = self.channel_index.get(&channel_key) else {
            self.fail_play("频道不存在", "频道列表可能已经刷新，请重新选择。");
            return;
        };
        let channel = self.channels[index].clone();

        if let Some(signature) = outcome.blocked {
            self.last_preflight_warning = None;
            let mut tried = pending.tried_lines;
            tried.insert(line_index);
            if let Some(next) = (0..channel.line_count()).find(|line| !tried.contains(line)) {
                self.notice = format!(
                    "已拦截广告流（{signature}），正在尝试线路 {}/{}",
                    next + 1,
                    channel.line_count()
                );
                self.notice_error = false;
                self.start_play(channel_key, next, tried, ctx);
            } else {
                self.last_error = Some((
                    "广告流已拦截".to_owned(),
                    format!(
                        "命中特征：{signature}\n当前频道的线路都已尝试。可换台或导入其他直播源。"
                    ),
                ));
                self.notice = "已拦截疑似广告流".to_owned();
                self.notice_error = true;
            }
            return;
        }

        self.last_preflight_warning = outcome.warning;
        self.launch_playback(
            &channel,
            line_index,
            token,
            outcome.final_url,
            pending.tried_lines,
        );
    }

    pub(crate) fn header_status(&self) -> String {
        if let Some(batch) = &self.batch {
            return format!("测速 {}/{}", batch.done, batch.total);
        }
        if self.speed_test_key.is_some() {
            return "测速中…".to_owned();
        }
        if self.pending_preflight.is_some() {
            return "检查线路…".to_owned();
        }
        if self.playback.is_some() {
            return "VLC 播放中".to_owned();
        }
        format!("共 {} 频道", self.channels.len())
    }

    pub(crate) fn vlc_ready(&self) -> bool {
        !self.config.vlc_path.trim().is_empty() && Path::new(self.config.vlc_path.trim()).is_file()
    }
}

impl eframe::App for TouchIptvApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_playback(ctx);
        self.poll_jobs(ctx);

        if self.pending_autoplay {
            self.pending_autoplay = false;
            if let Some(key) = self.selected_key.clone() {
                self.start_play(key, 0, HashSet::new(), ctx);
            }
        }

        self.handle_shortcuts(ctx);
        self.draw(ctx);

        if self.playback.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

impl TouchIptvApp {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.stop_playback();
        }
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            self.step_channel(1, ctx);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            self.step_channel(-1, ctx);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            if let Some(key) = self.selected_key.clone() {
                self.play_channel(key, ctx);
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F5)) {
            self.refresh_channels();
        }

        // 与 Android 版一致：数字键 1-5 直接切到对应线路。
        let mut line_shortcut = None;
        for (key, line) in [
            (egui::Key::Num1, 0usize),
            (egui::Key::Num2, 1),
            (egui::Key::Num3, 2),
            (egui::Key::Num4, 3),
            (egui::Key::Num5, 4),
        ] {
            if ctx.input(|input| input.key_pressed(key)) {
                line_shortcut = Some(line);
            }
        }
        if let Some(line) = line_shortcut {
            let channel_key = self
                .playback
                .as_ref()
                .map(|state| state.channel_key.clone())
                .or_else(|| self.selected_key.clone());
            if let Some(channel_key) = channel_key {
                if let Some(&index) = self.channel_index.get(&channel_key) {
                    if line < self.channels[index].line_count() {
                        self.start_play(channel_key, line, HashSet::new(), ctx);
                    }
                }
            }
        }
    }
}
