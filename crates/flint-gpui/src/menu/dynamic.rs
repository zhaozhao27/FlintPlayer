use flint_core::device::DeviceInfo;
use flint_engine::{ChapterInfo, TrackInfo};

use super::MenuEntry;

const MAX_VISIBLE_CHAPTERS: usize = 40;
const SPEEDS: [f32; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
const HOLD_BUTTONS: [(&str, &str); 3] = [
    ("left", "左键"),
    ("right", "右键"),
    ("middle", "中键(滚轮)"),
];

pub const EXTRA_ACTIONS: &[&str] = &[
    "ab-a",
    "ab-b",
    "ab-off",
    "audio-open-folder",
    "audio-record-play-folder-toggle",
    "audio-record-toggle",
    "chapter-next",
    "chapter-prev",
    "load-audio",
    "mute-toggle",
    "play-pause",
    "remember-position-toggle",
    "repeat-toggle",
    "seek-back-10",
    "seek-end",
    "seek-forward-10",
    "seek-start",
    "shuffle-toggle",
    "speed-down",
    "speed-up",
    "step-backward",
    "step-forward",
    "subtitle-open",
    "track:a:no",
    "track:s:no",
    "volume-down",
    "volume-up",
];

pub struct MenuData {
    pub chapters: Vec<ChapterInfo>,
    pub tracks: Vec<TrackInfo>,
    pub devices: Vec<DeviceInfo>,
    pub speed: f64,
    pub hold_left_speed: f64,
    pub hold_right_speed: f64,
    pub hold_middle_speed: f64,
    pub repeat: bool,
    pub shuffle: bool,
    pub remember_position: bool,
    pub has_prev: bool,
    pub has_next: bool,
    pub whisper: bool,
}

pub fn apply_dynamic(entries: &mut [MenuEntry], data: &MenuData) {
    for entry in entries.iter_mut() {
        let key = entry.dynamic.as_deref().unwrap_or(entry.title.as_str());
        match key {
            "play" | "播放" => replace(entry, play_menu(data)),
            "subtitle" | "字幕" => replace(entry, subtitle_menu(data)),
            "sound" | "声音" => replace(entry, sound_menu(data)),
            "device" | "摄像头/其他设备" => replace(entry, device_menu(&data.devices)),
            _ => apply_dynamic(&mut entry.children, data),
        }
    }
}

fn replace(entry: &mut MenuEntry, mut menu: MenuEntry) {
    if !entry.title.is_empty() {
        menu.title = std::mem::take(&mut entry.title);
    }
    menu.dynamic = entry.dynamic.take();
    *entry = menu;
}

fn device_menu(devices: &[DeviceInfo]) -> MenuEntry {
    if devices.is_empty() {
        return MenuEntry::submenu(
            "摄像头/其他设备",
            vec![MenuEntry::action("未检测到采集设备", "").disabled()],
        );
    }
    let items = devices
        .iter()
        .map(|device| {
            let name = if device.name.is_empty() {
                device.path.display().to_string()
            } else {
                format!("{} ({})", device.name, device.path.display())
            };
            MenuEntry::action(
                name,
                format!("capture:{}|{}", device.path.display(), device.name),
            )
        })
        .collect();
    MenuEntry::submenu("摄像头/其他设备", items)
}

fn play_menu(data: &MenuData) -> MenuEntry {
    let mut chapter_items = Vec::new();
    if data.chapters.is_empty() {
        chapter_items.push(MenuEntry::action("无章节信息", "").disabled());
    } else {
        for (index, chapter) in data.chapters.iter().take(MAX_VISIBLE_CHAPTERS).enumerate() {
            let title = if chapter.title.is_empty() {
                format!("章节 {}", index + 1)
            } else {
                chapter.title.clone()
            };
            chapter_items.push(
                MenuEntry::action(title, format!("chapter:{index}"))
                    .with_shortcut(format_seconds(chapter.start)),
            );
        }
        if data.chapters.len() > MAX_VISIBLE_CHAPTERS {
            chapter_items.push(
                MenuEntry::action(
                    format!("还有 {} 个章节未显示", data.chapters.len() - MAX_VISIBLE_CHAPTERS),
                    "",
                )
                .disabled(),
            );
        }
        chapter_items.push(MenuEntry::separator());
        chapter_items.push(MenuEntry::action("上一章节", "chapter-prev").with_shortcut("["));
        chapter_items.push(MenuEntry::action("下一章节", "chapter-next").with_shortcut("]"));
    }

    let mut speed_items = Vec::new();
    for speed in SPEEDS {
        let mut item = MenuEntry::action(speed_label(speed), format!("speed:{speed}"))
            .checked((data.speed - speed as f64).abs() < 0.05);
        if (speed - 1.0).abs() < f32::EPSILON {
            item = item.with_shortcut("Z");
        }
        speed_items.push(item);
    }
    speed_items.push(MenuEntry::action("更慢", "speed-down").with_shortcut("X"));
    speed_items.push(MenuEntry::action("更快", "speed-up").with_shortcut("C"));

    MenuEntry::submenu(
        "播放",
        vec![
            MenuEntry::action("播放/暂停", "play-pause").with_shortcut("空格键"),
            MenuEntry::action("上一文件", "prev-panel")
                .with_shortcut("PgUp")
                .disabled_if(!data.has_prev),
            MenuEntry::action("下一文件", "next-panel")
                .with_shortcut("PgDn")
                .disabled_if(!data.has_next),
            MenuEntry::separator(),
            MenuEntry::submenu("章节/书签", chapter_items),
            MenuEntry::submenu(
                "定位",
                vec![
                    MenuEntry::action("跳到开头", "seek-start"),
                    MenuEntry::action("跳到结尾", "seek-end"),
                    MenuEntry::action("后退 10 秒", "seek-back-10"),
                    MenuEntry::action("前进 10 秒", "seek-forward-10"),
                    MenuEntry::separator(),
                    MenuEntry::action("上一帧", "step-backward").with_shortcut(","),
                    MenuEntry::action("下一帧", "step-forward").with_shortcut("."),
                ],
            ),
            MenuEntry::submenu("播放速度", speed_items),
            hold_speed_menu(data),
            MenuEntry::submenu(
                "播放列表",
                vec![
                    MenuEntry::submenu(
                        "添加",
                        vec![
                            MenuEntry::action("添加文件...", "playlist-add-file"),
                            MenuEntry::action("添加文件夹...", "playlist-add-folder")
                                .with_shortcut("F2"),
                        ],
                    ),
                    MenuEntry::submenu(
                        "编辑",
                        vec![
                            MenuEntry::action("移除所选名单", "").disabled(),
                            MenuEntry::action("编辑列表...", "").disabled(),
                        ],
                    ),
                    MenuEntry::submenu(
                        "专辑",
                        vec![
                            MenuEntry::action("新建专辑...", "").disabled(),
                            MenuEntry::action("编辑专辑...", "").disabled(),
                            MenuEntry::action("删除专辑", "").disabled(),
                            MenuEntry::action("播放自选专辑", "").disabled(),
                            MenuEntry::action("打开列表文件夹...", "").disabled(),
                        ],
                    ),
                    MenuEntry::submenu(
                        "排序",
                        vec![
                            MenuEntry::action("按文件名正序", "playlist-sort-name"),
                            MenuEntry::action("按文件名反序", "playlist-sort-reverse"),
                        ],
                    ),
                    MenuEntry::action("清空播放列表", "playlist-clear"),
                ],
            ),
            MenuEntry::submenu(
                "AB 区段循环",
                vec![
                    MenuEntry::action("设置 A 点", "ab-a"),
                    MenuEntry::action("设置 B 点", "ab-b"),
                    MenuEntry::action("关闭 AB 循环", "ab-off"),
                ],
            ),
            MenuEntry::action("循环播放", "repeat-toggle").checked(data.repeat),
            MenuEntry::separator(),
            MenuEntry::action("无序(随机)播放", "shuffle-toggle").checked(data.shuffle),
            MenuEntry::action("记忆播放位置", "remember-position-toggle")
                .checked(data.remember_position),
            MenuEntry::separator(),
            MenuEntry::action("播放设置...", "device-settings"),
        ],
    )
}

fn hold_speed_menu(data: &MenuData) -> MenuEntry {
    let current = [data.hold_left_speed, data.hold_right_speed, data.hold_middle_speed];
    let items = HOLD_BUTTONS
        .iter()
        .zip(current)
        .map(|((button, label), value)| {
            let speeds: Vec<MenuEntry> = flint_core::config::HOLD_SPEED_PRESETS
                .iter()
                .map(|speed| {
                    MenuEntry::action(
                        hold_speed_label(*speed),
                        format!("hold-speed:{button}:{speed}"),
                    )
                    .checked((value - speed).abs() < f64::EPSILON)
                })
                .collect();
            MenuEntry::submenu(*label, speeds)
        })
        .collect();
    MenuEntry::submenu("按住鼠标键时", items)
}

fn hold_speed_label(speed: f64) -> String {
    if speed == 0.0 {
        "不使用".to_string()
    } else {
        format!("按下时以 {speed} 倍速度播放")
    }
}

fn subtitle_menu(data: &MenuData) -> MenuEntry {
    let tracks: Vec<&TrackInfo> = data.tracks.iter().filter(|track| track.kind == "sub").collect();
    let mut items = vec![
        MenuEntry::action("载入字幕...", "subtitle-open").with_shortcut("Alt+O"),
        MenuEntry::separator(),
        MenuEntry::action("关闭字幕", "track:s:no")
            .checked(tracks.iter().all(|track| !track.selected)),
    ];
    for (index, track) in tracks.iter().enumerate() {
        items.push(
            MenuEntry::action(track_label(track, index, "字幕"), format!("track:s:{}", track.id))
                .checked(track.selected),
        );
    }
    items.push(MenuEntry::separator());
    items.extend(super::generated::ass_engines());
    items.push(MenuEntry::separator());
    items.push(
        MenuEntry::action("生成有声字幕", "whisper-generate").disabled_if(!data.whisper),
    );
    MenuEntry::submenu("字幕", items)
}

fn sound_menu(data: &MenuData) -> MenuEntry {
    let tracks: Vec<&TrackInfo> = data
        .tracks
        .iter()
        .filter(|track| track.kind == "audio")
        .collect();
    let mut items = vec![
        MenuEntry::action("载入音频...", "load-audio"),
        MenuEntry::separator(),
        MenuEntry::action("关闭声音", "track:a:no")
            .checked(tracks.iter().all(|track| !track.selected)),
    ];
    for (index, track) in tracks.iter().enumerate() {
        items.push(
            MenuEntry::action(track_label(track, index, "音轨"), format!("track:a:{}", track.id))
                .checked(track.selected),
        );
    }
    items.push(MenuEntry::separator());
    items.push(MenuEntry::action("音量增大", "volume-up").with_shortcut("↑"));
    items.push(MenuEntry::action("音量减小", "volume-down").with_shortcut("↓"));
    items.push(MenuEntry::action("静音", "mute-toggle").with_shortcut("M"));
    items.push(MenuEntry::separator());
    items.push(MenuEntry::submenu(
        "声音输出方式",
        vec![
            MenuEntry::action("自动选择(推荐)", "audio-device:auto"),
            MenuEntry::action("PulseAudio", "audio-device:pulse"),
            MenuEntry::action("ALSA", "audio-device:alsa"),
            MenuEntry::action("JACK", "audio-device:jack"),
            MenuEntry::action("空设备(静音)", "audio-device:null"),
        ],
    ));
    items.push(MenuEntry::submenu(
        "音轨",
        vec![
            MenuEntry::action("声音录制...", "audio-record-toggle").with_shortcut("Shift+G"),
            MenuEntry::action("立即开始录制声音...", "audio-record-toggle"),
            MenuEntry::action("打开声音录制存档文件夹", "audio-open-folder"),
            MenuEntry::action("条件允许时储存在播放文件夹", "audio-record-play-folder-toggle"),
        ],
    ));
    MenuEntry::submenu("声音", items)
}

fn track_label(track: &TrackInfo, index: usize, fallback: &str) -> String {
    let name = if track.title.is_empty() {
        format!("{fallback} {}", index + 1)
    } else {
        track.title.clone()
    };
    if track.lang.is_empty() {
        name
    } else {
        format!("{name} ({})", track.lang)
    }
}

fn speed_label(speed: f32) -> String {
    if (speed - 1.0).abs() < f32::EPSILON {
        "正常速度".to_string()
    } else {
        format!("{speed}倍")
    }
}

fn format_seconds(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let minutes = total / 60;
    let secs = total % 60;
    format!("{minutes:02}:{secs:02}")
}
