use crate::keymap::{action_for, KeyAction};
use crate::{DeviceEntry, MainWindow, OsdToken, PanelInfo, ResolutionEntry};
use crossbeam_channel::{Receiver, Sender};
use flint_core::config::Config;
use flint_core::device::{self, DeviceInfo, FormatInfo, Resolution};
use flint_core::sysinfo;
use flint_media::{CaptureEngine, CaptureSettings, Frame, Shared};
use parking_lot::Mutex;
use slint::{
    ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, Timer, TimerMode, VecModel, Weak,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHROME_HIDE_DELAY: Duration = Duration::from_secs(2);
const STATS_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const NOTICE_DURATION: Duration = Duration::from_secs(4);
const MAX_PANELS: usize = 4;
const TONE_GREEN: i32 = 0;
const TONE_RED: i32 = 1;
const TONE_MAGENTA: i32 = 2;
const TONE_CYAN: i32 = 3;
const TONE_WHITE: i32 = 5;

type SharedEngine = Arc<Mutex<CaptureEngine>>;

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
    stats: PanelStats,
    last_error: Option<String>,
    running: bool,
    last_stats: Instant,
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
            stats: PanelStats::default(),
            last_error: None,
            running: false,
            last_stats: Instant::now(),
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
    panel_model: Rc<VecModel<PanelInfo>>,
    notice: Option<Notice>,
    osd_visible: bool,
    fullscreen: bool,
    presenter: Option<Rc<Timer>>,
    scan_tx: Sender<ScanResult>,
    scan_rx: Receiver<ScanResult>,
    scanning: bool,
    initial_scan: bool,
    cpu: CpuMeter,
    osd_at: Instant,
    osd_active: Option<usize>,
    notice_tx: Sender<String>,
    notice_rx: Receiver<String>,
}

impl AppState {
    fn new(config: Config, panel_model: Rc<VecModel<PanelInfo>>) -> Self {
        let (scan_tx, scan_rx) = crossbeam_channel::unbounded();
        let (notice_tx, notice_rx) = crossbeam_channel::unbounded();
        let panels: Vec<PanelState> = config_sources(&config)
            .into_iter()
            .map(|source| {
                let settings = capture_settings_for(&config, &source);
                PanelState::new(source.clone(), panel_title(&source), settings)
            })
            .collect();
        let active = (!panels.is_empty()).then_some(0);
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
            panel_model,
            notice: None,
            osd_visible: false,
            fullscreen: false,
            presenter: None,
            scan_tx,
            scan_rx,
            scanning: false,
            initial_scan: true,
            cpu: CpuMeter::new(),
            osd_at: Instant::now(),
            osd_active: None,
            notice_tx,
            notice_rx,
        };
        state.osd_visible = state.config.osd_visible;
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
            .or_else(|| {
                device_index.and_then(|index| preferred_fourcc(&self.devices[index]))
            });
        self.selected_format =
            fourcc.and_then(|fourcc| self.formats.iter().position(|format| format.fourcc == fourcc));
    }
}

pub fn run(initial_source: Option<String>) -> Result<(), slint::PlatformError> {
    let mut config = Config::load();
    if let Some(source) = initial_source {
        config.source = source.clone();
        config.sources = vec![source];
    }
    let ui = MainWindow::new()?;
    let panel_model = Rc::new(VecModel::<PanelInfo>::default());
    ui.set_panels(ModelRc::new(panel_model.clone()));
    let state = Rc::new(RefCell::new(AppState::new(config, panel_model)));

    ui.set_osd_visible(state.borrow().osd_visible);
    ui.set_osd_renderer(osd_tokens(vec![(
        "Slint Hardware Pipeline (OpenGL/Vulkan)".into(),
        TONE_RED,
    )]));
    ui.set_pinned(state.borrow().config.always_on_top);
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

    let hide_timer = Rc::new(Timer::default());
    wire_actions(&ui, &state, &hide_timer);
    wire_timers(&ui, &state, &hide_timer);

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
                    let visible = !state.borrow().osd_visible;
                    set_osd(&state, &ui, visible);
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
                KeyAction::ToggleFullscreen => {
                    let fullscreen = !state.borrow().fullscreen;
                    set_fullscreen(&state, &ui, fullscreen);
                    true
                }
                KeyAction::ExitFullscreen => {
                    if ui.get_menu_open() {
                        ui.set_menu_open(false);
                        arm_chrome_hide(&timer, &ui.as_weak());
                        true
                    } else if state.borrow().fullscreen {
                        set_fullscreen(&state, &ui, false);
                        true
                    } else {
                        false
                    }
                }
                KeyAction::ToggleDeviceMenu => {
                    toggle_device_menu(&ui, &timer);
                    true
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
            if let Some(index) = state.borrow().active {
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
            let visible = !state.borrow().osd_visible;
            set_osd(&state, &ui, visible);
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
            set_fullscreen(&state, &ui, fullscreen);
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
        ui.on_panel_toggle_fullscreen(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            activate_panel(&state, &ui, index.max(0) as usize);
            let fullscreen = !state.borrow().fullscreen;
            set_fullscreen(&state, &ui, fullscreen);
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
            toggle_always_on_top(&state, &ui);
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
            match action.as_str() {
                "open-file" | "open-device" | "open-network" => toggle_device_menu(&ui, &timer),
                "close-panel" => {
                    let index = state.borrow().active;
                    if let Some(index) = index {
                        close_panel(&state, &ui, index);
                    }
                }
                "play-pause" => toggle_freeze(&state, &ui),
                "stop" => {
                    let index = state.borrow().active;
                    if let Some(index) = index {
                        stop_panel(&state, &ui, index);
                    }
                }
                "fullscreen" => {
                    let fullscreen = !state.borrow().fullscreen;
                    set_fullscreen(&state, &ui, fullscreen);
                }
                "always-on-top" => toggle_always_on_top(&state, &ui),
                "freeze" => toggle_freeze(&state, &ui),
                "snapshot" => save_snapshot(&state, &ui),
                "osd" => {
                    let visible = !state.borrow().osd_visible;
                    set_osd(&state, &ui, visible);
                }
                "exit" => quit_app(&state, &ui),
                "about" => show_notice(
                    &state,
                    &ui,
                    format!("FlintPlayer {}", env!("CARGO_PKG_VERSION")),
                ),
                _ => {}
            }
            show_chrome(&ui, &timer);
        });
    }
}

fn quit_app(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    {
        let size = ui.window().size();
        let position = ui.window().position();
        let mut st = state.borrow_mut();
        st.config.window_w = size.width as f32;
        st.config.window_h = size.height as f32;
        st.config.window_x = Some(position.x as f32);
        st.config.window_y = Some(position.y as f32);
        st.config.osd_visible = st.osd_visible;
        st.config.sources = st.panels.iter().map(|panel| panel.source.clone()).collect();
        st.config.source = st
            .active
            .and_then(|index| st.panels.get(index))
            .or_else(|| st.panels.first())
            .map(|panel| panel.source.clone())
            .unwrap_or_default();
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
            for (index, panel) in st.panels.iter_mut().enumerate() {
                let status = panel.shared.status();
                let mut changed = false;
                if status.running && !panel.running {
                    panel.last_serial = 0;
                }
                if !status.running && panel.running {
                    panel.frozen = false;
                }
                if status.running != panel.running {
                    panel.running = status.running;
                    changed = true;
                }
                if status.error != panel.last_error {
                    panel.last_error = status.error.clone();
                    changed = true;
                }
                if !panel.frozen {
                    let visible = !fullscreen || active == Some(index);
                    if let Some(frame) = panel.shared.slot.take() {
                        if !visible {
                            panel.shared.slot.recycle(frame);
                        } else {
                            if frame.serial != panel.last_serial {
                                panel.last_serial = frame.serial;
                                let buffer = SharedPixelBuffer::clone_from_slice(
                                    &frame.rgba,
                                    frame.width as u32,
                                    frame.height as u32,
                                );
                                panel.image = Image::from_rgba8_premultiplied(buffer);
                                changed = true;
                            }
                            if let Some(previous) = panel.last_frame.replace(frame) {
                                panel.shared.slot.recycle(previous);
                            }
                        }
                    }
                }
                if panel.last_stats.elapsed() >= STATS_INTERVAL {
                    panel.last_stats = now;
                    update_panel_stats(panel);
                    changed = true;
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
        });
    }

    arm_chrome_hide(hide_timer, &ui.as_weak());
    state.borrow_mut().presenter = Some(Rc::new(presenter));
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

fn start_panel(state: &Rc<RefCell<AppState>>, ui: &MainWindow, index: usize) {
    let (engine, commands, started_once, generation, source, settings) = {
        let st = state.borrow();
        let Some(panel) = st.panels.get(index) else {
            return;
        };
        let settings = refine_settings(&st, panel);
        let generation = panel.commands.fetch_add(1, Ordering::AcqRel) + 1;
        (
            panel.engine.clone(),
            panel.commands.clone(),
            panel.started_once.clone(),
            generation,
            panel.source.clone(),
            settings,
        )
    };
    let row = {
        let mut st = state.borrow_mut();
        let active = st.active == Some(index);
        let Some(panel) = st.panels.get_mut(index) else {
            return;
        };
        panel.settings = settings.clone();
        panel.frozen = false;
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
    thread::spawn(move || {
        let mut engine = engine.lock();
        if commands.load(Ordering::Acquire) != generation {
            return;
        }
        if let Err(error) = engine.start(&source, settings) {
            log::error!("启动采集失败：{error}");
        } else {
            started_once.store(true, Ordering::Relaxed);
        }
    });
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
        let active = st.active;
        if let Some(panel) = st.panels.get_mut(index) {
            panel.frozen = false;
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
    {
        let mut st = state.borrow_mut();
        if index >= st.panels.len() {
            return;
        }
        if st.active != Some(index) {
            st.active = Some(index);
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
            if !ui.get_menu_open() {
                ui.set_chrome_visible(false);
            }
        }
    });
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

fn set_osd(state: &Rc<RefCell<AppState>>, ui: &MainWindow, visible: bool) {
    state.borrow_mut().osd_visible = visible;
    ui.set_osd_visible(visible);
}

fn toggle_freeze(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let mut st = state.borrow_mut();
    let Some(index) = st.active else {
        return;
    };
    let Some(panel) = st.panels.get_mut(index) else {
        return;
    };
    if !panel.running {
        return;
    }
    panel.frozen = !panel.frozen;
    let frozen = panel.frozen;
    let row = panel.row(true);
    st.panel_model.set_row_data(index, row);
    ui.set_frozen(frozen);
}

fn always_on_top_supported() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none()
}

fn toggle_always_on_top(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let enabled = {
        let mut st = state.borrow_mut();
        st.config.always_on_top = !st.config.always_on_top;
        st.config.always_on_top
    };
    ui.set_pinned(enabled);
    if enabled && !always_on_top_supported() {
        show_notice(
            state,
            ui,
            "当前 Wayland 会话不支持窗口置顶（X11 / XWayland 下可用）".into(),
        );
    }
}

fn set_fullscreen(state: &Rc<RefCell<AppState>>, ui: &MainWindow, fullscreen: bool) {
    if fullscreen && state.borrow().panels.is_empty() {
        return;
    }
    state.borrow_mut().fullscreen = fullscreen;
    ui.set_fullscreen(fullscreen);
    ui.window().set_fullscreen(fullscreen);
    if !fullscreen {
        ui.set_chrome_visible(true);
    }
}

fn save_snapshot(state: &Rc<RefCell<AppState>>, ui: &MainWindow) {
    let snapshot = {
        let st = state.borrow();
        st.active
            .and_then(|index| st.panels.get(index))
            .and_then(|panel| panel.last_frame.clone())
            .map(|frame| (frame, st.config.snapshot_dir.clone()))
    };
    let Some((frame, directory)) = snapshot else {
        show_notice(state, ui, "当前没有可截取的画面".to_string());
        return;
    };
    let notices = state.borrow().notice_tx.clone();
    thread::spawn(move || {
        let result = (|| -> Result<PathBuf, String> {
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("创建截图目录失败：{error}"))?;
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0);
            let path = directory.join(format!("flintplayer_{millis}.png"));
            let buffer = image::RgbaImage::from_raw(
                frame.width as u32,
                frame.height as u32,
                frame.rgba.clone(),
            )
            .ok_or_else(|| "截图缓冲尺寸无效".to_string())?;
            buffer
                .save(&path)
                .map_err(|error| format!("保存截图失败：{error}"))?;
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

fn show_notice(state: &Rc<RefCell<AppState>>, ui: &MainWindow, message: String) {
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
    let active = active_index.and_then(|index| st.panels.get(index));
    ui.set_active_index(active_index.map(|index| index as i32).unwrap_or(0));
    let message = match &st.notice {
        Some(notice) => notice.message.clone(),
        None => active
            .and_then(|panel| panel.last_error.clone())
            .unwrap_or_default(),
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
        }
        return;
    }
    st.osd_active = active_index;
    st.osd_at = Instant::now();
    let cpu = st.cpu.sample();

    let Some(panel) = active else {
        ui.set_running(false);
        ui.set_frozen(false);
        ui.set_bar_elapsed("00:00:00".into());
        ui.set_bar_codec("".into());
        ui.set_bar_size("".into());
        ui.set_bar_rate("".into());
        ui.set_bar_bitrate("".into());
        ui.set_bar_source("".into());
        clear_osd(ui);
        return;
    };
    let label = source_label(&st.devices, &panel.source);
    ui.set_running(panel.running);
    ui.set_frozen(panel.frozen);
    ui.set_bar_source(label.as_str().into());

    let snapshot = panel.shared.stats_snapshot();
    let status = panel.shared.status();
    let timeline = status
        .started_at
        .map(|started| format_clock(started.elapsed()))
        .unwrap_or_else(|| "00:00:00".into());

    ui.set_osd_file(osd_tokens(vec![(label.clone(), TONE_GREEN)]));
    ui.set_osd_time(osd_tokens(vec![
        (sysinfo::local_hms(), TONE_WHITE),
        (", 时间轴: ".into(), TONE_WHITE),
        (timeline.clone(), TONE_GREEN),
        (", #帧数: ".into(), TONE_WHITE),
        (snapshot.frames.to_string(), TONE_GREEN),
    ]));
    ui.set_osd_config(osd_tokens(vec![
        ("*默认配置".into(), TONE_RED),
        (", CPU: ".into(), TONE_WHITE),
        (format!("{cpu:.1}%"), TONE_GREEN),
    ]));
    ui.set_osd_version(osd_tokens(vec![
        (env!("CARGO_PKG_VERSION").into(), TONE_WHITE),
        (", ".into(), TONE_WHITE),
        (sysinfo::os_description(), TONE_CYAN),
    ]));
    ui.set_osd_audio(osd_tokens(vec![("无音频输入".into(), TONE_WHITE)]));
    ui.set_bar_elapsed(timeline.as_str().into());

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
    ui.set_bar_bitrate(format!("{:.2}M", snapshot.bitrate_mbps).as_str().into());

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
            format!(
                "{} ({} 位), {}×{}",
                info.pixel_format,
                bit_depth(&info.pixel_format),
                info.width,
                info.height
            ),
            TONE_GREEN,
        ),
        (
            format!("({})", aspect_ratio(info.width, info.height)),
            TONE_MAGENTA,
        ),
        (", 帧率: ".into(), TONE_WHITE),
        (format!("{:.2}", info.nominal_fps), TONE_GREEN),
        (", 位率: ".into(), TONE_WHITE),
        (
            format!("{:.0}kbps", snapshot.bitrate_mbps * 1000.0),
            TONE_GREEN,
        ),
    ]));
    ui.set_osd_format(osd_tokens(vec![
        (format!("{}p", info.height), TONE_GREEN),
        (", 像素: ".into(), TONE_WHITE),
        (decoded.clone(), TONE_MAGENTA),
    ]));
    ui.set_osd_output(osd_tokens(vec![
        ("RGBA8888".into(), TONE_MAGENTA),
        (
            format!(" (32 位), {}×{}", output_width, output_height),
            TONE_GREEN,
        ),
        (
            format!(" ({})", aspect_ratio(output_width, output_height)),
            TONE_MAGENTA,
        ),
        (", 帧率: ".into(), TONE_WHITE),
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
        (", 丢帧: ".into(), TONE_WHITE),
        (
            snapshot.dropped.to_string(),
            if snapshot.dropped > 0 {
                TONE_RED
            } else {
                TONE_GREEN
            },
        ),
        (format!(" ({:.1}%), 解码: ", snapshot.drop_percent), TONE_WHITE),
        (format!("{:.2}ms", snapshot.decode_ms), TONE_GREEN),
        (", 码率: ".into(), TONE_WHITE),
        (format!("{:.2}Mbps", snapshot.bitrate_mbps), TONE_GREEN),
    ]));
    ui.set_osd_frame_size(osd_tokens(vec![
        (format!("{}×{}", info.width, info.height), TONE_MAGENTA),
        (
            format!("({})", aspect_ratio(info.width, info.height)),
            TONE_GREEN,
        ),
        (" - ".into(), TONE_WHITE),
        (
            format!("{}×{}", output_width, output_height),
            TONE_MAGENTA,
        ),
        (
            format!("({})", aspect_ratio(output_width, output_height)),
            TONE_GREEN,
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
                    preferred_fourcc(device),
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
    if initial {
        let count = state.borrow().panels.len();
        for index in 0..count {
            start_panel(state, ui, index);
        }
        update_active_ui(ui, &mut state.borrow_mut());
    }
}

fn sync_sources(state: &AppState, ui: &MainWindow) {
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
    ui.set_selected_device(selected_device);
    ui.set_selected_resolution(
        state
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
    } else {
        ui.set_bar_source("".into());
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
    if source.starts_with("/dev/") {
        CaptureSettings {
            width: config.width,
            height: config.height,
            fps: config.fps.max(1),
            fourcc: None,
        }
    } else {
        CaptureSettings {
            width: 0,
            height: 0,
            fps: 0,
            fourcc: None,
        }
    }
}

fn settings_for_source(state: &AppState, source: &str) -> CaptureSettings {
    let mut settings = capture_settings_for(&state.config, source);
    if !source.starts_with("/dev/") {
        return settings;
    }
    let device_index = state
        .devices
        .iter()
        .position(|device| device.path.as_path() == Path::new(source));
    if let Some(index) = device_index {
        settings.fourcc = preferred_fourcc(&state.devices[index]);
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
        settings.fourcc = preferred_fourcc(&state.devices[device_index]);
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

fn closest_resolution(
    resolutions: &[Resolution],
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    resolutions
        .iter()
        .min_by_key(|resolution| {
            resolution.width.abs_diff(width) as u64 * 2 + resolution.height.abs_diff(height) as u64
        })
        .map(|resolution| (resolution.width, resolution.height))
}

fn closest_resolution_index(
    resolutions: &[Resolution],
    width: u32,
    height: u32,
) -> Option<usize> {
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
            ("内置 FFmpeg 解码器".into(), TONE_RED),
            (format!(" ({name}, 1 线程)"), TONE_MAGENTA),
        ],
        None => vec![
            ("内置 FFmpeg 像素转换".into(), TONE_RED),
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
        let full: Vec<String> = (0..4).map(|index| format!("/dev/video{index}")).collect();
        assert_eq!(
            panel_target(&full, Some(2), "/dev/video9"),
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
        let mut config = Config {
            source: "/dev/video0".into(),
            sources: vec![
                "/dev/video0".into(),
                " /dev/video0 ".into(),
                String::new(),
                "/dev/video2".into(),
                "/dev/video4".into(),
                "/dev/video6".into(),
                "/dev/video8".into(),
            ],
            ..Config::default()
        };
        assert_eq!(
            config_sources(&config),
            vec!["/dev/video0", "/dev/video2", "/dev/video4", "/dev/video6"]
        );
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
}
