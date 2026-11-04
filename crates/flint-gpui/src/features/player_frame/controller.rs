use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flint_core::config::Config;
use flint_media::CaptureSettings;
use flint_media::CaptureSmoothing;
use flint_media::stereo::StereoMode;
use gpui::*;

use crate::features::capture::CaptureController;
use crate::features::playback::{PlaybackController, RawInput};
use crate::features::playlist::PlaylistController;
use crate::features::settings::SettingsController;
use crate::menu::{self, apply_dynamic, MenuData, MenuEntry};

actions!(flint_player, [ToggleOsd, ToggleOsdBrief]);

pub const PLAYER_KEY_CONTEXT: &str = "FlintPlayer";

pub enum FrameAction {
    Exit,
    ToggleFullscreen,
    Action(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OsdMode {
    Full,
    Brief,
    Playback,
}

#[derive(Clone, Copy, PartialEq)]
enum TrackKind {
    Subtitle,
    Audio,
}

#[derive(Clone, Copy)]
pub enum WindowCommand {
    ToggleMaximize,
    Resize(f32, f32),
    Scale(f32),
    Fraction(f32),
}

struct CpuMeter {
    last: Option<(u64, Instant)>,
    percent: f64,
}

impl CpuMeter {
    fn new() -> Self {
        Self {
            last: None,
            percent: 0.0,
        }
    }

    fn sample(&mut self) {
        const INTERVAL: Duration = Duration::from_secs(1);
        let Some(ticks) = flint_core::sysinfo::process_cpu_ticks() else {
            return;
        };
        let now = Instant::now();
        let Some((previous_ticks, previous_at)) = self.last else {
            self.last = Some((ticks, now));
            return;
        };
        let seconds = now.saturating_duration_since(previous_at).as_secs_f64();
        if seconds < INTERVAL.as_secs_f64() {
            return;
        }
        let delta = ticks.saturating_sub(previous_ticks) as f64;
        self.percent = delta / flint_core::sysinfo::ticks_per_second() as f64 / seconds * 100.0;
        self.last = Some((ticks, now));
    }
}

pub struct PlayerFrameController {
    pub(crate) playback: Entity<PlaybackController>,
    pub(crate) capture: Entity<CaptureController>,
    pub(crate) playlist: Entity<PlaylistController>,
    pub(crate) settings: Entity<SettingsController>,
    menu: Vec<MenuEntry>,
    menu_open: bool,
    open_path: Vec<usize>,
    notice: Option<String>,
    focus_handle: FocusHandle,
    repeat: bool,
    shuffle: bool,
    remember_position: bool,
    sidebar_open: bool,
    pub(crate) config: Config,
    resume_pending: Option<f64>,
    eof_handled: bool,
    pub(crate) ticker_started: bool,
    pub(crate) pending_open: Option<std::path::PathBuf>,
    brightness: i16,
    contrast: i16,
    saturation: i16,
    rotation: u16,
    flip_horizontal: bool,
    flip_vertical: bool,
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
    osd_mode: Option<OsdMode>,
    last_snapshot: Option<std::path::PathBuf>,
    pub(crate) pending_window: Option<WindowCommand>,
    shortcuts: Vec<(String, String)>,
    pub(crate) margin: i32,
    pub(crate) margin_auto: bool,
    pub(crate) margin_force: bool,
    auto_rotate: Option<u16>,
    auto_rotate_dims: (u32, u32),
    active_panel: u8,
    last_fingerprint: String,
    cpu: CpuMeter,
}

impl PlayerFrameController {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let config = Config::load();
        let sources: Vec<std::path::PathBuf> = config
            .sources
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        let playlist = cx.new(|_| {
            let mut playlist = PlaylistController::new();
            playlist.set_items(sources);
            playlist
        });
        let brightness = config.brightness;
        let contrast = config.contrast;
        let saturation = config.saturation;
        let rotation = config.rotation;
        let flip_horizontal = config.flip_horizontal;
        let flip_vertical = config.flip_vertical;
        let osd_mode = config.osd_visible.then_some(OsdMode::Full);
        crate::i18n::set_language(crate::i18n::Language::from_code(&config.language));
        let menu = menu::main_menu();
        let shortcuts = menu::collect_shortcuts(&menu);
        let stereo_mode = StereoMode::from_key(&config.stereo_mode);
        let pano_view = config.pano_enabled.then(|| flint_media::panorama::View {
            yaw: config.pano_yaw.to_radians(),
            pitch: config.pano_pitch.to_radians(),
            fov: config.pano_fov.to_radians(),
        });
        let playback = cx.new(|_| PlaybackController::new());
        playback.update(cx, |playback, cx| {
            playback.set_stereo(stereo_mode, cx);
            playback.set_pano(pano_view, cx);
        });
        Self {
            playback,
            capture: cx.new(|_| CaptureController::new()),
            playlist,
            settings: cx.new(|_| SettingsController::new(config.capture_selections.clone())),
            menu,
            menu_open: false,
            open_path: Vec::new(),
            notice: None,
            focus_handle: cx.focus_handle(),
            repeat: false,
            shuffle: false,
            remember_position: config.remember_position,
            sidebar_open: false,
            config,
            resume_pending: None,
            eof_handled: true,
            ticker_started: false,
            pending_open: None,
            brightness,
            contrast,
            saturation,
            rotation,
            flip_horizontal,
            flip_vertical,
            zoom: 0.0,
            pan_x: 0.0,
            pan_y: 0.0,
            osd_mode,
            last_snapshot: None,
            pending_window: None,
            shortcuts,
            margin: 0,
            margin_auto: false,
            margin_force: false,
            auto_rotate: None,
            auto_rotate_dims: (0, 0),
            active_panel: 1,
            last_fingerprint: String::new(),
            cpu: CpuMeter::new(),
        }
    }

    pub fn queue_open(&mut self, path: impl Into<std::path::PathBuf>) {
        self.pending_open = Some(path.into());
    }

    pub(crate) fn open_settings(&mut self, tab: usize, cx: &mut Context<Self>) {
        let (width, height) = (self.config.width, self.config.height);
        let fps = self.config.fps;
        self.settings.update(cx, |settings, cx| {
            settings.set_capture_size(width, height, fps);
            settings.open(tab, cx);
        });
    }

    pub fn configure_source(
        &mut self,
        raw: Option<RawInput>,
        analysis: bool,
        cx: &mut Context<Self>,
    ) {
        self.playback.update(cx, |playback, _| {
            if let Some(raw) = raw {
                playback.set_raw_input(raw);
            }
            playback.set_analysis_mode(analysis);
        });
    }

    pub(crate) fn start_ticker(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let interval = this
                    .read_with(cx, |this, cx| {
                        if this.playback.read(cx).is_loaded() {
                            16
                        } else {
                            100
                        }
                    })
                    .unwrap_or(100);
                cx.background_executor()
                    .timer(Duration::from_millis(interval))
                    .await;
                let updated = this.update(cx, |this, cx| {
                    this.cpu.sample();
                    this.poll_playback(cx);
                    let state = this.playback.read(cx).state();
                    let busy = (state.loaded && !state.paused && !state.eof)
                        || this.capture.read(cx).is_active();
                    let fingerprint = this.view_fingerprint(cx);
                    if busy || fingerprint != this.last_fingerprint {
                        this.last_fingerprint = fingerprint;
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn view_fingerprint(&self, cx: &App) -> String {
        let playback = self.playback.read(cx);
        let state = playback.state();
        let capture = self.capture.read(cx);
        format!(
            "{loaded}|{title}|{w}x{h}|{pos:.2}|{dur:.2}|{paused}|{vol:.2}|{speed:.3}|{eof}|{error}|\
             {active}|{info}|{notice}|{osd}|{margin}|{auto}|{force}|{sidebar}|{menu}|{path}|{brightness}|\
             {contrast}|{saturation}|{rotation}|{tracks}|{flip_h}|{flip_v}|{zoom}|{panx}|{pany}",
            loaded = state.loaded,
            title = state.title,
            w = state.width,
            h = state.height,
            pos = state.position,
            dur = state.duration,
            paused = state.paused,
            vol = state.volume,
            speed = playback.speed(),
            eof = state.eof,
            error = state.error.is_some(),
            active = capture.is_active(),
            info = capture.display_info(),
            notice = self.notice.clone().unwrap_or_default(),
            osd = self.osd_mode.is_some(),
            margin = self.margin,
            auto = self.margin_auto,
            force = self.margin_force,
            sidebar = self.sidebar_open,
            menu = self.menu_open,
            path = self.open_path.len(),
            brightness = self.brightness,
            contrast = self.contrast,
            saturation = self.saturation,
            rotation = self.rotation,
            tracks = playback.tracks().len(),
            flip_h = self.flip_horizontal,
            flip_v = self.flip_vertical,
            zoom = self.zoom,
            panx = self.pan_x,
            pany = self.pan_y,
        )
    }

    fn poll_playback(&mut self, cx: &mut Context<Self>) {
        self.playback.update(cx, |playback, _| playback.refresh());
        let (loaded, duration, eof, width, height) = {
            let state = self.playback.read(cx).state();
            (state.loaded, state.duration, state.eof, state.width, state.height)
        };
        if let Some(base) = self.auto_rotate {
            if width > 0 && height > 0 && (width, height) != self.auto_rotate_dims {
                self.auto_rotate_dims = (width, height);
                let target = if height > width {
                    (base + 90) % 360
                } else {
                    base
                };
                if target != self.rotation {
                    self.set_rotation(target, cx);
                }
            }
        }
        if let Some(target) = self.resume_pending {
            if loaded && duration > 0.0 {
                self.resume_pending = None;
                let target = target.min(duration - 0.5);
                self.playback
                    .update(cx, |playback, cx| playback.seek_to(target, cx));
            }
        }
        if !eof {
            self.eof_handled = false;
        } else if !self.eof_handled {
            self.eof_handled = true;
            self.handle_eof(cx);
        }
    }

    fn handle_eof(&mut self, cx: &mut Context<Self>) {
        self.remember_current_position(cx);
        let wrap = self.repeat;
        let shuffle = self.shuffle;
        let next = self
            .playlist
            .update(cx, |playlist, _| playlist.step(1, wrap, shuffle));
        if self.post_action_at_eof(next.is_none(), cx) {
            return;
        }
        if let Some(path) = next {
            self.open_media(&path, cx);
        }
    }

    pub fn open_media(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.remember_current_position(cx);
        self.playlist.update(cx, |playlist, _| playlist.focus(path));
        self.resume_pending = self.saved_position(path);
        self.eof_handled = true;
        self.sync_playlist(cx);
        self.playback
            .update(cx, |playback, cx| playback.open(path, cx));
        self.apply_image(cx);
        cx.notify();
    }

    pub fn handle_action(&mut self, action: String, cx: &mut Context<Self>) {
        match action.as_str() {
            "open-file" => self.prompt_open_file(cx),
            "default-open-file" => {
                self.config.default_open_action = "file".to_string();
                self.save_config();
                self.prompt_open_file(cx)
            }
            "close-panel" => {
                self.playback.update(cx, |playback, cx| playback.stop(cx));
                self.capture.update(cx, |capture, cx| capture.stop(cx));
            }
            "open-device" => {
                self.show_notice("请从「摄像头/其他设备」选择采集设备", cx);
            }
            "device-settings" => self.open_settings(1, cx),
            "capture-settings" => self.open_settings(0, cx),
            "play-pause" => self
                .playback
                .update(cx, |playback, cx| playback.toggle_pause(cx)),
            "step-backward" => self
                .playback
                .update(cx, |playback, cx| playback.step_frame(-1, cx)),
            "step-forward" => self
                .playback
                .update(cx, |playback, cx| playback.step_frame(1, cx)),
            "prev-panel" => self.step_playlist(-1, cx),
            "next-panel" => self.step_playlist(1, cx),
            "playlist-add-file" => self.prompt_add_files(cx),
            "playlist-add-folder" => self.prompt_add_folder(cx),
            "playlist-clear" => self.clear_playlist(cx),
            "toggle-sidebar" => self.toggle_sidebar(cx),
            "seek-start" => self
                .playback
                .update(cx, |playback, cx| playback.seek_to(0.0, cx)),
            "seek-end" => {
                let duration = self.playback.read(cx).state().duration;
                self.playback
                    .update(cx, |playback, cx| playback.seek_to(duration, cx));
            }
            "seek-back-10" => self
                .playback
                .update(cx, |playback, cx| playback.seek_relative(-10.0, cx)),
            "seek-forward-10" => self
                .playback
                .update(cx, |playback, cx| playback.seek_relative(10.0, cx)),
            "chapter-prev" => self
                .playback
                .update(cx, |playback, cx| playback.seek_chapter(-1, cx)),
            "chapter-next" => self
                .playback
                .update(cx, |playback, cx| playback.seek_chapter(1, cx)),
            "speed-up" => {
                let speed = self.playback.read(cx).speed() + 0.1;
                self.playback
                    .update(cx, |playback, cx| playback.set_speed(speed, cx));
            }
            "speed-down" => {
                let speed = self.playback.read(cx).speed() - 0.1;
                self.playback
                    .update(cx, |playback, cx| playback.set_speed(speed, cx));
            }
            "volume-up" => {
                let volume = self.playback.read(cx).state().volume + 10.0;
                self.playback
                    .update(cx, |playback, cx| playback.set_volume(volume, cx));
            }
            "volume-down" => {
                let volume = self.playback.read(cx).state().volume - 10.0;
                self.playback
                    .update(cx, |playback, cx| playback.set_volume(volume, cx));
            }
            "mute-toggle" => self
                .playback
                .update(cx, |playback, cx| playback.toggle_mute(cx)),
            "repeat-toggle" => {
                self.repeat = !self.repeat;
                let repeat = self.repeat;
                self.playback
                    .update(cx, |playback, cx| playback.set_repeat(repeat, cx));
                cx.notify();
            }
            "shuffle-toggle" => {
                self.shuffle = !self.shuffle;
                let shuffle = self.shuffle;
                self.playback
                    .update(cx, |playback, cx| playback.set_shuffle(shuffle, cx));
                cx.notify();
            }
            "remember-position-toggle" => {
                self.remember_position = !self.remember_position;
                self.config.remember_position = self.remember_position;
                if !self.remember_position {
                    self.config.resume.clear();
                }
                self.save_config();
                cx.notify();
            }
            "ab-a" => {
                let position = self.playback.read(cx).state().position;
                let value = format!("{position:.3}");
                self.playback
                    .update(cx, |playback, cx| playback.set_property("ab-loop-a", &value, cx));
                self.show_notice(crate::i18n::trf("已设置 A 点：{position:.2} 秒", &[&format!("{position:.2}")]), cx);
            }
            "ab-b" => {
                let position = self.playback.read(cx).state().position;
                let value = format!("{position:.3}");
                self.playback
                    .update(cx, |playback, cx| playback.set_property("ab-loop-b", &value, cx));
                self.show_notice(crate::i18n::trf("已设置 B 点：{position:.2} 秒", &[&format!("{position:.2}")]), cx);
            }
            "ab-off" => {
                self.playback.update(cx, |playback, cx| {
                    playback.set_property("ab-loop-a", "no", cx);
                    playback.set_property("ab-loop-b", "no", cx);
                });
                self.show_notice("已关闭 AB 循环", cx);
            }
            "track:a:no" => self
                .playback
                .update(cx, |playback, cx| playback.set_track("audio", -1, cx)),
            "track:s:no" => self
                .playback
                .update(cx, |playback, cx| playback.set_track("sub", -1, cx)),
            "aspect-fit" | "zoom-none" | "zoom-reset" => self.set_aspect("no", cx),
            "aspect-stretch" => self.set_aspect("0", cx),
            "zoom-stretch-4-3" => self.set_aspect("4:3", cx),
            "zoom-stretch-16-9" => self.set_aspect("16:9", cx),
            "zoom-stretch-16-10" => self.set_aspect("16:10", cx),
            "zoom-stretch-1-85" => self.set_aspect("1.85:1", cx),
            "zoom-stretch-2-35" => self.set_aspect("2.35:1", cx),
            "zoom-crop-4-3" | "zoom-crop-16-9" | "zoom-crop-16-10" | "zoom-crop-1-85"
            | "zoom-crop-2-35" => self.set_crop(action.as_str(), cx),
            "aspect-cycle" => {
                let next = if self.playback.read(cx).aspect() == "0" {
                    "no"
                } else {
                    "0"
                };
                self.set_aspect(next, cx);
            }
            "aspect-ratio-cycle" => {
                const RATIOS: [&str; 6] = ["no", "4:3", "16:9", "16:10", "1.85:1", "2.35:1"];
                let current = self.playback.read(cx).aspect().to_string();
                let index = RATIOS.iter().position(|value| *value == current).unwrap_or(0);
                let next = RATIOS[(index + 1) % RATIOS.len()];
                self.set_aspect(next, cx);
            }
            "brightness-up" => {
                self.brightness = (self.brightness + 1).min(100);
                self.apply_image(cx);
            }
            "brightness-down" => {
                self.brightness = (self.brightness - 1).max(-100);
                self.apply_image(cx);
            }
            "contrast-up" => {
                self.contrast = (self.contrast + 1).min(100);
                self.apply_image(cx);
            }
            "contrast-down" => {
                self.contrast = (self.contrast - 1).max(-100);
                self.apply_image(cx);
            }
            "saturation-up" => {
                self.saturation = (self.saturation + 1).min(100);
                self.apply_image(cx);
            }
            "saturation-down" => {
                self.saturation = (self.saturation - 1).max(-100);
                self.apply_image(cx);
            }
            "adjust-reset" => {
                self.brightness = 0;
                self.contrast = 0;
                self.saturation = 0;
                self.rotation = 0;
                self.flip_horizontal = false;
                self.flip_vertical = false;
                self.zoom = 0.0;
                self.pan_x = 0.0;
                self.pan_y = 0.0;
                self.apply_image(cx);
            }
            "rotate-0" => {
                self.auto_rotate = None;
                self.set_rotation(0, cx)
            }
            "rotate-90" => {
                self.auto_rotate = None;
                self.set_rotation(90, cx)
            }
            "rotate-180" => {
                self.auto_rotate = None;
                self.set_rotation(180, cx)
            }
            "rotate-270" => {
                self.auto_rotate = None;
                self.set_rotation(270, cx)
            }
            "rotate-auto-0-90" => self.set_auto_rotate(0, cx),
            "rotate-auto-180-270" => self.set_auto_rotate(180, cx),
            "rotate-cycle" => {
                self.auto_rotate = None;
                self.set_rotation((self.rotation + 90) % 360, cx)
            }
            "rotate-toggle" => {
                self.auto_rotate = None;
                let next = if self.rotation == 0 { 90 } else { 0 };
                self.set_rotation(next, cx)
            }
            "flip-horizontal" => {
                self.flip_horizontal = !self.flip_horizontal;
                self.apply_image(cx);
            }
            "flip-vertical" => {
                self.flip_vertical = !self.flip_vertical;
                self.apply_image(cx);
            }
            "zoom-up" => {
                self.zoom = (self.zoom + 0.1).min(2.0);
                self.apply_image(cx);
            }
            "zoom-down" => {
                self.zoom = (self.zoom - 0.1).max(-1.0);
                self.apply_image(cx);
            }
            "zoom-left" => {
                self.pan_x = (self.pan_x - 0.02).max(-1.0);
                self.apply_image(cx);
            }
            "zoom-right" => {
                self.pan_x = (self.pan_x + 0.02).min(1.0);
                self.apply_image(cx);
            }
            "osd" => self.toggle_osd(OsdMode::Full, cx),
            "osd-brief" => self.toggle_osd(OsdMode::Brief, cx),
            "osd-playback" => self.toggle_osd(OsdMode::Playback, cx),
            "snapshot" | "snapshot-immediate" => self.take_snapshot(cx),
            "snapshot-format-png" => self.set_snapshot_format("png", cx),
            "snapshot-format-jpeg" => self.set_snapshot_format("jpeg", cx),
            "snapshot-format-bmp" => self.set_snapshot_format("bmp", cx),
            "snapshot-name-time" => self.set_snapshot_name("time", cx),
            "snapshot-name-playback" => self.set_snapshot_name("playback", cx),
            "snapshot-play-folder-toggle" => {
                self.config.snapshot_play_folder = !self.config.snapshot_play_folder;
                self.save_config();
                cx.notify();
            }
            "snapshot-path-choose" => self.prompt_snapshot_folder(cx),
            "snapshot-delete-last" => self.delete_last_snapshot(cx),
            "reopen" => self.reopen(cx),
            "about" => self.show_notice(
                format!("FlintPlayer {} · Rust + GPUI + libmpv", env!("CARGO_PKG_VERSION")),
                cx,
            ),
            "maximize" => self.request_window(WindowCommand::ToggleMaximize, cx),
            "window-grow" => self.request_window(WindowCommand::Scale(1.1), cx),
            "window-shrink" => self.request_window(WindowCommand::Scale(1.0 / 1.1), cx),
            "screen-size-45" => self.request_window(WindowCommand::Fraction(0.45), cx),
            "screen-size-60" => self.request_window(WindowCommand::Fraction(0.60), cx),
            "screen-size-75" => self.request_window(WindowCommand::Fraction(0.75), cx),
            "screen-size-borderless" => self.request_window(WindowCommand::Fraction(1.0), cx),
            "screen-size-custom" => {
                let (width, height) = (
                    self.config.window_custom_width as f32,
                    self.config.window_custom_height as f32,
                );
                self.request_window(WindowCommand::Resize(width, height), cx);
            }
            "copy-file-name" | "copy-file-dir" | "copy-file-path" => {
                self.copy_file_info(action.as_str(), cx)
            }
            "subtitle-open" => self.prompt_track(cx, TrackKind::Subtitle, false),
            "subtitle-secondary-open" => self.prompt_track(cx, TrackKind::Subtitle, true),
            "subtitle-reload" => {
                self.playback
                    .update(cx, |playback, cx| playback.command("sub-reload", cx));
                self.show_notice("已重载字幕", cx);
            }
            "load-audio" => self.prompt_track(cx, TrackKind::Audio, false),
            "snapshot-open-folder" => {
                self.open_directory(self.config.snapshot_dir.clone(), cx)
            }
            "video-open-folder" => {
                self.open_directory(self.config.video_record_dir.clone(), cx)
            }
            "video-path-choose" => self.prompt_video_folder(cx),
            "window-pos-top-left" => self.place_window("top-left", cx),
            "window-pos-top-right" => self.place_window("top-right", cx),
            "window-pos-bottom-left" => self.place_window("bottom-left", cx),
            "window-pos-bottom-right" => self.place_window("bottom-right", cx),
            "window-pos-center" => self.place_window("center", cx),
            "window-pos-restore" => self.place_window("restore", cx),
            "topmost-toggle" => {
                let enable = !self.config.always_on_top;
                self.config.always_on_top = enable;
                self.config.topmost_mode = if enable { "always" } else { "never" }.to_string();
                self.apply_topmost(cx);
            }
            "topmost-always" => self.set_topmost_mode("always", cx),
            "topmost-playing" => self.set_topmost_mode("playing", cx),
            "topmost-fullscreen" => self.set_topmost_mode("fullscreen", cx),
            "margin-cycle" => self.set_margin((self.margin + 5) % 35, cx),
            "margin-0" => self.set_margin(0, cx),
            "margin-5" => self.set_margin(5, cx),
            "margin-10" => self.set_margin(10, cx),
            "margin-15" => self.set_margin(15, cx),
            "margin-20" => self.set_margin(20, cx),
            "margin-25" => self.set_margin(25, cx),
            "margin-30" => self.set_margin(30, cx),
            "margin-auto-toggle" => {
                self.margin_auto = !self.margin_auto;
                self.show_notice(
                    format!("字幕自动下边距：{}", if self.margin_auto { "开" } else { "关" }),
                    cx,
                );
            }
            "margin-force-toggle" => {
                self.margin_force = !self.margin_force;
                self.show_notice(
                    format!("强制下边距：{}", if self.margin_force { "开" } else { "关" }),
                    cx,
                );
            }
            "frame-preset-50" => self.set_frame_scale(50, 50, cx),
            "frame-preset-100" => self.set_frame_scale(100, 100, cx),
            "frame-preset-150" => self.set_frame_scale(150, 150, cx),
            "frame-preset-200" => self.set_frame_scale(200, 200, cx),
            "frame-larger" => self.step_frame_scale(10, 10, cx),
            "frame-smaller" => self.step_frame_scale(-10, -10, cx),
            "frame-wider" => self.step_frame_scale(10, 0, cx),
            "frame-narrower" => self.step_frame_scale(-10, 0, cx),
            "frame-taller" => self.step_frame_scale(0, 10, cx),
            "frame-shorter" => self.step_frame_scale(0, -10, cx),
            "frame-shift-left" => self.shift_frame(-5, 0, cx),
            "frame-shift-right" => self.shift_frame(5, 0, cx),
            "frame-shift-up" => self.shift_frame(0, -5, cx),
            "frame-shift-down" => self.shift_frame(0, 5, cx),
            "frame-center" => self.reset_frame(cx),
            "frame-remember-toggle" => {
                self.config.remember_frame = !self.config.remember_frame;
                self.save_config();
                self.show_notice(
                    format!(
                        "记忆图像尺寸/位置：{}",
                        if self.config.remember_frame { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "stereo-cycle" => {
                let next = self.playback.read(cx).stereo().next();
                self.set_stereo_mode(next.key(), cx);
            }
            "stereo-off" => self.set_stereo_mode("off", cx),
            "stereo-anaglyph-sbs" => self.set_stereo_mode("anaglyph-sbs", cx),
            "stereo-anaglyph-tab" => self.set_stereo_mode("anaglyph-tab", cx),
            "stereo-left-sbs" => self.set_stereo_mode("left-sbs", cx),
            "stereo-right-sbs" => self.set_stereo_mode("right-sbs", cx),
            "stereo-left-tab" => self.set_stereo_mode("left-tab", cx),
            "stereo-right-tab" => self.set_stereo_mode("right-tab", cx),
            "pano-toggle" => self.toggle_pano(cx),
            "pano-reset" => self.reset_pano(cx),
            "renderer-auto" | "renderer-opengl" | "renderer-null-compressed"
            | "renderer-null-uncompressed" => self.set_renderer(action.as_str(), cx),
            "record" => self.toggle_recording(cx),
            "video-record-play-folder" => {
                self.config.video_record_play_folder = !self.config.video_record_play_folder;
                self.save_config();
                self.show_notice(
                    format!(
                        "录制存入播放文件夹：{}",
                        if self.config.video_record_play_folder { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "burst-config" => self.prompt_burst_config(cx),
            "burst-immediate" => self.burst_snapshots(cx),
            "capture-slot1" | "capture-start1" => self.start_screen_capture(1, cx),
            "capture-slot2" | "capture-start2" => self.start_screen_capture(2, cx),
            "capture-size1" => self.prompt_capture_size(1, cx),
            "capture-size2" => self.prompt_capture_size(2, cx),
            "capture-smoothing" => self.prompt_capture_smoothing(cx),
            "open-url" | "open-network" => self.prompt_open_url(cx),
            "browser" => self.prompt_external_browser(cx),
            "live-search" => self.prompt_live_search(cx),
            "open-folder" => self.prompt_open_folder(cx),
            "open-clipboard" => self.open_clipboard(cx),
            "default-open-url" => self.set_default_open_action("url", cx),
            "default-open-folder" => self.set_default_open_action("folder", cx),
            "default-open-navigate" => self.set_default_open_action("navigate", cx),
            "navigate-bookmarks" => self.prompt_bookmarks(cx),
            "navigate-settings" => self.open_settings(1, cx),
            "menu-search" => self.prompt_menu_search(cx),
            "panel-cycle" => self.cycle_panel(cx),
            "panel-1" => self.select_panel(1, cx),
            "panel-2" => self.select_panel(2, cx),
            "panel-3" => self.select_panel(3, cx),
            "panel-4" => self.select_panel(4, cx),
            "thumbnail-config" => self.create_thumbnail(cx),
            "config-manager" => self.prompt_config_manager(cx),
            "screen-size-settings" => self.prompt_window_size(cx),
            "avisynth-toggle" => self.toggle_filter("avisynth", cx),
            "avisynth-script-choose" => self.choose_filter_script("avisynth", cx),
            "avisynth-folder" => self.open_filter_folder("avisynth", cx),
            "vapoursynth-toggle" => self.toggle_filter("vapoursynth", cx),
            "vapoursynth-script-choose" => self.choose_filter_script("vapoursynth", cx),
            "vapoursynth-folder" => self.open_filter_folder("vapoursynth", cx),
            "similar-selected" => self.set_similar_mode("selected", cx),
            "similar-similar" => self.set_similar_mode("similar", cx),
            "similar-all" => self.set_similar_mode("all", cx),
            "language-auto" => self.set_language("auto", cx),
            "language-zh" => self.set_language("zh", cx),
            "language-en" => self.set_language("en", cx),
            "ignore-shortcuts-toggle" => {
                self.config.ignore_shortcuts = !self.config.ignore_shortcuts;
                self.save_config();
                self.show_notice(
                    format!(
                        "忽略默认快捷键：{}",
                        if self.config.ignore_shortcuts { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "favorites-memory-toggle" => {
                self.config.favorites_memory_points = !self.config.favorites_memory_points;
                self.save_config();
                self.show_notice(
                    format!(
                        "收藏菜单显示播放记忆点：{}",
                        if self.config.favorites_memory_points { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "touch-toggle" => {
                self.config.touch_enabled = !self.config.touch_enabled;
                self.save_config();
                self.show_notice(
                    format!(
                        "触屏功能：{}",
                        if self.config.touch_enabled { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "move-titlebar-toggle" => {
                self.config.move_only_titlebar = !self.config.move_only_titlebar;
                self.save_config();
                self.show_notice(
                    format!(
                        "仅在标题栏移动窗口：{}",
                        if self.config.move_only_titlebar { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "remember-post-toggle" => {
                self.config.remember_post_action = !self.config.remember_post_action;
                self.save_config();
                self.show_notice(
                    format!(
                        "记忆收尾操作：{}",
                        if self.config.remember_post_action { "开" } else { "关" }
                    ),
                    cx,
                );
            }
            "post-cycle" | "post-exit-current" | "post-exit-all" | "post-off-current"
            | "post-off-all" => self.set_post_action(action.as_str(), cx),
            "shutdown-default" | "shutdown-suspend" | "shutdown-hibernate" => {
                self.set_shutdown_mode(action.as_str(), cx)
            }
            _ => self.handle_dynamic_action(action, cx),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if key == "escape" {
            if self.settings.read(cx).is_open() {
                self.settings.update(cx, |settings, cx| settings.close(cx));
                return;
            }
            if self.menu_open {
                self.close_menu(cx);
            }
            return;
        }
        if self.menu_open || self.settings.read(cx).is_open() {
            return;
        }
        if self.config.ignore_shortcuts {
            return;
        }

        if modifiers.control && key == "o" {
            self.handle_action("open-file".into(), cx);
            return;
        }
        if modifiers.alt && key == "d" {
            self.handle_action("device-settings".into(), cx);
            return;
        }
        if modifiers.control && key == "f1" {
            self.handle_action("osd".into(), cx);
            return;
        }
        if modifiers.control && key == "e" {
            self.handle_action("snapshot".into(), cx);
            return;
        }
        if modifiers.control && key == "p" {
            self.handle_action("flip-vertical".into(), cx);
            return;
        }
        if modifiers.control && key == "z" {
            self.handle_action("flip-horizontal".into(), cx);
            return;
        }
        if modifiers.alt && key == "k" {
            self.handle_action("rotate-cycle".into(), cx);
            return;
        }
        if modifiers.control && key == "w" {
            self.handle_action("close-panel".into(), cx);
            return;
        }
        if modifiers.control && key == "f5" {
            self.handle_action("aspect-cycle".into(), cx);
            return;
        }
        if modifiers.control && key == "f6" {
            self.handle_action("aspect-ratio-cycle".into(), cx);
            return;
        }
        if modifiers.alt && key == "enter" {
            window.toggle_fullscreen();
            return;
        }
        if let Some(action) = self.shortcut_action(key, modifiers) {
            self.handle_action(action, cx);
            return;
        }
        if modifiers.control || modifiers.alt {
            return;
        }

        match key {
            "space" => self.handle_action("play-pause".into(), cx),
            "," | "comma" => self.handle_action("step-backward".into(), cx),
            "." | "period" => self.handle_action("step-forward".into(), cx),
            "f3" => self.handle_action("open-file".into(), cx),
            "f4" => self.handle_action("close-panel".into(), cx),
            "f6" => self.handle_action("toggle-sidebar".into(), cx),
            "s" | "k" => self.handle_action("snapshot".into(), cx),
            "w" => self.handle_action("brightness-down".into(), cx),
            "e" => self.handle_action("brightness-up".into(), cx),
            "y" => self.handle_action("saturation-down".into(), cx),
            "u" => self.handle_action("saturation-up".into(), cx),
            "q" => self.handle_action("adjust-reset".into(), cx),
            "pageup" => self.handle_action("prev-panel".into(), cx),
            "pagedown" => self.handle_action("next-panel".into(), cx),
            "f" | "enter" => window.toggle_fullscreen(),
            "left" => self
                .playback
                .update(cx, |playback, cx| playback.seek_relative(-5.0, cx)),
            "right" => self
                .playback
                .update(cx, |playback, cx| playback.seek_relative(5.0, cx)),
            "up" => {
                let volume = self.playback.read(cx).state().volume + 5.0;
                self.playback
                    .update(cx, |playback, cx| playback.set_volume(volume, cx));
            }
            "down" => {
                let volume = self.playback.read(cx).state().volume - 5.0;
                self.playback
                    .update(cx, |playback, cx| playback.set_volume(volume, cx));
            }
            "m" => self
                .playback
                .update(cx, |playback, cx| playback.toggle_mute(cx)),
            "c" => self.handle_action("speed-up".into(), cx),
            "x" => self.handle_action("speed-down".into(), cx),
            "z" => self.handle_action("speed:1".into(), cx),
            "[" => self.handle_action("chapter-prev".into(), cx),
            "]" => self.handle_action("chapter-next".into(), cx),
            _ => {}
        }
    }

    fn handle_dynamic_action(&mut self, action: String, cx: &mut Context<Self>) {
        if let Some(value) = action.strip_prefix("capture:") {
            let (source, name) = value
                .split_once('|')
                .map(|(source, name)| (source.to_string(), name.to_string()))
                .unwrap_or_else(|| (value.to_string(), String::new()));
            self.playback.update(cx, |playback, cx| playback.stop(cx));
            self.capture
                .update(cx, |capture, cx| capture.start(source, name, cx));
            return;
        }
        if let Some(value) = action.strip_prefix("speed:") {
            if let Ok(speed) = value.parse::<f64>() {
                self.playback
                    .update(cx, |playback, cx| playback.set_speed(speed, cx));
                return;
            }
        }
        if let Some(value) = action.strip_prefix("chapter:") {
            if let Ok(index) = value.parse::<usize>() {
                let start = self
                    .playback
                    .read(cx)
                    .chapters()
                    .get(index)
                    .map(|chapter| chapter.start);
                if let Some(start) = start {
                    self.playback
                        .update(cx, |playback, cx| playback.seek_to(start, cx));
                }
                return;
            }
        }
        if let Some(value) = action.strip_prefix("track:a:") {
            if let Ok(id) = value.parse::<i64>() {
                self.playback
                    .update(cx, |playback, cx| playback.set_track("audio", id, cx));
                return;
            }
        }
        if let Some(value) = action.strip_prefix("track:s:") {
            if let Ok(id) = value.parse::<i64>() {
                self.playback
                    .update(cx, |playback, cx| playback.set_track("sub", id, cx));
                return;
            }
        }
        self.show_notice(action, cx);
    }

    fn shortcut_action(&self, key: &str, modifiers: Modifiers) -> Option<String> {
        let key = key.to_ascii_lowercase();
        self.shortcuts.iter().find_map(|(shortcut, action)| {
            let (control, alt, shift, name) = parse_shortcut(shortcut)?;
            (name == key
                && control == modifiers.control
                && alt == modifiers.alt
                && shift == modifiers.shift)
                .then(|| action.clone())
        })
    }

    fn request_window(&mut self, command: WindowCommand, cx: &mut Context<Self>) {
        self.pending_window = Some(command);
        cx.notify();
    }

    fn copy_file_info(&mut self, action: &str, cx: &mut Context<Self>) {
        let current = {
            let playlist = self.playlist.read(cx);
            playlist
                .current()
                .and_then(|index| playlist.items().get(index).cloned())
        };
        let Some(path) = current else {
            self.show_notice("当前没有打开的文件", cx);
            return;
        };
        let text = match action {
            "copy-file-name" => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            "copy-file-dir" => path.parent().map(|dir| dir.display().to_string()),
            _ => Some(path.display().to_string()),
        };
        let Some(text) = text else {
            self.show_notice("无法获取文件路径", cx);
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.show_notice(crate::i18n::trf("已复制：{text}", &[&text]), cx);
    }

    fn prompt_track(&mut self, cx: &mut Context<Self>, kind: TrackKind, secondary: bool) {
        let (prompt, audio) = match (kind, secondary) {
            (TrackKind::Audio, _) => ("选择音频文件", true),
            (TrackKind::Subtitle, true) => ("选择次字幕文件", false),
            (TrackKind::Subtitle, false) => ("打开字幕文件", false),
        };
        let paths = self.pick_paths(cx, prompt, false, false);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                if let Some(path) = paths.first() {
                    let _ = this.update(cx, |this, cx| this.add_track(path, secondary, audio, cx));
                }
            }
        })
        .detach();
    }

    fn pick_paths(
        &mut self,
        cx: &mut Context<Self>,
        prompt: &'static str,
        directories: bool,
        multiple: bool,
    ) -> Task<Option<Vec<std::path::PathBuf>>> {
        if zenity_available() {
            return cx
                .background_executor()
                .spawn(async move { zenity_pick(prompt, directories, multiple) });
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple,
            prompt: Some(prompt.into()),
        });
        cx.background_executor().spawn(async move {
            match picked.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => Some(paths),
                _ => None,
            }
        })
    }

    fn add_track(
        &mut self,
        path: &Path,
        secondary: bool,
        audio: bool,
        cx: &mut Context<Self>,
    ) {
        let escaped = path.display().to_string().replace('\\', "\\\\");
        if audio {
            let command = format!("audio-add \"{escaped}\" select");
            self.playback.update(cx, |playback, cx| playback.command(&command, cx));
            self.show_notice(crate::i18n::trf("已载入音频：{}", &[&path.display()]), cx);
            return;
        }
        let flags = if secondary { "auto" } else { "select" };
        let command = format!("sub-add \"{escaped}\" {flags}");
        self.playback.update(cx, |playback, cx| playback.command(&command, cx));
        if secondary {
            let id = self
                .playback
                .read(cx)
                .tracks()
                .into_iter()
                .filter(|track| track.kind == "sub")
                .map(|track| track.id)
                .max();
            if let Some(id) = id {
                self.playback.update(cx, |playback, cx| {
                    playback.set_property("secondary-sid", &id.to_string(), cx)
                });
            }
        }
        self.show_notice(
            format!(
                "{}：{}",
                if secondary { "已载入次字幕" } else { "已载入字幕" },
                path.display()
            ),
            cx,
        );
    }

    fn open_directory(&mut self, directory: std::path::PathBuf, cx: &mut Context<Self>) {
        match std::process::Command::new("xdg-open")
            .arg(&directory)
            .spawn()
        {
            Ok(_) => self.show_notice(crate::i18n::trf("已打开文件夹：{}", &[&directory.display()]), cx),
            Err(error) => self.show_notice(crate::i18n::trf("打开文件夹失败：{error}", &[&error]), cx),
        }
    }

    fn prompt_video_folder(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "选择视频录制存档路径", true, false);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                if let Some(folder) = paths.first() {
                    let _ = this.update(cx, |this, cx| {
                        this.config.video_record_dir = folder.clone();
                        this.save_config();
                        this.show_notice(crate::i18n::trf("录制存档路径：{}", &[&folder.display()]), cx);
                    });
                }
            }
        })
        .detach();
    }

    fn set_margin(&mut self, margin: i32, cx: &mut Context<Self>) {
        self.margin = margin.clamp(0, 30);
        self.show_notice(crate::i18n::trf("下边距：{}%", &[&self.margin]), cx);
    }

    fn set_frame_scale(&mut self, scale_x: i32, scale_y: i32, cx: &mut Context<Self>) {
        self.config.frame_scale_x = scale_x.clamp(10, 400);
        self.config.frame_scale_y = scale_y.clamp(10, 400);
        self.persist_frame();
        self.show_notice(
            format!(
                "帧尺寸：{}% × {}%",
                self.config.frame_scale_x, self.config.frame_scale_y
            ),
            cx,
        );
    }

    fn step_frame_scale(&mut self, delta_x: i32, delta_y: i32, cx: &mut Context<Self>) {
        let scale_x = self.config.frame_scale_x + delta_x;
        let scale_y = self.config.frame_scale_y + delta_y;
        self.set_frame_scale(scale_x, scale_y, cx);
    }

    fn shift_frame(&mut self, delta_x: i32, delta_y: i32, cx: &mut Context<Self>) {
        self.config.frame_shift_x = (self.config.frame_shift_x + delta_x).clamp(-300, 300);
        self.config.frame_shift_y = (self.config.frame_shift_y + delta_y).clamp(-300, 300);
        self.persist_frame();
        self.show_notice(
            format!(
                "帧位偏移：{} × {}",
                self.config.frame_shift_x, self.config.frame_shift_y
            ),
            cx,
        );
    }

    fn reset_frame(&mut self, cx: &mut Context<Self>) {
        self.config.frame_shift_x = 0;
        self.config.frame_shift_y = 0;
        self.persist_frame();
        self.show_notice("帧位已复位", cx);
    }

    fn persist_frame(&mut self) {
        if self.config.remember_frame {
            self.save_config();
        }
    }

    fn set_stereo_mode(&mut self, key: &str, cx: &mut Context<Self>) {
        let mode = StereoMode::from_key(key);
        self.playback
            .update(cx, |playback, cx| playback.set_stereo(mode, cx));
        self.config.stereo_mode = mode.key().to_string();
        self.save_config();
        let label = match mode {
            StereoMode::Off => "关闭",
            StereoMode::AnaglyphSideBySide => "红蓝立体（左右并排）",
            StereoMode::AnaglyphTopBottom => "红蓝立体（上下）",
            StereoMode::LeftSideBySide => "仅左眼（左右并排）",
            StereoMode::RightSideBySide => "仅右眼（左右并排）",
            StereoMode::LeftTopBottom => "仅左眼（上下）",
            StereoMode::RightTopBottom => "仅右眼（上下）",
        };
        self.show_notice(crate::i18n::trf("3D 输出：{label}", &[&label]), cx);
    }

    fn toggle_pano(&mut self, cx: &mut Context<Self>) {
        self.config.pano_enabled = !self.config.pano_enabled;
        self.save_config();
        self.apply_pano(cx);
        self.show_notice(
            format!(
                "360° 视频：{}",
                if self.config.pano_enabled { "开" } else { "关" }
            ),
            cx,
        );
    }

    fn reset_pano(&mut self, cx: &mut Context<Self>) {
        self.config.pano_yaw = 0.0;
        self.config.pano_pitch = 0.0;
        self.config.pano_fov = 90.0;
        self.save_config();
        self.apply_pano(cx);
        self.show_notice("360° 视角已复位", cx);
    }

    fn apply_pano(&mut self, cx: &mut Context<Self>) {
        let view = self
            .config
            .pano_enabled
            .then(|| flint_media::panorama::View {
                yaw: self.config.pano_yaw.to_radians(),
                pitch: self.config.pano_pitch.to_radians(),
                fov: self.config.pano_fov.to_radians(),
            });
        self.playback
            .update(cx, |playback, cx| playback.set_pano(view, cx));
    }

    fn set_renderer(&mut self, action: &str, cx: &mut Context<Self>) {
        let (key, label) = match action {
            "renderer-opengl" => ("opengl", "内置 OpenGL 视频渲染器"),
            "renderer-null-compressed" => ("null-compressed", "空视频渲染器(压缩)"),
            "renderer-null-uncompressed" => ("null-uncompressed", "空视频渲染器(未压缩)"),
            _ => ("auto", "自动选择(推荐)"),
        };
        self.config.video_renderer = key.to_string();
        self.save_config();
        let muted = key.starts_with("null");
        let hwdec = if key == "opengl" { "no" } else { "auto" };
        self.playback.update(cx, |playback, cx| {
            playback.set_property("vid", if muted { "no" } else { "auto" }, cx);
            if !muted {
                playback.set_property("hwdec", hwdec, cx);
            }
        });
        self.show_notice(crate::i18n::trf("视频渲染器：{label}", &[&label]), cx);
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.playback.read(cx).is_recording() {
            let result = self.playback.update(cx, |playback, _| playback.stop_recording());
            match result {
                Some(Ok(summary)) => self.show_notice(
                    format!(
                        "录制已保存：{}（{} 帧）",
                        summary.path.display(),
                        summary.frames
                    ),
                    cx,
                ),
                Some(Err(error)) => self.show_notice(crate::i18n::trf("录制失败：{error}", &[&error]), cx),
                None => {}
            }
            return;
        }
        if !self.playback.read(cx).is_loaded() {
            self.show_notice("当前没有可录制的画面", cx);
            return;
        }
        let directory = if self.config.video_record_play_folder {
            let current = {
                let playlist = self.playlist.read(cx);
                playlist
                    .current()
                    .and_then(|index| playlist.items().get(index).cloned())
            };
            current
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.config.video_record_dir.clone())
        } else {
            self.config.video_record_dir.clone()
        };
        if let Err(error) = std::fs::create_dir_all(&directory) {
            self.show_notice(crate::i18n::trf("创建录制目录失败：{error}", &[&error]), cx);
            return;
        }
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let path = directory.join(format!("flintplayer_{millis}.mp4"));
        let fps = if self.config.fps > 0 {
            self.config.fps as f64
        } else {
            30.0
        };
        let result = self
            .playback
            .update(cx, |playback, _| playback.start_recording(&path, fps));
        match result {
            Ok(()) => self.show_notice(crate::i18n::trf("开始录制：{}", &[&path.display()]), cx),
            Err(error) => self.show_notice(crate::i18n::trf("无法录制：{error}", &[&error]), cx),
        }
    }

    fn prompt_burst_config(&mut self, cx: &mut Context<Self>) {
        let interval = format!("{}", self.config.snapshot_interval_ms);
        let count = format!("{}", self.config.snapshot_count);
        let forms = cx.background_executor().spawn(async move {
            zenity_forms(
                "连续截图",
                &[("间隔(毫秒)", &interval), ("数量", &count)],
            )
        });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(values) = forms.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(value) = values.first().and_then(|value| value.trim().parse().ok()) {
                    this.config.snapshot_interval_ms = u64::clamp(value, 50, 60_000);
                }
                if let Some(value) = values.get(1).and_then(|value| value.trim().parse().ok()) {
                    this.config.snapshot_count = i32::clamp(value, 1, 999);
                }
                this.save_config();
                let message = format!(
                    "连续截图：每 {} 毫秒，共 {} 张",
                    this.config.snapshot_interval_ms, this.config.snapshot_count
                );
                this.show_notice(message, cx);
            });
        })
        .detach();
    }

    fn burst_snapshots(&mut self, cx: &mut Context<Self>) {
        if !self.playback.read(cx).is_loaded() {
            self.show_notice("当前没有可截取的画面", cx);
            return;
        }
        let total = self.config.snapshot_count.clamp(1, 999) as u32;
        let interval = self.config.snapshot_interval_ms.clamp(50, 60_000);
        let this = cx.entity().downgrade();
        self.show_notice(crate::i18n::trf("连续截图：{total} 张", &[&total]), cx);
        cx.spawn(async move |_, cx| {
            for _ in 0..total {
                let _ = this.update(cx, |this, cx| this.take_snapshot(cx));
                cx.background_executor()
                    .timer(Duration::from_millis(interval))
                    .await;
            }
        })
        .detach();
    }

    fn start_screen_capture(&mut self, slot: u32, cx: &mut Context<Self>) {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        let source = format!("screen{slot}:{display}");
        let (width, height) = if slot == 2 {
            (self.config.screen2_width, self.config.screen2_height)
        } else {
            (self.config.screen1_width, self.config.screen1_height)
        };
        let fps = self.config.fps.max(1);
        self.playback
            .update(cx, |playback, cx| playback.stop(cx));
        self.capture.update(cx, |capture, cx| {
            capture.start_with(
                source,
                format!("X11 屏幕采集 {slot}#"),
                CaptureSettings {
                    width,
                    height,
                    fps,
                    fourcc: None,
                    input_format: None,
                    pixel_format: None,
                    smoothing: CaptureSmoothing::from_key(&self.config.capture_smoothing),
                },
                cx,
            )
        });
        self.show_notice(crate::i18n::trf("开始采集屏幕 {slot}#", &[&slot]), cx);
    }

    fn prompt_capture_size(&mut self, slot: u32, cx: &mut Context<Self>) {
        let title = format!("截屏 {slot} 尺寸设置");
        let forms = cx.background_executor().spawn(async move {
            zenity_forms(&title, &[("宽度", ""), ("高度", "")])
        });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(values) = forms.await else {
                return;
            };
            let parse = |value: Option<&String>, fallback: u32| {
                value
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .filter(|value| (16..=8192).contains(value))
                    .unwrap_or(fallback)
            };
            let _ = this.update(cx, |this, cx| {
                if slot == 2 {
                    this.config.screen2_width = parse(values.first(), this.config.screen2_width);
                    this.config.screen2_height = parse(values.get(1), this.config.screen2_height);
                } else {
                    this.config.screen1_width = parse(values.first(), this.config.screen1_width);
                    this.config.screen1_height = parse(values.get(1), this.config.screen1_height);
                }
                this.save_config();
                let (width, height) = if slot == 2 {
                    (this.config.screen2_width, this.config.screen2_height)
                } else {
                    (this.config.screen1_width, this.config.screen1_height)
                };
                this.show_notice(crate::i18n::trf("截屏 {slot} 尺寸：{width}×{height}", &[&slot, &width, &height]), cx);
            });
        })
        .detach();
    }

    fn prompt_capture_smoothing(&mut self, cx: &mut Context<Self>) {
        const OPTIONS: [(&str, &str); 6] = [
            ("none", "不使用平滑滤镜"),
            ("system", "使用系统默认平滑滤镜(慢)"),
            ("low", "使用 PotPlayer 低品质平滑滤镜"),
            ("normal", "使用 PotPlayer 普通品质平滑滤镜"),
            ("medium", "使用 PotPlayer 适中品质平滑滤镜"),
            ("high", "使用 PotPlayer 高品质平滑滤镜"),
        ];
        let current = OPTIONS
            .iter()
            .position(|option| option.0 == self.config.capture_smoothing.as_str())
            .unwrap_or(1);
        let labels: Vec<String> = OPTIONS
            .iter()
            .map(|option| crate::i18n::tr(option.1))
            .collect();
        let rows: Vec<Vec<String>> = labels.iter().map(|label| vec![label.clone()]).collect();
        let title = crate::i18n::tr("屏幕采集设置");
        let text = crate::i18n::tr(
            "选择更改图像尺寸时使用的滤镜。因使用平滑滤镜会造成CPU负载过高，所以如非必要请勿使用。",
        );
        let list = cx.background_executor().spawn(async move {
            zenity_radio_list(&title, &text, &["平滑滤镜"], rows, current, 460)
        });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(choice) = list.await.and_then(|cells| cells.into_iter().next()) else {
                return;
            };
            let Some(index) = labels.iter().position(|label| *label == choice) else {
                return;
            };
            let key = OPTIONS[index].0.to_string();
            let _ = this.update(cx, |this, cx| {
                this.config.capture_smoothing = key.clone();
                this.save_config();
                this.capture.update(cx, |capture, cx| {
                    capture.set_smoothing(CaptureSmoothing::from_key(&key), cx)
                });
                let prefix = crate::i18n::tr("平滑滤镜");
                this.show_notice(crate::i18n::trf("{prefix}: {choice}", &[&prefix, &choice]), cx);
            });
        })
        .detach();
    }

    fn prompt_open_url(&mut self, cx: &mut Context<Self>) {
        let entry = cx
            .background_executor()
            .spawn(async { zenity_entry("打开链接", "请输入链接地址或文件路径") });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(text) = entry.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open_source(&text, cx));
        })
        .detach();
    }

    fn prompt_external_browser(&mut self, cx: &mut Context<Self>) {
        let entry = cx
            .background_executor()
            .spawn(async { zenity_entry("浏览器", "输入要在浏览器中打开的地址") });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(text) = entry.await else {
                return;
            };
            let target = if text.contains("://") {
                text
            } else {
                format!("https://{text}")
            };
            let result = std::process::Command::new("xdg-open")
                .arg(&target)
                .spawn();
            let _ = this.update(cx, |this, cx| match result {
                Ok(_) => this.show_notice(crate::i18n::trf("已在浏览器中打开：{target}", &[&target]), cx),
                Err(error) => this.show_notice(crate::i18n::trf("无法启动浏览器：{error}", &[&error]), cx),
            });
        })
        .detach();
    }

    fn prompt_live_search(&mut self, cx: &mut Context<Self>) {
        let entry = cx
            .background_executor()
            .spawn(async { zenity_entry("简索直播", "输入直播流地址（RTSP/RTMP/HLS）") });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(url) = entry.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open_source(&url, cx));
        })
        .detach();
    }

    fn prompt_open_folder(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "打开文件夹", true, false);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(paths) = paths.await else {
                return;
            };
            if let Some(folder) = paths.first() {
                let _ = this.update(cx, |this, cx| this.open_folder(folder, cx));
            }
        })
        .detach();
    }

    fn open_folder(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let before = self.playlist.read(cx).len();
        let added = self
            .playlist
            .update(cx, |playlist, _| playlist.add_folder(folder));
        if added == 0 {
            self.show_notice("文件夹中没有可播放的媒体文件", cx);
            return;
        }
        self.sync_playlist(cx);
        if let Some(path) = self.playlist.read(cx).items().get(before).cloned() {
            self.open_media(&path, cx);
        }
        self.show_notice(
            format!("已打开文件夹：{}（{added} 个文件）", folder.display()),
            cx,
        );
    }

    fn open_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            self.show_notice("剪贴板为空", cx);
            return;
        };
        let Some(text) = item.text() else {
            self.show_notice("剪贴板中没有文本", cx);
            return;
        };
        self.open_source(text.trim(), cx);
    }

    fn open_source(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(text);
        if path.is_dir() {
            self.open_folder(&path, cx);
            return;
        }
        if path.is_file() {
            self.add_to_playlist(vec![path.clone()], cx);
            self.open_media(&path, cx);
            return;
        }
        self.playlist
            .update(cx, |playlist, _| playlist.add_files(vec![path.clone()]));
        self.sync_playlist(cx);
        self.open_media(&path, cx);
        self.show_notice(crate::i18n::trf("打开链接：{text}", &[&text]), cx);
    }

    fn set_default_open_action(&mut self, mode: &str, cx: &mut Context<Self>) {
        self.config.default_open_action = mode.to_string();
        self.save_config();
        let label = match mode {
            "url" => "打开链接",
            "folder" => "打开文件夹",
            "navigate" => "简索",
            _ => "打开文件",
        };
        self.show_notice(crate::i18n::trf("默认打开方式：{label}", &[&label]), cx);
    }

    fn prompt_bookmarks(&mut self, cx: &mut Context<Self>) {
        if self.config.bookmarks.is_empty() {
            self.show_notice("还没有书签", cx);
            return;
        }
        let mut rows = Vec::new();
        for (file, times) in &self.config.bookmarks {
            let name = Path::new(file)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| file.clone());
            for time in times {
                rows.push(vec![name.clone(), format_clock(*time), file.clone()]);
            }
        }
        let list = cx
            .background_executor()
            .spawn(async move { zenity_list("简索书签", &["文件", "时间"], rows) });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(row) = list.await else {
                return;
            };
            let Some(time) = row.get(1).and_then(|value| parse_clock(value)) else {
                return;
            };
            let Some(file) = row.get(2).cloned() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let current = {
                    let playlist = this.playlist.read(cx);
                    playlist
                        .current()
                        .and_then(|index| playlist.items().get(index).cloned())
                };
                if current.map(|path| path.to_string_lossy().to_string()) == Some(file.clone()) {
                    this.playback
                        .update(cx, |playback, cx| playback.seek_to(time, cx));
                    this.show_notice(crate::i18n::trf("跳转书签：{}", &[&format_clock(time)]), cx);
                    return;
                }
                this.resume_pending = Some(time);
                this.open_media(Path::new(&file), cx);
                this.show_notice(crate::i18n::trf("打开书签：{file}", &[&file]), cx);
            });
        })
        .detach();
    }

    fn prompt_menu_search(&mut self, cx: &mut Context<Self>) {
        let entry = cx
            .background_executor()
            .spawn(async { zenity_entry("简索菜单", "输入菜单项名称") });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(query) = entry.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.search_menu(&query, cx));
        })
        .detach();
    }

    fn search_menu(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let matches = menu::search_actions(&self.menu, &query);
        match matches.len() {
            0 => self.show_notice(crate::i18n::trf("没有找到菜单项：{query}", &[&query]), cx),
            1 => self.handle_action(matches[0].1.clone(), cx),
            _ => {
                let rows: Vec<Vec<String>> = matches
                    .iter()
                    .map(|(title, _)| vec![title.clone()])
                    .collect();
                let list = cx
                    .background_executor()
                    .spawn(async move { zenity_list("简索菜单", &["菜单项"], rows) });
                let this = cx.entity().downgrade();
                cx.spawn(async move |_this, cx| {
                    let Some(row) = list.await else {
                        return;
                    };
                    let Some(title) = row.first() else {
                        return;
                    };
                    let _ = this.update(cx, |this, cx| {
                        let found = menu::search_actions(&this.menu, &title.to_lowercase());
                        if let Some((_, action)) = found.first() {
                            let action = action.clone();
                            this.handle_action(action, cx);
                        } else {
                            this.show_notice("没有找到菜单项", cx);
                        }
                    });
                })
                .detach();
            }
        }
    }

    fn set_similar_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        self.config.similar_mode = mode.to_string();
        self.save_config();
        let label = match mode {
            "similar" => "同时打开相似文件",
            "all" => "同时打开全部文件",
            _ => "仅打开选定的文件",
        };
        self.show_notice(crate::i18n::trf("打开方式：{label}", &[&label]), cx);
    }

    fn set_language(&mut self, code: &str, cx: &mut Context<Self>) {
        crate::i18n::set_language(crate::i18n::Language::from_code(code));
        self.config.language = code.to_string();
        self.save_config();
        self.rebuild_menu(cx);
        let label = match code {
            "zh" => "简体中文",
            "en" => "English",
            _ => "自动选择",
        };
        self.show_notice(crate::i18n::trf("界面语言：{label}", &[&label]), cx);
    }

    fn set_post_action(&mut self, action: &str, cx: &mut Context<Self>) {
        const ORDER: [&str; 5] = [
            "none",
            "exit-current",
            "exit-all",
            "off-current",
            "off-all",
        ];
        let next = if action == "post-cycle" {
            let current = self.config.post_action.as_str();
            let index = ORDER
                .iter()
                .position(|value| *value == current)
                .unwrap_or(0);
            ORDER[(index + 1) % ORDER.len()]
        } else {
            action.trim_start_matches("post-")
        };
        self.config.post_action = next.to_string();
        self.save_config();
        self.show_notice(crate::i18n::trf("收尾方式：{}", &[&post_action_label(next)]), cx);
    }

    fn set_shutdown_mode(&mut self, action: &str, cx: &mut Context<Self>) {
        let (mode, label) = match action {
            "shutdown-suspend" => ("suspend", "待机"),
            "shutdown-hibernate" => ("hibernate", "休眠"),
            _ => ("poweroff", "关机"),
        };
        self.config.shutdown_mode = mode.to_string();
        self.save_config();
        self.show_notice(crate::i18n::trf("关机方式：{label}", &[&label]), cx);
    }

    fn post_action_at_eof(&mut self, has_next: bool, cx: &mut Context<Self>) -> bool {
        let action = self.config.post_action.clone();
        let trigger = match action.as_str() {
            "exit-current" | "off-current" => true,
            "exit-all" | "off-all" => !has_next,
            _ => false,
        };
        if !trigger {
            return false;
        }
        match action.as_str() {
            "exit-current" | "exit-all" => {
                self.quit(cx);
                true
            }
            _ => {
                self.request_shutdown(cx);
                true
            }
        }
    }

    fn request_shutdown(&mut self, cx: &mut Context<Self>) {
        let mode = self.config.shutdown_mode.clone();
        let this = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            let confirmed = cx.background_executor().spawn(async {
                zenity_question("播放已结束", "是否现在关机？")
            });
            if !confirmed.await {
                return;
            }
            let command_mode = mode.clone();
            let _ = this.update(cx, |this, cx| {
                match std::process::Command::new("systemctl")
                    .arg(&command_mode)
                    .spawn()
                {
                    Ok(_) => this.show_notice(crate::i18n::trf("已执行：systemctl {command_mode}", &[&command_mode]), cx),
                    Err(error) => this.show_notice(crate::i18n::trf("关机失败：{error}", &[&error]), cx),
                }
            });
        })
        .detach();
    }

    fn place_window(&mut self, kind: &str, cx: &mut Context<Self>) {
        let Some(window) = crate::x11::OwnWindow::find() else {
            self.show_notice("无法定位当前窗口", cx);
            return;
        };
        let (screen_width, screen_height) = window.screen_size();
        let Some((current_x, current_y, width, height)) = window.geometry() else {
            self.show_notice("无法读取窗口位置", cx);
            return;
        };
        let avaiable_w = screen_width as i32 - width as i32;
        let avaiable_h = screen_height as i32 - height as i32;
        log::debug!(
            "place_window {kind}: screen={screen_width}x{screen_height} window={current_x},{current_y} {width}x{height}"
        );
        let target = match kind {
            "top-left" => (0, 0),
            "top-right" => (avaiable_w, 0),
            "bottom-left" => (0, avaiable_h),
            "bottom-right" => (avaiable_w, avaiable_h),
            "center" => (avaiable_w / 2, avaiable_h / 2),
            _ => (
                self.config
                    .window_x
                    .map(|value| value as i32)
                    .unwrap_or(avaiable_w / 2),
                self.config
                    .window_y
                    .map(|value| value as i32)
                    .unwrap_or(avaiable_h / 2),
            ),
        };
        if kind != "restore" {
            self.config.window_x = Some(current_x as f32);
            self.config.window_y = Some(current_y as f32);
            self.save_config();
        }
        if window.move_to(target.0, target.1) {
            self.show_notice(crate::i18n::trf("窗口位置：{}, {}", &[&target.0, &target.1]), cx);
        } else {
            self.show_notice("窗口移动失败", cx);
        }
    }

    fn set_topmost_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        self.config.topmost_mode = mode.to_string();
        self.config.always_on_top = mode != "never";
        self.apply_topmost(cx);
    }

    fn apply_topmost(&mut self, cx: &mut Context<Self>) {
        let above = match self.config.topmost_mode.as_str() {
            "playing" => {
                let state = self.playback.read(cx).state();
                state.loaded && !state.paused
            }
            _ => self.config.always_on_top,
        };
        let applied =
            crate::x11::OwnWindow::find().is_some_and(|window| window.set_above(above));
        if applied {
            self.save_config();
            self.show_notice(
                format!("总在最前端：{}", if above { "开" } else { "关" }),
                cx,
            );
        } else {
            self.show_notice("当前窗口管理器不支持置顶", cx);
        }
    }

    fn prompt_open_file(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "打开媒体文件", false, true);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(path) = paths.first() {
                        this.add_to_playlist(paths.clone(), cx);
                        this.open_media(path, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn prompt_add_files(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "添加文件到播放列表", false, true);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                let _ = this.update(cx, |this, cx| this.add_to_playlist(paths, cx));
            }
        })
        .detach();
    }

    fn add_to_playlist(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let added = self
            .playlist
            .update(cx, |playlist, _| playlist.add_files(paths));
        self.sync_playlist(cx);
        if added == 0 {
            self.show_notice("文件已在播放列表中", cx);
        } else {
            self.show_notice(crate::i18n::trf("已添加 {added} 个文件到播放列表", &[&added]), cx);
        }
    }

    fn prompt_add_folder(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "添加文件夹到播放列表", true, false);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                let _ = this.update(cx, |this, cx| {
                    let Some(folder) = paths.first() else {
                        return;
                    };                    let added = this
                        .playlist
                        .update(cx, |playlist, _| playlist.add_folder(folder));
                    this.sync_playlist(cx);
                    if added == 0 {
                        this.show_notice("文件夹中没有可添加的媒体文件", cx);
                    } else {
                        this.show_notice(crate::i18n::trf("已添加 {added} 个文件到播放列表", &[&added]), cx);
                    }
                });
            }
        })
        .detach();
    }

    pub fn open_playlist_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(path) = self.playlist.update(cx, |playlist, _| playlist.select(index)) {
            self.open_media(&path, cx);
        }
    }

    fn step_playlist(&mut self, delta: isize, cx: &mut Context<Self>) {
        let wrap = self.repeat;
        let shuffle = self.shuffle;
        let path = self
            .playlist
            .update(cx, |playlist, _| playlist.step(delta, wrap, shuffle));
        match path {
            Some(path) => self.open_media(&path, cx),
            None => self.show_notice("播放列表中没有可切换的文件", cx),
        }
    }

    fn clear_playlist(&mut self, cx: &mut Context<Self>) {
        let removed = self.playlist.update(cx, |playlist, _| playlist.clear());
        self.sync_playlist(cx);
        if removed == 0 {
            self.show_notice("播放列表中没有可清空的项目", cx);
        } else {
            self.show_notice(crate::i18n::trf("已清空 {removed} 个播放列表项目（保留当前播放）", &[&removed]), cx);
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    pub fn is_sidebar_open(&self) -> bool {
        self.sidebar_open
    }

    fn sync_playlist(&mut self, cx: &mut Context<Self>) {
        let sources: Vec<String> = self
            .playlist
            .read(cx)
            .items()
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        if self.config.sources != sources {
            self.config.sources = sources;
            self.save_config();
        }
        cx.notify();
    }

    fn saved_position(&self, path: &Path) -> Option<f64> {
        if !self.config.remember_position || !path.is_file() {
            return None;
        }
        let key = file_key(path);
        self.config
            .resume
            .iter()
            .find(|(saved, _)| saved == &key)
            .map(|(_, position)| *position)
    }

    fn remember_current_position(&mut self, cx: &mut Context<Self>) {
        if !self.config.remember_position {
            return;
        }
        let (loaded, position, duration) = {
            let state = self.playback.read(cx).state();
            (state.loaded, state.position, state.duration)
        };
        if !loaded {
            return;
        }
        let path = {
            let playlist = self.playlist.read(cx);
            playlist
                .current()
                .and_then(|index| playlist.items().get(index).cloned())
        };
        let Some(path) = path else {
            return;
        };
        if !path.is_file() {
            return;
        }
        let key = file_key(&path);
        self.config.resume.retain(|(saved, _)| saved != &key);
        if position > 1.0 && (duration <= 0.0 || position < duration - 2.0) {
            self.config.resume.push((key, position));
        }
        self.save_config();
    }

    pub(super) fn save_config(&self) {
        if let Err(error) = self.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }

    pub fn menu(&self) -> &[MenuEntry] {
        &self.menu
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu_open
    }

    pub fn open_path(&self) -> &[usize] {
        &self.open_path
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn entries_at(&self, level: usize) -> &[MenuEntry] {
        let mut entries: &[MenuEntry] = &self.menu;
        for depth in 0..level {
            let Some(&index) = self.open_path.get(depth) else {
                return &[];
            };
            let Some(entry) = entries.get(index) else {
                return &[];
            };
            entries = &entry.children;
        }
        entries
    }

    fn set_aspect(&mut self, value: &str, cx: &mut Context<Self>) {
        self.playback
            .update(cx, |playback, cx| playback.set_aspect(value, cx));
    }

    fn set_crop(&mut self, action: &str, cx: &mut Context<Self>) {
        let (ratio, label) = match action {
            "zoom-crop-4-3" => (4.0 / 3.0, "4:3"),
            "zoom-crop-16-9" => (16.0 / 9.0, "16:9"),
            "zoom-crop-16-10" => (16.0 / 10.0, "16:10"),
            "zoom-crop-1-85" => (1.85, "1.85:1"),
            _ => (2.35, "2.35:1"),
        };
        self.playback
            .update(cx, |playback, cx| playback.set_crop(Some(ratio), cx));
        self.show_notice(crate::i18n::trf("裁剪：{label}", &[&label]), cx);
    }

    fn set_auto_rotate(&mut self, base: u16, cx: &mut Context<Self>) {
        self.auto_rotate = Some(base);
        self.auto_rotate_dims = (0, 0);
        let (width, height) = {
            let state = self.playback.read(cx).state();
            (state.width, state.height)
        };
        let target = if width > 0 && height > 0 && height > width {
            (base + 90) % 360
        } else {
            base
        };
        self.set_rotation(target, cx);
        self.show_notice(crate::i18n::trf("自动旋转：{base}°/{}°", &[&base, &((base + 90) % 360)]), cx);
    }

    fn current_media(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        let playlist = self.playlist.read(cx);
        playlist
            .current()
            .and_then(|index| playlist.items().get(index).cloned())
    }

    fn select_panel(&mut self, panel: u8, cx: &mut Context<Self>) {
        match panel {
            1 => {
                self.capture.update(cx, |capture, cx| capture.stop(cx));
                self.active_panel = 1;
                if !self.playback.read(cx).is_loaded() {
                    if let Some(path) = self.current_media(cx) {
                        self.resume_pending = self.saved_position(&path);
                        self.playback
                            .update(cx, |playback, cx| playback.open(&path, cx));
                        self.apply_image(cx);
                    }
                }
                self.show_notice("图像 1：本地播放", cx);
            }
            2 | 3 => {
                self.active_panel = panel;
                self.start_screen_capture(panel as u32 - 1, cx);
            }
            _ => self.show_notice("当前版本仅支持 3 个图像源", cx),
        }
    }

    fn cycle_panel(&mut self, cx: &mut Context<Self>) {
        let next = match self.active_panel {
            1 => 2,
            2 => 3,
            _ => 1,
        };
        self.select_panel(next, cx);
    }

    fn create_thumbnail(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current_media(cx) else {
            self.show_notice("没有可生成缩略图的媒体", cx);
            return;
        };
        let current_width = self.config.thumbnail_width.to_string();
        let entry = cx.background_executor().spawn(async move {
            zenity_entry("创建缩略图", &format!("缩略图宽度（当前 {current_width}）"))
        });
        let position = self.playback.read(cx).state().position;
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(value) = entry.await else {
                return;
            };
            let width = value
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|value| (16..=8192).contains(value));
            let _ = this.update(cx, |this, cx| {
                if let Some(width) = width {
                    this.config.thumbnail_width = width;
                    this.save_config();
                }
                let width = this.config.thumbnail_width.max(16);
                let mut directory = this.config.snapshot_dir.clone();
                if directory.as_os_str().is_empty() {
                    directory = std::env::temp_dir();
                }
                let _ = std::fs::create_dir_all(&directory);
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
                    .unwrap_or_else(|| "thumbnail".to_string());
                let output = directory.join(format!(
                    "{stem}_{}.png",
                    position.max(0.0) as u64
                ));
                let source = path.clone();
                let target = output.clone();
                cx.background_executor()
                    .spawn(async move {
                        std::process::Command::new("ffmpeg")
                            .args(["-y", "-ss", &format!("{position:.3}"), "-i"])
                            .arg(&source)
                            .args(["-frames:v", "1", "-vf", &format!("scale={width}:-1")])
                            .arg(&target)
                            .output()
                    })
                    .detach();
                this.show_notice(crate::i18n::trf("缩略图：{}", &[&output.display()]), cx);
            });
        })
        .detach();
    }

    fn prompt_config_manager(&mut self, cx: &mut Context<Self>) {
        let directory = flint_core::config::config_dir().join("profiles");
        let _ = std::fs::create_dir_all(&directory);
        let mut rows: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|value| value.to_str()) == Some("ini")).then(|| {
                    (
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path,
                    )
                })
            })
            .collect();
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        const SAVE_ROW: &str = "＋ 保存当前配置...";
        const DELETE_ROW: &str = "🗑 删除配置...";
        let mut cells: Vec<Vec<String>> = rows
            .iter()
            .map(|(name, _)| vec![name.clone()])
            .collect();
        cells.push(vec![SAVE_ROW.to_string()]);
        cells.push(vec![DELETE_ROW.to_string()]);
        let listing = cx
            .background_executor()
            .spawn(async move { zenity_list("配置管理", &["配置"], cells) });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(selected) = listing.await else {
                return;
            };
            let Some(name) = selected.first() else {
                return;
            };
            if name == SAVE_ROW {
                let _ = this.update(cx, |this, cx| this.prompt_save_profile(cx));
            } else if name == DELETE_ROW {
                let _ = this.update(cx, |this, cx| this.delete_profile(cx));
            } else if let Some((_, path)) = rows.iter().find(|(profile, _)| profile == name) {
                let path = path.clone();
                let config = Config::load_from(&path);
                let _ = this.update(cx, |this, cx| this.apply_config(config, cx));
            }
        })
        .detach();
    }

    fn prompt_save_profile(&mut self, cx: &mut Context<Self>) {
        let entry = cx
            .background_executor()
            .spawn(async { zenity_entry("保存配置", "输入配置名称") });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(name) = entry.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let directory = flint_core::config::config_dir().join("profiles");
                let _ = std::fs::create_dir_all(&directory);
                let safe: String = name
                    .chars()
                    .map(|value| {
                        if value.is_alphanumeric() || value == '-' || value == '_' {
                            value
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let path = directory.join(format!("{safe}.ini"));
                match this.config.save_to(&path) {
                    Ok(()) => this.show_notice(crate::i18n::trf("已保存配置：{name}", &[&name]), cx),
                    Err(error) => this.show_notice(crate::i18n::trf("保存配置失败：{error}", &[&error]), cx),
                }
            });
        })
        .detach();
    }

    fn delete_profile(&mut self, cx: &mut Context<Self>) {
        let directory = flint_core::config::config_dir().join("profiles");
        let names: Vec<String> = std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|value| value.to_str()) == Some("ini"))
                    .then(|| path.file_stem().map(|stem| stem.to_string_lossy().to_string()))
                    .flatten()
            })
            .collect();
        if names.is_empty() {
            self.show_notice("没有可删除的配置", cx);
            return;
        }
        let listing = cx.background_executor().spawn(async move {
            zenity_list(
                "删除配置",
                &["配置"],
                names.into_iter().map(|name| vec![name]).collect(),
            )
        });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(selected) = listing.await else {
                return;
            };
            let Some(name) = selected.first() else {
                return;
            };
            let path = flint_core::config::config_dir()
                .join("profiles")
                .join(format!("{name}.ini"));
            let result = std::fs::remove_file(&path);
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.show_notice(crate::i18n::trf("已删除配置：{name}", &[&name]), cx),
                Err(error) => this.show_notice(crate::i18n::trf("删除配置失败：{error}", &[&error]), cx),
            });
        })
        .detach();
    }

    fn apply_config(&mut self, config: Config, cx: &mut Context<Self>) {
        self.config = config;
        self.save_config();
        self.brightness = self.config.brightness;
        self.contrast = self.config.contrast;
        self.saturation = self.config.saturation;
        self.rotation = self.config.rotation;
        self.flip_horizontal = self.config.flip_horizontal;
        self.flip_vertical = self.config.flip_vertical;
        self.remember_position = self.config.remember_position;
        let stereo = StereoMode::from_key(&self.config.stereo_mode);
        let pano = self.config.pano_enabled.then(|| flint_media::panorama::View {
            yaw: self.config.pano_yaw.to_radians(),
            pitch: self.config.pano_pitch.to_radians(),
            fov: self.config.pano_fov.to_radians(),
        });
        crate::i18n::set_language(crate::i18n::Language::from_code(&self.config.language));
        self.playback.update(cx, |playback, cx| {
            playback.set_stereo(stereo, cx);
            playback.set_pano(pano, cx);
        });
        self.apply_image(cx);
        self.show_notice("已载入配置", cx);
    }

    fn prompt_window_size(&mut self, cx: &mut Context<Self>) {
        let (width, height) = (
            self.config.window_custom_width,
            self.config.window_custom_height,
        );
        let forms = cx.background_executor().spawn(async move {
            zenity_forms(
                "屏幕尺寸设置",
                &[
                    ("自定义宽度", &width.to_string()),
                    ("自定义高度", &height.to_string()),
                ],
            )
        });
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(values) = forms.await else {
                return;
            };
            let parse = |value: Option<&String>, fallback: u32| {
                value
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .filter(|value| (200..=8192).contains(value))
                    .unwrap_or(fallback)
            };
            let _ = this.update(cx, |this, cx| {
                this.config.window_custom_width = parse(values.first(), 1280);
                this.config.window_custom_height = parse(values.get(1), 720);
                this.save_config();
                this.show_notice(
                    format!(
                        "自定义尺寸：{}×{}",
                        this.config.window_custom_width, this.config.window_custom_height
                    ),
                    cx,
                );
            });
        })
        .detach();
    }

    fn toggle_filter(&mut self, kind: &str, cx: &mut Context<Self>) {
        let avisynth = kind == "avisynth";
        let (enabled, script) = if avisynth {
            self.config.avisynth_enabled = !self.config.avisynth_enabled;
            (
                self.config.avisynth_enabled,
                self.config.avisynth_script.clone(),
            )
        } else {
            self.config.vapoursynth_enabled = !self.config.vapoursynth_enabled;
            (
                self.config.vapoursynth_enabled,
                self.config.vapoursynth_script.clone(),
            )
        };
        self.save_config();
        let name = if avisynth { "AviSynth" } else { "VapourSynth" };
        if !enabled {
            let _ = self
                .playback
                .update(cx, |playback, _| playback.try_command("vf clr"));
            self.show_notice(crate::i18n::trf("{name} 滤镜：关", &[&name]), cx);
            return;
        }
        if script.is_empty() {
            self.show_notice(crate::i18n::trf("请先选择 {name} 脚本", &[&name]), cx);
            return;
        }
        if avisynth {
            self.show_notice("当前 libmpv 不支持 AviSynth，脚本已记录", cx);
            return;
        }
        let command = format!("vf set vapoursynth={script}");
        let result = self
            .playback
            .update(cx, |playback, _| playback.try_command(&command));
        match result {
            Ok(()) => self.show_notice(crate::i18n::trf("{name} 滤镜：开", &[&name]), cx),
            Err(error) => self.show_notice(crate::i18n::trf("{name} 滤镜加载失败：{error}", &[&name, &error]), cx),
        }
    }

    fn choose_filter_script(&mut self, kind: &str, cx: &mut Context<Self>) {
        let title = if kind == "avisynth" {
            "选择 AviSynth 脚本"
        } else {
            "选择 VapourSynth 脚本"
        };
        let paths = self.pick_paths(cx, title, false, false);
        let kind = kind.to_string();
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let Some(paths) = paths.await else {
                return;
            };
            let Some(path) = paths.first() else {
                return;
            };
            let value = path.to_string_lossy().to_string();
            let _ = this.update(cx, |this, cx| {
                if kind == "avisynth" {
                    this.config.avisynth_script = value;
                } else {
                    this.config.vapoursynth_script = value;
                }
                this.save_config();
                this.show_notice(crate::i18n::trf("脚本：{}", &[&path.display()]), cx);
            });
        })
        .detach();
    }

    fn open_filter_folder(&mut self, kind: &str, cx: &mut Context<Self>) {
        let script = if kind == "avisynth" {
            &self.config.avisynth_script
        } else {
            &self.config.vapoursynth_script
        };
        let directory = Path::new(script)
            .parent()
            .filter(|parent| parent.is_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(flint_core::config::config_dir);
        self.open_directory(directory, cx);
    }

    pub fn rebuild_menu(&mut self, cx: &mut Context<Self>) {
        let data = {
            let playback = self.playback.read(cx);
            let playlist = self.playlist.read(cx);
            MenuData {
                chapters: playback.chapters(),
                tracks: playback.tracks(),
                devices: flint_core::device::scan_devices(),
                speed: playback.speed(),
                repeat: self.repeat,
                shuffle: self.shuffle,
                remember_position: self.remember_position,
                has_prev: playlist.has_prev(self.shuffle),
                has_next: playlist.has_next(),
            }
        };
        self.menu = menu::main_menu();
        apply_dynamic(&mut self.menu, &data);
        menu::set_checked_all(&mut self.menu, "toggle-sidebar", self.sidebar_open);
        let aspect = self.playback.read(cx).aspect().to_string();
        for (action, value) in [
            ("aspect-fit", "no"),
            ("zoom-none", "no"),
            ("aspect-stretch", "0"),
            ("zoom-stretch-4-3", "4:3"),
            ("zoom-stretch-16-9", "16:9"),
            ("zoom-stretch-16-10", "16:10"),
            ("zoom-stretch-1-85", "1.85:1"),
            ("zoom-stretch-2-35", "2.35:1"),
        ] {
            menu::set_checked_all(&mut self.menu, action, aspect == value);
        }
        menu::set_checked_all(&mut self.menu, "osd", self.osd_mode == Some(OsdMode::Full));
        menu::set_checked_all(
            &mut self.menu,
            "osd-brief",
            self.osd_mode == Some(OsdMode::Brief),
        );
        menu::set_checked_all(
            &mut self.menu,
            "osd-playback",
            self.osd_mode == Some(OsdMode::Playback),
        );
        menu::set_checked_all(&mut self.menu, "flip-horizontal", self.flip_horizontal);
        menu::set_checked_all(&mut self.menu, "flip-vertical", self.flip_vertical);
        menu::set_checked_all(&mut self.menu, "snapshot-play-folder-toggle", self.config.snapshot_play_folder);
        let stereo = self.playback.read(cx).stereo().key().to_string();
        for (action, value) in [
            ("stereo-off", "off"),
            ("stereo-anaglyph-sbs", "anaglyph-sbs"),
            ("stereo-anaglyph-tab", "anaglyph-tab"),
            ("stereo-left-sbs", "left-sbs"),
            ("stereo-right-sbs", "right-sbs"),
            ("stereo-left-tab", "left-tab"),
            ("stereo-right-tab", "right-tab"),
        ] {
            menu::set_checked_all(&mut self.menu, action, stereo == value);
        }
        for (action, value) in [
            ("renderer-auto", "auto"),
            ("renderer-opengl", "opengl"),
            ("renderer-null-compressed", "null-compressed"),
            ("renderer-null-uncompressed", "null-uncompressed"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.video_renderer == value);
        }
        for (action, value) in [
            ("language-auto", "auto"),
            ("language-zh", "zh"),
            ("language-en", "en"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.language == value);
        }
        for (action, value) in [
            ("similar-selected", "selected"),
            ("similar-similar", "similar"),
            ("similar-all", "all"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.similar_mode == value);
        }
        for (action, value) in [
            ("topmost-always", "always"),
            ("topmost-playing", "playing"),
            ("topmost-fullscreen", "fullscreen"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.topmost_mode == value);
        }
        for (action, value) in [
            ("post-exit-current", "exit-current"),
            ("post-exit-all", "exit-all"),
            ("post-off-current", "off-current"),
            ("post-off-all", "off-all"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.post_action == value);
        }
        for (action, value) in [
            ("shutdown-default", "poweroff"),
            ("shutdown-suspend", "suspend"),
            ("shutdown-hibernate", "hibernate"),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.config.shutdown_mode == value);
        }
        for (action, value) in [
            ("ignore-shortcuts-toggle", self.config.ignore_shortcuts),
            ("favorites-memory-toggle", self.config.favorites_memory_points),
            ("touch-toggle", self.config.touch_enabled),
            ("move-titlebar-toggle", self.config.move_only_titlebar),
            ("remember-post-toggle", self.config.remember_post_action),
            ("frame-remember-toggle", self.config.remember_frame),
            ("margin-auto-toggle", self.margin_auto),
            ("margin-force-toggle", self.margin_force),
            ("video-record-play-folder", self.config.video_record_play_folder),
            ("avisynth-toggle", self.config.avisynth_enabled),
            ("vapoursynth-toggle", self.config.vapoursynth_enabled),
        ] {
            menu::set_checked_all(&mut self.menu, action, value);
        }
        for (action, value) in [
            ("panel-1", 1),
            ("panel-2", 2),
            ("panel-3", 3),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.active_panel == value);
        }
        for (action, value) in [
            ("rotate-0", 0),
            ("rotate-90", 90),
            ("rotate-180", 180),
            ("rotate-270", 270),
        ] {
            menu::set_checked_all(
                &mut self.menu,
                action,
                self.auto_rotate.is_none() && self.rotation == value,
            );
        }
        for (action, value) in [("rotate-auto-0-90", 0), ("rotate-auto-180-270", 180)] {
            menu::set_checked_all(&mut self.menu, action, self.auto_rotate == Some(value));
        }
        let crop = self.playback.read(cx).crop_ratio();
        for (action, value) in [
            ("zoom-crop-4-3", 4.0 / 3.0),
            ("zoom-crop-16-9", 16.0 / 9.0),
            ("zoom-crop-16-10", 16.0 / 10.0),
            ("zoom-crop-1-85", 1.85),
            ("zoom-crop-2-35", 2.35),
        ] {
            menu::set_checked_all(
                &mut self.menu,
                action,
                crop.is_some_and(|ratio| (ratio - value).abs() < 0.01),
            );
        }
        for (action, value) in [
            ("margin-0", 0),
            ("margin-5", 5),
            ("margin-10", 10),
            ("margin-15", 15),
            ("margin-20", 20),
            ("margin-25", 25),
            ("margin-30", 30),
        ] {
            menu::set_checked_all(&mut self.menu, action, self.margin == value);
        }
        menu::translate_titles(&mut self.menu);
    }

    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {        self.menu_open = !self.menu_open;
        self.open_path.clear();
        if self.menu_open {
            self.rebuild_menu(cx);
        }
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_open = false;
        self.open_path.clear();
        cx.notify();
    }

    pub fn hover_row(&mut self, level: usize, index: usize, has_children: bool, cx: &mut Context<Self>) {
        if !self.menu_open {
            return;
        }
        self.open_path.truncate(level);
        if has_children {
            self.open_path.push(index);
        }
        cx.notify();
    }

    pub fn activate(&mut self, action: Option<&str>, cx: &mut Context<Self>) -> Option<FrameAction> {
        self.menu_open = false;
        self.open_path.clear();
        cx.notify();
        let action = action?;
        Some(match action {
            "exit" => FrameAction::Exit,
            "fullscreen" => FrameAction::ToggleFullscreen,
            _ => FrameAction::Action(action.to_string()),
        })
    }

    pub fn quit(&mut self, cx: &mut Context<Self>) {
        self.remember_current_position(cx);
        cx.quit();
    }

    pub fn show_notice(&mut self, notice: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
    }

    pub fn osd_mode(&self) -> Option<OsdMode> {
        self.osd_mode
    }

    pub fn cpu_percent(&self) -> f64 {
        self.cpu.percent
    }

    pub fn image_summary(&self) -> String {
        format!(
            "亮度 {} 对比度 {} 饱和度 {} · 旋转 {}°",
            self.brightness, self.contrast, self.saturation, self.rotation
        )
    }

    fn apply_image(&mut self, cx: &mut Context<Self>) {
        let changed = self.config.brightness != self.brightness
            || self.config.contrast != self.contrast
            || self.config.saturation != self.saturation
            || self.config.rotation != self.rotation
            || self.config.flip_horizontal != self.flip_horizontal
            || self.config.flip_vertical != self.flip_vertical;
        self.config.brightness = self.brightness;
        self.config.contrast = self.contrast;
        self.config.saturation = self.saturation;
        self.config.rotation = self.rotation;
        self.config.flip_horizontal = self.flip_horizontal;
        self.config.flip_vertical = self.flip_vertical;
        if changed {
            self.save_config();
        }

        let mut filters: Vec<&str> = Vec::new();
        if self.flip_horizontal {
            filters.push("hflip");
        }
        if self.flip_vertical {
            filters.push("vflip");
        }
        let filter_command = if filters.is_empty() {
            "vf clr".to_string()
        } else {
            format!("vf set {}", filters.join(","))
        };
        let values = [
            ("brightness", self.brightness.to_string()),
            ("contrast", self.contrast.to_string()),
            ("saturation", self.saturation.to_string()),
            ("video-rotate", self.rotation.to_string()),
            ("video-zoom", format!("{:.2}", self.zoom)),
            ("video-pan-x", format!("{:.2}", self.pan_x)),
            ("video-pan-y", format!("{:.2}", self.pan_y)),
        ];
        self.playback.update(cx, |playback, cx| {
            for (name, value) in &values {
                playback.set_property(name, value, cx);
            }
            playback.command(&filter_command, cx);
        });
        cx.notify();
    }

    fn set_rotation(&mut self, rotation: u16, cx: &mut Context<Self>) {
        self.rotation = rotation % 360;
        self.apply_image(cx);
    }

    pub(crate) fn toggle_osd_shortcut(&mut self, mode: OsdMode, cx: &mut Context<Self>) {
        if self.settings.read(cx).is_open() {
            return;
        }
        self.toggle_osd(mode, cx);
    }

    fn toggle_osd(&mut self, mode: OsdMode, cx: &mut Context<Self>) {
        self.osd_mode = if self.osd_mode == Some(mode) {
            None
        } else {
            Some(mode)
        };
        self.config.osd_visible = self.osd_mode.is_some();
        self.save_config();
        cx.notify();
    }

    fn take_snapshot(&mut self, cx: &mut Context<Self>) {
        if !self.playback.read(cx).is_loaded() {
            self.show_notice("当前没有可截取的画面", cx);
            return;
        }
        let position = self.playback.read(cx).state().position;
        let mut directory = self.config.snapshot_dir.clone();
        if self.config.snapshot_play_folder {
            let current = {
                let playlist = self.playlist.read(cx);
                playlist
                    .current()
                    .and_then(|index| playlist.items().get(index).cloned())
            };
            if let Some(parent) = current.as_deref().and_then(Path::parent) {
                directory = parent.to_path_buf();
            }
        }
        if let Err(error) = std::fs::create_dir_all(&directory) {
            self.show_notice(crate::i18n::trf("创建截图目录失败：{error}", &[&error]), cx);
            return;
        }
        let extension = match self.config.snapshot_format.as_str() {
            "jpeg" => "jpg",
            "bmp" => "bmp",
            _ => "png",
        };
        let stem = if self.config.snapshot_name == "playback" {
            let total = position.max(0.0) as u64;
            format!(
                "flintplayer_{:02}-{:02}-{:02}",
                total / 3600,
                total / 60 % 60,
                total % 60
            )
        } else {
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0);
            format!("flintplayer_{millis}")
        };
        let path = directory.join(format!("{stem}.{extension}"));
        let quoted = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let command = format!("screenshot-to-file \"{quoted}\" video");
        self.playback
            .update(cx, |playback, cx| playback.command(&command, cx));
        self.last_snapshot = Some(path.clone());
        self.show_notice(crate::i18n::trf("已截取：{}", &[&path.display()]), cx);
    }

    fn set_snapshot_format(&mut self, format: &str, cx: &mut Context<Self>) {
        self.config.snapshot_format = format.to_string();
        self.save_config();
        self.show_notice(crate::i18n::trf("截图格式：{format}", &[&format]), cx);
    }

    fn set_snapshot_name(&mut self, name: &str, cx: &mut Context<Self>) {
        self.config.snapshot_name = name.to_string();
        self.save_config();
        let label = if name == "playback" {
            "以播放时间命名"
        } else {
            "以当前时间命名"
        };
        self.show_notice(crate::i18n::trf("截图命名：{label}", &[&label]), cx);
    }

    fn prompt_snapshot_folder(&mut self, cx: &mut Context<Self>) {
        let paths = self.pick_paths(cx, "选择截图存档路径", true, false);
        let this = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Some(paths) = paths.await {
                if let Some(folder) = paths.first() {
                    let _ = this.update(cx, |this, cx| {
                        this.config.snapshot_dir = folder.clone();
                        this.save_config();
                        this.show_notice(crate::i18n::trf("截图存档路径：{}", &[&folder.display()]), cx);
                    });
                }
            }
        })
        .detach();
    }

    fn delete_last_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.last_snapshot.clone() else {
            self.show_notice("没有可删除的截图", cx);
            return;
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.last_snapshot = None;
                self.show_notice(crate::i18n::trf("已删除：{}", &[&path.display()]), cx);
            }
            Err(error) => self.show_notice(crate::i18n::trf("删除失败：{error}", &[&error]), cx),
        }
    }

    fn reopen(&mut self, cx: &mut Context<Self>) {
        let current = {
            let playlist = self.playlist.read(cx);
            playlist
                .current()
                .and_then(|index| playlist.items().get(index).cloned())
        };
        match current {
            Some(path) => self.open_media(&path, cx),
            None => self.show_notice("没有可重新打开的媒体", cx),
        }
    }
}

fn file_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn parse_shortcut(shortcut: &str) -> Option<(bool, bool, bool, String)> {
    let normalized = shortcut
        .replace("数字键+", "numplus")
        .replace("数字键-", "numminus")
        .replace("数字键", "");
    let parts: Vec<&str> = normalized
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let (last, modifiers) = parts.split_last()?;
    let (mut control, mut alt, mut shift) = (false, false, false);
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => control = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    let key = match last.to_ascii_lowercase().as_str() {
        "numplus" => "add".to_string(),
        "numminus" => "subtract".to_string(),
        other => other.to_string(),
    };
    Some((control, alt, shift, key))
}

fn zenity_available() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join("zenity").is_file())
    })
}

fn zenity_pick(prompt: &str, directories: bool, multiple: bool) -> Option<Vec<std::path::PathBuf>> {
    let mut command = std::process::Command::new("zenity");
    command
        .arg("--file-selection")
        .arg("--title")
        .arg(prompt);
    if directories {
        command.arg("--directory");
    }
    if multiple {
        command.arg("--multiple").arg("--separator").arg("\n");
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<std::path::PathBuf> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

fn zenity_entry(title: &str, text: &str) -> Option<String> {
    let output = std::process::Command::new("zenity")
        .args(["--entry", "--title", title, "--text", text])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn zenity_forms(title: &str, fields: &[(&str, &str)]) -> Option<Vec<String>> {
    let mut command = std::process::Command::new("zenity");
    command.args(["--forms", "--title", title]);
    for (label, value) in fields {
        if value.is_empty() {
            command.arg(format!("--add-entry={label}"));
        } else {
            command.arg(format!("--add-entry={label} ({value})"));
        }
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let values: Vec<String> = line.trim().split('|').map(str::to_string).collect();
    (!values.is_empty()).then_some(values)
}

fn zenity_list(title: &str, columns: &[&str], rows: Vec<Vec<String>>) -> Option<Vec<String>> {
    let mut command = std::process::Command::new("zenity");
    command.args(["--list", "--title", title]);
    for column in columns {
        command.arg(format!("--column={column}"));
    }
    for row in &rows {
        for cell in row {
            command.arg(cell);
        }
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| text.split('|').map(str::to_string).collect())
}

fn zenity_radio_list(
    title: &str,
    text: &str,
    columns: &[&str],
    rows: Vec<Vec<String>>,
    checked: usize,
    height: u32,
) -> Option<Vec<String>> {
    let mut command = std::process::Command::new("zenity");
    command.args(["--list", "--radiolist", "--hide-header", "--title", title, "--text", text]);
    command.arg(format!("--height={height}"));
    command.arg("--column=选择");
    for column in columns {
        command.arg(format!("--column={column}"));
    }
    for (index, row) in rows.iter().enumerate() {
        command.arg(if index == checked { "TRUE" } else { "FALSE" });
        for cell in row {
            command.arg(cell);
        }
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| text.split('|').map(str::to_string).collect())
}

fn zenity_question(title: &str, text: &str) -> bool {
    std::process::Command::new("zenity")
        .args(["--question", "--title", title, "--text", text])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn post_action_label(action: &str) -> &'static str {
    match action {
        "exit-current" => "播放完当前后退出",
        "exit-all" => "播放完全部后退出",
        "off-current" => "播放完当前后关机",
        "off-all" => "播放完全部后关机",
        _ => "不做任何操作",
    }
}

fn format_clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_string();
    }
    let total = seconds as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

fn parse_clock(text: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in text.split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(total)
}
