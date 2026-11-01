# TouchIPTV Desktop · Win32 直播版 / Win32 Live TV

**中文**  
把原来的 Android 4.4 原生直播应用（`android/`）重写成适合 **Win32 低性能小平板** 的桌面程序。界面用 **Rust + eframe/egui**，视频解码交给本机 **外部 VLC**。不是网页套壳，不使用 WebView，不在主程序里解码视频。

**English**  
A Win32 desktop rewrite of the original Android 4.4 live-TV app in `android/`, aimed at **low-spec Windows tablets**. The UI is **Rust + eframe/egui**; playback is delegated to an **external VLC** installation. No WebView, no browser engine, and no video decoding inside the launcher process.

---

## 为什么用外部 VLC / Why external VLC

**中文**  
老 Win32 平板（尤其 32 位 Windows + 低功耗 Atom / 老核显）在浏览器里播放 H.265/HLS 经常没有硬解，Plex/网页播放器又会迫使服务器转码。VLC 对 HLS、TS、FLV、H.265 的支持成熟，硬解路径也完整。  
本程序只做频道浏览、搜索、收藏、测速、广告预检和启动 VLC，自身内存占用很小；解码、缓冲、音量和全屏都交给 VLC。

**English**  
Old Win32 tablets (especially 32-bit Windows with low-power Atom CPUs / old iGPUs) often cannot decode H.265/HLS well in a browser, and web players may force server-side transcoding. VLC has mature HLS/TS/FLV/H.265 support and a complete hardware-decode path.  
This launcher only browses channels, searches, manages favorites, tests speed, pre-checks known ad signatures, and starts VLC. Decoding, buffering, volume, and fullscreen are handled by VLC.

---

## 界面截图 / Screenshots

**主界面（铁灰主题，内置 104 个频道） / Main window (iron-gray, 104 bundled channels)**

![主界面 / Main window](snap-main.png)

**播放状态（VLC 在外部窗口解码） / Playing state (VLC decodes in its own window)**

![正在播放 / Playing](snap-playing.png)

**设置（Linux 开发截图；Windows 下路径为 `vlc.exe`） / Settings (Linux dev screenshot; use `vlc.exe` on Windows)**

![设置 / Settings](snap-settings.png)

**窄窗口布局 / Narrow layout**

![窄窗口 / Narrow layout](snap-narrow.png)

---

## 功能对照 / Feature parity

| Android 版功能 | Win32 版实现 |
| --- | --- |
| 104 个内置频道、分类胶囊 | ✅ 同一份 `assets/channels.json`，分类规则一比一移植 |
| 搜索名称 / 分类 / 画质 | ✅ 顶部搜索框实时过滤 |
| 收藏（点星 / 长按 / 右键） | ✅ 行尾星标、右键、长按都可切换 |
| 记住上次频道与分类 | ✅ 写入 `config.toml` |
| 单频道测速 / 批量测速 | ✅ 批量固定并发 3 路，结果直接标在列表 |
| 多线路切换与失败顺延 | ✅ 手动切线路；预检命中广告或 VLC 异常早退时自动顺延 |
| 广告流识别 | ✅ URL + HLS 清单预检；播放中途的限制见下文 |
| 播放引擎 | 外部 VLC（主程序不内嵌视频） |
| 全屏 | VLC `--fullscreen`，设置里可关 |
| 解码方式 | VLC 自动硬解；设置里可强制 `--avcodec-hw=none` |
| 自定义 M3U | ✅ 支持 http(s) 地址导入、清除、与内置源合并 |
| 主题 | 铁灰单主题（低配平板优先暗色） |
| 画中画 / 剧场模式 | ❌ 未做 |
| 应用内暂停 / 音量 / 画面比例 | ❌ 由 VLC 自带控制条负责 |

**中文**  
Win32 版把 Android 版的「播放器内嵌控制层」改成了「外部 VLC + 播放列表」：当前筛选结果会写成 VLC 播放列表，选中频道排第一，之后可以直接在 VLC 里用 `N` / `P` 换台。  
如果更看重逐频道广告预检，可以在设置里关闭「写入 VLC 播放列表」，这样每次换台都必须回到本程序重新预检。  
注意：在 VLC 里用 `N/P` 换台后，本程序的「正在播放」仍显示启动时的频道；需要同步状态时，请回到本程序换台。

**English**  
The Android in-player overlay is replaced by **external VLC + a generated playlist**: the current filtered list is written to an M3U, with the selected channel first. You can then use `N` / `P` inside VLC to change channels.  
If you prefer strict per-channel ad pre-checks, disable “write current filter to VLC playlist” in Settings; every channel change will then go through the launcher again.  
Note: after changing channels with `N/P` inside VLC, the launcher still displays the channel it originally started; switch channels in the launcher if you need the state to stay in sync.

---

## 技术方案 / Tech stack

**中文**  
Rust · `eframe` / `egui` 0.27 · `reqwest`（blocking + rustls）· `serde` / `toml` / `serde_json` · `url` · 外部 VLC 3.x

**English**  
Rust · `eframe` / `egui` 0.27 · `reqwest` (blocking + rustls) · `serde` / `toml` / `serde_json` · `url` · external VLC 3.x

---

## 目录结构 / Project layout

```text
.
|-- Cargo.toml
|-- config.example.toml
|-- DroidSansFallbackFull.ttf      # 内置 CJK 字体 / bundled CJK font
|-- assets/channels.json           # 104 个内置频道 / bundled channels
|-- packaging/windows/config.toml  # 发布包里的便携配置
|-- src/
|   |-- main.rs                    # 入口、字体、配置路径
|   |-- app.rs                     # 状态、播放会话、后台任务
|   |-- app_ui.rs                  # egui 界面
|   |-- config.rs                  # TOML 配置
|   |-- channels.rs                # 内置源 + 自定义源仓库
|   |-- m3u.rs                     # M3U/TXT 解析与分类
|   |-- models.rs                  # Channel / 播放状态
|   |-- net.rs                     # HTTP、测速、M3U 下载、广告预检
|   |-- adguard.rs                 # 广告特征识别
|   |-- player.rs                  # VLC 探测、参数、播放列表
|   |-- jobs.rs                    # 后台线程与事件
|   `-- ui/
|       |-- channel_list.rs        # 虚拟化频道行、分类胶囊
|       `-- theme.rs               # 铁灰主题
|-- android/                       # 原 Android 工程（保留作对照）
|-- web-source/                    # 原网页版（保留作对照）
`-- dist/                          # 原 Android APK 与截图
```

---

## 运行要求 / Requirements

**中文**
- Windows 10 32 位或 64 位（发布目标为 `i686-pc-windows-msvc`，32 位 exe 也能在 64 位 Windows 上运行；Windows 8/7 未实测）
- 至少 2 GB 内存
- OpenGL 2.1 驱动（egui 的 `glow` 后端）。若窗口打不开，先更新显卡驱动
- **VLC 3.x**。程序会自动探测常见安装位置，也可以在设置里手填 `vlc.exe`
- 32 位 Windows 请安装 32 位 VLC；64 位 Windows 装 32/64 位 VLC 都可以

**English**
- Windows 10, 32-bit or 64-bit (the release target is `i686-pc-windows-msvc`; the 32-bit exe also runs on 64-bit Windows; Windows 8/7 are untested)
- At least 2 GB RAM
- OpenGL 2.1 drivers (egui `glow` backend). Update the GPU driver if the window fails to open
- **VLC 3.x**. Auto-detected from common locations; you can also set `vlc.exe` manually
- Use 32-bit VLC on 32-bit Windows; either 32-bit or 64-bit VLC works on 64-bit Windows

---

## 构建 / Build

**中文**  
Linux 本地开发：

```bash
./start.sh            # debug
./start.sh --release  # release
```

或直接：

```bash
cargo run --release
```

Windows 32 位发布构建（需要 Rust + Visual Studio Build Tools）：

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
```

产物：`target\i686-pc-windows-msvc\release\touchiptv-desktop.exe`。

`.cargo/config.toml` 已为 MSVC 目标启用静态 CRT，尽量减少目标平板缺少 `vcruntime140.dll` 的问题。

**English**  
Linux development:

```bash
./start.sh            # debug
./start.sh --release  # release
```

Or:

```bash
cargo run --release
```

Windows 32-bit release (Rust + Visual Studio Build Tools required):

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
```

Output: `target\i686-pc-windows-msvc\release\touchiptv-desktop.exe`.

`.cargo/config.toml` enables static CRT for MSVC targets to avoid missing `vcruntime140.dll` on target tablets.

---

## GitHub Actions 构建 / GitHub Actions build

**中文**  
仓库内含 `.github/workflows/windows-win32.yml`：

- 在 `windows-latest` 上构建 `i686-pc-windows-msvc`
- 静态链接 MSVC C 运行库
- 产出可直接分发的 `.zip`：`touchiptv-desktop.exe`、`config.toml`、字体、`README.md`、截图、`LICENSE-RUST`
- 提交信息以 `[deploy]` 开头时自动构建；也可 `workflow_dispatch` 手动触发

**English**  
The repo includes `.github/workflows/windows-win32.yml`:

- Builds `i686-pc-windows-msvc` on `windows-latest`
- Statically links the MSVC C runtime
- Uploads a ready-to-share `.zip`: `touchiptv-desktop.exe`, `config.toml`, the font, `README.md`, screenshots, and `LICENSE-RUST`
- Auto-runs when the commit message starts with `[deploy]`; also supports `workflow_dispatch`

---

## 配置 / Configuration

**中文**  
配置查找顺序：

1. 当前工作目录的 `config.toml`
2. exe 同目录的 `config.toml`（双击启动的便携模式）
3. 系统应用数据目录（Windows 为 `%APPDATA%\gathin\TouchIPTV\config`）

把 `config.example.toml` 改名为 `config.toml` 放在 exe 同目录即可便携运行。

主要配置项：

| 键 | 说明 |
| --- | --- |
| `vlc_path` | VLC 路径；留空自动探测 |
| `last_channel_key` | 上次频道（内部 key，不建议手改） |
| `last_filter` | 上次分类 |
| `favorites` | 收藏频道 key 列表 |
| `custom_source_url` | 自定义 M3U 地址（界面导入成功后自动写入） |
| `fullscreen` | VLC 是否全屏 |
| `use_playlist` | 是否把当前筛选结果写成 VLC 播放列表 |
| `network_caching_ms` | VLC 网络缓存，默认 1500 |
| `disable_hw_decode` | 强制关闭 VLC 硬件解码 |
| `ad_guard` | 启动前检查已知广告特征 |
| `auto_play_last` | 启动时自动播放上次频道（默认关闭） |
| `user_agent` | 可选 HTTP UA；VLC 3.0 对完整浏览器 UA 支持不稳定 |
| `referer` | 可选 HTTP Referer |

**English**  
Config lookup order:

1. `config.toml` in the current working directory
2. `config.toml` next to the exe (portable double-click mode)
3. OS app-data directory (`%APPDATA%\gathin\TouchIPTV\config` on Windows)

Rename `config.example.toml` to `config.toml` next to the exe for portable use.

See the table above for the main keys. `vlc_path` empty means auto-detect; `use_playlist` trades strict per-channel pre-checks for in-VLC channel switching; `network_caching_ms` defaults to 1500.

---

## 使用说明 / Usage

**中文**
- 顶部胶囊切换分类；搜索框按名称 / 分类 / 画质过滤
- 点击频道行开始播放；行尾星标、右键或长按切换收藏
- 播放区按钮：停止、重新连接、上一台、下一台、切换线路、测速、收藏
- 设置：VLC 路径、全屏、播放列表、硬件解码、广告预检、自动续播、网络缓存、UA/Referer、自定义 M3U
- 快捷键：`↑/↓` 换台，`Enter` 播放选中频道，`Esc` 停止，`F5` 刷新频道列表，数字键 `1-5` 切换线路（搜索框获得焦点时快捷键不生效）
- VLC 内：`空格` 暂停，`F` 全屏，`N/P` 下一台/上一台，`Q` 退出
- 画面花屏/绿屏时，在设置里勾选「禁用硬件解码」
- 频道源卡顿时，把网络缓存调到 2000-3000 ms

**English**
- Category chips at the top; the search box filters by name / group / resolution
- Click a channel row to play; the star at the row end, right-click, or long-press toggles favorite
- Player buttons: stop, reconnect, previous, next, switch line, speed test, favorite
- Settings: VLC path, fullscreen, playlist, hardware decode, ad guard, auto-resume, network cache, UA/Referer, custom M3U
- Shortcuts: `Up/Down` change channel, `Enter` play selected, `Esc` stop, `F5` refresh, number keys `1-5` switch lines (disabled while the search box has focus)
- In VLC: `Space` pause, `F` fullscreen, `N/P` next/previous, `Q` quit
- If the picture is green or corrupted, enable “disable hardware decode” in Settings
- If a stream stutters, raise network caching to 2000-3000 ms

---

## 广告预检 / Ad guard

**中文**  
从 Android 版移植了同一组已知广告特征：

- 播放列表路径：`/applive/mkt.m3u8`、`/applive/byt.m3u8`、`/applive/yss.m3u8`
- 片段路径：`mkt/segment_*.ts`、`byt/segment_*.ts`、`yss/segment_*.ts`
- 路径段：`appadhw`、`zjstream2`
- 查询参数：`a=tvzb0006`、`app=tvzb0006`

启动前会检查：

1. 原始 URL
2. 重定向后的最终 URL
3. HLS 主清单正文（只扫描实际 URI 行和 `URI="..."` 属性）
4. 主清单里的前两个子清单

命中后自动尝试当前频道的下一条线路；全部命中则阻止播放并提示。

**限制（重要）**  
播放开始后，VLC 自己发起的请求不在本程序的控制范围内：

- 播放中途才插入的广告无法拦截
- 播放开始后，VLC 的请求不经过本程序；HTTPS 清单和 HTTP 中途清单都无法再检查（没有 MITM/本地代理）
- 使用 VLC 播放列表时，只有选中的频道经过预检；在 VLC 里用 `N/P` 切到的其他频道不会重新预检

如果需要更严格的逐频道预检，请在设置里关闭「把当前分类写入 VLC 播放列表」，并只在本程序里换台。

**English**  
The same known ad signatures as the Android app are ported:

- Playlist paths: `/applive/mkt.m3u8`, `/applive/byt.m3u8`, `/applive/yss.m3u8`
- Segment paths: `mkt/segment_*.ts`, `byt/segment_*.ts`, `yss/segment_*.ts`
- Path segments: `appadhw`, `zjstream2`
- Query parameters: `a=tvzb0006`, `app=tvzb0006`

Before launch, the app checks:

1. The original URL
2. The final URL after redirects
3. The HLS manifest body (only actual URI lines and `URI="..."` attributes)
4. The first two variant playlists

On a hit, it automatically tries the next line of the same channel; if every line is blocked, playback is refused with an explanation.

**Important limitations**  
Once VLC starts, its requests are outside the launcher’s control:

- Ads inserted mid-playback cannot be blocked
- After launch, VLC’s requests do not pass through the launcher; HTTPS bodies and mid-playback HTTP manifests cannot be inspected (there is no MITM/local proxy)
- With the VLC playlist enabled, only the selected channel is pre-checked; channels reached via `N/P` inside VLC are not re-checked

For stricter per-channel checks, disable “write current filter to VLC playlist” and switch channels only in the launcher.

---

## 低配优化 / Low-end optimizations

**中文**
- 主程序不解码视频，不加载网页，不使用 WebView
- 无动画、无海报图、无渐变/模糊；列表只绘制可见行
- 批量测速固定 3 路并发，避免老平板同时开太多连接
- 频道列表按固定行高虚拟化，自定义源几千个频道也不卡
- 后台任务全部走线程 + mpsc，UI 线程不做网络请求
- 32 位 exe + 静态 CRT + `opt-level = "s"` + LTO
- 网络缓存默认 1500 ms，可按源稳定性调整
- 硬件解码默认交给 VLC 自动选择

**English**
- No video decode, no web page, no WebView in the launcher
- No animations, posters, gradients, or blur; only visible list rows are drawn
- Batch speed tests use exactly 3 concurrent workers
- Fixed-row-height virtualized list handles thousands of custom channels
- All network work runs on background threads with mpsc; the UI thread never blocks
- 32-bit exe + static CRT + `opt-level = "s"` + LTO
- Default 1500 ms network cache, adjustable per source stability
- Hardware decode is left to VLC’s automatic choice

---

## 已知限制 / Known limitations

**中文**
- 公共直播源本身不稳定；部分频道可能已经失效、限地区或需要特定 UA/Referer
- 必须有外部 VLC；本程序不内嵌 libVLC
- 应用内没有暂停/音量/画面比例按钮，请用 VLC 控制条或快捷键
- VLC 的详细错误对话框由 VLC 自己显示，本程序无法读取；失败时请用「重新连接」或切换线路
- 自定义源目前只支持 http(s) 地址，不支持本地文件选择器
- 未实现画中画、剧场模式、白天主题
- VLC 全屏通常打开在主显示器；多屏环境可能需要手动移动窗口
- 广告预检只覆盖启动前（URL + 清单）；播放中途的请求不经过本程序，详见上一节
- 若 OpenGL 2.1 驱动不可用，egui 窗口可能无法启动；VLC 播放不受影响
- 内置 104 个频道来自原 Android 工程，可能随时间失效；建议导入自己的 M3U

**English**
- Public IPTV sources are inherently unstable; some may be dead, geo-blocked, or require a specific UA/Referer
- External VLC is required; libVLC is not embedded
- No in-app pause/volume/aspect buttons; use VLC’s controls or shortcuts
- Detailed VLC error dialogs are owned by VLC and cannot be read by the launcher; use Reconnect or switch lines when playback fails
- Custom sources support http(s) URLs only; there is no local file picker
- No PiP, theater mode, or day theme
- VLC fullscreen usually opens on the primary monitor; multi-monitor setups may need manual window movement
- Ad guard covers pre-launch checks (URL + manifests) only; requests made during playback do not pass through the launcher—see the previous section
- If OpenGL 2.1 is unavailable, the egui window may fail to start; VLC playback itself is unaffected
- The 104 bundled channels come from the original Android project and may age; importing your own M3U is recommended

---

## 数据与字体 / Data and fonts

**中文**
- `assets/channels.json` 来自原 Android 工程的 `android/app/src/main/assets/channels.json`
- `DroidSansFallbackFull.ttf` 为内置 CJK 字体，确保 Win32 上中文正常显示；字体遵循其原始 Apache 2.0 许可
- 原 Android 工程与网页版源码保留在 `android/`、`web-source/`，方便对照
- 新增的 Rust 代码以 MIT 许可发布（见 `LICENSE-RUST`）；内置频道数据与字体遵循原项目/字体许可

**English**
- `assets/channels.json` comes from the original Android project
- `DroidSansFallbackFull.ttf` is bundled for reliable CJK rendering on Win32; it follows its original Apache 2.0 license
- The original Android project and web source remain in `android/` and `web-source/` for reference
- The new Rust code is released under MIT (see `LICENSE-RUST`); bundled channel data and fonts follow their original project/font licenses

---

## 快速开始 / Quick start

**中文**
1. 安装 VLC 3.x
2. 解压发布包，双击 `touchiptv-desktop.exe`
3. 程序会自动探测 VLC；若没有，点「设置」填写 `vlc.exe`
4. 点一个频道，VLC 开始播放
5. 需要自己的源时，在设置里粘贴 M3U 地址并点「导入」

**English**
1. Install VLC 3.x
2. Unzip the release and run `touchiptv-desktop.exe`
3. VLC is auto-detected; if not, open Settings and set `vlc.exe`
4. Click a channel to start VLC
5. To use your own source, paste an M3U URL in Settings and click Import
