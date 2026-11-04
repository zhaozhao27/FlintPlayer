use crate::keymap::{action_for, KeyAction};
use crate::skin::{self, Skin};
use crate::{
    BrowserItem, BookmarkRow, DeviceEntry, FileEntry, MainWindow, MenuLeaf, MenuSub, OsdToken,
    PanelInfo, ResolutionEntry, SearchRow, SkinRow, Translation, UiOptions,
};
use crossbeam_channel::{Receiver, Sender};
use flint_core::config::{config_dir, config_path, Config};
use flint_core::device::{self, DeviceInfo, FormatInfo, Resolution};
use flint_core::sysinfo;
use flint_media::effects::{self, Adjust};
use flint_media::embedded_subtitles::{self, SubtitleStreamInfo};
use flint_media::panorama;
use flint_media::stereo::{self, StereoMode};
use flint_media::subtitles::{CueStyle, SubtitleTrack};
use flint_media::{CaptureEngine, CaptureSettings, CaptureSmoothing, ChapterInfo, Frame, Shared};
use parking_lot::Mutex;
use slint::{
    ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, Timer, TimerMode, VecModel, Weak,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHROME_HIDE_DELAY: Duration = Duration::from_secs(2);
const STATS_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const NOTICE_DURATION: Duration = Duration::from_secs(4);
const MAX_PANELS: usize = 64;
const TONE_GREEN: i32 = 0;
const TONE_RED: i32 = 1;
const TONE_MAGENTA: i32 = 2;
const TONE_CYAN: i32 = 3;
const TONE_WHITE: i32 = 5;

const SUBTITLE_FONTS: &[(&str, &str)] = &[
    ("系统默认", ""),
    ("思源黑体", "Noto Sans CJK SC"),
    ("思源宋体", "Noto Serif CJK SC"),
    ("等宽", "Noto Sans Mono CJK SC"),
];

type SharedEngine = Arc<Mutex<CaptureEngine>>;

#[derive(Clone, Debug, Default)]
pub struct StartOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub fourcc: Option<String>,
    pub input_format: Option<String>,
    pub pixel_format: Option<String>,
}

struct ScanResult {
    devices: Vec<DeviceInfo>,
    resolutions: Vec<Vec<Resolution>>,
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

    fn sample(&mut self) -> f64 {
        const INTERVAL: Duration = Duration::from_secs(1);
        let Some(ticks) = sysinfo::process_cpu_ticks() else {
            return self.percent;
        };
        let now = Instant::now();
        let Some((previous_ticks, previous_at)) = self.last else {
            self.last = Some((ticks, now));
            return self.percent;
        };
        let seconds = now.saturating_duration_since(previous_at).as_secs_f64();
        if seconds < INTERVAL.as_secs_f64() {
            return self.percent;
        }
        let delta = ticks.saturating_sub(previous_ticks) as f64;
        self.percent = delta / sysinfo::ticks_per_second() as f64 / seconds * 100.0;
        self.last = Some((ticks, now));
        self.percent
    }
}

#[derive(Clone)]
struct PanelStats {
    fps: String,
}

impl Default for PanelStats {
    fn default() -> Self {
        Self {
            fps: "0.00 -> 30.00 fps".into(),
        }
    }
}

struct PanelState {
    engine: SharedEngine,
    shared: Arc<Shared>,
    commands: Arc<AtomicU64>,
    source: String,
    title: String,
    settings: CaptureSettings,
    preferred_width: u32,
    preferred_height: u32,
    started_once: Arc<AtomicBool>,
    last_frame: Option<Arc<Frame>>,
    image: Image,
    last_serial: u64,
    frozen: bool,
    paused: bool,
    recording: bool,
    stats: PanelStats,
    last_error: Option<String>,
    running: bool,
    last_stats: Instant,
    subtitle: Option<SubtitleTrack>,
    subtitle2: Option<SubtitleTrack>,
    subtitle_delay: f64,
    subtitle_streams: Vec<SubtitleStreamInfo>,
    subtitle_stream: Option<usize>,
    bookmarks: Vec<f64>,
    display_buf: Vec<u8>,
    pano_buf: Vec<u8>,
    pano_map: Option<panorama::Map>,
    adjust_buf: Vec<u8>,
    flip_buf: Vec<u8>,
    rotate_buf: Vec<u8>,
}

impl PanelState {
    fn new(source: String, title: String, settings: CaptureSettings) -> Self {
        let engine = Arc::new(Mutex::new(CaptureEngine::new()));
        let shared = engine.lock().shared();
        let preferred_width = settings.width;
        let preferred_height = settings.height;
        Self {
            engine,
            shared,
            commands: Arc::new(AtomicU64::new(0)),
            source,
            title,
            settings,
            preferred_width,
            preferred_height,
            started_once: Arc::new(AtomicBool::new(false)),
            last_frame: None,
            image: Image::default(),
            last_serial: 0,
            frozen: false,
            paused: false,
            recording: false,
            stats: PanelStats::default(),
            last_error: None,
            running: false,
            last_stats: Instant::now(),
            subtitle: None,
            subtitle2: None,
            subtitle_delay: 0.0,
            subtitle_streams: Vec::new(),
            subtitle_stream: None,
            bookmarks: Vec::new(),
            display_buf: Vec::new(),
            pano_buf: Vec::new(),
            pano_map: None,
            adjust_buf: Vec::new(),
            flip_buf: Vec::new(),
            rotate_buf: Vec::new(),
        }
    }

    fn row(&self, active: bool) -> PanelInfo {
        PanelInfo {
            frame: self.image.clone(),
            title: self.title.as_str().into(),
            fps: self.stats.fps.as_str().into(),
            running: self.running,
            frozen: self.frozen,
            active,
            error: self.last_error.clone().unwrap_or_default().as_str().into(),
        }
    }
}

struct Notice {
    message: String,
    until: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelTarget {
    Existing(usize),
    New,
    Replace(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathDialogTarget {
    Snapshot,
    VideoRecord,
    AudioRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordFolder {
    Video,
    Audio,
}

struct AppState {
    config: Config,
    panels: Vec<PanelState>,
    active: Option<usize>,
    devices: Vec<DeviceInfo>,
    resolutions: Vec<Resolution>,
    formats: Vec<FormatInfo>,
    device_resolutions: Vec<Vec<Resolution>>,
    selected_resolution: Option<usize>,
    selected_format: Option<usize>,
    settings_format_index: Option<usize>,
    format_overrides: HashMap<String, String>,
    panel_model: Rc<VecModel<PanelInfo>>,
    notice: Option<Notice>,
    osd_visible: bool,
    osd_mode: String,
    pending_seek: Option<(String, f64)>,
    fullscreen: bool,
    stretch: bool,
    zoom_ratio: i32,
    zoom_crop: bool,
    zoom_shift_x: i32,
    zoom_shift_y: i32,
    frame_scale_x: i32,
    frame_scale_y: i32,
    remember_frame: bool,
    margin_bottom: i32,
    margin_auto: bool,
    margin_force: bool,
    presenter: Option<Rc<Timer>>,
    burst_timer: Option<Rc<Timer>>,
    burst_remaining: i32,
    burst_total: i32,
    scan_tx: Sender<ScanResult>,
    scan_rx: Receiver<ScanResult>,
    scanning: bool,
    initial_scan: bool,
    cpu: CpuMeter,
    osd_at: Instant,
    osd_active: Option<usize>,
    notice_tx: Sender<String>,
    notice_rx: Receiver<String>,
    start: StartOptions,
    file_dir: PathBuf,
    file_paths: Vec<PathBuf>,
    file_last_pick: Option<(usize, Instant)>,
    repeat: bool,
    shuffle: bool,
    ab_start: Option<f64>,
    ab_end: Option<f64>,
    append_files: bool,
    subtitle_pick: bool,
    subtitle2_pick: bool,
    audio_pick: bool,
    menu_search_all: Vec<(String, String)>,
    menu_search_commands: Vec<String>,
    logo_pick: u8,
    script_pick: u8,
    chapter_source: Option<Arc<Vec<ChapterInfo>>>,
    subtitle_menu_key: String,
    skin_menu_key: String,
    translate_pending: String,
    translation: Arc<Mutex<Option<(String, String)>>>,
    audio_menu_key: String,
    audio_auto_key: String,
    filter_menu_key: String,
    sound_menu_base: Vec<MenuLeaf>,
    stereo_mode: StereoMode,
    pano_enabled: bool,
    pano_view: panorama::View,
    pano_dirty: bool,
    tts_enabled: bool,
    tts_spoken: String,
    adjust: effects::Adjust,
    rotation: u16,
    flip_horizontal: bool,
    flip_vertical: bool,
    video_dirty: bool,
    audio_channel: flint_media::audio::Channel,
    audio_swap: bool,
    audio_output_rate: u32,
    audio_output_format: u32,
    audio_device: Option<String>,
    audio_device2: Option<String>,
    volume_control: String,
    audio_sinks: Vec<AudioOutput>,
    audio_sink_rx: Receiver<Vec<AudioOutput>>,
    path_dialog_target: PathDialogTarget,
    last_snapshot: Option<PathBuf>,
    skin_opacity_backup: f32,
    skin_library: Vec<Skin>,
    skin_backup: String,
    skin_pick: bool,
    window_saved_pos: Option<(i32, i32)>,
}

impl AppState {
    fn new(config: Config, panel_model: Rc<VecModel<PanelInfo>>, start: StartOptions) -> Self {
        let mut config = config;
        if !config.remember_post_action {
            config.post_action = "none".into();
        }
        let (scan_tx, scan_rx) = crossbeam_channel::unbounded();
        let (notice_tx, notice_rx) = crossbeam_channel::unbounded();
        let (audio_sink_tx, audio_sink_rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || loop {
            if audio_sink_tx.send(pulse_sinks()).is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        });
        let panels: Vec<PanelState> = config_sources(&config)
            .into_iter()
            .map(|source| {
                let settings = capture_settings_for(&config, &source);
                PanelState::new(source.clone(), panel_title(&source), settings)
            })
            .collect();
        let active = (!panels.is_empty()).then_some(0);
        let stereo_mode = StereoMode::from_key(&config.stereo_mode);
        let pano_enabled = config.pano_enabled;
        let tts_enabled = config.tts_enabled;
        let adjust = effects::Adjust {
            brightness: config.brightness,
            contrast: config.contrast,
            saturation: config.saturation,
        };
        let rotation = config.rotation;
        let flip_horizontal = config.flip_horizontal;
        let flip_vertical = config.flip_vertical;
        let audio_device = Some(config.audio_device.clone()).filter(|name| !name.is_empty());
        let audio_device2 = Some(config.audio_device2.clone()).filter(|name| !name.is_empty());
        let audio_output_rate = config.audio_output_rate;
        let audio_output_format = config.audio_output_format;
        let volume_control = config.volume_control.clone();
        let remember_frame = config.remember_frame;
        let (frame_scale_x, frame_scale_y, zoom_shift_x, zoom_shift_y) = if remember_frame {
            (
                config.frame_scale_x,
                config.frame_scale_y,
                config.frame_shift_x,
                config.frame_shift_y,
            )
        } else {
            (100, 100, 0, 0)
        };
        let pano_view = panorama::View {
            yaw: config.pano_yaw.to_radians(),
            pitch: config.pano_pitch.to_radians(),
            fov: config.pano_fov.to_radians(),
        }
        .clamped();
        let mut state = Self {
            panels,
            active,
            config,
            devices: Vec::new(),
            resolutions: Vec::new(),
            formats: Vec::new(),
            device_resolutions: Vec::new(),
            selected_resolution: None,
            selected_format: None,
            settings_format_index: None,
            format_overrides: HashMap::new(),
            panel_model,
            notice: None,
            osd_visible: false,
            osd_mode: "off".into(),
            pending_seek: None,
            fullscreen: false,
            stretch: false,
            zoom_ratio: 0,
            zoom_crop: false,
            zoom_shift_x,
            zoom_shift_y,
            frame_scale_x,
            frame_scale_y,
            remember_frame,
            margin_bottom: 0,
            margin_auto: false,
            margin_force: false,
            presenter: None,
            burst_timer: None,
            burst_remaining: 0,
            burst_total: 0,
            scan_tx,
            scan_rx,
            scanning: false,
            initial_scan: true,
            cpu: CpuMeter::new(),
            osd_at: Instant::now(),
            osd_active: None,
            notice_tx,
            notice_rx,
            start,
            file_dir: PathBuf::new(),
            file_paths: Vec::new(),
            file_last_pick: None,
            repeat: false,
            shuffle: false,
            ab_start: None,
            ab_end: None,
            append_files: false,
            subtitle_pick: false,
            subtitle2_pick: false,
            audio_pick: false,
            menu_search_all: Vec::new(),
            menu_search_commands: Vec::new(),
            logo_pick: 0,
            script_pick: 0,
            chapter_source: None,
            subtitle_menu_key: String::new(),
            skin_menu_key: String::new(),
            translate_pending: String::new(),
            translation: Arc::new(Mutex::new(None)),
            audio_menu_key: String::new(),
            audio_auto_key: String::new(),
            filter_menu_key: String::new(),
            sound_menu_base: Vec::new(),
            stereo_mode,
            pano_enabled,
            pano_view,
            pano_dirty: false,
            tts_enabled,
            tts_spoken: String::new(),
            adjust,
            rotation,
            flip_horizontal,
            flip_vertical,
            video_dirty: false,
            audio_channel: flint_media::audio::Channel::Stereo,
            audio_swap: false,
            audio_output_rate,
            audio_output_format,
            audio_device,
            audio_device2,
            volume_control,
            audio_sinks: Vec::new(),
            audio_sink_rx,
            path_dialog_target: PathDialogTarget::Snapshot,
            last_snapshot: None,
            skin_opacity_backup: 1.0,
            skin_library: skin::load_library(),
            skin_backup: String::new(),
            skin_pick: false,
            window_saved_pos: None,
        };
        state.osd_visible = state.config.osd_visible;
        if state.osd_visible {
            state.osd_mode = "full".into();
        }
        state.sync_panel_model();
        state
    }

    fn sync_panel_model(&self) {
        let active = self.active;
        let rows: Vec<PanelInfo> = self
            .panels
            .iter()
            .enumerate()
            .map(|(index, panel)| panel.row(active == Some(index)))
            .collect();
        self.panel_model.set_vec(rows);
    }

    fn refresh_menu(&mut self) {
        let (source, width, height) = match self.active.and_then(|index| self.panels.get(index)) {
            Some(panel) => (
                Some(panel.source.clone()),
                panel.preferred_width,
                panel.preferred_height,
            ),
            None => (None, self.config.width, self.config.height),
        };
        let device_index = source.and_then(|source| {
            self.devices
                .iter()
                .position(|device| device.path.as_path() == Path::new(&source))
        });
        self.resolutions = device_index
            .and_then(|index| self.device_resolutions.get(index).cloned())
            .unwrap_or_default();
        self.selected_resolution = closest_resolution_index(&self.resolutions, width, height);
        self.formats = device_index
            .and_then(|index| self.devices.get(index))
            .map(|device| device.formats.clone())
            .unwrap_or_default();
        let fourcc = self
            .active
            .and_then(|index| self.panels.get(index))
            .and_then(|panel| panel.settings.fourcc.clone())
            .or_else(|| device_index.and_then(|index| preferred_fourcc(&self.devices[index])));
        self.selected_format = fourcc.and_then(|fourcc| {
            self.formats
                .iter()
                .position(|format| format.fourcc == fourcc)
        });
    }
}

pub fn run(
    initial_source: Option<String>,
    start: StartOptions,
) -> Result<(), slint::PlatformError> {
    let mut config = Config::load();
    if config.audio_multi_channel_volume || config.volume_control == "multi" {
        config.volume_control = "multi".into();
        config.audio_multi_channel_volume = true;
    }
    if let Some(width) = start.width {
        config.width = width;
    }
    if let Some(height) = start.height {
        config.height = height;
    }
    if let Some(fps) = start.fps {
        config.fps = fps;
    }
    if let Some(source) = initial_source {
        config.source = source.clone();
        config.sources = vec![source];
    }
    let ui = MainWindow::new()?;
    crate::i18n::set_language(crate::i18n::Language::from_code(config.language.as_str()));
    let translation = ui.global::<Translation>();
    translation.on_tr(|text, _generation| crate::i18n::tr(text.as_str()).into());
    translation.set_language_gen(1);
    ui.set_language(config.language.clone().into());
    let panel_model = Rc::new(VecModel::<PanelInfo>::default());
    ui.set_panels(ModelRc::new(panel_model.clone()));
    let state = Rc::new(RefCell::new(AppState::new(config, panel_model, start)));
    state.borrow_mut().sound_menu_base = ui.get_sound_menu().iter().collect();

    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    let renderer = state.borrow().config.video_renderer.clone();
    ui.set_osd_renderer(osd_tokens(vec![(
        renderer_osd_label(&renderer).into(),
        TONE_RED,
    )]));
    apply_config_to_ui(&state, &ui);
    {
        let st = state.borrow();
        ui.window().set_size(slint::PhysicalSize::new(
            st.config.window_w as u32,
            st.config.window_h as u32,
        ));
        if let (Some(x), Some(y)) = (st.config.window_x, st.config.window_y) {
            ui.window()
                .set_position(slint::PhysicalPosition::new(x as i32, y as i32));
        }
        if st.config.separate_geometry {
            apply_mode_geometry(&ui, &st.config, st.config.video_skin_start);
        }
    }

    let url_source = state
        .borrow()
        .panels
        .iter()
        .map(|panel| panel.source.clone())
        .find(|source| source.contains("://"));
    if let Some(source) = url_source {
        ui.set_network_url(source.as_str().into());
    }

    request_scan(&state);
    sync_sources(&state.borrow(), &ui);
    refresh_browser(&state, &ui);

    let hide_timer = Rc::new(Timer::default());
    wire_actions(&ui, &state, &hide_timer);
    wire_timers(&ui, &state, &hide_timer);
    refresh_favorite_menu(&state, &ui);
    refresh_play_menu(&state, &ui);

    ui.show()?;
    ui.run()
}

fn wire_actions(ui: &MainWindow, state: &Rc<RefCell<AppState>>, hide_timer: &Rc<Timer>) {
    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_key_command(move |text| {
            let Some(ui) = weak.upgrade() else {
                return false;
            };
            match action_for(text.as_str()) {
                KeyAction::ToggleOsd => {
                    toggle_osd_mode(&state, &ui, "full");
                    true
                }
                KeyAction::BriefInfo => {
                    toggle_osd_mode(&state, &ui, "brief");
                    true
                }
                KeyAction::PlaybackInfo => {
                    toggle_osd_mode(&state, &ui, "playback");
                    true
                }
                KeyAction::ToggleFreeze => {
                    toggle_freeze(&state, &ui);
                    true
                }
                KeyAction::Snapshot => {
                    save_snapshot(&state, &ui);
                    true
                }
                KeyAction::ToggleRecording => {
                    toggle_recording(&state, &ui);
                    true
                }
                KeyAction::ToggleAudioRecording => {
                    toggle_audio_recording(&state, &ui);
                    true
                }
                KeyAction::ToggleAudioProcessing => {
                    toggle_audio_processing(&state, &ui);
                    true
                }
                KeyAction::ToggleAudioNormalize => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Normalize);
                    true
                }
                KeyAction::ToggleAudioCrystallize => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Crystallize);
                    true
                }
                KeyAction::ToggleAudioDenoise => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Denoise);
                    true
                }
                KeyAction::ToggleAudioReverb => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Reverb);
                    true
                }
                KeyAction::ToggleAudioVocalRemove => {
                    toggle_audio_effect(&state, &ui, AudioEffect::VocalRemove);
                    true
                }
                KeyAction::ToggleAudioVocalEnhance => {
                    toggle_audio_effect(&state, &ui, AudioEffect::VocalEnhance);
                    true
                }
                KeyAction::ToggleEqualizer => {
                    toggle_equalizer(&state, &ui);
                    true
                }
                KeyAction::StepBackward => {
                    step_active_frame(&state, &ui, -1);
                    true
                }
                KeyAction::StepForward => {
                    step_active_frame(&state, &ui, 1);
                    true
                }
                KeyAction::ToggleFullscreen => {
                    let fullscreen = !state.borrow().fullscreen;
                    set_fullscreen(&state, &ui, fullscreen, &timer);
                    true
                }
                KeyAction::ExitFullscreen => {
                    if ui.get_menu_open() {
                        ui.set_menu_open(false);
                        arm_chrome_hide(&timer, &ui.as_weak());
                        true
                    } else if state.borrow().fullscreen {
                        set_fullscreen(&state, &ui, false, &timer);
                        true
                    } else {
                        false
                    }
                }
                KeyAction::ClosePanel => {
                    let index = state.borrow().active;
                    if let Some(index) = index {
                        close_panel(&state, &ui, index);
                        if state.borrow().panels.is_empty() {
                            quit_app(&state, &ui);
                        }
                    } else {
                        quit_app(&state, &ui);
                    }
                    true
                }
                KeyAction::OpenSettings => {
                    ui.set_settings_tab(1);
                    ui.set_settings_open(true);
                    sync_settings_selection(&state, &ui);
                    true
                }
                KeyAction::OpenFile => {
                    open_file_dialog(&state, &ui);
                    true
                }
                KeyAction::OpenClipboard => {
                    open_clipboard_source(&state, &ui);
                    true
                }
                KeyAction::OpenUrl => {
                    show_open_url_dialog(&ui);
                    true
                }
                KeyAction::MenuSearch => {
                    show_menu_search(&state, &ui);
                    true
                }
                KeyAction::OpenBurst => {
                    open_burst_dialog(&state, &ui);
                    true
                }
                KeyAction::OpenThumbnail => {
                    open_thumbnail_dialog(&state, &ui);
                    true
                }
                KeyAction::Reopen => {
                    let active = state.borrow().active;
                    if let Some(index) = active {
                        stop_panel(&state, &ui, index);
                        start_panel(&state, &ui, index);
                    }
                    true
                }
                KeyAction::OpenSubtitle => {
                    show_subtitle_dialog(&state, &ui);
                    true
                }
                KeyAction::OpenSubtitleBrowser => {
                    show_subtitle_dialog(&state, &ui);
                    true
                }
                KeyAction::ManualSubtitle => {
                    ui.set_manual_text("".into());
                    ui.set_manual_open(true);
                    true
                }
                KeyAction::CycleStereo => {
                    cycle_stereo(&state, &ui);
                    true
                }
                KeyAction::CaptureSlot1 => {
                    open_screen_slot(&state, &ui, 1);
                    true
                }
                KeyAction::CaptureSlot2 => {
                    open_screen_slot(&state, &ui, 2);
                    true
                }
                KeyAction::CaptureStart1 => {
                    open_screen_slot(&state, &ui, 1);
                    true
                }
                KeyAction::CaptureStart2 => {
                    open_screen_slot(&state, &ui, 2);
                    true
                }
                KeyAction::BrightnessDown => {
                    nudge_adjust(&state, &ui, AdjustField::Brightness, -1);
                    true
                }
                KeyAction::BrightnessUp => {
                    nudge_adjust(&state, &ui, AdjustField::Brightness, 1);
                    true
                }
                KeyAction::SaturationDown => {
                    nudge_adjust(&state, &ui, AdjustField::Saturation, -1);
                    true
                }
                KeyAction::SaturationUp => {
                    nudge_adjust(&state, &ui, AdjustField::Saturation, 1);
                    true
                }
                KeyAction::AdjustReset => {
                    reset_video_adjust(&state, &ui);
                    true
                }
                KeyAction::AudioSyncLag => {
                    nudge_audio_sync(&state, &ui, 50);
                    true
                }
                KeyAction::AudioSyncLead => {
                    nudge_audio_sync(&state, &ui, -50);
                    true
                }
                KeyAction::AudioSyncReset => {
                    reset_audio_sync(&state, &ui);
                    true
                }
                KeyAction::AudioSwap => {
                    toggle_audio_swap(&state, &ui);
                    true
                }
                KeyAction::FlipVertical => {
                    toggle_flip(false, &state, &ui);
                    true
                }
                KeyAction::FlipHorizontal => {
                    toggle_flip(true, &state, &ui);
                    true
                }
                KeyAction::CycleMargin => {
                    cycle_margin(&state, &ui);
                    true
                }
                KeyAction::RotateCycle => {
                    cycle_rotation(&state, &ui);
                    true
                }
                KeyAction::AspectCycle => {
                    cycle_aspect(&state, &ui);
                    true
                }
                KeyAction::AspectRatioCycle => {
                    cycle_zoom_ratio(&state, &ui);
                    true
                }
                KeyAction::FrameLarger => {
                    step_frame_scale(10, 10, &state, &ui);
                    true
                }
                KeyAction::FrameSmaller => {
                    step_frame_scale(-10, -10, &state, &ui);
                    true
                }
                KeyAction::FrameWider => {
                    step_frame_scale(10, 0, &state, &ui);
                    true
                }
                KeyAction::FrameNarrower => {
                    step_frame_scale(-10, 0, &state, &ui);
                    true
                }
                KeyAction::FrameTaller => {
                    step_frame_scale(0, 10, &state, &ui);
                    true
                }
                KeyAction::FrameShorter => {
                    step_frame_scale(0, -10, &state, &ui);
                    true
                }
                KeyAction::FrameShiftLeft => {
                    shift_zoom(-5, 0, "左移", &state, &ui);
                    true
                }
                KeyAction::FrameShiftRight => {
                    shift_zoom(5, 0, "右移", &state, &ui);
                    true
                }
                KeyAction::FrameShiftUp => {
                    shift_zoom(0, -5, "上移", &state, &ui);
                    true
                }
                KeyAction::FrameShiftDown => {
                    shift_zoom(0, 5, "下移", &state, &ui);
                    true
                }
                KeyAction::FrameCenter => {
                    reset_zoom(&state, &ui);
                    true
                }
                KeyAction::CyclePostAction => {
                    cycle_post_action(&state, &ui);
                    true
                }
                KeyAction::TopmostToggle => {
                    toggle_topmost(&state, &ui);
                    true
                }
                KeyAction::ToggleSubtitles => {
                    toggle_subtitles(&state, &ui);
                    true
                }
                KeyAction::OpenFolder => {
                    open_folder_dialog(&state, &ui);
                    true
                }
                KeyAction::OpenAbout => {
                    ui.set_about_open(true);
                    true
                }
                KeyAction::ToggleSidebar => {
                    let open = !ui.get_sidebar_open();
                    ui.set_sidebar_open(open);
                    if open {
                        refresh_browser(&state, &ui);
                    }
                    true
                }
                KeyAction::PreviousPanel => {
                    ui.invoke_panel_prev();
                    true
                }
                KeyAction::NextPanel => {
                    ui.invoke_panel_next();
                    true
                }
                KeyAction::SeekBackward => {
                    let step = state.borrow().config.seek_step_seconds;
                    seek_relative(&state, &ui, -step);
                    true
                }
                KeyAction::SeekForward => {
                    let step = state.borrow().config.seek_step_seconds;
                    seek_relative(&state, &ui, step);
                    true
                }
                KeyAction::VolumeUp => {
                    adjust_volume(&state, &ui, 0.05);
                    true
                }
                KeyAction::VolumeDown => {
                    adjust_volume(&state, &ui, -0.05);
                    true
                }
                KeyAction::ToggleMute => {
                    let muted = !ui.get_muted();
                    ui.set_muted(muted);
                    apply_audio_settings(&state, &ui);
                    true
                }
                KeyAction::SpeedUp => {
                    adjust_speed(&state, &ui, 0.1);
                    true
                }
                KeyAction::SpeedDown => {
                    adjust_speed(&state, &ui, -0.1);
                    true
                }
                KeyAction::SpeedReset => {
                    set_playback_speed(&state, &ui, 1.0);
                    true
                }
                KeyAction::ToggleAbLoop => {
                    cycle_ab_loop(&state, &ui);
                    true
                }
                KeyAction::AddBookmark => {
                    add_bookmark(&state, &ui);
                    true
                }
                KeyAction::PreviousChapter => {
                    jump_chapter(&state, &ui, false);
                    true
                }
                KeyAction::NextChapter => {
                    jump_chapter(&state, &ui, true);
                    true
                }
                KeyAction::FavoriteAddFile => {
                    add_favorite_file(&state, &ui);
                    true
                }
                KeyAction::FavoriteAddFolder => {
                    add_favorite_folder(&state, &ui);
                    true
                }
                KeyAction::None => false,
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_device_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let picked = state
                .borrow()
                .devices
                .get(index.max(0) as usize)
                .map(|device| (device.path.display().to_string(), device.name.clone()));
            let Some((source, title)) = picked else {
                return;
            };
            open_source(&state, &ui, source, title);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_settings_device_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let selected = index.max(0) as usize;
            if state.borrow().devices.get(selected).is_none() {
                return;
            }
            refresh_settings_formats(&state, &ui);
            let format_index = state.borrow().settings_format_index;
            state.borrow_mut().selected_format = format_index;
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_settings_format_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let selected = ui.get_selected_device().max(0) as usize;
            let (source, fourcc, format_index) = {
                let st = state.borrow();
                let Some(device) = st.devices.get(selected) else {
                    return;
                };
                let Some(format) = device.formats.get(index.max(0) as usize) else {
                    return;
                };
                (
                    device.path.display().to_string(),
                    format.fourcc.clone(),
                    index.max(0) as usize,
                )
            };
            let running = {
                let mut st = state.borrow_mut();
                st.format_overrides.insert(source.clone(), fourcc.clone());
                st.settings_format_index = Some(format_index);
                let active = st.active;
                st.selected_format = Some(format_index);
                active.and_then(|active| {
                    let panel = st.panels.get_mut(active)?;
                    if panel.source != source {
                        return None;
                    }
                    panel.settings.fourcc = Some(fourcc.clone());
                    panel.running.then_some(active)
                })
            };
            if let Some(active) = running {
                start_panel(&state, &ui, active);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_settings_device_open(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let picked = state
                .borrow()
                .devices
                .get(index.max(0) as usize)
                .map(|device| (device.path.display().to_string(), device.name.clone()));
            let Some((source, title)) = picked else {
                return;
            };
            open_source(&state, &ui, source, title);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_resolution_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let picked = {
                let st = state.borrow();
                let Some(active) = st.active else {
                    return;
                };
                let Some(panel) = st.panels.get(active) else {
                    return;
                };
                st.resolutions
                    .get(index.max(0) as usize)
                    .map(|resolution| (active, resolution.width, resolution.height, panel.running))
            };
            let Some((active, width, height, running)) = picked else {
                return;
            };
            {
                let mut st = state.borrow_mut();
                if let Some(panel) = st.panels.get_mut(active) {
                    panel.settings.width = width;
                    panel.settings.height = height;
                    panel.preferred_width = width;
                    panel.preferred_height = height;
                }
                st.config.width = width;
                st.config.height = height;
                st.selected_resolution = Some(index.max(0) as usize);
            }
            sync_sources(&state.borrow(), &ui);
            if running {
                start_panel(&state, &ui, active);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_format_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let picked = {
                let st = state.borrow();
                let Some(active) = st.active else {
                    return;
                };
                let Some(panel) = st.panels.get(active) else {
                    return;
                };
                st.formats
                    .get(index.max(0) as usize)
                    .map(|format| (active, format.fourcc.clone(), panel.running))
            };
            let Some((active, fourcc, running)) = picked else {
                return;
            };
            {
                let mut st = state.borrow_mut();
                if let Some(panel) = st.panels.get_mut(active) {
                    panel.settings.fourcc = Some(fourcc);
                }
                st.selected_format = Some(index.max(0) as usize);
            }
            sync_sources(&state.borrow(), &ui);
            if running {
                start_panel(&state, &ui, active);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_refresh_devices(move || {
            if weak.upgrade().is_some() {
                request_scan(&state);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_connect_network(move |url| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let url = url.trim().to_string();
            if url.is_empty() {
                return;
            }
            ui.set_network_url(url.as_str().into());
            open_source(&state, &ui, url.clone(), url);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_url(move |url| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let url = url.trim().to_string();
            if url.is_empty() {
                return;
            }
            let (source, title) = if let Some(rest) = url.strip_prefix("file://") {
                let path = percent_decode(rest);
                let title = Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                (path, title)
            } else {
                (url.clone(), url.clone())
            };
            ui.set_open_url_text(url.as_str().into());
            open_source(&state, &ui, source, title);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_bookmark_jump(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let target = {
                let st = state.borrow();
                st.active
                    .and_then(|active| st.panels.get(active))
                    .and_then(|panel| panel.bookmarks.get(index.max(0) as usize).copied())
            };
            let Some(target) = target else {
                show_notice(&state, &ui, "没有可跳转的书签".into());
                return;
            };
            seek_seconds(&state, &ui, target);
            show_notice(&state, &ui, format!("跳转到书签 {target:.1} 秒"));
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_bookmark_remove(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            remove_bookmark(&state, &ui, index);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_menu_search(move |text| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            filter_menu_search(&state, &ui, text.as_str());
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_menu_search_pick(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let command = {
                let st = state.borrow();
                st.menu_search_commands
                    .get(index.max(0) as usize)
                    .cloned()
            };
            let Some(command) = command else {
                return;
            };
            ui.set_menu_search_open(false);
            ui.invoke_menu_command(command.as_str().into());
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_navigation_settings_apply(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            apply_navigation_settings(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_capture_size_apply(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            apply_capture_size(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_window_size_apply(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            apply_window_size(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_start_requested(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let (active, first_device) = {
                let st = state.borrow();
                let first_device = st
                    .devices
                    .first()
                    .map(|device| (device.path.display().to_string(), device.name.clone()));
                (st.active, first_device)
            };
            match active {
                Some(index) => {
                    let running = state
                        .borrow()
                        .panels
                        .get(index)
                        .map(|panel| panel.running)
                        .unwrap_or(false);
                    if !running {
                        start_panel(&state, &ui, index);
                    }
                }
                None => match first_device {
                    Some((source, title)) => open_source(&state, &ui, source, title),
                    None => show_notice(&state, &ui, "没有可用的采集设备".to_string()),
                },
            }
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_stop_requested(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let active = state.borrow().active;
            if let Some(index) = active {
                stop_panel(&state, &ui, index);
            }
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_snapshot_requested(move || {
            if let Some(ui) = weak.upgrade() {
                save_snapshot(&state, &ui);
                show_chrome(&ui, &timer);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_record_requested(move || {
            if let Some(ui) = weak.upgrade() {
                toggle_recording(&state, &ui);
                show_chrome(&ui, &timer);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_toggle_freeze(move || {
            if let Some(ui) = weak.upgrade() {
                toggle_freeze(&state, &ui);
                show_chrome(&ui, &timer);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_toggle_osd(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            toggle_osd_mode(&state, &ui, "full");
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_toggle_fullscreen(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let fullscreen = !state.borrow().fullscreen;
            set_fullscreen(&state, &ui, fullscreen, &timer);
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let timer = hide_timer.clone();
        ui.on_toggle_menu(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            toggle_device_menu(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_seek_requested(move |fraction| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            seek_active(&state, &ui, fraction);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_volume_changed(move |_value| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            apply_audio_settings(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_seek_preview(move |fraction| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            update_seek_hover(&state, &ui, fraction);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_file_navigate(move |path| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            navigate_file_dialog(&state, &ui, path.as_str());
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_file_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            file_picked(&state, &ui, index);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_browser_navigate(move |path| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            browser_navigate(&state, &ui, path.as_str());
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_browser_open(move |path| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let path = PathBuf::from(path.as_str());
            if !path.is_file() {
                return;
            }
            let dir = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_media_dir);
            load_browser_dir(&state, &ui, dir);
            open_media_file(&state, &ui, &path);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_browser_up(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            browser_up(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_file_open_selected(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            open_file_selected(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_file_choose_dir(move |dir| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            choose_folder(&state, &ui, dir.as_str());
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_panel_picked(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            activate_panel(&state, &ui, index.max(0) as usize);
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_panel_toggle_fullscreen(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            activate_panel(&state, &ui, index.max(0) as usize);
            let fullscreen = !state.borrow().fullscreen;
            set_fullscreen(&state, &ui, fullscreen, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_panel_prev(move || step_panel(&state, &weak, -1));
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_panel_next(move || step_panel(&state, &weak, 1));
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_panel_closed(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            close_panel(&state, &ui, index.max(0) as usize);
            show_chrome(&ui, &timer);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_toggle_always_on_top(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            toggle_topmost(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_minimize_window(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        });
    }

    {
        let weak = ui.as_weak();
        ui.on_maximize_window(move || {
            if let Some(ui) = weak.upgrade() {
                let maximized = !ui.window().is_maximized();
                ui.window().set_maximized(maximized);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_close_window(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            quit_app(&state, &ui);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let timer = hide_timer.clone();
        ui.on_menu_command(move |action| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if let Some(rest) = action.as_str().strip_prefix("audio-device2-") {
                if rest == "none" {
                    set_audio_device2(None, "不使用(推荐)", &state, &ui);
                } else if let Ok(index) = rest.parse::<usize>() {
                    let sink = state.borrow().audio_sinks.get(index).cloned();
                    if let Some(sink) = sink {
                        set_audio_device2(Some(sink.name), &sink.label, &state, &ui);
                    }
                }
                return;
            }
            if let Some(rest) = action.as_str().strip_prefix("audio-device-") {
                if rest == "default" {
                    set_audio_device(None, "自动选择(推荐)", &state, &ui);
                } else if let Ok(index) = rest.parse::<usize>() {
                    let sink = state.borrow().audio_sinks.get(index).cloned();
                    if let Some(sink) = sink {
                        set_audio_device(Some(sink.name), &sink.label, &state, &ui);
                    }
                }
                return;
            }
            if let Some(rest) = action.as_str().strip_prefix("audio-passthrough-") {
                if rest == "default" {
                    set_audio_passthrough_device(None, "默认声音输出设备(推荐)", &state, &ui);
                } else if let Ok(index) = rest.parse::<usize>() {
                    let sink = state.borrow().audio_sinks.get(index).cloned();
                    if let Some(sink) = sink {
                        set_audio_passthrough_device(Some(sink.name), &sink.label, &state, &ui);
                    }
                }
                return;
            }
            match action.as_str() {
                "open-file" => open_file_dialog(&state, &ui),
                "open-folder" => open_folder_dialog(&state, &ui),
                "open-clipboard" => open_clipboard_source(&state, &ui),
                "open-url" | "open-network" => show_open_url_dialog(&ui),
                "open-default" => {
                    let action = state.borrow().config.default_open_action.clone();
                    match action.as_str() {
                        "url" => show_open_url_dialog(&ui),
                        "folder" => open_folder_dialog(&state, &ui),
                        _ => open_file_dialog(&state, &ui),
                    }
                }
                "default-open-file" => set_default_open_action("file", &state, &ui),
                "default-open-url" => set_default_open_action("url", &state, &ui),
                "default-open-folder" => set_default_open_action("folder", &state, &ui),
                "default-open-navigate" => set_default_open_action("navigate", &state, &ui),
                "open-device" => toggle_device_menu(&ui, &timer),
                "capture-slot1" => open_screen_slot(&state, &ui, 1),
                "capture-slot2" => open_screen_slot(&state, &ui, 2),
                "capture-start1" => open_screen_slot(&state, &ui, 1),
                "capture-start2" => open_screen_slot(&state, &ui, 2),
                "capture-size1" => open_capture_size_dialog(&state, &ui, 1),
                "capture-size2" => open_capture_size_dialog(&state, &ui, 2),
                "device-settings" => {
                    ui.set_settings_tab(1);
                    ui.set_settings_open(true);
                    sync_settings_selection(&state, &ui);
                }
                "skin-settings" => {
                    let skin_id = {
                        let mut st = state.borrow_mut();
                        st.skin_opacity_backup = st.config.skin_opacity;
                        st.skin_backup = st.config.skin.clone();
                        st.skin_library = skin::load_library();
                        st.config.skin.clone()
                    };
                    let library = state.borrow().skin_library.clone();
                    apply_skin_id(&ui, &library, &skin_id);
                    sync_skin_rows(&ui, &library, &skin_id);
                    ui.set_skin_settings_open(true);
                }
                "close-panel" => {
                    let index = state.borrow().active;
                    if let Some(index) = index {
                        close_panel(&state, &ui, index);
                    }
                }
                "play-pause" => toggle_freeze(&state, &ui),
                "start" => {
                    let pending = {
                        let st = state.borrow();
                        st.active.filter(|index| {
                            st.panels
                                .get(*index)
                                .map(|panel| !panel.running)
                                .unwrap_or(false)
                        })
                    };
                    match pending {
                        Some(index) => start_panel(&state, &ui, index),
                        None => toggle_device_menu(&ui, &timer),
                    }
                }
                "reopen" => {
                    let active = state.borrow().active;
                    if let Some(index) = active {
                        stop_panel(&state, &ui, index);
                        start_panel(&state, &ui, index);
                    }
                }
                "aspect-fit" => {
                    state.borrow_mut().stretch = false;
                    ui.set_stretch(false);
                }
                "aspect-stretch" => {
                    state.borrow_mut().stretch = true;
                    ui.set_stretch(true);
                }
                "aspect-cycle" => cycle_aspect(&state, &ui),
                "aspect-ratio-cycle" => cycle_zoom_ratio(&state, &ui),
                "zoom-none" => set_zoom(0, false, "不使用缩放功能", &state, &ui),
                "zoom-stretch-4-3" => set_zoom(1333, false, "按 4:3 拉伸", &state, &ui),
                "zoom-stretch-16-9" => set_zoom(1778, false, "按 16:9 拉伸", &state, &ui),
                "zoom-stretch-16-10" => set_zoom(1600, false, "按 16:10 拉伸", &state, &ui),
                "zoom-stretch-1-85" => set_zoom(1850, false, "按 1.85:1 拉伸", &state, &ui),
                "zoom-stretch-2-35" => set_zoom(2350, false, "按 2.35:1 拉伸", &state, &ui),
                "zoom-crop-4-3" => set_zoom(1333, true, "按 4:3 裁剪", &state, &ui),
                "zoom-crop-16-9" => set_zoom(1778, true, "按 16:9 裁剪", &state, &ui),
                "zoom-crop-16-10" => set_zoom(1600, true, "按 16:10 裁剪", &state, &ui),
                "zoom-crop-1-85" => set_zoom(1850, true, "按 1.85:1 裁剪", &state, &ui),
                "zoom-crop-2-35" => set_zoom(2350, true, "按 2.35:1 裁剪", &state, &ui),
                "zoom-reset" => reset_zoom(&state, &ui),
                "zoom-left" => shift_zoom(-5, 0, "左移", &state, &ui),
                "zoom-right" => shift_zoom(5, 0, "右移", &state, &ui),
                "zoom-up" => shift_zoom(0, -5, "上移", &state, &ui),
                "zoom-down" => shift_zoom(0, 5, "下移", &state, &ui),
                "frame-larger" => step_frame_scale(10, 10, &state, &ui),
                "frame-smaller" => step_frame_scale(-10, -10, &state, &ui),
                "frame-wider" => step_frame_scale(10, 0, &state, &ui),
                "frame-narrower" => step_frame_scale(-10, 0, &state, &ui),
                "frame-taller" => step_frame_scale(0, 10, &state, &ui),
                "frame-shorter" => step_frame_scale(0, -10, &state, &ui),
                "frame-shift-left" => shift_zoom(-5, 0, "左移", &state, &ui),
                "frame-shift-right" => shift_zoom(5, 0, "右移", &state, &ui),
                "frame-shift-up" => shift_zoom(0, -5, "上移", &state, &ui),
                "frame-shift-down" => shift_zoom(0, 5, "下移", &state, &ui),
                "frame-center" => reset_zoom(&state, &ui),
                "frame-preset-50" => set_frame_scale(50, 50, &state, &ui),
                "frame-preset-100" => set_frame_scale(100, 100, &state, &ui),
                "frame-preset-150" => set_frame_scale(150, 150, &state, &ui),
                "frame-preset-200" => set_frame_scale(200, 200, &state, &ui),
                "frame-remember-toggle" => toggle_remember_frame(&state, &ui),
                "margin-cycle" => cycle_margin(&state, &ui),
                "margin-0" => set_margin(0, &state, &ui),
                "margin-5" => set_margin(5, &state, &ui),
                "margin-10" => set_margin(10, &state, &ui),
                "margin-15" => set_margin(15, &state, &ui),
                "margin-20" => set_margin(20, &state, &ui),
                "margin-25" => set_margin(25, &state, &ui),
                "margin-30" => set_margin(30, &state, &ui),
                "margin-auto-toggle" => {
                    let value = {
                        let mut st = state.borrow_mut();
                        st.margin_auto = !st.margin_auto;
                        st.margin_auto
                    };
                    ui.set_margin_auto(value);
                    show_notice(
                        &state,
                        &ui,
                        format!("有字幕则显示下边距：{}", if value { "开" } else { "关" }),
                    );
                }
                "margin-force-toggle" => {
                    let value = {
                        let mut st = state.borrow_mut();
                        st.margin_force = !st.margin_force;
                        st.margin_force
                    };
                    ui.set_margin_force(value);
                    show_notice(
                        &state,
                        &ui,
                        format!("强制生成下边距：{}", if value { "开" } else { "关" }),
                    );
                }
                "stop" => {
                    let index = state.borrow().active;
                    if let Some(index) = index {
                        stop_panel(&state, &ui, index);
                    }
                }
                "fullscreen" => {
                    let fullscreen = !state.borrow().fullscreen;
                    set_fullscreen(&state, &ui, fullscreen, &timer);
                }
                "prev-panel" => ui.invoke_panel_prev(),
                "next-panel" => ui.invoke_panel_next(),
                "panel-cycle" => ui.invoke_panel_next(),
                "panel-1" => activate_panel(&state, &ui, 0),
                "panel-2" => activate_panel(&state, &ui, 1),
                "panel-3" => activate_panel(&state, &ui, 2),
                "panel-4" => activate_panel(&state, &ui, 3),
                "post-cycle" => cycle_post_action(&state, &ui),
                "post-exit-current" => set_post_action("exit-current", &state, &ui),
                "post-exit-all" => set_post_action("exit-all", &state, &ui),
                "post-off-current" => set_post_action("off-current", &state, &ui),
                "post-off-all" => set_post_action("off-all", &state, &ui),
                "shutdown-default" => set_shutdown_mode("poweroff", &state, &ui),
                "shutdown-suspend" => set_shutdown_mode("suspend", &state, &ui),
                "shutdown-hibernate" => set_shutdown_mode("hibernate", &state, &ui),
                "remember-post-toggle" => toggle_remember_post_action(&state, &ui),
                "config-manager" => open_config_manager(&ui),
                "burst-config" => open_burst_dialog(&state, &ui),
                "burst-immediate" => {
                    let (interval, count) = {
                        let st = state.borrow();
                        (st.config.snapshot_interval_ms, st.config.snapshot_count)
                    };
                    start_burst(&state, &ui, interval, count);
                }
                "snapshot-path-choose" => open_path_dialog(&state, &ui, PathDialogTarget::Snapshot),
                "thumbnail-config" => open_thumbnail_dialog(&state, &ui),
                "maximize" => ui.invoke_maximize_window(),
                "screen-size-45" => resize_window_to_screen(&ui, &state, 45),
                "screen-size-60" => resize_window_to_screen(&ui, &state, 60),
                "screen-size-75" => resize_window_to_screen(&ui, &state, 75),
                "screen-size-borderless" => resize_window_to_screen(&ui, &state, 100),
                "screen-size-settings" => open_window_size_dialog(&state, &ui),
                "screen-size-custom" => {
                    let (width, height) = {
                        let st = state.borrow();
                        (st.config.window_custom_width, st.config.window_custom_height)
                    };
                    resize_window(&ui, &state, width, height);
                    show_notice(&state, &ui, format!("窗口尺寸：{width}×{height}"));
                }
                "window-grow" | "window-shrink" => {
                    let step = if action.as_str() == "window-grow" { 1.1 } else { 1.0 / 1.1 };
                    let size = ui.window().size();
                    let width = ((size.width as f32 * step) as u32).max(320);
                    let height = ((size.height as f32 * step) as u32).max(200);
                    ui.window()
                        .set_size(slint::PhysicalSize::new(width, height));
                }
                "toggle-sidebar" => {
                    let open = !ui.get_sidebar_open();
                    ui.set_sidebar_open(open);
                    if open {
                        refresh_browser(&state, &ui);
                    }
                }
                "always-on-top" => toggle_topmost(&state, &ui),
                "topmost-toggle" => toggle_topmost(&state, &ui),
                "topmost-always" => set_topmost_mode("always", &state, &ui),
                "topmost-playing" => set_topmost_mode("playing", &state, &ui),
                "topmost-fullscreen" => set_topmost_mode("fullscreen", &state, &ui),
                "autohide-toggle" => toggle_autohide_controls(&state, &ui),
                "fullscreen-no-cover-toggle" => toggle_fullscreen_no_cover(&state, &ui),
                "copy-file-name" => copy_file_info("name", &state, &ui),
                "copy-file-dir" => copy_file_info("dir", &state, &ui),
                "copy-file-path" => copy_file_info("path", &state, &ui),
                "freeze" => toggle_freeze(&state, &ui),
                "snapshot" => save_snapshot(&state, &ui),
                "record" => toggle_recording(&state, &ui),
                "snapshot-format-png" => set_snapshot_format("png", &state, &ui),
                "snapshot-format-jpeg" => set_snapshot_format("jpeg", &state, &ui),
                "snapshot-format-bmp" => set_snapshot_format("bmp", &state, &ui),
                "snapshot-name-time" => set_snapshot_name("time", &state, &ui),
                "snapshot-name-playback" => set_snapshot_name("playback", &state, &ui),
                "snapshot-play-folder-toggle" => toggle_snapshot_play_folder(&state, &ui),
                "snapshot-open-folder" => open_snapshot_folder(&state, &ui),
                "snapshot-delete-last" => delete_last_snapshot(&state, &ui),
                "video-path-choose" => open_path_dialog(&state, &ui, PathDialogTarget::VideoRecord),
                "video-open-folder" => open_video_folder(&state, &ui),
                "video-record-play-folder" => {
                    toggle_record_play_folder(&state, &ui, RecordFolder::Video)
                }
                "audio-record-toggle" => toggle_audio_recording(&state, &ui),
                "audio-path-choose" => open_path_dialog(&state, &ui, PathDialogTarget::AudioRecord),
                "audio-open-folder" => open_audio_folder(&state, &ui),
                "audio-record-play-folder" => {
                    toggle_record_play_folder(&state, &ui, RecordFolder::Audio)
                }
                "step-backward" => step_active_frame(&state, &ui, -1),
                "step-forward" => step_active_frame(&state, &ui, 1),
                "osd" => toggle_osd_mode(&state, &ui, "full"),
                "osd-brief" => toggle_osd_mode(&state, &ui, "brief"),
                "osd-playback" => toggle_osd_mode(&state, &ui, "playback"),
                "ignore-shortcuts-toggle" => toggle_ignore_shortcuts(&state, &ui),
                "favorites-memory-toggle" => toggle_favorites_memory_points(&state, &ui),
                "similar-selected" => set_similar_mode(&state, &ui, "selected"),
                "similar-similar" => set_similar_mode(&state, &ui, "similar"),
                "similar-all" => set_similar_mode(&state, &ui, "all"),
                "touch-toggle" => toggle_touch_enabled(&state, &ui),
                "move-titlebar-toggle" => toggle_move_only_titlebar(&state, &ui),
                "exit" => quit_app(&state, &ui),
                "about" => ui.set_about_open(true),
                "speed-up" => adjust_speed(&state, &ui, 0.1),
                "speed-down" => adjust_speed(&state, &ui, -0.1),
                "speed-reset" => set_playback_speed(&state, &ui, 1.0),
                "repeat-toggle" => {
                    let value = !state.borrow().repeat;
                    state.borrow_mut().repeat = value;
                    ui.set_repeat(value);
                    refresh_play_menu(&state, &ui);
                    show_notice(
                        &state,
                        &ui,
                        format!("循环播放：{}", if value { "开" } else { "关" }),
                    );
                }
                "shuffle-toggle" => {
                    let value = !state.borrow().shuffle;
                    state.borrow_mut().shuffle = value;
                    ui.set_shuffle(value);
                    refresh_play_menu(&state, &ui);
                    show_notice(
                        &state,
                        &ui,
                        format!("随机播放：{}", if value { "开" } else { "关" }),
                    );
                }
                "remember-position-toggle" => {
                    let value = {
                        let mut st = state.borrow_mut();
                        st.config.remember_position = !st.config.remember_position;
                        if !st.config.remember_position {
                            st.config.resume.clear();
                        }
                        if let Err(error) = st.config.save() {
                            log::warn!("保存配置失败：{error}");
                        }
                        st.config.remember_position
                    };
                    refresh_play_menu(&state, &ui);
                    show_notice(
                        &state,
                        &ui,
                        format!("记忆播放位置：{}", if value { "开" } else { "关" }),
                    );
                }
                "marker-toggle" => {
                    let value = {
                        let mut st = state.borrow_mut();
                        st.config.show_markers = !st.config.show_markers;
                        if let Err(error) = st.config.save() {
                            log::warn!("保存配置失败：{error}");
                        }
                        st.config.show_markers
                    };
                    if !value {
                        clear_seek_markers(&ui);
                    }
                    refresh_play_menu(&state, &ui);
                    show_notice(
                        &state,
                        &ui,
                        format!("进度条书签/章节标记：{}", if value { "开" } else { "关" }),
                    );
                }
                "ab-a" => set_ab_start(&state, &ui),
                "ab-b" => set_ab_end(&state, &ui),
                "ab-off" => clear_ab_loop(&state, &ui),
                "seek-start" => seek_active(&state, &ui, 0.0),
                "seek-end" => seek_active(&state, &ui, 1.0),
                "seek-back-10" => seek_relative(&state, &ui, -10.0),
                "seek-forward-10" => seek_relative(&state, &ui, 10.0),
                "playlist-add-file" => add_file_dialog(&state, &ui),
                "playlist-add-folder" => add_folder_dialog(&state, &ui),
                "playlist-clear" => clear_playlist(&state, &ui),
                "subtitle-open" => show_subtitle_dialog(&state, &ui),
                "subtitle-secondary-open" => show_secondary_subtitle_dialog(&state, &ui),
                "load-audio" => show_audio_dialog(&state, &ui),
                "subtitle-browser" => show_subtitle_dialog(&state, &ui),
                "subtitle-off" => clear_subtitle(&state, &ui),
                "subtitle-toggle" => toggle_subtitles(&state, &ui),
                "subtitle-reload" => reload_subtitle(&state, &ui),
                "subtitle-save-srt" => save_subtitle(&state, &ui, false),
                "subtitle-save-ass" => save_subtitle(&state, &ui, true),
                "subtitle-manual" => {
                    ui.set_manual_text("".into());
                    ui.set_manual_open(true);
                }
                "subtitle-settings" => ui.set_subtitle_settings_open(true),
                "subtitle-copy" => copy_current_subtitle(&state, &ui),
                "subtitle-bold-toggle" => toggle_subtitle_bold(&state, &ui),
                "subtitle-shadow-toggle" => toggle_subtitle_shadow(&state, &ui),
                "ass-styles-toggle" => toggle_ass_styles(&state, &ui),
                "subtitle-graphic-outside-toggle" => {
                    toggle_subtitle_flag(&state, &ui, "graphic-outside")
                }
                "subtitle-text-inside-toggle" => toggle_subtitle_flag(&state, &ui, "text-inside"),
                "subtitle-bottom-toggle" => toggle_subtitle_flag(&state, &ui, "bottom"),
                "subtitle-border-opaque-toggle" => {
                    toggle_subtitle_flag(&state, &ui, "border-opaque")
                }
                "subtitle-smooth-vmr-toggle" => toggle_subtitle_flag(&state, &ui, "smooth-vmr"),
                "avisynth-toggle" => toggle_synth(&state, &ui, true),
                "vapoursynth-toggle" => toggle_synth(&state, &ui, false),
                "avisynth-script-choose" => show_synth_script_dialog(&state, &ui, true),
                "vapoursynth-script-choose" => show_synth_script_dialog(&state, &ui, false),
                "avisynth-folder" => open_synth_folder(&state, &ui, true),
                "vapoursynth-folder" => open_synth_folder(&state, &ui, false),
                "subtitle-3d-off" => set_subtitle_3d(&state, &ui, "off"),
                "subtitle-3d-sbs" => set_subtitle_3d(&state, &ui, "sbs"),
                "subtitle-3d-tab" => set_subtitle_3d(&state, &ui, "tab"),
                "subtitle-3d-anaglyph" => set_subtitle_3d(&state, &ui, "anaglyph"),
                "subtitle-tts-toggle" => toggle_tts(&state, &ui),
                "subtitle-translate-toggle" => toggle_subtitle_translation(&state, &ui),
                "subtitle-download-open" => {
                    ui.set_download_url("".into());
                    ui.set_download_open(true);
                }
                "subtitle-download-apply" => download_subtitle(&state, &ui),
                "subtitle-tts-say" => speak_current_subtitle(&state, &ui),
                "subtitle-tts-stop" => {
                    stop_speech();
                    show_notice(&state, &ui, "已停止朗读".to_string());
                }
                "stereo-cycle" => cycle_stereo(&state, &ui),
                "stereo-off" => set_stereo_mode(&state, &ui, StereoMode::Off),
                "stereo-anaglyph-sbs" => {
                    set_stereo_mode(&state, &ui, StereoMode::AnaglyphSideBySide)
                }
                "stereo-anaglyph-tab" => {
                    set_stereo_mode(&state, &ui, StereoMode::AnaglyphTopBottom)
                }
                "stereo-left-sbs" => set_stereo_mode(&state, &ui, StereoMode::LeftSideBySide),
                "stereo-right-sbs" => set_stereo_mode(&state, &ui, StereoMode::RightSideBySide),
                "stereo-left-tab" => set_stereo_mode(&state, &ui, StereoMode::LeftTopBottom),
                "stereo-right-tab" => set_stereo_mode(&state, &ui, StereoMode::RightTopBottom),
                "renderer-auto" => set_video_renderer(&state, &ui, "auto"),
                "renderer-opengl" => set_video_renderer(&state, &ui, "opengl"),
                "renderer-null-compressed" => set_video_renderer(&state, &ui, "null-compressed"),
                "renderer-null-uncompressed" => {
                    set_video_renderer(&state, &ui, "null-uncompressed")
                }
                "window-pos-top-left" => place_window(&ui, &state, Some(WindowPlacement::TopLeft)),
                "window-pos-top-right" => {
                    place_window(&ui, &state, Some(WindowPlacement::TopRight))
                }
                "window-pos-bottom-left" => {
                    place_window(&ui, &state, Some(WindowPlacement::BottomLeft))
                }
                "window-pos-bottom-right" => {
                    place_window(&ui, &state, Some(WindowPlacement::BottomRight))
                }
                "window-pos-center" => place_window(&ui, &state, Some(WindowPlacement::Center)),
                "window-pos-restore" => place_window(&ui, &state, None),
                "pano-toggle" => {
                    let enabled = !state.borrow().pano_enabled;
                    set_panorama(enabled, &state, &ui);
                }
                "pano-reset" => reset_panorama(&state, &ui),
                "adjust-reset" => reset_video_adjust(&state, &ui),
                "brightness-down" => nudge_adjust(&state, &ui, AdjustField::Brightness, -1),
                "brightness-up" => nudge_adjust(&state, &ui, AdjustField::Brightness, 1),
                "contrast-down" => nudge_adjust(&state, &ui, AdjustField::Contrast, -1),
                "contrast-up" => nudge_adjust(&state, &ui, AdjustField::Contrast, 1),
                "saturation-down" => nudge_adjust(&state, &ui, AdjustField::Saturation, -1),
                "saturation-up" => nudge_adjust(&state, &ui, AdjustField::Saturation, 1),
                "rotate-0" => set_rotation(0, &state, &ui),
                "rotate-90" => set_rotation(90, &state, &ui),
                "rotate-180" => set_rotation(180, &state, &ui),
                "rotate-270" => set_rotation(270, &state, &ui),
                "rotate-toggle" => toggle_rotation(&state, &ui),
                "rotate-cycle" => cycle_rotation(&state, &ui),
                "rotate-auto-0-90" => auto_rotate(0, &state, &ui),
                "rotate-auto-180-270" => auto_rotate(180, &state, &ui),
                "flip-horizontal" => toggle_flip(true, &state, &ui),
                "flip-vertical" => toggle_flip(false, &state, &ui),
                "audio-stereo" => {
                    set_audio_channel(flint_media::audio::Channel::Stereo, &state, &ui)
                }
                "audio-left" => set_audio_channel(flint_media::audio::Channel::Left, &state, &ui),
                "audio-right" => set_audio_channel(flint_media::audio::Channel::Right, &state, &ui),
                "audio-output-auto" => {
                    set_audio_output(Some(0), Some(0), "自动选择(推荐)", &state, &ui)
                }
                "audio-format-16" => set_audio_output(None, Some(0), "16 位", &state, &ui),
                "audio-format-24" => set_audio_output(None, Some(1), "24 位", &state, &ui),
                "audio-format-32f" => set_audio_output(None, Some(2), "32 位浮点", &state, &ui),
                "audio-rate-44100" => set_audio_output(Some(44100), None, "44.1 kHz", &state, &ui),
                "audio-rate-48000" => set_audio_output(Some(48000), None, "48 kHz", &state, &ui),
                "audio-rate-96000" => set_audio_output(Some(96000), None, "96 kHz", &state, &ui),
                "audio-rate-192000" => set_audio_output(Some(192000), None, "192 kHz", &state, &ui),
                "audio-output-method-auto" => {
                    set_audio_output_method("auto", "自动选择(推荐)", &state, &ui)
                }
                "audio-output-method-pulse" => {
                    set_audio_output_method("pulse", "PulseAudio", &state, &ui)
                }
                "audio-output-method-null" => {
                    set_audio_output_method("null", "空设备(静音)", &state, &ui)
                }
                "multi-channel-volume-toggle" => toggle_multi_channel_volume(&state, &ui),
                "ac3-dts-volume-toggle" => toggle_ac3_dts_volume(&state, &ui),
                "volume-control-settings" => open_volume_settings(&state, &ui),
                "audio-output-settings" => open_audio_output_settings(&state, &ui),
                "audio-cycle" => cycle_audio_channel(&state, &ui),
                "audio-swap" => toggle_audio_swap(&state, &ui),
                "audio-processing-toggle" => toggle_audio_processing(&state, &ui),
                "audio-effect-normalize" => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Normalize)
                }
                "audio-effect-crystallize" => {
                    toggle_audio_effect(&state, &ui, AudioEffect::Crystallize)
                }
                "audio-effect-denoise" => toggle_audio_effect(&state, &ui, AudioEffect::Denoise),
                "audio-effect-reverb" => toggle_audio_effect(&state, &ui, AudioEffect::Reverb),
                "audio-effect-vocal-remove" => {
                    toggle_audio_effect(&state, &ui, AudioEffect::VocalRemove)
                }
                "audio-effect-vocal-enhance" => {
                    toggle_audio_effect(&state, &ui, AudioEffect::VocalEnhance)
                }
                "equalizer-toggle" => toggle_equalizer(&state, &ui),
                "eq-preset-flat" => set_eq_preset(&state, &ui, "flat"),
                "eq-preset-pop" => set_eq_preset(&state, &ui, "pop"),
                "eq-preset-rock" => set_eq_preset(&state, &ui, "rock"),
                "eq-preset-classical" => set_eq_preset(&state, &ui, "classical"),
                "eq-preset-dance" => set_eq_preset(&state, &ui, "dance"),
                "eq-preset-bass" => set_eq_preset(&state, &ui, "bass"),
                "eq-preset-treble" => set_eq_preset(&state, &ui, "treble"),
                "eq-preset-vocal" => set_eq_preset(&state, &ui, "vocal"),
                "audio-sync-lag" => nudge_audio_sync(&state, &ui, 50),
                "audio-sync-lead" => nudge_audio_sync(&state, &ui, -50),
                "audio-sync-reset" => reset_audio_sync(&state, &ui),
                "audio-sync-settings" => {
                    ui.set_audio_sync_settings_value(ui.get_audio_sync_ms() as f32);
                    ui.set_audio_sync_settings_open(true);
                }
                "audio-sync-keep-none" => set_audio_sync_mode(&state, &ui, "none"),
                "audio-sync-keep-always" => set_audio_sync_mode(&state, &ui, "always"),
                "audio-sync-reset-exit-toggle" => toggle_audio_sync_reset_exit(&state, &ui),
                "audio-general-settings" => {
                    let config = state.borrow().config.clone();
                    ui.set_audio_general_language(audio_language_index(&config.audio_language));
                    ui.set_audio_general_select_language(config.audio_select_language);
                    ui.set_audio_general_ac3(config.audio_ac3_dts);
                    ui.set_audio_general_processing(config.audio_processing);
                    ui.set_audio_general_reset_exit(config.audio_sync_reset_exit);
                    ui.set_audio_general_settings_open(true);
                }
                "volume-up" => adjust_volume(&state, &ui, 0.05),
                "volume-down" => adjust_volume(&state, &ui, -0.05),
                "master-volume-up" => adjust_volume(&state, &ui, 0.05),
                "master-volume-down" => adjust_volume(&state, &ui, -0.05),
                "mute-toggle" => {
                    let muted = !ui.get_muted();
                    ui.set_muted(muted);
                    apply_audio_settings(&state, &ui);
                }
                "volume-control-default" => {
                    set_volume_control("default", "默认音量控制(推荐)", &state, &ui)
                }
                "volume-control-wave" => {
                    set_volume_control("system-wave", "系统波形音量控制", &state, &ui)
                }
                "volume-control-master" => {
                    set_volume_control("system-master", "系统主音量控制", &state, &ui)
                }
                "wave-volume-up" => adjust_volume(&state, &ui, 0.05),
                "wave-volume-down" => adjust_volume(&state, &ui, -0.05),
                "wave-mute-toggle" => {
                    let muted = !ui.get_muted();
                    ui.set_muted(muted);
                    apply_audio_settings(&state, &ui);
                    show_notice(
                        &state,
                        &ui,
                        format!("静音：{}", if muted { "开" } else { "关" }),
                    );
                }
                "system-volume-up" => step_system_volume(5, &state, &ui),
                "system-volume-down" => step_system_volume(-5, &state, &ui),
                "system-mute-toggle" => toggle_system_mute(&state, &ui),
                "bookmark-add" => add_bookmark(&state, &ui),
                "navigate-bookmarks" => show_bookmark_dialog(&state, &ui),
                "menu-search" => show_menu_search(&state, &ui),
                "navigate-settings" => show_navigation_settings(&state, &ui),
                "bookmark-prev" => jump_bookmark(&state, &ui, false),
                "bookmark-next" => jump_bookmark(&state, &ui, true),
                "bookmark-clear" => clear_bookmarks(&state, &ui),
                "subtitle-pos-bottom" | "subtitle-pos-top" => {
                    let top = action.as_str() == "subtitle-pos-top";
                    let mut st = state.borrow_mut();
                    st.config.subtitle_top = top;
                    if let Err(error) = st.config.save() {
                        log::warn!("保存配置失败：{error}");
                    }
                    drop(st);
                    ui.set_subtitle_top(top);
                    show_notice(
                        &state,
                        &ui,
                        format!("字幕位置：{}", if top { "顶部" } else { "底部" }),
                    );
                }
                "subtitle-color-white"
                | "subtitle-color-yellow"
                | "subtitle-color-cyan"
                | "subtitle-color-green" => {
                    let name = action
                        .as_str()
                        .strip_prefix("subtitle-color-")
                        .unwrap_or("white")
                        .to_string();
                    {
                        let mut st = state.borrow_mut();
                        st.config.subtitle_color = name.clone();
                        if let Err(error) = st.config.save() {
                            log::warn!("保存配置失败：{error}");
                        }
                    }
                    ui.set_subtitle_color(subtitle_color_value(&name));
                    ui.set_subtitle_color_name(name.as_str().into());
                    show_notice(&state, &ui, format!("字幕颜色：{name}"));
                }
                "chapter-prev" => jump_chapter(&state, &ui, false),
                "chapter-next" => jump_chapter(&state, &ui, true),
                "favorite-add-file" => add_favorite_file(&state, &ui),
                "favorite-add-folder" => add_favorite_folder(&state, &ui),
                "favorite-clear" => clear_favorites(&state, &ui),
                "skin-browse-menu" => show_skin_file_dialog(&state, &ui),
                other if other.starts_with("subtitle-lang-") => {
                    set_subtitle_language(&state, &ui, &other["subtitle-lang-".len()..]);
                }
                "logo-common-pick" => show_logo_file_dialog(&state, &ui, 1),
                "logo-play-pick" => show_logo_file_dialog(&state, &ui, 2),
                "logo-common-clear" => clear_logo(&state, &ui, 1),
                "logo-play-clear" => clear_logo(&state, &ui, 2),
                "logo-common-toggle" | "logo-play-toggle" => {
                    let slot = u8::from(action.as_str() == "logo-play-toggle");
                    toggle_logo(&state, &ui, slot + 1);
                }
                other if other.starts_with("skin-menu-select-") => {
                    if let Ok(index) = other["skin-menu-select-".len()..].parse::<usize>() {
                        select_skin_from_menu(&state, &ui, index);
                    }
                }
                other if other.starts_with("accent-") => {
                    set_menu_accent(&state, &ui, &other["accent-".len()..]);
                }
                other if other.starts_with("skin-opacity-set-") => {
                    if let Ok(percent) = other["skin-opacity-set-".len()..].parse::<i32>() {
                        set_skin_opacity_percent(&state, &ui, percent);
                    }
                }
                "skin-layer-toggle" => toggle_skin_layer_output(&state, &ui),
                "popup-menu-type-skin" => set_popup_menu_type(&state, &ui, "skin"),
                "popup-menu-type-system" => set_popup_menu_type(&state, &ui, "system"),
                "menu-output-auto" => set_menu_output_mode(&state, &ui, "auto"),
                "menu-output-dark" => set_menu_output_mode(&state, &ui, "dark"),
                "touch-skin-toggle" => toggle_touch_skin(&state, &ui),
                "video-skin-start-toggle" => toggle_video_skin_start(&state, &ui),
                "menu-show-names-toggle" => toggle_menu_show_names(&state, &ui),
                "enlarge-skin-menu-toggle" => toggle_enlarge_skin_menu(&state, &ui),
                "keep-ui-mode-toggle" => toggle_keep_ui_mode(&state, &ui),
                "separate-geometry-toggle" => toggle_separate_geometry(&state, &ui),
                "language-auto" => set_language(&state, &ui, "auto"),
                "language-zh" => set_language(&state, &ui, "zh"),
                "language-en" => set_language(&state, &ui, "en"),
                _ => {
                    if action.as_str() == "subtitle-delay-reset" {
                        set_subtitle_delay(&state, &ui, 0.0);
                    } else if action.as_str() == "audio-external" {
                        show_notice(&state, &ui, "当前使用外部音频".into());
                    } else if let Some(value) = action.as_str().strip_prefix("audio-track-") {
                        if let Ok(index) = value.parse::<usize>() {
                            select_audio_track(&state, &ui, index);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("subtitle-stream-") {
                        if let Ok(index) = value.parse::<usize>() {
                            load_embedded_subtitle(&state, &ui, index);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("subtitle-delay-") {
                        if let Ok(delta) = value.parse::<f64>() {
                            adjust_subtitle_delay(&state, &ui, delta);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("subtitle-font-step-")
                    {
                        if let Ok(delta) = value.parse::<i32>() {
                            step_subtitle_font(&state, &ui, delta);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("subtitle-font-") {
                        if let Ok(index) = value.parse::<usize>() {
                            set_subtitle_font(&state, &ui, index);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("subtitle-size-") {
                        if let Ok(scale) = value.parse::<f32>() {
                            let scale = scale.clamp(0.6, 2.0);
                            {
                                let mut st = state.borrow_mut();
                                st.config.subtitle_scale = scale;
                                if let Err(error) = st.config.save() {
                                    log::warn!("保存配置失败：{error}");
                                }
                            }
                            ui.set_subtitle_scale(scale);
                            show_notice(&state, &ui, format!("字幕字号：{scale:.2}"));
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("stereo-") {
                        set_stereo_mode(&state, &ui, StereoMode::from_key(value));
                    } else if let Some(value) = action.as_str().strip_prefix("speed-") {
                        if let Ok(speed) = value.parse::<f32>() {
                            set_playback_speed(&state, &ui, speed);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("chapter-") {
                        if let Ok(index) = value.parse::<usize>() {
                            jump_chapter_index(&state, &ui, index);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("favorite-") {
                        if let Ok(index) = value.parse::<usize>() {
                            open_favorite(&state, &ui, index);
                        }
                    } else if let Some(value) = action.as_str().strip_prefix("memory-") {
                        if let Some((favorite, point)) = value.split_once('-') {
                            if let (Ok(favorite), Ok(point)) =
                                (favorite.parse::<usize>(), point.parse::<usize>())
                            {
                                jump_memory_point(&state, &ui, favorite, point);
                            }
                        }
                    }
                }
            }
            show_chrome(&ui, &timer);
        });
    }
}

fn quit_app(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    stop_burst(state);
    {
        let size = ui.window().size();
        let position = ui.window().position();
        let video_mode = ui.get_video_skin();
        let mut st = state.borrow_mut();
        st.config.window_w = size.width as f32;
        st.config.window_h = size.height as f32;
        st.config.window_x = Some(position.x as f32);
        st.config.window_y = Some(position.y as f32);
        if st.config.separate_geometry {
            record_mode_geometry(&mut st.config, video_mode, size, position);
        }
        st.config.osd_visible = st.osd_visible;
        st.config.sources = st.panels.iter().map(|panel| panel.source.clone()).collect();
        st.config.source = st
            .active
            .and_then(|index| st.panels.get(index))
            .or_else(|| st.panels.first())
            .map(|panel| panel.source.clone())
            .unwrap_or_default();
        if st.config.audio_sync_reset_exit {
            st.config.audio_sync_ms = 0;
        } else if st.config.audio_sync_mode == "always" {
            st.config.audio_sync_ms = ui.get_audio_sync_ms();
        }
        remember_running_positions(&mut st);
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        for panel in &st.panels {
            panel.shared.request_stop();
        }
    }
    let _ = ui.hide();
    let _ = slint::quit_event_loop();
}

fn wire_timers(ui: &MainWindow, state: &Rc<RefCell<AppState>>, hide_timer: &Rc<Timer>) {
    {
        let weak = ui.as_weak();
        let timer = hide_timer.clone();
        ui.on_activity(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_chrome_visible(true);
                arm_chrome_hide(&timer, &ui.as_weak());
            }
        });
    }

    {
        let state = state.clone();
        ui.on_pano_drag(move |dx, dy| rotate_panorama(&state, dx, dy));
    }
    {
        let state = state.clone();
        ui.on_pano_zoom(move |delta| zoom_panorama(&state, delta));
    }
    {
        let weak = ui.as_weak();
        ui.on_window_drag(move |dx, dy| {
            if let Some(ui) = weak.upgrade() {
                let position = ui.window().position();
                ui.window().set_position(slint::PhysicalPosition::new(
                    position.x + dx.round() as i32,
                    position.y + dy.round() as i32,
                ));
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_panel_touch_seek(move |index, fraction| {
            if let Some(ui) = weak.upgrade() {
                touch_seek_panel(&state, &ui, index.max(0) as usize, fraction);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_shutdown_confirmed(move || {
            if let Some(ui) = weak.upgrade() {
                perform_shutdown(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_shutdown_cancelled(move || {
            if let Some(ui) = weak.upgrade() {
                let label = shutdown_label(&state.borrow().config.shutdown_mode);
                show_notice(&state, &ui, format!("已取消{label}"));
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_config_picked(move |index| {
            if let Some(ui) = weak.upgrade() {
                if let Some(name) = ui.get_config_profiles().row_data(index.max(0) as usize) {
                    ui.set_config_rename_text(name.as_str().into());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_config_load(move || {
            if let Some(ui) = weak.upgrade() {
                load_config_profile(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_config_create(move || {
            if let Some(ui) = weak.upgrade() {
                create_config_profile(&state, &ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_config_rename(move |name| {
            if let Some(ui) = weak.upgrade() {
                rename_config_profile(&ui, name.as_str());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_config_delete(move || {
            if let Some(ui) = weak.upgrade() {
                delete_config_profile(&ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_skin_pick(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(skin) = state
                .borrow()
                .skin_library
                .get(index.max(0) as usize)
                .cloned()
            else {
                return;
            };
            let id = skin.id();
            apply_skin(&ui, &skin);
            let library = {
                let mut st = state.borrow_mut();
                st.config.skin = id.clone();
                st.skin_library.clone()
            };
            sync_skin_rows(&ui, &library, &id);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_skin_browse(move || {
            if let Some(ui) = weak.upgrade() {
                show_skin_file_dialog(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_file_dialog_closed(move || {
            let skin_pick = {
                let mut st = state.borrow_mut();
                let pick = st.skin_pick;
                st.skin_pick = false;
                pick
            };
            if skin_pick {
                if let Some(ui) = weak.upgrade() {
                    ui.set_skin_settings_open(true);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_skin_opacity_changed(move |value| {
            let opacity = (value.clamp(0.4, 1.0) * 100.0).round() / 100.0;
            state.borrow_mut().config.skin_opacity = opacity;
            if let Some(ui) = weak.upgrade() {
                ui.set_skin_opacity(opacity);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_volume_settings_apply(move || {
            if let Some(ui) = weak.upgrade() {
                apply_volume_settings(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_audio_sync_settings_apply(move || {
            if let Some(ui) = weak.upgrade() {
                let target = ui.get_audio_sync_settings_value().round() as i32;
                let delta = target.clamp(-5000, 5000) - ui.get_audio_sync_ms();
                ui.set_audio_sync_settings_open(false);
                ui.set_audio_sync_settings_value(target.clamp(-5000, 5000) as f32);
                nudge_audio_sync(&state, &ui, delta);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_audio_settings_apply(move || {
            if let Some(ui) = weak.upgrade() {
                apply_audio_output_settings(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_audio_general_settings_apply(move || {
            if let Some(ui) = weak.upgrade() {
                apply_audio_general_settings(&state, &ui);
            }
        });
    }
    {
        let state = state.clone();
        ui.on_skin_settings_confirm(move || {
            let st = state.borrow();
            if let Err(error) = st.config.save() {
                log::warn!("保存配置失败：{error}");
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_skin_settings_cancel(move || {
            let (opacity, skin_id, library) = {
                let mut st = state.borrow_mut();
                st.config.skin_opacity = st.skin_opacity_backup;
                st.config.skin = st.skin_backup.clone();
                (
                    st.config.skin_opacity,
                    st.config.skin.clone(),
                    st.skin_library.clone(),
                )
            };
            if let Some(ui) = weak.upgrade() {
                ui.set_skin_opacity(opacity);
                apply_skin_id(&ui, &library, &skin_id);
                sync_skin_rows(&ui, &library, &skin_id);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_burst_start(move |interval, count| {
            if let Some(ui) = weak.upgrade() {
                let interval = interval.trim().parse::<u64>().unwrap_or(1000);
                let count = count.trim().parse::<i32>().unwrap_or(10);
                start_burst(&state, &ui, interval, count);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_path_dialog_apply(move |path| {
            if let Some(ui) = weak.upgrade() {
                apply_path_dialog(&state, &ui, path.as_str());
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_thumbnail_create(move |width| {
            if let Some(ui) = weak.upgrade() {
                create_thumbnail(&state, &ui, width.as_str());
            }
        });
    }

    let presenter = Timer::default();
    {
        let weak = ui.as_weak();
        let state = state.clone();
        let mut last_ui_stats = Instant::now();
        presenter.start(TimerMode::Repeated, FRAME_INTERVAL, move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let scan = { state.borrow().scan_rx.try_recv().ok() };
            if let Some(scan) = scan {
                apply_scan(&state, &ui, scan);
            }

            let now = Instant::now();
            let mut st = state.borrow_mut();
            let active = st.active;
            let fullscreen = st.fullscreen;
            let mut dirty = Vec::new();
            let mut external_notice = None;
            let mut ended_active = None;
            let (ab_start, ab_end) = (st.ab_start, st.ab_end);
            let stereo_mode = st.stereo_mode;
            let pano_view = st.pano_enabled.then_some(st.pano_view);
            let pano_dirty = st.pano_dirty;
            let adjust = st.adjust;
            let rotation = st.rotation;
            let flip_horizontal = st.flip_horizontal;
            let flip_vertical = st.flip_vertical;
            let video_dirty = st.video_dirty;
            let transform = VideoTransform {
                stereo: stereo_mode,
                pano: pano_view,
                adjust,
                rotation,
                flip_horizontal,
                flip_vertical,
            };
            for (index, panel) in st.panels.iter_mut().enumerate() {
                let status = panel.shared.status();
                if let Some(notice) = panel.shared.take_external_audio_notice() {
                    external_notice = Some(notice);
                }
                let mut changed = false;
                if status.running && !panel.running {
                    panel.last_serial = 0;
                }
                if !status.running && panel.running {
                    panel.frozen = false;
                    panel.paused = false;
                    panel.recording = false;
                    if status.error.as_deref() == Some(flint_media::EOF_ERROR)
                        && is_file_source(&panel.source)
                        && active == Some(index)
                    {
                        ended_active = Some(index);
                    }
                }
                if status.running != panel.running {
                    panel.running = status.running;
                    changed = true;
                }
                if status.error != panel.last_error {
                    panel.last_error = status.error.clone();
                    changed = true;
                }
                if !panel.frozen || panel.paused {
                    let visible = !fullscreen || active == Some(index);
                    let mut refreshed = false;
                    if let Some(frame) = panel.shared.slot.take() {
                        if !visible {
                            panel.shared.slot.recycle(frame);
                        } else {
                            if frame.serial != panel.last_serial {
                                panel.last_serial = frame.serial;
                                render_panel_image(panel, &frame, transform);
                                refreshed = true;
                                changed = true;
                            }
                            if let Some(previous) = panel.last_frame.replace(frame) {
                                panel.shared.slot.recycle(previous);
                            }
                        }
                    }
                    if (pano_dirty || video_dirty) && !refreshed {
                        if let Some(frame) = panel.last_frame.clone() {
                            render_panel_image(panel, &frame, transform);
                            changed = true;
                        }
                    }
                }
                if panel.last_stats.elapsed() >= STATS_INTERVAL {
                    panel.last_stats = now;
                    update_panel_stats(panel);
                    changed = true;
                }
                if active == Some(index) && panel.running && !panel.paused && !panel.frozen {
                    if let (Some(start), Some(end)) = (ab_start, ab_end) {
                        let engine = panel.engine.lock();
                        if engine.position_seconds() >= end {
                            engine.seek_to(start);
                        }
                    }
                }
                if changed {
                    dirty.push(index);
                }
            }
            for index in dirty {
                let row = st
                    .panels
                    .get(index)
                    .map(|panel| panel.row(active == Some(index)));
                if let Some(row) = row {
                    st.panel_model.set_row_data(index, row);
                }
            }
            if last_ui_stats.elapsed() >= STATS_INTERVAL {
                last_ui_stats = now;
                update_active_ui(&ui, &mut st);
            }
            if pano_dirty {
                st.pano_dirty = false;
            }
            if video_dirty {
                st.video_dirty = false;
            }
            drop(st);
            if let Some(notice) = external_notice {
                show_notice(&state, &ui, notice);
            }
            apply_topmost(&state, &ui);
            if let Some(index) = ended_active {
                let (count, repeat, shuffle) = {
                    let st = state.borrow();
                    (st.panels.len(), st.repeat, st.shuffle)
                };
                let next = if shuffle && count > 1 {
                    let seed = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|elapsed| elapsed.subsec_nanos() as usize)
                        .unwrap_or(0);
                    Some((index + 1 + seed % (count - 1)) % count)
                } else if index + 1 < count {
                    Some(index + 1)
                } else if repeat && count > 0 {
                    Some(0)
                } else {
                    None
                };
                let post_action = state.borrow().config.post_action.clone();
                if post_action == "exit-current" || (post_action == "exit-all" && next.is_none()) {
                    quit_app(&state, &ui);
                    return;
                }
                if post_action == "off-current" || (post_action == "off-all" && next.is_none()) {
                    ask_shutdown(&state, &ui);
                    return;
                }
                if let Some(next) = next {
                    activate_panel(&state, &ui, next);
                }
            }
        });
    }

    arm_chrome_hide(hide_timer, &ui.as_weak());
    state.borrow_mut().presenter = Some(Rc::new(presenter));
}

fn apply_config_to_ui(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        let config = st.config.clone();
        st.osd_visible = config.osd_visible;
        st.stereo_mode = StereoMode::from_key(&config.stereo_mode);
        st.pano_enabled = config.pano_enabled;
        st.pano_view = panorama::View {
            yaw: config.pano_yaw.to_radians(),
            pitch: config.pano_pitch.to_radians(),
            fov: config.pano_fov.to_radians(),
        }
        .clamped();
        st.pano_dirty = true;
        st.adjust = effects::Adjust {
            brightness: config.brightness,
            contrast: config.contrast,
            saturation: config.saturation,
        };
        st.rotation = config.rotation;
        st.flip_horizontal = config.flip_horizontal;
        st.flip_vertical = config.flip_vertical;
        st.video_dirty = true;
        st.tts_enabled = config.tts_enabled;
    }
    let st = state.borrow();
    let config = &st.config;
    ui.set_osd_visible(st.osd_visible);
    ui.set_ignore_shortcuts(config.ignore_shortcuts);
    ui.set_favorites_memory_points(config.favorites_memory_points);
    ui.set_similar_mode(config.similar_mode.as_str().into());
    ui.set_video_renderer(config.video_renderer.as_str().into());
    ui.set_touch_enabled(config.touch_enabled);
    ui.set_move_only_titlebar(config.move_only_titlebar);
    ui.set_skin_opacity(config.skin_opacity);
    let skin_id = config.skin.clone();
    let library = st.skin_library.clone();
    apply_skin_id(ui, &library, &skin_id);
    sync_skin_rows(ui, &library, &skin_id);
    ui.set_popup_menu_type(config.popup_menu_type.as_str().into());
    ui.set_default_open_action(config.default_open_action.as_str().into());
    ui.set_menu_output_mode(config.menu_output_mode.as_str().into());
    ui.set_menu_scale(menu_scale_for(config));
    ui.global::<UiOptions>()
        .set_menu_show_names(config.menu_show_names);
    ui.set_panel_skin_output(config.skin_layer_output);
    ui.set_video_skin(config.video_skin_start);
    if config.video_skin_start {
        ui.set_chrome_visible(false);
    }
    ui.set_topmost_mode(config.topmost_mode.as_str().into());
    ui.set_pinned(config.topmost_mode == "always");
    ui.set_subtitle_scale(config.subtitle_scale);
    ui.set_subtitle_color(subtitle_color_value(&config.subtitle_color));
    ui.set_subtitle_color_name(config.subtitle_color.as_str().into());
    ui.set_subtitle_font_name(subtitle_font_name(&config.subtitle_font).into());
    ui.set_subtitle_font_family(config.subtitle_font.as_str().into());
    ui.set_subtitle_bold(config.subtitle_bold);
    ui.set_subtitle_shadow(config.subtitle_shadow);
    ui.set_subtitle_top(config.subtitle_top);
    ui.set_subtitle_3d_mode(config.subtitle_3d.as_str().into());
    if let Some(image) = load_logo_image(&config.logo_file) {
        ui.set_logo_image(image);
        ui.set_logo_visible(config.logo_show);
    }
    if let Some(image) = load_logo_image(&config.logo_play_file) {
        ui.set_play_logo_image(image);
        ui.set_play_logo_visible(config.logo_play_show);
    }
    ui.set_stereo_mode(st.stereo_mode.key().into());
    ui.set_pano_enabled(st.pano_enabled);
    ui.set_rotation(st.rotation as i32);
    ui.set_flip_horizontal(st.flip_horizontal);
    ui.set_flip_vertical(st.flip_vertical);
    ui.set_remember_frame(st.remember_frame);
    ui.set_volume_control(st.volume_control.as_str().into());
    ui.set_audio_sync_mode(config.audio_sync_mode.as_str().into());
    ui.set_audio_sync_reset_exit(config.audio_sync_reset_exit);
    if config.audio_sync_mode == "always" {
        ui.set_audio_sync_ms(config.audio_sync_ms);
    }
    ui.set_frame_scale_x(st.frame_scale_x);
    ui.set_frame_scale_y(st.frame_scale_y);
    ui.set_zoom_shift_x(st.zoom_shift_x);
    ui.set_zoom_shift_y(st.zoom_shift_y);
    ui.set_snapshot_format(config.snapshot_format.as_str().into());
    ui.set_snapshot_name(config.snapshot_name.as_str().into());
    ui.set_snapshot_play_folder(config.snapshot_play_folder);
    ui.set_video_record_play_folder(config.video_record_play_folder);
    ui.set_audio_record_play_folder(config.audio_record_play_folder);
    ui.set_audio_output_method(config.audio_output_method.as_str().into());
    ui.set_multi_channel_volume(config.audio_multi_channel_volume);
    ui.set_ac3_dts_volume(config.audio_ac3_dts);
    ui.set_audio_processing(config.audio_processing);
    ui.set_audio_effect_normalize(config.audio_effect_normalize);
    ui.set_audio_effect_crystallize(config.audio_effect_crystallize);
    ui.set_audio_effect_denoise(config.audio_effect_denoise);
    ui.set_audio_effect_reverb(config.audio_effect_reverb);
    ui.set_audio_effect_vocal_remove(config.audio_effect_vocal_remove);
    ui.set_audio_effect_vocal_enhance(config.audio_effect_vocal_enhance);
    ui.set_equalizer_enabled(config.audio_equalizer);
    ui.set_equalizer_preset(config.audio_eq_preset.as_str().into());
    ui.set_post_action(config.post_action.as_str().into());
    ui.set_shutdown_mode(config.shutdown_mode.as_str().into());
    ui.set_remember_post_action(config.remember_post_action);
    ui.set_autohide_controls(config.autohide_controls);
    ui.set_fullscreen_no_cover(config.fullscreen_no_cover);
}

fn screen_source(slot: u32) -> String {
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    format!("screen{slot}:{display}")
}

fn open_screen_slot(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u32) {
    let source = screen_source(slot);
    let title = format!("X11 屏幕采集 {slot}#");
    open_source(state, ui, source, title);
}

fn parse_capture_dim(text: &str, fallback: u32) -> u32 {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|value| (16..=8192).contains(value))
        .unwrap_or(fallback)
}

fn open_capture_size_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u32) {
    let (width, height) = {
        let st = state.borrow();
        if slot == 2 {
            (st.config.screen2_width, st.config.screen2_height)
        } else {
            (st.config.screen1_width, st.config.screen1_height)
        }
    };
    ui.set_capture_size_slot(if slot == 2 { 2 } else { 1 });
    ui.set_capture_size_width(width.to_string().as_str().into());
    ui.set_capture_size_height(height.to_string().as_str().into());
    ui.set_capture_size_open(true);
}

fn apply_capture_size(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let slot = if ui.get_capture_size_slot() == 2 { 2 } else { 1 };
    let (fallback_w, fallback_h) = {
        let st = state.borrow();
        if slot == 2 {
            (st.config.screen2_width, st.config.screen2_height)
        } else {
            (st.config.screen1_width, st.config.screen1_height)
        }
    };
    let width = parse_capture_dim(ui.get_capture_size_width().as_str(), fallback_w);
    let height = parse_capture_dim(ui.get_capture_size_height().as_str(), fallback_h);
    let source = screen_source(slot);
    let index = {
        let mut st = state.borrow_mut();
        if slot == 2 {
            st.config.screen2_width = width;
            st.config.screen2_height = height;
        } else {
            st.config.screen1_width = width;
            st.config.screen1_height = height;
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.panels
            .iter()
            .position(|panel| panel.source == source)
    };
    ui.set_capture_size_open(false);
    if let Some(index) = index {
        let running = state
            .borrow()
            .panels
            .get(index)
            .map(|panel| panel.running)
            .unwrap_or(false);
        if running {
            start_panel(state, ui, index);
        }
    }
    show_notice(state, ui, format!("截屏{slot} 尺寸：{width}×{height}"));
}

fn monitor_bounds(ui: &MainWindow) -> Option<((i32, i32), u32, u32)> {
    use slint::winit_030::WinitWindowAccessor;

    ui.window()
        .with_winit_window(|window| {
            let monitor = window.current_monitor()?;
            let size = monitor.size();
            (size.width > 1 && size.height > 1).then(|| {
                (
                    (monitor.position().x, monitor.position().y),
                    size.width,
                    size.height,
                )
            })
        })
        .flatten()
        .or_else(|| {
            x11_screen_bounds().map(|((x, y), width, height)| ((x, y), width as u32, height as u32))
        })
}

fn resize_window_to_screen(ui: &MainWindow, state: &Rc<RefCell<AppState>>, percent: u32) {
    let Some((_, screen_width, screen_height)) = monitor_bounds(ui) else {
        show_notice(state, ui, "无法获取屏幕尺寸".to_string());
        return;
    };
    let width = (screen_width * percent / 100).max(320);
    let height = (screen_height * percent / 100).max(200);
    resize_window(ui, state, width, height);
    show_notice(state, ui, format!("窗口尺寸：{width}×{height}"));
}

fn resize_window(ui: &MainWindow, state: &Rc<RefCell<AppState>>, width: u32, height: u32) {
    let width = width.max(320);
    let height = height.max(200);
    ui.window().set_size(slint::PhysicalSize::new(width, height));
    let mut st = state.borrow_mut();
    st.config.window_custom_width = width;
    st.config.window_custom_height = height;
    if let Err(error) = st.config.save() {
        log::warn!("保存配置失败：{error}");
    }
}

fn open_window_size_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (width, height) = {
        let st = state.borrow();
        (st.config.window_custom_width, st.config.window_custom_height)
    };
    ui.set_window_size_width(width.to_string().into());
    ui.set_window_size_height(height.to_string().into());
    ui.set_window_size_open(true);
}

fn apply_window_size(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (fallback_width, fallback_height) = {
        let st = state.borrow();
        (st.config.window_custom_width, st.config.window_custom_height)
    };
    let width = parse_capture_dim(ui.get_window_size_width().as_str(), fallback_width);
    let height = parse_capture_dim(ui.get_window_size_height().as_str(), fallback_height);
    ui.set_window_size_open(false);
    resize_window(ui, state, width, height);
    show_notice(state, ui, format!("窗口尺寸：{width}×{height}"));
}

fn open_source(state: &Rc<RefCell<AppState>>, ui: &MainWindow, source: String, title: String) {
    let target = {
        let st = state.borrow();
        let sources: Vec<String> = st.panels.iter().map(|panel| panel.source.clone()).collect();
        panel_target(&sources, st.active, &source)
    };

    let index = match target {
        PanelTarget::Existing(index) => {
            let was_running = state
                .borrow()
                .panels
                .get(index)
                .map(|panel| panel.running)
                .unwrap_or(false);
            {
                let mut st = state.borrow_mut();
                if let Some(panel) = st.panels.get_mut(index) {
                    panel.title = title;
                }
                st.active = Some(index);
                st.refresh_menu();
                st.sync_panel_model();
            }
            sync_sources(&state.borrow(), ui);
            if !was_running {
                start_panel(state, ui, index);
            } else {
                update_active_ui(ui, &mut state.borrow_mut());
            }
            return;
        }
        PanelTarget::New => {
            let settings = { settings_for_source(&state.borrow(), &source) };
            let mut st = state.borrow_mut();
            st.panels
                .push(PanelState::new(source.clone(), title.clone(), settings));
            let index = st.panels.len() - 1;
            st.active = Some(index);
            index
        }
        PanelTarget::Replace(index) => {
            let engine = {
                let st = state.borrow();
                st.panels.get(index).map(|panel| {
                    panel.commands.fetch_add(1, Ordering::AcqRel);
                    panel.engine.clone()
                })
            };
            if let Some(engine) = engine {
                thread::spawn(move || {
                    engine.lock().stop();
                });
            }
            let settings = { settings_for_source(&state.borrow(), &source) };
            let mut st = state.borrow_mut();
            if index >= st.panels.len() {
                return;
            }
            if remember_position_for(&mut st, index) {
                if let Err(error) = st.config.save() {
                    log::warn!("保存播放位置失败：{error}");
                }
            }
            st.panels[index] = PanelState::new(source.clone(), title.clone(), settings);
            st.active = Some(index);
            index
        }
    };

    {
        let mut st = state.borrow_mut();
        st.refresh_menu();
        st.sync_panel_model();
        st.notice = None;
    }
    sync_sources(&state.borrow(), ui);
    start_panel(state, ui, index);
    update_active_ui(ui, &mut state.borrow_mut());
}

fn clipboard_source() -> Option<String> {
    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        (!text.trim().is_empty()).then_some(text)
    }
    let text = run(
        "xclip",
        &["-selection", "clipboard", "-o", "-t", "text/uri-list"],
    )
    .or_else(|| run("xclip", &["-selection", "clipboard", "-o"]))
    .or_else(|| run("wl-paste", &["--no-newline"]))?;
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line.strip_prefix("file://") {
            Some(path) => percent_decode(path),
            None => line.to_string(),
        })
        .next()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&text[index + 1..index + 3], 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn open_clipboard_source(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Some(source) = clipboard_source() else {
        show_notice(state, ui, "剪贴板中没有可播放的内容".to_string());
        return;
    };
    if !source.contains("://") && !std::path::Path::new(&source).exists() {
        show_notice(state, ui, "剪贴板中没有可播放的内容".to_string());
        return;
    }
    let title = if source.contains("://") {
        source.clone()
    } else {
        std::path::Path::new(&source)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| source.clone())
    };
    open_source(state, ui, source, title);
}

fn stop_other_files(state: &Rc<RefCell<AppState>>, ui: &MainWindow, keep: usize) {
    let running: Vec<usize> = {
        let st = state.borrow();
        st.panels
            .iter()
            .enumerate()
            .filter(|(index, panel)| {
                *index != keep && panel.running && is_file_source(&panel.source)
            })
            .map(|(index, _)| index)
            .collect()
    };
    for index in running {
        stop_panel(state, ui, index);
    }
}

fn start_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let (
        engine,
        commands,
        started_once,
        generation,
        source,
        settings,
        initial_bookmarks,
        resume,
        pending_seek,
        initial_streams,
    ) = {
        let st = state.borrow();
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        let settings = refine_settings(&st, panel);
        let generation = panel.commands.fetch_add(1, Ordering::AcqRel) + 1;
        let fresh = !panel.started_once.load(Ordering::Relaxed);
        let is_file = is_file_source(&panel.source);
        let key = file_key(&panel.source);
        let initial_bookmarks = (fresh && is_file).then(|| saved_bookmarks(&st, &panel.source));
        let resume = if fresh && is_file && st.config.remember_position {
            st.config
                .resume
                .iter()
                .find(|(path, _)| path == &key)
                .map(|(_, position)| *position)
        } else {
            None
        };
        let pending_seek = st
            .pending_seek
            .as_ref()
            .filter(|(path, _)| path == &key)
            .map(|(_, position)| *position);
        let initial_streams = (fresh && is_file)
            .then(|| embedded_subtitles::subtitle_streams(Path::new(&panel.source)).ok())
            .flatten();
        (
            panel.engine.clone(),
            panel.commands.clone(),
            panel.started_once.clone(),
            generation,
            panel.source.clone(),
            settings,
            initial_bookmarks,
            resume,
            pending_seek,
            initial_streams,
        )
    };
    if pending_seek.is_some() {
        state.borrow_mut().pending_seek = None;
    }
    if is_file_source(&source) {
        stop_other_files(state, ui, index);
    }
    let row = {
        let mut st = state.borrow_mut();
        let active = st.active == Some(index);
        if active {
            st.ab_start = None;
            st.ab_end = None;
        }
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.settings = settings.clone();
        if let Some(bookmarks) = initial_bookmarks {
            panel.bookmarks = bookmarks;
        }
        if let Some(streams) = initial_streams {
            panel.subtitle_streams = streams;
            panel.subtitle_stream = None;
        }
        panel.frozen = false;
        panel.paused = false;
        panel.recording = false;
        panel.last_error = None;
        panel.stats = PanelStats::default();
        panel.shared.clear_error();
        panel.row(active)
    };
    state.borrow().panel_model.set_row_data(index, row);
    ui.set_status_error("".into());
    log::info!(
        "启动采集：{} {}x{} {} fps fourcc={:?}",
        source,
        settings.width,
        settings.height,
        settings.fps,
        settings.fourcc
    );
    let (volume, muted) = (ui.get_volume(), ui.get_muted());
    engine.lock().set_volume(volume);
    engine.lock().set_muted(muted);
    let (audio_channel, audio_swap) = {
        let st = state.borrow();
        (st.audio_channel, st.audio_swap)
    };
    engine.lock().set_audio_channel(audio_channel);
    engine.lock().set_audio_swap(audio_swap);
    let (effect_flags, eq_gains) = {
        let st = state.borrow();
        (
            audio_effect_flags(&st.config),
            eq_preset_gains(&st.config.audio_eq_preset),
        )
    };
    engine.lock().set_audio_effects(effect_flags);
    engine.lock().set_audio_eq_gains(&eq_gains);
    let (audio_rate, audio_format, audio_device, audio_device2, passthrough, ac3, ac3_gain) = {
        let st = state.borrow();
        (
            st.audio_output_rate,
            st.audio_output_format,
            st.audio_device.clone(),
            st.audio_device2.clone(),
            (!st.config.audio_passthrough_device.is_empty())
                .then(|| st.config.audio_passthrough_device.clone()),
            st.config.audio_ac3_dts,
            st.config.audio_ac3_dts_gain_db,
        )
    };
    {
        let engine = engine.lock();
        engine.set_audio_output_rate(audio_rate);
        engine.set_audio_output_format(flint_media::audio::SampleFormat::from_code(audio_format));
        engine.set_audio_device(audio_device);
        engine.set_audio_device2(audio_device2);
        engine.set_audio_passthrough_device(passthrough);
        engine.set_audio_ac3_dts(ac3, ac3_gain);
        engine.set_audio_output_method(flint_media::audio_method_code(
            &state.borrow().config.audio_output_method,
        ));
    }
    let sync_ms = if state.borrow().config.audio_sync_mode == "always" {
        ui.get_audio_sync_ms()
    } else {
        0
    };
    ui.set_audio_sync_ms(sync_ms);
    let renderer = state.borrow().config.video_renderer.clone();
    thread::spawn(move || {
        let mut engine = engine.lock();
        if commands.load(Ordering::Acquire) != generation {
            return;
        }
        engine.set_video_renderer(&renderer);
        if let Err(error) = engine.start(&source, settings) {
            log::error!("启动采集失败：{error}");
        } else {
            engine.set_audio_sync_ms(sync_ms);
            started_once.store(true, Ordering::Relaxed);
            if let Some(position) = pending_seek.or(resume) {
                engine.seek_to(position);
            }
        }
    });
    auto_select_subtitle_for(state, ui, false);
}

fn stop_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let (engine, commands, generation) = {
        let st = state.borrow();
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        panel.shared.clear_error();
        let generation = panel.commands.fetch_add(1, Ordering::AcqRel) + 1;
        (panel.engine.clone(), panel.commands.clone(), generation)
    };
    {
        let mut st = state.borrow_mut();
        if remember_position_for(&mut st, index) {
            if let Err(error) = st.config.save() {
                log::warn!("保存播放位置失败：{error}");
            }
        }
    }
    {
        let mut st = state.borrow_mut();
        let active = st.active;
        if let Some(panel) = st.panels.get_mut(index) {
            panel.frozen = false;
            panel.paused = false;
            panel.recording = false;
            let row = panel.row(active == Some(index));
            st.panel_model.set_row_data(index, row);
        }
    }
    thread::spawn(move || {
        let mut engine = engine.lock();
        if commands.load(Ordering::Acquire) != generation {
            return;
        }
        engine.stop();
        engine.clear_error();
    });
    ui.set_frozen(false);
    ui.set_recording(false);
    ui.set_status_error("".into());
}

fn close_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let engine = {
        let st = state.borrow();
        st.panels.get(index).map(|panel| {
            panel.commands.fetch_add(1, Ordering::AcqRel);
            panel.engine.clone()
        })
    };
    let Some(engine) = engine else {
        return;
    };
    thread::spawn(move || {
        engine.lock().stop();
    });

    let empty = {
        let mut st = state.borrow_mut();
        if remember_position_for(&mut st, index) {
            if let Err(error) = st.config.save() {
                log::warn!("保存播放位置失败：{error}");
            }
        }
        st.panels.remove(index);
        st.active = next_active_after_remove(st.active, index, st.panels.len());
        if st.panels.is_empty() {
            st.fullscreen = false;
        }
        st.refresh_menu();
        st.sync_panel_model();
        st.panels.is_empty()
    };
    if empty {
        ui.set_fullscreen(false);
        ui.window().set_fullscreen(false);
    }
    sync_sources(&state.borrow(), ui);
    update_active_ui(ui, &mut state.borrow_mut());
}

fn step_panel(state: &Rc<RefCell<AppState>>, weak: &Weak<MainWindow>, delta: isize) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    let (count, current) = {
        let st = state.borrow();
        (st.panels.len(), st.active.unwrap_or(0))
    };
    if count == 0 {
        return;
    }
    let next = (current as isize + delta).rem_euclid(count as isize) as usize;
    activate_panel(state, &ui, next);
}

fn activate_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let (source, running) = {
        let st = state.borrow();
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        (panel.source.clone(), panel.running)
    };
    if is_file_source(&source) {
        stop_other_files(state, ui, index);
        if !running {
            start_panel(state, ui, index);
        }
    }
    {
        let mut st = state.borrow_mut();
        if index >= st.panels.len() {
            return;
        }
        if st.active != Some(index) {
            st.active = Some(index);
            st.ab_start = None;
            st.ab_end = None;
            st.sync_panel_model();
        }
        st.refresh_menu();
    }
    sync_sources(&state.borrow(), ui);
    update_active_ui(ui, &mut state.borrow_mut());
}

fn arm_chrome_hide(timer: &Timer, weak: &Weak<MainWindow>) {
    let weak = weak.clone();
    timer.start(TimerMode::SingleShot, CHROME_HIDE_DELAY, move || {
        if let Some(ui) = weak.upgrade() {
            if !ui.get_menu_open() && (ui.get_autohide_controls() || ui.get_video_skin()) {
                ui.set_chrome_visible(false);
            }
        }
    });
}

fn toggle_autohide_controls(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.autohide_controls = !st.config.autohide_controls;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.autohide_controls
    };
    ui.set_autohide_controls(enabled);
    if !enabled {
        ui.set_chrome_visible(true);
    }
    show_notice(
        state,
        ui,
        format!("视频下自动隐藏：{}", if enabled { "开" } else { "关" }),
    );
}

fn toggle_ignore_shortcuts(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.ignore_shortcuts = !st.config.ignore_shortcuts;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.ignore_shortcuts
    };
    ui.set_ignore_shortcuts(enabled);
    show_notice(
        state,
        ui,
        format!("忽略默认快捷键：{}", if enabled { "开" } else { "关" }),
    );
}

fn toggle_favorites_memory_points(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.favorites_memory_points = !st.config.favorites_memory_points;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.favorites_memory_points
    };
    ui.set_favorites_memory_points(enabled);
    refresh_favorite_menu(state, ui);
    show_notice(
        state,
        ui,
        format!(
            "在收藏菜单中显示播放记忆点：{}",
            if enabled { "开" } else { "关" }
        ),
    );
}

fn set_similar_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: &str) {
    {
        let mut st = state.borrow_mut();
        st.config.similar_mode = mode.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_similar_mode(mode.into());
    let label = match mode {
        "similar" => "同时打开相似文件",
        "all" => "同时打开全部文件",
        _ => "仅打开选定的文件",
    };
    show_notice(state, ui, format!("相似文件操作：{label}"));
}

fn toggle_touch_enabled(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.touch_enabled = !st.config.touch_enabled;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.touch_enabled
    };
    ui.set_touch_enabled(enabled);
    show_notice(
        state,
        ui,
        format!("使用触屏功能：{}", if enabled { "开" } else { "关" }),
    );
}

fn toggle_move_only_titlebar(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.move_only_titlebar = !st.config.move_only_titlebar;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.move_only_titlebar
    };
    ui.set_move_only_titlebar(enabled);
    show_notice(
        state,
        ui,
        format!(
            "仅在标题栏上移动窗口：{}",
            if enabled { "开" } else { "关" }
        ),
    );
}

fn shutdown_label(mode: &str) -> &'static str {
    match mode {
        "suspend" => "待机",
        "hibernate" => "休眠",
        _ => "关机",
    }
}

fn set_shutdown_mode(mode: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.config.shutdown_mode = mode.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_shutdown_mode(mode.into());
    let label = match mode {
        "suspend" => "使用待机方式替代关机",
        "hibernate" => "使用休眠方式替代关机",
        _ => "使用默认关机",
    };
    show_notice(state, ui, format!("收尾处理：{label}"));
}

fn toggle_remember_post_action(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.remember_post_action = !st.config.remember_post_action;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.remember_post_action
    };
    ui.set_remember_post_action(enabled);
    show_notice(
        state,
        ui,
        format!("记忆收尾操作：{}", if enabled { "开" } else { "关" }),
    );
}

fn ask_shutdown(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let mode = state.borrow().config.shutdown_mode.clone();
    let label = shutdown_label(&mode);
    ui.set_shutdown_prompt_text(format!("播放已结束，是否立即{label}？").into());
    ui.set_shutdown_prompt_open(true);
}

fn perform_shutdown(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let mode = state.borrow().config.shutdown_mode.clone();
    let action = match mode.as_str() {
        "suspend" => "suspend",
        "hibernate" => "hibernate",
        _ => "poweroff",
    };
    let label = shutdown_label(&mode);
    match Command::new("systemctl").arg(action).output() {
        Ok(output) if output.status.success() => {
            log::info!("已请求{label}");
            quit_app(state, ui);
        }
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            show_notice(state, ui, format!("{label}失败：{detail}"));
        }
        Err(error) => show_notice(state, ui, format!("{label}失败：{error}")),
    }
}

fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

fn profile_names() -> Vec<String> {
    let mut names = vec!["默认配置".to_string()];
    let Ok(entries) = std::fs::read_dir(profiles_dir()) else {
        return names;
    };
    let mut stems: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|ext| ext == "ini").unwrap_or(false))
        .filter_map(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
        })
        .collect();
    stems.sort_by_key(|name| name.to_lowercase());
    names.extend(stems);
    names
}

fn refresh_config_profiles(ui: &MainWindow) {
    let names = profile_names();
    let model: VecModel<slint::SharedString> = VecModel::from(
        names
            .iter()
            .map(|name| name.as_str().into())
            .collect::<Vec<_>>(),
    );
    ui.set_config_profiles(ModelRc::new(model));
    let selected = ui.get_config_selected().max(0) as usize;
    if selected >= names.len() {
        ui.set_config_selected(names.len().saturating_sub(1) as i32);
    }
}

fn open_config_manager(ui: &MainWindow) {
    refresh_config_profiles(ui);
    ui.set_config_selected(0);
    ui.set_config_status("".into());
    ui.set_config_rename_text("默认配置".into());
    ui.set_config_manager_open(true);
}

fn selected_profile(ui: &MainWindow) -> Option<(usize, String)> {
    let index = ui.get_config_selected().max(0) as usize;
    ui.get_config_profiles()
        .row_data(index)
        .map(|name| (index, name.to_string()))
}

fn create_config_profile(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let directory = profiles_dir();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        ui.set_config_status(format!("创建配置目录失败：{error}").into());
        return;
    }
    let existing = profile_names();
    let mut number = 1;
    let name = loop {
        let candidate = format!("配置 {number}");
        if !existing.contains(&candidate) {
            break candidate;
        }
        number += 1;
    };
    let result = {
        state
            .borrow()
            .config
            .save_to(&directory.join(format!("{name}.ini")))
    };
    match result {
        Ok(()) => {
            refresh_config_profiles(ui);
            if let Some(index) = profile_names().iter().position(|item| item == &name) {
                ui.set_config_selected(index as i32);
            }
            ui.set_config_rename_text(name.as_str().into());
            ui.set_config_status(format!("已新建配置：{name}").into());
        }
        Err(error) => ui.set_config_status(format!("新建配置失败：{error}").into()),
    }
}

fn rename_config_profile(ui: &MainWindow, new_name: &str) {
    let Some((index, old_name)) = selected_profile(ui) else {
        return;
    };
    if index == 0 {
        ui.set_config_status("默认配置无法重命名".into());
        return;
    }
    let clean = new_name.trim();
    if clean.is_empty() || clean == "默认配置" || clean.contains(['/', '\\']) {
        ui.set_config_status("配置名称无效".into());
        return;
    }
    let target = profiles_dir().join(format!("{clean}.ini"));
    if target.exists() {
        ui.set_config_status(format!("已存在同名配置：{clean}").into());
        return;
    }
    let source = profiles_dir().join(format!("{old_name}.ini"));
    match std::fs::rename(&source, &target) {
        Ok(()) => {
            refresh_config_profiles(ui);
            if let Some(index) = profile_names().iter().position(|item| item == clean) {
                ui.set_config_selected(index as i32);
            }
            ui.set_config_status(format!("已重命名：{old_name} → {clean}").into());
        }
        Err(error) => ui.set_config_status(format!("重命名失败：{error}").into()),
    }
}

fn delete_config_profile(ui: &MainWindow) {
    let Some((index, name)) = selected_profile(ui) else {
        return;
    };
    if index == 0 {
        ui.set_config_status("默认配置无法删除".into());
        return;
    }
    match std::fs::remove_file(profiles_dir().join(format!("{name}.ini"))) {
        Ok(()) => {
            refresh_config_profiles(ui);
            ui.set_config_selected(0);
            ui.set_config_rename_text("默认配置".into());
            ui.set_config_status(format!("已删除配置：{name}").into());
        }
        Err(error) => ui.set_config_status(format!("删除失败：{error}").into()),
    }
}

fn load_config_profile(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Some((index, name)) = selected_profile(ui) else {
        return;
    };
    let path = if index == 0 {
        config_path()
    } else {
        profiles_dir().join(format!("{name}.ini"))
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            ui.set_config_status(format!("载入配置失败：{error}").into());
            return;
        }
    };
    let mut loaded = Config::parse(&text);
    if !loaded.remember_post_action {
        loaded.post_action = "none".into();
    }
    {
        let mut st = state.borrow_mut();
        st.config = loaded;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    apply_config_to_ui(state, ui);
    ui.set_config_rename_text(name.as_str().into());
    ui.set_config_status(format!("已载入配置：{name}").into());
    show_notice(state, ui, format!("已载入配置：{name}"));
}

fn open_burst_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (interval, count, directory) = {
        let st = state.borrow();
        (
            st.config.snapshot_interval_ms,
            st.config.snapshot_count,
            st.config.snapshot_dir.display().to_string(),
        )
    };
    ui.set_burst_interval(interval.to_string().into());
    ui.set_burst_count(count.to_string().into());
    ui.set_burst_status(format!("输出目录：{directory}").into());
    ui.set_burst_open(true);
}

fn stop_burst(state: &Rc<RefCell<AppState>>) {
    if let Some(timer) = state.borrow_mut().burst_timer.take() {
        timer.stop();
    }
}

fn start_burst(state: &Rc<RefCell<AppState>>, ui: &MainWindow, interval_ms: u64, count: i32) {
    let interval_ms = interval_ms.clamp(100, 60000);
    let count = count.clamp(1, 100);
    let has_frame = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.last_frame.is_some())
            .unwrap_or(false)
    };
    if !has_frame {
        ui.set_burst_status("当前没有可截取的画面".into());
        show_notice(state, ui, "当前没有可截取的画面".to_string());
        return;
    }
    {
        let mut st = state.borrow_mut();
        st.config.snapshot_interval_ms = interval_ms;
        st.config.snapshot_count = count;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_burst_interval(interval_ms.to_string().into());
    ui.set_burst_count(count.to_string().into());
    ui.set_burst_open(false);
    stop_burst(state);
    show_notice(
        state,
        ui,
        format!("连续截图开始：{count} 张，间隔 {interval_ms} ms"),
    );
    let timer = Rc::new(Timer::default());
    {
        let mut st = state.borrow_mut();
        st.burst_remaining = count;
        st.burst_total = count;
        st.burst_timer = Some(timer.clone());
    }
    let weak = ui.as_weak();
    let state = state.clone();
    let ticker = timer.clone();
    timer.start(
        TimerMode::Repeated,
        Duration::from_millis(interval_ms),
        move || {
            let Some(ui) = weak.upgrade() else {
                ticker.stop();
                return;
            };
            save_snapshot(&state, &ui);
            let finished = {
                let mut st = state.borrow_mut();
                st.burst_remaining -= 1;
                if st.burst_remaining <= 0 {
                    if let Some(active) = st.burst_timer.take() {
                        active.stop();
                    }
                    true
                } else {
                    false
                }
            };
            if finished {
                ticker.stop();
                let total = state.borrow().burst_total;
                show_notice(&state, &ui, format!("连续截图完成：{total} 张"));
            }
        },
    );
}

fn open_path_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, target: PathDialogTarget) {
    let (title, hint, directory) = {
        let st = state.borrow();
        match target {
            PathDialogTarget::Snapshot => (
                crate::i18n::tr("截图存档路径"),
                crate::i18n::tr("输入截图保存目录，按回车确认"),
                st.config.snapshot_dir.display().to_string(),
            ),
            PathDialogTarget::VideoRecord => (
                crate::i18n::tr("视频录制存档路径"),
                crate::i18n::tr("输入视频录制保存目录，按回车确认"),
                st.config.video_record_dir.display().to_string(),
            ),
            PathDialogTarget::AudioRecord => (
                crate::i18n::tr("声音录制存档文件夹"),
                crate::i18n::tr("输入声音录制保存目录，按回车确认"),
                st.config.audio_record_dir.display().to_string(),
            ),
        }
    };
    state.borrow_mut().path_dialog_target = target;
    ui.set_path_dialog_title(title.into());
    ui.set_path_dialog_hint(hint.into());
    ui.set_path_dialog_text(directory.into());
    ui.set_path_dialog_open(true);
}

fn apply_path_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &str) {
    let target = state.borrow().path_dialog_target;
    let clean = path.trim();
    if clean.is_empty() {
        show_notice(state, ui, "存档路径不能为空".to_string());
        return;
    }
    let directory = PathBuf::from(clean);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        show_notice(state, ui, format!("创建存档目录失败：{error}"));
        return;
    }
    {
        let mut st = state.borrow_mut();
        match target {
            PathDialogTarget::Snapshot => st.config.snapshot_dir = directory.clone(),
            PathDialogTarget::VideoRecord => st.config.video_record_dir = directory.clone(),
            PathDialogTarget::AudioRecord => st.config.audio_record_dir = directory.clone(),
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    let label = match target {
        PathDialogTarget::Snapshot => "截图",
        PathDialogTarget::VideoRecord => "视频录制",
        PathDialogTarget::AudioRecord => "声音录制",
    };
    show_notice(
        state,
        ui,
        format!("{label}存档路径：{}", directory.display()),
    );
}

fn snapshot_directory(st: &AppState) -> PathBuf {
    let source = st
        .active
        .and_then(|index| st.panels.get(index))
        .map(|panel| panel.source.clone());
    match source {
        Some(source) if st.config.snapshot_play_folder => Path::new(&source)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| st.config.snapshot_dir.clone()),
        _ => st.config.snapshot_dir.clone(),
    }
}

fn open_thumbnail_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (width, directory, has_frame) = {
        let st = state.borrow();
        let has_frame = st
            .active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.last_frame.is_some())
            .unwrap_or(false);
        (
            st.config.thumbnail_width,
            snapshot_directory(&st),
            has_frame,
        )
    };
    ui.set_thumbnail_width(width.to_string().into());
    ui.set_thumbnail_directory(directory.display().to_string().into());
    ui.set_thumbnail_status(
        if has_frame {
            "使用当前画面，保存为 <视频名>_thumb.<格式>"
        } else {
            "当前没有可截取的画面"
        }
        .into(),
    );
    ui.set_thumbnail_open(true);
}

fn create_thumbnail(state: &Rc<RefCell<AppState>>, ui: &MainWindow, width: &str) {
    let width = width.trim().parse::<u32>().unwrap_or(320).clamp(64, 3840);
    let capture = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| {
                panel
                    .last_frame
                    .clone()
                    .map(|frame| (frame, panel.source.clone(), snapshot_directory(&st)))
            })
            .map(|(frame, source, directory)| {
                (frame, source, directory, st.config.snapshot_format.clone())
            })
    };
    let Some((frame, source, directory, format)) = capture else {
        ui.set_thumbnail_status("当前没有可截取的画面".into());
        show_notice(state, ui, "当前没有可截取的画面".to_string());
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.config.thumbnail_width = width;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    let extension = match format.as_str() {
        "jpeg" => "jpg",
        "bmp" => "bmp",
        _ => "png",
    };
    let stem = Path::new(&source)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("flintplayer")
        .to_string();
    let mut path = directory.join(format!("{stem}_thumb.{extension}"));
    if path.exists() {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        path = directory.join(format!("{stem}_thumb_{millis}.{extension}"));
    }
    ui.set_thumbnail_open(false);
    show_notice(state, ui, "正在创建缩略图…".to_string());
    let notices = state.borrow().notice_tx.clone();
    thread::spawn(move || {
        let result = (|| -> Result<PathBuf, String> {
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("创建缩略图目录失败：{error}"))?;
            let source_image = image::RgbaImage::from_raw(
                frame.width as u32,
                frame.height as u32,
                frame.rgba.clone(),
            )
            .ok_or_else(|| "缩略图缓冲尺寸无效".to_string())?;
            let width = width.min(source_image.width()).max(1);
            let height = ((source_image.height() as f32 * width as f32
                / source_image.width() as f32)
                .round() as u32)
                .max(1);
            let resized = image::DynamicImage::ImageRgba8(source_image).resize_exact(
                width,
                height,
                image::imageops::FilterType::Lanczos3,
            );
            if extension == "jpg" {
                resized
                    .to_rgb8()
                    .save(&path)
                    .map_err(|error| format!("保存缩略图失败：{error}"))?;
            } else {
                resized
                    .save(&path)
                    .map_err(|error| format!("保存缩略图失败：{error}"))?;
            }
            Ok(path)
        })();
        match result {
            Ok(path) => {
                log::info!("缩略图已保存：{}", path.display());
                let _ = notices.send(format!("缩略图已保存：{}", path.display()));
            }
            Err(error) => {
                log::warn!("{error}");
                let _ = notices.send(error);
            }
        }
    });
}

fn touch_seek_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize, fraction: f32) {
    let (target, duration, active) = {
        let st = state.borrow();
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        if !panel.running || !is_file_source(&panel.source) {
            return;
        }
        let duration = panel
            .shared
            .status()
            .info
            .as_ref()
            .map(|info| info.duration)
            .unwrap_or(0.0);
        if duration <= 0.0 {
            return;
        }
        let position = panel.engine.lock().position_seconds();
        let delta = duration * fraction.clamp(-1.0, 1.0) as f64;
        let target = (position + delta).clamp(0.0, (duration - 0.05).max(0.0));
        (target, duration, st.active == Some(index))
    };
    let panel = {
        let st = state.borrow();
        st.panels.get(index).map(|panel| panel.engine.clone())
    };
    let Some(engine) = panel else {
        return;
    };
    engine.lock().seek_to(target);
    if active {
        ui.set_bar_progress((target / duration) as f32);
    }
}

fn toggle_fullscreen_no_cover(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.fullscreen_no_cover = !st.config.fullscreen_no_cover;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.fullscreen_no_cover
    };
    ui.set_fullscreen_no_cover(enabled);
    show_notice(
        state,
        ui,
        format!("全屏时不遮盖画面：{}", if enabled { "开" } else { "关" }),
    );
}

fn show_chrome(ui: &MainWindow, hide_timer: &Rc<Timer>) {
    ui.set_chrome_visible(true);
    arm_chrome_hide(hide_timer, &ui.as_weak());
}

fn toggle_device_menu(ui: &MainWindow, hide_timer: &Rc<Timer>) {
    let open = !ui.get_menu_open();
    ui.set_menu_open(open);
    ui.set_chrome_visible(true);
    if !open {
        arm_chrome_hide(hide_timer, &ui.as_weak());
    }
}

fn set_osd_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: &str) {
    let mut st = state.borrow_mut();
    st.osd_mode = mode.to_string();
    st.osd_visible = mode == "full";
    ui.set_osd_visible(st.osd_visible);
    ui.set_osd_brief(mode == "brief");
    ui.set_osd_playback(mode == "playback");
}

fn toggle_osd_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: &str) {
    let next = if state.borrow().osd_mode == mode {
        "off"
    } else {
        mode
    };
    set_osd_mode(state, ui, next);
}

fn toggle_freeze(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let mut restart = None;
    {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        if !panel.running {
            if panel.last_error.as_deref() == Some(flint_media::EOF_ERROR)
                && is_file_source(&panel.source)
            {
                restart = Some(index);
            }
        } else {
            if is_file_source(&panel.source) {
                let paused = !panel.paused;
                panel.paused = paused;
                panel.frozen = paused;
                panel.engine.lock().set_paused(paused);
            } else {
                panel.frozen = !panel.frozen;
            }
            let frozen = panel.frozen;
            let row = panel.row(true);
            st.panel_model.set_row_data(index, row);
            ui.set_frozen(frozen);
        }
    }
    if let Some(index) = restart {
        stop_panel(state, ui, index);
        start_panel(state, ui, index);
    }
}

fn is_file_source(source: &str) -> bool {
    !source.starts_with("/dev/video")
        && !flint_media::capture_stream::is_screen(source)
        && !source.contains("://")
}

fn source_format(source: &str) -> String {
    if !is_file_source(source) {
        return String::new();
    }
    Path::new(source)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_uppercase())
        .unwrap_or_default()
}

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "ts", "m2ts", "mpg", "mpeg", "wmv", "ogv", "m4v",
    "rmvb", "mp3", "flac", "wav", "aac", "ogg", "m4a", "opus",
];

struct FileCandidate {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: String,
}

fn default_media_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| MEDIA_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.2} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else {
        format!("{:.0} KB", (bytes / KB).max(1.0))
    }
}

const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "lrc", "txt"];

fn is_subtitle_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| SUBTITLE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq)]
enum FileFilter {
    Media,
    Subtitle,
    Skin,
    Image,
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "ico" | "tif" | "tiff"
            )
        })
        .unwrap_or(false)
}

fn is_skin_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case(skin::SKIN_EXTENSION))
        .unwrap_or(false)
}

fn read_file_candidates(dir: &Path, filter: FileFilter) -> Vec<FileCandidate> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            dirs.push(FileCandidate {
                name,
                path,
                is_dir: true,
                size: String::new(),
            });
        } else if match filter {
            FileFilter::Media => is_media_file(&path),
            FileFilter::Subtitle => is_subtitle_file(&path),
            FileFilter::Skin => is_skin_file(&path),
            FileFilter::Image => is_image_file(&path),
        } {
            let size = item
                .metadata()
                .map(|meta| format_file_size(meta.len()))
                .unwrap_or_default();
            files.push(FileCandidate {
                name,
                path,
                is_dir: false,
                size,
            });
        }
    }
    dirs.sort_by_key(|candidate| candidate.name.to_lowercase());
    files.sort_by_key(|candidate| candidate.name.to_lowercase());
    dirs.extend(files);
    dirs
}

const ACCENTS: &[(&str, &str, u32)] = &[
    ("blue", "蓝色", 0x4aa3ff),
    ("red", "红色", 0xe5534b),
    ("green", "绿色", 0x3fbf5f),
    ("purple", "紫色", 0x9a6bff),
    ("orange", "橙色", 0xff9f43),
    ("cyan", "青色", 0x2fd3d3),
    ("silver", "银色", 0x9a9a9a),
];

fn accent_value(name: &str) -> Option<slint::Color> {
    ACCENTS
        .iter()
        .find(|(key, _, _)| *key == name)
        .map(|(_, _, value)| {
            slint::Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, *value as u8)
        })
}

fn accent_label(name: &str) -> &'static str {
    ACCENTS
        .iter()
        .find(|(key, _, _)| *key == name)
        .map(|(_, label, _)| *label)
        .unwrap_or("自动(跟随皮肤)")
}

fn skin_menu_leaves(library: &[Skin], config: &Config) -> Vec<MenuLeaf> {
    let selected_id = config.skin.as_str();
    let selected = skin_index(library, selected_id);
    let builtin_count = skin::builtin_skins().len().min(library.len());
    let mut leaves = vec![menu_leaf("［内置皮肤］", "", false, "")];
    for (index, skin) in library.iter().take(builtin_count).enumerate() {
        leaves.push(skin_menu_item(skin, index, selected));
    }
    leaves.push(menu_leaf("［外部皮肤］", "", false, ""));
    if library.len() > builtin_count {
        for (index, skin) in library.iter().enumerate().skip(builtin_count) {
            leaves.push(skin_menu_item(skin, index, selected));
        }
    } else {
        leaves.push(menu_leaf(
            "（无外部皮肤，用“浏览皮肤...”载入）",
            "",
            false,
            "",
        ));
    }
    leaves.push(menu_separator());
    let mut accent_items = vec![menu_sub_check(
        "自动(跟随皮肤)",
        config.menu_accent.is_empty(),
        "accent-auto",
    )];
    for (key, label, _) in ACCENTS {
        accent_items.push(menu_sub_check(
            label,
            config.menu_accent == *key,
            &format!("accent-{key}"),
        ));
    }
    leaves.push(menu_group("配色", true, accent_items));
    let selected_opacity = (config.skin_opacity * 100.0).round() as i32;
    let opacity_items = [40, 50, 60, 70, 80, 90, 100]
        .into_iter()
        .map(|percent| {
            menu_sub_check(
                &format!("{percent}%"),
                selected_opacity == percent,
                &format!("skin-opacity-set-{percent}"),
            )
        })
        .collect();
    leaves.push(menu_group("不透明度", true, opacity_items));
    leaves.push(menu_separator());
    leaves.push(menu_leaf("浏览皮肤...", "", true, "skin-browse-menu"));
    leaves.push(menu_leaf("皮肤设置...", "", true, "skin-settings"));
    leaves.push(menu_separator());
    let common_logo_items = vec![
        menu_sub(
            if config.logo_file.is_empty() {
                "选择图片..."
            } else {
                "更换图片..."
            },
            "",
            true,
            "logo-common-pick",
        ),
        menu_sub_check(
            "显示常用徽标",
            !config.logo_file.is_empty() && config.logo_show,
            "logo-common-toggle",
        ),
        menu_sub(
            "清除徽标",
            "",
            !config.logo_file.is_empty(),
            "logo-common-clear",
        ),
    ];
    leaves.push(menu_group("常用徽标", true, common_logo_items));
    let play_logo_items = vec![
        menu_sub(
            if config.logo_play_file.is_empty() {
                "选择图片..."
            } else {
                "更换图片..."
            },
            "",
            true,
            "logo-play-pick",
        ),
        menu_sub_check(
            "显示播放时徽标",
            !config.logo_play_file.is_empty() && config.logo_play_show,
            "logo-play-toggle",
        ),
        menu_sub(
            "清除徽标",
            "",
            !config.logo_play_file.is_empty(),
            "logo-play-clear",
        ),
    ];
    leaves.push(menu_group("播放时的徽标", true, play_logo_items));
    leaves.push(menu_separator());
    leaves.push(menu_check(
        "图层面板皮肤输出",
        config.skin_layer_output,
        "skin-layer-toggle",
    ));
    let popup_items = vec![
        menu_sub_check(
            "皮肤中定义的菜单",
            config.popup_menu_type == "skin",
            "popup-menu-type-skin",
        ),
        menu_sub_check(
            "系统菜单",
            config.popup_menu_type == "system",
            "popup-menu-type-system",
        ),
    ];
    leaves.push(menu_group("弹出菜单类型", true, popup_items));
    let output_items = vec![
        menu_sub_check(
            "自动选择(推荐)",
            config.menu_output_mode == "auto",
            "menu-output-auto",
        ),
        menu_sub_check(
            "黑暗系统菜单",
            config.menu_output_mode == "dark",
            "menu-output-dark",
        ),
    ];
    leaves.push(menu_group("菜单输出方式", true, output_items));
    leaves.push(menu_check(
        "查看菜单名称",
        config.menu_show_names,
        "menu-show-names-toggle",
    ));
    leaves.push(menu_check(
        "放大皮肤中定义的菜单",
        config.enlarge_skin_menu,
        "enlarge-skin-menu-toggle",
    ));
    leaves.push(menu_check(
        "不切换界面模式",
        config.keep_ui_mode,
        "keep-ui-mode-toggle",
    ));
    leaves.push(menu_check(
        "单独保存音频/视频界面位置/尺寸",
        config.separate_geometry,
        "separate-geometry-toggle",
    ));
    leaves.push(menu_check(
        "触控皮肤",
        config.touch_skin,
        "touch-skin-toggle",
    ));
    leaves.push(menu_check(
        "以视频皮肤启动",
        config.video_skin_start,
        "video-skin-start-toggle",
    ));
    leaves
}

fn set_menu_accent(state: &Rc<RefCell<AppState>>, ui: &MainWindow, name: &str) {
    let name = if name == "auto" { "" } else { name };
    if !name.is_empty() && accent_value(name).is_none() {
        return;
    }
    let (library, skin_id) = {
        let mut st = state.borrow_mut();
        st.config.menu_accent = name.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        (st.skin_library.clone(), st.config.skin.clone())
    };
    match accent_value(name) {
        Some(color) => ui.set_skin_accent(color),
        None => apply_skin_id(ui, &library, &skin_id),
    }
    show_notice(state, ui, format!("配色：{}", accent_label(name)));
}

fn set_skin_opacity_percent(state: &Rc<RefCell<AppState>>, ui: &MainWindow, percent: i32) {
    let opacity = (percent.clamp(40, 100) as f32) / 100.0;
    {
        let mut st = state.borrow_mut();
        st.config.skin_opacity = opacity;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_skin_opacity(opacity);
    show_notice(state, ui, format!("菜单不透明度：{percent}%"));
}

fn toggle_skin_layer_output(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.skin_layer_output = !st.config.skin_layer_output;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.skin_layer_output
    };
    ui.set_panel_skin_output(enabled);
    show_notice(
        state,
        ui,
        format!(
            "图层面板皮肤输出：{}",
            if enabled { "开启" } else { "关闭" }
        ),
    );
}

fn set_popup_menu_type(state: &Rc<RefCell<AppState>>, ui: &MainWindow, value: &str) {
    let value = if value == "system" { "system" } else { "skin" };
    {
        let mut st = state.borrow_mut();
        st.config.popup_menu_type = value.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_popup_menu_type(value.into());
    show_notice(
        state,
        ui,
        format!(
            "弹出菜单类型：{}",
            if value == "system" {
                "系统菜单"
            } else {
                "皮肤中定义的菜单"
            }
        ),
    );
}

fn set_menu_output_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, value: &str) {
    let value = if value == "dark" { "dark" } else { "auto" };
    {
        let mut st = state.borrow_mut();
        st.config.menu_output_mode = value.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_menu_output_mode(value.into());
    show_notice(
        state,
        ui,
        format!(
            "菜单输出方式：{}",
            if value == "dark" {
                "黑暗系统菜单"
            } else {
                "自动选择"
            }
        ),
    );
}

fn menu_scale_for(config: &Config) -> f32 {
    if config.touch_skin || config.enlarge_skin_menu {
        1.25
    } else {
        1.0
    }
}

fn toggle_touch_skin(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.touch_skin = !st.config.touch_skin;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.touch_skin
    };
    ui.set_menu_scale(menu_scale_for(&state.borrow().config));
    show_notice(
        state,
        ui,
        format!("触控皮肤：{}", if enabled { "开启" } else { "关闭" }),
    );
}

fn toggle_enlarge_skin_menu(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.enlarge_skin_menu = !st.config.enlarge_skin_menu;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.enlarge_skin_menu
    };
    ui.set_menu_scale(menu_scale_for(&state.borrow().config));
    show_notice(
        state,
        ui,
        format!(
            "放大皮肤中定义的菜单：{}",
            if enabled { "开启" } else { "关闭" }
        ),
    );
}

fn toggle_menu_show_names(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.menu_show_names = !st.config.menu_show_names;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.menu_show_names
    };
    ui.global::<UiOptions>().set_menu_show_names(enabled);
    show_notice(
        state,
        ui,
        format!("查看菜单名称：{}", if enabled { "开启" } else { "关闭" }),
    );
}

fn toggle_keep_ui_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.keep_ui_mode = !st.config.keep_ui_mode;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.keep_ui_mode
    };
    show_notice(
        state,
        ui,
        format!("不切换界面模式：{}", if enabled { "开启" } else { "关闭" }),
    );
}

fn toggle_separate_geometry(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.separate_geometry = !st.config.separate_geometry;
        if st.config.separate_geometry {
            let size = ui.window().size();
            let position = ui.window().position();
            record_mode_geometry(&mut st.config, ui.get_video_skin(), size, position);
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.separate_geometry
    };
    show_notice(
        state,
        ui,
        format!(
            "单独保存音频/视频界面位置/尺寸：{}",
            if enabled { "开启" } else { "关闭" }
        ),
    );
}

fn record_mode_geometry(
    config: &mut Config,
    video: bool,
    size: slint::PhysicalSize,
    position: slint::PhysicalPosition,
) {
    let (window_w, window_h, window_x, window_y) = (
        Some(size.width as f32),
        Some(size.height as f32),
        Some(position.x as f32),
        Some(position.y as f32),
    );
    if video {
        config.video_window_w = window_w;
        config.video_window_h = window_h;
        config.video_window_x = window_x;
        config.video_window_y = window_y;
    } else {
        config.audio_window_w = window_w;
        config.audio_window_h = window_h;
        config.audio_window_x = window_x;
        config.audio_window_y = window_y;
    }
}

fn apply_mode_geometry(ui: &MainWindow, config: &Config, video: bool) {
    let (window_w, window_h, window_x, window_y) = if video {
        (
            config.video_window_w,
            config.video_window_h,
            config.video_window_x,
            config.video_window_y,
        )
    } else {
        (
            config.audio_window_w,
            config.audio_window_h,
            config.audio_window_x,
            config.audio_window_y,
        )
    };
    if let (Some(width), Some(height)) = (window_w, window_h) {
        ui.window()
            .set_size(slint::PhysicalSize::new(width as u32, height as u32));
    }
    if let (Some(x), Some(y)) = (window_x, window_y) {
        ui.window()
            .set_position(slint::PhysicalPosition::new(x as i32, y as i32));
    }
}

fn switch_ui_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, video: bool) {
    if ui.get_video_skin() == video {
        return;
    }
    let separate = state.borrow().config.separate_geometry;
    if separate {
        let size = ui.window().size();
        let position = ui.window().position();
        let mut st = state.borrow_mut();
        record_mode_geometry(&mut st.config, ui.get_video_skin(), size, position);
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_video_skin(video);
    ui.set_chrome_visible(!video);
    if separate {
        let st = state.borrow();
        apply_mode_geometry(ui, &st.config, video);
    }
}

fn toggle_video_skin_start(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.video_skin_start = !st.config.video_skin_start;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.video_skin_start
    };
    switch_ui_mode(state, ui, enabled);
    show_notice(
        state,
        ui,
        format!("以视频皮肤启动：{}", if enabled { "开启" } else { "关闭" }),
    );
}

fn set_language(state: &Rc<RefCell<AppState>>, ui: &MainWindow, code: &str) {
    if !matches!(code, "auto" | "zh" | "en") {
        return;
    }
    {
        let mut st = state.borrow_mut();
        st.config.language = code.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.subtitle_menu_key.clear();
        st.skin_menu_key.clear();
        st.audio_menu_key.clear();
        st.filter_menu_key.clear();
    }
    crate::i18n::set_language(crate::i18n::Language::from_code(code));
    ui.set_language(code.into());
    let translation = ui.global::<Translation>();
    translation.set_language_gen(translation.get_language_gen() + 1);
    refresh_play_menu(state, ui);
    refresh_favorite_menu(state, ui);
    let label = match crate::i18n::language() {
        crate::i18n::Language::Zh => "简体中文",
        crate::i18n::Language::En => "English",
    };
    show_notice(state, ui, format!("界面语言：{label}"));
}

fn skin_menu_item(skin: &Skin, index: usize, selected: i32) -> MenuLeaf {
    menu_check(
        &skin.name,
        index as i32 == selected,
        &format!("skin-menu-select-{index}"),
    )
}

fn select_skin_from_menu(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let Some(skin) = state.borrow().skin_library.get(index).cloned() else {
        return;
    };
    let id = skin.id();
    apply_skin(ui, &skin);
    if !state.borrow().config.keep_ui_mode {
        switch_ui_mode(state, ui, skin.video_mode);
    }
    let library = {
        let mut st = state.borrow_mut();
        st.config.skin = id.clone();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.skin_library.clone()
    };
    sync_skin_rows(ui, &library, &id);
    show_notice(state, ui, format!("皮肤：{}", skin.name));
}

fn skin_rows(library: &[Skin], selected: i32) -> Vec<SkinRow> {
    let builtin_count = skin::builtin_skins().len().min(library.len());
    let mut rows = Vec::with_capacity(library.len() + 3);
    rows.push(SkinRow {
        label: "［内置皮肤］".into(),
        kind: 0,
        index: -1,
    });
    for (index, skin) in library.iter().take(builtin_count).enumerate() {
        rows.push(skin_row(skin, index, selected));
    }
    rows.push(SkinRow {
        label: "［外部皮肤］".into(),
        kind: 0,
        index: -1,
    });
    if library.len() > builtin_count {
        for (index, skin) in library.iter().enumerate().skip(builtin_count) {
            rows.push(skin_row(skin, index, selected));
        }
    } else {
        rows.push(SkinRow {
            label: "将 .fskin 文件放入皮肤目录后点击“浏览...”载入".into(),
            kind: 2,
            index: -1,
        });
    }
    rows
}

fn skin_row(skin: &Skin, index: usize, selected: i32) -> SkinRow {
    let label = if index as i32 == selected {
        format!("✓ {}", skin.name)
    } else {
        format!("　{}", skin.name)
    };
    SkinRow {
        label: label.into(),
        kind: 1,
        index: index as i32,
    }
}

fn skin_index(library: &[Skin], id: &str) -> i32 {
    skin::find_skin(library, id)
        .and_then(|skin| library.iter().position(|item| item.id() == skin.id()))
        .map(|index| index as i32)
        .unwrap_or(-1)
}

fn sync_skin_rows(ui: &MainWindow, library: &[Skin], id: &str) {
    let selected = skin_index(library, id);
    ui.set_skin_selected(selected);
    ui.set_skin_rows(ModelRc::new(VecModel::from(skin_rows(library, selected))));
}

fn apply_skin(ui: &MainWindow, skin: &Skin) {
    ui.set_skin_accent(skin.accent);
    ui.set_skin_menu_bg(skin.menu_bg);
    ui.set_skin_menu_hover(skin.menu_hover);
}

fn apply_skin_id(ui: &MainWindow, library: &[Skin], id: &str) {
    if let Some(skin) = skin::find_skin(library, id) {
        apply_skin(ui, skin);
    } else {
        let builtin = skin::builtin_skins();
        apply_skin(ui, skin::find_skin(&builtin, id).unwrap_or(&builtin[0]));
    }
}

fn open_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    show_file_dialog(state, ui, false);
}

fn show_open_url_dialog(ui: &MainWindow) {
    ui.set_open_url_open(true);
}

fn set_default_open_action(action: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    if state.borrow().config.default_open_action == action {
        return;
    }
    let label = match action {
        "file" => "打开文件",
        "url" => "打开链接",
        "folder" => "打开文件夹",
        _ => "简索",
    };
    state.borrow_mut().config.default_open_action = action.to_string();
    ui.set_default_open_action(action.into());
    if let Err(error) = state.borrow().config.save() {
        log::warn!("保存首选打开方式失败：{error}");
    }
    show_notice(state, ui, format!("默认打开：{label}"));
}

fn add_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    show_file_dialog(state, ui, true);
}

fn show_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, append: bool) {
    let dir = {
        let st = state.borrow();
        if st.file_dir.as_os_str().is_empty() {
            default_media_dir()
        } else {
            st.file_dir.clone()
        }
    };
    load_file_dir(state, ui, dir);
    state.borrow_mut().append_files = append;
    state.borrow_mut().skin_pick = false;
    state.borrow_mut().subtitle_pick = false;
    state.borrow_mut().subtitle2_pick = false;
    state.borrow_mut().audio_pick = false;
    state.borrow_mut().logo_pick = 0;
    state.borrow_mut().script_pick = 0;
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn open_folder_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    show_folder_dialog(state, ui, false);
}

fn add_folder_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    show_folder_dialog(state, ui, true);
}

fn show_folder_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, append: bool) {
    let dir = {
        let st = state.borrow();
        if st.file_dir.as_os_str().is_empty() {
            default_media_dir()
        } else {
            st.file_dir.clone()
        }
    };
    load_file_dir(state, ui, dir);
    state.borrow_mut().append_files = append;
    state.borrow_mut().skin_pick = false;
    state.borrow_mut().subtitle_pick = false;
    state.borrow_mut().subtitle2_pick = false;
    state.borrow_mut().audio_pick = false;
    ui.set_file_folder_mode(true);
    ui.set_file_open(true);
}

fn show_subtitle_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = browser_dir(state);
    {
        let mut st = state.borrow_mut();
        st.skin_pick = false;
        st.subtitle_pick = true;
        st.subtitle2_pick = false;
    st.audio_pick = false;
        st.logo_pick = 0;
        st.script_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn show_secondary_subtitle_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = browser_dir(state);
    {
        let mut st = state.borrow_mut();
        st.skin_pick = false;
        st.subtitle_pick = true;
        st.subtitle2_pick = true;
        st.audio_pick = false;
        st.logo_pick = 0;
        st.script_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn show_audio_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = browser_dir(state);
    {
        let mut st = state.borrow_mut();
        st.skin_pick = false;
        st.subtitle_pick = false;
        st.subtitle2_pick = false;
        st.audio_pick = true;
        st.logo_pick = 0;
        st.script_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn load_external_audio(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    let Some(engine) = active_engine(state) else {
        show_notice(state, ui, "请先打开视频文件再载入音频".into());
        return;
    };
    ui.set_file_open(false);
    if !path.is_file() {
        return;
    }
    engine
        .lock()
        .request_external_audio(path.to_string_lossy().as_ref());
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    show_notice(state, ui, format!("正在载入外部音频：{name}"));
}

fn show_skin_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = skin::skin_dir();
    let _ = std::fs::create_dir_all(&dir);
    {
        let mut st = state.borrow_mut();
        st.skin_pick = true;
        st.subtitle_pick = false;
        st.subtitle2_pick = false;
    st.audio_pick = false;
        st.logo_pick = 0;
        st.script_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn show_logo_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u8) {
    let dir = browser_dir(state);
    {
        let mut st = state.borrow_mut();
        st.skin_pick = false;
        st.subtitle_pick = false;
        st.logo_pick = slot;
        st.script_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn load_logo_from_path(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u8, path: &Path) {
    let Some(image) = load_logo_image(&path.to_string_lossy()) else {
        show_notice(state, ui, format!("无法读取图片: {}", path.display()));
        return;
    };
    ui.set_file_open(false);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    {
        let mut st = state.borrow_mut();
        if slot == 1 {
            st.config.logo_file = path.to_string_lossy().to_string();
            st.config.logo_show = true;
            ui.set_logo_image(image);
            ui.set_logo_visible(true);
        } else {
            st.config.logo_play_file = path.to_string_lossy().to_string();
            st.config.logo_play_show = true;
            ui.set_play_logo_image(image);
            ui.set_play_logo_visible(true);
        }
        st.logo_pick = 0;
        st.script_pick = 0;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    let label = if slot == 1 {
        "常用徽标"
    } else {
        "播放时的徽标"
    };
    show_notice(state, ui, format!("{label}：{name}"));
}

fn toggle_logo(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u8) {
    let hidden = {
        let st = state.borrow();
        if slot == 1 {
            st.config.logo_file.is_empty()
        } else {
            st.config.logo_play_file.is_empty()
        }
    };
    if hidden {
        show_logo_file_dialog(state, ui, slot);
        return;
    }
    let show = {
        let mut st = state.borrow_mut();
        if slot == 1 {
            st.config.logo_show = !st.config.logo_show;
        } else {
            st.config.logo_play_show = !st.config.logo_play_show;
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        if slot == 1 {
            st.config.logo_show
        } else {
            st.config.logo_play_show
        }
    };
    if slot == 1 {
        ui.set_logo_visible(show);
    } else {
        ui.set_play_logo_visible(show);
    }
    show_notice(
        state,
        ui,
        format!(
            "{}：{}",
            if slot == 1 {
                "常用徽标"
            } else {
                "播放时的徽标"
            },
            if show { "显示" } else { "隐藏" }
        ),
    );
}

fn clear_logo(state: &Rc<RefCell<AppState>>, ui: &MainWindow, slot: u8) {
    {
        let mut st = state.borrow_mut();
        if slot == 1 {
            st.config.logo_file.clear();
        } else {
            st.config.logo_play_file.clear();
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    if slot == 1 {
        ui.set_logo_visible(false);
    } else {
        ui.set_play_logo_visible(false);
    }
    show_notice(state, ui, "已清除徽标".to_string());
}

fn load_logo_image(path: &str) -> Option<Image> {
    if path.trim().is_empty() {
        return None;
    }
    let image = image::open(path).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    let buffer = SharedPixelBuffer::clone_from_slice(image.as_raw(), width, height);
    Some(Image::from_rgba8(buffer))
}

fn load_skin_from_path(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    ui.set_file_open(false);
    let Some(skin) = skin::load_skin(path) else {
        show_notice(state, ui, format!("无法读取皮肤文件: {}", path.display()));
        return;
    };
    let id = skin.id();
    let video_mode = skin.video_mode;
    apply_skin(ui, &skin);
    if !state.borrow().config.keep_ui_mode {
        switch_ui_mode(state, ui, video_mode);
    }
    let library = {
        let mut st = state.borrow_mut();
        if !st.skin_library.iter().any(|item| item.id() == id) {
            st.skin_library.push(skin);
        }
        st.config.skin = id.clone();
        st.skin_pick = false;
        st.skin_library.clone()
    };
    sync_skin_rows(ui, &library, &id);
    ui.set_skin_settings_open(true);
}

fn load_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path, secondary: bool) {
    ui.set_file_open(false);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let track = match SubtitleTrack::load(path) {
        Ok(track) => track,
        Err(error) => {
            show_notice(state, ui, error);
            return;
        }
    };
    let count = track.cues.len();
    let loaded = {
        let mut st = state.borrow_mut();
        st.subtitle_pick = false;
        st.subtitle2_pick = false;
    st.audio_pick = false;
        match st.active {
            Some(index) => {
                if let Some(panel) = st.panels.get_mut(index) {
                    if secondary {
                        panel.subtitle2 = Some(track);
                    } else {
                        panel.subtitle = Some(track);
                        panel.subtitle_stream = None;
                        panel.subtitle_delay = 0.0;
                    }
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    };
    if !loaded {
        show_notice(state, ui, "没有可显示字幕的画面".to_string());
        return;
    }
    ui.set_subtitles_visible(true);
    ui.set_subtitle_loaded(true);
    let message = if secondary {
        format!("已加载次字幕 {name}（{count} 条）")
    } else {
        format!("已加载字幕 {name}（{count} 条）")
    };
    show_notice(state, ui, message);
}

fn reload_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let path = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| panel.subtitle.as_ref())
            .map(|track| track.path.clone())
    };
    let Some(path) = path else {
        show_notice(state, ui, "当前没有加载字幕".to_string());
        return;
    };
    load_subtitle(state, ui, &path, false);
}

fn download_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let url = ui.get_download_url().to_string();
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        show_notice(
            state,
            ui,
            "字幕地址必须以 http:// 或 https:// 开头".to_string(),
        );
        return;
    }
    let dir = config_dir().join("subtitles");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        show_notice(state, ui, format!("无法创建字幕目录：{error}"));
        return;
    }
    let mut name = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| "subtitle.srt".to_string());
    if !is_subtitle_file(Path::new(&name)) {
        name.push_str(".srt");
    }
    let mut dest = dir.join(&name);
    let mut counter = 1;
    while dest.exists() {
        let stem = Path::new(&name)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "subtitle".to_string());
        let extension = Path::new(&name)
            .extension()
            .map(|extension| extension.to_string_lossy().to_string())
            .unwrap_or_else(|| "srt".to_string());
        dest = dir.join(format!("{stem}-{counter}.{extension}"));
        counter += 1;
        if counter > 99 {
            break;
        }
    }
    show_notice(state, ui, "正在下载字幕...".to_string());
    let status = Command::new("curl")
        .args(["-s", "-L", "--fail", "--max-time", "20", "-o"])
        .arg(&dest)
        .arg(&url)
        .stdin(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() && dest.is_file() => {
            load_subtitle(state, ui, &dest, false);
        }
        _ => {
            let _ = std::fs::remove_file(&dest);
            show_notice(state, ui, "字幕下载失败".to_string());
        }
    }
}

fn toggle_subtitle_translation(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.translate_enabled = !st.config.translate_enabled;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.translate_pending.clear();
        st.translation.lock().take();
        st.config.translate_enabled
    };
    if !enabled {
        ui.set_subtitle_translation("".into());
    }
    show_notice(
        state,
        ui,
        format!("实时字幕翻译：{}", if enabled { "开启" } else { "关闭" }),
    );
}

fn translate_text(text: &str) -> Option<String> {
    let encoded: String = text
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect();
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=zh-CN&dt=t&q={encoded}"
    );
    let output = Command::new("curl")
        .args(["-s", "--max-time", "6", &url])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let start = body.find("[[[\"")? + 4;
    let mut translated = String::new();
    let mut chars = body[start..].chars();
    while let Some(character) = chars.next() {
        match character {
            '"' => return (!translated.is_empty()).then_some(translated),
            '\\' => match chars.next()? {
                'n' => translated.push('\n'),
                't' => translated.push('\t'),
                'r' => translated.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(character) =
                        u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                    {
                        translated.push(character);
                    }
                }
                other => translated.push(other),
            },
            other => translated.push(other),
        }
    }
    None
}

fn subtitle_language_label(language: &str) -> &'static str {
    match language {
        "chi" => "中文",
        "eng" => "英文",
        "jpn" => "日文",
        "kor" => "韩文",
        _ => "自动选择",
    }
}

fn language_matches(stream_language: &str, preference: &str) -> bool {
    let stream_language = stream_language.trim().to_ascii_lowercase();
    let matches = match preference {
        "chi" => &["chi", "zho", "zh", "chs", "cht"][..],
        "eng" => &["eng", "en"][..],
        "jpn" => &["jpn", "ja"][..],
        "kor" => &["kor", "ko"][..],
        _ => return false,
    };
    matches
        .iter()
        .any(|code| stream_language == *code || stream_language.starts_with(&format!("{code}-")))
}

fn set_subtitle_language(state: &Rc<RefCell<AppState>>, ui: &MainWindow, language: &str) {
    if !matches!(language, "auto" | "chi" | "eng" | "jpn" | "kor") {
        return;
    }
    {
        let mut st = state.borrow_mut();
        st.config.subtitle_language = language.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    auto_select_subtitle_for(state, ui, true);
    show_notice(
        state,
        ui,
        format!("字幕语言：{}", subtitle_language_label(language)),
    );
}

fn auto_select_subtitle_for(state: &Rc<RefCell<AppState>>, ui: &MainWindow, force: bool) {
    let stream_index = {
        let st = state.borrow();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        if st.config.subtitle_language == "auto" || (panel.subtitle.is_some() && !force) {
            return;
        }
        panel
            .subtitle_streams
            .iter()
            .find(|stream| language_matches(&stream.language, &st.config.subtitle_language))
            .map(|stream| stream.index)
    };
    if let Some(stream_index) = stream_index {
        load_embedded_subtitle(state, ui, stream_index);
    }
}

fn load_embedded_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow, stream_index: usize) {
    let payload = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| {
                (
                    panel.source.clone(),
                    panel
                        .subtitle_streams
                        .iter()
                        .find(|stream| stream.index == stream_index)
                        .map(SubtitleStreamInfo::label),
                )
            })
    };
    let Some((source, label)) = payload else {
        return;
    };
    let Some(label) = label else {
        return;
    };
    match embedded_subtitles::load_subtitle_stream(Path::new(&source), stream_index) {
        Ok(track) => {
            let count = track.cues.len();
            let loaded = {
                let mut st = state.borrow_mut();
                let Some(index) = st.active else {
                    return;
                };
                let Some(panel) = st.panels.get_mut(index) else {
                    return;
                };
                if panel.source != source {
                    return;
                }
                panel.subtitle = Some(track);
                panel.subtitle_stream = Some(stream_index);
                panel.subtitle_delay = 0.0;
                true
            };
            if loaded {
                ui.set_subtitles_visible(true);
                ui.set_subtitle_loaded(true);
                show_notice(state, ui, format!("已加载{label}（{count} 条）"));
            }
        }
        Err(error) => show_notice(state, ui, error),
    }
}

fn clear_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.subtitle = None;
        panel.subtitle2 = None;
        panel.subtitle_stream = None;
    }
    ui.set_subtitle_text("".into());
    ui.set_subtitle_loaded(false);
    show_notice(state, ui, "已关闭字幕".to_string());
}

fn toggle_subtitles(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let visible = !ui.get_subtitles_visible();
    ui.set_subtitles_visible(visible);
    if !visible {
        ui.set_subtitle_text("".into());
    }
    let loaded = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .is_some_and(|panel| panel.subtitle.is_some() || panel.subtitle2.is_some())
    };
    if !loaded {
        show_notice(state, ui, "尚未加载字幕，按 Alt+O 打开字幕文件".to_string());
    }
}

fn adjust_subtitle_delay(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: f64) {
    let delay = {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let loaded = st
            .panels
            .get(index)
            .is_some_and(|panel| panel.subtitle.is_some() || panel.subtitle2.is_some());
        if !loaded {
            drop(st);
            show_notice(state, ui, "尚未加载字幕".to_string());
            return;
        }
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.subtitle_delay = (panel.subtitle_delay + delta).clamp(-30.0, 30.0);
        panel.subtitle_delay
    };
    ui.set_subtitle_delay(delay as f32);
    show_notice(state, ui, format!("字幕延迟：{:+.1} 秒", delay));
}

fn set_subtitle_delay(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delay: f64) {
    {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.subtitle_delay = delay;
    }
    ui.set_subtitle_delay(delay as f32);
    show_notice(state, ui, format!("字幕延迟：{:+.1} 秒", delay));
}

fn set_subtitle_font(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let Some((name, family)) = SUBTITLE_FONTS.get(index) else {
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.config.subtitle_font = (*family).to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_subtitle_font_name((*name).into());
    ui.set_subtitle_font_family((*family).into());
    show_notice(state, ui, format!("字幕字体：{name}"));
}

fn step_subtitle_font(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: i32) {
    let current = state.borrow().config.subtitle_font.clone();
    let count = SUBTITLE_FONTS.len() as i32;
    let index = (subtitle_font_index(&current) as i32 + delta).rem_euclid(count);
    set_subtitle_font(state, ui, index as usize);
}

fn toggle_subtitle_bold(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let value = {
        let mut st = state.borrow_mut();
        st.config.subtitle_bold = !st.config.subtitle_bold;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.subtitle_bold
    };
    ui.set_subtitle_bold(value);
    show_notice(
        state,
        ui,
        format!("字幕粗体：{}", if value { "开" } else { "关" }),
    );
}

fn toggle_ass_styles(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let value = {
        let mut st = state.borrow_mut();
        st.config.ass_styles = !st.config.ass_styles;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.ass_styles
    };
    if !value {
        apply_subtitle_cue_style(ui, None);
    }
    show_notice(
        state,
        ui,
        format!("ASS/SSA 字幕样式：{}", if value { "开" } else { "关" }),
    );
}

fn set_subtitle_3d(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: &str) {
    {
        let mut st = state.borrow_mut();
        st.config.subtitle_3d = mode.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_subtitle_3d_mode(mode.into());
    let label = match mode {
        "sbs" => "左右排列",
        "tab" => "上下排列",
        "anaglyph" => "红蓝立体",
        _ => "关闭",
    };
    show_notice(state, ui, format!("3D 字幕：{label}"));
}

fn apply_subtitle_cue_style(ui: &MainWindow, style: Option<&CueStyle>) {
    let fallback = ui.get_subtitle_color();
    let (color, scale, bold, top, font) = match style {
        Some(style) => (
            style.color.map(ass_color).unwrap_or(fallback),
            style
                .font_size
                .map(|size| (size / 20.0).clamp(0.5, 3.0))
                .unwrap_or(1.0),
            style.bold.unwrap_or(false),
            style.top.unwrap_or(false),
            style.font.clone().unwrap_or_default(),
        ),
        None => (fallback, 1.0, false, false, String::new()),
    };
    if ui.get_subtitle_cue_color() != color {
        ui.set_subtitle_cue_color(color);
    }
    if (ui.get_subtitle_cue_scale() - scale).abs() > f32::EPSILON {
        ui.set_subtitle_cue_scale(scale);
    }
    if ui.get_subtitle_cue_bold() != bold {
        ui.set_subtitle_cue_bold(bold);
    }
    if ui.get_subtitle_cue_top() != top {
        ui.set_subtitle_cue_top(top);
    }
    if ui.get_subtitle_cue_font().as_str() != font.as_str() {
        ui.set_subtitle_cue_font(font.as_str().into());
    }
}

fn ass_color(value: u32) -> slint::Color {
    slint::Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn toggle_subtitle_shadow(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let value = {
        let mut st = state.borrow_mut();
        st.config.subtitle_shadow = !st.config.subtitle_shadow;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.subtitle_shadow
    };
    ui.set_subtitle_shadow(value);
    show_notice(
        state,
        ui,
        format!("字幕阴影：{}", if value { "开" } else { "关" }),
    );
}

fn toggle_subtitle_flag(state: &Rc<RefCell<AppState>>, ui: &MainWindow, flag: &str) {
    let (value, label) = {
        let mut st = state.borrow_mut();
        let target = match flag {
            "graphic-outside" => &mut st.config.subtitle_graphic_outside,
            "text-inside" => &mut st.config.subtitle_text_inside,
            "bottom" => &mut st.config.subtitle_bottom,
            "border-opaque" => &mut st.config.subtitle_border_opaque,
            _ => &mut st.config.subtitle_smooth_vmr,
        };
        *target = !*target;
        let value = *target;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        let label = match flag {
            "graphic-outside" => "将标准图形字幕输出到画面外",
            "text-inside" => "将标准文本字幕输出到画面内",
            "bottom" => "尽可能将字幕显示在画面底部",
            "border-opaque" => "字幕边框不透明",
            _ => "平滑输出 VMR/EVR 字幕",
        };
        (value, label)
    };
    show_notice(
        state,
        ui,
        format!("{label}：{}", if value { "开" } else { "关" }),
    );
}

fn synth_names(avisynth: bool) -> (&'static str, &'static str) {
    if avisynth {
        ("AviSynth", "avs2yuv")
    } else {
        ("VapourSynth", "vspipe")
    }
}

fn synth_engine_available(avisynth: bool) -> bool {
    let (_, binary) = synth_names(avisynth);
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(binary).is_file())
    })
}

fn toggle_synth(state: &Rc<RefCell<AppState>>, ui: &MainWindow, avisynth: bool) {
    let (label, _) = synth_names(avisynth);
    let message = if synth_engine_available(avisynth) {
        format!("当前构建未启用 {label} 处理管线")
    } else {
        format!("未找到 {label} 引擎")
    };
    show_notice(state, ui, message);
}

fn synth_script_dir(avisynth: bool) -> PathBuf {
    let name = if avisynth { "avisynth" } else { "vapoursynth" };
    flint_core::config::config_dir().join(name)
}

fn show_synth_script_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, avisynth: bool) {
    let dir = synth_script_dir(avisynth);
    let _ = std::fs::create_dir_all(&dir);
    {
        let mut st = state.borrow_mut();
        st.script_pick = if avisynth { 1 } else { 2 };
        st.audio_pick = false;
        st.skin_pick = false;
        st.subtitle_pick = false;
        st.subtitle2_pick = false;
    st.audio_pick = false;
        st.logo_pick = 0;
        st.append_files = false;
    }
    load_file_dir(state, ui, dir);
    ui.set_file_folder_mode(false);
    ui.set_file_open(true);
}

fn open_synth_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow, avisynth: bool) {
    let (label, _) = synth_names(avisynth);
    let directory = synth_script_dir(avisynth);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        show_notice(state, ui, format!("创建 {label} 脚本目录失败：{error}"));
        return;
    }
    if let Err(error) = std::process::Command::new("xdg-open")
        .arg(&directory)
        .spawn()
    {
        show_notice(state, ui, format!("打开 {label} 脚本目录失败：{error}"));
    }
}

fn load_synth_script(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path, avisynth: bool) {
    let (label, _) = synth_names(avisynth);
    {
        let mut st = state.borrow_mut();
        if avisynth {
            st.config.avisynth_script = path.to_string_lossy().to_string();
        } else {
            st.config.vapoursynth_script = path.to_string_lossy().to_string();
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    show_notice(state, ui, format!("{label} 脚本：{name}"));
}

fn stereo_label(mode: StereoMode) -> &'static str {
    match mode {
        StereoMode::Off => "关闭",
        StereoMode::AnaglyphSideBySide => "红蓝立体（左右并排）",
        StereoMode::AnaglyphTopBottom => "红蓝立体（上下）",
        StereoMode::LeftSideBySide => "仅左眼（左右并排）",
        StereoMode::RightSideBySide => "仅右眼（左右并排）",
        StereoMode::LeftTopBottom => "仅左眼（上下）",
        StereoMode::RightTopBottom => "仅右眼（上下）",
    }
}

fn set_stereo_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: StereoMode) {
    {
        let mut st = state.borrow_mut();
        st.stereo_mode = mode;
        st.config.stereo_mode = mode.key().to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_stereo_mode(mode.key().into());
    show_notice(state, ui, format!("3D 视频输出：{}", stereo_label(mode)));
}

fn cycle_stereo(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let next = state.borrow().stereo_mode.next();
    set_stereo_mode(state, ui, next);
}

fn renderer_osd_label(name: &str) -> &'static str {
    match name {
        "opengl" => "OpenGL Video Renderer",
        "null-compressed" => "Null Video Renderer (compressed)",
        "null-uncompressed" => "Null Video Renderer (uncompressed)",
        _ => "Slint Hardware Pipeline (OpenGL/Vulkan)",
    }
}

fn renderer_notice_label(name: &str) -> &'static str {
    match name {
        "opengl" => "内置 OpenGL 视频渲染器",
        "null-compressed" => "空视频渲染器(压缩)",
        "null-uncompressed" => "空视频渲染器(未压缩)",
        _ => "自动选择(推荐)",
    }
}

fn set_video_renderer(state: &Rc<RefCell<AppState>>, ui: &MainWindow, name: &str) {
    let engines: Vec<SharedEngine> = {
        let mut st = state.borrow_mut();
        st.config.video_renderer = name.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.panels.iter().map(|panel| panel.engine.clone()).collect()
    };
    ui.set_video_renderer(name.into());
    ui.set_osd_renderer(osd_tokens(vec![(renderer_osd_label(name).into(), TONE_RED)]));
    for engine in engines {
        engine.lock().set_video_renderer(name);
    }
    show_notice(
        state,
        ui,
        format!("视频渲染器：{}", renderer_notice_label(name)),
    );
}

#[derive(Clone, Copy)]
enum WindowPlacement {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

fn x11_screen_bounds() -> Option<((i32, i32), i32, i32)> {
    use x11rb::connection::Connection;
    let (connection, screen_num) = x11rb::connect(None).ok()?;
    let screen = connection.setup().roots.get(screen_num)?;
    Some((
        (0, 0),
        screen.width_in_pixels as i32,
        screen.height_in_pixels as i32,
    ))
}

fn place_window(
    ui: &MainWindow,
    state: &Rc<RefCell<AppState>>,
    placement: Option<WindowPlacement>,
) {
    use slint::winit_030::{winit, WinitWindowAccessor};

    let window = ui.window();
    match placement {
        None => {
            if let Some((x, y)) = state.borrow_mut().window_saved_pos.take() {
                window.with_winit_window(|winit_window| {
                    winit_window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
                });
            }
        }
        Some(placement) => {
            if let Some(Ok(position)) = window.with_winit_window(|w| w.outer_position()) {
                state.borrow_mut().window_saved_pos = Some((position.x, position.y));
            }
            window.with_winit_window(move |winit_window| {
                let Some(monitor) = winit_window.current_monitor() else {
                    return;
                };
                let size = winit_window.outer_size();
                let area = monitor.size();
                let origin = monitor.position();
                // winit yields a 1x1 dummy monitor when the X server exposes no usable RandR CRTCs.
                let ((origin_x, origin_y), width, height) = if area.width > 1 && area.height > 1 {
                    ((origin.x, origin.y), area.width as i32, area.height as i32)
                } else if let Some(bounds) = x11_screen_bounds() {
                    bounds
                } else {
                    return;
                };
                let x = match placement {
                    WindowPlacement::TopLeft | WindowPlacement::BottomLeft => origin_x,
                    WindowPlacement::TopRight | WindowPlacement::BottomRight => {
                        origin_x + width - size.width as i32
                    }
                    WindowPlacement::Center => origin_x + (width - size.width as i32) / 2,
                };
                let y = match placement {
                    WindowPlacement::TopLeft | WindowPlacement::TopRight => origin_y,
                    WindowPlacement::BottomLeft | WindowPlacement::BottomRight => {
                        origin_y + height - size.height as i32
                    }
                    WindowPlacement::Center => origin_y + (height - size.height as i32) / 2,
                };
                winit_window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
            });
        }
    }
}

#[derive(Clone, Copy)]
struct VideoTransform {
    stereo: StereoMode,
    pano: Option<panorama::View>,
    adjust: effects::Adjust,
    rotation: u16,
    flip_horizontal: bool,
    flip_vertical: bool,
}

fn render_panel_image(panel: &mut PanelState, frame: &Frame, transform: VideoTransform) {
    let VideoTransform {
        stereo: stereo_mode,
        pano: pano_view,
        adjust,
        rotation,
        flip_horizontal,
        flip_vertical,
    } = transform;
    let mut pixels: &[u8] = &frame.rgba;
    if let Some(view) = pano_view {
        let valid = panel
            .pano_map
            .as_ref()
            .is_some_and(|map| map.matches(frame.width, frame.height, view));
        if !valid {
            panel.pano_map = panorama::Map::build(frame.width, frame.height, view);
        }
        if let Some(map) = &panel.pano_map {
            if map.render(frame, &mut panel.pano_buf) {
                pixels = &panel.pano_buf;
            }
        }
    }
    if stereo::transform_rgba(
        frame.width,
        frame.height,
        pixels,
        stereo_mode,
        &mut panel.display_buf,
    ) {
        pixels = &panel.display_buf;
    }
    if !adjust.is_neutral() {
        panel.adjust_buf.clear();
        panel.adjust_buf.extend_from_slice(pixels);
        if effects::apply_rgba(frame.width, frame.height, &mut panel.adjust_buf, adjust) {
            pixels = &panel.adjust_buf;
        }
    }
    if (flip_horizontal || flip_vertical)
        && effects::flip_rgba(
            frame.width,
            frame.height,
            pixels,
            flip_horizontal,
            flip_vertical,
            &mut panel.flip_buf,
        )
    {
        pixels = &panel.flip_buf;
    }
    let (mut width, mut height) = (frame.width, frame.height);
    if rotation != 0 {
        if let Some((rotated_width, rotated_height)) = effects::rotate_rgba(
            frame.width,
            frame.height,
            pixels,
            rotation,
            &mut panel.rotate_buf,
        ) {
            pixels = &panel.rotate_buf;
            width = rotated_width;
            height = rotated_height;
        }
    }
    let buffer = SharedPixelBuffer::clone_from_slice(pixels, width as u32, height as u32);
    panel.image = Image::from_rgba8_premultiplied(buffer);
}

fn set_panorama(enabled: bool, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.pano_enabled = enabled;
        st.pano_dirty = true;
        st.config.pano_enabled = enabled;
        st.config.pano_yaw = st.pano_view.yaw.to_degrees();
        st.config.pano_pitch = st.pano_view.pitch.to_degrees();
        st.config.pano_fov = st.pano_view.fov.to_degrees();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_pano_enabled(enabled);
    show_notice(
        state,
        ui,
        format!("360° 视频播放：{}", if enabled { "开" } else { "关" }),
    );
}

fn reset_panorama(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.pano_view = panorama::View::default();
        st.pano_dirty = true;
        st.config.pano_yaw = 0.0;
        st.config.pano_pitch = 0.0;
        st.config.pano_fov = 90.0;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    show_notice(state, ui, "360° 视角已复位".to_string());
}

fn rotate_panorama(state: &Rc<RefCell<AppState>>, dx: f32, dy: f32) {
    let mut st = state.borrow_mut();
    if !st.pano_enabled {
        return;
    }
    st.pano_view.yaw -= dx * 0.004;
    st.pano_view.pitch += dy * 0.004;
    st.pano_view = st.pano_view.clamped();
    st.pano_dirty = true;
}

fn zoom_panorama(state: &Rc<RefCell<AppState>>, delta: f32) {
    let mut st = state.borrow_mut();
    if !st.pano_enabled {
        return;
    }
    st.pano_view.fov *= (-delta * 0.05).exp();
    st.pano_view = st.pano_view.clamped();
    st.pano_dirty = true;
}

#[derive(Clone, Copy)]
enum AdjustField {
    Brightness,
    Contrast,
    Saturation,
}

fn nudge_adjust(state: &Rc<RefCell<AppState>>, ui: &MainWindow, field: AdjustField, delta: i16) {
    let (label, value) = {
        let mut st = state.borrow_mut();
        let mut adjust = st.adjust;
        match field {
            AdjustField::Brightness => {
                adjust.brightness = (adjust.brightness + delta).clamp(-100, 100)
            }
            AdjustField::Contrast => adjust.contrast = (adjust.contrast + delta).clamp(-100, 100),
            AdjustField::Saturation => {
                adjust.saturation = (adjust.saturation + delta).clamp(-100, 100)
            }
        }
        st.adjust = adjust;
        st.video_dirty = true;
        st.config.brightness = adjust.brightness;
        st.config.contrast = adjust.contrast;
        st.config.saturation = adjust.saturation;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        let (label, value) = match field {
            AdjustField::Brightness => ("亮度", adjust.brightness),
            AdjustField::Contrast => ("对比度", adjust.contrast),
            AdjustField::Saturation => ("饱和度", adjust.saturation),
        };
        (label, value)
    };
    show_notice(state, ui, format!("{label}：{value}%"));
}

fn reset_video_adjust(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.adjust = Adjust::default();
        st.video_dirty = true;
        st.config.brightness = 0;
        st.config.contrast = 0;
        st.config.saturation = 0;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    show_notice(state, ui, "图像属性已复位".to_string());
}

fn cycle_aspect(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let stretch = !state.borrow().stretch;
    state.borrow_mut().stretch = stretch;
    ui.set_stretch(stretch);
    let label = if stretch {
        "不保持宽高比"
    } else {
        "保持宽高比(推荐)"
    };
    show_notice(state, ui, label.to_string());
}

fn cycle_zoom_ratio(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    const RATIOS: [(i32, &str); 6] = [
        (0, "不使用缩放功能"),
        (1333, "按 4:3 拉伸"),
        (1778, "按 16:9 拉伸"),
        (1600, "按 16:10 拉伸"),
        (1850, "按 1.85:1 拉伸"),
        (2350, "按 2.35:1 拉伸"),
    ];
    let current = state.borrow().zoom_ratio;
    let next = RATIOS
        .iter()
        .position(|(ratio, _)| *ratio == current)
        .map_or(1, |index| (index + 1) % RATIOS.len());
    let (ratio, label) = RATIOS[next];
    set_zoom(ratio, false, label, state, ui);
}

fn set_zoom(ratio: i32, crop: bool, label: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.zoom_ratio = ratio;
        st.zoom_crop = crop;
        st.zoom_shift_x = 0;
        st.zoom_shift_y = 0;
    }
    ui.set_zoom_ratio(ratio);
    ui.set_zoom_crop(crop);
    ui.set_zoom_shift_x(0);
    ui.set_zoom_shift_y(0);
    persist_frame_geometry(state);
    show_notice(state, ui, format!("缩放：{label}"));
}

fn set_margin(percent: i32, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    state.borrow_mut().margin_bottom = percent;
    ui.set_margin_bottom(percent);
    let label = if percent == 0 {
        "关闭".to_string()
    } else {
        format!("{percent}%")
    };
    show_notice(state, ui, format!("下边距：{label}"));
}

fn cycle_margin(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let next = match state.borrow().margin_bottom {
        0 => 5,
        5 => 10,
        10 => 15,
        15 => 20,
        20 => 25,
        25 => 30,
        _ => 0,
    };
    set_margin(next, state, ui);
}

fn reset_zoom(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.zoom_shift_x = 0;
        st.zoom_shift_y = 0;
    }
    ui.set_zoom_shift_x(0);
    ui.set_zoom_shift_y(0);
    persist_frame_geometry(state);
    show_notice(state, ui, "画面位置已复位".to_string());
}

fn shift_zoom(dx: i32, dy: i32, label: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (x, y) = {
        let mut st = state.borrow_mut();
        st.zoom_shift_x = (st.zoom_shift_x + dx).clamp(-50, 50);
        st.zoom_shift_y = (st.zoom_shift_y + dy).clamp(-50, 50);
        (st.zoom_shift_x, st.zoom_shift_y)
    };
    ui.set_zoom_shift_x(x);
    ui.set_zoom_shift_y(y);
    persist_frame_geometry(state);
    show_notice(state, ui, format!("画面位置：{label}"));
}

const FRAME_SCALE_MIN: i32 = 10;
const FRAME_SCALE_MAX: i32 = 400;

fn step_frame_scale(dx: i32, dy: i32, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (x, y) = {
        let st = state.borrow();
        (st.frame_scale_x + dx, st.frame_scale_y + dy)
    };
    set_frame_scale(x, y, state, ui);
}

fn set_frame_scale(x: i32, y: i32, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (x, y) = {
        let mut st = state.borrow_mut();
        st.frame_scale_x = x.clamp(FRAME_SCALE_MIN, FRAME_SCALE_MAX);
        st.frame_scale_y = y.clamp(FRAME_SCALE_MIN, FRAME_SCALE_MAX);
        (st.frame_scale_x, st.frame_scale_y)
    };
    ui.set_frame_scale_x(x);
    ui.set_frame_scale_y(y);
    persist_frame_geometry(state);
    show_notice(state, ui, format!("帧大小：宽 {x}% 高 {y}%"));
}

fn persist_frame_geometry(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    if !st.remember_frame {
        return;
    }
    st.config.frame_scale_x = st.frame_scale_x;
    st.config.frame_scale_y = st.frame_scale_y;
    st.config.frame_shift_x = st.zoom_shift_x;
    st.config.frame_shift_y = st.zoom_shift_y;
    if let Err(error) = st.config.save() {
        log::warn!("保存配置失败：{error}");
    }
}

fn toggle_remember_frame(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.remember_frame = !st.remember_frame;
        st.config.remember_frame = st.remember_frame;
        st.config.frame_scale_x = st.frame_scale_x;
        st.config.frame_scale_y = st.frame_scale_y;
        st.config.frame_shift_x = st.zoom_shift_x;
        st.config.frame_shift_y = st.zoom_shift_y;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.remember_frame
    };
    ui.set_remember_frame(enabled);
    show_notice(
        state,
        ui,
        format!(
            "记忆下次运行的图像尺寸/位置：{}",
            if enabled { "开" } else { "关" }
        ),
    );
}

fn set_rotation(degrees: u16, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let degrees = degrees % 360;
    {
        let mut st = state.borrow_mut();
        st.rotation = degrees;
        st.video_dirty = true;
        st.config.rotation = degrees;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_rotation(degrees as i32);
    show_notice(state, ui, format!("图像旋转：{degrees}°"));
}

fn toggle_rotation(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let current = state.borrow().rotation;
    set_rotation(if current == 90 { 0 } else { 90 }, state, ui);
}

fn cycle_rotation(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let current = state.borrow().rotation;
    let next = match current {
        0 => 90,
        90 => 180,
        180 => 270,
        _ => 0,
    };
    set_rotation(next, state, ui);
}

fn auto_rotate(base: u16, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let portrait = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| {
                panel
                    .last_frame
                    .as_ref()
                    .map(|frame| frame.height > frame.width)
            })
            .unwrap_or_else(|| st.config.height > st.config.width)
    };
    set_rotation(if portrait { base + 90 } else { base }, state, ui);
}

fn toggle_flip(horizontal: bool, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (flip_horizontal, flip_vertical) = {
        let mut st = state.borrow_mut();
        if horizontal {
            st.flip_horizontal = !st.flip_horizontal;
        } else {
            st.flip_vertical = !st.flip_vertical;
        }
        st.video_dirty = true;
        st.config.flip_horizontal = st.flip_horizontal;
        st.config.flip_vertical = st.flip_vertical;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        (st.flip_horizontal, st.flip_vertical)
    };
    ui.set_flip_horizontal(flip_horizontal);
    ui.set_flip_vertical(flip_vertical);
    let enabled = if horizontal {
        flip_horizontal
    } else {
        flip_vertical
    };
    show_notice(
        state,
        ui,
        format!(
            "{}：{}",
            if horizontal {
                "水平翻转"
            } else {
                "垂直翻转"
            },
            if enabled { "开" } else { "关" }
        ),
    );
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let attempts: [(&str, &[&str]); 3] = [
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("wl-copy", &[]),
    ];
    let mut last_error = String::new();
    for (program, args) in attempts {
        let mut child = match Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                last_error = format!("{program}: {error}");
                continue;
            }
        };
        let written = match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(text.as_bytes()),
            None => Ok(()),
        };
        match written {
            Ok(()) => match child.wait() {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => last_error = format!("{program} 退出状态 {status}"),
                Err(error) => last_error = format!("{program}: {error}"),
            },
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                last_error = format!("{program}: {error}");
            }
        }
    }
    Err(if last_error.is_empty() {
        "未找到可用的剪贴板工具".to_string()
    } else {
        last_error
    })
}

fn current_subtitle_text(state: &Rc<RefCell<AppState>>, ui: &MainWindow) -> String {
    if ui.get_manual_open() && !ui.get_manual_text().is_empty() {
        return ui.get_manual_text().to_string();
    }
    let st = state.borrow();
    st.active
        .and_then(|index| st.panels.get(index))
        .and_then(|panel| {
            let position = panel.engine.lock().position_seconds() + panel.subtitle_delay;
            panel
                .subtitle
                .as_ref()
                .map(|track| track.text_at(position).to_string())
        })
        .unwrap_or_default()
}

fn run_speech(args: &[&str]) {
    match Command::new("spd-say")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => log::warn!("启动语音朗读失败：{error}"),
    }
}

fn speak_subtitle(text: &str) {
    let language = if text.chars().any(|character| {
        matches!(
            character,
            '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
        )
    }) {
        "zh"
    } else {
        "en"
    };
    run_speech(&["-C"]);
    run_speech(&["-l", language, "-r", "-10", text]);
}

fn stop_speech() {
    run_speech(&["-C"]);
}

fn toggle_tts(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.tts_enabled = !st.tts_enabled;
        st.tts_spoken.clear();
        st.config.tts_enabled = st.tts_enabled;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.tts_enabled
    };
    if !enabled {
        stop_speech();
    }
    show_notice(
        state,
        ui,
        format!("生成有声字幕：{}", if enabled { "开" } else { "关" }),
    );
}

fn speak_current_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let text = current_subtitle_text(state, ui);
    if text.is_empty() {
        show_notice(state, ui, "当前没有显示中的字幕".to_string());
        return;
    }
    speak_subtitle(&text);
    show_notice(state, ui, format!("朗读字幕：{text}"));
}

fn copy_current_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let text = current_subtitle_text(state, ui);
    if text.is_empty() {
        show_notice(state, ui, "当前没有显示中的字幕".to_string());
        return;
    }
    match copy_to_clipboard(&text) {
        Ok(()) => show_notice(state, ui, format!("已复制当前字幕：{text}")),
        Err(error) => show_notice(state, ui, format!("复制字幕失败：{error}")),
    }
}

fn save_subtitle(state: &Rc<RefCell<AppState>>, ui: &MainWindow, ass: bool) {
    let payload = {
        let st = state.borrow();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        panel.subtitle.as_ref().map(|track| {
            let text = if ass { track.to_ass() } else { track.to_srt() };
            let extension = if ass { "ass" } else { "srt" };
            let source = Path::new(&panel.source);
            let stem = source
                .file_stem()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "subtitle".to_string());
            let dir = source
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| track.path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| st.config.snapshot_dir.clone());
            (text, extension, stem, dir)
        })
    };
    let Some((text, extension, stem, dir)) = payload else {
        show_notice(state, ui, "尚未加载字幕".to_string());
        return;
    };
    let target = unique_path(&dir, &stem, extension);
    match std::fs::write(&target, text) {
        Ok(()) => show_notice(state, ui, format!("字幕已保存：{}", target.display())),
        Err(error) => show_notice(state, ui, format!("保存字幕失败：{error}")),
    }
}

fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{extension}"));
    let mut index = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}.{index}.{extension}"));
        index += 1;
    }
    candidate
}

fn add_files_to_playlist(
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
    paths: &[PathBuf],
    play_first: bool,
) -> usize {
    let first_source = paths.first().map(|path| path.to_string_lossy().to_string());
    let (start, added_count) = {
        let mut st = state.borrow_mut();
        let mut added = None;
        let mut added_count = 0;
        for path in paths {
            let source = path.to_string_lossy().to_string();
            if st.panels.iter().any(|panel| panel.source == source) || st.panels.len() >= MAX_PANELS
            {
                continue;
            }
            let settings = settings_for_source(&st, &source);
            st.panels.push(PanelState::new(
                source.clone(),
                panel_title(&source),
                settings,
            ));
            added.get_or_insert(st.panels.len() - 1);
            added_count += 1;
        }
        let first = first_source
            .and_then(|source| st.panels.iter().position(|panel| panel.source == source))
            .or(added);
        let had_active = st.active.is_some();
        if play_first || !had_active {
            if let Some(index) = first {
                st.active = Some(index);
            }
        }
        st.refresh_menu();
        st.sync_panel_model();
        let start = if play_first || !had_active {
            first
        } else {
            None
        };
        (start, added_count)
    };
    sync_sources(&state.borrow(), ui);
    update_active_ui(ui, &mut state.borrow_mut());
    if let Some(index) = start {
        start_panel(state, ui, index);
    }
    added_count
}

fn choose_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow, dir: &str) {
    let dir = PathBuf::from(dir);
    let dir = dir.canonicalize().unwrap_or(dir);
    if !dir.is_dir() {
        show_notice(state, ui, format!("路径不存在: {}", dir.display()));
        return;
    }
    let paths: Vec<PathBuf> = read_file_candidates(&dir, FileFilter::Media)
        .into_iter()
        .filter(|candidate| !candidate.is_dir)
        .map(|candidate| candidate.path)
        .collect();
    if paths.is_empty() {
        show_notice(
            state,
            ui,
            format!("文件夹中没有支持的媒体文件: {}", dir.display()),
        );
        return;
    }
    let append = {
        let mut st = state.borrow_mut();
        st.file_dir = dir.clone();
        let append = st.append_files;
        st.append_files = false;
        append
    };
    load_browser_dir(state, ui, dir);
    ui.set_file_open(false);
    let added = add_files_to_playlist(state, ui, &paths, !append);
    if append {
        show_notice(state, ui, format!("已添加 {} 个文件到播放列表", added));
    }
}

fn load_file_dir(state: &Rc<RefCell<AppState>>, ui: &MainWindow, dir: PathBuf) {
    let filter = {
        let st = state.borrow();
        if st.subtitle_pick || st.subtitle2_pick {
            FileFilter::Subtitle
        } else if st.skin_pick {
            FileFilter::Skin
        } else if st.logo_pick != 0 {
            FileFilter::Image
        } else {
            FileFilter::Media
        }
    };
    let candidates = read_file_candidates(&dir, filter);
    let paths: Vec<PathBuf> = candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    let entries: Vec<FileEntry> = candidates
        .into_iter()
        .map(|candidate| FileEntry {
            name: candidate.name.into(),
            path: candidate.path.to_string_lossy().to_string().into(),
            is_dir: candidate.is_dir,
            size: candidate.size.into(),
        })
        .collect();
    {
        let mut st = state.borrow_mut();
        st.file_dir = dir.clone();
        st.file_paths = paths;
        st.file_last_pick = None;
    }
    let dir_text = dir.to_string_lossy().to_string();
    ui.set_file_dir(dir_text.as_str().into());
    ui.set_file_path_input(dir_text.as_str().into());
    ui.set_file_entries(ModelRc::new(VecModel::from(entries)));
    ui.set_file_selected(-1);
}

fn browser_dir(state: &Rc<RefCell<AppState>>) -> PathBuf {
    let st = state.borrow();
    if !st.file_dir.as_os_str().is_empty() {
        return st.file_dir.clone();
    }
    st.active
        .and_then(|index| st.panels.get(index))
        .and_then(|panel| Path::new(&panel.source).parent().map(Path::to_path_buf))
        .unwrap_or_else(default_media_dir)
}

fn load_browser_dir(state: &Rc<RefCell<AppState>>, ui: &MainWindow, dir: PathBuf) {
    let items: Vec<BrowserItem> = read_file_candidates(&dir, FileFilter::Media)
        .into_iter()
        .map(|candidate| BrowserItem {
            name: candidate.name.into(),
            path: candidate.path.to_string_lossy().to_string().into(),
            is_dir: candidate.is_dir,
        })
        .collect();
    state.borrow_mut().file_dir = dir.clone();
    let dir_text = dir.to_string_lossy().to_string();
    ui.set_browser_path(dir_text.as_str().into());
    ui.set_browser_items(ModelRc::new(VecModel::from(items)));
}

fn browser_navigate(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &str) {
    let dir = PathBuf::from(path);
    let dir = dir.canonicalize().unwrap_or(dir);
    if dir.is_dir() {
        load_browser_dir(state, ui, dir);
    }
}

fn browser_up(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = browser_dir(state);
    if let Some(parent) = dir.parent() {
        load_browser_dir(state, ui, parent.to_path_buf());
    }
}

fn refresh_browser(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let dir = browser_dir(state);
    load_browser_dir(state, ui, dir);
}

fn navigate_file_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow, input: &str) {
    let input = input.trim();
    if input.is_empty() {
        return;
    }
    let base = state.borrow().file_dir.clone();
    let mut path = PathBuf::from(input);
    if path.is_relative() {
        path = base.join(path);
    }
    let path = path.canonicalize().unwrap_or(path);
    if path.is_dir() {
        load_file_dir(state, ui, path);
    } else if path.is_file() {
        if take_skin_flag(state) {
            load_skin_from_path(state, ui, &path);
        } else if take_audio_flag(state) {
            load_external_audio(state, ui, &path);
        } else if let Some(secondary) = take_subtitle_flag(state) {
            load_subtitle(state, ui, &path, secondary);
        } else if let Some(slot) = take_logo_flag(state) {
            load_logo_from_path(state, ui, slot, &path);
        } else if let Some(avisynth) = take_script_flag(state) {
            load_synth_script(state, ui, &path, avisynth);
        } else {
            open_media_file(state, ui, &path);
        }
    } else {
        show_notice(state, ui, format!("路径不存在: {}", path.display()));
    }
}

fn file_picked(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: i32) {
    let index = index.max(0) as usize;
    let path = { state.borrow().file_paths.get(index).cloned() };
    let Some(path) = path else {
        return;
    };
    if path.is_dir() {
        let path = path.canonicalize().unwrap_or(path);
        load_file_dir(state, ui, path);
        return;
    }
    let now = Instant::now();
    let repeated = {
        let st = state.borrow();
        matches!(st.file_last_pick, Some((last, at)) if last == index && now.duration_since(at) < Duration::from_millis(500))
    };
    state.borrow_mut().file_last_pick = Some((index, now));
    if repeated {
        if take_skin_flag(state) {
            load_skin_from_path(state, ui, &path);
        } else if take_audio_flag(state) {
            load_external_audio(state, ui, &path);
        } else if let Some(secondary) = take_subtitle_flag(state) {
            load_subtitle(state, ui, &path, secondary);
        } else if let Some(slot) = take_logo_flag(state) {
            load_logo_from_path(state, ui, slot, &path);
        } else if let Some(avisynth) = take_script_flag(state) {
            load_synth_script(state, ui, &path, avisynth);
        } else {
            open_media_file(state, ui, &path);
        }
    }
}

fn open_file_selected(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let index = ui.get_file_selected().max(0) as usize;
    let path = { state.borrow().file_paths.get(index).cloned() };
    let Some(path) = path else {
        return;
    };
    if path.is_dir() {
        let path = path.canonicalize().unwrap_or(path);
        load_file_dir(state, ui, path);
    } else if take_skin_flag(state) {
        load_skin_from_path(state, ui, &path);
    } else if take_audio_flag(state) {
        load_external_audio(state, ui, &path);
    } else if let Some(secondary) = take_subtitle_flag(state) {
        load_subtitle(state, ui, &path, secondary);
    } else if let Some(slot) = take_logo_flag(state) {
        load_logo_from_path(state, ui, slot, &path);
    } else if let Some(avisynth) = take_script_flag(state) {
        load_synth_script(state, ui, &path, avisynth);
    } else if take_append_flag(state) {
        append_file_to_playlist(state, ui, &path);
    } else {
        open_media_file(state, ui, &path);
    }
}

fn take_append_flag(state: &Rc<RefCell<AppState>>) -> bool {
    let mut st = state.borrow_mut();
    let append = st.append_files;
    st.append_files = false;
    append
}

fn take_skin_flag(state: &Rc<RefCell<AppState>>) -> bool {
    let mut st = state.borrow_mut();
    let pick = st.skin_pick;
    st.skin_pick = false;
    pick
}

fn take_script_flag(state: &Rc<RefCell<AppState>>) -> Option<bool> {
    let mut st = state.borrow_mut();
    let pick = st.script_pick;
    st.script_pick = 0;
    match pick {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn take_logo_flag(state: &Rc<RefCell<AppState>>) -> Option<u8> {
    let mut st = state.borrow_mut();
    let slot = st.logo_pick;
    st.logo_pick = 0;
    (slot != 0).then_some(slot)
}

fn take_subtitle_flag(state: &Rc<RefCell<AppState>>) -> Option<bool> {
    let mut st = state.borrow_mut();
    let pick = st.subtitle_pick || st.subtitle2_pick;
    let secondary = st.subtitle2_pick;
    st.subtitle_pick = false;
    st.subtitle2_pick = false;
    pick.then_some(secondary)
}

fn take_audio_flag(state: &Rc<RefCell<AppState>>) -> bool {
    let mut st = state.borrow_mut();
    let pick = st.audio_pick;
    st.audio_pick = false;
    pick
}

fn append_file_to_playlist(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    ui.set_file_open(false);
    let paths = [path.to_path_buf()];
    let added = add_files_to_playlist(state, ui, &paths, false);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let message = if added > 0 {
        format!("已添加 {} 到播放列表", name)
    } else {
        format!("{} 已在播放列表中", name)
    };
    show_notice(state, ui, message);
}

fn open_media_file(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        state.borrow_mut().file_dir = parent.to_path_buf();
        load_browser_dir(state, ui, parent.to_path_buf());
    }
    ui.set_file_open(false);
    open_source(state, ui, path.to_string_lossy().to_string(), title);
    open_similar_media(state, ui, path);
}

fn open_similar_media(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    let mode = state.borrow().config.similar_mode.clone();
    if mode == "selected" {
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    let selected = file_key(&path.to_string_lossy());
    let key = path
        .file_stem()
        .map(|stem| similar_stem_key(&stem.to_string_lossy()))
        .unwrap_or_default();
    let siblings: Vec<PathBuf> = read_file_candidates(parent, FileFilter::Media)
        .into_iter()
        .filter(|candidate| !candidate.is_dir)
        .map(|candidate| candidate.path)
        .filter(|candidate| file_key(&candidate.to_string_lossy()) != selected)
        .filter(|candidate| {
            mode == "all"
                || candidate
                    .file_stem()
                    .map(|stem| similar_stem_key(&stem.to_string_lossy()) == key)
                    .unwrap_or(false)
        })
        .collect();
    if siblings.is_empty() {
        return;
    }
    let added = add_files_to_playlist(state, ui, &siblings, false);
    if added > 0 {
        show_notice(state, ui, format!("已同时打开 {added} 个文件"));
    }
}

fn similar_stem_key(stem: &str) -> String {
    let mut key = String::new();
    let mut digits = false;
    for ch in stem.chars() {
        if ch.is_ascii_digit() {
            digits = true;
            continue;
        }
        if digits {
            digits = false;
            key.push('#');
        }
        key.push(ch.to_ascii_lowercase());
    }
    if digits {
        key.push('#');
    }
    key.trim().to_string()
}

fn step_active_frame(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: i32) {
    let mut st = state.borrow_mut();
    let Some(index) = st.active else {
        return;
    };
    let Some(panel) = st.panels.get_mut(index) else {
        return;
    };
    if !panel.running || !is_file_source(&panel.source) {
        return;
    }
    if !panel.paused {
        panel.paused = true;
        panel.frozen = true;
        panel.engine.lock().set_paused(true);
    }
    panel.engine.lock().step_frames(delta);
    ui.set_frozen(true);
}

fn seek_active(state: &Rc<RefCell<AppState>>, ui: &MainWindow, fraction: f32) {
    let mut st = state.borrow_mut();
    let Some(index) = st.active else {
        return;
    };
    let Some(panel) = st.panels.get_mut(index) else {
        return;
    };
    if !panel.running || !is_file_source(&panel.source) {
        return;
    }
    let duration = panel
        .shared
        .status()
        .info
        .as_ref()
        .map(|info| info.duration)
        .unwrap_or(0.0);
    if duration <= 0.0 {
        return;
    }
    let fraction = fraction.clamp(0.0, 1.0) as f64;
    let target = (fraction * duration).clamp(0.0, (duration - 0.05).max(0.0));
    panel.engine.lock().seek_to(target);
    ui.set_bar_progress(fraction as f32);
}

fn seek_relative(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: f64) {
    let mut st = state.borrow_mut();
    let Some(index) = st.active else {
        return;
    };
    let Some(panel) = st.panels.get_mut(index) else {
        return;
    };
    if !panel.running || !is_file_source(&panel.source) {
        return;
    }
    let duration = panel
        .shared
        .status()
        .info
        .as_ref()
        .map(|info| info.duration)
        .unwrap_or(0.0);
    if duration <= 0.0 {
        return;
    }
    let position = panel.engine.lock().position_seconds();
    let target = (position + delta).clamp(0.0, (duration - 0.05).max(0.0));
    panel.engine.lock().seek_to(target);
    ui.set_bar_progress((target / duration) as f32);
}

fn active_file_position(state: &Rc<RefCell<AppState>>) -> Option<f64> {
    let st = state.borrow();
    let index = st.active?;
    let panel = st.panels.get(index)?;
    if !panel.running || !is_file_source(&panel.source) {
        return None;
    }
    let position = panel.engine.lock().position_seconds();
    Some(position)
}

fn file_key(source: &str) -> String {
    std::fs::canonicalize(source)
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| source.to_string())
}

fn saved_bookmarks(st: &AppState, source: &str) -> Vec<f64> {
    let key = file_key(source);
    st.config
        .bookmarks
        .iter()
        .find(|(path, _)| path == &key)
        .map(|(_, times)| times.clone())
        .unwrap_or_default()
}

fn store_bookmarks(st: &mut AppState, source: &str, times: &[f64]) {
    let key = file_key(source);
    st.config.bookmarks.retain(|(path, _)| path != &key);
    if !times.is_empty() {
        st.config.bookmarks.push((key, times.to_vec()));
    }
    if let Err(error) = st.config.save() {
        log::warn!("保存书签失败：{error}");
    }
}

fn remember_position_for(st: &mut AppState, index: usize) -> bool {
    if !st.config.remember_position {
        return false;
    }
    let Some(panel) = st.panels.get(index) else {
        return false;
    };
    if !panel.running || !is_file_source(&panel.source) {
        return false;
    }
    let position = panel.engine.lock().position_seconds();
    let duration = panel
        .shared
        .status()
        .info
        .as_ref()
        .map(|info| info.duration)
        .unwrap_or(0.0);
    let key = file_key(&panel.source);
    st.config.resume.retain(|(path, _)| path != &key);
    if position > 1.0 && (duration <= 0.0 || position < duration - 2.0) {
        st.config.resume.push((key, position));
    }
    true
}

fn remember_running_positions(st: &mut AppState) {
    for index in 0..st.panels.len() {
        remember_position_for(st, index);
    }
}

fn add_bookmark(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Some(position) = active_file_position(state) else {
        show_notice(state, ui, "书签仅支持正在播放的本地文件".into());
        return;
    };
    let (source, times, count) = {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.bookmarks.push(position);
        panel.bookmarks.sort_by(f64::total_cmp);
        panel
            .bookmarks
            .dedup_by(|left, right| (*left - *right).abs() < 1.0);
        (
            panel.source.clone(),
            panel.bookmarks.clone(),
            panel.bookmarks.len(),
        )
    };
    {
        let mut st = state.borrow_mut();
        store_bookmarks(&mut st, &source, &times);
    }
    show_notice(
        state,
        ui,
        format!("已添加书签 {position:.1} 秒（共 {count} 个）"),
    );
    refresh_bookmark_rows(state, ui);
}

fn refresh_bookmark_rows(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let rows = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| {
                panel
                    .bookmarks
                    .iter()
                    .map(|time| BookmarkRow {
                        label: format_seconds(*time).into(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    ui.set_bookmark_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_bookmark_selected(-1);
}

fn show_bookmark_dialog(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    refresh_bookmark_rows(state, ui);
    ui.set_bookmarks_open(true);
}

fn remove_bookmark(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: i32) {
    if index < 0 {
        return;
    }
    let (source, times) = {
        let mut st = state.borrow_mut();
        let Some(active) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(active) else {
            return;
        };
        if index as usize >= panel.bookmarks.len() {
            return;
        }
        panel.bookmarks.remove(index as usize);
        (panel.source.clone(), panel.bookmarks.clone())
    };
    {
        let mut st = state.borrow_mut();
        store_bookmarks(&mut st, &source, &times);
    }
    refresh_bookmark_rows(state, ui);
    show_notice(state, ui, "已删除书签".to_string());
}

fn show_menu_search(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let catalog = ui.get_menu_catalog();
    let mut all: Vec<(String, String)> = Vec::new();
    for leaf in catalog.iter() {
        if !leaf.command.is_empty() && !leaf.title.is_empty() {
            all.push((leaf.title.to_string(), leaf.command.to_string()));
        }
        for child in leaf.children.iter() {
            if !child.command.is_empty() && !child.title.is_empty() {
                all.push((child.title.to_string(), child.command.to_string()));
            }
        }
    }
    state.borrow_mut().menu_search_all = all;
    ui.set_menu_search_selected(-1);
    filter_menu_search(state, ui, "");
    ui.set_menu_search_open(true);
}

fn filter_menu_search(state: &Rc<RefCell<AppState>>, ui: &MainWindow, query: &str) {
    let query = query.trim().to_lowercase();
    let (rows, commands) = {
        let st = state.borrow();
        let mut rows = Vec::new();
        let mut commands = Vec::new();
        for (label, command) in &st.menu_search_all {
            if !query.is_empty() && !label.to_lowercase().contains(&query) {
                continue;
            }
            rows.push(SearchRow {
                label: label.as_str().into(),
                hint: "".into(),
            });
            commands.push(command.clone());
            if rows.len() >= 300 {
                break;
            }
        }
        (rows, commands)
    };
    ui.set_menu_search_rows(ModelRc::new(VecModel::from(rows)));
    state.borrow_mut().menu_search_commands = commands;
}

fn show_navigation_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (step, remember, markers, autohide) = {
        let st = state.borrow();
        (
            st.config.seek_step_seconds,
            st.config.remember_position,
            st.config.show_markers,
            st.config.autohide_controls,
        )
    };
    ui.set_navigation_seek_step(format!("{step}").into());
    ui.set_navigation_remember_position(remember);
    ui.set_navigation_show_markers(markers);
    ui.set_navigation_autohide_controls(autohide);
    ui.set_navigation_settings_open(true);
}

fn apply_navigation_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Ok(step) = ui.get_navigation_seek_step().trim().parse::<f64>() else {
        show_notice(state, ui, "快进/快退步长无效（1-600 秒）".into());
        return;
    };
    if !step.is_finite() {
        show_notice(state, ui, "快进/快退步长无效（1-600 秒）".into());
        return;
    }
    let step = step.clamp(1.0, 600.0);
    let remember = ui.get_navigation_remember_position();
    let markers = ui.get_navigation_show_markers();
    let autohide = ui.get_navigation_autohide_controls();
    {
        let mut st = state.borrow_mut();
        st.config.seek_step_seconds = step;
        st.config.remember_position = remember;
        if !remember {
            st.config.resume.clear();
        }
        st.config.show_markers = markers;
        st.config.autohide_controls = autohide;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_navigation_settings_open(false);
    ui.set_navigation_seek_step(format!("{step}").into());
    ui.set_autohide_controls(autohide);
    if !autohide {
        ui.set_chrome_visible(true);
    }
    if !markers {
        clear_seek_markers(ui);
    }
    refresh_play_menu(state, ui);
    show_notice(state, ui, format!("简索设置已保存：快进/快退 {} 秒", step));
}

fn jump_bookmark(state: &Rc<RefCell<AppState>>, ui: &MainWindow, forward: bool) {
    let Some(position) = active_file_position(state) else {
        show_notice(state, ui, "书签跳转仅支持正在播放的本地文件".into());
        return;
    };
    let target = {
        let st = state.borrow();
        let Some(panel) = st.active.and_then(|index| st.panels.get(index)) else {
            return;
        };
        if forward {
            panel
                .bookmarks
                .iter()
                .copied()
                .find(|bookmark| *bookmark > position + 0.3)
        } else {
            panel
                .bookmarks
                .iter()
                .rev()
                .copied()
                .find(|bookmark| *bookmark < position - 0.3)
        }
    };
    let Some(target) = target else {
        show_notice(state, ui, "没有可跳转的书签".into());
        return;
    };
    seek_seconds(state, ui, target);
    show_notice(state, ui, format!("跳转到书签 {target:.1} 秒"));
}

fn clear_bookmarks(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (count, source) = {
        let mut st = state.borrow_mut();
        let Some(index) = st.active else {
            return;
        };
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        let count = panel.bookmarks.len();
        panel.bookmarks.clear();
        (count, panel.source.clone())
    };
    {
        let mut st = state.borrow_mut();
        store_bookmarks(&mut st, &source, &[]);
    }
    let message = if count > 0 {
        format!("已清除 {count} 个书签")
    } else {
        "当前没有书签".to_string()
    };
    show_notice(state, ui, message);
    refresh_bookmark_rows(state, ui);
}

fn subtitle_color_value(name: &str) -> slint::Color {
    match name {
        "yellow" => slint::Color::from_rgb_u8(255, 255, 0),
        "cyan" => slint::Color::from_rgb_u8(102, 255, 255),
        "green" => slint::Color::from_rgb_u8(120, 255, 120),
        _ => slint::Color::from_rgb_u8(255, 255, 255),
    }
}

fn subtitle_font_name(family: &str) -> &'static str {
    SUBTITLE_FONTS
        .iter()
        .find(|(_, value)| *value == family)
        .map(|(name, _)| *name)
        .unwrap_or(SUBTITLE_FONTS[0].0)
}

fn subtitle_font_index(family: &str) -> usize {
    SUBTITLE_FONTS
        .iter()
        .position(|(_, value)| *value == family)
        .unwrap_or(0)
}

fn menu_check(title: &str, checked: bool, command: &str) -> MenuLeaf {
    MenuLeaf {
        title: crate::i18n::tr(title).into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked,
        arrow: false,
        command: command.into(),
        children: Default::default(),
    }
}

fn menu_arrow(title: &str, enabled: bool) -> MenuLeaf {
    MenuLeaf {
        title: crate::i18n::tr(title).into(),
        shortcut: "".into(),
        enabled,
        checkable: false,
        checked: false,
        arrow: true,
        command: "".into(),
        children: Default::default(),
    }
}

fn menu_leaf(title: &str, shortcut: &str, enabled: bool, command: &str) -> MenuLeaf {
    MenuLeaf {
        title: crate::i18n::tr(title).into(),
        shortcut: crate::i18n::tr(shortcut).into(),
        enabled,
        checkable: false,
        checked: false,
        arrow: false,
        command: command.into(),
        children: Default::default(),
    }
}

fn menu_separator() -> MenuLeaf {
    menu_leaf("", "", false, "")
}

fn menu_sub(title: &str, shortcut: &str, enabled: bool, command: &str) -> MenuSub {
    MenuSub {
        title: crate::i18n::tr(title).into(),
        shortcut: crate::i18n::tr(shortcut).into(),
        enabled,
        checkable: false,
        checked: false,
        arrow: false,
        command: command.into(),
    }
}

fn menu_sub_check(title: &str, checked: bool, command: &str) -> MenuSub {
    MenuSub {
        title: crate::i18n::tr(title).into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked,
        arrow: false,
        command: command.into(),
    }
}

fn menu_sub_separator() -> MenuSub {
    menu_sub("", "", false, "")
}

fn menu_group(title: &str, enabled: bool, children: Vec<MenuSub>) -> MenuLeaf {
    MenuLeaf {
        title: crate::i18n::tr(title).into(),
        shortcut: "".into(),
        enabled,
        checkable: false,
        checked: false,
        arrow: true,
        command: "".into(),
        children: ModelRc::new(VecModel::from(children)),
    }
}

fn active_chapters(state: &Rc<RefCell<AppState>>) -> Arc<Vec<ChapterInfo>> {
    let st = state.borrow();
    st.active
        .and_then(|index| st.panels.get(index))
        .map(|panel| panel.shared.chapters())
        .unwrap_or_default()
}

fn jump_chapter(state: &Rc<RefCell<AppState>>, ui: &MainWindow, forward: bool) {
    let chapters = active_chapters(state);
    if chapters.is_empty() {
        show_notice(state, ui, "当前文件没有章节信息".into());
        return;
    }
    let Some(position) = active_file_position(state) else {
        show_notice(state, ui, "章节跳转仅支持正在播放的本地文件".into());
        return;
    };
    let target = if forward {
        chapters
            .iter()
            .find(|chapter| chapter.start > position + 0.5)
    } else {
        chapters
            .iter()
            .rev()
            .find(|chapter| chapter.start < position - 0.5)
            .or_else(|| chapters.first())
    };
    let Some(chapter) = target else {
        show_notice(state, ui, "已经是最后一个章节".into());
        return;
    };
    let (title, start) = (chapter.title.clone(), chapter.start);
    seek_seconds(state, ui, start);
    show_notice(
        state,
        ui,
        format!("章节：{title}（{}）", format_seconds(start)),
    );
}

fn jump_chapter_index(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let chapters = active_chapters(state);
    let Some(chapter) = chapters.get(index) else {
        return;
    };
    let (title, start) = (chapter.title.clone(), chapter.start);
    seek_seconds(state, ui, start);
    show_notice(
        state,
        ui,
        format!("章节：{title}（{}）", format_seconds(start)),
    );
}

fn play_menu_leaves(
    chapters: &[ChapterInfo],
    repeat: bool,
    shuffle: bool,
    speed: f32,
    remember_position: bool,
    show_markers: bool,
) -> Vec<MenuLeaf> {
    const MAX_VISIBLE: usize = 40;
    let mut chapter_items = Vec::new();
    if chapters.is_empty() {
        chapter_items.push(menu_sub("无章节信息", "", false, ""));
    } else {
        for (index, chapter) in chapters.iter().take(MAX_VISIBLE).enumerate() {
            chapter_items.push(menu_sub(
                &chapter.title,
                &format_seconds(chapter.start),
                true,
                &format!("chapter-{index}"),
            ));
        }
        if chapters.len() > MAX_VISIBLE {
            chapter_items.push(menu_sub(
                &crate::i18n::tr_template(
                    "还有 {} 个章节未显示",
                    &[("", &(chapters.len() - MAX_VISIBLE).to_string())],
                ),
                "",
                false,
                "",
            ));
        }
    }
    let has_chapters = !chapters.is_empty();
    chapter_items.push(menu_sub_separator());
    chapter_items.push(menu_sub("上一章节", "[", has_chapters, "chapter-prev"));
    chapter_items.push(menu_sub("下一章节", "]", has_chapters, "chapter-next"));
    chapter_items.push(menu_sub_separator());
    chapter_items.push(menu_sub("添加书签", "H", true, "bookmark-add"));
    chapter_items.push(menu_sub("上一个书签", "", true, "bookmark-prev"));
    chapter_items.push(menu_sub("下一个书签", "", true, "bookmark-next"));
    chapter_items.push(menu_sub("清除书签", "", true, "bookmark-clear"));

    let seek_items = vec![
        menu_sub("跳到开头", "", true, "seek-start"),
        menu_sub("跳到结尾", "", true, "seek-end"),
        menu_sub("后退 10 秒", "", true, "seek-back-10"),
        menu_sub("前进 10 秒", "", true, "seek-forward-10"),
    ];

    let mut speed_items = Vec::new();
    for (label, value) in [
        ("0.25倍", 0.25_f32),
        ("0.5倍", 0.5),
        ("0.75倍", 0.75),
        ("正常速度", 1.0),
        ("1.25倍", 1.25),
        ("1.5倍", 1.5),
        ("2倍", 2.0),
        ("3倍", 3.0),
        ("4倍", 4.0),
    ] {
        let shortcut = if (value - 1.0).abs() < f32::EPSILON {
            "Z"
        } else {
            ""
        };
        let mut item = menu_sub_check(
            label,
            (speed - value).abs() < 0.05,
            &format!("speed-{value}"),
        );
        item.shortcut = crate::i18n::tr(shortcut).into();
        speed_items.push(item);
    }
    speed_items.push(menu_sub("更慢", "X", true, "speed-down"));
    speed_items.push(menu_sub("更快", "C", true, "speed-up"));

    let playlist_items = vec![
        menu_sub("添加文件...", "", true, "playlist-add-file"),
        menu_sub("添加文件夹...", "F2", true, "playlist-add-folder"),
        menu_sub("清空播放列表", "", true, "playlist-clear"),
    ];

    let ab_items = vec![
        menu_sub("设置 A 点", "B", true, "ab-a"),
        menu_sub("设置 B 点", "B", true, "ab-b"),
        menu_sub("关闭 AB 循环", "B", true, "ab-off"),
    ];

    let mut leaves = vec![
        menu_leaf("播放/暂停", "空格键", true, "play-pause"),
        menu_leaf("上一文件", "PgUp", true, "prev-panel"),
        menu_leaf("下一文件", "PgDn", true, "next-panel"),
        menu_separator(),
        menu_group("章节/书签", true, chapter_items),
        menu_group("定位", true, seek_items),
        menu_group("播放速度", true, speed_items),
        menu_arrow("跳略播放", false),
        menu_group("播放列表", true, playlist_items),
        menu_group("AB 区段循环", true, ab_items),
        menu_check("循环播放", repeat, "repeat-toggle"),
        menu_arrow("无缝播放", false),
        menu_separator(),
        menu_check("无序(随机)播放", shuffle, "shuffle-toggle"),
        menu_check(
            "记忆播放位置",
            remember_position,
            "remember-position-toggle",
        ),
        menu_separator(),
    ];
    leaves.push(MenuLeaf {
        title: "播放时隐藏指针".into(),
        shortcut: "".into(),
        enabled: false,
        checkable: true,
        checked: true,
        arrow: false,
        command: "".into(),
        children: Default::default(),
    });
    leaves.push(MenuLeaf {
        title: "播放时禁止屏保".into(),
        shortcut: "".into(),
        enabled: false,
        checkable: true,
        checked: true,
        arrow: false,
        command: "".into(),
        children: Default::default(),
    });
    leaves.push(menu_leaf("播放完关闭媒体", "", false, ""));
    leaves.push(menu_leaf("播放完移除相关文件", "", false, ""));
    leaves.push(menu_separator());
    leaves.push(menu_leaf("鼠标在进度条上时显示时间", "", false, ""));
    leaves.push(menu_leaf("鼠标指向进度条时显示缩略图", "", false, ""));
    leaves.push(menu_check(
        "在进度条上显示书签/章节标记",
        show_markers,
        "marker-toggle",
    ));
    leaves.push(menu_separator());
    leaves.push(menu_leaf("播放设置...", "", true, "device-settings"));
    leaves
}

fn refresh_play_menu(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let chapters = active_chapters(state);
    let (repeat, shuffle, speed, remember_position, show_markers) = {
        let st = state.borrow();
        let speed = st
            .active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.lock().speed())
            .unwrap_or(1.0);
        (
            st.repeat,
            st.shuffle,
            speed,
            st.config.remember_position,
            st.config.show_markers,
        )
    };
    let leaves = play_menu_leaves(
        &chapters,
        repeat,
        shuffle,
        speed,
        remember_position,
        show_markers,
    );
    ui.set_play_menu(ModelRc::new(VecModel::from(leaves)));
}

fn filter_leaf(title: String, command: &str) -> MenuLeaf {
    MenuLeaf {
        title: crate::i18n::tr_notice(&title).into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked: true,
        arrow: false,
        command: command.into(),
        children: Default::default(),
    }
}

fn filter_menu_leaves(st: &AppState, audio_sync_ms: i32) -> (String, Vec<MenuLeaf>) {
    let adjust = st.adjust;
    let rotation = st.rotation;
    let stereo = st.stereo_mode;
    let mut leaves = Vec::new();
    if !adjust.is_neutral() {
        leaves.push(filter_leaf(
            format!(
                "画面调整（亮度 {}，对比度 {}，饱和度 {}）",
                adjust.brightness, adjust.contrast, adjust.saturation
            ),
            "adjust-reset",
        ));
    }
    if rotation != 0 {
        leaves.push(filter_leaf(format!("画面旋转 {rotation}°"), "rotate-0"));
    }
    if st.flip_horizontal {
        leaves.push(filter_leaf("水平翻转".into(), "flip-horizontal"));
    }
    if st.flip_vertical {
        leaves.push(filter_leaf("垂直翻转".into(), "flip-vertical"));
    }
    if stereo != StereoMode::Off {
        leaves.push(filter_leaf(
            format!("3D 视频输出（{}）", stereo_label(stereo)),
            "stereo-off",
        ));
    }
    if st.pano_enabled {
        leaves.push(filter_leaf("360° 全景播放".into(), "pano-toggle"));
    }
    if st.audio_swap {
        leaves.push(filter_leaf("左右声道互换".into(), "audio-swap"));
    }
    if audio_sync_ms != 0 {
        leaves.push(filter_leaf(
            format!("声音同步 {audio_sync_ms:+} ms"),
            "audio-sync-reset",
        ));
    }
    if leaves.is_empty() {
        leaves.push(menu_leaf("无正在使用的滤镜", "", false, ""));
    }
    let key = format!(
        "{}:{}:{}|{rotation}|{}|{}|{stereo:?}|{}|{}|{audio_sync_ms}",
        adjust.brightness,
        adjust.contrast,
        adjust.saturation,
        st.flip_horizontal,
        st.flip_vertical,
        st.pano_enabled,
        st.audio_swap
    );
    (key, leaves)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AudioOutput {
    name: String,
    label: String,
}

fn pulse_sinks() -> Vec<AudioOutput> {
    let Ok(output) = std::process::Command::new("pactl")
        .env("LC_ALL", "C")
        .args(["list", "sinks"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut sinks = Vec::new();
    let mut name = String::new();
    let mut label = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Sink #") {
            if !name.is_empty() {
                let title = if label.is_empty() {
                    name.clone()
                } else {
                    std::mem::take(&mut label)
                };
                sinks.push(AudioOutput {
                    name: std::mem::take(&mut name),
                    label: title,
                });
            }
        } else if let Some(value) = line.strip_prefix("Name: ") {
            name = value.to_string();
        } else if let Some(value) = line.strip_prefix("Description: ") {
            label = value.to_string();
        }
    }
    if !name.is_empty() {
        let title = if label.is_empty() {
            name.clone()
        } else {
            label
        };
        sinks.push(AudioOutput { name, label: title });
    }
    sinks
}

fn translated_sound_leaf(leaf: &MenuLeaf, state: &SoundMenuState<'_>) -> MenuLeaf {
    let children: Vec<MenuSub> = leaf
        .children
        .iter()
        .map(|child| MenuSub {
            title: crate::i18n::tr(child.title.as_str()).into(),
            checked: sound_checked(child.command.as_str(), child.checked, state),
            ..child.clone()
        })
        .collect();
    MenuLeaf {
        title: crate::i18n::tr(leaf.title.as_str()).into(),
        checked: sound_checked(leaf.command.as_str(), leaf.checked, state),
        children: ModelRc::new(VecModel::from(children)),
        ..leaf.clone()
    }
}

struct SoundMenuState<'a> {
    labels: &'a [String],
    selected: Option<usize>,
    sinks: &'a [AudioOutput],
    device: Option<&'a str>,
    device2: Option<&'a str>,
    passthrough: Option<&'a str>,
    volume_control: &'a str,
    muted: bool,
    audio_record_play_folder: bool,
    audio_channel: flint_media::audio::Channel,
    audio_output_rate: u32,
    audio_output_format: u32,
    audio_swap: bool,
    external_audio: Option<&'a str>,
    config: &'a Config,
}

fn sound_checked(command: &str, fallback: bool, state: &SoundMenuState<'_>) -> bool {
    use flint_media::audio::Channel;
    use flint_media::audio_dsp as dsp;
    let flags = audio_effect_flags(state.config);
    match command {
        "audio-stereo" => state.audio_channel == Channel::Stereo,
        "audio-left" => state.audio_channel == Channel::Left,
        "audio-right" => state.audio_channel == Channel::Right,
        "audio-output-auto" => state.audio_output_rate == 0 && state.audio_output_format == 0,
        "audio-format-16" => state.audio_output_format == 0,
        "audio-format-24" => state.audio_output_format == 1,
        "audio-format-32f" => state.audio_output_format == 2,
        "audio-rate-44100" => state.audio_output_rate == 44100,
        "audio-rate-48000" => state.audio_output_rate == 48000,
        "audio-rate-96000" => state.audio_output_rate == 96000,
        "audio-rate-192000" => state.audio_output_rate == 192000,
        "audio-output-method-auto" => state.config.audio_output_method == "auto",
        "audio-output-method-pulse" => state.config.audio_output_method == "pulse",
        "audio-output-method-null" => state.config.audio_output_method == "null",
        "mute-toggle" => state.muted,
        "audio-record-play-folder" => state.audio_record_play_folder,
        "audio-processing-toggle" => state.config.audio_processing,
        "audio-effect-normalize" => flags & dsp::EFFECT_NORMALIZE != 0,
        "audio-effect-crystallize" => flags & dsp::EFFECT_CRYSTALLIZE != 0,
        "audio-effect-denoise" => flags & dsp::EFFECT_DENOISE != 0,
        "audio-effect-reverb" => flags & dsp::EFFECT_REVERB != 0,
        "audio-effect-vocal-remove" => flags & dsp::EFFECT_VOCAL_REMOVE != 0,
        "audio-effect-vocal-enhance" => flags & dsp::EFFECT_VOCAL_ENHANCE != 0,
        "audio-swap" => state.audio_swap,
        "audio-sync-keep-none" => state.config.audio_sync_mode == "none",
        "audio-sync-keep-always" => state.config.audio_sync_mode == "always",
        "audio-sync-reset-exit-toggle" => state.config.audio_sync_reset_exit,
        "equalizer-toggle" => state.config.audio_equalizer,
        "eq-preset-flat" => state.config.audio_eq_preset == "flat",
        "eq-preset-pop" => state.config.audio_eq_preset == "pop",
        "eq-preset-rock" => state.config.audio_eq_preset == "rock",
        "eq-preset-classical" => state.config.audio_eq_preset == "classical",
        "eq-preset-dance" => state.config.audio_eq_preset == "dance",
        "eq-preset-bass" => state.config.audio_eq_preset == "bass",
        "eq-preset-treble" => state.config.audio_eq_preset == "treble",
        "eq-preset-vocal" => state.config.audio_eq_preset == "vocal",
        _ => fallback,
    }
}

fn sound_menu_with_tracks(base: &[MenuLeaf], state: &SoundMenuState<'_>) -> Vec<MenuLeaf> {
    let SoundMenuState {
        labels,
        selected,
        sinks,
        device,
        device2,
        passthrough,
        volume_control,
        muted,
        external_audio,
        config,
        ..
    } = *state;
    let mut track_rows: Vec<MenuSub> = Vec::new();
    if let Some(label) = external_audio {
        track_rows.push(MenuSub {
            title: format!("外部音频：{label}").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: true,
            arrow: false,
            command: "audio-external".into(),
        });
    }
    track_rows.extend(labels.iter().enumerate().map(|(index, label)| MenuSub {
        title: localized_track_label(label).into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked: selected == Some(index),
        arrow: false,
        command: format!("audio-track-{index}").into(),
    }));
    let tracks = ModelRc::new(VecModel::from(track_rows));
    let track_header = crate::i18n::tr("声音输出轨道");
    let device_header = crate::i18n::tr("默认声音输出设备");
    let mut device_children = vec![MenuSub {
        title: crate::i18n::tr("自动选择(推荐)").into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked: device.is_none(),
        arrow: false,
        command: "audio-device-default".into(),
    }];
    for (index, sink) in sinks.iter().enumerate() {
        device_children.push(MenuSub {
            title: sink.label.as_str().into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: device == Some(sink.name.as_str()),
            arrow: false,
            command: format!("audio-device-{index}").into(),
        });
    }
    let device_children = ModelRc::new(VecModel::from(device_children));
    let secondary_header = crate::i18n::tr("次声音输出设备");
    let mut secondary_children = vec![MenuSub {
        title: crate::i18n::tr("不使用(推荐)").into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked: device2.is_none(),
        arrow: false,
        command: "audio-device2-none".into(),
    }];
    for (index, sink) in sinks.iter().enumerate() {
        secondary_children.push(MenuSub {
            title: sink.label.as_str().into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: device2 == Some(sink.name.as_str()),
            arrow: false,
            command: format!("audio-device2-{index}").into(),
        });
    }
    let secondary_children = ModelRc::new(VecModel::from(secondary_children));
    let passthrough_header = crate::i18n::tr("Pass Through 声音输出设备");
    let mut passthrough_children = vec![MenuSub {
        title: crate::i18n::tr("默认声音输出设备(推荐)").into(),
        shortcut: "".into(),
        enabled: true,
        checkable: true,
        checked: passthrough.is_none(),
        arrow: false,
        command: "audio-passthrough-default".into(),
    }];
    for (index, sink) in sinks.iter().enumerate() {
        passthrough_children.push(MenuSub {
            title: sink.label.as_str().into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: passthrough == Some(sink.name.as_str()),
            arrow: false,
            command: format!("audio-passthrough-{index}").into(),
        });
    }
    let passthrough_children = ModelRc::new(VecModel::from(passthrough_children));
    let volume_header = crate::i18n::tr("播放音量控制");
    let volume_children = ModelRc::new(VecModel::from(vec![
        MenuSub {
            title: crate::i18n::tr("默认音量控制(推荐)").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: volume_control == "default",
            arrow: false,
            command: "volume-control-default".into(),
        },
        MenuSub {
            title: crate::i18n::tr("系统波形音量控制").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: volume_control == "system-wave",
            arrow: false,
            command: "volume-control-wave".into(),
        },
        MenuSub {
            title: crate::i18n::tr("系统主音量控制").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: volume_control == "system-master",
            arrow: false,
            command: "volume-control-master".into(),
        },
        MenuSub {
            title: crate::i18n::tr("多通道音量控制").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: volume_control == "multi",
            arrow: false,
            command: "multi-channel-volume-toggle".into(),
        },
        MenuSub {
            title: crate::i18n::tr("主音量 +").into(),
            shortcut: crate::i18n::tr("Shift+↑").into(),
            enabled: true,
            checkable: false,
            checked: false,
            arrow: false,
            command: "master-volume-up".into(),
        },
        MenuSub {
            title: crate::i18n::tr("主音量 -").into(),
            shortcut: crate::i18n::tr("Shift+↓").into(),
            enabled: true,
            checkable: false,
            checked: false,
            arrow: false,
            command: "master-volume-down".into(),
        },
        MenuSub {
            title: crate::i18n::tr("开启 AC3/DTS 音量处理").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: config.audio_ac3_dts,
            arrow: false,
            command: "ac3-dts-volume-toggle".into(),
        },
        MenuSub {
            title: crate::i18n::tr("放大音量").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: false,
            checked: false,
            arrow: false,
            command: "wave-volume-up".into(),
        },
        MenuSub {
            title: crate::i18n::tr("减小音量").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: false,
            checked: false,
            arrow: false,
            command: "wave-volume-down".into(),
        },
        MenuSub {
            title: crate::i18n::tr("音量控制设置...").into(),
            shortcut: "".into(),
            enabled: true,
            checkable: false,
            checked: false,
            arrow: false,
            command: "volume-control-settings".into(),
        },
    ]));
    base.iter()
        .map(|leaf| {
            if !labels.is_empty()
                && (leaf.title.as_str() == "声音输出轨道" || leaf.title.as_str() == track_header)
            {
                MenuLeaf {
                    title: track_header.clone().into(),
                    shortcut: "".into(),
                    enabled: true,
                    checkable: false,
                    checked: false,
                    arrow: true,
                    command: "".into(),
                    children: tracks.clone(),
                }
            } else if leaf.title.as_str() == "默认声音输出设备"
                || leaf.title.as_str() == device_header
            {
                MenuLeaf {
                    title: device_header.clone().into(),
                    shortcut: "".into(),
                    enabled: true,
                    checkable: false,
                    checked: false,
                    arrow: true,
                    command: "".into(),
                    children: device_children.clone(),
                }
            } else if leaf.title.as_str() == "次声音输出设备"
                || leaf.title.as_str() == secondary_header
            {
                MenuLeaf {
                    title: secondary_header.clone().into(),
                    shortcut: "".into(),
                    enabled: true,
                    checkable: false,
                    checked: false,
                    arrow: true,
                    command: "".into(),
                    children: secondary_children.clone(),
                }
            } else if leaf.title.as_str() == "Pass Through 声音输出设备"
                || leaf.title.as_str() == passthrough_header
            {
                MenuLeaf {
                    title: passthrough_header.clone().into(),
                    shortcut: "".into(),
                    enabled: true,
                    checkable: false,
                    checked: false,
                    arrow: true,
                    command: "".into(),
                    children: passthrough_children.clone(),
                }
            } else if leaf.title.as_str() == "播放音量控制" || leaf.title.as_str() == volume_header
            {
                MenuLeaf {
                    title: volume_header.clone().into(),
                    shortcut: "".into(),
                    enabled: true,
                    checkable: false,
                    checked: false,
                    arrow: true,
                    command: "".into(),
                    children: volume_children.clone(),
                }
            } else if leaf.title.as_str() == "静音" {
                MenuLeaf {
                    title: crate::i18n::tr("静音").into(),
                    shortcut: "M".into(),
                    enabled: true,
                    checkable: true,
                    checked: muted,
                    arrow: false,
                    command: "mute-toggle".into(),
                    children: ModelRc::new(VecModel::from(Vec::<MenuSub>::new())),
                }
            } else {
                translated_sound_leaf(leaf, state)
            }
        })
        .collect()
}

fn subtitle_menu_leaves(
    streams: &[SubtitleStreamInfo],
    selected: Option<usize>,
    loaded: bool,
    visible: bool,
    delay: f64,
    config: &Config,
) -> Vec<MenuLeaf> {
    let scale = config.subtitle_scale;
    let color = config.subtitle_color.as_str();
    let top = config.subtitle_top;
    let font = config.subtitle_font.as_str();
    let bold = config.subtitle_bold;
    let shadow = config.subtitle_shadow;
    let mut stream_items = Vec::new();
    if streams.is_empty() {
        stream_items.push(menu_sub("（没有内嵌字幕轨）", "", false, ""));
    }
    for stream in streams {
        stream_items.push(MenuSub {
            title: stream.label().into(),
            shortcut: stream.codec.to_ascii_uppercase().into(),
            enabled: true,
            checkable: true,
            checked: selected == Some(stream.index),
            arrow: false,
            command: format!("subtitle-stream-{}", stream.index).into(),
        });
    }
    stream_items.push(menu_sub_separator());
    stream_items.push(menu_sub("打开字幕...", "Alt+O", true, "subtitle-open"));
    stream_items.push(menu_sub("关闭字幕", "", loaded, "subtitle-off"));

    let mut font_items = Vec::new();
    for (index, (name, family)) in SUBTITLE_FONTS.iter().enumerate() {
        font_items.push(menu_sub_check(
            &crate::i18n::tr_template("字体：{name}", &[("name", name)]),
            font == *family,
            &format!("subtitle-font-{index}"),
        ));
    }
    font_items.push(menu_sub_check("粗体", bold, "subtitle-bold-toggle"));

    let style_items = vec![
        menu_sub_check("字号：小", scale < 0.9, "subtitle-size-0.8"),
        menu_sub_check("字号：中", (0.9..1.15).contains(&scale), "subtitle-size-1"),
        menu_sub_check(
            "字号：大",
            (1.15..1.4).contains(&scale),
            "subtitle-size-1.3",
        ),
        menu_sub_check("字号：特大", scale >= 1.4, "subtitle-size-1.6"),
        menu_sub_separator(),
        menu_sub_check("颜色：白色", color == "white", "subtitle-color-white"),
        menu_sub_check("颜色：黄色", color == "yellow", "subtitle-color-yellow"),
        menu_sub_check("颜色：青色", color == "cyan", "subtitle-color-cyan"),
        menu_sub_check("颜色：绿色", color == "green", "subtitle-color-green"),
        menu_sub_separator(),
        menu_sub_check("位置：底部", !top, "subtitle-pos-bottom"),
        menu_sub_check("位置：顶部", top, "subtitle-pos-top"),
    ];

    let sync_items = vec![
        menu_sub("字幕延迟 -0.5 秒", "", loaded, "subtitle-delay--0.5"),
        menu_sub("字幕延迟 -0.1 秒", "", loaded, "subtitle-delay--0.1"),
        menu_sub("字幕延迟 +0.1 秒", "", loaded, "subtitle-delay-0.1"),
        menu_sub("字幕延迟 +0.5 秒", "", loaded, "subtitle-delay-0.5"),
        MenuSub {
            title: crate::i18n::tr_template(
                "重置字幕延迟（当前 {delay} 秒）",
                &[("delay", &format!("{delay:+.1}"))],
            )
            .into(),
            shortcut: "".into(),
            enabled: loaded,
            checkable: true,
            checked: delay.abs() < f64::EPSILON,
            arrow: false,
            command: "subtitle-delay-reset".into(),
        },
    ];

    let save_items = vec![
        menu_sub("保存为 SRT（视频同目录）", "", loaded, "subtitle-save-srt"),
        menu_sub("保存为 ASS（视频同目录）", "", loaded, "subtitle-save-ass"),
    ];

    let three_d = config.subtitle_3d.as_str();
    let three_d_items = vec![
        menu_sub_check("3D 字幕：关闭", three_d == "off", "subtitle-3d-off"),
        menu_sub_check("3D 字幕：左右排列", three_d == "sbs", "subtitle-3d-sbs"),
        menu_sub_check("3D 字幕：上下排列", three_d == "tab", "subtitle-3d-tab"),
        menu_sub_check(
            "3D 字幕：红蓝立体",
            three_d == "anaglyph",
            "subtitle-3d-anaglyph",
        ),
    ];

    let language = config.subtitle_language.as_str();
    let language_items = vec![
        menu_sub_check("自动选择(推荐)", language == "auto", "subtitle-lang-auto"),
        menu_sub_check("中文", language == "chi", "subtitle-lang-chi"),
        menu_sub_check("英文", language == "eng", "subtitle-lang-eng"),
        menu_sub_check("日文", language == "jpn", "subtitle-lang-jpn"),
        menu_sub_check("韩文", language == "kor", "subtitle-lang-kor"),
    ];

    let tts_items = vec![
        menu_sub("朗读当前字幕", "", loaded, "subtitle-tts-say"),
        menu_sub("停止朗读", "", config.tts_enabled, "subtitle-tts-stop"),
    ];

    let leaves = vec![
        menu_group("选择字幕", true, stream_items),
        MenuLeaf {
            title: "显示字幕".into(),
            shortcut: "Alt+H".into(),
            enabled: true,
            checkable: true,
            checked: visible,
            arrow: false,
            command: "subtitle-toggle".into(),
            children: Default::default(),
        },
        menu_leaf("字幕浏览器...", "Alt+E", true, "subtitle-browser"),
        menu_leaf("手动输入字幕...", "Alt+P", true, "subtitle-manual"),
        menu_leaf("直接输出在视频中(TV输出)", "Alt+I", false, ""),
        menu_leaf("覆盖 VMR/EVR 输出(高品质)", "Alt+I", false, ""),
        menu_separator(),
        menu_group("字幕字体处理", true, font_items),
        menu_group("字幕样式", true, style_items),
        menu_group("字幕同步(帧率)", true, sync_items),
        menu_group("保存字幕", loaded, save_items),
        menu_group("3D 字幕(立体)", true, three_d_items),
        menu_group("字幕语言", true, language_items),
        menu_leaf("在线字幕搜索", "", true, "subtitle-download-open"),
        MenuLeaf {
            title: "实时字幕翻译".into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: config.translate_enabled,
            arrow: false,
            command: "subtitle-translate-toggle".into(),
            children: Default::default(),
        },
        MenuLeaf {
            title: "生成有声字幕 (TTS)".into(),
            shortcut: "".into(),
            enabled: true,
            checkable: true,
            checked: config.tts_enabled,
            arrow: true,
            command: "subtitle-tts-toggle".into(),
            children: ModelRc::new(VecModel::from(tts_items)),
        },
        menu_separator(),
        menu_leaf("复制当前字幕到剪贴板", "", loaded, "subtitle-copy"),
        menu_check(
            "将标准图形字幕输出到画面外",
            config.subtitle_graphic_outside,
            "subtitle-graphic-outside-toggle",
        ),
        menu_check(
            "将标准文本字幕输出到画面内",
            config.subtitle_text_inside,
            "subtitle-text-inside-toggle",
        ),
        menu_check(
            "尽可能将字幕显示在画面底部",
            config.subtitle_bottom,
            "subtitle-bottom-toggle",
        ),
        menu_check(
            "开启 ASS/SSA 动态字幕",
            config.ass_styles,
            "ass-styles-toggle",
        ),
        menu_check(
            "字幕边框不透明",
            config.subtitle_border_opaque,
            "subtitle-border-opaque-toggle",
        ),
        menu_check("字幕阴影", shadow, "subtitle-shadow-toggle"),
        menu_check(
            "平滑输出 VMR/EVR 字幕",
            config.subtitle_smooth_vmr,
            "subtitle-smooth-vmr-toggle",
        ),
        menu_separator(),
        menu_leaf("字幕设置...", "", true, "subtitle-settings"),
    ];
    leaves
}

fn favorite_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string())
}

fn refresh_favorite_menu(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (favorites, show_memory) = {
        let st = state.borrow();
        (
            st.config.favorites.clone(),
            st.config.favorites_memory_points,
        )
    };
    let mut leaves = Vec::new();
    if favorites.is_empty() {
        leaves.push(menu_leaf("暂无收藏", "", false, ""));
    } else {
        for (index, path) in favorites.iter().enumerate() {
            let mut children = Vec::new();
            if show_memory {
                let points = memory_points_for(&state.borrow(), path);
                for (point_index, point) in points.iter().enumerate() {
                    children.push(menu_sub(
                        &crate::i18n::tr_template("记忆点 {}", &[("", &format_seconds(*point))]),
                        "",
                        true,
                        &format!("memory-{index}-{point_index}"),
                    ));
                }
            }
            let mut leaf = menu_leaf(
                &favorite_label(path),
                "",
                true,
                &format!("favorite-{index}"),
            );
            if !children.is_empty() {
                leaf.arrow = true;
                leaf.children = ModelRc::new(VecModel::from(children));
            }
            leaves.push(leaf);
        }
    }
    leaves.push(menu_separator());
    leaves.push(menu_leaf(
        "添加当前文件",
        "Alt+Insert",
        true,
        "favorite-add-file",
    ));
    leaves.push(menu_leaf(
        "添加当前文件夹",
        "Ctrl+Insert",
        true,
        "favorite-add-folder",
    ));
    leaves.push(menu_separator());
    leaves.push(menu_leaf(
        "清除收藏",
        "",
        !favorites.is_empty(),
        "favorite-clear",
    ));
    ui.set_favorite_menu(ModelRc::new(VecModel::from(leaves)));
}

fn add_favorite(state: &Rc<RefCell<AppState>>, ui: &MainWindow, path: &Path) {
    let entry = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let entry = entry.display().to_string();
    let added = {
        let mut st = state.borrow_mut();
        if st.config.favorites.contains(&entry) {
            false
        } else {
            st.config.favorites.push(entry.clone());
            if let Err(error) = st.config.save() {
                log::warn!("保存配置失败：{error}");
            }
            true
        }
    };
    refresh_favorite_menu(state, ui);
    let name = favorite_label(&entry);
    let message = if added {
        format!("已收藏 {name}")
    } else {
        format!("{name} 已在收藏中")
    };
    show_notice(state, ui, message);
}

fn add_favorite_file(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let source = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.source.clone())
    };
    match source {
        Some(source) if is_file_source(&source) && Path::new(&source).is_file() => {
            add_favorite(state, ui, Path::new(&source));
        }
        _ => show_notice(state, ui, "仅支持收藏正在播放的本地文件".into()),
    }
}

fn add_favorite_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let folder = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .filter(|panel| is_file_source(&panel.source))
            .and_then(|panel| Path::new(&panel.source).parent().map(Path::to_path_buf))
            .or_else(|| (!st.file_dir.as_os_str().is_empty()).then(|| st.file_dir.clone()))
    };
    match folder {
        Some(folder) => add_favorite(state, ui, &folder),
        None => show_notice(state, ui, "没有可收藏的文件夹".into()),
    }
}

fn clear_favorites(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let count = {
        let mut st = state.borrow_mut();
        let count = st.config.favorites.len();
        if count > 0 {
            st.config.favorites.clear();
            if let Err(error) = st.config.save() {
                log::warn!("保存配置失败：{error}");
            }
        }
        count
    };
    refresh_favorite_menu(state, ui);
    let message = if count > 0 {
        format!("已清除 {count} 个收藏")
    } else {
        "当前没有收藏".to_string()
    };
    show_notice(state, ui, message);
}

fn memory_points_for(st: &AppState, source: &str) -> Vec<f64> {
    let key = file_key(source);
    let mut points = Vec::new();
    if let Some((_, position)) = st.config.resume.iter().find(|(path, _)| path == &key) {
        points.push(*position);
    }
    if let Some((_, times)) = st.config.bookmarks.iter().find(|(path, _)| path == &key) {
        points.extend(times.iter().copied());
    }
    points
}

fn jump_memory_point(
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
    favorite_index: usize,
    point_index: usize,
) {
    let (path, target) = {
        let st = state.borrow();
        let Some(entry) = st.config.favorites.get(favorite_index) else {
            return;
        };
        let points = memory_points_for(&st, entry);
        let Some(target) = points.get(point_index).copied() else {
            return;
        };
        (PathBuf::from(entry), target)
    };
    if !path.is_file() {
        show_notice(state, ui, format!("收藏路径不存在：{}", path.display()));
        return;
    }
    let existing = {
        let st = state.borrow();
        st.panels
            .iter()
            .position(|panel| Path::new(&panel.source) == path)
    };
    let running = existing
        .and_then(|index| state.borrow().panels.get(index).map(|panel| panel.running))
        .unwrap_or(false);
    if let Some(index) = existing.filter(|_| running) {
        let mut st = state.borrow_mut();
        st.active = Some(index);
        st.refresh_menu();
        if let Some(panel) = st.panels.get_mut(index) {
            panel.engine.lock().seek_to(target);
        }
    } else {
        state.borrow_mut().pending_seek = Some((file_key(path.to_string_lossy().as_ref()), target));
        match existing {
            Some(index) => start_panel(state, ui, index),
            None => open_media_file(state, ui, &path),
        }
    }
    show_notice(
        state,
        ui,
        format!("跳转到记忆点 {}", format_seconds(target)),
    );
}

fn open_favorite(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let Some(entry) = state.borrow().config.favorites.get(index).cloned() else {
        return;
    };
    let path = PathBuf::from(entry);
    if path.is_dir() {
        let folder = path.canonicalize().unwrap_or(path);
        state.borrow_mut().file_dir = folder.clone();
        load_file_dir(state, ui, folder.clone());
        refresh_browser(state, ui);
        ui.set_file_folder_mode(false);
        ui.set_file_open(true);
        show_notice(
            state,
            ui,
            format!(
                "打开收藏文件夹 {}",
                favorite_label(&folder.display().to_string())
            ),
        );
    } else if path.is_file() {
        let existing = {
            let st = state.borrow();
            st.panels
                .iter()
                .position(|panel| Path::new(&panel.source) == path)
        };
        if let Some(index) = existing {
            stop_panel(state, ui, index);
            start_panel(state, ui, index);
        } else {
            open_media_file(state, ui, &path);
        }
    } else {
        show_notice(state, ui, format!("收藏路径不存在：{}", path.display()));
    }
}

fn seek_seconds(state: &Rc<RefCell<AppState>>, ui: &MainWindow, seconds: f64) {
    let mut st = state.borrow_mut();
    let Some(index) = st.active else {
        return;
    };
    let Some(panel) = st.panels.get_mut(index) else {
        return;
    };
    if !panel.running || !is_file_source(&panel.source) {
        return;
    }
    let duration = panel
        .shared
        .status()
        .info
        .as_ref()
        .map(|info| info.duration)
        .unwrap_or(0.0);
    if duration <= 0.0 {
        return;
    }
    let target = seconds.clamp(0.0, (duration - 0.05).max(0.0));
    panel.engine.lock().seek_to(target);
    ui.set_bar_progress((target / duration) as f32);
}

fn set_ab_start(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Some(position) = active_file_position(state) else {
        show_notice(state, ui, "AB 循环仅支持正在播放的本地文件".into());
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.ab_start = Some(position);
        if st.ab_end.is_some_and(|end| end <= position) {
            st.ab_end = None;
        }
    }
    show_notice(state, ui, format!("A 点：{}", format_seconds(position)));
}

fn set_ab_end(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let Some(position) = active_file_position(state) else {
        show_notice(state, ui, "AB 循环仅支持正在播放的本地文件".into());
        return;
    };
    let start = state.borrow().ab_start;
    let Some(start) = start else {
        show_notice(state, ui, "请先设置 A 点".into());
        return;
    };
    if position <= start + 0.2 {
        show_notice(state, ui, "B 点必须晚于 A 点".into());
        return;
    }
    state.borrow_mut().ab_end = Some(position);
    show_notice(
        state,
        ui,
        format!(
            "AB 循环：{} - {}",
            format_seconds(start),
            format_seconds(position)
        ),
    );
}

fn clear_ab_loop(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let had = {
        let mut st = state.borrow_mut();
        let had = st.ab_start.is_some();
        st.ab_start = None;
        st.ab_end = None;
        had
    };
    let message = if had {
        "AB 循环：关"
    } else {
        "AB 循环：未设置"
    };
    show_notice(state, ui, message.into());
}

fn cycle_ab_loop(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (start, end) = {
        let st = state.borrow();
        (st.ab_start, st.ab_end)
    };
    match (start, end) {
        (None, _) => set_ab_start(state, ui),
        (Some(_), None) => set_ab_end(state, ui),
        _ => clear_ab_loop(state, ui),
    }
}

fn clear_playlist(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let remove: Vec<usize> = {
        let st = state.borrow();
        st.panels
            .iter()
            .enumerate()
            .filter(|(index, panel)| is_file_source(&panel.source) && st.active != Some(*index))
            .map(|(index, _)| index)
            .collect()
    };
    if remove.is_empty() {
        show_notice(state, ui, "播放列表中没有可清空的项目".into());
        return;
    }
    let count = remove.len();
    for index in remove.into_iter().rev() {
        close_panel(state, ui, index);
    }
    show_notice(
        state,
        ui,
        format!("已清空 {} 个播放列表项目（保留当前播放）", count),
    );
}

fn adjust_speed(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: f32) {
    let current = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.lock().speed())
            .unwrap_or(1.0)
    };
    set_playback_speed(state, ui, current + delta);
}

fn set_playback_speed(state: &Rc<RefCell<AppState>>, ui: &MainWindow, speed: f32) {
    let speed = (speed.clamp(0.1, 4.0) * 10.0).round() / 10.0;
    {
        let st = state.borrow();
        let Some(panel) = st.active.and_then(|index| st.panels.get(index)) else {
            return;
        };
        if !panel.running || !is_file_source(&panel.source) {
            return;
        }
        panel.engine.lock().set_speed(speed);
    }
    refresh_play_menu(state, ui);
    show_notice(state, ui, format!("播放速度 {speed:.1}x"));
}

fn volume_ceiling(config: &Config) -> f32 {
    if config.audio_multi_channel_volume {
        (config.audio_max_volume as f32 / 100.0).clamp(1.0, 5.0)
    } else {
        1.0
    }
}

fn adjust_volume(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: f32) {
    if state.borrow().volume_control == "system-master" {
        step_system_volume((delta * 100.0).round() as i32, state, ui);
        return;
    }
    let ceiling = volume_ceiling(&state.borrow().config);
    let volume = (ui.get_volume() + delta).clamp(0.0, ceiling);
    ui.set_volume(volume);
    ui.set_muted(volume <= 0.0);
    apply_audio_settings(state, ui);
}

fn set_volume_control(mode: &str, label: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let multi = mode == "multi";
    {
        let mut st = state.borrow_mut();
        st.volume_control = mode.to_string();
        st.config.volume_control = mode.to_string();
        st.config.audio_multi_channel_volume = multi;
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_volume_control(mode.into());
    ui.set_multi_channel_volume(multi);
    if !multi && ui.get_volume() > 1.0 {
        ui.set_volume(1.0);
        apply_audio_settings(state, ui);
    }
    show_notice(state, ui, format!("音量控制方式：{label}"));
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|error| format!("无法运行 pactl：{error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.is_empty() {
            "pactl 执行失败".to_string()
        } else {
            message
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn system_volume_percent() -> Option<i32> {
    let text = pactl(&["get-sink-volume", "@DEFAULT_SINK@"]).ok()?;
    let before = text.split('%').next()?;
    before.split_whitespace().last()?.trim().parse().ok()
}

fn step_system_volume(delta: i32, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let step = format!("{delta:+}%");
    match pactl(&["set-sink-volume", "@DEFAULT_SINK@", &step]) {
        Ok(_) => match system_volume_percent() {
            Some(percent) => show_notice(state, ui, format!("系统音量：{percent}%")),
            None => show_notice(state, ui, "系统音量已调整".to_string()),
        },
        Err(error) => show_notice(state, ui, format!("调整系统音量失败：{error}")),
    }
}

fn toggle_system_mute(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    match pactl(&["set-sink-mute", "@DEFAULT_SINK@", "toggle"]) {
        Ok(_) => {
            let muted = pactl(&["get-sink-mute", "@DEFAULT_SINK@"])
                .map(|text| text.contains("yes"))
                .unwrap_or(false);
            show_notice(
                state,
                ui,
                format!("系统静音：{}", if muted { "开" } else { "关" }),
            );
        }
        Err(error) => show_notice(state, ui, format!("切换系统静音失败：{error}")),
    }
}

fn apply_audio_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (volume, muted) = (ui.get_volume(), ui.get_muted());
    let st = state.borrow();
    if let Some(panel) = st.active.and_then(|index| st.panels.get(index)) {
        let engine = panel.engine.lock();
        engine.set_volume(volume);
        engine.set_muted(muted);
    }
}

fn set_audio_channel(
    channel: flint_media::audio::Channel,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    let (key, label) = match channel {
        flint_media::audio::Channel::Stereo => ("stereo", "立体声"),
        flint_media::audio::Channel::Left => ("left", "左声道"),
        flint_media::audio::Channel::Right => ("right", "右声道"),
    };
    state.borrow_mut().audio_channel = channel;
    ui.set_audio_channel(key.into());
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_channel(channel);
    }
    show_notice(state, ui, format!("选择声音：{label}"));
}

fn cycle_audio_channel(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let next = match state.borrow().audio_channel {
        flint_media::audio::Channel::Stereo => flint_media::audio::Channel::Left,
        flint_media::audio::Channel::Left => flint_media::audio::Channel::Right,
        flint_media::audio::Channel::Right => flint_media::audio::Channel::Stereo,
    };
    set_audio_channel(next, state, ui);
}

fn toggle_audio_swap(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let swap = {
        let mut st = state.borrow_mut();
        st.audio_swap = !st.audio_swap;
        st.audio_swap
    };
    ui.set_audio_swap(swap);
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_swap(swap);
    }
    show_notice(
        state,
        ui,
        format!("左右声道互换：{}", if swap { "开" } else { "关" }),
    );
}

fn audio_effect_flags(config: &Config) -> u32 {
    use flint_media::audio_dsp;
    if !config.audio_processing {
        return 0;
    }
    let mut flags = 0;
    if config.audio_effect_normalize {
        flags |= audio_dsp::EFFECT_NORMALIZE;
    }
    if config.audio_effect_crystallize {
        flags |= audio_dsp::EFFECT_CRYSTALLIZE;
    }
    if config.audio_effect_denoise {
        flags |= audio_dsp::EFFECT_DENOISE;
    }
    if config.audio_effect_reverb {
        flags |= audio_dsp::EFFECT_REVERB;
    }
    if config.audio_effect_vocal_remove {
        flags |= audio_dsp::EFFECT_VOCAL_REMOVE;
    }
    if config.audio_effect_vocal_enhance {
        flags |= audio_dsp::EFFECT_VOCAL_ENHANCE;
    }
    if config.audio_equalizer {
        flags |= audio_dsp::EFFECT_EQUALIZER;
    }
    flags
}

fn eq_preset_gains(preset: &str) -> [f32; flint_media::audio_dsp::EQ_BANDS.len()] {
    match preset {
        "pop" => [-1.5, -0.5, 1.5, 3.0, 3.5, 2.0, 0.0, -1.0, -1.5, -2.0],
        "rock" => [4.5, 3.5, 2.0, -0.5, -1.5, -0.5, 1.5, 3.0, 4.0, 4.5],
        "classical" => [4.0, 3.0, 2.0, 1.0, 0.0, 0.0, -1.0, -1.5, -1.5, -2.0],
        "dance" => [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 1.0, 2.5, 3.5, 4.0],
        "bass" => [7.0, 6.0, 4.5, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        "treble" => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.5, 6.5],
        "vocal" => [-2.0, -1.5, -0.5, 1.5, 3.5, 4.0, 3.5, 1.5, 0.0, -1.0],
        _ => [0.0; 10],
    }
}

fn eq_preset_label(preset: &str) -> &'static str {
    match preset {
        "pop" => "流行",
        "rock" => "摇滚",
        "classical" => "古典",
        "dance" => "舞曲",
        "bass" => "低音增强",
        "treble" => "高音增强",
        "vocal" => "人声",
        _ => "默认",
    }
}

#[derive(Clone, Copy)]
enum AudioEffect {
    Normalize,
    Crystallize,
    Denoise,
    Reverb,
    VocalRemove,
    VocalEnhance,
}

impl AudioEffect {
    fn label(self) -> &'static str {
        match self {
            AudioEffect::Normalize => "规格化",
            AudioEffect::Crystallize => "晶化",
            AudioEffect::Denoise => "降噪",
            AudioEffect::Reverb => "混响",
            AudioEffect::VocalRemove => "语音消除",
            AudioEffect::VocalEnhance => "语音增强",
        }
    }

    fn enabled(self, config: &Config) -> bool {
        match self {
            AudioEffect::Normalize => config.audio_effect_normalize,
            AudioEffect::Crystallize => config.audio_effect_crystallize,
            AudioEffect::Denoise => config.audio_effect_denoise,
            AudioEffect::Reverb => config.audio_effect_reverb,
            AudioEffect::VocalRemove => config.audio_effect_vocal_remove,
            AudioEffect::VocalEnhance => config.audio_effect_vocal_enhance,
        }
    }

    fn set_enabled(self, config: &mut Config, enabled: bool) {
        match self {
            AudioEffect::Normalize => config.audio_effect_normalize = enabled,
            AudioEffect::Crystallize => config.audio_effect_crystallize = enabled,
            AudioEffect::Denoise => config.audio_effect_denoise = enabled,
            AudioEffect::Reverb => config.audio_effect_reverb = enabled,
            AudioEffect::VocalRemove => config.audio_effect_vocal_remove = enabled,
            AudioEffect::VocalEnhance => config.audio_effect_vocal_enhance = enabled,
        }
    }

    fn apply_to_ui(self, ui: &MainWindow, enabled: bool) {
        match self {
            AudioEffect::Normalize => ui.set_audio_effect_normalize(enabled),
            AudioEffect::Crystallize => ui.set_audio_effect_crystallize(enabled),
            AudioEffect::Denoise => ui.set_audio_effect_denoise(enabled),
            AudioEffect::Reverb => ui.set_audio_effect_reverb(enabled),
            AudioEffect::VocalRemove => ui.set_audio_effect_vocal_remove(enabled),
            AudioEffect::VocalEnhance => ui.set_audio_effect_vocal_enhance(enabled),
        }
    }
}

fn sync_audio_effects(state: &Rc<RefCell<AppState>>) {
    let (flags, gains, engine) = {
        let st = state.borrow();
        (
            audio_effect_flags(&st.config),
            eq_preset_gains(&st.config.audio_eq_preset),
            st.active
                .and_then(|index| st.panels.get(index))
                .map(|panel| panel.engine.clone()),
        )
    };
    if let Some(engine) = engine {
        let engine = engine.lock();
        engine.set_audio_effects(flags);
        engine.set_audio_eq_gains(&gains);
    }
}

fn toggle_audio_processing(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.audio_processing = !st.config.audio_processing;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.audio_processing
    };
    ui.set_audio_processing(enabled);
    sync_audio_effects(state);
    show_notice(
        state,
        ui,
        format!("声音处理：{}", if enabled { "开" } else { "关" }),
    );
}

fn toggle_audio_effect(state: &Rc<RefCell<AppState>>, ui: &MainWindow, effect: AudioEffect) {
    let enabled = {
        let mut st = state.borrow_mut();
        let enabled = !effect.enabled(&st.config);
        effect.set_enabled(&mut st.config, enabled);
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        enabled
    };
    effect.apply_to_ui(ui, enabled);
    sync_audio_effects(state);
    show_notice(
        state,
        ui,
        format!("{}：{}", effect.label(), if enabled { "开" } else { "关" }),
    );
}

fn toggle_equalizer(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.audio_equalizer = !st.config.audio_equalizer;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.audio_equalizer
    };
    ui.set_equalizer_enabled(enabled);
    sync_audio_effects(state);
    show_notice(
        state,
        ui,
        format!("均衡器：{}", if enabled { "开" } else { "关" }),
    );
}

fn set_eq_preset(state: &Rc<RefCell<AppState>>, ui: &MainWindow, preset: &str) {
    {
        let mut st = state.borrow_mut();
        st.config.audio_eq_preset = preset.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_equalizer_preset(preset.into());
    sync_audio_effects(state);
    show_notice(
        state,
        ui,
        format!("均衡器预置：{}", eq_preset_label(preset)),
    );
}

fn set_audio_output(
    rate: Option<u32>,
    format: Option<u32>,
    label: &str,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    let (rate, format) = {
        let mut st = state.borrow_mut();
        if let Some(rate) = rate {
            st.audio_output_rate = rate;
        }
        if let Some(format) = format {
            st.audio_output_format = format;
        }
        st.config.audio_output_rate = st.audio_output_rate;
        st.config.audio_output_format = st.audio_output_format;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.audio_menu_key.clear();
        (st.audio_output_rate, st.audio_output_format)
    };
    ui.set_audio_output_rate(rate as i32);
    ui.set_audio_output_format(format as i32);
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        let engine = engine.lock();
        engine.set_audio_output_rate(rate);
        engine.set_audio_output_format(flint_media::audio::SampleFormat::from_code(format));
    }
    show_notice(state, ui, format!("声音输出位率/采样率：{label}"));
}

fn set_audio_output_method(
    method: &str,
    label: &str,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    {
        let mut st = state.borrow_mut();
        st.config.audio_output_method = method.to_string();
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_audio_output_method(method.into());
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine
            .lock()
            .set_audio_output_method(flint_media::audio_method_code(method));
    }
    show_notice(state, ui, format!("声音输出方式：{label}"));
}

fn active_engine(state: &Rc<RefCell<AppState>>) -> Option<SharedEngine> {
    let st = state.borrow();
    st.active
        .and_then(|index| st.panels.get(index))
        .map(|panel| panel.engine.clone())
}

fn toggle_multi_channel_volume(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    if state.borrow().volume_control == "multi" {
        set_volume_control("default", "默认音量控制(推荐)", state, ui);
    } else {
        set_volume_control("multi", "多通道音量控制", state, ui);
    }
}

fn toggle_ac3_dts_volume(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (enabled, gain) = {
        let mut st = state.borrow_mut();
        st.config.audio_ac3_dts = !st.config.audio_ac3_dts;
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        (st.config.audio_ac3_dts, st.config.audio_ac3_dts_gain_db)
    };
    ui.set_ac3_dts_volume(enabled);
    if let Some(engine) = active_engine(state) {
        engine.lock().set_audio_ac3_dts(enabled, gain);
    }
    show_notice(
        state,
        ui,
        if enabled {
            format!("AC3/DTS 音量处理：开（放大 {gain:.1} dB）")
        } else {
            "AC3/DTS 音量处理：关".to_string()
        },
    );
}

fn open_volume_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (multi, max, ac3, gain) = {
        let st = state.borrow();
        (
            st.config.audio_multi_channel_volume,
            st.config.audio_max_volume as f32,
            st.config.audio_ac3_dts,
            st.config.audio_ac3_dts_gain_db,
        )
    };
    ui.set_volume_settings_multi(multi);
    ui.set_volume_settings_max(max);
    ui.set_volume_settings_ac3(ac3);
    ui.set_volume_settings_ac3_gain(gain);
    ui.set_volume_settings_open(true);
}

fn apply_volume_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let multi = ui.get_volume_settings_multi();
    let max = ui.get_volume_settings_max().round().clamp(100.0, 500.0);
    let ac3 = ui.get_volume_settings_ac3();
    let gain = ui.get_volume_settings_ac3_gain().clamp(0.0, 24.0);
    let mode = {
        let mut st = state.borrow_mut();
        st.config.audio_multi_channel_volume = multi;
        st.config.audio_max_volume = max as u32;
        st.config.audio_ac3_dts = ac3;
        st.config.audio_ac3_dts_gain_db = gain;
        if multi {
            st.volume_control = "multi".to_string();
        } else if st.volume_control == "multi" {
            st.volume_control = "default".to_string();
        }
        st.config.volume_control = st.volume_control.clone();
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.volume_control.clone()
    };
    ui.set_volume_settings_open(false);
    ui.set_volume_settings_max(max);
    ui.set_volume_settings_ac3_gain(gain);
    ui.set_volume_control(mode.as_str().into());
    ui.set_multi_channel_volume(multi);
    ui.set_ac3_dts_volume(ac3);
    if let Some(engine) = active_engine(state) {
        engine.lock().set_audio_ac3_dts(ac3, gain);
    }
    let ceiling = if multi { max / 100.0 } else { 1.0 };
    if ui.get_volume() > ceiling {
        ui.set_volume(ceiling);
        apply_audio_settings(state, ui);
    }
    show_notice(
        state,
        ui,
        format!(
            "音量控制设置：最大 {max:.0}%{}",
            if ac3 {
                format!("，AC3/DTS 放大 {gain:.1} dB")
            } else {
                String::new()
            }
        ),
    );
}

fn set_audio_passthrough_device(
    device: Option<String>,
    label: &str,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    {
        let mut st = state.borrow_mut();
        st.config.audio_passthrough_device = device.clone().unwrap_or_default();
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    if let Some(engine) = active_engine(state) {
        engine.lock().set_audio_passthrough_device(device);
    }
    show_notice(state, ui, format!("Pass Through 声音输出设备：{label}"));
}

fn sink_index(sinks: &[AudioOutput], name: Option<&String>) -> i32 {
    name.and_then(|name| sinks.iter().position(|sink| &sink.name == name))
        .map(|index| index as i32 + 1)
        .unwrap_or(0)
}

fn open_audio_output_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (device, device2, passthrough, method, format, rate, sinks) = {
        let st = state.borrow();
        (
            st.audio_device.clone(),
            st.audio_device2.clone(),
            (!st.config.audio_passthrough_device.is_empty())
                .then(|| st.config.audio_passthrough_device.clone()),
            st.config.audio_output_method.clone(),
            st.audio_output_format,
            st.audio_output_rate,
            st.audio_sinks.clone(),
        )
    };
    let mut default_devices: Vec<slint::SharedString> =
        vec![crate::i18n::tr("自动选择(推荐)").into()];
    let mut passthrough_devices: Vec<slint::SharedString> =
        vec![crate::i18n::tr("使用默认输出设备(推荐)").into()];
    let mut secondary_devices: Vec<slint::SharedString> =
        vec![crate::i18n::tr("不使用(推荐)").into()];
    for sink in &sinks {
        default_devices.push(sink.label.as_str().into());
        passthrough_devices.push(sink.label.as_str().into());
        secondary_devices.push(sink.label.as_str().into());
    }
    ui.set_audio_default_devices(ModelRc::new(VecModel::from(default_devices)));
    ui.set_audio_passthrough_devices(ModelRc::new(VecModel::from(passthrough_devices)));
    ui.set_audio_secondary_devices(ModelRc::new(VecModel::from(secondary_devices)));
    ui.set_audio_settings_device(sink_index(&sinks, device.as_ref()));
    ui.set_audio_settings_passthrough(sink_index(&sinks, passthrough.as_ref()));
    ui.set_audio_settings_secondary(sink_index(&sinks, device2.as_ref()));
    ui.set_audio_settings_method(match method.as_str() {
        "pulse" => 1,
        "null" => 2,
        _ => 0,
    });
    ui.set_audio_settings_format(format.min(2) as i32);
    ui.set_audio_settings_rate(match rate {
        44100 => 1,
        48000 => 2,
        96000 => 3,
        192000 => 4,
        _ => 0,
    });
    ui.set_audio_settings_open(true);
}

fn apply_audio_output_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let sinks = state.borrow().audio_sinks.clone();
    let pick = |index: i32| -> Option<String> {
        if index <= 0 {
            None
        } else {
            sinks
                .get((index - 1) as usize)
                .map(|sink| sink.name.clone())
        }
    };
    let device = pick(ui.get_audio_settings_device());
    let device2 = pick(ui.get_audio_settings_secondary());
    let passthrough = pick(ui.get_audio_settings_passthrough());
    let method = match ui.get_audio_settings_method() {
        1 => "pulse",
        2 => "null",
        _ => "auto",
    };
    let format = ui.get_audio_settings_format().clamp(0, 2) as u32;
    let rate = match ui.get_audio_settings_rate() {
        1 => 44100,
        2 => 48000,
        3 => 96000,
        4 => 192000,
        _ => 0,
    };
    {
        let mut st = state.borrow_mut();
        st.audio_device = device.clone();
        st.audio_device2 = device2.clone();
        st.audio_output_format = format;
        st.audio_output_rate = rate;
        st.config.audio_device = device.clone().unwrap_or_default();
        st.config.audio_device2 = device2.clone().unwrap_or_default();
        st.config.audio_passthrough_device = passthrough.clone().unwrap_or_default();
        st.config.audio_output_method = method.to_string();
        st.config.audio_output_format = format;
        st.config.audio_output_rate = rate;
        st.audio_menu_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_audio_output_method(method.into());
    ui.set_audio_output_format(format as i32);
    ui.set_audio_output_rate(rate as i32);
    if let Some(engine) = active_engine(state) {
        let engine = engine.lock();
        engine.set_audio_device(device);
        engine.set_audio_device2(device2);
        engine.set_audio_passthrough_device(passthrough);
        engine.set_audio_output_method(flint_media::audio_method_code(method));
        engine.set_audio_output_format(flint_media::audio::SampleFormat::from_code(format));
        engine.set_audio_output_rate(rate);
    }
    ui.set_audio_settings_open(false);
    show_notice(state, ui, "声音输出设置已应用".to_string());
}

fn audio_language_index(language: &str) -> i32 {
    match language {
        "chi" => 1,
        "eng" => 2,
        "jpn" => 3,
        "kor" => 4,
        _ => 0,
    }
}

fn audio_language_key(index: i32) -> &'static str {
    match index {
        1 => "chi",
        2 => "eng",
        3 => "jpn",
        4 => "kor",
        _ => "auto",
    }
}

fn apply_audio_general_settings(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let language = audio_language_key(ui.get_audio_general_language()).to_string();
    let select_language = ui.get_audio_general_select_language();
    let ac3 = ui.get_audio_general_ac3();
    let processing = ui.get_audio_general_processing();
    let reset_exit = ui.get_audio_general_reset_exit();
    let (effects, gain) = {
        let mut st = state.borrow_mut();
        st.config.audio_language = language;
        st.config.audio_select_language = select_language;
        st.config.audio_ac3_dts = ac3;
        st.config.audio_processing = processing;
        st.config.audio_sync_reset_exit = reset_exit;
        st.audio_menu_key.clear();
        st.audio_auto_key.clear();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        (
            audio_effect_flags(&st.config),
            st.config.audio_ac3_dts_gain_db,
        )
    };
    ui.set_audio_sync_reset_exit(reset_exit);
    ui.set_ac3_dts_volume(ac3);
    ui.set_audio_processing(processing);
    ui.set_audio_general_settings_open(false);
    if let Some(engine) = active_engine(state) {
        let engine = engine.lock();
        engine.set_audio_effects(effects);
        engine.set_audio_ac3_dts(ac3, gain);
    }
    show_notice(state, ui, "声音设置已应用".to_string());
}

fn set_audio_device(
    device: Option<String>,
    label: &str,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    {
        let mut st = state.borrow_mut();
        st.audio_device = device.clone();
        st.config.audio_device = device.clone().unwrap_or_default();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.audio_menu_key.clear();
    }
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_device(device);
    }
    show_notice(state, ui, format!("声音输出设备：{label}"));
}

fn set_audio_device2(
    device: Option<String>,
    label: &str,
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
) {
    {
        let mut st = state.borrow_mut();
        st.audio_device2 = device.clone();
        st.config.audio_device2 = device.clone().unwrap_or_default();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.audio_menu_key.clear();
    }
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_device2(device);
    }
    show_notice(state, ui, format!("次声音输出设备：{label}"));
}

fn select_audio_track(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let (shared, label) = {
        let st = state.borrow();
        let Some(panel) = st.active.and_then(|active| st.panels.get(active)) else {
            return;
        };
        let Some(label) = panel
            .shared
            .status()
            .info
            .and_then(|info| info.audio_tracks.get(index).cloned())
        else {
            return;
        };
        (panel.shared.clone(), label)
    };
    shared.set_audio_track(Some(index));
    shared.clear_external_audio();
    show_notice(
        state,
        ui,
        format!("音频轨道：{}", localized_track_tail(&label)),
    );
}

fn auto_select_audio_track(ui: &MainWindow, st: &mut AppState, active_index: Option<usize>) {
    if !st.config.audio_select_language || st.config.audio_language == "auto" {
        return;
    }
    let Some(panel) = active_index.and_then(|index| st.panels.get(index)) else {
        return;
    };
    if panel.shared.audio_track().is_some() {
        return;
    }
    let Some(info) = panel.shared.status().info else {
        return;
    };
    let key = format!("{}|{}", panel.source, info.audio_tracks.join("\u{1f}"));
    if st.audio_auto_key == key {
        return;
    }
    let Some(track) = info
        .audio_tracks
        .iter()
        .position(|label| audio_label_matches_language(label, &st.config.audio_language))
    else {
        return;
    };
    let shared = panel.shared.clone();
    let message = crate::i18n::tr_notice(&format!(
        "音频轨道：{}",
        localized_track_tail(&info.audio_tracks[track])
    ));
    st.audio_auto_key = key;
    shared.set_audio_track(Some(track));
    st.notice = Some(Notice {
        message: message.clone(),
        until: Instant::now() + NOTICE_DURATION,
    });
    ui.set_status_error(message.as_str().into());
}

fn audio_label_matches_language(label: &str, preference: &str) -> bool {
    let tail = track_label_parts(label)
        .map(|(_, tail)| tail)
        .unwrap_or_else(|| label.to_string());
    let token = tail
        .split([' ', '-', '_', '(', '/'])
        .next()
        .unwrap_or_default();
    if language_matches(token, preference) {
        return true;
    }
    let lower = tail.to_lowercase();
    match preference {
        "chi" => ["中文", "汉语", "国语", "國語"]
            .iter()
            .any(|name| tail.contains(name)),
        "eng" => lower.contains("english") || tail.contains("英语") || tail.contains("英文"),
        "jpn" => lower.contains("japanese") || tail.contains("日语") || tail.contains("日本語"),
        "kor" => lower.contains("korean") || tail.contains("韩语") || tail.contains("한국어"),
        _ => false,
    }
}

fn localized_track_label(label: &str) -> String {
    match track_label_parts(label) {
        Some((index, tail)) => {
            let tail = localized_track_tail(&tail);
            if crate::i18n::language() == crate::i18n::Language::En {
                format!("Audio track {index}: {tail}")
            } else {
                format!("音轨 {index}: {tail}")
            }
        }
        None => label.to_string(),
    }
}

fn localized_track_tail(label: &str) -> String {
    let tail = track_label_parts(label)
        .map(|(_, tail)| tail)
        .unwrap_or_else(|| label.to_string());
    if crate::i18n::language() != crate::i18n::Language::En {
        return tail;
    }
    let Some(position) = tail.find("声道") else {
        return tail;
    };
    let digits_start = tail[..position]
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    let digits = &tail[digits_start..position];
    if digits.is_empty() {
        return tail;
    }
    format!(
        "{} {digits} channels{}",
        &tail[..digits_start].trim_end(),
        &tail[position + "声道".len()..]
    )
}

fn track_label_parts(label: &str) -> Option<(String, String)> {
    let rest = label.strip_prefix("音轨 ")?;
    let (index, tail) = rest.split_once(": ")?;
    Some((index.to_string(), tail.to_string()))
}

fn nudge_audio_sync(state: &Rc<RefCell<AppState>>, ui: &MainWindow, delta: i32) {
    let sync_ms = (ui.get_audio_sync_ms() + delta).clamp(-5000, 5000);
    ui.set_audio_sync_ms(sync_ms);
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_sync_ms(sync_ms);
    }
    persist_audio_sync(state, sync_ms);
    show_notice(
        state,
        ui,
        format!("声音同步：{}", audio_sync_label(sync_ms)),
    );
}

fn persist_audio_sync(state: &Rc<RefCell<AppState>>, sync_ms: i32) {
    let mut st = state.borrow_mut();
    if st.config.audio_sync_mode != "always" {
        return;
    }
    st.config.audio_sync_ms = sync_ms;
    if let Err(error) = st.config.save() {
        log::warn!("保存配置失败：{error}");
    }
}

fn set_audio_sync_mode(state: &Rc<RefCell<AppState>>, ui: &MainWindow, mode: &str) {
    {
        let mut st = state.borrow_mut();
        st.config.audio_sync_mode = mode.into();
        if mode == "always" {
            st.config.audio_sync_ms = ui.get_audio_sync_ms();
        }
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_audio_sync_mode(mode.into());
    let label = if mode == "always" {
        "始终保持声音同步"
    } else {
        "不保持声音同步"
    };
    show_notice(state, ui, format!("音频同步保持：{label}"));
}

fn toggle_audio_sync_reset_exit(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.audio_sync_reset_exit = !st.config.audio_sync_reset_exit;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.audio_sync_reset_exit
    };
    ui.set_audio_sync_reset_exit(enabled);
    show_notice(
        state,
        ui,
        format!(
            "退出时初始化声音同步：{}",
            if enabled { "开" } else { "关" }
        ),
    );
}

fn reset_audio_sync(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    ui.set_audio_sync_ms(0);
    let engine = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .map(|panel| panel.engine.clone())
    };
    if let Some(engine) = engine {
        engine.lock().set_audio_sync_ms(0);
    }
    persist_audio_sync(state, 0);
    show_notice(state, ui, "声音同步：已复位".to_string());
}

fn audio_sync_label(sync_ms: i32) -> String {
    match sync_ms.cmp(&0) {
        std::cmp::Ordering::Greater => format!("滞后 {sync_ms}ms"),
        std::cmp::Ordering::Less => format!("超前 {}ms", -sync_ms),
        std::cmp::Ordering::Equal => "0ms".to_string(),
    }
}

fn update_seek_hover(state: &Rc<RefCell<AppState>>, ui: &MainWindow, fraction: f32) {
    let st = state.borrow();
    let text = st
        .active
        .and_then(|index| st.panels.get(index))
        .filter(|panel| panel.running && is_file_source(&panel.source))
        .and_then(|panel| panel.shared.status().info)
        .filter(|info| info.duration > 0.0)
        .map(|info| format_seconds(fraction.clamp(0.0, 1.0) as f64 * info.duration))
        .unwrap_or_default();
    ui.set_seek_hover_time(text.into());
}

fn toggle_recording(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (engine, recording, directory) = {
        let st = state.borrow();
        let Some(panel) = st.active.and_then(|index| st.panels.get(index)) else {
            return;
        };
        if !panel.running {
            return;
        }
        (
            panel.engine.clone(),
            panel.recording,
            video_record_directory(&st),
        )
    };
    let engine = engine.lock();
    if recording {
        match engine.stop_recording() {
            Some(Ok(summary)) => {
                log::info!(
                    "录制已保存：{}（{} 帧，丢 {} 帧）",
                    summary.path.display(),
                    summary.frames,
                    summary.dropped
                );
                drop(engine);
                set_panel_recording(state, ui, false);
                show_notice(state, ui, format!("录制已保存：{}", summary.path.display()));
            }
            Some(Err(error)) => {
                drop(engine);
                set_panel_recording(state, ui, false);
                show_notice(state, ui, error);
            }
            None => {
                drop(engine);
                set_panel_recording(state, ui, false);
            }
        }
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    if let Err(error) = std::fs::create_dir_all(&directory) {
        drop(engine);
        show_notice(state, ui, format!("创建录制目录失败：{error}"));
        return;
    }
    let path = directory.join(format!("record_{millis}.mp4"));
    match engine.start_recording(&path) {
        Ok(()) => {
            drop(engine);
            set_panel_recording(state, ui, true);
            show_notice(state, ui, format!("开始录制：{}", path.display()));
        }
        Err(error) => {
            drop(engine);
            show_notice(state, ui, format!("开始录制失败：{error}"));
        }
    }
}

fn set_panel_recording(state: &Rc<RefCell<AppState>>, ui: &MainWindow, recording: bool) {
    let mut st = state.borrow_mut();
    let index = st.active;
    if let Some(index) = index {
        if let Some(panel) = st.panels.get_mut(index) {
            panel.recording = recording;
            let row = panel.row(true);
            st.panel_model.set_row_data(index, row);
        }
    }
    ui.set_recording(recording);
}

fn record_directory(st: &AppState, play_folder: bool, configured: &Path) -> PathBuf {
    if !play_folder {
        return configured.to_path_buf();
    }
    st.active
        .and_then(|index| st.panels.get(index))
        .map(|panel| panel.source.clone())
        .filter(|source| is_file_source(source))
        .and_then(|source| Path::new(&source).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| configured.to_path_buf())
}

fn video_record_directory(st: &AppState) -> PathBuf {
    record_directory(
        st,
        st.config.video_record_play_folder,
        &st.config.video_record_dir,
    )
}

fn audio_record_directory(st: &AppState) -> PathBuf {
    record_directory(
        st,
        st.config.audio_record_play_folder,
        &st.config.audio_record_dir,
    )
}

fn toggle_record_play_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow, folder: RecordFolder) {
    let enabled = {
        let mut st = state.borrow_mut();
        let value = match folder {
            RecordFolder::Video => {
                st.config.video_record_play_folder = !st.config.video_record_play_folder;
                st.config.video_record_play_folder
            }
            RecordFolder::Audio => {
                st.config.audio_record_play_folder = !st.config.audio_record_play_folder;
                st.config.audio_record_play_folder
            }
        };
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        value
    };
    let state_text = if enabled { "开" } else { "关" };
    match folder {
        RecordFolder::Video => {
            ui.set_video_record_play_folder(enabled);
            show_notice(state, ui, format!("视频录制储存在播放文件夹：{state_text}"));
        }
        RecordFolder::Audio => {
            ui.set_audio_record_play_folder(enabled);
            show_notice(state, ui, format!("声音录制储存在播放文件夹：{state_text}"));
        }
    }
}

fn toggle_audio_recording(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let prepared = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .filter(|panel| panel.running)
            .map(|panel| {
                let rate = if st.audio_output_rate == 0 {
                    48_000
                } else {
                    st.audio_output_rate
                };
                (panel.engine.clone(), audio_record_directory(&st), rate)
            })
    };
    let Some((engine, directory, rate)) = prepared else {
        show_notice(state, ui, "当前面板未在播放，无法录制声音".to_string());
        return;
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let engine = engine.lock();
    if engine.is_audio_recording() {
        let outcome = engine.stop_audio_recording();
        drop(engine);
        match outcome {
            Some(Ok(summary)) => show_notice(
                state,
                ui,
                format!(
                    "声音录制已保存：{}（{:.1} 秒）",
                    summary.path.display(),
                    summary.seconds
                ),
            ),
            Some(Err(error)) => show_notice(state, ui, error),
            None => show_notice(state, ui, "没有进行中的声音录制".to_string()),
        }
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&directory) {
        drop(engine);
        show_notice(state, ui, format!("创建声音录制目录失败：{error}"));
        return;
    }
    let path = directory.join(format!("audio_{millis}.wav"));
    let started = engine.start_audio_recording(&path, rate);
    drop(engine);
    match started {
        Ok(()) => show_notice(state, ui, format!("开始录制声音：{}", path.display())),
        Err(error) => show_notice(state, ui, format!("开始录制声音失败：{error}")),
    }
}

fn open_video_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let directory = state.borrow().config.video_record_dir.clone();
    open_folder(state, ui, &directory, "视频录制");
}

fn open_audio_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let directory = state.borrow().config.audio_record_dir.clone();
    open_folder(state, ui, &directory, "声音录制");
}

fn open_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow, directory: &Path, label: &str) {
    if let Err(error) = std::fs::create_dir_all(directory) {
        show_notice(state, ui, format!("创建{label}目录失败：{error}"));
        return;
    }
    match std::process::Command::new("xdg-open")
        .arg(directory)
        .spawn()
    {
        Ok(_) => log::info!("打开{label}文件夹：{}", directory.display()),
        Err(error) => show_notice(state, ui, format!("打开{label}文件夹失败：{error}")),
    }
}

fn always_on_top_supported() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none()
}

fn apply_topmost(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let (mode, playing) = {
        let st = state.borrow();
        let playing = st.panels.iter().any(|panel| panel.running && !panel.paused);
        (st.config.topmost_mode.clone(), playing)
    };
    let pinned = match mode.as_str() {
        "always" => true,
        "playing" => playing,
        "fullscreen" => ui.get_fullscreen(),
        _ => false,
    };
    if ui.get_pinned() != pinned {
        ui.set_pinned(pinned);
    }
}

fn set_topmost_mode(mode: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        if st.config.topmost_mode == mode {
            return;
        }
        st.config.topmost_mode = mode.to_string();
        st.config.always_on_top = mode == "always";
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_topmost_mode(mode.into());
    apply_topmost(state, ui);
    let label = match mode {
        "always" => "总在最前端",
        "playing" => "播放时总在最前端",
        "fullscreen" => "全屏时总在最前端",
        _ => "不置顶",
    };
    show_notice(state, ui, format!("置顶方式：{label}"));
    if mode != "never" && !always_on_top_supported() {
        show_notice(
            state,
            ui,
            "当前 Wayland 会话不支持窗口置顶（X11 / XWayland 下可用）".into(),
        );
    }
}

fn toggle_topmost(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let next = if ui.get_pinned() { "never" } else { "always" };
    set_topmost_mode(next, state, ui);
}

fn copy_file_info(kind: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let source = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .or_else(|| st.panels.first())
            .map(|panel| panel.source.clone())
    };
    let Some(source) = source else {
        show_notice(state, ui, "当前没有打开的文件".into());
        return;
    };
    let path = Path::new(&source);
    let text = match kind {
        "name" => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&source)
            .to_string(),
        "dir" => path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default(),
        _ => source.clone(),
    };
    if text.is_empty() {
        show_notice(state, ui, "当前没有可复制的文件路径".into());
        return;
    }
    match copy_to_clipboard(&text) {
        Ok(()) => show_notice(state, ui, format!("已复制到剪贴板：{text}")),
        Err(error) => show_notice(state, ui, format!("复制失败：{error}")),
    }
}

fn set_fullscreen(
    state: &Rc<RefCell<AppState>>,
    ui: &MainWindow,
    fullscreen: bool,
    hide_timer: &Rc<Timer>,
) {
    if fullscreen && state.borrow().panels.is_empty() {
        return;
    }
    state.borrow_mut().fullscreen = fullscreen;
    ui.set_fullscreen(fullscreen);
    ui.window().set_fullscreen(fullscreen);
    if fullscreen {
        show_chrome(ui, hide_timer);
    } else {
        ui.set_chrome_visible(true);
    }
    apply_topmost(state, ui);
}

fn save_snapshot(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let capture = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| {
                panel.last_frame.clone().map(|frame| {
                    let position = panel.engine.lock().position_seconds();
                    let directory = if st.config.snapshot_play_folder {
                        Path::new(&panel.source)
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| st.config.snapshot_dir.clone())
                    } else {
                        st.config.snapshot_dir.clone()
                    };
                    (
                        frame,
                        position,
                        directory,
                        st.config.snapshot_format.clone(),
                        st.config.snapshot_name.clone(),
                    )
                })
            })
    };
    let Some((frame, position, directory, format, name_mode)) = capture else {
        show_notice(state, ui, "当前没有可截取的画面".to_string());
        return;
    };
    let extension = match format.as_str() {
        "jpeg" => "jpg",
        "bmp" => "bmp",
        _ => "png",
    };
    let stem = if name_mode == "playback" {
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
    {
        let mut st = state.borrow_mut();
        st.last_snapshot = Some(path.clone());
    }
    let notices = state.borrow().notice_tx.clone();
    thread::spawn(move || {
        let result = (|| -> Result<PathBuf, String> {
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("创建截图目录失败：{error}"))?;
            let buffer = image::RgbaImage::from_raw(
                frame.width as u32,
                frame.height as u32,
                frame.rgba.clone(),
            )
            .ok_or_else(|| "截图缓冲尺寸无效".to_string())?;
            if extension == "jpg" {
                image::DynamicImage::ImageRgba8(buffer)
                    .to_rgb8()
                    .save(&path)
                    .map_err(|error| format!("保存截图失败：{error}"))?;
            } else {
                buffer
                    .save(&path)
                    .map_err(|error| format!("保存截图失败：{error}"))?;
            }
            Ok(path)
        })();
        match result {
            Ok(path) => log::info!("截图已保存：{}", path.display()),
            Err(error) => {
                log::warn!("{error}");
                let _ = notices.send(error);
            }
        }
    });
}

fn set_snapshot_format(format: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.config.snapshot_format = format.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_snapshot_format(format.into());
    let label = match format {
        "jpeg" => "JPEG",
        "bmp" => "BMP",
        _ => "PNG",
    };
    show_notice(state, ui, format!("截图格式：{label}"));
}

fn set_snapshot_name(mode: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.config.snapshot_name = mode.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_snapshot_name(mode.into());
    let label = if mode == "playback" {
        "以播放时间"
    } else {
        "以当前时间"
    };
    show_notice(state, ui, format!("截图命名：{label}"));
}

fn toggle_snapshot_play_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let value = {
        let mut st = state.borrow_mut();
        st.config.snapshot_play_folder = !st.config.snapshot_play_folder;
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
        st.config.snapshot_play_folder
    };
    ui.set_snapshot_play_folder(value);
    show_notice(
        state,
        ui,
        format!("储存到播放文件夹：{}", if value { "开" } else { "关" }),
    );
}

fn open_snapshot_folder(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let directory = state.borrow().config.snapshot_dir.clone();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        show_notice(state, ui, format!("创建截图目录失败：{error}"));
        return;
    }
    match std::process::Command::new("xdg-open")
        .arg(&directory)
        .spawn()
    {
        Ok(_) => log::info!("打开截图文件夹：{}", directory.display()),
        Err(error) => show_notice(state, ui, format!("打开截图文件夹失败：{error}")),
    }
}

fn delete_last_snapshot(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let last = state.borrow_mut().last_snapshot.take();
    let Some(path) = last else {
        show_notice(state, ui, "没有可删除的采集文件".to_string());
        return;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => show_notice(state, ui, format!("已删除：{}", path.display())),
        Err(error) => show_notice(state, ui, format!("删除失败：{error}")),
    }
}

fn set_post_action(mode: &str, state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let mut st = state.borrow_mut();
        st.config.post_action = mode.to_string();
        if let Err(error) = st.config.save() {
            log::warn!("保存配置失败：{error}");
        }
    }
    ui.set_post_action(mode.into());
    let label = match mode {
        "exit-current" => "播放完当前后退出",
        "exit-all" => "播放完全部后退出",
        "off-current" => "播放完当前后关机",
        "off-all" => "播放完全部后关机",
        _ => "不处理",
    };
    show_notice(state, ui, format!("收尾处理：{label}"));
}

fn cycle_post_action(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let next = {
        let st = state.borrow();
        match st.config.post_action.as_str() {
            "exit-current" => "exit-all",
            "exit-all" => "off-current",
            "off-current" => "off-all",
            "off-all" => "none",
            _ => "exit-current",
        }
    };
    set_post_action(next, state, ui);
}

fn show_notice(state: &Rc<RefCell<AppState>>, ui: &MainWindow, message: String) {
    let message = crate::i18n::tr_notice(&message);
    {
        let mut st = state.borrow_mut();
        st.notice = Some(Notice {
            message: message.clone(),
            until: Instant::now() + NOTICE_DURATION,
        });
    }
    ui.set_status_error(message.as_str().into());
}

fn update_panel_stats(panel: &mut PanelState) {
    let snapshot = panel.shared.stats_snapshot();
    let status = panel.shared.status();
    let nominal = status
        .info
        .as_ref()
        .map(|info| info.nominal_fps)
        .unwrap_or(30.0);
    panel.stats.fps = format!("{:.2} -> {:.2} fps", snapshot.actual_fps, nominal);
}

fn clear_seek_markers(ui: &MainWindow) {
    ui.set_bar_bookmarks(ModelRc::new(VecModel::from(Vec::<f32>::new())));
    ui.set_bar_chapters(ModelRc::new(VecModel::from(Vec::<f32>::new())));
}

fn update_seek_markers(ui: &MainWindow, panel: &PanelState, show: bool) {
    let (bookmarks, chapters) = if show && is_file_source(&panel.source) {
        let duration = panel
            .shared
            .status()
            .info
            .as_ref()
            .map(|info| info.duration)
            .unwrap_or(0.0);
        if duration > 0.0 {
            let fraction = |time: f64| (time / duration).clamp(0.0, 1.0) as f32;
            let bookmarks = panel
                .bookmarks
                .iter()
                .copied()
                .filter(|time| *time > 0.0 && *time < duration)
                .map(fraction)
                .collect::<Vec<f32>>();
            let chapters = panel
                .shared
                .chapters()
                .iter()
                .filter(|chapter| chapter.start > 0.0 && chapter.start < duration)
                .map(|chapter| fraction(chapter.start))
                .collect::<Vec<f32>>();
            (bookmarks, chapters)
        } else {
            (Vec::new(), Vec::new())
        }
    } else {
        (Vec::new(), Vec::new())
    };
    ui.set_bar_bookmarks(ModelRc::new(VecModel::from(bookmarks)));
    ui.set_bar_chapters(ModelRc::new(VecModel::from(chapters)));
}

fn update_active_ui(ui: &MainWindow, st: &mut AppState) {
    while let Ok(message) = st.notice_rx.try_recv() {
        st.notice = Some(Notice {
            message,
            until: Instant::now() + NOTICE_DURATION,
        });
    }
    if st
        .notice
        .as_ref()
        .is_some_and(|notice| Instant::now() >= notice.until)
    {
        st.notice = None;
    }
    let active_index = st.active;
    let chapters = active_index
        .and_then(|index| st.panels.get(index))
        .map(|panel| panel.shared.chapters())
        .unwrap_or_default();
    let chapters_changed = !st
        .chapter_source
        .as_ref()
        .is_some_and(|source| Arc::ptr_eq(source, &chapters));
    if chapters_changed {
        st.chapter_source = Some(chapters.clone());
    }
    let (subtitle_menu_key, subtitle_menu) = {
        let panel = active_index.and_then(|index| st.panels.get(index));
        let streams = panel
            .map(|panel| panel.subtitle_streams.clone())
            .unwrap_or_default();
        let selected = panel.and_then(|panel| panel.subtitle_stream);
        let loaded = panel.is_some_and(|panel| panel.subtitle.is_some());
        let delay = panel.map(|panel| panel.subtitle_delay).unwrap_or(0.0);
        let visible = ui.get_subtitles_visible();
        let labels: Vec<String> = streams.iter().map(SubtitleStreamInfo::label).collect();
        let key = format!(
            "{active_index:?}|{}|{selected:?}|{loaded}|{visible}|{delay:.3}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            labels.join("\u{1f}"),
            st.config.subtitle_scale,
            st.config.subtitle_color,
            st.config.subtitle_top,
            st.config.subtitle_font,
            st.config.subtitle_bold,
            st.config.subtitle_shadow,
            st.config.tts_enabled,
            st.config.subtitle_language,
            st.config.translate_enabled
        );
        let leaves = subtitle_menu_leaves(&streams, selected, loaded, visible, delay, &st.config);
        (key, leaves)
    };
    if st.subtitle_menu_key != subtitle_menu_key {
        st.subtitle_menu_key = subtitle_menu_key;
        ui.set_subtitle_menu(ModelRc::new(VecModel::from(subtitle_menu)));
    }
    let (skin_menu_key, skin_menu) = {
        let ids: Vec<String> = st.skin_library.iter().map(Skin::id).collect();
        (
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                st.config.skin,
                st.config.menu_accent,
                st.config.skin_opacity,
                st.config.logo_file,
                st.config.logo_play_file,
                st.config.logo_show,
                st.config.logo_play_show,
                st.config.skin_layer_output,
                st.config.popup_menu_type,
                st.config.menu_output_mode,
                st.config.touch_skin,
                st.config.video_skin_start,
                ids.join("\u{1f}")
            ),
            skin_menu_leaves(&st.skin_library, &st.config),
        )
    };
    if st.skin_menu_key != skin_menu_key {
        st.skin_menu_key = skin_menu_key;
        ui.set_skin_menu(ModelRc::new(VecModel::from(skin_menu)));
    }
    if let Some(color) = accent_value(&st.config.menu_accent) {
        if ui.get_skin_accent() != color {
            ui.set_skin_accent(color);
        }
    }
    if let Ok(sinks) = st.audio_sink_rx.try_recv() {
        if st.audio_sinks != sinks {
            st.audio_sinks = sinks;
            st.audio_menu_key.clear();
        }
    }
    let (audio_menu_key, sound_menu) = {
        let panel = active_index.and_then(|index| st.panels.get(index));
        let labels = panel
            .and_then(|panel| panel.shared.status().info)
            .map(|info| info.audio_tracks)
            .unwrap_or_default();
        let selected = panel.and_then(|panel| panel.shared.audio_track());
        let external_audio = panel.and_then(|panel| panel.shared.external_audio_label());
        let sinks_key = st
            .audio_sinks
            .iter()
            .map(|sink| sink.name.as_str())
            .collect::<Vec<_>>()
            .join("\u{1e}");
        let muted = ui.get_muted();
        let effects = audio_effect_flags(&st.config);
        let passthrough = (!st.config.audio_passthrough_device.is_empty())
            .then_some(st.config.audio_passthrough_device.as_str());
        let key = format!(
            "{active_index:?}|{}|{selected:?}|{:?}|{:?}|{sinks_key}|{}|{muted}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}|{}|{}",
            labels.join("\u{1f}"),
            st.audio_device,
            st.audio_device2,
            st.volume_control,
            st.config.audio_record_play_folder,
            st.audio_channel,
            st.audio_output_rate,
            st.audio_output_format,
            st.audio_swap,
            effects,
            st.config.audio_equalizer,
            st.config.audio_eq_preset,
            st.config.audio_output_method,
            passthrough,
            st.config.audio_multi_channel_volume,
            st.config.audio_ac3_dts,
            st.config.audio_ac3_dts_gain_db,
            st.config.audio_sync_mode,
            st.config.audio_sync_reset_exit,
            st.config.audio_processing
        );
        let key = format!("{key}|{external_audio:?}");
        (
            key,
            sound_menu_with_tracks(
                &st.sound_menu_base,
                &SoundMenuState {
                    labels: &labels,
                    selected,
                    sinks: &st.audio_sinks,
                    device: st.audio_device.as_deref(),
                    device2: st.audio_device2.as_deref(),
                    passthrough,
                    volume_control: &st.volume_control,
                    muted,
                    audio_record_play_folder: st.config.audio_record_play_folder,
                    audio_channel: st.audio_channel,
                    audio_output_rate: st.audio_output_rate,
                    audio_output_format: st.audio_output_format,
                    audio_swap: st.audio_swap,
                    external_audio: external_audio.as_deref(),
                    config: &st.config,
                },
            ),
        )
    };
    if st.audio_menu_key != audio_menu_key {
        st.audio_menu_key = audio_menu_key;
        ui.set_sound_menu(ModelRc::new(VecModel::from(sound_menu)));
    }
    auto_select_audio_track(ui, st, active_index);
    let audio_sync_ms = ui.get_audio_sync_ms();
    let (filter_menu_key, filter_menu) = filter_menu_leaves(st, audio_sync_ms);
    if st.filter_menu_key != filter_menu_key {
        st.filter_menu_key = filter_menu_key;
        ui.set_filter_menu(ModelRc::new(VecModel::from(filter_menu)));
    }
    let active = active_index.and_then(|index| st.panels.get(index));
    if chapters_changed {
        let speed = active
            .map(|panel| panel.engine.lock().speed())
            .unwrap_or(1.0);
        let leaves = play_menu_leaves(
            &chapters,
            st.repeat,
            st.shuffle,
            speed,
            st.config.remember_position,
            st.config.show_markers,
        );
        ui.set_play_menu(ModelRc::new(VecModel::from(leaves)));
    }
    let subtitle_loaded =
        active.is_some_and(|panel| panel.subtitle.is_some() || panel.subtitle2.is_some());
    if ui.get_subtitle_loaded() != subtitle_loaded {
        ui.set_subtitle_loaded(subtitle_loaded);
    }
    let (subtitle_text, cue_style) = if ui.get_subtitles_visible() {
        active
            .and_then(|panel| {
                panel.subtitle.as_ref().map(|track| {
                    let position = panel.engine.lock().position_seconds() + panel.subtitle_delay;
                    let cue = (st.config.ass_styles && !ui.get_manual_open())
                        .then(|| track.cue_at(position))
                        .flatten()
                        .map(|cue| cue.style.clone());
                    (track.text_at(position).to_string(), cue)
                })
            })
            .unwrap_or_default()
    } else {
        (String::new(), None)
    };
    let displayed_subtitle = if ui.get_subtitles_visible() {
        let secondary = active
            .and_then(|panel| {
                panel.subtitle2.as_ref().map(|track| {
                    let position = panel.engine.lock().position_seconds() + panel.subtitle_delay;
                    track.text_at(position).to_string()
                })
            })
            .unwrap_or_default();
        if subtitle_text.is_empty() {
            secondary
        } else if secondary.is_empty() {
            subtitle_text.clone()
        } else {
            format!("{subtitle_text}\n{secondary}")
        }
    } else {
        String::new()
    };
    if ui.get_subtitle_text().as_str() != displayed_subtitle.as_str() {
        ui.set_subtitle_text(displayed_subtitle.as_str().into());
    }
    apply_subtitle_cue_style(ui, cue_style.as_ref());
    let translation =
        if st.config.translate_enabled && !subtitle_text.is_empty() && !ui.get_manual_open() {
            if st.translate_pending != subtitle_text {
                st.translate_pending = subtitle_text.clone();
                let slot = st.translation.clone();
                let text = subtitle_text.clone();
                thread::spawn(move || {
                    if let Some(translated) = translate_text(&text) {
                        *slot.lock() = Some((text, translated));
                    }
                });
            }
            match st.translation.lock().clone() {
                Some((source, translated)) if source == subtitle_text => translated,
                _ => String::new(),
            }
        } else {
            st.translate_pending.clear();
            String::new()
        };
    if ui.get_subtitle_translation().as_str() != translation.as_str() {
        ui.set_subtitle_translation(translation.as_str().into());
    }
    if subtitle_text.is_empty() {
        st.tts_spoken.clear();
    } else if st.tts_enabled && st.tts_spoken != subtitle_text {
        st.tts_spoken = subtitle_text.clone();
        speak_subtitle(&subtitle_text);
    }
    ui.set_active_index(active_index.map(|index| index as i32).unwrap_or(0));
    let message = match &st.notice {
        Some(notice) => notice.message.clone(),
        None => crate::i18n::tr_notice(
            active
                .and_then(|panel| panel.last_error.as_deref())
                .unwrap_or_default(),
        ),
    };
    if ui.get_status_error().as_str() != message.as_str() {
        ui.set_status_error(message.as_str().into());
    }

    let switched = st.osd_active != active_index;
    if !switched && st.osd_at.elapsed() < Duration::from_millis(500) {
        if let Some(panel) = active {
            if ui.get_running() != panel.running {
                ui.set_running(panel.running);
            }
            if ui.get_frozen() != panel.frozen {
                ui.set_frozen(panel.frozen);
            }
            if ui.get_recording() != panel.recording {
                ui.set_recording(panel.recording);
            }
        }
        return;
    }
    st.osd_active = active_index;
    st.osd_at = Instant::now();
    let cpu = st.cpu.sample();

    let Some(panel) = active else {
        ui.set_running(false);
        ui.set_frozen(false);
        ui.set_recording(false);
        ui.set_bar_elapsed("00:00:00".into());
        ui.set_bar_duration("00:00:00".into());
        ui.set_bar_progress(0.0);
        ui.set_bar_codec("".into());
        ui.set_bar_size("".into());
        ui.set_bar_rate("".into());
        ui.set_bar_bitrate("".into());
        ui.set_bar_source("".into());
        ui.set_bar_format("".into());
        clear_seek_markers(ui);
        clear_osd(ui);
        ui.set_osd_brief_text("".into());
        ui.set_osd_playback_text("".into());
        return;
    };
    let label = source_label(&st.devices, &panel.source);
    ui.set_running(panel.running);
    ui.set_frozen(panel.frozen);
    ui.set_recording(panel.recording);
    ui.set_bar_source(label.as_str().into());
    ui.set_bar_format(source_format(&panel.source).into());
    update_seek_markers(ui, panel, st.config.show_markers);

    let snapshot = panel.shared.stats_snapshot();
    let status = panel.shared.status();
    let timeline = if is_file_source(&panel.source) {
        format_seconds(panel.engine.lock().position_seconds())
    } else {
        status
            .started_at
            .map(|started| format_clock(started.elapsed()))
            .unwrap_or_else(|| "00:00:00".into())
    };

    ui.set_osd_file(osd_tokens(vec![(label.clone(), TONE_GREEN)]));
    ui.set_osd_time(osd_tokens(vec![
        (sysinfo::local_hms(), TONE_WHITE),
        (crate::i18n::tr(", 时间轴: "), TONE_WHITE),
        (timeline.clone(), TONE_GREEN),
        (crate::i18n::tr(", #帧数: "), TONE_WHITE),
        (snapshot.frames.to_string(), TONE_GREEN),
    ]));
    ui.set_osd_config(osd_tokens(vec![
        (crate::i18n::tr("*默认配置"), TONE_RED),
        (", CPU: ".into(), TONE_WHITE),
        (format!("{cpu:.1}%"), TONE_GREEN),
    ]));
    ui.set_osd_version(osd_tokens(vec![
        (env!("CARGO_PKG_VERSION").into(), TONE_WHITE),
        (", ".into(), TONE_WHITE),
        (sysinfo::os_description(), TONE_CYAN),
    ]));
    ui.set_osd_audio(osd_tokens(vec![(
        crate::i18n::tr("无音频输入"),
        TONE_WHITE,
    )]));
    ui.set_bar_elapsed(timeline.as_str().into());

    ui.set_bar_duration("00:00:00".into());
    let Some(info) = status.info.as_ref() else {
        ui.set_bar_codec("".into());
        ui.set_bar_size("".into());
        ui.set_bar_rate("".into());
        ui.set_bar_bitrate("".into());
        clear_osd_details(ui);
        return;
    };

    let chip_codec = if info.transport == "V4L2" {
        info.pixel_format.as_str()
    } else {
        info.codec.as_str()
    };
    ui.set_bar_codec(chip_codec.into());
    ui.set_bar_size(format!("{}×{}", info.width, info.height).as_str().into());
    ui.set_bar_rate(format!("{:.2}", info.nominal_fps).as_str().into());
    ui.set_bar_duration(format_seconds(info.duration).into());
    let progress = if is_file_source(&panel.source) && info.duration > 0.0 {
        (panel.engine.lock().position_seconds() / info.duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    ui.set_bar_progress(progress);
    ui.set_bar_bitrate(format!("{:.2}M", snapshot.bitrate_mbps).as_str().into());

    let duration_text = format_seconds(info.duration);
    let status_text = if !panel.running {
        crate::i18n::tr("已停止")
    } else if panel.paused || panel.frozen {
        crate::i18n::tr("暂停")
    } else {
        crate::i18n::tr("播放中")
    };
    let speed = panel.engine.lock().speed();
    let volume_text = format!("{:.0}%", (ui.get_volume() * 100.0).round());
    ui.set_osd_brief_text(format!("{label}   {timeline} / {duration_text}").into());
    ui.set_osd_playback_text(
        format!(
            "{}: {label}\n{}: {status_text}   {}: {speed:.1}x   {}: {volume_text}{}\n{}: {timeline} / {duration_text}",
            crate::i18n::tr("文件名"),
            crate::i18n::tr("状态"),
            crate::i18n::tr("速度"),
            crate::i18n::tr("音量"),
            if ui.get_muted() { crate::i18n::tr(" (静音)") } else { String::new() },
            crate::i18n::tr("时间"),
        )
        .into(),
    );

    let decoded = panel
        .shared
        .pixel_format()
        .unwrap_or_else(|| info.pixel_format.to_lowercase());
    let (output_width, output_height) = panel
        .last_frame
        .as_ref()
        .map(|frame| (frame.width as u32, frame.height as u32))
        .unwrap_or((info.width, info.height));

    ui.set_osd_decoder(osd_tokens(decoder_tokens(&info.codec, &decoded)));
    ui.set_osd_input(osd_tokens(vec![
        (
            crate::i18n::tr_template(
                "{format} ({bits} 位), {w}×{h}",
                &[
                    ("format", &info.pixel_format),
                    ("bits", &bit_depth(&info.pixel_format).to_string()),
                    ("w", &info.width.to_string()),
                    ("h", &info.height.to_string()),
                ],
            ),
            TONE_GREEN,
        ),
        (
            format!("({})", aspect_ratio(info.width, info.height)),
            TONE_MAGENTA,
        ),
        (crate::i18n::tr(", 帧率: "), TONE_WHITE),
        (format!("{:.2}", info.nominal_fps), TONE_GREEN),
        (crate::i18n::tr(", 位率: "), TONE_WHITE),
        (
            format!("{:.0}kbps", snapshot.bitrate_mbps * 1000.0),
            TONE_GREEN,
        ),
    ]));
    ui.set_osd_format(osd_tokens(vec![
        (format!("{}p", info.height), TONE_GREEN),
        (crate::i18n::tr(", 像素: "), TONE_WHITE),
        (decoded.clone(), TONE_MAGENTA),
    ]));
    ui.set_osd_output(osd_tokens(vec![
        ("RGBA8888".into(), TONE_MAGENTA),
        (
            crate::i18n::tr_template(
                " (32 位), {w}×{h}",
                &[
                    ("w", &output_width.to_string()),
                    ("h", &output_height.to_string()),
                ],
            ),
            TONE_MAGENTA,
        ),
        (
            format!(" ({})", aspect_ratio(output_width, output_height)),
            TONE_MAGENTA,
        ),
        (crate::i18n::tr(", 帧率: "), TONE_WHITE),
        (
            format!("{:.2} ({:.2})", snapshot.actual_fps, info.nominal_fps),
            TONE_GREEN,
        ),
    ]));
    ui.set_osd_pixel(osd_tokens(vec![
        (decoded, TONE_MAGENTA),
        (" (Input)->RGBA (Display)".into(), TONE_RED),
    ]));
    ui.set_osd_frames(osd_tokens(vec![
        (snapshot.frames.to_string(), TONE_GREEN),
        (crate::i18n::tr(", 丢帧: "), TONE_WHITE),
        (
            snapshot.dropped.to_string(),
            if snapshot.dropped > 0 {
                TONE_RED
            } else {
                TONE_GREEN
            },
        ),
        (
            crate::i18n::tr_template(
                " ({percent}%), 解码: ",
                &[("percent", &format!("{:.1}", snapshot.drop_percent))],
            ),
            TONE_WHITE,
        ),
        (format!("{:.2}ms", snapshot.decode_ms), TONE_GREEN),
        (crate::i18n::tr(", 码率: "), TONE_WHITE),
        (format!("{:.2}Mbps", snapshot.bitrate_mbps), TONE_GREEN),
    ]));
    ui.set_osd_frame_size(osd_tokens(vec![
        (format!("{}×{}", info.width, info.height), TONE_MAGENTA),
        (
            format!("({})", aspect_ratio(info.width, info.height)),
            TONE_MAGENTA,
        ),
        (" - ".into(), TONE_WHITE),
        (format!("{}×{}", output_width, output_height), TONE_MAGENTA),
        (
            format!("({})", aspect_ratio(output_width, output_height)),
            TONE_MAGENTA,
        ),
    ]));
}

fn request_scan(state: &Rc<RefCell<AppState>>) {
    let sender = {
        let mut st = state.borrow_mut();
        if st.scanning {
            return;
        }
        st.scanning = true;
        st.scan_tx.clone()
    };
    thread::spawn(move || {
        let devices = device::scan_devices();
        let resolutions = devices
            .iter()
            .map(|device| {
                let Some(fourcc) = preferred_fourcc(device) else {
                    return Vec::new();
                };
                let mut list = device::resolutions(&device.path, &fourcc);
                list.sort_by_key(|resolution| resolution.width * resolution.height);
                list.dedup();
                list
            })
            .collect();
        let _ = sender.send(ScanResult {
            devices,
            resolutions,
        });
    });
}

fn apply_scan(state: &Rc<RefCell<AppState>>, ui: &MainWindow, scan: ScanResult) {
    log::info!("发现 {} 个 V4L2 采集节点", scan.devices.len());
    let initial = state.borrow().initial_scan;
    {
        let mut st = state.borrow_mut();
        st.scanning = false;
        st.initial_scan = false;
        st.devices = scan.devices;
        st.device_resolutions = scan.resolutions;
        for index in 0..st.panels.len() {
            let source = st.panels[index].source.clone();
            let Some(device_index) = st
                .devices
                .iter()
                .position(|device| device.path.as_path() == Path::new(&source))
            else {
                continue;
            };
            let (title, fourcc, resolutions) = {
                let device = &st.devices[device_index];
                (
                    device.name.clone(),
                    st.format_overrides
                        .get(&source)
                        .cloned()
                        .or_else(|| preferred_fourcc(device)),
                    st.device_resolutions
                        .get(device_index)
                        .cloned()
                        .unwrap_or_default(),
                )
            };
            let panel = &mut st.panels[index];
            panel.title = title;
            if !panel.started_once.load(Ordering::Relaxed) {
                if let Some((width, height)) =
                    closest_resolution(&resolutions, panel.settings.width, panel.settings.height)
                {
                    panel.settings.width = width;
                    panel.settings.height = height;
                    panel.preferred_width = width;
                    panel.preferred_height = height;
                }
                panel.settings.fourcc = fourcc;
            }
        }
        if st.active.is_none() && !st.panels.is_empty() {
            st.active = Some(0);
        }
        st.refresh_menu();
        st.sync_panel_model();
    }
    sync_sources(&state.borrow(), ui);
    if !ui.get_settings_open() {
        refresh_settings_formats(state, ui);
    }
    if initial {
        let (count, active) = {
            let st = state.borrow();
            (st.panels.len(), st.active.unwrap_or(0))
        };
        for index in 0..count {
            let skip_file = {
                let st = state.borrow();
                st.panels
                    .get(index)
                    .map(|panel| is_file_source(&panel.source) && index != active)
                    .unwrap_or(true)
            };
            if skip_file {
                continue;
            }
            start_panel(state, ui, index);
        }
        update_active_ui(ui, &mut state.borrow_mut());
    }
}

fn refresh_settings_formats(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let selected = ui.get_selected_device();
    if selected < 0 {
        ui.set_settings_format_names(ModelRc::new(VecModel::from(Vec::<slint::SharedString>::new())));
        ui.set_settings_format_index(-1);
        state.borrow_mut().settings_format_index = None;
        return;
    }
    let (names, format_index) = {
        let st = state.borrow();
        let Some(device) = st.devices.get(selected as usize) else {
            return;
        };
        let key = device.path.display().to_string();
        let names: Vec<slint::SharedString> = device
            .formats
            .iter()
            .map(|format| format!("{} - {}", format.fourcc, format.description).into())
            .collect();
        let chosen = st
            .format_overrides
            .get(&key)
            .cloned()
            .or_else(|| preferred_fourcc(device));
        let format_index = chosen.and_then(|fourcc| {
            device
                .formats
                .iter()
                .position(|format| format.fourcc == fourcc)
        });
        (names, format_index)
    };
    ui.set_settings_format_names(ModelRc::new(VecModel::from(names)));
    ui.set_settings_format_index(format_index.map(|index| index as i32).unwrap_or(-1));
    state.borrow_mut().settings_format_index = format_index;
}

fn sync_settings_selection(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let selected = {
        let st = state.borrow();
        let active = st
            .active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| {
                st.devices
                    .iter()
                    .position(|device| device.path.as_path() == Path::new(&panel.source))
            })
            .map(|index| index as i32)
            .unwrap_or(-1);
        if active < 0 && !st.devices.is_empty() {
            0
        } else {
            active
        }
    };
    ui.set_selected_device(selected);
    refresh_settings_formats(state, ui);
}

fn sync_sources(state: &AppState, ui: &MainWindow) {
    let split = state
        .panels
        .iter()
        .any(|panel| !is_file_source(&panel.source));
    if ui.get_split_view() != split {
        ui.set_split_view(split);
    }
    let capture2_running = state
        .panels
        .iter()
        .any(|panel| panel.source.starts_with("screen2:") && panel.running);
    if ui.get_capture2_running() != capture2_running {
        ui.set_capture2_running(capture2_running);
    }
    let devices: Vec<DeviceEntry> = state
        .devices
        .iter()
        .map(|device| DeviceEntry {
            title: device.name.as_str().into(),
            subtitle: format!(
                "{}  {}  {}",
                if device.bus.to_ascii_lowercase().contains("usb") {
                    "USB"
                } else {
                    "本地"
                },
                device.path.display(),
                device
                    .formats
                    .iter()
                    .map(|format| format.fourcc.as_str())
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(" / ")
            )
            .as_str()
            .into(),
        })
        .collect();
    ui.set_devices(ModelRc::new(VecModel::from(devices)));
    let names: Vec<slint::SharedString> = state
        .devices
        .iter()
        .map(|device| device.name.as_str().into())
        .collect();
    ui.set_device_names(ModelRc::new(VecModel::from(names)));

    let resolutions: Vec<ResolutionEntry> = state
        .resolutions
        .iter()
        .map(|resolution| ResolutionEntry {
            title: format!("{} x {}", resolution.width, resolution.height)
                .as_str()
                .into(),
        })
        .collect();
    ui.set_resolutions(ModelRc::new(VecModel::from(resolutions)));
    let selected_device = state
        .active
        .and_then(|index| state.panels.get(index))
        .and_then(|panel| {
            state
                .devices
                .iter()
                .position(|device| device.path.as_path() == Path::new(&panel.source))
        })
        .map(|index| index as i32)
        .unwrap_or(-1);
    if !ui.get_settings_open() {
        ui.set_selected_device(selected_device);
    }
    ui.set_selected_resolution(state
            .selected_resolution
            .map(|index| index as i32)
            .unwrap_or(-1),
    );
    let formats: Vec<ResolutionEntry> = state
        .formats
        .iter()
        .map(|format| ResolutionEntry {
            title: format!("{} - {}", format.fourcc, format.description).into(),
        })
        .collect();
    ui.set_formats(ModelRc::new(VecModel::from(formats)));
    ui.set_selected_format(
        state
            .selected_format
            .map(|index| index as i32)
            .unwrap_or(-1),
    );
    if let Some(panel) = state.active.and_then(|index| state.panels.get(index)) {
        ui.set_bar_source(panel.source.as_str().into());
        ui.set_bar_format(source_format(&panel.source).into());
    } else {
        ui.set_bar_source("".into());
        ui.set_bar_format("".into());
    }
}

fn next_active_after_remove(active: Option<usize>, removed: usize, len: usize) -> Option<usize> {
    match (active, len) {
        (_, 0) => None,
        (Some(current), _) if current == removed => Some(removed.min(len - 1)),
        (Some(current), _) if current > removed => Some(current - 1),
        (active, _) => active,
    }
}

fn panel_target(sources: &[String], active: Option<usize>, source: &str) -> PanelTarget {
    if let Some(index) = sources.iter().position(|candidate| candidate == source) {
        return PanelTarget::Existing(index);
    }
    if sources.len() < MAX_PANELS {
        return PanelTarget::New;
    }
    PanelTarget::Replace(active.unwrap_or(0).min(sources.len().saturating_sub(1)))
}

fn config_sources(config: &Config) -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    let candidates = if config.sources.is_empty() {
        vec![config.source.as_str()]
    } else {
        config.sources.iter().map(String::as_str).collect()
    };
    for candidate in candidates {
        let source = candidate.trim();
        if source.is_empty() || sources.iter().any(|existing| existing == source) {
            continue;
        }
        sources.push(source.to_string());
        if sources.len() == MAX_PANELS {
            break;
        }
    }
    sources
}

fn capture_settings_for(config: &Config, source: &str) -> CaptureSettings {
    if flint_media::capture_stream::is_screen(source) {
        let (width, height) = if source.starts_with("screen2:") {
            (config.screen2_width, config.screen2_height)
        } else {
            (config.screen1_width, config.screen1_height)
        };
        CaptureSettings {
            width,
            height,
            fps: 30,
            fourcc: None,
            input_format: None,
            pixel_format: None,
            smoothing: CaptureSmoothing::from_key(&config.capture_smoothing),
        }
    } else if source.starts_with("/dev/") {
        CaptureSettings {
            width: config.width,
            height: config.height,
            fps: config.fps.max(1),
            fourcc: None,
            input_format: None,
            pixel_format: None,
            smoothing: CaptureSmoothing::from_key(&config.capture_smoothing),
        }
    } else {
        CaptureSettings {
            width: 0,
            height: 0,
            fps: 0,
            fourcc: None,
            input_format: None,
            pixel_format: None,
            smoothing: CaptureSmoothing::from_key(&config.capture_smoothing),
        }
    }
}

fn settings_for_source(state: &AppState, source: &str) -> CaptureSettings {
    let mut settings = capture_settings_for(&state.config, source);
    settings.input_format = state.start.input_format.clone();
    settings.pixel_format = state.start.pixel_format.clone();
    if !source.starts_with("/dev/") {
        return settings;
    }
    let device_index = state
        .devices
        .iter()
        .position(|device| device.path.as_path() == Path::new(source));
    if let Some(index) = device_index {
        settings.fourcc = state
            .start
            .fourcc
            .clone()
            .or_else(|| state.format_overrides.get(source).cloned())
            .or_else(|| preferred_fourcc(&state.devices[index]));
        let resolutions = state
            .device_resolutions
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some((width, height)) =
            closest_resolution(resolutions, settings.width, settings.height)
        {
            settings.width = width;
            settings.height = height;
        }
    }
    settings
}

fn refine_settings(state: &AppState, panel: &PanelState) -> CaptureSettings {
    if flint_media::capture_stream::is_screen(&panel.source) {
        return settings_for_source(state, &panel.source);
    }
    let mut settings = panel.settings.clone();
    if !panel.source.starts_with("/dev/") {
        return settings;
    }
    let Some(device_index) = state
        .devices
        .iter()
        .position(|device| device.path.as_path() == Path::new(&panel.source))
    else {
        return settings;
    };
    if settings.fourcc.is_none() {
        settings.fourcc = state
            .format_overrides
            .get(&panel.source)
            .cloned()
            .or_else(|| preferred_fourcc(&state.devices[device_index]));
    }
    if !panel.started_once.load(Ordering::Relaxed) {
        let resolutions = state
            .device_resolutions
            .get(device_index)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some((width, height)) =
            closest_resolution(resolutions, settings.width, settings.height)
        {
            settings.width = width;
            settings.height = height;
        }
    }
    settings
}

fn source_label(devices: &[DeviceInfo], source: &str) -> String {
    if flint_media::capture_stream::is_screen(source) {
        let slot = if source.starts_with("screen2:") { 2 } else { 1 };
        return format!("X11 屏幕采集 {slot}#");
    }
    devices
        .iter()
        .find(|device| device.path.to_string_lossy() == source)
        .map(|device| device.name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| panel_title(source))
}

fn panel_title(source: &str) -> String {
    if source.contains("://") {
        return source.to_string();
    }
    Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| source.to_string())
}

fn closest_resolution(resolutions: &[Resolution], width: u32, height: u32) -> Option<(u32, u32)> {
    resolutions
        .iter()
        .min_by_key(|resolution| {
            resolution.width.abs_diff(width) as u64 * 2 + resolution.height.abs_diff(height) as u64
        })
        .map(|resolution| (resolution.width, resolution.height))
}

fn closest_resolution_index(resolutions: &[Resolution], width: u32, height: u32) -> Option<usize> {
    resolutions
        .iter()
        .enumerate()
        .min_by_key(|(_, resolution)| {
            resolution.width.abs_diff(width) as u64 * 2 + resolution.height.abs_diff(height) as u64
        })
        .map(|(index, _)| index)
}

fn preferred_fourcc(device: &DeviceInfo) -> Option<String> {
    const ORDER: [&str; 10] = [
        "MJPG", "JPEG", "H264", "HEVC", "YUYV", "NV12", "UYVY", "RGB3", "BGR3", "GREY",
    ];
    ORDER.iter().find_map(|wanted| {
        device
            .formats
            .iter()
            .find(|format| format.fourcc == *wanted)
            .map(|format| format.fourcc.clone())
    })
}

fn aspect_ratio(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return "0:0".into();
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
    a.max(1)
}

fn format_seconds(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00:00".into();
    }
    format_clock(Duration::from_secs_f64(seconds))
}

fn format_clock(duration: Duration) -> String {
    let total = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        total / 3_600,
        (total / 60) % 60,
        total % 60
    )
}

fn bit_depth(fourcc: &str) -> u32 {
    match fourcc {
        "NV12" => 12,
        "YUYV" | "YUY2" | "UYVY" => 16,
        "GREY" => 8,
        _ => 24,
    }
}

fn decoder_tokens(codec: &str, decoded: &str) -> Vec<(String, i32)> {
    let name = match codec {
        "MJPEG" => Some("mjpeg"),
        "H264" => Some("h264"),
        "HEVC" | "H265" => Some("hevc"),
        "MPEG4" => Some("mpeg4"),
        _ => None,
    };
    match name {
        Some(name) => vec![
            (crate::i18n::tr("内置 FFmpeg 解码器"), TONE_RED),
            (
                crate::i18n::tr_template(" ({name}, 1 线程)", &[("name", name)]),
                TONE_MAGENTA,
            ),
        ],
        None => vec![
            (crate::i18n::tr("内置 FFmpeg 像素转换"), TONE_RED),
            (format!(" ({decoded})"), TONE_MAGENTA),
        ],
    }
}

fn osd_tokens(parts: Vec<(String, i32)>) -> ModelRc<OsdToken> {
    let tokens: Vec<OsdToken> = parts
        .into_iter()
        .map(|(text, tone)| OsdToken {
            text: text.into(),
            tone,
        })
        .collect();
    ModelRc::new(VecModel::from(tokens))
}

fn empty_osd() -> ModelRc<OsdToken> {
    ModelRc::new(VecModel::from(Vec::<OsdToken>::new()))
}

fn clear_osd_details(ui: &MainWindow) {
    ui.set_osd_decoder(empty_osd());
    ui.set_osd_input(empty_osd());
    ui.set_osd_format(empty_osd());
    ui.set_osd_output(empty_osd());
    ui.set_osd_pixel(empty_osd());
    ui.set_osd_frames(empty_osd());
    ui.set_osd_frame_size(empty_osd());
}

fn clear_osd(ui: &MainWindow) {
    clear_osd_details(ui);
    ui.set_osd_file(empty_osd());
    ui.set_osd_time(empty_osd());
    ui.set_osd_config(empty_osd());
    ui.set_osd_version(empty_osd());
    ui.set_osd_audio(empty_osd());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timeline() {
        assert_eq!(format_clock(Duration::from_millis(83_456)), "00:01:23");
        assert_eq!(format_clock(Duration::ZERO), "00:00:00");
        assert_eq!(format_clock(Duration::from_secs(3_661)), "01:01:01");
    }

    #[test]
    fn computes_aspect_ratio() {
        assert_eq!(aspect_ratio(1920, 1080), "16:9");
        assert_eq!(aspect_ratio(1280, 720), "16:9");
        assert_eq!(aspect_ratio(0, 0), "0:0");
    }

    #[test]
    fn caps_volume_by_control_mode() {
        let mut config = Config::default();
        assert_eq!(volume_ceiling(&config), 1.0);
        config.audio_multi_channel_volume = true;
        config.audio_max_volume = 135;
        assert_eq!(volume_ceiling(&config), 1.35);
        config.audio_max_volume = 50;
        assert_eq!(volume_ceiling(&config), 1.0);
        config.audio_max_volume = 900;
        assert_eq!(volume_ceiling(&config), 5.0);
    }

    #[test]
    fn picks_closest_resolution() {
        let resolutions = vec![
            Resolution {
                width: 640,
                height: 480,
            },
            Resolution {
                width: 1280,
                height: 720,
            },
            Resolution {
                width: 1920,
                height: 1080,
            },
        ];
        assert_eq!(
            closest_resolution(&resolutions, 1200, 700),
            Some((1280, 720))
        );
        assert_eq!(closest_resolution_index(&resolutions, 1920, 1080), Some(2));
        assert_eq!(closest_resolution(&[], 1280, 720), None);
    }

    #[test]
    fn allocates_panel_slots() {
        let sources = vec!["/dev/video0".to_string()];
        assert_eq!(
            panel_target(&sources, Some(0), "/dev/video0"),
            PanelTarget::Existing(0)
        );
        assert_eq!(
            panel_target(&sources, Some(0), "/dev/video2"),
            PanelTarget::New
        );
        let full: Vec<String> = (0..MAX_PANELS)
            .map(|index| format!("/dev/video{index}"))
            .collect();
        assert_eq!(
            panel_target(&full, Some(2), "/dev/video999"),
            PanelTarget::Replace(2)
        );
    }

    #[test]
    fn keeps_active_panel_valid_after_close() {
        assert_eq!(next_active_after_remove(None, 0, 0), None);
        assert_eq!(next_active_after_remove(Some(0), 0, 1), Some(0));
        assert_eq!(next_active_after_remove(Some(2), 2, 2), Some(1));
        assert_eq!(next_active_after_remove(Some(3), 1, 3), Some(2));
        assert_eq!(next_active_after_remove(Some(1), 2, 3), Some(1));
    }

    #[test]
    fn dedupes_and_caps_config_sources() {
        let mut sources = vec![
            "/dev/video0".to_string(),
            " /dev/video0 ".to_string(),
            String::new(),
        ];
        sources.extend((1..=MAX_PANELS + 2).map(|index| format!("/dev/video{index}")));
        let mut config = Config {
            source: "/dev/video0".into(),
            sources,
            ..Config::default()
        };
        let expected: Vec<String> = (0..MAX_PANELS)
            .map(|index| format!("/dev/video{index}"))
            .collect();
        assert_eq!(config_sources(&config), expected);
        config.sources.clear();
        assert_eq!(config_sources(&config), vec!["/dev/video0"]);
    }

    #[test]
    fn prefers_compressed_formats() {
        let device = DeviceInfo {
            path: PathBuf::from("/dev/video0"),
            name: "Camera".into(),
            driver: "uvcvideo".into(),
            bus: "usb-0000:00:14.0-1".into(),
            formats: vec![
                flint_core::device::FormatInfo {
                    fourcc: "YUYV".into(),
                    description: String::new(),
                    compressed: false,
                },
                flint_core::device::FormatInfo {
                    fourcc: "MJPG".into(),
                    description: String::new(),
                    compressed: true,
                },
            ],
        };
        assert_eq!(preferred_fourcc(&device), Some("MJPG".into()));
    }

    #[test]
    fn normalizes_similar_stems() {
        assert_eq!(similar_stem_key("Show - 01"), similar_stem_key("Show - 02"));
        assert_eq!(similar_stem_key("Show - 01"), "show - #");
        assert_ne!(
            similar_stem_key("Show - 01"),
            similar_stem_key("Other - 01")
        );
        assert_eq!(similar_stem_key("EP12[1080p]"), "ep#[#p]");
        assert_eq!(similar_stem_key("2024"), "#");
    }
}
