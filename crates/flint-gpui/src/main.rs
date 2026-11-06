mod control;
mod extensions;
mod features;
mod i18n;
mod icons;
mod menu;
mod shaders;
mod subtitle_engine;
mod hdr;
mod theme;
mod whisper;
mod x11;

// Rust 标准库以弱符号引用 glibc 2.39 新增的 pidfd_spawnp/pidfd_getpid。
// 这里提供本地实现，让链接器不再记录 GLIBC_2.39 版本需求，
// 从而把二进制的 glibc 下限压到 2.34，兼容 glibc 2.36 及以上的发行版。
use std::ffi::c_char;
use std::os::raw::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pidfd_spawnp(
    _pid: *mut c_int,
    _path: *const c_char,
    _file_actions: *const u8,
    _attrp: *const u8,
    _argv: *const *mut c_char,
    _envp: *const *mut c_char,
    _pidfd: *mut c_int,
) -> c_int {
    unsafe { *libc::__errno_location() = libc::ENOSYS };
    -1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pidfd_getpid(_pidfd: c_int) -> c_int {
    unsafe { *libc::__errno_location() = libc::EINVAL };
    -1
}

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
    config: Option<String>,
}

fn parse_args() -> LaunchArgs {
    parse_args_from(
        std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned()),
    )
}

fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> LaunchArgs {
    let mut path = None;
    let mut raw = RawInput::default();
    let mut has_raw = false;
    let mut analysis = false;
    let mut config = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next();
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
            "--config" => {
                if let Some(name) = value() {
                    config = Some(name);
                }
            }
            flag if flag.starts_with("--config=") => {
                config = Some(flag["--config=".len()..].to_string());
            }
            flag if flag.starts_with("config=") => {
                config = Some(flag["config=".len()..].to_string());
            }
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
        config,
    }
}

fn parse_size(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once(['x', 'X'])?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn usage() -> ! {
    eprintln!("用法: flintplayer [--config=预设名] [--version] [--size WxH] [--fps N] [--raw 输入格式] [--pix-fmt 像素格式] [--frame-analysis] <设备或文件>");
    std::process::exit(2);
}

fn window_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    image::load_from_memory(include_bytes!("../../../assets/icons/flintplayer.png"))
        .ok()
        .map(|icon| std::sync::Arc::new(icon.to_rgba8()))
}

fn main() {
    env_logger::init();
    theme::init();
    menu::init();
    let launch = parse_args();
    for name in flint_core::config::migrate_legacy_profiles() {
        log::info!("已迁移配置预设 {name}");
    }
    if let Some(requested) = launch.config.as_deref() {
        match flint_core::config::set_profile(Some(requested)) {
            Some(active) => log::info!(
                "使用配置预设 {active}：{}",
                flint_core::config::config_path().display()
            ),
            None => log::warn!("配置预设名 {requested:?} 无效，使用默认配置"),
        }
    }
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
            app_id: Some("flintplayer".into()),
            icon: window_icon(),
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

#[cfg(test)]
mod tests {
    use super::{LaunchArgs, parse_args_from};

    fn launch(items: &[&str]) -> LaunchArgs {
        parse_args_from(items.iter().map(|item| item.to_string()))
    }

    #[test]
    fn parses_config_preset_flags() {
        assert_eq!(launch(&["--config=work"]).config.as_deref(), Some("work"));
        assert_eq!(launch(&["--config", "demo"]).config.as_deref(), Some("demo"));
        assert_eq!(launch(&["config=night"]).config.as_deref(), Some("night"));
        assert_eq!(launch(&["--config="]).config.as_deref(), Some(""));
        assert_eq!(launch(&["movie.mkv"]).config, None);
    }

    #[test]
    fn keeps_arguments_next_to_config_preset() {
        let parsed = launch(&["--config=living", "--size", "1280x720", "/dev/video0"]);
        assert_eq!(
            parsed.path.as_deref(),
            Some(std::path::Path::new("/dev/video0"))
        );
        let raw = parsed.raw.expect("raw input");
        assert_eq!((raw.width, raw.height), (1280, 720));
    }
}
