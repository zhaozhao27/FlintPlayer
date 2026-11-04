use gpui::*;

use crate::i18n::{tr, trf};
use crate::theme;

const LABEL_COLOR: u32 = 0xffffff;
const ROW_HEIGHT_RATIO: f32 = 1.4;
const LABEL_GAP_RATIO: f32 = 0.4;
const SECTION_GAP_RATIO: f32 = 0.2;
const OSD_INSET: f32 = 18.0;

pub(super) const GREEN: u32 = 0;
pub(super) const RED: u32 = 1;
pub(super) const MAGENTA: u32 = 2;
pub(super) const CYAN: u32 = 3;
pub(super) const WHITE: u32 = 5;

pub(super) enum FullData {
    Capture(CaptureData),
    Playback(PlaybackData),
}

pub(super) struct CaptureData {
    pub label: String,
    pub clock: String,
    pub timeline: String,
    pub frames: u64,
    pub cpu: f64,
    pub codec: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub nominal_fps: f64,
    pub bitrate_mbps: f64,
    pub decoded: String,
    pub output_width: u32,
    pub output_height: u32,
    pub actual_fps: f64,
    pub dropped: u64,
    pub drop_percent: f64,
    pub decode_ms: f32,
    pub audio_codec: Option<String>,
}

pub(super) struct PlaybackData {
    pub title: String,
    pub clock: String,
    pub position: String,
    pub frames: i64,
    pub cpu: f64,
    pub codec: String,
    pub container: String,
    pub fps: f64,
    pub bitrate: f64,
    pub width: u32,
    pub height: u32,
    pub audio_codec: Option<String>,
}

enum OsdRow {
    Field(&'static str, Vec<(String, u32)>),
    Gap,
}

pub(super) fn full_osd(width: f32, text_size: f32, data: &FullData) -> AnyElement {
    let mut column = div()
        .absolute()
        .left(px(OSD_INSET))
        .top(px(theme::TITLEBAR_HEIGHT + OSD_INSET))
        .w(px(width))
        .flex()
        .flex_col()
        .overflow_hidden();

    for row in rows(data) {
        column = match row {
            OsdRow::Field(label, tokens) => match field_row(label, &tokens, text_size) {
                Some(element) => column.child(element),
                None => column,
            },
            OsdRow::Gap => column.child(div().w_full().h(px(text_size * SECTION_GAP_RATIO))),
        };
    }

    column.into_any()
}

pub(super) fn brief_osd(width: f32, text: &str) -> AnyElement {
    div()
        .absolute()
        .left(px(OSD_INSET))
        .top(px(theme::TITLEBAR_HEIGHT + OSD_INSET))
        .w(px(width))
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(3.0))
        .bg(rgba(0x00000099))
        .text_size(px(14.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0xe8e8e8))
        .whitespace_nowrap()
        .text_ellipsis()
        .overflow_hidden()
        .child(text.to_string())
        .into_any()
}

pub(super) fn playback_osd(width: f32, text: &str) -> AnyElement {
    div()
        .absolute()
        .left(px(OSD_INSET))
        .top(px(theme::TITLEBAR_HEIGHT + OSD_INSET))
        .w(px(width.min(460.0)))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(3.0))
        .bg(rgba(0x000000b3))
        .text_size(px(13.0))
        .text_color(rgb(0xe8e8e8))
        .whitespace_normal()
        .child(text.to_string())
        .into_any()
}

pub(super) fn analysis_overlay(width: f32, text: &str) -> AnyElement {
    div()
        .absolute()
        .right(px(OSD_INSET))
        .top(px(theme::TITLEBAR_HEIGHT + OSD_INSET))
        .w(px(width.min(560.0)))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(3.0))
        .bg(rgba(0x000000b3))
        .border_1()
        .border_color(rgba(0xead300aa))
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0xffd400))
        .whitespace_nowrap()
        .text_ellipsis()
        .overflow_hidden()
        .child(text.to_string())
        .into_any()
}

pub(super) fn format_timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let total = seconds as u64;
    let millis = ((seconds - total as f64) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total / 3600,
        total / 60 % 60,
        total % 60,
        millis.min(999)
    )
}

pub(super) fn format_clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00:00".to_string();
    }
    let total = seconds as u64;
    format!("{:02}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

fn rows(data: &FullData) -> Vec<OsdRow> {
    match data {
        FullData::Capture(data) => capture_rows(data),
        FullData::Playback(data) => playback_rows(data),
    }
}

fn capture_rows(data: &CaptureData) -> Vec<OsdRow> {
    let width = data.width;
    let height = data.height;
    let output_width = data.output_width;
    let output_height = data.output_height;
    let decoded = data.decoded.clone();
    vec![
        OsdRow::Field("文件名:", vec![(data.label.clone(), GREEN)]),
        OsdRow::Field(
            "当前时间:",
            vec![
                (data.clock.clone(), WHITE),
                (tr(", 时间轴: "), WHITE),
                (data.timeline.clone(), GREEN),
                (tr(", #帧数: "), WHITE),
                (data.frames.to_string(), GREEN),
            ],
        ),
        OsdRow::Field(
            "当前配置:",
            vec![
                (tr("*默认配置"), RED),
                (", CPU: ".to_string(), WHITE),
                (format!("{:.1}%", data.cpu), GREEN),
            ],
        ),
        OsdRow::Field("版本:", version_tokens()),
        OsdRow::Gap,
        OsdRow::Field("视频解码器:", decoder_tokens(&data.codec, &decoded)),
        OsdRow::Field(
            "输入:",
            vec![
                (
                    trf(
                        "{format} ({bits} 位), {w}×{h}",
                        &[
                            &data.pixel_format,
                            &bit_depth(&data.pixel_format).to_string(),
                            &width.to_string(),
                            &height.to_string(),
                        ],
                    ),
                    GREEN,
                ),
                (format!("({})", ratio(width, height)), MAGENTA),
                (tr(", 帧率: "), WHITE),
                (format!("{:.2}", data.nominal_fps), GREEN),
                (tr(", 位率: "), WHITE),
                (format!("{:.0}kbps", data.bitrate_mbps * 1000.0), GREEN),
            ],
        ),
        OsdRow::Field(
            "格式:",
            vec![
                (format!("{}p", height), GREEN),
                (tr(", 像素: "), WHITE),
                (decoded.clone(), MAGENTA),
            ],
        ),
        OsdRow::Field(
            "输出:",
            vec![
                ("RGBA8888".to_string(), MAGENTA),
                (
                    trf(
                        " (32 位), {w}×{h}",
                        &[&output_width.to_string(), &output_height.to_string()],
                    ),
                    MAGENTA,
                ),
                (
                    format!(" ({})", ratio(output_width, output_height)),
                    MAGENTA,
                ),
                (tr(", 帧率: "), WHITE),
                (
                    format!("{:.2} ({:.2})", data.actual_fps, data.nominal_fps),
                    GREEN,
                ),
            ],
        ),
        OsdRow::Field(
            "视频渲染器:",
            vec![("GPUI 视频渲染器 (OpenGL/Vulkan)".to_string(), RED)],
        ),
        OsdRow::Field(
            "- 像素格式:",
            vec![
                (decoded, MAGENTA),
                (" (Input)->RGBA (Display)".to_string(), RED),
            ],
        ),
        OsdRow::Field(
            "- 帧:",
            vec![
                (data.frames.to_string(), GREEN),
                (tr(", 丢帧: "), WHITE),
                (
                    data.dropped.to_string(),
                    if data.dropped > 0 { RED } else { GREEN },
                ),
                (
                    trf(
                        " ({percent}%), 解码: ",
                        &[&format!("{:.1}", data.drop_percent)],
                    ),
                    WHITE,
                ),
                (format!("{:.2}ms", data.decode_ms), GREEN),
                (tr(", 码率: "), WHITE),
                (format!("{:.2}Mbps", data.bitrate_mbps), GREEN),
            ],
        ),
        OsdRow::Field(
            "视频帧大小:",
            vec![
                (format!("{width}×{height}"), MAGENTA),
                (format!("({})", ratio(width, height)), MAGENTA),
                (" - ".to_string(), WHITE),
                (format!("{output_width}×{output_height}"), MAGENTA),
                (
                    format!("({})", ratio(output_width, output_height)),
                    MAGENTA,
                ),
            ],
        ),
        OsdRow::Gap,
        OsdRow::Field(
            "音频渲染器:",
            match &data.audio_codec {
                Some(codec) => vec![(codec.clone(), MAGENTA)],
                None => vec![(tr("无音频输入"), WHITE)],
            },
        ),
    ]
}

fn playback_rows(data: &PlaybackData) -> Vec<OsdRow> {
    let width = data.width;
    let height = data.height;
    let mut time = vec![
        (data.clock.clone(), WHITE),
        (tr(", 时间轴: "), WHITE),
        (data.position.clone(), GREEN),
    ];
    if data.frames > 0 {
        time.push((tr(", #帧数: "), WHITE));
        time.push((data.frames.to_string(), GREEN));
    }
    vec![
        OsdRow::Field("文件名:", vec![(data.title.clone(), GREEN)]),
        OsdRow::Field("当前时间:", time),
        OsdRow::Field(
            "当前配置:",
            vec![
                (tr("*默认配置"), RED),
                (", CPU: ".to_string(), WHITE),
                (format!("{:.1}%", data.cpu), GREEN),
            ],
        ),
        OsdRow::Field("版本:", version_tokens()),
        OsdRow::Gap,
        OsdRow::Field(
            "视频解码器:",
            if data.codec.is_empty() {
                Vec::new()
            } else {
                vec![
                    (data.codec.clone(), MAGENTA),
                    (", ".to_string(), WHITE),
                    ("libmpv".to_string(), WHITE),
                ]
            },
        ),
        OsdRow::Field("输入:", playback_input_tokens(data)),
        OsdRow::Field(
            "格式:",
            if height == 0 {
                Vec::new()
            } else {
                vec![
                    (format!("{height}p"), GREEN),
                    (tr(", 像素: "), WHITE),
                    (data.codec.clone(), MAGENTA),
                ]
            },
        ),
        OsdRow::Field(
            "输出:",
            if width == 0 || height == 0 {
                Vec::new()
            } else {
                let mut tokens = vec![
                    ("RGBA8888".to_string(), MAGENTA),
                    (
                        trf(" (32 位), {w}×{h}", &[&width.to_string(), &height.to_string()]),
                        MAGENTA,
                    ),
                    (format!(" ({})", ratio(width, height)), MAGENTA),
                ];
                if data.fps > 0.0 {
                    tokens.push((tr(", 帧率: "), WHITE));
                    tokens.push((format!("{:.2}", data.fps), GREEN));
                }
                tokens
            },
        ),
        OsdRow::Field(
            "视频渲染器:",
            vec![
                ("libmpv".to_string(), GREEN),
                (" (OpenGL)".to_string(), WHITE),
            ],
        ),
        OsdRow::Field(
            "- 像素格式:",
            if data.codec.is_empty() {
                Vec::new()
            } else {
                vec![
                    (data.codec.clone(), MAGENTA),
                    (" (Input)->RGBA (Display)".to_string(), RED),
                ]
            },
        ),
        OsdRow::Field("- 帧:", Vec::new()),
        OsdRow::Field(
            "视频帧大小:",
            if width == 0 || height == 0 {
                Vec::new()
            } else {
                vec![
                    (format!("{width}×{height}"), MAGENTA),
                    (format!("({})", ratio(width, height)), MAGENTA),
                    (" - ".to_string(), WHITE),
                    (format!("{width}×{height}"), MAGENTA),
                    (format!("({})", ratio(width, height)), MAGENTA),
                ]
            },
        ),
        OsdRow::Gap,
        OsdRow::Field(
            "音频渲染器:",
            match &data.audio_codec {
                Some(codec) => vec![(codec.clone(), MAGENTA)],
                None => vec![(tr("无音频输入"), WHITE)],
            },
        ),
    ]
}

fn version_tokens() -> Vec<(String, u32)> {
    vec![
        (env!("CARGO_PKG_VERSION").to_string(), WHITE),
        (", ".to_string(), WHITE),
        (flint_core::sysinfo::os_description(), CYAN),
    ]
}

fn decoder_tokens(codec: &str, decoded: &str) -> Vec<(String, u32)> {
    let name = match codec {
        "MJPEG" => Some("mjpeg"),
        "H264" => Some("h264"),
        "HEVC" | "H265" => Some("hevc"),
        "MPEG4" => Some("mpeg4"),
        _ => None,
    };
    match name {
        Some(name) => vec![
            (tr("内置 FFmpeg 解码器"), RED),
            (trf(" ({name}, 1 线程)", &[&name]), MAGENTA),
        ],
        None => vec![
            (tr("内置 FFmpeg 像素转换"), RED),
            (format!(" ({decoded})"), MAGENTA),
        ],
    }
}

fn bit_depth(fourcc: &str) -> u32 {
    match fourcc {
        "NV12" => 12,
        "YUYV" | "YUY2" | "UYVY" => 16,
        "GREY" => 8,
        _ => 24,
    }
}

fn playback_input_tokens(data: &PlaybackData) -> Vec<(String, u32)> {
    let mut tokens = Vec::new();
    if !data.container.is_empty() {
        tokens.push((data.container.clone(), GREEN));
    }
    if data.fps > 0.0 {
        tokens.push((tr(", 帧率: "), WHITE));
        tokens.push((format!("{:.2}", data.fps), GREEN));
    }
    if data.bitrate > 0.0 {
        tokens.push((tr(", 位率: "), WHITE));
        tokens.push((format!("{:.2}Mbps", data.bitrate / 1_000_000.0), GREEN));
    }
    tokens
}

fn field_row(label: &str, tokens: &[(String, u32)], size: f32) -> Option<AnyElement> {
    if tokens.is_empty() {
        return None;
    }
    let mut row = div()
        .h(px(size * ROW_HEIGHT_RATIO))
        .w_full()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .overflow_hidden()
        .child(text(&tr(label), LABEL_COLOR, size))
        .child(div().w(px(size * LABEL_GAP_RATIO)).flex_shrink_0());
    for (value, tone) in tokens {
        row = row.child(text(value, tone_color(*tone), size));
    }
    Some(row.into_any())
}

fn text(value: &str, color: u32, size: f32) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(size))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(color))
        .child(value.to_string())
}

fn tone_color(tone: u32) -> u32 {
    match tone {
        1 => 0xff3b30,
        2 => 0xff00ff,
        3 => 0x00d0ff,
        4 => 0xffd400,
        5 => 0xffffff,
        _ => 0x00ff00,
    }
}

fn ratio(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return "0:0".to_string();
    }
    let divisor = gcd(width, height);
    format!("{}:{}", width / divisor, height / divisor)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}
