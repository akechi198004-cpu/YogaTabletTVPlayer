//! 配置读写。配置使用 TOML，原子保存，避免断电时留下半个文件。

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

static TEMP_CONFIG_SEQ: AtomicU64 = AtomicU64::new(0);

/// 用户可持久化的全部设置。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// VLC 可执行文件路径。为空时启动时自动探测。
    pub vlc_path: String,
    /// 上次播放的频道 key。
    pub last_channel_key: String,
    /// 上次使用的分类。
    pub last_filter: String,
    /// 收藏频道的 key 列表。
    pub favorites: Vec<String>,
    /// 自定义 M3U 源地址。
    pub custom_source_url: String,
    /// VLC 是否全屏播放。
    pub fullscreen: bool,
    /// 是否把当前筛选结果写成 VLC 播放列表。
    pub use_playlist: bool,
    /// VLC 网络缓存（毫秒）。
    pub network_caching_ms: u32,
    /// 是否强制关闭 VLC 硬件解码。
    pub disable_hw_decode: bool,
    /// 启动前是否检查已知广告特征。
    pub ad_guard: bool,
    /// 启动时是否自动播放上次频道。
    pub auto_play_last: bool,
    /// 可选的自定义 HTTP User-Agent。
    pub user_agent: String,
    /// 可选的 HTTP Referer。
    pub referer: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            vlc_path: String::new(),
            last_channel_key: String::new(),
            last_filter: "全部".to_owned(),
            favorites: Vec::new(),
            custom_source_url: String::new(),
            fullscreen: true,
            use_playlist: true,
            network_caching_ms: 1500,
            disable_hw_decode: false,
            ad_guard: true,
            auto_play_last: false,
            user_agent: String::new(),
            referer: String::new(),
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let mut config = toml::from_str::<Self>(&content)?;
        config.normalize();
        Ok(config)
    }

    /// 修正越界或重复字段。配置是用户可手改的，不能假设一定合法。
    pub fn normalize(&mut self) {
        self.network_caching_ms = self.network_caching_ms.min(60_000);
        if self.last_filter.trim().is_empty() {
            self.last_filter = "全部".to_owned();
        }
        let mut seen = std::collections::HashSet::new();
        self.favorites
            .retain(|key| !key.is_empty() && seen.insert(key.clone()));
        self.vlc_path = self.vlc_path.trim().trim_matches('"').trim().to_owned();
        self.user_agent = self.user_agent.trim().to_owned();
        self.referer = self.referer.trim().to_owned();
    }

    /// 原子保存：先写同目录临时文件，成功后再替换目标。
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        let sequence = TEMP_CONFIG_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), sequence));

        let result = (|| -> Result<(), ConfigError> {
            use std::io::Write;
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp_path, path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("读取或写入配置文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置文件格式错误：{0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("序列化配置失败：{0}")]
    TomlSer(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "touchiptv-config-test-{name}-{}-{}",
            std::process::id(),
            TEMP_CONFIG_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn invalid_config_reports_error_and_keeps_file() {
        let path = temp_path("invalid");
        let original = "vlc_path = [broken";
        fs::write(&path, original).expect("write invalid config");

        let error = AppConfig::load_or_default(&path).expect_err("must fail");
        assert!(matches!(error, ConfigError::TomlDe(_)));
        assert_eq!(fs::read_to_string(&path).expect("read"), original);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_is_atomic_and_replaces_original() {
        let path = temp_path("atomic");
        fs::write(&path, "old = true").expect("write old config");
        let mut config = AppConfig::default();
        config.vlc_path = "C:\\Program Files\\VideoLAN\\VLC\\vlc.exe".to_owned();
        config.favorites.push("cctv1".to_owned());

        config.save(&path).expect("save config");
        let loaded = AppConfig::load_or_default(&path).expect("load");
        assert_eq!(loaded.vlc_path, config.vlc_path);
        assert_eq!(loaded.favorites, config.favorites);

        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_string_lossy().contains(".tmp-")
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .contains("touchiptv-config-test")
            })
            .collect();
        assert!(leftovers.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn normalize_clamps_and_dedupes() {
        let mut config = AppConfig {
            network_caching_ms: 99_999,
            favorites: vec!["a".into(), "a".into(), "".into(), "b".into()],
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.network_caching_ms, 60_000);
        assert_eq!(config.favorites, vec!["a".to_owned(), "b".to_owned()]);
    }
}
