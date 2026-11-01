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

/// 在用户选择的 VLC 安装目录里寻找 vlc.exe。
///
/// 先检查常见布局（目录本身、`VLC\`、`VideoLAN\VLC\`），再在最多两层
/// 子目录里查找，兼容便携版解压到自定义文件夹的情况。
pub fn find_vlc_in_dir(dir: &Path) -> Option<PathBuf> {
    for candidate in [
        dir.join("vlc.exe"),
        dir.join("VLC").join("vlc.exe"),
        dir.join("VideoLAN").join("VLC").join("vlc.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((current, depth)) = pending.pop() {
        if depth >= 2 || visited >= 400 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let child = entry.path();
            let candidate = child.join("vlc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
            pending.push((child, depth + 1));
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
    let is_vlc = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("vlc.exe")
                || (!cfg!(windows) && name.eq_ignore_ascii_case("vlc"))
        });
    if !is_vlc {
        return Err(PlayerError::NotVlc(trimmed.to_owned()));
    }
    Ok(path)
}

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("还没有配置 VLC 路径")]
    MissingPath,
    #[error("找不到 VLC 可执行文件：{0}")]
    MissingExecutable(String),
    #[error("所选文件不是 vlc.exe：{0}")]
    NotVlc(String),
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "touchiptv-player-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn finds_vlc_in_common_and_nested_layouts() {
        let root = temp_dir("find-vlc");

        let direct = root.join("direct");
        fs::create_dir_all(&direct).expect("create direct dir");
        let direct_exe = direct.join("vlc.exe");
        fs::write(&direct_exe, b"test").expect("write direct vlc");
        assert_eq!(find_vlc_in_dir(&direct), Some(direct_exe));

        let nested = root.join("nested");
        let video_lan_vlc = nested.join("VideoLAN").join("VLC");
        fs::create_dir_all(&video_lan_vlc).expect("create VideoLAN dir");
        let nested_exe = video_lan_vlc.join("vlc.exe");
        fs::write(&nested_exe, b"test").expect("write nested vlc");
        assert_eq!(find_vlc_in_dir(&nested), Some(nested_exe));

        let portable = root.join("portable");
        let portable_vlc = portable.join("apps").join("vlc-3.0.21");
        fs::create_dir_all(&portable_vlc).expect("create portable dir");
        let portable_exe = portable_vlc.join("vlc.exe");
        fs::write(&portable_exe, b"test").expect("write portable vlc");
        assert_eq!(find_vlc_in_dir(&portable), Some(portable_exe));

        let empty = root.join("empty");
        fs::create_dir_all(&empty).expect("create empty dir");
        assert_eq!(find_vlc_in_dir(&empty), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn validates_vlc_executable_name() {
        let root = temp_dir("validate-vlc");
        let not_vlc = root.join("player.exe");
        fs::write(&not_vlc, b"test").expect("write other exe");
        assert!(matches!(
            validate_vlc_path(&not_vlc.to_string_lossy()),
            Err(PlayerError::NotVlc(_))
        ));

        let vlc = root.join("vlc.exe");
        fs::write(&vlc, b"test").expect("write vlc");
        assert_eq!(
            validate_vlc_path(&vlc.to_string_lossy()).expect("valid vlc"),
            vlc
        );

        assert!(matches!(
            validate_vlc_path(&root.join("missing.exe").to_string_lossy()),
            Err(PlayerError::MissingExecutable(_))
        ));
        let _ = fs::remove_dir_all(root);
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
