//! 外部 VLC 播放器：路径探测、参数拼装、播放列表写入与进程启动。
//!
//! 本程序不在自己的窗口里解码视频。这样在 Win32 低配平板上可以把解码、
//! 硬解和 HLS 缓冲全部交给成熟的 VLC，主程序只负责频道浏览与启动。

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use thiserror::Error;

#[derive(Clone, Debug)]
pub struct VlcOptions {
    pub vlc_path: String,
    pub fullscreen: bool,
    pub network_caching_ms: u32,
    pub disable_hw_decode: bool,
    pub user_agent: String,
    pub referer: String,
}

#[derive(Clone, Debug)]
pub enum PlaybackTarget {
    Url { url: String, title: String },
    Playlist { path: PathBuf },
}

#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
}

/// 启动 VLC。返回子进程句柄，调用方负责在换台/停止时结束它。
pub fn launch_vlc(options: &VlcOptions, target: &PlaybackTarget) -> Result<Child, PlayerError> {
    let executable = validate_vlc_path(&options.vlc_path)?;
    let args = build_args(options, target);

    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(PlayerError::LaunchFailed)
}

/// 拼装 VLC 命令行。拆出来便于单元测试，也方便以后加参数。
pub fn build_args(options: &VlcOptions, target: &PlaybackTarget) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();

    args.push("--no-one-instance".into());
    args.push("--no-video-title-show".into());
    args.push("--play-and-exit".into());
    // 避免首次启动 VLC 时弹出隐私询问，挡住电视画面。
    args.push("--no-qt-privacy-ask".into());
    args.push(if options.fullscreen {
        "--fullscreen".into()
    } else {
        "--no-fullscreen".into()
    });
    args.push(
        format!(
            "--network-caching={}",
            options.network_caching_ms.min(60_000)
        )
        .into(),
    );

    if options.disable_hw_decode {
        args.push("--avcodec-hw=none".into());
    }
    if !options.referer.trim().is_empty() {
        args.push(format!("--http-referrer={}", options.referer.trim()).into());
    }
    if !options.user_agent.trim().is_empty() {
        args.push(format!("--http-user-agent={}", options.user_agent.trim()).into());
    }

    match target {
        PlaybackTarget::Url { url, title } => {
            if !title.trim().is_empty() {
                args.push("--meta-title".into());
                args.push(title.trim().into());
                args.push("--input-title-format".into());
                args.push(title.trim().into());
            }
            args.push(url.trim().into());
        }
        PlaybackTarget::Playlist { path } => {
            args.push(path.as_os_str().to_owned());
        }
    }

    args
}

/// 写入 VLC 可读的 M3U 播放列表。先写临时文件再替换，避免 VLC 读到一半。
pub fn write_playlist(path: &Path, entries: &[PlaylistEntry]) -> Result<(), PlayerError> {
    if entries.is_empty() {
        return Err(PlayerError::NoEntries);
    }

    let mut text = String::from("#EXTM3U\r\n");
    let mut written = 0usize;
    for entry in entries {
        let url = entry.url.trim();
        if url.is_empty() {
            continue;
        }
        text.push_str("#EXTINF:-1,");
        text.push_str(&sanitize_title(&entry.title));
        text.push_str("\r\n");
        text.push_str(url);
        text.push_str("\r\n");
        written += 1;
    }
    if written == 0 {
        return Err(PlayerError::NoEntries);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, text.as_bytes())?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(PlayerError::Io(error));
    }
    Ok(())
}

/// M3U 的标题在第一个逗号之后，标题本身带逗号容易让部分播放器解析错位。
fn sanitize_title(value: &str) -> String {
    let title = value
        .replace(['\r', '\n'], " ")
        .replace(',', "，")
        .trim()
        .to_owned();
    if title.is_empty() {
        "未命名频道".to_owned()
    } else {
        title
    }
}

/// 在常见安装位置和 PATH 里寻找 VLC。
pub fn find_vlc() -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(root) = std::env::var_os(variable) {
                candidates.push(
                    PathBuf::from(root)
                        .join("VideoLAN")
                        .join("VLC")
                        .join("vlc.exe"),
                );
            }
        }
        candidates.push(PathBuf::from(r"C:\Program Files\VideoLAN\VLC\vlc.exe"));
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
        ));
    }

    #[cfg(not(windows))]
    {
        candidates.push(PathBuf::from("/snap/bin/vlc"));
        candidates.push(PathBuf::from("/usr/bin/vlc"));
        candidates.push(PathBuf::from("/usr/local/bin/vlc"));
    }

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    let executable = if cfg!(windows) { "vlc.exe" } else { "vlc" };
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    None
}

pub fn validate_vlc_path(path: &str) -> Result<PathBuf, PlayerError> {
    let trimmed = path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return Err(PlayerError::MissingPath);
    }
    let path = PathBuf::from(trimmed);
    if !path.is_file() {
        return Err(PlayerError::MissingExecutable(trimmed.to_owned()));
    }
    Ok(path)
}

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("还没有配置 VLC 路径")]
    MissingPath,
    #[error("找不到 VLC 可执行文件：{0}")]
    MissingExecutable(String),
    #[error("启动 VLC 失败：{0}")]
    LaunchFailed(std::io::Error),
    #[error("播放列表为空")]
    NoEntries,
    #[error("写入播放列表失败：{0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> VlcOptions {
        VlcOptions {
            vlc_path: "vlc".to_owned(),
            fullscreen: true,
            network_caching_ms: 1500,
            disable_hw_decode: true,
            user_agent: "TouchIPTV/2.0".to_owned(),
            referer: "http://example.com/".to_owned(),
        }
    }

    #[test]
    fn builds_url_args() {
        let args = build_args(
            &options(),
            &PlaybackTarget::Url {
                url: "http://example.com/live.m3u8".to_owned(),
                title: "CCTV-1 综合".to_owned(),
            },
        );
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--fullscreen".to_owned()));
        assert!(args.contains(&"--network-caching=1500".to_owned()));
        assert!(args.contains(&"--avcodec-hw=none".to_owned()));
        assert!(args.contains(&"--http-referrer=http://example.com/".to_owned()));
        assert!(args.contains(&"http://example.com/live.m3u8".to_owned()));
        assert!(args.contains(&"CCTV-1 综合".to_owned()));
    }

    #[test]
    fn builds_playlist_args() {
        let args = build_args(
            &options(),
            &PlaybackTarget::Playlist {
                path: PathBuf::from(r"C:\cache\playlist.m3u"),
            },
        );
        let args: Vec<String> = args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&r"C:\cache\playlist.m3u".to_owned()));
        assert!(!args.contains(&"--meta-title".to_owned()));
    }

    #[test]
    fn sanitizes_playlist_titles() {
        assert_eq!(sanitize_title("A,B\r\nC"), "A，B  C");
    }

    #[test]
    fn playlist_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "touchiptv-playlist-test-{}.m3u",
            std::process::id()
        ));
        write_playlist(
            &path,
            &[
                PlaylistEntry {
                    title: "CCTV-1".to_owned(),
                    url: "http://example.com/1.m3u8".to_owned(),
                },
                PlaylistEntry {
                    title: "CCTV-2".to_owned(),
                    url: "http://example.com/2.m3u8".to_owned(),
                },
            ],
        )
        .expect("write playlist");
        let text = fs::read_to_string(&path).expect("read playlist");
        assert!(text.starts_with("#EXTM3U"));
        assert!(text.contains("#EXTINF:-1,CCTV-1"));
        assert!(text.contains("http://example.com/2.m3u8"));
        let _ = fs::remove_file(path);
    }
}
