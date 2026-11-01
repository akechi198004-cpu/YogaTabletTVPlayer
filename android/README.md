# TouchIPTV · Android 原生版（适配 Android 4.4.2 平板）

把原来的网页版（React + hls.js）重写成**原生 Android 应用**，目标是给一台只有 Android 4.4.2（API 19）的 8 寸旧平板当电视直播机用。

```
tv/
├─ android/        ← 新的原生 App 工程（Gradle + Java）
├─ web-source/     ← 原来的网页版源码，仅作对照参考
├─ dist/           ← 打好的 APK
└─ README.md
```

## 为什么不能直接套 WebView

Android 4.4 的 WebView 是 Chromium 30/33：

- **没有 MediaSource Extensions（MSE）**，hls.js 完全无法工作；
- 不支持 React 19 需要的现代 JS 语法（可选链、`??`、原生 ESM 等）；
- 没有 `document.fullscreenElement` 等 API。

也就是说「网页套壳」在 4.4 上必然白屏或黑屏，只能重写成原生播放器。

## 技术方案

| 项目 | 方案 |
| --- | --- |
| 播放引擎 | **ExoPlayer 2.18.7**（支持 HLS / TS / FLV，minSdk 16）为主，**系统 MediaPlayer** 自动回退 |
| 失败链 | ExoPlayer 报错 → 自动切系统播放器；仍失败再切备用线路。识别到广告流时会跳过系统播放器，直接尝试备用线路 |
| 广告流拦截 | ExoPlayer 网络层检查重定向落地 URL 和 HLS 清单；命中已知广告签名后最多尝试 6 次（含首次请求），仍命中则阻断并自动切换频道备用线路 |
| HTTP/TLS | 4.4 默认只谈 TLS 1.0，已在内置的 `Net` 里启用 TLS 1.1/1.2，并额外信任 Let's Encrypt / Amazon / DigiCert G2 根证书（系统根库没有它们） |
| 界面 | 纯 framework 控件（无 AppCompat），暗色「铁灰」主题，横屏左右分栏、竖屏上下堆叠，自动适配 |
| 图标 | GDI+ 生成的 PNG（4.4 不支持 VectorDrawable），运行时用 colorFilter 着色 |
| 数据 | 内置 104 个频道（`assets/channels.json`，与网页版数据一致）+ 可导入自定义 M3U |

## 功能对照（网页版 → App）

| 网页版功能 | App 里的实现 |
| --- | --- |
| 104 个频道、分类筛选 | 顶部分类胶囊：全部 / 收藏 / 央视 / 卫视 / 苏州 / 香港 / 台湾 / 日本 / 国际 / 纪录 / 少儿 |
| 搜索 | 右侧搜索框（名称 / 分类 / 画质） |
| 收藏 | 点 ★ 或**长按频道行** |
| 上一次看的频道 | 记住并自动续播（SharedPreferences） |
| 单频道测速 / 批量测速 | 播放器上的 ⚡ 按钮 = 单频道；标题栏 ⚡ = 当前分类并发 3 路测速，结果直接标在列表上（绿≤500ms / 黄≤1200ms / 红） |
| 备用线路切换 | 多线路时显示「线路 x/n」，点按或数字键 1-5 切换；失败会自动顺延 |
| 全屏 | 播放器右下角按钮 / 返回键退出 |
| 视频信息（分辨率、延迟） | 播放器左上角徽标，分辨率会按实际解码结果更新 |
| 状态提示条（notice） | 底部提示条 |
| 刷新频道列表 | 频道面板右上角 ⟳ |
| 主题切换（铁灰/素白） | 未做（旧平板上暗色更合适，需要的话可以加） |
| PiP / 剧场模式 | 未做（4.4 无 PiP，剧场模式已由分栏布局覆盖） |

## 构建

环境已固定在本机 `LeTV/tools` 下（JDK 17 + SDK 34 + Gradle 8.9）：

```powershell
cd tv\android
.\build.ps1              # 生成 release APK
```

产物：

```
android\app\build\outputs\apk\release\app-release.apk
dist\TouchIPTV-1.0.0.apk
```

（release 用 debug 签名，方便直接 `adb install -r` 覆盖安装。）

## 安装到平板

```powershell
# 方式一：USB 调试
adb devices                      # 平板需先允许调试授权
adb install -r dist\TouchIPTV-1.0.0.apk

# 方式二：网络调试（平板与电脑同一 WiFi）
adb connect 192.168.0.56:5555
adb install -r dist\TouchIPTV-1.0.0.apk
```

## 操作说明

| 操作 | 效果 |
| --- | --- |
| 点视频画面 | 显示/隐藏控制条 |
| 中央 ▶ | 继续播放（暂停时出现） |
| ⟳ | 重新连接当前频道 |
| ⚡（播放器上） | 给当前频道测速 |
| ⚡（标题栏） | 给当前分类全部频道测速 |
| ⚙ / 点「解码：自动」 | 设置：解码方式、导入自定义 M3U 源 |
| 长按频道行 | 收藏 / 取消收藏 |
| 数字键 1-5 | 切换线路 |
| 返回键 | 退出全屏（再按一次退出 App） |
| 音量键 | 调整系统音量；播放器上的喇叭按钮是静音开关 |

## 已知限制

- 部分频道源本身不稳定（公共源），失败时请换台或导入自己的源。
- 少数 HTTPS 频道如果用了 4.4 时代之后新增的 CA / 特殊 TLS 套件，仍可能失败；这类只能靠换线路或自建代理。
- 极少数频道是 `.flv`（凤凰系列），只有 ExoPlayer 能放，系统播放器回退时会失败。
- 广告识别覆盖已知 URL 签名和 HLS 清单中的 `mkt/segment_`、`byt/segment_`、`yss/segment_` 路径；系统 MediaPlayer 无法检查重定向后的地址或清单正文。自动模式在 ExoPlayer 已确认广告时不会回退到系统播放器；手动强制系统播放器时，仅能拦截原始 URL 中直接出现的已知签名。
- 自定义源目前支持 **http(s) 地址**（M3U/M3U8），暂不支持从本地文件选择。

## 一个已经做过的网络检查

从开发机对 104 个内置源做了一次首字节探测：**102 个返回了播放列表内容**，2 个异常（CCTV-5+ 提示内容受限、NHK World Premium 403）。说明内置列表整体可用，剩下的就是 4.4 设备上的解码兼容性验证。
