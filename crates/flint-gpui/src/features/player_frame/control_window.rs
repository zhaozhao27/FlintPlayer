use std::cell::Cell;
use std::rc::Rc;

use gpui::*;

use super::controller::PlayerFrameController;
use crate::i18n::tr;
use crate::theme;

const PANEL_WIDTH: f32 = 560.0;
const PANEL_HEIGHT: f32 = 320.0;
const TITLE_HEIGHT: f32 = 26.0;
const TAB_STRIP_TOP: f32 = 26.0;
const TAB_STRIP_HEIGHT: f32 = 26.0;
const CONTENT_TOP: f32 = TAB_STRIP_TOP + TAB_STRIP_HEIGHT;
const CONTENT_HEIGHT: f32 = PANEL_HEIGHT - CONTENT_TOP;

const PANEL_BG: u32 = 0xf0f0f0;
const PANEL_BORDER: u32 = 0x6a6a6a;
const TITLE_BG: u32 = 0x1f1f1f;
const TITLE_TEXT: u32 = 0xeeeeee;
const TITLE_CLOSE: u32 = 0xd0d0d0;
const GROUP_BORDER: u32 = 0xb8b8b8;
const GROUP_TITLE: u32 = 0x1c1c1c;
const CONTROL_TEXT: u32 = 0x1a1a1a;
const CONTROL_TEXT_DIM: u32 = 0x707070;
const CONTROL_BORDER: u32 = 0x8f8f8f;
const BUTTON_BG: u32 = 0xf4f4f4;
const BUTTON_BG_HOVER: u32 = 0xe6e6e6;
const BUTTON_BORDER: u32 = 0x8f8f8f;
const TAB_BG: u32 = 0xe3e3e3;
const TAB_BG_ACTIVE: u32 = 0xf5f5f5;
const TAB_TEXT: u32 = 0x3a3a3a;
const TAB_TEXT_ACTIVE: u32 = 0x111111;
const TAB_BORDER: u32 = 0xa0a0a0;
const TRACK_BG: u32 = 0xc8c8c8;
const TRACK_FILL: u32 = 0x2a7fd4;
const TRACK_THUMB: u32 = 0xffffff;
const TRACK_DISABLED: u32 = 0xdadada;
const CHECK_BORDER: u32 = 0x7a7a7a;
const CHECK_MARK: u32 = 0x1a5fb4;
const EQ_LABEL: u32 = 0x505050;

pub fn control_window(
    frame: &PlayerFrameController,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let tab = frame.control_tab();
    let content = match tab {
        1 => video_tab(frame, cx),
        2 => subtitle_tab(frame, cx),
        3 => playback_tab(frame, cx),
        _ => sound_tab(frame, cx),
    };

    let mut title_bar = div()
        .absolute()
        .left_0()
        .top_0()
        .w(px(PANEL_WIDTH))
        .h(px(TITLE_HEIGHT))
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
                .child(tr("控制窗口")),
        );
    title_bar = title_bar.child(
        div()
            .id("control-window-close")
            .absolute()
            .left(px(PANEL_WIDTH - 36.0))
            .top_0()
            .w(px(36.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(rgb(TITLE_CLOSE))
            .hover(|style| style.bg(rgb(0xc42b1c)))
            .on_click(cx.listener(|this, _, _, cx| this.close_control_window(cx)))
            .child("✕"),
    );

    let mut tab_strip = div()
        .absolute()
        .left_0()
        .top(px(TAB_STRIP_TOP))
        .w(px(PANEL_WIDTH))
        .h(px(TAB_STRIP_HEIGHT))
        .flex();
    for (index, label) in ["声音", "视频", "字幕", "播放"].into_iter().enumerate() {
        let active = tab == index;
        tab_strip = tab_strip.child(
            div()
                .id(("control-tab", index))
                .w(px(88.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(if active { TAB_BG_ACTIVE } else { TAB_BG }))
                .border_1()
                .border_color(rgb(TAB_BORDER))
                .text_size(px(11.0))
                .text_color(rgb(if active { TAB_TEXT_ACTIVE } else { TAB_TEXT }))
                .on_click(cx.listener(move |this, _, _, cx| this.set_control_tab(index, cx)))
                .child(tr(label)),
        );
    }
    tab_strip = tab_strip.child(div().flex_1().h_full().bg(rgb(TAB_BG)));

    let body = div()
        .absolute()
        .left_0()
        .top(px(CONTENT_TOP))
        .w(px(PANEL_WIDTH))
        .h(px(CONTENT_HEIGHT))
        .bg(rgb(PANEL_BG))
        .children(content);

    div()
        .id("control-window")
        .absolute()
        .right(px(8.0))
        .bottom(px(theme::CONTROL_BAR_HEIGHT + 6.0))
        .w(px(PANEL_WIDTH))
        .h(px(PANEL_HEIGHT))
        .bg(rgb(PANEL_BG))
        .border_1()
        .border_color(rgb(PANEL_BORDER))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(title_bar)
        .child(tab_strip)
        .child(body)
        .into_any_element()
}

fn sound_tab(frame: &PlayerFrameController, cx: &mut Context<PlayerFrameController>) -> Vec<AnyElement> {
    let volume = (frame.playback.read(cx).state().volume / 100.0).clamp(0.0, 1.0) as f32;
    let volume_slider = slider(
        cx,
        ("cw-volume", 0),
        70.0,
        30.0,
        380.0,
        volume,
        true,
        |this, fraction, cx| {
            this.playback
                .update(cx, |playback, cx| playback.set_volume(fraction as f64 * 100.0, cx));
        },
    );

    let mut eq_group = group(8.0, 82.0, 544.0, 170.0, "均衡器");
    eq_group = eq_group
        .child(checkbox(
            "cw-eq-enable",
            14.0,
            26.0,
            160.0,
            "启用均衡器",
            frame.eq_enabled(),
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_equalizer(cx))))
        .child(
            button("cw-eq-preset", 196.0, 26.0, 130.0, &tr("预设："))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(format!("{}", frame.eq_preset_label())),
                )
                .on_click(cx.listener(|this, _, _, cx| this.cycle_eq_preset(cx))),
        );

    let bands = frame.eq_bands();
    let enabled = frame.eq_enabled();
    for (index, gain) in bands.iter().enumerate() {
        let column = index / 5;
        let row = index % 5;
        let left = 14.0 + column as f32 * 270.0;
        let top = 54.0 + row as f32 * 22.0;
        eq_group = eq_group
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top + 2.0))
                    .w(px(46.0))
                    .h(px(16.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .text_size(px(10.0))
                    .text_color(rgb(EQ_LABEL))
                    .child(format_band_label(index)),
            )
            .child(slider(
                cx,
                ("cw-eq-band", index),
                left + 50.0,
                top,
                150.0,
                (*gain as f32 + 12.0) / 24.0,
                enabled,
                move |this, fraction, cx| {
                    this.set_eq_band(index, (fraction * 24.0 - 12.0).round() as i16, cx);
                },
            ))
            .child(
                div()
                    .absolute()
                    .left(px(left + 204.0))
                    .top(px(top + 2.0))
                    .w(px(32.0))
                    .h(px(16.0))
                    .flex()
                    .items_center()
                    .text_size(px(10.0))
                    .text_color(rgb(if enabled { CONTROL_TEXT } else { CONTROL_TEXT_DIM }))
                    .child(format!("{gain:+}")),
            );
    }

    vec![
        group(8.0, 8.0, 544.0, 62.0, "音量")
            .child(
                div()
                    .absolute()
                    .left(px(14.0))
                    .top(px(28.0))
                    .w(px(52.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(tr("音量：")),
            )
            .child(volume_slider)
            .child(
                div()
                    .absolute()
                    .left(px(456.0))
                    .top(px(28.0))
                    .w(px(58.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(format!("{:.0}", volume * 100.0)),
            )
            .into_any_element(),
        eq_group.into_any_element(),
    ]
}

fn video_tab(frame: &PlayerFrameController, cx: &mut Context<PlayerFrameController>) -> Vec<AnyElement> {
    const CHANNELS: [&str; 4] = ["亮度：", "对比度：", "饱和度：", "色相："];
    let mut adjust_group = group(8.0, 8.0, 544.0, 128.0, "画面调节");
    for (channel, label) in CHANNELS.iter().enumerate() {
        let top = 26.0 + channel as f32 * 24.0;
        let value = frame.video_value(channel) as f32;
        adjust_group = adjust_group
            .child(
                div()
                    .absolute()
                    .left(px(14.0))
                    .top(px(top))
                    .w(px(62.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(tr(label)),
            )
            .child(slider(
                cx,
                ("cw-video", channel),
                82.0,
                top + 2.0,
                380.0,
                (value + 100.0) / 200.0,
                true,
                move |this, fraction, cx| {
                    this.set_video_value(channel, (fraction * 200.0 - 100.0).round() as i16, cx);
                },
            ))
            .child(
                div()
                    .absolute()
                    .left(px(470.0))
                    .top(px(top))
                    .w(px(48.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(format!("{:+}", value.round() as i16)),
            );
    }

    let renderer = tr(frame.renderer_label());
    vec![
        adjust_group.into_any_element(),
        group(8.0, 146.0, 544.0, 106.0, "画面处理")
            .child(
                button("cw-video-reset", 14.0, 30.0, 90.0, "默认").on_click(
                    cx.listener(|this, _, _, cx| this.reset_video_values(cx)),
                ),
            )
            .child(
                button("cw-video-snapshot", 114.0, 30.0, 90.0, "截取").on_click(
                    cx.listener(|this, _, _, cx| this.take_snapshot(cx)),
                ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(216.0))
                    .top(px(32.0))
                    .w(px(310.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT_DIM))
                    .child(format!("{}: {renderer}", tr("视频渲染器"))),
            )
            .into_any_element(),
    ]
}

fn subtitle_tab(
    frame: &PlayerFrameController,
    cx: &mut Context<PlayerFrameController>,
) -> Vec<AnyElement> {
    const ROWS: [(&str, f32, f32, &str); 4] = [
        ("字体大小：", 8.0, 72.0, ""),
        ("位置：", 0.0, 100.0, "%"),
        ("速度：", 0.5, 3.0, "x"),
        ("延迟(秒)：", -5.0, 5.0, ""),
    ];
    let mut content_group = group(8.0, 8.0, 544.0, 190.0, "字幕样式");
    for (slot, (label, min, max, _)) in ROWS.iter().enumerate() {
        let top = 30.0 + slot as f32 * 30.0;
        let value = frame.subtitle_value(slot);
        let (min, max) = (*min as f64, *max as f64);
        let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
        content_group = content_group
            .child(
                div()
                    .absolute()
                    .left(px(14.0))
                    .top(px(top))
                    .w(px(74.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(tr(label)),
            )
            .child(slider(
                cx,
                ("cw-subtitle", slot),
                96.0,
                top + 2.0,
                340.0,
                fraction,
                true,
                move |this, fraction, cx| {
                    this.set_subtitle_value(slot, min + fraction as f64 * (max - min), cx);
                },
            ))
            .child(
                div()
                    .absolute()
                    .left(px(446.0))
                    .top(px(top))
                    .w(px(60.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.0))
                    .text_color(rgb(CONTROL_TEXT))
                    .child(format_subtitle_value(slot, value)),
            );
    }
    content_group = content_group.child(
        div()
            .absolute()
            .left(px(14.0))
            .top(px(158.0))
            .w(px(500.0))
            .h(px(20.0))
            .flex()
            .items_center()
            .text_size(px(11.0))
            .text_color(rgb(CONTROL_TEXT_DIM))
            .child(tr("调整会立即应用到当前播放的字幕，无字幕时不可见")),
    );
    vec![content_group.into_any_element()]
}

fn playback_tab(frame: &PlayerFrameController, cx: &mut Context<PlayerFrameController>) -> Vec<AnyElement> {
    let (position, duration) = frame.playback_position(cx);
    let fraction = if duration > 0.0 {
        (position / duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let enabled = duration > 0.0;
    let speed = frame.playback.read(cx).speed();

    let position_group = group(8.0, 8.0, 544.0, 66.0, "播放位置")
        .child(
            div()
                .absolute()
                .left(px(14.0))
                .top(px(30.0))
                .w(px(40.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .child(tr("位置：")),
        )
        .child(slider(
            cx,
            ("cw-seek", 0),
            62.0,
            32.0,
            380.0,
            fraction,
            enabled,
            |this, fraction, cx| this.control_seek(fraction as f64, cx),
        ))
        .child(
            div()
                .absolute()
                .left(px(450.0))
                .top(px(30.0))
                .w(px(88.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .child(format_clock(position, duration)),
        );

    let speed_group = group(8.0, 84.0, 544.0, 168.0, "播放速度")
        .child(
            div()
                .absolute()
                .left(px(14.0))
                .top(px(30.0))
                .w(px(74.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .child(tr("速度：")),
        )
        .child(slider(
            cx,
            ("cw-speed", 0),
            96.0,
            32.0,
            340.0,
            ((speed - 0.25) / 3.75).clamp(0.0, 1.0) as f32,
            true,
            |this, fraction, cx| {
                this.control_set_speed(0.25 + fraction as f64 * 3.75, cx);
            },
        ))
        .child(
            div()
                .absolute()
                .left(px(446.0))
                .top(px(30.0))
                .w(px(60.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT))
                .child(format!("{speed:.2}x")),
        )
        .child(
            button("cw-ab-start", 14.0, 76.0, 96.0, "设置 A 点").on_click(
                cx.listener(|this, _, _, cx| this.mark_ab(false, cx)),
            ),
        )
        .child(
            button("cw-ab-end", 120.0, 76.0, 96.0, "设置 B 点")
                .on_click(cx.listener(|this, _, _, cx| this.mark_ab(true, cx))),
        )
        .child(
            button("cw-ab-clear", 226.0, 76.0, 96.0, "取消 A-B").on_click(
                cx.listener(|this, _, _, cx| this.clear_ab(cx)),
            ),
        )
        .child(
            div()
                .absolute()
                .left(px(14.0))
                .top(px(112.0))
                .w(px(500.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .text_size(px(11.0))
                .text_color(rgb(CONTROL_TEXT_DIM))
                .child(tr("依次设置 A 点与 B 点后在区间内循环播放")),
        );

    vec![
        position_group.into_any_element(),
        speed_group.into_any_element(),
    ]
}

fn slider(
    cx: &mut Context<PlayerFrameController>,
    id: (&'static str, usize),
    left: f32,
    top: f32,
    width: f32,
    fraction: f32,
    enabled: bool,
    on_change: impl Fn(&mut PlayerFrameController, f32, &mut Context<PlayerFrameController>)
        + Clone
        + 'static,
) -> Stateful<Div> {
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let down_bounds = bounds.clone();
    let move_bounds = bounds.clone();
    let down_change = on_change.clone();
    let move_change = on_change;
    let fraction = fraction.clamp(0.0, 1.0);
    let thumb_left = (width * fraction - 5.0).clamp(0.0, width - 10.0);

    div()
        .id(id)
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(16.0))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if !enabled {
                    return;
                }
                if let Some(fraction) = pointer_fraction(event.position, down_bounds.get()) {
                    down_change(this, fraction, cx);
                }
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if !enabled || event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            if let Some(fraction) = pointer_fraction(event.position, move_bounds.get()) {
                move_change(this, fraction, cx);
            }
        }))
        .child(canvas(
            move |value: Bounds<Pixels>, _, _| bounds.set(value),
            |_, _, _, _| {},
        )
        .size_full())
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(6.0))
                .w(px(width))
                .h(px(3.0))
                .bg(rgb(if enabled { TRACK_BG } else { TRACK_DISABLED })),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(6.0))
                .w(px(width * fraction))
                .h(px(3.0))
                .bg(rgb(if enabled { TRACK_FILL } else { TRACK_DISABLED })),
        )
        .child(
            div()
                .absolute()
                .left(px(thumb_left))
                .top(px(3.0))
                .w(px(10.0))
                .h(px(10.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(rgb(if enabled { CONTROL_BORDER } else { TRACK_DISABLED }))
                .bg(rgb(TRACK_THUMB)),
        )
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

fn button(id: &'static str, left: f32, top: f32, width: f32, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(BUTTON_BG))
        .border_1()
        .border_color(rgb(BUTTON_BORDER))
        .text_size(px(11.0))
        .text_color(rgb(CONTROL_TEXT))
        .hover(|style| style.bg(rgb(BUTTON_BG_HOVER)))
        .child(tr(label))
}

fn checkbox(
    id: &'static str,
    left: f32,
    top: f32,
    width: f32,
    label: &str,
    checked: bool,
) -> Stateful<Div> {
    let mut row = div()
        .id(id)
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(20.0))
        .flex()
        .items_center()
        .text_size(px(11.0))
        .text_color(rgb(CONTROL_TEXT));
    let mut box_ = div()
        .w(px(13.0))
        .h(px(13.0))
        .mr(px(6.0))
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(CHECK_BORDER))
        .flex()
        .items_center()
        .justify_center();
    if checked {
        box_ = box_.child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(CHECK_MARK))
                .child("✓"),
        );
    }
    row = row.child(box_).child(label.to_string());
    row
}

fn format_band_label(index: usize) -> String {
    match index {
        0 => "31".to_string(),
        1 => "62".to_string(),
        2 => "125".to_string(),
        3 => "250".to_string(),
        4 => "500".to_string(),
        5 => "1k".to_string(),
        6 => "2k".to_string(),
        7 => "4k".to_string(),
        8 => "8k".to_string(),
        _ => "16k".to_string(),
    }
}

fn format_subtitle_value(slot: usize, value: f64) -> String {
    match slot {
        0 => format!("{value:.0}"),
        1 => format!("{value:.0}%"),
        2 => format!("{value:.2}x"),
        _ => format!("{value:+.1}"),
    }
}

fn format_clock(position: f64, duration: f64) -> String {
    let total = position.max(0.0) as u64;
    let end = duration.max(0.0) as u64;
    format!(
        "{:02}:{:02}/{:02}:{:02}",
        total / 60,
        total % 60,
        end / 60,
        end % 60
    )
}

fn pointer_fraction(position: Point<Pixels>, bounds: Bounds<Pixels>) -> Option<f32> {
    let width = f32::from(bounds.size.width);
    if width <= 0.0 {
        return None;
    }
    Some(((f32::from(position.x) - f32::from(bounds.origin.x)) / width).clamp(0.0, 1.0))
}
