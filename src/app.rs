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
use crate::config::{AppConfig, PlayerBackend};
use crate::embedded::{self, EmbeddedEvent, EmbeddedPlayer, EndReason, LoadOptions, VideoWindow};
use crate::jobs::{self, JobEvent};
use crate::m3u;
use crate::models::{
    BatchState, Channel, LatencyState, PlaybackSource, PlayingState, PreflightOutcome, SpeedInfo,
};
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
    pub player_backend: PlayerBackend,
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
            player_backend: config.player_backend,
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
    BrowseVlc,
    BrowseVlcFolder,
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

/// 内置播放失败后，等用户明确选择才切换到外部 VLC。
#[derive(Clone)]
pub(crate) struct VlcFallback {
    channel_key: String,
    channel_name: String,
    line_index: usize,
    url: String,
    tried_lines: HashSet<usize>,
    token: u64,
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
    pub(crate) vlc_fallback: Option<VlcFallback>,
    /// 主窗口 HWND（仅 Windows；用于创建内置播放器视频子窗口）。
    window_hwnd: Option<isize>,
    /// 应用生命周期内长期持有的内置播放器工作线程。
    /// 换台、停止、全屏重启只投递命令，清理由后台串行完成。
    pub(crate) embedded: Option<EmbeddedPlayer>,
    /// 承载内置播放器画面的 Win32 子窗口；跨频道复用。
    pub(crate) video_window: VideoWindow,
    pub(crate) window_fullscreen: bool,
    /// 全屏/退出全屏后，等视频子窗口完成尺寸更新再重启 mpv，
    /// 避免 mpv 仍按旧窗口尺寸输出、周围留下空白。
    pending_embedded_resize: bool,
    pub(crate) last_error: Option<(String, String)>,
    pub(crate) last_preflight_warning: Option<String>,

    pub(crate) notice: String,
    pub(crate) notice_error: bool,
    pub(crate) settings: Option<SettingsDraft>,
    pub(crate) settings_message: Option<(String, bool)>,
    pub(crate) import_message: Option<(String, bool)>,
    pub(crate) importing: bool,
    /// 当前有效导入任务的有效性标识；清除源或开始新导入后旧任务失效。
    import_seq: u64,
    active_import: Option<u64>,
    pub(crate) confirm_clear_source: bool,
    pub(crate) custom_source_present: bool,

    pub(crate) speed_test_key: Option<String>,
    pub(crate) batch: Option<BatchState>,
    pub(crate) job_tx: Sender<JobEvent>,
    pub(crate) job_rx: Receiver<JobEvent>,
    pub(crate) client: Client,
    /// 仅测试使用：记录客户端成功替换的次数，便于验证失败时不替换。
    #[cfg(test)]
    pub(crate) client_generation: u64,

    pub(crate) play_seq: u64,
    pub(crate) batch_seq: u64,
    pub(crate) scroll_to_current: bool,
    pub(crate) pending_autoplay: bool,
    pub(crate) last_playlist_path: Option<PathBuf>,
}

#[cfg(windows)]
fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn window_handle(_cc: &eframe::CreationContext<'_>) -> Option<isize> {
    None
}

/// 退出时对外部 VLC 做有界等待；普通停止走后台回收线程。
fn terminate_external_child_bounded(child: &mut std::process::Child, timeout: Duration) {
    let _ = child.kill();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            _ if Instant::now() >= deadline => return,
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// 选择当前频道下一条尚未尝试的线路。
fn next_untried_line(line_count: usize, tried: &HashSet<usize>) -> Option<usize> {
    (0..line_count).find(|line| !tried.contains(line))
}

impl TouchIptvApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths) -> Self {
        theme::apply(&cc.egui_ctx);
        Self::build_with_window(paths, window_handle(cc))
    }

    /// 不依赖 egui 窗口的构造逻辑，便于回归测试直接构造应用状态。
    #[cfg(test)]
    fn build(paths: AppPaths) -> Self {
        Self::build_with_window(paths, None)
    }

    fn build_with_window(paths: AppPaths, window_hwnd: Option<isize>) -> Self {
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
            vlc_fallback: None,
            window_hwnd,
            embedded: if embedded::available() {
                EmbeddedPlayer::new().ok()
            } else {
                None
            },
            video_window: VideoWindow::new(),
            window_fullscreen: false,
            pending_embedded_resize: false,
            last_error: None,
            last_preflight_warning: None,
            notice: String::new(),
            notice_error: false,
            settings: None,
            settings_message: None,
            import_message: None,
            importing: false,
            import_seq: 0,
            active_import: None,
            confirm_clear_source: false,
            custom_source_present: custom_text.is_some(),
            speed_test_key: None,
            batch: None,
            job_tx,
            job_rx,
            client,
            #[cfg(test)]
            client_generation: 0,
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
        self.vlc_fallback = None;
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

        match self.config.player_backend {
            PlayerBackend::Embedded => {
                self.launch_embedded_playback(channel, line_index, token, url, tried_lines);
            }
            PlayerBackend::ExternalVlc => {
                self.vlc_fallback = None;
                self.launch_external_playback(
                    channel,
                    line_index,
                    token,
                    url,
                    tried_lines,
                    self.config.use_playlist,
                );
            }
        }
    }

    /// 启动内置播放器。失败时只记录可选的 VLC 回退，不自动启动外部进程。
    fn launch_embedded_playback(
        &mut self,
        channel: &Channel,
        line_index: usize,
        token: u64,
        url: String,
        tried_lines: HashSet<usize>,
    ) {
        if !embedded::available() {
            let detail = "当前发布包缺少内置播放器运行时（mpv/mpv.exe）。\
                         完整压缩包应包含 mpv 文件夹；也可改用外部 VLC 兼容模式。";
            self.register_vlc_fallback(
                channel,
                line_index,
                token,
                url,
                tried_lines,
                "内置播放器不可用",
                detail.to_owned(),
            );
            return;
        }

        let Some(parent_hwnd) = self.window_hwnd else {
            self.register_vlc_fallback(
                channel,
                line_index,
                token,
                url,
                tried_lines,
                "内置播放器不可用",
                "无法取得应用主窗口句柄。可改用外部 VLC 兼容模式。".to_owned(),
            );
            return;
        };
        if let Err(error) = self.video_window.ensure(parent_hwnd) {
            self.register_vlc_fallback(
                channel,
                line_index,
                token,
                url,
                tried_lines,
                "内置播放器不可用",
                format!("{error}。可改用外部 VLC 兼容模式。"),
            );
            return;
        }

        if self.embedded.is_none() {
            self.embedded = EmbeddedPlayer::new().ok();
        }
        let Some(player) = self.embedded.as_ref() else {
            self.register_vlc_fallback(
                channel,
                line_index,
                token,
                url,
                tried_lines,
                "内置播放器启动失败",
                "无法创建内置播放器工作线程。可改用外部 VLC 兼容模式。".to_owned(),
            );
            return;
        };
        player.set_video_window(self.video_window.hwnd());
        player.set_volume(self.config.embedded_volume);
        player.load(token, self.embedded_load_options(url.clone(), false));

        self.playback = Some(PlayingState {
            token,
            channel_key: channel.key(),
            channel_name: channel.name.clone(),
            line_index,
            tried_lines,
            url,
            source: PlaybackSource::Embedded,
            paused: false,
            external_fallback: false,
            started_at: Instant::now(),
        });
        self.pending_preflight = None;
        self.last_error = None;
        self.notice = format!(
            "正在连接：{}（线路 {}/{}）",
            channel.name,
            line_index + 1,
            channel.line_count()
        );
        self.notice_error = false;
        self.scroll_to_current = true;
    }

    /// 启动外部 VLC。兼容模式或用户明确选择回退时才会调用。
    fn launch_external_playback(
        &mut self,
        channel: &Channel,
        line_index: usize,
        token: u64,
        url: String,
        tried_lines: HashSet<usize>,
        use_playlist: bool,
    ) {
        let from_fallback = self.vlc_fallback.is_some();
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
                "请在设置中点击「选择安装目录」定位 VLC，或直接填写 vlc.exe 的完整路径。",
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
        let target = if use_playlist {
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
                self.video_window.hide();
                self.vlc_fallback = None;
                self.playback = Some(PlayingState {
                    token,
                    channel_key: channel.key(),
                    channel_name: channel.name.clone(),
                    line_index,
                    tried_lines,
                    url,
                    source: PlaybackSource::External(child),
                    paused: false,
                    external_fallback: from_fallback,
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

    /// 内置播放失败：保留线路地址，等待用户明确选择是否改用外部 VLC。
    #[allow(clippy::too_many_arguments)]
    fn register_vlc_fallback(
        &mut self,
        channel: &Channel,
        line_index: usize,
        token: u64,
        url: String,
        tried_lines: HashSet<usize>,
        title: impl Into<String>,
        detail: String,
    ) {
        let title = title.into();
        if let Some(player) = self.embedded.as_ref() {
            player.stop();
        }
        self.video_window.hide();
        self.pending_preflight = None;
        self.last_preflight_warning = None;
        self.vlc_fallback = Some(VlcFallback {
            channel_key: channel.key(),
            channel_name: channel.name.clone(),
            line_index,
            url,
            tried_lines,
            token,
        });
        self.notice = format!("{title}：已保留当前线路，可改用 VLC 播放");
        self.notice_error = true;
        self.last_error = Some((
            title,
            format!("{detail}\n\n不会自动启动外部 VLC；如需要，请点击「改用 VLC 播放」。"),
        ));
    }

    /// 用户点击回退按钮后，用同一线路启动外部 VLC。
    pub(crate) fn use_vlc_fallback(&mut self) {
        let Some(fallback) = self.vlc_fallback.clone() else {
            return;
        };
        if fallback.token != self.play_seq {
            self.vlc_fallback = None;
            return;
        }
        let Some(&index) = self.channel_index.get(&fallback.channel_key) else {
            self.vlc_fallback = None;
            self.fail_play("无法切换 VLC", "频道列表已刷新，请重新选择频道。");
            return;
        };
        let channel = self.channels[index].clone();
        self.notice = format!("正在改用外部 VLC 播放：{}", fallback.channel_name);
        self.notice_error = false;
        self.launch_external_playback(
            &channel,
            fallback.line_index,
            fallback.token,
            fallback.url,
            fallback.tried_lines,
            false,
        );
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

    pub(crate) fn stop_playback(&mut self, ctx: &egui::Context) {
        if self.playback.is_none() && self.pending_preflight.is_none() {
            self.notice = "当前没有正在播放的频道".to_owned();
            self.notice_error = false;
            return;
        }
        self.play_seq = self.play_seq.wrapping_add(1);
        self.pending_preflight = None;
        self.vlc_fallback = None;
        self.exit_window_fullscreen(ctx);
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

    /// 关闭应用时使用：有界等待内置播放器清理；外部 VLC 也只等待很短时间。
    /// 只在 `on_exit` 调用，普通换台/停止不会走这里。
    fn shutdown_playback_blocking(&mut self) {
        if let Some(mut state) = self.playback.take() {
            match &mut state.source {
                PlaybackSource::External(child) => {
                    terminate_external_child_bounded(child, Duration::from_millis(700));
                }
                PlaybackSource::Embedded => {}
            }
        }
        if let Some(mut player) = self.embedded.take() {
            player.shutdown_blocking(Duration::from_millis(1_500));
        }
        self.video_window.hide();
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }

    /// 换台、停止、失败顺延、全屏重启：只投递命令，绝不在 UI 线程等待
    /// 进程退出或线程 join。内置播放器由后台工作线程串行清理旧会话。
    fn kill_playback_process(&mut self) {
        if let Some(state) = self.playback.take() {
            match state.source {
                PlaybackSource::External(mut child) => {
                    // VLC 的 wait 也可能阻塞；交给后台回收线程。
                    let _ = child.kill();
                    let _ = std::thread::Builder::new()
                        .name("vlc-reaper".to_owned())
                        .spawn(move || {
                            let _ = child.wait();
                        });
                }
                PlaybackSource::Embedded => {
                    if let Some(player) = self.embedded.as_ref() {
                        player.stop();
                    }
                }
            }
        }
        self.video_window.hide();
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn embedded_load_options(&self, url: String, start_paused: bool) -> LoadOptions {
        LoadOptions {
            url,
            user_agent: self.config.user_agent.clone(),
            referer: self.config.referer.clone(),
            disable_hw_decode: self.config.disable_hw_decode,
            start_paused,
        }
    }

    fn fail_play(&mut self, title: impl Into<String>, detail: impl Into<String>) {
        let title = title.into();
        let detail = detail.into();
        self.video_window.hide();
        self.pending_preflight = None;
        self.last_preflight_warning = None;
        self.last_error = Some((title.clone(), detail.clone()));
        self.notice = format!("{title}：{detail}");
        self.notice_error = true;
    }

    fn poll_playback(&mut self, ctx: &egui::Context) {
        let mut embedded_events = Vec::new();
        let mut external_exit = None;
        // 即使当前没有 playback（例如失败后等待回退），也要排空事件队列，
        // 避免旧会话的迟到事件堆积。
        if let Some(player) = self.embedded.as_ref() {
            while let Some(event) = player.try_recv() {
                embedded_events.push(event);
            }
        }
        if let Some(state) = self.playback.as_mut() {
            if let PlaybackSource::External(child) = &mut state.source {
                if let Ok(Some(status)) = child.try_wait() {
                    external_exit = Some((
                        status,
                        state.started_at.elapsed(),
                        state.channel_key.clone(),
                        state.channel_name.clone(),
                        state.line_index,
                        state.tried_lines.clone(),
                        state.external_fallback,
                    ));
                }
            }
        }

        for event in embedded_events {
            self.handle_embedded_event(event, ctx);
        }

        let Some((status, elapsed, key, name, line_index, mut tried, from_fallback)) =
            external_exit
        else {
            return;
        };

        self.playback = None;
        self.last_preflight_warning = None;
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }

        if !status.success() && elapsed < Duration::from_secs(5) {
            if from_fallback {
                self.last_error = Some((
                    "VLC 已退出".to_owned(),
                    "改用 VLC 播放后播放器很快退出，可能是 VLC 路径、流地址或解码问题。\
                     不会自动切回内置播放器；可点击「重新连接」或切换频道。"
                        .to_owned(),
                ));
                self.notice = format!("外部 VLC 异常退出：{name}");
                self.notice_error = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                return;
            }
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

    fn handle_embedded_event(&mut self, event: EmbeddedEvent, ctx: &egui::Context) {
        // 只有当前会话确实是内置播放器时才处理事件；外部 VLC 可能复用
        // 失败回退时的同一个 token，不能把迟到的 mpv 事件当成 VLC 会话。
        let current_token = self
            .playback
            .as_ref()
            .filter(|state| state.source.is_embedded())
            .map(|state| state.token);
        let Some(current_token) = current_token else {
            return;
        };
        match event {
            EmbeddedEvent::Loading { load_id } => {
                if load_id == current_token {
                    self.notice = self
                        .playback
                        .as_ref()
                        .map(|state| format!("正在缓冲：{}", state.channel_name))
                        .unwrap_or_default();
                    self.notice_error = false;
                }
            }
            EmbeddedEvent::Playing { load_id } => {
                if load_id == current_token {
                    self.notice = self
                        .playback
                        .as_ref()
                        .map(|state| {
                            format!(
                                "内置播放中：{}（线路 {}/{}）",
                                state.channel_name,
                                state.line_index + 1,
                                self.playing_channel()
                                    .map(|channel| channel.line_count())
                                    .unwrap_or(1)
                            )
                        })
                        .unwrap_or_default();
                    self.notice_error = false;
                }
            }
            EmbeddedEvent::Error { load_id, message } => {
                if load_id == current_token {
                    if self.try_embedded_line_failover(ctx, "播放器报告错误") {
                        return;
                    }
                    self.fail_embedded_playback(
                        "内置播放失败",
                        format!("播放器报告错误：{message}"),
                        ctx,
                    );
                }
            }
            EmbeddedEvent::Ended {
                load_id,
                reason,
                error,
            } => {
                if load_id != current_token {
                    return;
                }
                match reason {
                    EndReason::Stop | EndReason::Quit | EndReason::Redirect => {}
                    EndReason::Eof => {
                        if self.try_embedded_line_failover(ctx, "直播流已结束") {
                            return;
                        }
                        self.fail_embedded_playback(
                            "直播流已结束",
                            "内置播放器已到达流末尾，可能是源暂时中断。".to_owned(),
                            ctx,
                        );
                    }
                    EndReason::Error => {
                        if self.try_embedded_line_failover(ctx, "无法解码或连接") {
                            return;
                        }
                        self.fail_embedded_playback(
                            "内置播放失败",
                            format!(
                                "无法解码或连接该直播流{}。",
                                error.map(|error| format!("：{error}")).unwrap_or_default()
                            ),
                            ctx,
                        );
                    }
                }
            }
            EmbeddedEvent::ProcessExited { load_id, code } => {
                if load_id == current_token {
                    if self.try_embedded_line_failover(ctx, "播放器进程退出") {
                        return;
                    }
                    self.fail_embedded_playback(
                        "内置播放器已退出",
                        format!(
                            "mpv 进程意外退出（退出码 {}）。",
                            code.map(|code| code.to_string())
                                .unwrap_or_else(|| "未知".to_owned())
                        ),
                        ctx,
                    );
                }
            }
        }
    }

    /// 内置播放失败时先尝试当前频道的下一条未试线路。
    fn try_embedded_line_failover(&mut self, ctx: &egui::Context, reason: &str) -> bool {
        let Some(state) = self.playback.as_ref() else {
            return false;
        };
        let key = state.channel_key.clone();
        let line_index = state.line_index;
        let mut tried = state.tried_lines.clone();
        let Some(&index) = self.channel_index.get(&key) else {
            return false;
        };
        let channel = self.channels[index].clone();
        tried.insert(line_index);
        if let Some(next) = next_untried_line(channel.line_count(), &tried) {
            self.notice = format!(
                "线路 {} 不可用（{}），正在尝试线路 {}/{}",
                line_index + 1,
                reason,
                next + 1,
                channel.line_count()
            );
            self.notice_error = false;
            self.start_play(key, next, tried, ctx);
            true
        } else {
            false
        }
    }

    fn fail_embedded_playback(&mut self, title: &str, detail: String, ctx: &egui::Context) {
        let Some(state) = self.playback.take() else {
            return;
        };
        if let Some(player) = self.embedded.as_ref() {
            player.stop();
        }
        self.video_window.hide();
        self.exit_window_fullscreen(ctx);
        self.last_preflight_warning = None;
        if let Some(path) = self.last_playlist_path.take() {
            let _ = std::fs::remove_file(path);
        }
        self.vlc_fallback = Some(VlcFallback {
            channel_key: state.channel_key.clone(),
            channel_name: state.channel_name.clone(),
            line_index: state.line_index,
            url: state.url.clone(),
            tried_lines: state.tried_lines,
            token: state.token,
        });
        self.last_error = Some((
            title.to_owned(),
            format!("{detail}\n\n不会自动启动外部 VLC；如需要，请点击「改用 VLC 播放」。"),
        ));
        self.notice = format!("{title}：已保留当前线路，可改用 VLC 播放");
        self.notice_error = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ------------------------------------------------------------------ 内置播放控制

    pub(crate) fn embedded_playing(&self) -> bool {
        self.playback
            .as_ref()
            .is_some_and(|state| state.source.is_embedded())
    }

    pub(crate) fn embedded_ready(&self) -> bool {
        embedded::available()
    }

    pub(crate) fn set_embedded_paused(&mut self, paused: bool) {
        let Some(state) = self.playback.as_ref() else {
            return;
        };
        if !state.source.is_embedded() {
            return;
        }
        let name = state.channel_name.clone();
        if let Some(player) = self.embedded.as_ref() {
            player.pause(paused);
        }
        if let Some(state) = self.playback.as_mut() {
            state.paused = paused;
        }
        self.notice = if paused {
            format!("已暂停：{name}")
        } else {
            format!("继续播放：{name}")
        };
        self.notice_error = false;
    }

    pub(crate) fn toggle_embedded_pause(&mut self) {
        if let Some(state) = self.playback.as_ref() {
            if state.source.is_embedded() {
                let paused = state.paused;
                self.set_embedded_paused(!paused);
            }
        }
    }

    pub(crate) fn set_embedded_volume(&mut self, volume: u8) {
        let volume = volume.min(100);
        self.config.embedded_volume = volume;
        if self.embedded_playing() {
            if let Some(player) = self.embedded.as_ref() {
                player.set_volume(volume);
            }
        }
    }

    /// 全屏切换后视频子窗口尺寸会变化；重启当前内置播放会话，
    /// 让 mpv 按新窗口尺寸创建视频输出。
    fn restart_embedded_after_resize(&mut self, ctx: &egui::Context) {
        let Some(state) = self.playback.as_ref() else {
            return;
        };
        if !state.source.is_embedded() {
            return;
        }
        let url = state.url.clone();
        // 全屏/退出全屏只重建视频输出，不能改变用户当前的暂停状态。
        let paused = state.paused;
        let Some(player) = self.embedded.as_ref() else {
            return;
        };
        self.play_seq = self.play_seq.wrapping_add(1);
        let token = self.play_seq;
        // 后台工作线程会先释放旧 mpv，再按新的子窗口尺寸启动新进程；
        // UI 线程只投递命令并更新会话 token。初始暂停状态随 Load 一起传入，
        // 新进程在加载文件前就保持暂停，避免短暂播放或出声。
        player.set_video_window(self.video_window.hwnd());
        player.load(token, self.embedded_load_options(url, paused));
        if let Some(state) = self.playback.as_mut() {
            state.token = token;
            state.paused = paused;
            state.started_at = Instant::now();
        }
        self.notice = if paused {
            "画面尺寸已更新，保持暂停…".to_owned()
        } else {
            "画面尺寸已更新，正在重新连接内置播放器…".to_owned()
        };
        self.notice_error = false;
        let _ = ctx;
    }

    pub(crate) fn toggle_window_fullscreen(&mut self, ctx: &egui::Context) {
        if !self.embedded_playing() {
            self.notice = "当前没有内置播放器画面可全屏".to_owned();
            self.notice_error = false;
            return;
        }
        self.window_fullscreen = !self.window_fullscreen;
        self.pending_embedded_resize = true;
        if self.window_fullscreen {
            // 全屏时由播放器底栏承担控制，关闭可能遮挡画面的设置窗口。
            self.settings = None;
            self.settings_message = None;
            self.confirm_clear_source = false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.window_fullscreen));
    }

    fn exit_window_fullscreen(&mut self, ctx: &egui::Context) {
        if self.window_fullscreen {
            self.window_fullscreen = false;
            self.pending_embedded_resize = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// 把视频子窗口定位到 egui 播放区。传入 None 或设置窗口打开时隐藏。
    pub(crate) fn update_video_bounds(&mut self, rect: Option<egui::Rect>, ctx: &egui::Context) {
        if !self.embedded_playing() || self.settings.is_some() {
            self.video_window.hide();
            return;
        }
        let Some(rect) = rect else {
            self.video_window.hide();
            return;
        };
        let ppp = ctx.pixels_per_point();
        let x = (rect.min.x * ppp).round() as i32;
        let y = (rect.min.y * ppp).round() as i32;
        let width = (rect.width() * ppp).round() as i32;
        let height = (rect.height() * ppp).round() as i32;
        if width > 0 && height > 0 {
            self.video_window.set_bounds(Some((x, y, width, height)));
        } else {
            self.video_window.hide();
        }
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

    /// 开始一次导入并返回有效性标识。旧任务的结果会因 token 不匹配被丢弃。
    fn begin_import(&mut self) -> u64 {
        self.import_seq = self.import_seq.wrapping_add(1);
        let token = self.import_seq;
        self.active_import = Some(token);
        self.importing = true;
        self.import_message = None;
        self.settings_message = None;
        token
    }

    /// 让当前在途导入失效，并同步 UI 的 importing 状态。
    fn invalidate_import(&mut self) {
        self.import_seq = self.import_seq.wrapping_add(1);
        self.active_import = None;
        self.importing = false;
    }

    pub(crate) fn open_settings(&mut self) {
        self.video_window.hide();
        self.settings = Some(SettingsDraft::from_config(&self.config));
        self.settings_message = None;
        // 导入进行中时保留进度状态；只有没有任务时才清掉旧结果。
        if !self.importing {
            self.import_message = None;
        }
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
            SettingsAction::BrowseVlc => self.browse_vlc(),
            SettingsAction::BrowseVlcFolder => self.browse_vlc_folder(),
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
                let token = self.begin_import();
                jobs::spawn_import(
                    self.client.clone(),
                    url,
                    token,
                    self.job_tx.clone(),
                    ctx.clone(),
                );
            }
            SettingsAction::ClearSource => {
                if self.config.custom_source_url.trim().is_empty() && !self.custom_source_present {
                    self.settings_message = Some(("当前没有自定义源".to_owned(), false));
                } else {
                    self.confirm_clear_source = true;
                }
            }
            SettingsAction::ConfirmClear => {
                // 先让所有在途导入失效，避免删除后又被迟到的下载结果恢复。
                self.invalidate_import();
                self.import_message = None;
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

    /// 原生文件选择器：优先直接选择 vlc.exe；结果只写入设置草稿。
    fn browse_vlc(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择 vlc.exe")
            .add_filter("VLC", &["exe"])
            .pick_file()
        else {
            // 用户取消：保持原路径不变，并清掉上一次的错误提示。
            self.settings_message = None;
            return;
        };
        self.apply_selected_vlc_path(path);
    }

    /// 目录选择：在所选 VLC 安装目录（或其上级）中查找 vlc.exe。
    fn browse_vlc_folder(&mut self) {
        let Some(directory) = rfd::FileDialog::new()
            .set_title("选择 VLC 安装目录")
            .pick_folder()
        else {
            self.settings_message = None;
            return;
        };
        self.apply_selected_vlc_path(directory);
    }

    fn apply_selected_vlc_path(&mut self, path: PathBuf) {
        let display = path.display().to_string();
        let selected = if path.is_dir() {
            player::find_vlc_in_dir(&path)
        } else {
            Some(path)
        };
        let Some(selected) = selected else {
            self.settings_message = Some((
                format!("所选位置未找到 vlc.exe：{display}。请选择 vlc.exe 或 VLC 安装目录。"),
                true,
            ));
            return;
        };
        let is_vlc = selected.is_file()
            && selected
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("vlc.exe"));
        if !is_vlc {
            self.settings_message = Some(("请选择名为 vlc.exe 的可执行文件".to_owned(), true));
            return;
        }
        if let Err(error) = player::validate_vlc_path(&selected.to_string_lossy()) {
            self.settings_message = Some((format!("VLC 校验失败：{error}"), true));
            return;
        }
        let Some(draft) = self.settings.as_mut() else {
            return;
        };
        draft.vlc_path = selected.to_string_lossy().into_owned();
        self.settings_message = Some((format!("已选择 VLC：{}", draft.vlc_path), false));
    }

    fn save_settings(&mut self) {
        let Some(draft) = self.settings.clone() else {
            return;
        };

        let mut vlc_path = draft.vlc_path.trim().trim_matches('"').trim().to_owned();
        if vlc_path.is_empty() && draft.player_backend == PlayerBackend::ExternalVlc {
            match player::find_vlc() {
                Some(found) => vlc_path = found,
                None => {
                    self.settings_message = Some((
                        "未找到 VLC，请点击「选择安装目录」定位 VLC".to_owned(),
                        true,
                    ));
                    return;
                }
            }
        }
        // 内置模式允许 VLC 路径为空；只有填写时才校验。
        if !vlc_path.is_empty() {
            if let Err(error) = player::validate_vlc_path(&vlc_path) {
                self.settings_message = Some((format!("VLC 路径无效：{error}"), true));
                return;
            }
        }

        // 先在候选配置上完成所有可能失败的步骤，全部成功后才替换运行状态。
        let mut candidate = self.config.clone();
        candidate.player_backend = draft.player_backend;
        candidate.vlc_path = vlc_path;
        candidate.fullscreen = draft.fullscreen;
        candidate.use_playlist = draft.use_playlist;
        candidate.network_caching_ms = draft.network_caching_ms.min(60_000);
        candidate.disable_hw_decode = draft.disable_hw_decode;
        candidate.ad_guard = draft.ad_guard;
        candidate.auto_play_last = draft.auto_play_last;
        candidate.user_agent = draft.user_agent.trim().to_owned();
        candidate.referer = draft.referer.trim().to_owned();
        candidate.normalize();

        let candidate_client = match net::build_client(&candidate.user_agent) {
            Ok(client) => client,
            Err(error) => {
                self.settings_message = Some((format!("重建 HTTP 客户端失败：{error}"), true));
                return;
            }
        };

        if let Err(error) = candidate.save(&self.config_path) {
            self.settings_message = Some((format!("保存配置失败：{error}"), true));
            return;
        }

        self.config = candidate;
        self.client = candidate_client;
        #[cfg(test)]
        {
            self.client_generation = self.client_generation.wrapping_add(1);
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

    fn handle_import(&mut self, token: u64, url: String, result: Result<(String, usize), String>) {
        // 迟到结果：清除源或开始了新导入后，旧任务不能再改缓存/配置/频道。
        if self.active_import != Some(token) {
            return;
        }
        self.active_import = None;
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
            JobEvent::Import { token, url, result } => self.handle_import(token, url, result),
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
        if self.embedded_playing() {
            return "内置播放中".to_owned();
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

        if self.pending_embedded_resize {
            self.pending_embedded_resize = false;
            self.restart_embedded_after_resize(ctx);
        }

        if self.playback.is_some() || self.pending_preflight.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 退出时做有界清理；内置播放器超时会终止作业对象，避免 mpv 残留。
        self.play_seq = self.play_seq.wrapping_add(1);
        self.shutdown_playback_blocking();
        self.video_window.destroy();
    }
}

impl TouchIptvApp {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.settings.is_some() || ctx.wants_keyboard_input() {
            return;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.window_fullscreen {
                self.exit_window_fullscreen(ctx);
            } else {
                self.stop_playback(ctx);
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Space)) && self.embedded_playing() {
            self.toggle_embedded_pause();
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F)) && self.embedded_playing() {
            self.toggle_window_fullscreen(ctx);
        }
        if self.embedded_playing() {
            if ctx.input(|input| {
                input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals)
            }) {
                let volume = self.config.embedded_volume.saturating_add(5).min(100);
                self.set_embedded_volume(volume);
            }
            if ctx.input(|input| input.key_pressed(egui::Key::Minus)) {
                let volume = self.config.embedded_volume.saturating_sub(5);
                self.set_embedded_volume(volume);
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::N)) {
            self.step_channel(1, ctx);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::P)) {
            self.step_channel(-1, ctx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU64;

    static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "touchiptv-app-test-{name}-{}-{}",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn dummy_vlc(dir: &Path) -> PathBuf {
        let path = dir.join("vlc.exe");
        fs::write(&path, b"test").expect("write dummy vlc");
        path
    }

    fn test_paths(dir: &Path, config_file: PathBuf) -> AppPaths {
        AppPaths {
            config_dir: dir.to_path_buf(),
            config_file,
            cache_dir: dir.join("cache"),
        }
    }

    fn settings_draft(app: &TouchIptvApp, vlc: &Path) -> SettingsDraft {
        let mut draft = SettingsDraft::from_config(&app.config);
        draft.vlc_path = vlc.to_string_lossy().into_owned();
        draft
    }

    #[test]
    fn fullscreen_restart_preserves_paused_state() {
        let dir = temp_dir("fullscreen-pause");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(test_paths(&dir, config_file));
        let (player, load_rx) = EmbeddedPlayer::new_test_recording();
        app.embedded = Some(player);
        app.playback = Some(PlayingState {
            token: 1,
            channel_key: "test".to_owned(),
            channel_name: "测试频道".to_owned(),
            line_index: 0,
            tried_lines: HashSet::new(),
            url: "http://example.com/live.m3u8".to_owned(),
            source: PlaybackSource::Embedded,
            paused: true,
            external_fallback: false,
            started_at: Instant::now(),
        });
        let ctx = egui::Context::default();

        // 暂停 -> 进入全屏 -> 后台重建播放器，必须继续保持暂停。
        app.toggle_window_fullscreen(&ctx);
        app.restart_embedded_after_resize(&ctx);
        let (_, options) = load_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("fullscreen restart load");
        assert!(options.start_paused, "fullscreen restart lost paused state");
        assert!(app.playback.as_ref().is_some_and(|state| state.paused));

        // 退出全屏再次重建，仍保持暂停。
        app.toggle_window_fullscreen(&ctx);
        app.restart_embedded_after_resize(&ctx);
        let (_, options) = load_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("exit-fullscreen restart load");
        assert!(
            options.start_paused,
            "exit-fullscreen restart lost paused state"
        );
        assert!(app.playback.as_ref().is_some_and(|state| state.paused));

        // 原本播放中的频道，全屏重建后仍以播放状态启动。
        if let Some(state) = app.playback.as_mut() {
            state.paused = false;
        }
        app.restart_embedded_after_resize(&ctx);
        let (_, options) = load_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("playing restart load");
        assert!(
            !options.start_paused,
            "playing restart must not stay paused"
        );
        assert!(app.playback.as_ref().is_some_and(|state| !state.paused));

        if let Some(mut player) = app.embedded.take() {
            player.shutdown_fast();
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn embedded_stop_does_not_wait_for_background_cleanup() {
        let dir = temp_dir("nonblocking-embedded");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(test_paths(&dir, config_file));
        // 假工作线程对每条命令延迟 450ms，模拟初始化/清理耗时超过 350ms。
        app.embedded = Some(EmbeddedPlayer::new_test_slow(Duration::from_millis(450)));
        app.playback = Some(PlayingState {
            token: 1,
            channel_key: "test".to_owned(),
            channel_name: "测试频道".to_owned(),
            line_index: 0,
            tried_lines: HashSet::new(),
            url: "http://example.com/live.m3u8".to_owned(),
            source: PlaybackSource::Embedded,
            paused: false,
            external_fallback: false,
            started_at: Instant::now(),
        });

        let start = Instant::now();
        app.kill_playback_process();
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "stop blocked UI thread: {:?}",
            start.elapsed()
        );
        assert!(app.playback.is_none());

        // 连续停止/换台只投递命令，同样不能等待后台。
        let start = Instant::now();
        app.kill_playback_process();
        if let Some(player) = app.embedded.as_ref() {
            player.stop();
        }
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "repeated stop blocked UI thread: {:?}",
            start.elapsed()
        );

        if let Some(mut player) = app.embedded.take() {
            player.shutdown_fast();
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_settings_keeps_state_when_config_save_fails() {
        let dir = temp_dir("save-unwritable");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"not a directory").expect("write blocker");
        let config_file = blocker.join("config.toml");
        let vlc = dummy_vlc(&dir);

        let mut app = TouchIptvApp::build(test_paths(&dir, config_file.clone()));
        app.config.vlc_path = vlc.to_string_lossy().into_owned();
        app.config.fullscreen = true;
        app.config.user_agent = "old-agent".to_owned();

        let original_config = app.config.clone();
        let original_generation = app.client_generation;
        let mut draft = settings_draft(&app, &vlc);
        draft.fullscreen = false;
        draft.user_agent = "new-agent".to_owned();
        app.settings = Some(draft);

        app.save_settings();

        assert_eq!(app.config, original_config);
        assert_eq!(app.client_generation, original_generation);
        assert!(app.settings.is_some(), "draft must be kept on failure");
        assert!(
            app.settings_message
                .as_ref()
                .is_some_and(|(_, error)| *error),
            "error message must be shown"
        );
        assert!(!config_file.exists(), "no config file may be written");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_settings_keeps_state_when_client_build_fails() {
        let dir = temp_dir("save-client");
        let config_file = dir.join("config.toml");
        let vlc = dummy_vlc(&dir);

        let initial = AppConfig {
            vlc_path: vlc.to_string_lossy().into_owned(),
            fullscreen: true,
            user_agent: "old-agent".to_owned(),
            ..AppConfig::default()
        };
        initial.save(&config_file).expect("save initial config");
        let disk_before = fs::read(&config_file).expect("read initial config");

        let mut app = TouchIptvApp::build(test_paths(&dir, config_file.clone()));
        let original_config = app.config.clone();
        let original_generation = app.client_generation;
        let mut draft = settings_draft(&app, &vlc);
        draft.fullscreen = false;
        // 换行符不是合法 HTTP HeaderValue，build_client 应失败。
        draft.user_agent = "bad\nagent".to_owned();
        app.settings = Some(draft);

        app.save_settings();

        assert_eq!(app.config, original_config);
        assert_eq!(app.client_generation, original_generation);
        assert_eq!(
            fs::read(&config_file).expect("read config after failure"),
            disk_before,
            "disk config must stay unchanged"
        );
        assert!(app.settings.is_some(), "draft must be kept on failure");
        assert!(
            app.settings_message
                .as_ref()
                .is_some_and(|(_, error)| *error),
            "error message must be shown"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_settings_applies_candidate_only_after_all_success() {
        let dir = temp_dir("save-success");
        let config_file = dir.join("config.toml");
        let vlc = dummy_vlc(&dir);

        let initial = AppConfig {
            vlc_path: vlc.to_string_lossy().into_owned(),
            fullscreen: true,
            user_agent: "old-agent".to_owned(),
            ..AppConfig::default()
        };
        initial.save(&config_file).expect("save initial config");

        let mut app = TouchIptvApp::build(test_paths(&dir, config_file.clone()));
        let original_generation = app.client_generation;
        let mut draft = settings_draft(&app, &vlc);
        draft.fullscreen = false;
        draft.user_agent = "new-agent".to_owned();
        app.settings = Some(draft);

        app.save_settings();

        assert!(!app.config.fullscreen);
        assert_eq!(app.config.user_agent, "new-agent");
        assert_eq!(app.client_generation, original_generation + 1);
        assert!(app.settings.is_none(), "settings close on success");
        let loaded = AppConfig::load_or_default(&config_file).expect("load saved config");
        assert_eq!(loaded, app.config);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn embedded_backend_can_save_without_vlc_path() {
        let dir = temp_dir("save-embedded");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(test_paths(&dir, config_file.clone()));
        app.config.vlc_path.clear();
        let mut draft = SettingsDraft::from_config(&app.config);
        draft.player_backend = PlayerBackend::Embedded;
        draft.vlc_path.clear();
        app.settings = Some(draft);

        app.save_settings();

        assert!(app.settings.is_none(), "settings should close on success");
        assert_eq!(app.config.player_backend, PlayerBackend::Embedded);
        assert!(app.config.vlc_path.is_empty());
        let loaded = AppConfig::load_or_default(&config_file).expect("load saved config");
        assert_eq!(loaded.player_backend, PlayerBackend::Embedded);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clearing_source_invalidates_in_flight_import() {
        let dir = temp_dir("import-clear");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(test_paths(&dir, config_file));

        let old_text =
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"old\",Old Source\nhttp://old.example/old.m3u8\n";
        channels::save_custom_source(&app.custom_source_path, old_text).expect("save old source");
        app.config.custom_source_url = "http://old.example/list.m3u".to_owned();
        app.reload_channels();
        assert!(app
            .channels
            .iter()
            .any(|channel| channel.name == "Old Source"));
        app.settings = Some(SettingsDraft::from_config(&app.config));

        let old_token = app.begin_import();
        assert!(app.importing);
        assert_eq!(app.active_import, Some(old_token));

        app.apply_settings_action(SettingsAction::ConfirmClear, &egui::Context::default());

        assert!(!app.importing, "clear must end importing state");
        assert!(app.active_import.is_none());
        assert!(!app.custom_source_path.exists(), "cache must be removed");
        assert!(app.config.custom_source_url.is_empty());
        assert!(
            app.channels
                .iter()
                .all(|channel| channel.name != "Old Source"),
            "channel list must fall back to bundled channels"
        );
        assert!(
            app.settings
                .as_ref()
                .is_some_and(|draft| draft.source_input.is_empty()),
            "draft source input must be cleared"
        );

        let late_text =
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"new\",New Source\nhttp://new.example/new.m3u8\n";
        app.handle_import(
            old_token,
            "http://old.example/list.m3u".to_owned(),
            Ok((late_text.to_owned(), 1)),
        );

        assert!(
            !app.custom_source_path.exists(),
            "late import must not recreate cache"
        );
        assert!(
            app.config.custom_source_url.is_empty(),
            "late import must not restore config url"
        );
        assert!(
            app.channels
                .iter()
                .all(|channel| channel.name != "New Source"),
            "late import must not restore channels"
        );
        assert!(!app.importing);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_import_keeps_newer_import_state_and_result_is_discarded() {
        let dir = temp_dir("import-stale");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(test_paths(&dir, config_file));

        let first_token = app.begin_import();
        let second_token = app.begin_import();
        assert_ne!(first_token, second_token);
        assert!(app.importing);
        assert_eq!(app.active_import, Some(second_token));

        let first_text =
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"one\",One Source\nhttp://one.example/one.m3u8\n";
        app.handle_import(
            first_token,
            "http://one.example/list.m3u".to_owned(),
            Ok((first_text.to_owned(), 1)),
        );

        assert!(app.importing, "stale result must not end newer import");
        assert_eq!(app.active_import, Some(second_token));
        assert!(
            app.channels
                .iter()
                .all(|channel| channel.name != "One Source"),
            "stale result must be discarded"
        );

        let second_text =
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"two\",Two Source\nhttp://two.example/two.m3u8\n";
        app.handle_import(
            second_token,
            "http://two.example/list.m3u".to_owned(),
            Ok((second_text.to_owned(), 1)),
        );

        assert!(!app.importing);
        assert!(app.active_import.is_none());
        assert_eq!(app.config.custom_source_url, "http://two.example/list.m3u");
        assert!(app
            .channels
            .iter()
            .any(|channel| channel.name == "Two Source"));
        assert_eq!(
            fs::read_to_string(&app.custom_source_path).expect("read imported cache"),
            second_text
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use std::fs;

    fn measure_settings_widths(screen: egui::Vec2, pixels_per_point: f32) -> Vec<f32> {
        let dir = std::env::temp_dir().join(format!(
            "touchiptv-layout-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(AppPaths {
            config_dir: dir.clone(),
            config_file,
            cache_dir: dir.join("cache"),
        });

        let mut draft = SettingsDraft::from_config(&app.config);
        draft.vlc_path = format!("C:\\{}", "very-long-directory\\".repeat(200));
        draft.source_input = format!("http://example.com/{}/playlist.m3u", "a".repeat(500));
        draft.user_agent = "A".repeat(500);
        draft.referer = "B".repeat(500);
        app.settings = Some(draft);
        app.settings_message = Some(("布局错误信息".repeat(80), true));
        app.import_message = Some(("导入结果信息".repeat(80), false));
        app.config.custom_source_url = format!("http://example.com/{}/list.m3u", "c".repeat(500));

        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        crate::ui::theme::apply(&ctx);
        ctx.set_pixels_per_point(pixels_per_point);

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        };

        let mut widths = Vec::new();
        for _ in 0..121 {
            let _ = ctx.run(input.clone(), |ctx| app.draw_settings_for_test(ctx));
            if let Some(rect) = ctx.memory(|memory| memory.area_rect(egui::Id::new("设置"))) {
                widths.push(rect.width());
            }
        }
        let _ = fs::remove_dir_all(dir);
        widths
    }

    fn assert_stable(widths: &[f32], expected_max: f32) {
        assert!(
            widths.len() >= 120,
            "settings window was not measured enough"
        );
        let stable = &widths[1..];
        let first = stable[0];
        for width in stable {
            assert!(
                (width - first).abs() < 0.01,
                "settings width drifted across frames: {widths:?}"
            );
        }
        assert!(
            first <= expected_max + 0.01,
            "settings window exceeded constrained width: {first} > {expected_max}"
        );
    }

    #[test]
    fn settings_width_is_stable_over_120_frames_at_multiple_sizes_and_dpi() {
        for (screen, ppp, expected_max) in [
            // fixed_size 约束的是内宽；Area 外框含少量边距/阴影。
            (egui::vec2(760.0, 500.0), 1.0, 640.0 + 16.0),
            (egui::vec2(1200.0, 760.0), 1.0, 640.0 + 16.0),
            (egui::vec2(760.0, 500.0), 1.5, 640.0 + 16.0),
            (egui::vec2(506.7, 333.3), 1.5, 475.0 + 16.0),
        ] {
            let widths = measure_settings_widths(screen, ppp);
            assert_stable(&widths, expected_max);
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use std::fs;

    #[test]
    fn next_untried_line_walks_all_lines_then_stops() {
        let mut tried = HashSet::new();
        assert_eq!(next_untried_line(3, &tried), Some(0));
        tried.insert(0);
        assert_eq!(next_untried_line(3, &tried), Some(1));
        tried.insert(1);
        assert_eq!(next_untried_line(3, &tried), Some(2));
        tried.insert(2);
        assert_eq!(next_untried_line(3, &tried), None);
    }

    #[test]
    fn browse_result_accepts_chinese_space_path_and_rejects_non_vlc() {
        let dir = std::env::temp_dir().join(format!(
            "touchiptv-browse-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let chinese = dir.join("中文 目录");
        fs::create_dir_all(&chinese).expect("create dir");
        let vlc = chinese.join("vlc.exe");
        fs::write(&vlc, b"test").expect("write vlc");

        let config_file = dir.join("config.toml");
        let mut app = TouchIptvApp::build(AppPaths {
            config_dir: dir.clone(),
            config_file,
            cache_dir: dir.join("cache"),
        });
        app.settings = Some(SettingsDraft::from_config(&app.config));

        app.apply_selected_vlc_path(chinese.clone());
        let selected = app
            .settings
            .as_ref()
            .map(|draft| draft.vlc_path.clone())
            .unwrap_or_default();
        assert_eq!(selected, vlc.to_string_lossy());

        let other = chinese.join("player.exe");
        fs::write(&other, b"test").expect("write other");
        app.apply_selected_vlc_path(other);
        let unchanged = app
            .settings
            .as_ref()
            .map(|draft| draft.vlc_path.clone())
            .unwrap_or_default();
        assert_eq!(unchanged, vlc.to_string_lossy());
        assert!(app
            .settings_message
            .as_ref()
            .is_some_and(|(_, error)| *error));
        let _ = fs::remove_dir_all(dir);
    }
}
