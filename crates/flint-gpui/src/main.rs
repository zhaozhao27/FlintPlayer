mod features;
mod i18n;
mod icons;
mod menu;
mod theme;
mod x11;

use std::path::PathBuf;

use features::playback::RawInput;
use features::player_frame::controller::{
    PLAYER_KEY_CONTEXT, ToggleOsd, ToggleOsdBrief,
};
use features::player_frame::PlayerFrameController;
use gpui_kit::*;

struct LaunchArgs {
    path: Option<PathBuf>,
    raw: Option<RawInput>,
    analysis: bool,
}

fn parse_args() -> LaunchArgs {
    let mut path = None;
    let mut raw = RawInput::default();
    let mut has_raw = false;
    let mut analysis = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy().into_owned();
        let mut value = || args.next().and_then(|value| value.into_string().ok());
        match arg.as_str() {
            "--size" | "-s" => {
                if let Some((width, height)) = value().as_deref().and_then(parse_size) {
                    raw.width = width;
                    raw.height = height;
                    has_raw = true;
                }
            }
            "--fps" => {
                if let Some(fps) = value().as_deref().and_then(|value| value.parse().ok()) {
                    raw.fps = Some(fps);
                    has_raw = true;
                }
            }
            "--raw" => {
                if let Some(format) = value() {
                    raw.demuxer = format;
                    has_raw = true;
                }
            }
            "--pix-fmt" => {
                if let Some(format) = value() {
                    raw.pixel_format = format;
                    has_raw = true;
                }
            }
            "--format" | "-f" => {
                value();
            }
            "--frame-analysis" | "--analysis" => analysis = true,
            "--version" | "-V" => {
                println!("flintplayer {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => usage(),
            flag if flag.starts_with('-') => usage(),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => {}
        }
    }
    LaunchArgs {
        path,
        raw: has_raw.then_some(raw),
        analysis,
    }
}

fn parse_size(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once(['x', 'X'])?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn usage() -> ! {
    eprintln!("用法: flintplayer [--version] [--size WxH] [--fps N] [--raw 输入格式] [--pix-fmt 像素格式] [--frame-analysis] <设备或文件>");
    std::process::exit(2);
}

fn main() {
    env_logger::init();
    let launch = parse_args();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("tab", ToggleOsd, Some(PLAYER_KEY_CONTEXT)),
            KeyBinding::new("shift-tab", ToggleOsdBrief, Some(PLAYER_KEY_CONTEXT)),
        ]);
        let options = WindowOptions {
            window_decorations: Some(WindowDecorations::Client),
            titlebar: Some(TitlebarOptions {
                title: Some("FlintPlayer".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            ..WindowOptions::default()
        };
        let window = gpui_kit::open_window(options, cx, move |_, cx| {
            let launch = &launch;
            cx.new(|cx| {
                let mut controller = PlayerFrameController::new(cx);
                controller.configure_source(launch.raw.clone(), launch.analysis, cx);
                if let Some(path) = &launch.path {
                    controller.queue_open(path.clone());
                }
                controller
            })
        });
        if let Err(error) = window {
            log::error!("无法创建窗口：{error}");
            cx.quit();
        }
    });
}
