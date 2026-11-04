# FlintPlayer

Linux 平台低延迟 USB 视频采集与监看工具，使用 Rust + GPUI 构建，界面与 Tab OSD 参照 PotPlayer 设计。

## 功能

- 原生 V4L2 采集（`/dev/video*`），支持 MJPEG、H.264、HEVC、YUYV、UYVY、NV12、RGB/BGR、GRAY
- 单窗口多路监看：最多 4 路设备或网络源按 2×2 宫格同屏显示，单击切换活动画面，双击单路全屏，右上角关闭单路
- 网络或本地裸流输入（设备菜单中填写 URL）
- FFmpeg 低延迟解码（`LOW_DELAY`、单解码线程、swscale → RGBA8888）
- 深度为 1 的最新帧槽：新帧覆盖未消费旧帧并计入丢帧，队列零积压
- PotPlayer 风格 Tab OSD：播放时间、实际/标称帧率、丢帧、码率、编码器、输入输出、渲染器、解码延迟、音频状态
- 无边框窗口、拖动/缩放、底部悬浮控制栏（鼠标静止 2 秒自动隐藏）
- PNG / JPEG / BMP 截图、画面冻结、全屏切换、USB 设备热插拔重扫
- 窗口位置、采集设备、分辨率与 OSD 状态持久化
- 本地文件播放列表：打开文件夹批量加入，播放结束自动下一集，支持循环 / 随机 / AB 区段循环与 0.25×–4× 变速，列表随退出保存
- 本地文件音频输出：FFmpeg 解码后经 libswresample 重采样为 48 kHz 立体声，通过 PulseAudio 播放；音量滑块与 `M` 静音即时生效（变速时暂停音频输出）
- 外部字幕：SRT / ASS / SSA / VTT 文本字幕加载与按时间轴显示，支持 UTF-8 与 GB18030（GBK）编码自动识别，菜单或 `L` / `Alt+O` 打开，`Alt+H` 显示/隐藏
- 内嵌字幕：列出 MP4 / MKV 等容器中的文本字幕轨（显示语言与标题），可从“字幕 → 选择字幕”直接加载，无需先导出
- 字幕样式：字号（小/中/大/特大）、颜色（白/黄/青/绿）、位置（顶部/底部）、字体（系统默认/思源黑体/思源宋体/等宽）、粗体与阴影可在“字幕”菜单调整并保存到配置；也可在“字幕 → 字幕设置...”对话框中集中修改
- ASS/SSA 样式：默认读取 `[V4+ Styles]` 的字体、字号、颜色、粗体与对齐方式，并支持 `\an` / `\fs` / `\b` / `\c` / `\fn` 行内覆盖；可在“字幕 → 开启 ASS/SSA 动态字幕”切换，开关保存到配置
- 3D 字幕：“字幕 → 3D 字幕(立体)”支持关闭、左右排列、上下排列与红蓝立体输出，字幕会按对应 3D 格式分别渲染到左右或上下画面
- 手动字幕：`Alt+P` 或“字幕 → 手动输入字幕...”在画面上实时叠加输入的文字，字体与样式跟随字幕设置，关闭后恢复时间轴字幕
- 字幕工具：`Alt+E` 打开字幕浏览器，“复制当前字幕到剪贴板”把正在显示的字幕写入系统剪贴板（xclip/xsel/wl-copy）
- 字幕同步：字幕延迟可按 ±0.1 / ±0.5 秒微调并复位，保存字幕可导出为 SRT 或 ASS（与视频同目录，自动避免覆盖已有文件）
- 首选字幕语言：“字幕 → 字幕语言”可设置为自动 / 中文 / 英文 / 日文 / 韩文，打开内嵌多语字幕的视频时按语言自动选择对应字幕轨；设置保存到配置
- 在线字幕搜索：“字幕 → 在线字幕搜索”输入字幕文件 URL（http/https）后回车，通过 curl 下载到 `~/.config/flintplayer/subtitles` 并立即加载显示
- 实时字幕翻译：“字幕 → 实时字幕翻译”开启后，当前字幕会通过在线翻译服务生成中文译文并显示在原文下方一行；关闭后恢复单行显示，开关保存到配置
- 语音朗读字幕（TTS）：在“字幕 → 生成有声字幕 (TTS)”开启后，字幕变化时通过 Speech Dispatcher（`spd-say`）自动朗读，中文/英文自动切换语种；菜单可朗读当前字幕或立即停止朗读，开关状态保存到配置
- 3D 视频输出：支持左右并排与上下排列的立体片源，`J` 或“视频 → 3D 视频输出”可在关闭、红蓝立体（左右并排/上下）、仅左眼、仅右眼之间切换并保存到配置
- 360° 视频：等距柱状投影全景片源可在“视频 → 播放 360° 视频”中开启，鼠标左键拖动旋转视角、滚轮缩放，菜单可复位视角；视角状态保存到配置
- 画面调整：亮度 / 对比度 / 饱和度以 1% 步进微调，“图像属性复位”或 `Q` 一键还原；图像旋转支持正常、90°、180°、270°，并可水平 / 垂直翻转，均从“视频”菜单操作并保存到配置；“视频 → 图像处理”提供与 PotPlayer 相同的上下翻转（`Ctrl+P`）、左右翻转（`Ctrl+Z`）与旋转切换；“视频 → 图像旋转”支持按序选择旋转方式（`Alt+K`）与按宽高比自动旋转
- 裁剪/拉伸：`视频 → 裁剪/拉伸` 可按 4:3、16:9、16:10、1.85:1、2.35:1 拉伸或裁剪画面，并提供“位置复位”与左移 / 右移 / 上移 / 下移微调，菜单结构与 PotPlayer 一致
- 下边距：`视频 → 下边距` 可设置 5%–30% 的画面下边距（`Ctrl+Alt+B` 按序切换），并支持“有字幕则显示下边距”与“强制生成下边距”，缩小画面以便放置字幕
- 声音菜单：音量增减与静音、选择声音（立体声 / 左声道 / 右声道）、左右声道互换，以及声音匹配（同步）滞后 / 超前 50ms 微调与复位，全部与 PotPlayer 菜单同名同结构；`声音 → 声音输出轨道` 会按当前文件动态列出全部音轨（含语言标签），可在播放中即时切换
- 滤镜菜单：`滤镜` 子菜单按当前生效的滤镜动态列出画面调整、画面旋转、水平 / 垂直翻转、3D 视频输出、360° 全景播放、左右声道互换与声音同步，点击任一项即可即时关闭该滤镜，无滤镜时显示“无正在使用的滤镜”
- 播放书签：`H` 在当前画面位置添加书签，菜单中可跳转到上一个/下一个书签或清除（每个画面独立记录，按文件路径持久化到配置文件）
- 记忆播放位置：默认开启，再次打开同一文件时从上次停止的位置继续播放（播放菜单可关闭）
- 进度条标记：在进度条上显示当前文件的书签（蓝色）与章节（灰色）刻度，可在“播放”菜单中关闭
- 内嵌章节：读取 MP4 / MKV 等容器中的章节标记，在“播放 → 章节/书签”中列出章节名与起始时间，点击或按 `[` / `]` 跳转
- 收藏夹：`Alt+Insert` 收藏当前文件、`Ctrl+Insert` 收藏当前文件夹，收藏项写入配置文件，可从“收藏”菜单直接打开文件或浏览文件夹
- 收尾处理：播放结束后可选择不处理、播放完当前后退出、播放完全部后退出、播放完当前后关机或播放完全部后关机，`Ctrl+Alt+A` 按序切换，设置保存到配置
- 关机方式：选择关机时可改用待机或休眠代替，播放结束触发收尾时先弹出确认对话框（关机 / 待机 / 休眠与取消），确认后才会执行系统操作
- 记忆收尾操作：开启时记住上次选择的收尾方式并在下次启动时沿用；关闭后每次启动都从“不处理”开始
- 置顶方式：“配置/语言/其他 → 置顶方式”可在切换最前端（`Ctrl+T`）、总在最前端、播放时总在最前端、全屏时总在最前端之间选择，当前方式在菜单中显示勾选，设置保存到配置
- 辅助处理：“配置/语言/其他 → 辅助处理”可将当前文件名、所在文件夹或完整路径复制到剪贴板（xclip / xsel / wl-copy）
- 皮肤选项：“皮肤”菜单实时列出内置与外部皮肤（当前皮肤带 ✓，可直接切换并保存），并可在“配色”中选择强调色（自动跟随皮肤或蓝/红/绿/紫/橙/青/银），在“不透明度”中即时切换 40%–100% 的菜单不透明度；菜单还可切换“视频下自动隐藏”与“全屏时不遮盖画面”
- 徽标：“皮肤 → 常用徽标 / 播放时的徽标”可从文件对话框选择 PNG / JPG / BMP / GIF / WebP / ICO / TIFF 图片作为画面右上角徽标，勾选“显示常用徽标 / 显示播放时徽标”控制显隐，停止播放时显示常用徽标、播放时显示播放时徽标；图片路径与显隐状态保存到配置，“清除徽标”移除当前图片
- 皮肤设置：“皮肤 → 皮肤设置...”打开对话框，皮肤文件列表实时读取内置皮肤（默认 / 暗黑 / 琥珀 / 青绿）与 `~/.config/flintplayer/skins` 下的 `.fskin` 外部皮肤，点击条目即可预览主题色与菜单底色；“浏览...”打开文件对话框载入任意位置的 `.fskin` 并加入列表；不透明度滑块（40%–100%）即时作用于弹出菜单与子菜单；“确定”保存到配置，“取消”或 `Esc` 还原本次修改
- 皮肤文件：`.fskin` 为 `键=值` 文本，支持 `name`（显示名称）、`accent`（强调色，用于书签标记与侧栏选中项）、`menu_bg`（菜单底色）与 `menu_hover`（菜单悬停色），另可用 `mode=audio` / `mode=video` 指定切换该皮肤时进入的界面模式（受“不切换界面模式”控制），颜色为 `#RRGGBB` 或 `RRGGBB`
- 皮肤输出：“皮肤 → 图层面板皮肤输出”关闭后，控制栏不再覆盖画面，而是为画面让出底部的控制栏高度；“弹出菜单类型”可在“皮肤中定义的菜单”与“系统菜单”之间切换，“菜单输出方式”进一步选择系统菜单的“自动选择(推荐)”（浅色）或“黑暗系统菜单”（深色）
- 触控与视频皮肤：“皮肤 → 触控皮肤”把弹出菜单整体放大 1.25 倍，便于触控；“放大皮肤中定义的菜单”在未启用触控皮肤时也可把皮肤中定义的菜单放大到 1.25 倍；“以视频皮肤启动”启用时隐藏标题栏并在启动后自动隐藏控制栏，鼠标移动时控制栏重新出现
- 菜单与界面：“皮肤 → 查看菜单名称”关闭后隐藏子菜单中的分组标题，只保留可执行项；“不切换界面模式”勾选时切换皮肤仅更换配色，取消勾选且皮肤声明 `mode=video` 时同时切换到视频皮肤界面；“单独保存音频/视频界面位置/尺寸”启用后分别为普通界面与视频皮肤界面保存窗口位置与尺寸，切换界面模式或重启时自动还原
- 界面语言：“配置/语言/其他 → 语言”可在“自动选择(推荐)”“简体中文”“English”之间切换；自动选择按系统区域决定（`LC_ALL` / `LC_MESSAGES` / `LANG` 以 `zh` 开头时为中文，其余为英文）；切换立即生效，菜单、子菜单、对话框、信息 OSD 与状态提示随之更新，并保存到配置
- 信息 OSD：`Tab` 显示完整播放信息，`Shift+Tab` 显示简要信息（文件名与时间轴），`Scroll Lock` 显示播放信息（状态、速度、音量与时间），也可从“配置/语言/其他”菜单切换
- 忽略默认快捷键：开启后播放器不再响应内置快捷键，便于把按键让给系统或全局热键，设置保存到配置
- 收藏记忆点：开启“在收藏菜单中显示播放记忆点”后，收藏菜单会在每个收藏文件下列出续播位置与书签，点击即可从该位置开始播放
- 相似文件操作：“配置/语言/其他 → 相似文件操作”可选择仅打开选定的文件、同时打开相似文件（同一文件夹中名称模式相同的文件）或同时打开全部文件，后两者会把同目录文件一并加入播放面板，设置保存到配置
- 使用触屏功能：开启后可在画面上左右拖动按比例跳转进度，关闭后拖动不再跳转，设置保存到配置
- 配置管理：“配置/语言/其他 → 配置管理...”可把当前设置另存为命名配置、重命名或删除配置，并从列表载入默认配置或已保存的配置，配置保存在 `~/.config/flintplayer/profiles/`
- 关于对话框：`F1` 或菜单“关于...”显示版本号与构建信息，可按“确定”或 `Esc` 关闭

## 快捷键

| 按键 | 功能 |
| --- | --- |
| `Tab` | 显示/隐藏播放信息 OSD |
| `Shift+Tab` | 显示/隐藏简要信息 |
| `Scroll Lock` | 显示/隐藏播放信息 |
| `Space` | 冻结/恢复画面显示 |
| `S` / `K` | 将当前 RGBA 帧保存为无损 PNG |
| `R` | 开始/停止录制当前画面 |
| `Enter` / `F` / `Alt+Enter` | 切换全屏 |
| `Esc` | 关闭菜单/对话框；非全屏时退出全屏 |
| `F1` | 关于 FlintPlayer |
| `F2` | 打开文件夹（加入播放列表） |
| `F3` / `Ctrl+O` | 打开文件 |
| `Ctrl+Y` | 重开当前/最后文件 |
| `F4` / `Ctrl+W` | 关闭当前画面 |
| `F5` / `Alt+D` | 打开设备设置 |
| `F6` | 显示/隐藏播放列表侧栏 |
| `PgUp` / `PgDn` | 上一个 / 下一个文件 |
| `,` / `.` | 逐帧后退 / 前进 |
| `M` | 静音 |
| `↑` / `↓` | 音量 + / - |
| `Shift+,` / `Shift+.` | 声音同步滞后 / 超前 50ms |
| `Shift+/` | 声音同步复位 |
| `Shift+T` | 左右声道互换 |
| `L` / `Alt+O` | 打开外部字幕（SRT / ASS / SSA / VTT，自动识别 UTF-8 与 GB18030） |
| `Alt+E` | 打开字幕浏览器（选择字幕文件） |
| `Alt+P` | 手动输入字幕（实时叠加到画面） |
| `Alt+H` | 显示/隐藏字幕 |
| `J` | 切换 3D 视频输出模式（关闭 / 红蓝立体 / 红蓝立体上下） |
| `W` / `E` | 亮度 -1% / +1% |
| `Y` / `U` | 饱和度 -1% / +1% |
| `Q` | 图像属性复位（亮度 / 对比度 / 饱和度） |
| `Ctrl+P` / `Ctrl+Z` | 上下翻转 / 左右翻转画面 |
| `Ctrl+Alt+B` | 按序切换下边距（无 / 5% / 10% / 15% / 20% / 25% / 30%） |
| `Alt+K` | 按序选择旋转方式（0° / 90° / 180° / 270°） |
| `Alt+V` | 按序切换活动画面（图像顺序选择） |
| `Ctrl+Alt+A` | 按序切换收尾处理（不处理 / 播放完当前后退出 / 播放完全部后退出 / 播放完当前后关机 / 播放完全部后关机） |
| `Ctrl+T` | 切换最前端置顶（开 / 关） |
| `C` / `X` / `Z` | 加速 / 减速 / 恢复正常速度 |
| `B` | AB 区段循环：依次设置 A 点、B 点、关闭 |
| `H` | 在当前播放位置添加书签（菜单“播放 → 章节/书签”可跳转/清除） |
| `[` / `]` | 跳到上一 / 下一章节（文件包含内嵌章节时可用） |
| `Alt+Insert` | 收藏当前文件 |
| `Ctrl+Insert` | 收藏当前文件夹 |
| `Ctrl+G` | 打开连续截图设置（间隔与张数） |
| `Alt+N` | 创建当前画面的缩略图（可设置宽度） |

截图默认保存到 `~/图片/FlintPlayer`（遵循系统图片目录），文件名形如 `flintplayer_<毫秒时间戳>.png`。“视频 → 图像截取”可切换 PNG / JPEG / BMP 格式、以当前时间或播放时间命名、保存到播放文件所在文件夹，并可打开截图文件夹或删除最近的采集文件。“连续截图...”按设定的间隔（100–60000 毫秒）与张数（1–100）自动连拍，“立即连续截图...”使用上次设定立即开始，“选择截图存档路径...”可修改保存目录并立即生效；“创建缩略图...”用当前画面生成 `<视频名>_thumb.<格式>`，宽度可设为 64–3840 像素。

配置文件位于 `~/.config/flintplayer/config.ini`，记录上次使用的设备、分辨率、窗口位置、OSD 开关、收藏夹、书签与播放位置；多路设备以 `sources=/dev/video0|/dev/video2` 的形式保存，收藏项以 `favorites=<路径>|<路径>` 保存，书签以 `bookmarks=<路径>\t<秒>,<秒>` 保存，续播位置以 `resume=<路径>\t<秒>` 保存，字幕样式以 `subtitle_scale` / `subtitle_color` / `subtitle_top` / `subtitle_font` / `subtitle_bold` / `subtitle_shadow` 保存，字幕输出方式以 `subtitle_graphic_outside` / `subtitle_text_inside` / `subtitle_bottom` / `subtitle_border_opaque` / `subtitle_smooth_vmr` 保存，ASS/SSA 样式读取以 `ass_styles` 保存，3D 字幕显示以 `subtitle_3d`（`off` / `sbs` / `tab` / `anaglyph`）保存，首选字幕语言以 `subtitle_language`（`auto` / `chi` / `eng` / `jpn` / `kor`）保存，3D 输出模式以 `stereo_mode` 保存，360° 播放与视角以 `pano_enabled` / `pano_yaw` / `pano_pitch` / `pano_fov` 保存，字幕朗读以 `tts_enabled` 保存，实时字幕翻译以 `translate_enabled` 保存，画面调整以 `brightness` / `contrast` / `saturation` / `rotation` 保存，翻转以 `flip_horizontal` / `flip_vertical` 保存，截图设置以 `snapshot_dir` / `snapshot_format` / `snapshot_name` / `snapshot_play_folder` 与连续截图的 `snapshot_interval_ms` / `snapshot_count`、缩略图宽度 `thumbnail_width` 保存，收尾处理以 `post_action` 保存，关机方式以 `shutdown_mode`（`poweroff` / `suspend` / `hibernate`）保存，记忆收尾操作以 `remember_post_action` 保存，置顶方式以 `topmost_mode` 保存，“视频下自动隐藏”与“全屏时不遮盖画面”以 `autohide_controls` / `fullscreen_no_cover` 保存，忽略默认快捷键与收藏记忆点以 `ignore_shortcuts` / `favorites_memory_points` 保存，相似文件操作以 `similar_mode`（`selected` / `similar` / `all`）保存，使用触屏功能以 `touch_enabled` 保存，“仅在标题栏上移动窗口”以 `move_only_titlebar` 保存，皮肤不透明度与皮肤文件以 `skin_opacity` / `skin` 保存，菜单配色以 `menu_accent`（空 / `blue` / `red` / `green` / `purple` / `orange` / `cyan` / `silver`）保存，画面徽标以 `logo_file` / `logo_play_file` 与 `logo_show` / `logo_play_show` 保存，皮肤输出选项以 `skin_layer_output` / `popup_menu_type`（`skin` / `system`）/ `menu_output_mode`（`auto` / `dark`）/ `touch_skin` / `video_skin_start` 保存，界面语言以 `language`（`auto` / `zh` / `en`）保存，AviSynth / VapourSynth 滤镜以 `avisynth_enabled` / `avisynth_script` 与 `vapoursynth_enabled` / `vapoursynth_script` 保存，菜单显示与界面尺寸选项以 `menu_show_names` / `enlarge_skin_menu` / `keep_ui_mode` / `separate_geometry` 保存，分界面窗口几何以 `video_window_w` / `video_window_h` / `video_window_x` / `video_window_y` 与 `audio_window_w` / `audio_window_h` / `audio_window_x` / `audio_window_y` 保存。

## 多路监看

在设备设置（`F5` / `Alt+D`）或菜单的“打开”子菜单中选择设备时，若画面未满 4 路，会在新格位中追加；已满 4 路时替换当前活动画面。

单击任意画面将其设为活动画面，双击切换该画面的单路全屏与宫格视图，活动画面右上角显示关闭按钮（单路视图下鼠标移入时显示）。也可从“视频 → 选择图像”直接选取第 1–4 路画面，或用 `Alt+V` 按序切换。

设备菜单的分辨率选择、截图（`S`）、冻结（`Space`）、全屏（`Enter` / `F`）与 Tab OSD 均作用于活动画面，OSD 与底部控制栏都显示在活动画面内（OSD 位于左上角），切换活动画面时一同移动。

## 构建与运行

系统依赖（Debian/Ubuntu）：

```bash
sudo apt install build-essential pkg-config \
    libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev \
    libfontconfig1-dev libxkbcommon-dev fonts-noto-cjk
```

```bash
cargo run -p flintplayer-desktop
cargo build --release -p flintplayer-desktop   # 产物：target/release/flintplayer
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

打包工具依赖 `cargo-deb`（可通过 `cargo install cargo-deb` 安装），支持本地动态链接与发布版静态链接两种构建方式。

### 1. 本地动态构建

默认动态链接当前系统的 FFmpeg 库（如 `libavcodec`、`libavformat` 等），适合在本地开发或同版本发行版上使用：

```bash
cargo deb -p flintplayer-desktop
```

### 2. 发布版静态构建

官方发布包采用 FFmpeg 6.1.2 静态库编译，二进制不再依赖系统 `libavcodec`、`libavformat` 等动态库；仅依赖 `glibc >= 2.35`（不适用于 `musl`），实际支持 `glibc 2.36` 及以上的发行版。

编译静态 FFmpeg 6.1.2 时需启用以下参数：

```bash
./configure \
  --prefix=/path/to/ffmpeg-static \
  --disable-autodetect \
  --enable-static \
  --disable-shared \
  --enable-pic \
  --enable-v4l2-m2m \
  --enable-indev=v4l2
make -j"$(nproc)" && make install
```

随后指定 `FFMPEG_DIR` 编译二进制并生成 `.deb` 包：

```bash
FFMPEG_DIR=/path/to/ffmpeg-static cargo build --release -p flintplayer-desktop --features static-ffmpeg
cargo deb -p flintplayer-desktop --no-build
```

### 运行时依赖

静态构建的 `.deb` 包不再依赖系统 FFmpeg 动态库，自动依赖仅保留：

- `libmpv2 | libmpv1`
- `libvulkan1`、`mesa-vulkan-drivers`
- `libxkbcommon`、`libxcb`
- `fonts-noto-cjk`

`sudo apt install ./target/debian/flintplayer_1.0.0-1_amd64.deb` 即可安装。

## 架构

```
flintplayer-desktop ──> flint-media ──> flint-core
         │                   │                │
      GPUI 界面        采集/解码管线    设备枚举、命令、统计、配置
         │
    flint-engine（libmpv 播放引擎）
```

```
V4L2 / 网络流
   → CaptureEngine（字节计数、Mbps 统计）
   → FFmpeg 解码（MJPEG / H.264 / YUYV / NV12 → RGBA8888，记录单帧解码耗时）
   → Latest Frame Slot（深度 1，新帧覆盖旧帧，Dropped Frames +1）
   → GPUI Presenter（16ms 轮询最新帧，统计实际 FPS）
```

### 原生 V4L2 而非 FFmpeg `device` 输入

目标环境缺少 libavdevice 的开发文件与 pkg-config 条目，FFmpeg 的 `device` feature 无法编译。因此 `flint-media/capture_v4l2.rs` 直接通过 `ioctl` 完成格式枚举、请求缓冲、`DQBUF`/`QBUF` 与 `STREAMON`；FFmpeg 只负责解码与像素格式转换。这样既保留了低延迟的 mmap 采集路径，也避免了额外运行时依赖。V4L2 缓冲区通过 `mmap` 直接映射，解码前的未压缩帧按行拷入复用的 `AVFrame`，压缩帧每包拷入一个 `AVPacket`。

### 低延迟要点

- V4L2 打开 `O_NONBLOCK`，`DQBUF` 120ms 超时轮询，停止标志可及时退出
- FFmpeg 解码器启用 `AV_CODEC_FLAG_LOW_DELAY`，线程数固定为 1
- 最新帧槽深度固定为 1，UI 线程只取最新帧，不排空历史帧
- RGBA 转换缓冲从帧槽缓冲池复用，媒体侧稳态下无逐帧 RGBA `Vec` 分配；渲染时上传到 GPUI `DynamicTexture`
- 设备断开、启动失败、线程异常均以状态错误返回，不 panic

## 目录

```
crates/flint-core     设备枚举、命令、统计、配置
crates/flint-media    V4L2/网络采集、FFmpeg 解码、最新帧槽
crates/flint-gpui      GPUI 界面、快捷键、菜单树、应用装配
crates/flint-engine    libmpv 播放引擎
crates/flint-ui        旧 Slint 界面（已从 workspace 移除，保留备查）
assets                .desktop 启动器与 256×256 图标
```

## 许可

MIT，见 [LICENSE](LICENSE)。
