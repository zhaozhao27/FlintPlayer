# FlintPlayer

Linux 下的低延迟 USB 视频采集与本地媒体播放工具，界面与菜单设计参考了 PotPlayer。

- 支持 USB 摄像头（V4L2）低延迟采集，兼容 MJPEG、H.264、HEVC、YUYV、UYVY、NV12、RGB24、BGR24、GRAY8。
- 支持播放本地音视频与网络流，内置播放列表、字幕、音轨、章节跳转与倍速控制。
- 提供画面截图、连续截图、生成缩略图，以及 MP4 视频录制与音频录制。
- 延续 PotPlayer 风格的右键菜单与快捷键，按 Tab 键可通过 OSD 实时查看帧率、丢帧、码率与延迟。
- 支持画面色彩调节、3D 输出、360° 全景播放与 VapourSynth 滤镜。

![FlintPlayer 单帧分析界面](analysis-a.png)

## 功能

**播放**

- 可打开本地文件、文件夹、网络流或剪贴板链接，支持文件拖放。
- 播放列表：支持侧栏展示、添加文件与文件夹、按文件名排序及一键清空。
- 支持循环、随机播放与进度记忆，当前媒体结束后自动播放下一项。
- 支持章节跳转、AB 区段循环、0.25×–4× 变速以及逐帧步进。

**采集**

- 自动枚举 V4L2 设备，可按设备分别指定分辨率、帧率与像素格式。
- 支持 MJPEG、H.264、HEVC、YUYV、UYVY、NV12、RGB24、BGR24、GRAY8 格式。
- 提供两路 X11 屏幕采集，两路分辨率可独立配置。
- 支持多设备同步采集：当已有一路采集运行且所选设备与当前活动设备不同时，可在“设备设置 → 摄像头”中勾选“多设备同步采集（分屏显示）”并打开另一台设备，多路画面在同一窗口内按每行两路的网格同屏显示。

**字幕与声音**

- 支持加载外部主/次字幕、切换内嵌字幕轨及重新加载字幕。
- 支持切换音轨，音频可输出至 PulseAudio、ALSA、JACK 或空设备。
- 支持音量调节与静音，采集画面时可同步录制 WAV 音频。

**画面**

- 亮度、对比度、饱和度支持按 1% 步进调节，可一键复位。
- 支持 0°/90°/180°/270° 旋转、水平/垂直翻转，以及按宽高比自动旋转。
- 支持 4:3、16:9、16:10、1.85:1、2.35:1 画面拉伸或裁剪，并可微调位置。
- 下边距可在 0%–30% 间调节，支持画面帧位缩放与偏移。
- 支持左右/上下 3D 输出、360° 全景视角与 VapourSynth 滤镜。

**工具与界面**

- 支持 PNG/JPEG/BMP 截图、连续截图、生成缩略图与录制 MP4 视频。
- Tab OSD 可显示文件名、播放时间、帧率、丢帧数、码率、解码耗时与延迟。
- 可将当前媒体的文件名、所在目录或完整路径复制到剪贴板。
- 提供中文/English 界面、无边框模式、窗口置顶、多配置管理及播完后动作设置。

## 安装与构建

### 系统依赖

Debian/Ubuntu：

```bash
sudo apt install build-essential pkg-config \
    libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev \
    libfontconfig1-dev libxkbcommon-dev fonts-noto-cjk
```

访问 `/dev/video*` 设备需要将当前用户加入 `video` 组：

```bash
sudo usermod -aG video "$USER"
```

注销并重新登录后生效。

### 从源码运行

```bash
cargo run -p flintplayer-desktop
cargo build --release -p flintplayer-desktop
```

编译产物位于 `target/release/flintplayer`。

### Debian 打包

打包前需安装 `cargo-deb`：

```bash
cargo install cargo-deb
```

默认动态链接系统 FFmpeg，适合在本机或同版本发行版上使用：

```bash
cargo deb -p flintplayer-desktop
```

官方发布包静态链接了 FFmpeg 6.1.2，不依赖系统的 FFmpeg 动态库，可在 glibc 2.36 及以上环境中运行：

```bash
scripts/build-static-ffmpeg.sh
FFMPEG_DIR=target/ffmpeg-static \
  cargo build --release -p flintplayer-desktop --features static-ffmpeg
cargo deb -p flintplayer-desktop --no-build
```

安装软件包：

```bash
sudo apt install ./target/debian/flintplayer_1.1.4-1_amd64.deb
```

运行时依赖：`libmpv2 | libmpv1`、`libvulkan1`、`mesa-vulkan-drivers`、`fonts-noto-cjk`；建议同时安装 `zenity`、`ffmpeg`、`xdg-utils`，以支持文件对话框、缩略图生成和打开所在文件夹。

## 使用方法

### 打开媒体

- `F3` / `Ctrl+O`：打开本地文件。
- `F2`：打开文件夹，将其加入播放列表并开始播放。
- `Ctrl+U` / `Alt+F12`：打开网络链接或流媒体地址。
- `Ctrl+V`：直接打开剪贴板中的链接。
- 直接将媒体文件拖入窗口即可播放。

### 采集设备

1. 按 `F5` / `Alt+D` 打开设备设置。
2. 选择目标设备与分辨率，点击“打开设备”。
3. 也可以通过菜单“打开 → 摄像头/其他设备”直接开启设备。

通过菜单“直播相关工具 → 采集器I/II”或快捷键 `Ctrl+Alt+F1` / `Ctrl+Alt+F2` 可启动 X11 屏幕采集；在“截屏1 尺寸设置”或“截屏2 尺寸设置”中可调整采集分辨率。

按 `Alt+V` 或通过菜单“视频 → 选择图像”，可在本地播放、屏幕采集 1、屏幕采集 2 之间切换主画面。

### 菜单与播放列表

- 点击标题栏左上角的“FlintPlayer”，或在窗口内任意位置点击右键，即可呼出主菜单。
- 按 `F6` 显示或隐藏播放列表，点击列表项即可切换媒体。
- 在“播放 → 播放列表”菜单中可添加文件或文件夹、排序列表或清空内容。
- “播放”菜单下还可设置循环模式、随机播放、记忆播放进度与倍速。

### OSD 与画面

- 按 `Tab` 显示完整 OSD，`Shift+Tab` 显示简要信息；“播放信息”可从“配置/语言/其他”菜单打开。
- OSD 面板涵盖文件名、时间进度、帧率、丢帧统计、码率、解码耗时、延迟与音频状态。
- “视频”菜单集成了色彩调节、旋转、翻转、裁剪/拉伸、下边距、3D 模式与 360° 全景选项。
- 本地播放时，可在“视频 → 图像截取”中设置截图格式、命名规则与保存路径，或执行连续截图与创建缩略图。
- 本地播放时，可通过“视频 → 视频录制”开始或停止 MP4 录制；采集时可通过“声音 → 音轨 → 声音录制”录制 WAV 音频。

### 命令行

```bash
flintplayer [--size WxH] [--fps N] [--raw 输入格式] [--pix-fmt 像素格式] [--frame-analysis] <设备或文件>
```

加上 `--frame-analysis` 可进入逐帧分析模式，该模式下 Tab 可开关帧信息 OSD（显示帧号、时间戳、分辨率与像素格式），PgUp/PgDn、鼠标侧键或 `,` / `.` 键可逐帧检视画面。

## 快捷键

### 播放、文件与通用

| 快捷键 | 功能 |
| --- | --- |
| `Space` | 播放 / 暂停 |
| `PgUp` / `PgDn` | 上一个 / 下一个文件 |
| `←` / `→` | 后退 / 前进 5 秒 |
| `,` / `.` | 上一帧 / 下一帧 |
| `C` / `X` / `Z` | 加速 / 减速 / 恢复正常速度 |
| `↑` / `↓` | 音量 + / − |
| `M` | 静音 |
| `[` / `]` | 上一 / 下一章节 |
| `Ctrl+Y` | 重开当前 / 最后文件 |
| `F4` / `Ctrl+W` | 关闭当前画面 |
| `F6` | 显示 / 隐藏播放列表 |
| `F3` / `Ctrl+O` | 打开文件 |
| `F2` | 打开文件夹 |
| `Ctrl+U` / `Alt+F12` | 打开链接 |
| `Ctrl+V` | 打开剪贴板 |
| `Enter` / `F` / `Alt+Enter` | 全屏 |
| `Esc` | 关闭菜单 / 对话框 |
| `F1` | 关于 FlintPlayer |
| `F12` | 搜索文件 |
| `F9` | 在浏览器中打开地址 |
| `Ctrl+F12` | 搜索菜单 |
| `F5` / `Alt+D` / `Ctrl+Alt+F5` | 设备设置 |
| `Ctrl+Alt+F1` / `Ctrl+Alt+F2` | 开始屏幕采集 1 / 2（`F3` / `F4` 同效） |
| `Ctrl+T` | 切换置顶 |
| `Ctrl+Alt+A` | 切换收尾处理 |

### 画面与字幕

| 快捷键 | 功能 |
| --- | --- |
| `J` | 切换 3D 输出模式 |
| `Alt+K` | 切换旋转角度 |
| `Ctrl+P` / `Ctrl+Z` | 上下 / 左右翻转 |
| `Ctrl+F5` / `Ctrl+F6` | 切换拉伸与原始比例 / 依次切换画面比例 |
| `Ctrl+Alt+B` | 切换下边距 |
| `W` / `E` | 亮度 − / + 1% |
| `Y` / `U` | 饱和度 − / + 1% |
| `Q` | 图像属性复位 |
| `Alt+O` | 加载字幕 |
| `Ctrl+Alt+Y` | 重载字幕 |
| `Ctrl+E` / `Ctrl+Alt+E` | 截存当前画面 |
| `Ctrl+G` | 连续截图设置 |
| `Alt+N` | 创建缩略图 |
| `Alt+C` | 录制视频 |
| `Alt+V` | 切换图像源 |
| `Ctrl+F1` | 显示 OSD |
| `Tab` / `Shift+Tab` | 完整 / 简要 OSD |

### 帧位与窗口

| 快捷键 | 功能 |
| --- | --- |
| `数字键 5` | 帧位居中 / 复位 |
| `数字键 1` / `9` | 全帧 − / + |
| `数字键 4` / `6` | 帧宽 − / + |
| `数字键 2` / `8` | 帧高 − / + |
| `Ctrl+数字键 4` / `6` | 帧位左移 / 右移 |
| `Ctrl+数字键 8` / `2` | 帧位上移 / 下移 |
| `Ctrl+数字键 5` | 帧位居中 |
| `Alt+2` / `Alt+3` / `Alt+4` | 窗口 45% / 60% / 75% |
| `Ctrl+Alt+数字键 +` / `-` | 窗口放大 / 缩小 |

## 配置文件位置

- 主配置：`~/.config/flintplayer/config.ini`，记录设备参数、窗口位置与尺寸、音量、画面调节、界面语言及快捷键开关。
- 预设方案：`~/.config/flintplayer/profiles/`，可在菜单“配置/语言/其他 → 配置管理”中保存、加载或删除。
- 截图：默认保存在系统图片目录下的 `FlintPlayer` 文件夹。
- 视频录制：默认保存在系统视频目录下的 `FlintPlayer` 文件夹。
- 声音录制：默认保存在系统音乐目录下的 `FlintPlayer` 文件夹。

截图与视频录制的保存路径可在对应菜单中修改；声音录制路径可通过配置文件调整。

## LGPL 合规

FlintPlayer 本身基于 MIT 许可证发布，详见 [LICENSE](LICENSE)。官方发布包静态链接了 FFmpeg 6.1.2，FFmpeg 采用 LGPL-2.1-or-later 许可证。

* **FFmpeg 源码归档**：<https://ffmpeg.org/releases/ffmpeg-6.1.2.tar.xz>
* **内置子模块**：FFmpeg 6.1.2 已作为 Git 子模块固定在仓库的 `vendor/ffmpeg` 目录（对应 tag `n6.1.2`）。
* **静态构建脚本**：仓库提供 `scripts/build-static-ffmpeg.sh` 脚本用于编译静态 FFmpeg。其源目录默认为 `vendor/ffmpeg`，安装前缀默认为 `target/ffmpeg-static`。该脚本会自动执行 `git submodule update --init --depth 1`，随后按照脚本内固定的配置参数执行 `configure`、`make` 和 `make install`，并在完成时打印 `FFMPEG_DIR=<prefix>`。

如需自行构建静态链接版本的 FlintPlayer 或替换其中的 FFmpeg 库，请执行以下命令：

```bash
git submodule update --init --depth 1
scripts/build-static-ffmpeg.sh
FFMPEG_DIR=target/ffmpeg-static cargo build --release -p flintplayer-desktop --features static-ffmpeg
```

如需获取目标文件或重链接脚本，请联系 <flint.lzzhang@ingenic.com>。
