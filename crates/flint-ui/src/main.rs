mod app;
mod keymap;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let initial_source = std::env::args().nth(1).filter(|arg| !arg.trim().is_empty());
    app::run(initial_source)
}
