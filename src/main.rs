//! TouchIPTV 直播桌面版入口。
//!
//! 面向 Win32 低性能平板的轻量直播启动器：Rust + egui 负责界面，
//! 外部 VLC 负责解码。程序本身不加载网页、不使用 WebView。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adguard;
mod app;
mod channels;
mod config;
mod embedded;
mod jobs;
mod m3u;
mod models;
mod net;
mod player;
mod ui;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

use app::TouchIptvApp;

/// 应用持久化路径。
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
}

fn main() -> Result<(), eframe::Error> {
    let paths = app_paths();
    if let Err(error) = fs::create_dir_all(&paths.config_dir) {
        eprintln!("创建配置目录失败：{error}");
    }
    if let Err(error) = fs::create_dir_all(&paths.cache_dir) {
        eprintln!("创建缓存目录失败：{error}");
    }
    cleanup_old_playlists(&paths.cache_dir);

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([760.0, 500.0])
            .with_maximized(true)
            .with_title("TouchIPTV 直播"),
        ..Default::default()
    };

    eframe::run_native(
        "TouchIPTV 直播",
        native_options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Box::new(TouchIptvApp::new(cc, paths.clone()))
        }),
    )
}

/// 配置查找顺序：
/// 1. 当前工作目录的 config.toml（便携模式）；
/// 2. exe 同目录的 config.toml（双击启动时的便携模式）；
/// 3. 系统应用数据目录。
fn app_paths() -> AppPaths {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let mut candidates = vec![current_dir.clone()];
    if let Some(dir) = &exe_dir {
        if dir != &current_dir {
            candidates.push(dir.clone());
        }
    }

    for dir in candidates {
        let config_file = dir.join("config.toml");
        if config_file.exists() {
            return AppPaths {
                config_file,
                cache_dir: dir.join("cache"),
                config_dir: dir,
            };
        }
    }

    if let Some(project_dirs) = ProjectDirs::from("com", "gathin", "TouchIPTV") {
        let config_dir = project_dirs.config_dir().to_path_buf();
        let cache_dir = project_dirs.cache_dir().to_path_buf();
        return AppPaths {
            config_file: config_dir.join("config.toml"),
            config_dir,
            cache_dir,
        };
    }

    let root = exe_dir.unwrap_or(current_dir);
    AppPaths {
        config_file: root.join("config.toml"),
        config_dir: root.clone(),
        cache_dir: root.join("cache"),
    }
}

/// 清理一天前的 VLC 播放列表，避免长期使用后缓存目录堆积。
fn cleanup_old_playlists(cache_dir: &Path) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("playlist-") || !name.ends_with(".m3u") {
            continue;
        }
        let old = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .map(|elapsed| elapsed > Duration::from_secs(24 * 60 * 60))
            .unwrap_or(false);
        if old {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn configure_fonts(ctx: &egui::Context) {
    let Some(font_bytes) = load_cjk_font_bytes() else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk_ui".to_owned(), FontData::from_owned(font_bytes));

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, "cjk_ui".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

fn load_cjk_font_bytes() -> Option<Vec<u8>> {
    let mut candidates = vec![
        PathBuf::from("./DroidSansFallbackFull.ttf"),
        PathBuf::from("./fonts/DroidSansFallbackFull.ttf"),
        PathBuf::from("./NotoSansCJKsc-Regular.otf"),
        PathBuf::from("./NotoSansCJKjp-Regular.otf"),
    ];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join("DroidSansFallbackFull.ttf"));
        candidates.push(dir.join("fonts").join("DroidSansFallbackFull.ttf"));
    }

    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            return Some(bytes);
        }
    }

    Some(include_bytes!("../DroidSansFallbackFull.ttf").to_vec())
}
