mod app;
mod i18n;
mod keymap;
mod skin;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut source = None;
    let mut start = app::StartOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--size" | "-s" => {
                if let Some((width, height)) = args.next().as_deref().and_then(parse_size) {
                    start.width = Some(width);
                    start.height = Some(height);
                }
            }
            "--fps" => start.fps = args.next().and_then(|value| value.parse().ok()),
            "--format" | "-f" => start.fourcc = args.next(),
            "--raw" => start.input_format = args.next(),
            "--pix-fmt" => start.pixel_format = args.next(),
            "--help" | "-h" => {
                println!(
                    "用法: flintplayer [--size WxH] [--fps N] [--format 像素格式] [--raw 输入格式] <设备或文件>"
                );
                return Ok(());
            }
            other if source.is_none() => source = Some(other.to_string()),
            _ => {}
        }
    }
    app::run(source, start)
}

fn parse_size(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once(['x', 'X'])?;
    Some((width.parse().ok()?, height.parse().ok()?))
}
