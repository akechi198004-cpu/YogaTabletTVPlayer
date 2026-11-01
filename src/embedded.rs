//! 内置播放器后端。
//!
//! 选型：随包附带的 **mpv i686 Windows 运行时**（成熟播放器，不自行实现解码）。
//! 通过 `--wid` 把 mpv 嵌入应用自己的 Win32 子窗口，使用命名管道 JSON IPC
//! 控制播放、暂停、音量和停止；mpv/FFmpeg 负责 HLS / TS / FLV、硬件解码与
//! 网络缓冲。所有进程和 IPC 操作都在工作线程里，不阻塞 egui UI 线程。
//!
//! 内置播放不依赖 VLC/libVLC。非 Windows 平台或发布包缺少 mpv 时，
//! `available()` 返回 false；界面会给出明确提示，并允许用户主动选择外部
//! VLC 兼容模式，但不会静默启动外部程序。

// Windows 实现之外的分支在 Linux 上只用于单元测试，避免整模块出现 dead_code 噪声。
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum EmbeddedError {
    #[error("当前平台不支持内置播放器")]
    #[allow(dead_code)]
    Unsupported,
    #[error("未找到随包 mpv 运行时（mpv/mpv.exe）")]
    MissingRuntime,
    #[error("内置播放器初始化失败：{0}")]
    Init(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOptions {
    pub url: String,
    pub user_agent: String,
    pub referer: String,
    pub disable_hw_decode: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stop,
    Error,
    Quit,
    Redirect,
}

#[derive(Clone, Debug)]
pub enum EmbeddedEvent {
    Loading {
        load_id: u64,
    },
    Playing {
        load_id: u64,
    },
    Ended {
        load_id: u64,
        reason: EndReason,
        error: Option<String>,
    },
    ProcessExited {
        load_id: u64,
        code: Option<i32>,
    },
    Error {
        load_id: u64,
        message: String,
    },
}

/// 随包 mpv 运行时的位置。Windows 发布包把它放在 exe 同目录的 `mpv/`。
pub fn find_mpv() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut roots = Vec::new();
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        {
            roots.push(exe_dir);
        }
        if let Ok(current) = std::env::current_dir() {
            roots.push(current);
        }
        find_mpv_in_roots(&roots)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 在给定根目录下按约定位置查找 mpv。拆出来便于单元测试。
fn find_mpv_in_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        for relative in ["mpv/mpv.exe", "mpv.exe", "vendor/mpv/mpv.exe"] {
            let candidate = root.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 当前平台/发布包是否具备内置播放器运行时。
pub fn available() -> bool {
    find_mpv().is_some()
}

/// 内置播放器句柄。工作线程在首次加载时才启动 mpv 进程。
pub struct EmbeddedPlayer {
    command_tx: Sender<PlayerCommand>,
    event_rx: Receiver<EmbeddedEvent>,
    worker: Option<thread::JoinHandle<()>>,
}

impl EmbeddedPlayer {
    pub fn new() -> Result<Self, EmbeddedError> {
        #[cfg(not(windows))]
        {
            Err(EmbeddedError::Unsupported)
        }

        #[cfg(windows)]
        {
            let mpv_path = find_mpv().ok_or(EmbeddedError::MissingRuntime)?;
            let (command_tx, command_rx) = mpsc::channel();
            let (event_tx, event_rx) = mpsc::channel();
            let worker = thread::Builder::new()
                .name("embedded-mpv".to_owned())
                .spawn(move || worker_loop(mpv_path, command_rx, event_tx))
                .map_err(|error| EmbeddedError::Init(error.to_string()))?;
            Ok(Self {
                command_tx,
                event_rx,
                worker: Some(worker),
            })
        }
    }

    /// 把 mpv 视频输出绑定到应用创建的 Win32 子窗口。
    pub fn set_video_window(&self, hwnd: isize) {
        let _ = self.command_tx.send(PlayerCommand::SetHwnd(hwnd));
    }

    pub fn load(&self, load_id: u64, options: LoadOptions) {
        let _ = self
            .command_tx
            .send(PlayerCommand::Load { load_id, options });
    }

    pub fn pause(&self, paused: bool) {
        let _ = self.command_tx.send(PlayerCommand::Pause(paused));
    }

    pub fn set_volume(&self, volume: u8) {
        let _ = self.command_tx.send(PlayerCommand::Volume(volume.min(100)));
    }

    pub fn try_recv(&self) -> Option<EmbeddedEvent> {
        self.event_rx.try_recv().ok()
    }

    /// 通知工作线程退出并等待清理。最多等待 2 秒，避免关闭应用时卡死。
    pub fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self
            .command_tx
            .send(PlayerCommand::Shutdown { ack: ack_tx });
        if ack_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
            let _ = worker.join();
        } else {
            // 清理超时：不要阻塞 UI 退出，交给进程终止回收。
            drop(worker);
        }
    }
}

impl Drop for EmbeddedPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 应用主窗口内承载 mpv 视频输出的子窗口。
pub struct VideoWindow {
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    parent: isize,
    #[cfg(windows)]
    last_bounds: Option<(i32, i32, i32, i32)>,
    #[cfg(windows)]
    visible: bool,
    #[cfg(not(windows))]
    _private: (),
}

impl Default for VideoWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoWindow {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            Self {
                hwnd: 0,
                parent: 0,
                last_bounds: None,
                visible: false,
            }
        }
        #[cfg(not(windows))]
        {
            Self { _private: () }
        }
    }

    #[cfg(windows)]
    pub fn ensure(&mut self, parent: isize) -> Result<(), EmbeddedError> {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
        };

        const SS_BLACKRECT: u32 = 0x0000_0004;

        if self.hwnd != 0 && self.parent == parent {
            return Ok(());
        }
        self.destroy();
        if parent == 0 {
            return Err(EmbeddedError::Init("父窗口句柄为空".to_owned()));
        }

        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "\0".encode_utf16().collect();
        let style = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | SS_BLACKRECT;
        let hinstance = unsafe { GetModuleHandleW(null()) };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                class.as_ptr(),
                title.as_ptr(),
                style,
                0,
                0,
                1,
                1,
                parent as HWND,
                null_mut(),
                hinstance,
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(EmbeddedError::Init(format!(
                "创建视频子窗口失败：{}",
                std::io::Error::last_os_error()
            )));
        }
        self.hwnd = hwnd as isize;
        self.parent = parent;
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn ensure(&mut self, _parent: isize) -> Result<(), EmbeddedError> {
        Ok(())
    }

    pub fn hwnd(&self) -> isize {
        #[cfg(windows)]
        {
            self.hwnd
        }
        #[cfg(not(windows))]
        {
            0
        }
    }

    #[cfg(windows)]
    pub fn set_bounds(&mut self, bounds: Option<(i32, i32, i32, i32)>) {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, ShowWindow, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW,
        };

        let Some((x, y, width, height)) = bounds else {
            self.hide();
            return;
        };
        if self.hwnd == 0 {
            return;
        }
        let width = width.max(1);
        let height = height.max(1);
        if self.visible && self.last_bounds == Some((x, y, width, height)) {
            return;
        }
        let hwnd = self.hwnd as HWND;
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            ShowWindow(hwnd, SW_SHOW);
        }
        self.visible = true;
        self.last_bounds = Some((x, y, width, height));
    }

    #[cfg(not(windows))]
    pub fn set_bounds(&mut self, _bounds: Option<(i32, i32, i32, i32)>) {}

    #[cfg(windows)]
    pub fn hide(&mut self) {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

        if self.hwnd != 0 {
            unsafe {
                ShowWindow(self.hwnd as HWND, SW_HIDE);
            }
        }
        self.visible = false;
        self.last_bounds = None;
    }

    #[cfg(not(windows))]
    pub fn hide(&mut self) {}

    #[cfg(windows)]
    pub fn destroy(&mut self) {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

        if self.hwnd != 0 {
            unsafe {
                DestroyWindow(self.hwnd as HWND);
            }
        }
        self.hwnd = 0;
        self.parent = 0;
        self.last_bounds = None;
        self.visible = false;
    }

    #[cfg(not(windows))]
    pub fn destroy(&mut self) {}
}

impl Drop for VideoWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------- 协议

#[derive(Debug)]
enum PlayerCommand {
    SetHwnd(isize),
    Load { load_id: u64, options: LoadOptions },
    Pause(bool),
    Volume(u8),
    Shutdown { ack: Sender<()> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MpvRawEvent {
    StartFile,
    FileLoaded,
    EndFile {
        reason: EndReason,
        error: Option<String>,
    },
    Idle,
    PipeClosed,
}

fn parse_mpv_line(line: &str) -> Option<MpvRawEvent> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event = value.get("event")?.as_str()?;
    match event {
        "start-file" => Some(MpvRawEvent::StartFile),
        "file-loaded" => Some(MpvRawEvent::FileLoaded),
        "end-file" => {
            let reason = match value
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("")
            {
                "eof" => EndReason::Eof,
                "stop" => EndReason::Stop,
                "quit" => EndReason::Quit,
                "error" => EndReason::Error,
                "redirect" => EndReason::Redirect,
                _ => EndReason::Eof,
            };
            let error = value
                .get("error")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            Some(MpvRawEvent::EndFile { reason, error })
        }
        "idle" => Some(MpvRawEvent::Idle),
        _ => None,
    }
}

/// Windows 命名管道路径必须是 `\\.\pipe\<name>`。
fn pipe_name(process_id: u32, sequence: u64) -> String {
    format!(r"\\.\pipe\touchiptv-{process_id}-{sequence}")
}

fn build_mpv_args(
    wid: isize,
    pipe: &str,
    options: &LoadOptions,
    volume: u8,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = [
        "--no-config",
        "--no-terminal",
        "--idle=yes",
        "--force-window=yes",
        "--keep-open=no",
        "--osc=no",
        "--osd-level=0",
        "--input-default-bindings=no",
        "--input-vo-keyboard=no",
        "--load-scripts=no",
        "--no-ytdl",
        "--cache=yes",
        "--demuxer-max-bytes=16MiB",
        "--demuxer-max-back-bytes=4MiB",
        "--network-timeout=15",
        "--background-color=#000000",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();

    args.push(format!("--wid={wid}").into());
    args.push(format!("--input-ipc-server={pipe}").into());
    args.push(format!("--volume={}", volume.min(100)).into());
    args.push(if options.disable_hw_decode {
        "--hwdec=no".into()
    } else {
        "--hwdec=auto-safe".into()
    });
    if !options.user_agent.trim().is_empty() {
        args.push(format!("--user-agent={}", options.user_agent.trim()).into());
    }
    if !options.referer.trim().is_empty() {
        args.push(format!("--referrer={}", options.referer.trim()).into());
    }
    args
}

// ---------------------------------------------------------------------- Windows 工作线程

#[cfg(windows)]
fn worker_loop(
    mpv_path: PathBuf,
    command_rx: Receiver<PlayerCommand>,
    event_tx: Sender<EmbeddedEvent>,
) {
    let mut worker = MpvWorker::new(mpv_path, command_rx, event_tx);
    worker.run();
}

#[cfg(windows)]
struct MpvWorker {
    mpv_path: PathBuf,
    command_rx: Receiver<PlayerCommand>,
    event_tx: Sender<EmbeddedEvent>,
    raw_tx: Sender<MpvRawEvent>,
    raw_rx: Receiver<MpvRawEvent>,
    hwnd: Option<isize>,
    child: Option<std::process::Child>,
    pipe: Option<std::fs::File>,
    reader: Option<thread::JoinHandle<()>>,
    current_options: Option<LoadOptions>,
    current_load_id: u64,
    volume: u8,
    pipe_seq: u64,
}

#[cfg(windows)]
impl MpvWorker {
    fn new(
        mpv_path: PathBuf,
        command_rx: Receiver<PlayerCommand>,
        event_tx: Sender<EmbeddedEvent>,
    ) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel();
        Self {
            mpv_path,
            command_rx,
            event_tx,
            raw_tx,
            raw_rx,
            hwnd: None,
            child: None,
            pipe: None,
            reader: None,
            current_options: None,
            current_load_id: 0,
            volume: 80,
            pipe_seq: 0,
        }
    }

    fn run(&mut self) {
        loop {
            match self.command_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PlayerCommand::Shutdown { ack }) => {
                    self.cleanup_process();
                    let _ = ack.send(());
                    break;
                }
                Ok(command) => self.handle_command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.cleanup_process();
                    break;
                }
            }
            self.drain_raw_events();
            self.check_process_exit();
        }
    }

    fn handle_command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::SetHwnd(hwnd) => {
                if self.hwnd != Some(hwnd) {
                    self.cleanup_process();
                    self.hwnd = Some(hwnd);
                }
            }
            PlayerCommand::Load { load_id, options } => self.handle_load(load_id, options),
            PlayerCommand::Pause(paused) => {
                let _ = self.send_command(serde_json::json!({
                    "command": ["set_property", "pause", paused]
                }));
            }
            PlayerCommand::Volume(volume) => {
                self.volume = volume.min(100);
                let _ = self.send_command(serde_json::json!({
                    "command": ["set_property", "volume", self.volume]
                }));
            }
            PlayerCommand::Shutdown { .. } => {}
        }
    }

    fn handle_load(&mut self, load_id: u64, options: LoadOptions) {
        if let Err(error) = self.ensure_process(&options) {
            self.send_event(EmbeddedEvent::Error {
                load_id,
                message: error,
            });
            return;
        }
        self.current_load_id = load_id;
        if let Err(error) = self.send_command(serde_json::json!({
            "command": ["set_property", "pause", false]
        })) {
            self.send_event(EmbeddedEvent::Error {
                load_id,
                message: error,
            });
            return;
        }
        if let Err(error) = self.send_command(serde_json::json!({
            "command": ["loadfile", options.url, "replace"]
        })) {
            self.send_event(EmbeddedEvent::Error {
                load_id,
                message: error,
            });
            return;
        }
        self.send_event(EmbeddedEvent::Loading { load_id });
    }

    fn ensure_process(&mut self, options: &LoadOptions) -> Result<(), String> {
        if self.child.is_some() && self.current_options.as_ref() == Some(options) {
            return Ok(());
        }
        self.cleanup_process();

        let hwnd = self.hwnd.ok_or_else(|| "视频窗口尚未就绪".to_owned())?;
        let pipe_name = pipe_name(std::process::id(), self.pipe_seq);
        self.pipe_seq = self.pipe_seq.wrapping_add(1);

        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let args = build_mpv_args(hwnd, &pipe_name, options, self.volume);
        let mut command = std::process::Command::new(&self.mpv_path);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let child = command
            .spawn()
            .map_err(|error| format!("启动 mpv 失败：{error}"))?;

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let pipe = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe_name)
            {
                Ok(file) => break file,
                Err(error) if std::time::Instant::now() < deadline => {
                    let _ = error;
                    thread::sleep(Duration::from_millis(40));
                }
                Err(error) => {
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("连接 mpv IPC 失败：{error}"));
                }
            }
        };

        let reader_file = pipe
            .try_clone()
            .map_err(|error| format!("复制 mpv IPC 句柄失败：{error}"))?;
        let raw_tx = self.raw_tx.clone();
        let reader = thread::Builder::new()
            .name("embedded-mpv-ipc".to_owned())
            .spawn(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(reader_file);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            if let Some(event) = parse_mpv_line(&line) {
                                let _ = raw_tx.send(event);
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = raw_tx.send(MpvRawEvent::PipeClosed);
            })
            .map_err(|error| format!("启动 mpv IPC 读取线程失败：{error}"))?;

        self.child = Some(child);
        self.pipe = Some(pipe);
        self.reader = Some(reader);
        self.current_options = Some(options.clone());
        Ok(())
    }

    fn send_command(&mut self, value: serde_json::Value) -> Result<(), String> {
        let pipe = self.pipe.as_mut().ok_or_else(|| "mpv 未启动".to_owned())?;
        use std::io::Write;
        let mut line = serde_json::to_string(&value).map_err(|error| error.to_string())?;
        line.push('\n');
        pipe.write_all(line.as_bytes())
            .map_err(|error| format!("写入 mpv IPC 失败：{error}"))?;
        pipe.flush()
            .map_err(|error| format!("刷新 mpv IPC 失败：{error}"))
    }

    fn send_event(&self, event: EmbeddedEvent) {
        let _ = self.event_tx.send(event);
    }

    fn drain_raw_events(&mut self) {
        while let Ok(raw) = self.raw_rx.try_recv() {
            let load_id = self.current_load_id;
            match raw {
                MpvRawEvent::StartFile => {
                    if load_id != 0 {
                        self.send_event(EmbeddedEvent::Loading { load_id });
                    }
                }
                MpvRawEvent::FileLoaded => {
                    if load_id != 0 {
                        self.send_event(EmbeddedEvent::Playing { load_id });
                    }
                }
                MpvRawEvent::EndFile { reason, error } => {
                    if load_id != 0 {
                        self.send_event(EmbeddedEvent::Ended {
                            load_id,
                            reason,
                            error,
                        });
                        self.current_load_id = 0;
                    }
                }
                MpvRawEvent::Idle | MpvRawEvent::PipeClosed => {}
            }
        }
    }

    fn check_process_exit(&mut self) {
        let exited = match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(status),
                _ => None,
            },
            None => None,
        };
        let Some(status) = exited else {
            return;
        };
        let load_id = self.current_load_id;
        self.child = None;
        self.close_pipe_and_reader();
        self.current_options = None;
        self.current_load_id = 0;
        if load_id != 0 {
            self.send_event(EmbeddedEvent::ProcessExited {
                load_id,
                code: status.code(),
            });
        }
    }

    fn cleanup_process(&mut self) {
        if self.child.is_some() {
            let _ = self.send_command(serde_json::json!({
                "command": ["quit"]
            }));
        }
        if let Some(mut child) = self.child.take() {
            let deadline = std::time::Instant::now() + Duration::from_millis(1500);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if std::time::Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(30));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        self.close_pipe_and_reader();
        self.current_options = None;
        self.current_load_id = 0;
    }

    fn close_pipe_and_reader(&mut self) {
        self.pipe = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpv_args_include_embedding_ipc_and_network_options() {
        let options = LoadOptions {
            url: "http://example.com/live.m3u8".to_owned(),
            user_agent: "TouchIPTV/2.0".to_owned(),
            referer: "http://example.com/".to_owned(),
            disable_hw_decode: true,
        };
        let args = build_mpv_args(1234, r"\\.\pipe\touchiptv-test", &options, 70);
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--wid=1234".to_owned()));
        assert!(args.contains(&r"--input-ipc-server=\\.\pipe\touchiptv-test".to_owned()));
        assert!(args.contains(&"--hwdec=no".to_owned()));
        assert!(args.contains(&"--user-agent=TouchIPTV/2.0".to_owned()));
        assert!(args.contains(&"--referrer=http://example.com/".to_owned()));
        assert!(args.contains(&"--volume=70".to_owned()));
    }

    #[test]
    fn builds_windows_named_pipe_path() {
        assert_eq!(pipe_name(1234, 7), r"\\.\pipe\touchiptv-1234-7".to_owned());
    }

    #[test]
    fn parses_mpv_events() {
        assert_eq!(
            parse_mpv_line(r#"{"event":"file-loaded"}"#),
            Some(MpvRawEvent::FileLoaded)
        );
        assert_eq!(
            parse_mpv_line(r#"{"event":"end-file","reason":"eof"}"#),
            Some(MpvRawEvent::EndFile {
                reason: EndReason::Eof,
                error: None,
            })
        );
        assert_eq!(
            parse_mpv_line(r#"{"event":"end-file","reason":"error","error":"failed"}"#),
            Some(MpvRawEvent::EndFile {
                reason: EndReason::Error,
                error: Some("failed".to_owned()),
            })
        );
        assert_eq!(parse_mpv_line(r#"{"event":"property-change"}"#), None);
    }

    #[test]
    fn embedded_backend_is_unavailable_without_runtime() {
        if cfg!(not(windows)) {
            assert!(!available());
            assert!(matches!(
                EmbeddedPlayer::new(),
                Err(EmbeddedError::Unsupported)
            ));
        }
    }

    #[test]
    fn finds_bundled_mpv_layouts() {
        let root = std::env::temp_dir().join(format!(
            "touchiptv-mpv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mpv_dir = root.join("mpv");
        std::fs::create_dir_all(&mpv_dir).expect("create mpv dir");
        let expected = mpv_dir.join("mpv.exe");
        std::fs::write(&expected, b"test").expect("write mpv");

        assert_eq!(
            find_mpv_in_roots(std::slice::from_ref(&root)),
            Some(expected.clone())
        );
        assert_eq!(
            find_mpv_in_roots(&[root.join("missing"), root.clone()]),
            Some(expected)
        );
        assert_eq!(find_mpv_in_roots(&[root.join("missing")]), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn video_window_stub_is_safe_on_non_windows() {
        if cfg!(not(windows)) {
            let mut window = VideoWindow::new();
            assert_eq!(window.hwnd(), 0);
            window.ensure(1).expect("stub ensure");
            window.set_bounds(Some((0, 0, 10, 10)));
            window.hide();
            window.destroy();
        }
    }
}
