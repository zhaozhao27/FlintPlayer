# FlintPlayer

Linux 平台低延迟 USB 视频采集与监看工具，使用 Rust + Slint 构建，界面与 Tab OSD 参照 PotPlayer 设计。

## 功能

- 原生 V4L2 采集（`/dev/video*`），支持 MJPEG、H.264、HEVC、YUYV、UYVY、NV12、RGB/BGR、GRAY
- 单窗口多路监看：最多 4 路设备或网络源按 2×2 宫格同屏显示，单击切换活动画面，双击单路全屏，右上角关闭单路
- 网络或本地裸流输入（设备菜单中填写 URL）
- FFmpeg 低延迟解码（`LOW_DELAY`、单解码线程、swscale → RGBA8888）
- 深度为 1 的最新帧槽：新帧覆盖未消费旧帧并计入丢帧，队列零积压
- PotPlayer 风格 Tab OSD：播放时间、实际/标称帧率、丢帧、码率、编码器、输入输出、渲染器、解码延迟、音频状态
- 无边框窗口、拖动/缩放、底部悬浮控制栏（鼠标静止 2 秒自动隐藏）
- 无损 PNG 截图、画面冻结、全屏切换、USB 设备热插拔重扫
- 窗口位置、采集设备、分辨率与 OSD 状态持久化

## 快捷键

| 按键 | 功能 |
| --- | --- |
| `Tab` | 显示/隐藏播放信息 OSD |
| `Space` | 冻结/恢复画面显示 |
| `S` | 将当前 RGBA 帧保存为无损 PNG |
| `Enter` / `F` | 切换全屏 |
| `Esc` | 关闭设备菜单；非全屏时退出全屏 |
| `Alt+D` | 显示/隐藏设备菜单，选择要打开的 USB 采集设备 |

截图默认保存到 `~/图片/FlintPlayer`（遵循系统图片目录），文件名形如 `flintplayer_<毫秒时间戳>.png`。

配置文件位于 `~/.config/flintplayer/config.ini`，记录上次使用的设备、分辨率、窗口位置与 OSD 开关；多路设备以 `sources=/dev/video0|/dev/video2` 的形式保存。

## 多路监看

`Alt+D` 设备菜单中选择设备时，若画面未满 4 路，会在新格位中追加；已满 4 路时替换当前活动画面。选择后菜单保持打开，便于连续添加多路，按 `Esc` 或再次 `Alt+D` 关闭。

单击任意画面将其设为活动画面，双击切换该画面的单路全屏与宫格视图，活动画面右上角显示关闭按钮（单路视图下鼠标移入时显示）。

设备菜单的分辨率选择、截图（`S`）、冻结（`Space`）、全屏（`Enter` / `F`）与 Tab OSD 均作用于活动画面，OSD 与底部控制栏都显示在活动画面内（OSD 位于左上角），切换活动画面时一同移动。

## 构建与运行

系统依赖（Debian/Ubuntu）：

```bash
sudo apt install build-essential pkg-config \
    libavcodec-dev libavformat-dev libavutil-dev libswscale-dev \
    libfontconfig1-dev libxkbcommon-dev fonts-noto-cjk
```

```bash
cargo run -p flint-ui
cargo build --release -p flint-ui   # 产物：target/release/flintplayer
cargo test --workspace
```

连接真实摄像头后运行集成测试：

```bash
cargo test -p flint-media --test capture_smoke -- --ignored --nocapture
```

访问 `/dev/video*` 需要当前用户属于 `video` 组：

```bash
sudo usermod -aG video "$USER"
```

## Debian 打包

```bash
cargo install cargo-deb   # 仅首次
cargo deb -p flint-ui
sudo apt install ./target/debian/flintplayer_0.1.0-1_amd64.deb
```

`depends = "$auto, fonts-noto-cjk"` 会让 `cargo-deb` 通过 `ldd` 自动补全 FFmpeg 等动态库依赖，并确保中文字体可用。

## 架构

```
flint-ui ──> flint-media ──> flint-core
   │             │               │
 Slint 界面   采集/解码管线    设备枚举、命令、统计、配置
```

```
V4L2 / 网络流
   → CaptureEngine（字节计数、Mbps 统计）
   → FFmpeg 解码（MJPEG / H.264 / YUYV / NV12 → RGBA8888，记录单帧解码耗时）
   → Latest Frame Slot（深度 1，新帧覆盖旧帧，Dropped Frames +1）
   → Slint Presenter（16ms 轮询最新帧，统计实际 FPS）
```

### 原生 V4L2 而非 FFmpeg `device` 输入

目标环境缺少 libavdevice 的开发文件与 pkg-config 条目，FFmpeg 的 `device` feature 无法编译。因此 `flint-media/capture_v4l2.rs` 直接通过 `ioctl` 完成格式枚举、请求缓冲、`DQBUF`/`QBUF` 与 `STREAMON`；FFmpeg 只负责解码与像素格式转换。这样既保留了低延迟的 mmap 采集路径，也避免了额外运行时依赖。V4L2 缓冲区通过 `mmap` 直接映射，解码前的未压缩帧按行拷入复用的 `AVFrame`，压缩帧每包拷入一个 `AVPacket`。

### 低延迟要点

- V4L2 打开 `O_NONBLOCK`，`DQBUF` 120ms 超时轮询，停止标志可及时退出
- FFmpeg 解码器启用 `AV_CODEC_FLAG_LOW_DELAY`，线程数固定为 1
- 最新帧槽深度固定为 1，UI 线程只取最新帧，不排空历史帧
- RGBA 转换缓冲从帧槽缓冲池复用，媒体侧稳态下无逐帧 RGBA `Vec` 分配；渲染时 Slint 需要一份 `SharedPixelBuffer` 拷贝
- 设备断开、启动失败、线程异常均以状态错误返回，不 panic

## 目录

```
crates/flint-core     设备枚举、命令、统计、配置
crates/flint-media    V4L2/网络采集、FFmpeg 解码、最新帧槽
crates/flint-ui       Slint 界面、快捷键、应用装配
assets                .desktop 启动器与 256×256 图标
```

## 许可

MIT，见 [LICENSE](LICENSE)。
# FlintPlayer
