use gpui::*;

use super::controller::{DropdownKind, SettingsController};
use crate::features::player_frame::PlayerFrameController;
use crate::i18n::tr;

const PANEL_WIDTH: f32 = 660.0;
const PANEL_HEIGHT: f32 = 540.0;
const CONTENT_TOP: f32 = 54.0;
const CONTENT_HEIGHT: f32 = PANEL_HEIGHT - CONTENT_TOP;
const TAB_STRIP_TOP: f32 = 28.0;
const TAB_STRIP_WIDTH_USED: f32 = 388.0;

const PANEL_BG: u32 = 0xf0f0f0;
const PANEL_BORDER: u32 = 0x6a6a6a;
const TITLE_BG: u32 = 0x1f1f1f;
const TITLE_TEXT: u32 = 0xeeeeee;
const TITLE_CLOSE: u32 = 0xd0d0d0;
const GROUP_BORDER: u32 = 0xb8b8b8;
const GROUP_TITLE: u32 = 0x1c1c1c;
const CONTROL_BORDER: u32 = 0x9a9a9a;
const CONTROL_ARROW: u32 = 0x555555;
const CONTROL_ARROW_DIM: u32 = 0xb0b0b0;
const CONTROL_BG: u32 = 0xffffff;
const CONTROL_BG_DISABLED: u32 = 0xececec;
const CONTROL_TEXT: u32 = 0x1a1a1a;
const CONTROL_TEXT_DISABLED: u32 = 0x9a9a9a;
const CONTROL_TEXT_PLACEHOLDER: u32 = 0xb0b0b0;
const TAB_BG: u32 = 0xe3e3e3;
const TAB_BG_ACTIVE: u32 = 0xf5f5f5;
const TAB_TEXT: u32 = 0x3a3a3a;
const TAB_TEXT_ACTIVE: u32 = 0x111111;
const TAB_BORDER: u32 = 0xa0a0a0;
const BUTTON_BG: u32 = 0xf4f4f4;
const BUTTON_BG_HOVER: u32 = 0xe6e6e6;
const BUTTON_BORDER: u32 = 0x8f8f8f;
const OPTION_HOVER: u32 = 0x3a8fd9;
const CHECK_BORDER: u32 = 0x7a7a7a;
const CHECK_BORDER_DISABLED: u32 = 0xbcbcbc;
const CHECK_MARK: u32 = 0x1a5fb4;

pub fn settings_overlay(
    settings: &Entity<SettingsController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let (
        device_names,
        device_index,
        format_names,
        format_index,
        screen_names,
        screen_label,
        tab,
        dropdown,
    ) = {
        let controller = settings.read(cx);
        let screen_names = controller
            .screen_sources()
            .iter()
            .map(|source| match source.window {
                Some(_) => format!("窗口：{} ({}×{})", source.title, source.width, source.height),
                None => "X11 屏幕采集 (XShm)".to_string(),
            })
            .collect::<Vec<_>>();
        let screen_label = controller.screen_label();
        let device_names = controller
            .devices()
            .iter()
            .map(|device| format!("{} ({})", device.name, device.path.display()))
            .collect::<Vec<_>>();
        let format_names = controller
            .modes()
            .iter()
            .map(|mode| {
                format!(
                    "{} {}×{}@{}",
                    mode.fourcc,
                    mode.width,
                    mode.height,
                    format_fps(mode.fps)
                )
            })
            .collect::<Vec<_>>();
        (
            device_names,
            controller.device_index(),
            format_names,
            controller.mode_index(),
            screen_names,
            screen_label,
            controller.tab(),
            controller.dropdown(),
        )
    };
    let (capture_width, capture_height, capture_fps) = {
        let controller = settings.read(cx);
        controller.capture_size()
    };
    let (
        output_label,
        audio_label,
        time_positioning,
        audio_monitor,
        manual_device,
        split_enabled,
        split_available,
    ) = {
        let controller = settings.read(cx);
        (
            controller.output_label(),
            controller.audio_label(),
            controller.time_positioning(),
            controller.audio_monitor(),
            controller.manual_device().map(str::to_string),
            controller.split_enabled(),
            controller.split_available(),
        )
    };

    let backdrop = div()
        .id("settings-backdrop")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .bg(rgba(0x00000090))
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                if !settings.update(cx, |settings, cx| settings.close_dropdown(cx)) {
                    settings.update(cx, |settings, cx| settings.close(cx));
                }
            }
        });

    let mut title_bar = div()
        .absolute()
        .left_0()
        .top_0()
        .w(px(PANEL_WIDTH))
        .h(px(28.0))
        .bg(rgb(TITLE_BG))
        .child(
            div()
                .absolute()
                .left(px(10.0))
                .top_0()
                .w(px(PANEL_WIDTH - 50.0))
                .h_full()
                .flex()
                .items_center()
                .text_size(px(12.0))
                .text_color(rgb(TITLE_TEXT))
                .child(tr("设备设置")),
        )
        .child(
            div()
                .absolute()
                .left(px(PANEL_WIDTH - 30.0))
                .top_0()
                .w(px(20.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .text_color(rgb(TITLE_CLOSE))
                .child("✕"),
        );
    title_bar = title_bar.child(
        div()
            .id("settings-close")
            .absolute()
            .left(px(PANEL_WIDTH - 36.0))
            .top_0()
            .w(px(36.0))
            .h_full()
            .on_click({
                let settings = settings.clone();
                move |_, _, cx: &mut App| {
                    settings.update(cx, |settings, cx| settings.close(cx));
                }
            }),
    );

    let mut tab_strip = div()
        .absolute()
        .left_0()
        .top(px(TAB_STRIP_TOP))
        .w(px(PANEL_WIDTH))
        .h(px(26.0))
        .flex();
    for (index, (label, width)) in [
        ("采集器", 70.0),
        ("摄像头", 70.0),
        ("模拟 TV", 76.0),
        ("数字 TV", 76.0),
        ("DVD/蓝光碟", 96.0),
    ]
    .into_iter()
    .enumerate()
    {
        let active = tab == index;
        tab_strip = tab_strip.child(
            div()
                .id(("settings-tab", index))
                .w(px(width))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(if active { TAB_BG_ACTIVE } else { TAB_BG }))
                .border_1()
                .border_color(rgb(TAB_BORDER))
                .text_size(px(11.0))
                .text_color(rgb(if active { TAB_TEXT_ACTIVE } else { TAB_TEXT }))
                .on_click({
                    let settings = settings.clone();
                    move |_, _, cx: &mut App| {
                        settings.update(cx, |settings, cx| settings.set_tab(index, cx));
                    }
                })
                .child(tr(label)),
        );
    }
    tab_strip = tab_strip.child(
        div()
            .w(px(PANEL_WIDTH - TAB_STRIP_WIDTH_USED))
            .h_full()
            .bg(rgb(TAB_BG)),
    );

    let device_label = manual_device
        .clone()
        .or_else(|| {
            device_index
                .and_then(|index| device_names.get(index).cloned())
        })
        .unwrap_or_else(|| tr("UVC 摄像头 (V4L2)"));
    let device_placeholder = device_index.is_none() && manual_device.is_none();
    let format_label = format_index
        .and_then(|index| format_names.get(index).cloned())
        .unwrap_or_else(|| tr("自动选择(推荐)"));

    let content_children = match tab {
        1 => camera_tab(
            settings,
            cx,
            &device_label,
            device_placeholder,
            &format_label,
            format_index.is_none(),
            &output_label,
            &audio_label,
            time_positioning,
            audio_monitor,
            split_enabled,
            split_available,
        ),
        2 => atv_tab(),
        3 => hdtv_tab(),
        4 => disc_tab(),
        _ => capture_tab(
            settings,
            cx,
            &screen_label,
            capture_width,
            capture_height,
            capture_fps,
        ),
    };

    let content = div()
        .absolute()
        .left_0()
        .top(px(CONTENT_TOP))
        .w(px(PANEL_WIDTH))
        .h(px(CONTENT_HEIGHT))
        .bg(rgb(PANEL_BG))
        .border_1()
        .border_color(rgb(0xc0c0c0))
        .children(content_children);

    let confirm = dlg_button("settings-confirm", 248.0, PANEL_HEIGHT - 36.0, 76.0, "确定(O)")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| settings.close(cx));
            }
        });
    let cancel = dlg_button("settings-cancel", 328.0, PANEL_HEIGHT - 36.0, 76.0, "取消(C)")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| settings.close(cx));
            }
        });
    let open_device = dlg_button(
        "settings-open-device",
        PANEL_WIDTH - 122.0,
        PANEL_HEIGHT - 36.0,
        96.0,
        "打开设备(O)",
    )
    .on_click(cx.listener(|this, _, _, cx| match this.settings.read(cx).tab() {
        0 => {
            this.settings.update(cx, |settings, cx| settings.close(cx));
            this.handle_action("capture-slot1".to_string(), cx);
        }
        1 => this
            .settings
            .update(cx, |settings, cx| settings.open_selected(cx)),
        _ => this.show_notice(crate::i18n::tr("该设备类型尚不支持"), cx),
    }));

    let popup = match dropdown {
        Some(DropdownKind::Screen) if tab == 0 && !screen_names.is_empty() => Some(
            dropdown_popup(
                108.0,
                114.0,
                360.0,
                &screen_names,
                settings,
                DropdownKind::Screen,
            )
            .into_any_element(),
        ),
        Some(DropdownKind::Device) if tab == 1 && !device_names.is_empty() => Some(
            dropdown_popup(
                108.0,
                114.0,
                340.0,
                &device_names,
                settings,
                DropdownKind::Device,
            )
            .into_any_element(),
        ),
        Some(DropdownKind::Format) if tab == 1 && !format_names.is_empty() => Some(
            dropdown_popup(
                334.0,
                142.0,
                310.0,
                &format_names,
                settings,
                DropdownKind::Format,
            )
            .into_any_element(),
        ),
        Some(DropdownKind::Output) if tab == 1 => {
            let names = settings.read(cx).output_names();
            Some(
                dropdown_popup(108.0, 142.0, 160.0, &names, settings, DropdownKind::Output)
                    .into_any_element(),
            )
        }
        Some(DropdownKind::Smoothing) if tab == 0 => Some(smoothing_popup(settings, cx)),
        Some(DropdownKind::Audio) if tab == 1 => Some(audio_popup(settings, cx)),
        _ => None,
    };

    let mut panel = div()
        .id("settings-panel")
        .relative()
        .w(px(PANEL_WIDTH))
        .h(px(PANEL_HEIGHT))
        .bg(rgb(PANEL_BG))
        .border_1()
        .border_color(rgb(PANEL_BORDER))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(title_bar)
        .child(tab_strip)
        .child(content)
        .child(confirm)
        .child(cancel)
        .child(open_device);
    if let Some(popup) = popup {
        let dismiss = div()
            .id("settings-dismiss")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .on_click({
                let settings = settings.clone();
                move |_, _, cx: &mut App| {
                    settings.update(cx, |settings, cx| settings.close_dropdown(cx));
                }
            });
        panel = panel.child(dismiss).child(popup);
    }

    div()
        .id("settings-overlay")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .child(backdrop)
        .child(panel)
        .into_any_element()
}

fn capture_tab(
    settings: &Entity<SettingsController>,
    cx: &mut Context<PlayerFrameController>,
    screen_label: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> Vec<AnyElement> {
    let device_dropdown = dropdown_box(100.0, 30.0, 360.0, screen_label, false, true)
        .id("settings-screen-dropdown")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| {
                    settings.toggle_dropdown(DropdownKind::Screen, cx)
                });
            }
        });
    let slot1 = dlg_button("settings-capture-slot1", 100.0, 86.0, 120.0, "显示 1# 截屏器")
        .on_click(cx.listener(|this, _, _, cx| {
            this.settings.update(cx, |settings, cx| settings.close(cx));
            this.handle_action("capture-slot1".to_string(), cx);
        }));
    let slot2 = dlg_button("settings-capture-slot2", 228.0, 86.0, 120.0, "显示 2# 截屏器")
        .on_click(cx.listener(|this, _, _, cx| {
            this.settings.update(cx, |settings, cx| settings.close(cx));
            this.handle_action("capture-slot2".to_string(), cx);
        }));
    let size1 = dlg_button(
        "settings-capture-size1",
        100.0,
        112.0,
        120.0,
        "1# 截屏器尺寸设置",
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.settings.update(cx, |settings, cx| settings.close(cx));
        this.handle_action("capture-size1".to_string(), cx);
    }));
    let size2 = dlg_button(
        "settings-capture-size2",
        228.0,
        112.0,
        120.0,
        "2# 截屏器尺寸设置",
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.settings.update(cx, |settings, cx| settings.close(cx));
        this.handle_action("capture-size2".to_string(), cx);
    }));
    let smoothing = dlg_button(
        "settings-capture-smoothing",
        356.0,
        112.0,
        140.0,
        "屏幕采集设置...",
    )
    .on_click({
        let settings = settings.clone();
        move |_, _, cx: &mut App| {
            settings.update(cx, |settings, cx| {
                settings.toggle_dropdown(DropdownKind::Smoothing, cx)
            });
        }
    });

    let mut capture_group = group(8.0, 8.0, 644.0, 150.0, "截屏器设置");
    capture_group = capture_group
        .child(field_label(14.0, 30.0, 82.0, "设备："))
        .child(device_dropdown)
        .child(field_label(14.0, 58.0, 82.0, "分辨率,FPS："))
        .child(capture_spin(cx, "settings-capture-width", 100.0, width))
        .child(text_box(
            170.0,
            58.0,
            12.0,
            22.0,
            11.0,
            CONTROL_TEXT,
            true,
            "×",
        ))
        .child(capture_spin(cx, "settings-capture-height", 186.0, height))
        .child(combo(
            264.0,
            58.0,
            180.0,
            &format!("{fps} FPS(推荐)"),
            "",
            true,
        ))
        .child(slot1)
        .child(slot2)
        .child(size1)
        .child(size2)
        .child(smoothing);

    let audio_device = tr("PulseAudio - 默认设备");
    let mut audio_group = group(8.0, 166.0, 644.0, 130.0, "音频采集设备");
    audio_group = audio_group
        .child(field_label(14.0, 34.0, 82.0, "设备："))
        .child(combo(100.0, 34.0, 360.0, &audio_device, "", true))
        .child(field_label(14.0, 60.0, 82.0, "输入："))
        .child(combo(100.0, 60.0, 360.0, "", "", false))
        .child(check(
            100.0,
            94.0,
            380.0,
            "采集在我的电脑上可听到的声音",
            false,
            true,
        ));

    vec![
        capture_group.into_any_element(),
        audio_group.into_any_element(),
    ]
}

fn camera_tab(
    settings: &Entity<SettingsController>,
    cx: &mut Context<PlayerFrameController>,
    device_label: &str,
    device_placeholder: bool,
    format_label: &str,
    format_placeholder: bool,
    output_label: &str,
    audio_label: &str,
    time_positioning: bool,
    audio_monitor: bool,
    split_enabled: bool,
    split_available: bool,
) -> Vec<AnyElement> {
    let device_dropdown = dropdown_box(
        100.0,
        30.0,
        340.0,
        device_label,
        device_placeholder,
        true,
    )
    .id("settings-device-dropdown")
    .on_click({
        let settings = settings.clone();
        move |_, _, cx: &mut App| {
            settings.update(cx, |settings, cx| {
                settings.toggle_dropdown(DropdownKind::Device, cx)
            });
        }
    });
    let format_dropdown = dropdown_box(
        326.0,
        58.0,
        310.0,
        format_label,
        format_placeholder,
        true,
    )
    .id("settings-format-dropdown")
    .on_click({
        let settings = settings.clone();
        move |_, _, cx: &mut App| {
            settings.update(cx, |settings, cx| {
                settings.toggle_dropdown(DropdownKind::Format, cx)
            });
        }
    });
    let output_dropdown = dropdown_box(100.0, 58.0, 160.0, output_label, false, true)
        .id("settings-output-dropdown")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| {
                    settings.toggle_dropdown(DropdownKind::Output, cx)
                });
            }
        });
    let audio_dropdown = dropdown_box(100.0, 86.0, 160.0, audio_label, false, true)
        .id("settings-audio-dropdown")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| {
                    settings.toggle_dropdown(DropdownKind::Audio, cx)
                });
            }
        });

    let audio_device = tr("PulseAudio - 默认设备");
    let mut video_group = group(8.0, 8.0, 644.0, 150.0, "视频录制设备");
    video_group = video_group
        .child(field_label(14.0, 30.0, 82.0, "设备："))
        .child(device_dropdown)
        .child(
            dlg_button("settings-camera-browse", 448.0, 30.0, 20.0, "...")
                .on_click(cx.listener(|this, _, _, cx| this.choose_camera_device(cx))),
        )
        .child(field_label(14.0, 58.0, 82.0, "输出："))
        .child(output_dropdown)
        .child(field_label(276.0, 58.0, 48.0, "格式："))
        .child(format_dropdown)
        .child(field_label(14.0, 86.0, 82.0, "音频："))
        .child(audio_dropdown)
        .child(dlg_button(
            "settings-camera-refresh",
            270.0,
            86.0,
            140.0,
            "检索更新",
        )
        .on_click(cx.listener(|this, _, _, cx| {
            let tab = this.settings.read(cx).tab();
            this.settings.update(cx, |settings, cx| settings.open(tab, cx));
        })));

    let mut audio_group = group(8.0, 166.0, 644.0, 126.0, "音频录制设备");
    audio_group = audio_group
        .child(field_label(14.0, 30.0, 82.0, "设备："))
        .child(combo(100.0, 30.0, 360.0, &audio_device, "", true))
        .child(field_label(14.0, 56.0, 82.0, "输入："))
        .child(combo(100.0, 56.0, 360.0, "", "", false))
        .child(
            check(
                100.0,
                88.0,
                400.0,
                "连接在我的电脑上能够听到的声音",
                audio_monitor,
                true,
            )
            .id("settings-audio-monitor")
            .on_click({
                let settings = settings.clone();
                move |_, _, cx: &mut App| {
                    settings.update(cx, |settings, cx| settings.toggle_audio_monitor(cx));
                }
            }),
        );

    let mut advanced_group = group(8.0, 300.0, 644.0, 58.0, "高级设置");
    advanced_group = advanced_group.child(
        check(
            14.0,
            30.0,
            300.0,
            "使用时间定位功能",
            time_positioning,
            true,
        )
        .id("settings-time-positioning")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| settings.toggle_time_positioning(cx));
            }
        }),
    );

    let split_group = group(8.0, 364.0, 644.0, 56.0, "多设备").child(
        check(
            14.0,
            30.0,
            420.0,
            "多设备同步采集（分屏显示）",
            split_enabled && split_available,
            split_available,
        )
        .id("settings-split")
        .on_click({
            let settings = settings.clone();
            move |_, _, cx: &mut App| {
                settings.update(cx, |settings, cx| settings.toggle_split(cx));
            }
        }),
    );

    vec![
        video_group.into_any_element(),
        audio_group.into_any_element(),
        advanced_group.into_any_element(),
        split_group.into_any_element(),
    ]
}

fn atv_tab() -> Vec<AnyElement> {
    tv_tab(
        "BDA(ATV) 设备",
        "未找到模拟 TV 设备",
    )
}

fn hdtv_tab() -> Vec<AnyElement> {
    tv_tab("BDA(HDTV) 设备", "未找到 BDA 设备")
}

fn tv_tab(title: &str, device_placeholder: &str) -> Vec<AnyElement> {
    let mut device_group = group(8.0, 8.0, 644.0, 184.0, title);
    device_group = device_group
        .child(field_label(14.0, 26.0, 82.0, "设备："))
        .child(combo(
            100.0,
            26.0,
            340.0,
            "",
            &tr(device_placeholder),
            false,
        ))
        .child(field_label(14.0, 52.0, 82.0, "格式："))
        .child(combo(
            100.0,
            52.0,
            340.0,
            &tr("自动选择(推荐)"),
            "",
            false,
        ))
        .child(field_label(14.0, 78.0, 82.0, "分解器："))
        .child(combo(
            100.0,
            78.0,
            340.0,
            &tr("MS MPEG2 分离器(推荐)"),
            "",
            false,
        ))
        .child(field_label(4.0, 104.0, 96.0, "用户自定义 CLSID："))
        .child(combo(100.0, 104.0, 340.0, "", "", false))
        .child(check(
            100.0,
            132.0,
            320.0,
            "使用 Microsoft 默认调谐器",
            false,
            false,
        ))
        .child(check(
            100.0,
            152.0,
            320.0,
            "使用新的连接方式(推荐)",
            true,
            false,
        ));

    let mut time_group = group(8.0, 200.0, 644.0, 90.0, "时间定位");
    time_group = time_group
        .child(check(
            14.0,
            26.0,
            300.0,
            "使用时间跳移功能",
            false,
            false,
        ))
        .child(text_box(
            26.0,
            52.0,
            40.0,
            20.0,
            11.0,
            CONTROL_TEXT,
            false,
            &tr("时间："),
        ))
        .child(
            div()
                .absolute()
                .left(px(68.0))
                .top(px(61.0))
                .w(px(330.0))
                .h(px(4.0))
                .bg(rgb(0xd6d6d6)),
        )
        .child(
            div()
                .absolute()
                .left(px(68.0))
                .top(px(56.0))
                .w(px(6.0))
                .h(px(14.0))
                .bg(rgb(OPTION_HOVER)),
        )
        .child(spin(410.0, 52.0, 60.0, 1))
        .child(text_box(
            474.0,
            52.0,
            20.0,
            22.0,
            11.0,
            CONTROL_TEXT,
            false,
            &tr("分"),
        ));

    let mut other_group = group(8.0, 298.0, 644.0, 54.0, "其他");
    other_group = other_group.child(check(
        14.0,
        28.0,
        300.0,
        "暂不支持该 EPG。",
        false,
        false,
    ));

    vec![
        device_group.into_any_element(),
        time_group.into_any_element(),
        other_group.into_any_element(),
    ]
}

fn disc_tab() -> Vec<AnyElement> {
    let mut disc_group = group(8.0, 8.0, 644.0, 100.0, "光盘设备");
    disc_group = disc_group
        .child(field_label(14.0, 30.0, 82.0, "设备："))
        .child(combo(
            100.0,
            30.0,
            340.0,
            "",
            &tr("未找到光驱"),
            false,
        ))
        .child(field_label(14.0, 58.0, 82.0, "区域："))
        .child(combo(100.0, 58.0, 160.0, &tr("未设置"), "", false));

    let mut playback_group = group(8.0, 116.0, 644.0, 100.0, "播放设置");
    playback_group = playback_group
        .child(check(14.0, 32.0, 320.0, "使用系统解码器", false, false))
        .child(check(14.0, 58.0, 320.0, "打开后自动播放", true, false));

    vec![
        disc_group.into_any_element(),
        playback_group.into_any_element(),
    ]
}

fn dropdown_popup(
    left: f32,
    top: f32,
    width: f32,
    options: &[String],
    settings: &Entity<SettingsController>,
    kind: DropdownKind,
) -> AnyElement {
    let full_height = options.len() as f32 * 20.0 + 2.0;
    let mut inner = div().relative().w_full().h(px(full_height));
    for (index, option) in options.iter().enumerate() {
        inner = inner.child(
            div()
                .id(("settings-option", index))
                .absolute()
                .left(px(1.0))
                .top(px(index as f32 * 20.0 + 1.0))
                .w(px(width - 2.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .px(px(6.0))
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .hover(|style| {
                    style
                        .bg(rgb(OPTION_HOVER))
                        .text_color(rgb(0xffffff))
                })
                .on_click({
                    let settings = settings.clone();
                    move |_, _, cx: &mut App| {
                        settings.update(cx, |settings, cx| match kind {
                            DropdownKind::Device => settings.select_device(index, cx),
                            DropdownKind::Format => settings.select_mode(index, cx),
                            DropdownKind::Screen => settings.select_screen(index, cx),
                            DropdownKind::Output => settings.select_output(index, cx),
                            DropdownKind::Smoothing | DropdownKind::Audio => {}
                        });
                    }
                })
                .child(option.clone()),
        );
    }
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(full_height.min(360.0)))
        .id("settings-dropdown-popup")
        .occlude()
        .overflow_y_scroll()
        .bg(rgb(CONTROL_BG))
        .border_1()
        .border_color(rgb(BUTTON_BORDER))
        .child(inner)
        .into_any_element()
}

fn audio_popup(
    settings: &Entity<SettingsController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    const WIDTH: f32 = 340.0;
    let (names, current) = {
        let controller = settings.read(cx);
        (controller.audio_names(), controller.audio_index())
    };
    let full_height = names.len() as f32 * 20.0 + 2.0;
    let mut inner = div().relative().w_full().h(px(full_height));
    for (index, name) in names.iter().enumerate() {
        let selected = index == current;
        inner = inner.child(
            div()
                .id(("settings-audio-option", index))
                .absolute()
                .left(px(1.0))
                .top(px(index as f32 * 20.0 + 1.0))
                .w(px(WIDTH - 2.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .px(px(6.0))
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .bg(rgb(if selected { 0xdce9f7 } else { CONTROL_BG }))
                .hover(|style| {
                    style
                        .bg(rgb(OPTION_HOVER))
                        .text_color(rgb(0xffffff))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let source =
                        this.settings
                            .update(cx, |settings, cx| settings.select_audio(index, cx));
                    if let Some(source) = source {
                        this.apply_capture_audio(source, cx);
                    }
                }))
                .child(name.clone()),
        );
    }
    div()
        .absolute()
        .left(px(108.0))
        .top(px(170.0))
        .w(px(WIDTH))
        .h(px(full_height.min(360.0)))
        .id("settings-audio-popup")
        .occlude()
        .overflow_y_scroll()
        .bg(rgb(CONTROL_BG))
        .border_1()
        .border_color(rgb(BUTTON_BORDER))
        .child(inner)
        .into_any_element()
}

fn smoothing_popup(
    settings: &Entity<SettingsController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    const WIDTH: f32 = 300.0;
    let (names, current) = {
        let controller = settings.read(cx);
        (controller.smoothing_names(), controller.smoothing_index())
    };
    let full_height = names.len() as f32 * 20.0 + 2.0;
    let mut inner = div().relative().w_full().h(px(full_height));
    for (index, name) in names.iter().enumerate() {
        let selected = index == current;
        inner = inner.child(
            div()
                .id(("settings-smoothing-option", index))
                .absolute()
                .left(px(1.0))
                .top(px(index as f32 * 20.0 + 1.0))
                .w(px(WIDTH - 2.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .px(px(6.0))
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .bg(rgb(if selected { 0xdce9f7 } else { CONTROL_BG }))
                .hover(|style| {
                    style
                        .bg(rgb(OPTION_HOVER))
                        .text_color(rgb(0xffffff))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let key = this.settings.update(cx, |settings, cx| {
                        settings.select_smoothing(index, cx)
                    });
                    if let Some(key) = key {
                        this.apply_capture_smoothing(key, cx);
                    }
                }))
                .child(name.clone()),
        );
    }
    div()
        .absolute()
        .left(px(356.0))
        .top(px(199.0))
        .w(px(WIDTH))
        .id("settings-smoothing-popup")
        .occlude()
        .bg(rgb(CONTROL_BG))
        .border_1()
        .border_color(rgb(BUTTON_BORDER))
        .child(inner)
        .into_any_element()
}

fn format_fps(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.01 {
        format!("{:.0}", fps)
    } else {
        format!("{fps:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn group(left: f32, top: f32, width: f32, height: f32, title: &str) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .border_1()
        .border_color(rgb(GROUP_BORDER))
        .child(
            div()
                .absolute()
                .left(px(8.0))
                .top(px(4.0))
                .w(px(width - 16.0))
                .h(px(18.0))
                .flex()
                .items_center()
                .text_size(px(11.0))
                .text_color(rgb(GROUP_TITLE))
                .child(tr(title)),
        )
}

fn field_label(left: f32, top: f32, width: f32, text: &str) -> Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(22.0))
        .flex()
        .items_center()
        .justify_end()
        .text_size(px(11.0))
        .text_color(rgb(CONTROL_TEXT))
        .child(tr(text))
}

fn text_box(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    size: f32,
    color: u32,
    center: bool,
    text: &str,
) -> Div {
    let mut root = div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .flex()
        .items_center()
        .text_size(px(size))
        .text_color(rgb(color));
    if center {
        root = root.justify_center();
    }
    root.child(text.to_string())
}

fn combo(
    left: f32,
    top: f32,
    width: f32,
    value: &str,
    placeholder: &str,
    enabled: bool,
) -> Div {
    let empty = value.is_empty();
    let display = if empty { placeholder } else { value };
    let text_color = if !enabled {
        CONTROL_TEXT_DISABLED
    } else if empty {
        CONTROL_TEXT_PLACEHOLDER
    } else {
        CONTROL_TEXT
    };
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(22.0))
        .bg(rgb(if enabled {
            CONTROL_BG
        } else {
            CONTROL_BG_DISABLED
        }))
        .border_1()
        .border_color(rgb(CONTROL_BORDER))
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .top_0()
                .w(px(width - 24.0))
                .h_full()
                .flex()
                .items_center()
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(text_color))
                .child(display.to_string()),
        )
        .child(
            div()
                .absolute()
                .left(px(width - 17.0))
                .top_0()
                .w(px(11.0))
                .h_full()
                .flex()
                .items_center()
                .text_size(px(10.0))
                .text_color(rgb(if enabled {
                    CONTROL_ARROW
                } else {
                    CONTROL_ARROW_DIM
                }))
                .child("▾"),
        )
}

fn dropdown_box(
    left: f32,
    top: f32,
    width: f32,
    value: &str,
    placeholder: bool,
    enabled: bool,
) -> Div {
    let text_color = if !enabled {
        CONTROL_TEXT_DISABLED
    } else if placeholder {
        CONTROL_TEXT_PLACEHOLDER
    } else {
        CONTROL_TEXT
    };
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(22.0))
        .bg(rgb(if enabled {
            CONTROL_BG
        } else {
            CONTROL_BG_DISABLED
        }))
        .border_1()
        .border_color(rgb(CONTROL_BORDER))
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .top_0()
                .w(px(width - 24.0))
                .h_full()
                .flex()
                .items_center()
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(text_color))
                .child(value.to_string()),
        )
        .child(
            div()
                .absolute()
                .left(px(width - 17.0))
                .top_0()
                .w(px(11.0))
                .h_full()
                .flex()
                .items_center()
                .text_size(px(10.0))
                .text_color(rgb(if enabled {
                    CONTROL_ARROW
                } else {
                    CONTROL_ARROW_DIM
                }))
                .child("▾"),
        )
}

fn capture_spin(
    cx: &mut Context<PlayerFrameController>,
    id: &'static str,
    left: f32,
    value: u32,
) -> Stateful<Div> {
    let label = if value > 0 {
        value.to_string()
    } else {
        tr("全屏")
    };
    spin(left, 58.0, 66.0, label)
        .id(id)
        .on_click(cx.listener(|this, _, _, cx| {
            this.settings.update(cx, |settings, cx| settings.close(cx));
            this.handle_action("capture-size1".to_string(), cx);
        }))
}

fn spin(left: f32, top: f32, width: f32, value: impl std::fmt::Display) -> Div {
    let half = 22.0 / 2.0;
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(22.0))
        .bg(rgb(CONTROL_BG))
        .border_1()
        .border_color(rgb(CONTROL_BORDER))
        .child(
            div()
                .absolute()
                .left(px(6.0))
                .top_0()
                .w(px(width - 22.0))
                .h_full()
                .flex()
                .items_center()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .child(value.to_string()),

        )
        .child(spin_arrow(width, 1.0, half - 1.0, "▴"))
        .child(spin_arrow(width, half, half - 1.0, "▾"))
}

fn spin_arrow(width: f32, y: f32, height: f32, glyph: &str) -> Div {
    div()
        .absolute()
        .left(px(width - 14.0))
        .top(px(y))
        .w(px(13.0))
        .h(px(height))
        .bg(rgb(0xf2f2f2))
        .border_1()
        .border_color(rgb(0xcfcfcf))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(7.0))
        .text_color(rgb(CONTROL_ARROW))
        .child(glyph.to_string())
}

fn check(left: f32, top: f32, width: f32, label: &str, checked: bool, enabled: bool) -> Div {
    let mut root = div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(18.0))
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(2.0))
                .w(px(13.0))
                .h(px(13.0))
                .bg(rgb(if enabled {
                    CONTROL_BG
                } else {
                    CONTROL_BG_DISABLED
                }))
                .border_1()
                .border_color(rgb(if enabled {
                    CHECK_BORDER
                } else {
                    CHECK_BORDER_DISABLED
                })),
        );
    if checked {
        root = root.child(
            div()
                .absolute()
                .left(px(1.0))
                .top(px(2.0))
                .w(px(11.0))
                .h(px(13.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.0))
                .text_color(rgb(if enabled { CHECK_MARK } else {
                    CONTROL_TEXT_DISABLED
                }))
                .child("✓"),
        );
    }
    root.child(
        div()
            .absolute()
            .left(px(18.0))
            .top_0()
            .w(px(width - 18.0))
            .h_full()
            .flex()
            .items_center()
            .truncate()
            .text_size(px(11.0))
            .text_color(rgb(if enabled {
                CONTROL_TEXT
            } else {
                CONTROL_TEXT_DISABLED
            }))
            .child(tr(label)),
    )
}

fn dlg_button(id: &'static str, left: f32, top: f32, width: f32, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(23.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .bg(rgb(BUTTON_BG))
        .border_1()
        .border_color(rgb(BUTTON_BORDER))
        .text_size(px(11.0))
        .text_color(rgb(CONTROL_TEXT))
        .hover(|style| style.bg(rgb(BUTTON_BG_HOVER)))
        .child(tr(label))
}
