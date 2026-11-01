#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo 未找到，请先安装 Rust：https://rustup.rs" >&2
    exit 1
fi

CONFIG_FILE="$SCRIPT_DIR/config.toml"
if [[ ! -f "$CONFIG_FILE" ]]; then
    DEFAULT_VLC_PATH="${VLC_PATH:-}"
    if [[ -z "$DEFAULT_VLC_PATH" ]]; then
        DEFAULT_VLC_PATH="$(command -v vlc || true)"
    fi
    if [[ -z "$DEFAULT_VLC_PATH" ]]; then
        DEFAULT_VLC_PATH="/snap/bin/vlc"
    fi

    cat >"$CONFIG_FILE" <<EOF
# TouchIPTV 直播桌面版 · 本地开发配置
vlc_path = "$DEFAULT_VLC_PATH"
last_channel_key = ""
last_filter = "全部"
favorites = []
custom_source_url = ""
fullscreen = true
use_playlist = true
network_caching_ms = 1500
disable_hw_decode = false
ad_guard = true
auto_play_last = false
user_agent = ""
referer = ""
EOF
    echo "已创建初始配置：$CONFIG_FILE"
fi

echo "配置文件：$CONFIG_FILE"

if [[ "${1:-}" == "--release" ]]; then
    shift
    exec cargo run --release "$@"
fi

exec cargo run "$@"
