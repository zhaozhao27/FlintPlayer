use std::collections::HashMap;

use flint_core::device::{self, CaptureMode, DeviceInfo};
use flint_media::CaptureSettings;
use gpui::*;

pub struct CaptureRequest {
    pub source: String,
    pub name: String,
    pub settings: CaptureSettings,
    pub split: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropdownKind {
    Device,
    Format,
    Screen,
    Output,
    Smoothing,
    Audio,
}

pub const SMOOTHING_OPTIONS: [(&str, &str); 6] = [
    ("none", "不使用平滑滤镜"),
    ("system", "使用系统默认平滑滤镜(慢)"),
    ("low", "使用 FlintPlayer 低品质平滑滤镜"),
    ("normal", "使用 FlintPlayer 普通品质平滑滤镜"),
    ("medium", "使用 FlintPlayer 适中品质平滑滤镜"),
    ("high", "使用 FlintPlayer 高品质平滑滤镜"),
];

pub const OUTPUT_OPTIONS: [(&str, &str); 2] = [
    ("default", "默认输出"),
    ("rgba", "软件 RGBA 输出"),
];

#[derive(Clone)]
pub struct AudioSource {
    pub name: String,
    pub label: String,
}

fn audio_sources() -> Vec<AudioSource> {
    let Ok(output) = std::process::Command::new("pactl")
        .args(["list", "sources"])
        .env("LC_ALL", "C")
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut sources: Vec<AudioSource> = Vec::new();
    let mut name: Option<String> = None;
    let mut label: Option<String> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.starts_with("Source #") {
            if let Some(name) = name.take() {
                let label = label.take().unwrap_or_else(|| name.clone());
                sources.push(AudioSource { name, label });
            }
        } else if let Some(value) = line.strip_prefix("\tName: ") {
            name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("\tDescription: ") {
            label = Some(value.trim().to_string());
        }
    }
    if let Some(name) = name {
        let label = label.unwrap_or_else(|| name.clone());
        sources.push(AudioSource { name, label });
    }
    sources
}

#[derive(Clone)]
pub struct ScreenSource {
    pub display: String,
    pub window: Option<u32>,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

fn screen_sources() -> Vec<ScreenSource> {
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let mut sources = vec![ScreenSource {
        display: display.clone(),
        window: None,
        title: String::new(),
        width: 0,
        height: 0,
    }];
    for window in crate::x11::visible_windows() {
        sources.push(ScreenSource {
            display: display.clone(),
            window: Some(window.id),
            title: window.title,
            width: window.width,
            height: window.height,
        });
    }
    sources
}

#[derive(Clone, Default)]
struct SavedSelection {
    fourcc: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

pub struct SettingsController {
    open: bool,
    tab: usize,
    dropdown: Option<DropdownKind>,
    devices: Vec<DeviceInfo>,
    device_index: Option<usize>,
    modes: Vec<CaptureMode>,
    mode_index: Option<usize>,
    saved: HashMap<String, SavedSelection>,
    pending_selections: Option<Vec<(String, String, u32, u32)>>,
    request: Option<CaptureRequest>,
    capture_size: (u32, u32, u32),
    screen_sources: Vec<ScreenSource>,
    screen_index: usize,
    output_index: usize,
    smoothing_key: String,
    audio_sources: Vec<AudioSource>,
    audio_selection: String,
    time_positioning: bool,
    audio_monitor: bool,
    manual_device: Option<String>,
    split_enabled: bool,
    active_source: Option<String>,
}

impl SettingsController {
    pub fn new(selections: Vec<(String, String, u32, u32)>) -> Self {
        let saved = selections
            .iter()
            .map(|(device, fourcc, width, height)| {
                (
                    device.clone(),
                    SavedSelection {
                        fourcc: (!fourcc.is_empty()).then(|| fourcc.clone()),
                        width: (*width > 0).then_some(*width),
                        height: (*height > 0).then_some(*height),
                    },
                )
            })
            .collect();
        Self {
            open: false,
            tab: 0,
            dropdown: None,
            devices: Vec::new(),
            device_index: None,
            modes: Vec::new(),
            mode_index: None,
            saved,
            pending_selections: None,
            request: None,
            capture_size: (1280, 720, 30),
            screen_sources: Vec::new(),
            screen_index: 0,
            output_index: 0,
            smoothing_key: "system".to_string(),
            audio_sources: Vec::new(),
            audio_selection: String::new(),
            time_positioning: false,
            audio_monitor: true,
            manual_device: None,
            split_enabled: false,
            active_source: None,
        }
    }

    pub fn capture_size(&self) -> (u32, u32, u32) {
        self.capture_size
    }

    pub fn set_capture_size(&mut self, width: u32, height: u32, fps: u32) {
        self.capture_size = (width, height, fps);
    }

    pub fn take_selection_update(&mut self) -> Option<Vec<(String, String, u32, u32)>> {
        self.pending_selections.take()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn tab(&self) -> usize {
        self.tab
    }

    pub fn set_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.tab = tab.min(4);
        self.dropdown = None;
        cx.notify();
    }

    pub fn dropdown(&self) -> Option<DropdownKind> {
        self.dropdown
    }

    pub fn toggle_dropdown(&mut self, kind: DropdownKind, cx: &mut Context<Self>) {
        self.dropdown = (self.dropdown != Some(kind)).then_some(kind);
        cx.notify();
    }

    pub fn close_dropdown(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self.dropdown.take().is_some();
        if closed {
            cx.notify();
        }
        closed
    }

    pub fn screen_sources(&self) -> &[ScreenSource] {
        &self.screen_sources
    }

    pub fn screen_label(&self) -> String {
        let Some(source) = self.screen_sources.get(self.screen_index) else {
            return String::new();
        };
        match source.window {
            Some(_) => format!("窗口：{} ({}×{})", source.title, source.width, source.height),
            None => "X11 屏幕采集 (XShm)".to_string(),
        }
    }

    pub fn screen_source(&self, slot: u32) -> Option<String> {
        let source = self.screen_sources.get(self.screen_index)?;
        Some(match source.window {
            Some(window) => format!("screen{slot}:window:0x{window:x}"),
            None => format!("screen{slot}:{}", source.display),
        })
    }

    pub fn select_screen(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.screen_sources.len() {
            return;
        }
        self.screen_index = index;
        self.apply_screen_size();
        self.dropdown = None;
        cx.notify();
    }

    fn apply_screen_size(&mut self) {
        let Some(source) = self.screen_sources.get(self.screen_index) else {
            return;
        };
        if source.window.is_some() && source.width > 0 && source.height > 0 {
            self.capture_size.0 = source.width;
            self.capture_size.1 = source.height;
        }
    }

    pub fn output_label(&self) -> String {
        crate::i18n::tr(OUTPUT_OPTIONS[self.output_index.min(OUTPUT_OPTIONS.len() - 1)].1)
    }

    pub fn output_names(&self) -> Vec<String> {
        OUTPUT_OPTIONS
            .iter()
            .map(|(_, label)| crate::i18n::tr(label))
            .collect()
    }

    pub fn select_output(&mut self, index: usize, cx: &mut Context<Self>) {
        self.output_index = index.min(OUTPUT_OPTIONS.len() - 1);
        self.dropdown = None;
        cx.notify();
    }

    pub fn audio_label(&self) -> String {
        let selection = self.audio_selection.trim();
        if selection.is_empty() {
            return crate::i18n::tr("默认音频");
        }
        self.audio_sources
            .iter()
            .find(|source| source.name == selection)
            .map(|source| source.label.clone())
            .unwrap_or_else(|| selection.to_string())
    }

    pub fn audio_names(&self) -> Vec<String> {
        let mut names = vec![crate::i18n::tr("默认音频")];
        names.extend(self.audio_sources.iter().map(|source| source.label.clone()));
        names
    }

    pub fn audio_index(&self) -> usize {
        let selection = self.audio_selection.trim();
        if selection.is_empty() {
            return 0;
        }
        self.audio_sources
            .iter()
            .position(|source| source.name == selection)
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    pub fn audio_source(&self) -> Option<String> {
        if self.audio_sources.is_empty() {
            return None;
        }
        let selection = self.audio_selection.trim();
        if self
            .audio_sources
            .iter()
            .any(|source| source.name == selection)
        {
            Some(selection.to_string())
        } else {
            Some("default".to_string())
        }
    }

    pub fn set_audio_source(&mut self, source: String) {
        self.audio_selection = source;
    }

    pub fn select_audio(&mut self, index: usize, cx: &mut Context<Self>) -> Option<String> {
        let selection = if index == 0 {
            String::new()
        } else {
            self.audio_sources.get(index - 1)?.name.clone()
        };
        self.audio_selection = selection.clone();
        self.dropdown = None;
        cx.notify();
        Some(selection)
    }

    pub fn smoothing_key(&self) -> &str {
        &self.smoothing_key
    }

    pub fn set_smoothing_key(&mut self, key: String) {
        self.smoothing_key = key;
    }

    pub fn smoothing_names(&self) -> Vec<String> {
        SMOOTHING_OPTIONS
            .iter()
            .map(|(_, label)| crate::i18n::tr(label))
            .collect()
    }

    pub fn smoothing_index(&self) -> usize {
        SMOOTHING_OPTIONS
            .iter()
            .position(|(key, _)| *key == self.smoothing_key)
            .unwrap_or(1)
    }

    pub fn select_smoothing(&mut self, index: usize, cx: &mut Context<Self>) -> Option<String> {
        let key = SMOOTHING_OPTIONS.get(index)?.0.to_string();
        self.smoothing_key = key.clone();
        self.dropdown = None;
        cx.notify();
        Some(key)
    }

    pub fn time_positioning(&self) -> bool {
        self.time_positioning
    }

    pub fn toggle_time_positioning(&mut self, cx: &mut Context<Self>) {
        self.time_positioning = !self.time_positioning;
        cx.notify();
    }

    pub fn audio_monitor(&self) -> bool {
        self.audio_monitor
    }

    pub fn toggle_audio_monitor(&mut self, cx: &mut Context<Self>) {
        self.audio_monitor = !self.audio_monitor;
        cx.notify();
    }

    pub fn manual_device(&self) -> Option<&str> {
        self.manual_device.as_deref()
    }

    pub fn select_manual_device(&mut self, path: String, cx: &mut Context<Self>) {
        self.manual_device = Some(path);
        self.device_index = None;
        self.modes.clear();
        self.mode_index = None;
        self.dropdown = None;
        cx.notify();
    }

    pub fn set_active_source(&mut self, source: Option<String>) {
        self.active_source = source;
    }

    pub fn split_enabled(&self) -> bool {
        self.split_enabled
    }

    pub fn split_available(&self) -> bool {
        let Some(active) = self.active_source.as_deref() else {
            return false;
        };
        self.selected_source().is_some_and(|source| source != active)
    }

    pub fn toggle_split(&mut self, cx: &mut Context<Self>) {
        if !self.split_available() {
            return;
        }
        self.split_enabled = !self.split_enabled;
        cx.notify();
    }

    fn split_requested(&self) -> bool {
        self.split_enabled && self.split_available()
    }

    fn selected_source(&self) -> Option<String> {
        self.manual_device.clone().or_else(|| self.device_key())
    }

    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    pub fn device_index(&self) -> Option<usize> {
        self.device_index
    }

    pub fn modes(&self) -> &[CaptureMode] {
        &self.modes
    }

    pub fn mode_index(&self) -> Option<usize> {
        self.mode_index
    }

    pub fn open(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.tab = tab.min(4);
        self.screen_sources = screen_sources();
        self.screen_index = self.screen_index.min(self.screen_sources.len().saturating_sub(1));
        self.apply_screen_size();
        self.devices = device::scan_devices();
        self.audio_sources = audio_sources();
        self.open = true;
        self.dropdown = None;
        if self.device_index.map_or(true, |index| index >= self.devices.len()) {
            self.device_index = (!self.devices.is_empty()).then_some(0);
        }
        self.refresh_modes();
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    pub fn select_device(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.devices.len() {
            return;
        }
        self.device_index = Some(index);
        self.manual_device = None;
        self.dropdown = None;
        self.refresh_modes();
        cx.notify();
    }

    pub fn select_mode(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(mode) = self.modes.get(index).cloned() else {
            return;
        };
        let Some(key) = self.device_key() else {
            return;
        };
        let entry = self.saved.entry(key).or_default();
        entry.fourcc = Some(mode.fourcc);
        entry.width = Some(mode.width);
        entry.height = Some(mode.height);
        self.mode_index = Some(index);
        self.dropdown = None;
        self.sync_selections();
        cx.notify();
    }

    pub fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.manual_device.clone() {
            self.request = Some(CaptureRequest {
                source: path.clone(),
                name: path,
                settings: CaptureSettings {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    fourcc: None,
                    input_format: None,
                    pixel_format: None,
                    smoothing: Default::default(),
                    yuv_output: self.output_index == 0,
                },
                split: self.split_requested(),
            });
            self.open = false;
            cx.notify();
            return;
        }
        let Some(device) = self.device_index.and_then(|index| self.devices.get(index)) else {
            return;
        };
        let source = device.path.display().to_string();
        let name = device.name.clone();
        let mode = self.mode_index.and_then(|index| self.modes.get(index));
        let fourcc = mode.map(|mode| mode.fourcc.clone());
        let width = mode.map_or(1280, |mode| mode.width);
        let height = mode.map_or(720, |mode| mode.height);
        let fps = mode.map_or(30, |mode| mode.fps.round().max(1.0) as u32);
        self.request = Some(CaptureRequest {
            source,
            name,
            settings: CaptureSettings {
                width,
                height,
                fps,
                fourcc,
                input_format: None,
                pixel_format: None,
                smoothing: Default::default(),
                yuv_output: self.output_index == 0,
            },
            split: self.split_requested(),
        });
        self.open = false;
        cx.notify();
    }

    pub fn take_request(&mut self) -> Option<CaptureRequest> {
        self.request.take()
    }

    fn device_key(&self) -> Option<String> {
        self.device_index
            .and_then(|index| self.devices.get(index))
            .map(|device| device.path.display().to_string())
    }

    fn sync_selections(&mut self) {
        let mut selections: Vec<(String, String, u32, u32)> = self
            .saved
            .iter()
            .map(|(device, selection)| {
                (
                    device.clone(),
                    selection.fourcc.clone().unwrap_or_default(),
                    selection.width.unwrap_or(0),
                    selection.height.unwrap_or(0),
                )
            })
            .collect();
        selections.sort_by(|left, right| left.0.cmp(&right.0));
        self.pending_selections = Some(selections);
    }

    fn refresh_modes(&mut self) {
        self.modes.clear();
        self.mode_index = None;

        let Some(device) = self.device_index.and_then(|index| self.devices.get(index)) else {
            return;
        };
        let key = device.path.display().to_string();
        for format in &device.formats {
            self.modes
                .extend(device::modes(&device.path, &format.fourcc));
        }
        let saved = self.saved.get(&key).cloned().unwrap_or_default();
        self.mode_index = saved
            .fourcc
            .as_deref()
            .and_then(|fourcc| {
                self.modes.iter().position(|mode| {
                    mode.fourcc == fourcc
                        && saved.width.map_or(true, |width| mode.width == width)
                        && saved.height.map_or(true, |height| mode.height == height)
                })
            })
            .or_else(|| preferred_mode(&self.modes));
    }
}

fn preferred_mode(modes: &[CaptureMode]) -> Option<usize> {
    const ORDER: [&str; 10] = [
        "MJPG", "JPEG", "H264", "HEVC", "YUYV", "NV12", "UYVY", "RGB3", "BGR3", "GREY",
    ];
    modes
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            let (left_format, left_size, left_fps) = mode_rank(left, &ORDER);
            let (right_format, right_size, right_fps) = mode_rank(right, &ORDER);
            left_format
                .cmp(&right_format)
                .then(left_size.cmp(&right_size))
                .then(left_fps.total_cmp(&right_fps))
        })
        .map(|(index, _)| index)
}

fn mode_rank(mode: &CaptureMode, order: &[&str]) -> (u32, u32, f64) {
    let format = order
        .iter()
        .position(|fourcc| *fourcc == mode.fourcc)
        .unwrap_or(order.len()) as u32;
    let size = if mode.width == 1280 && mode.height == 720 {
        0
    } else if mode.width <= 1920 && mode.height <= 1080 {
        1_000_000_000 - mode.width * mode.height
    } else {
        2_000_000_000
    };
    (format, size, (mode.fps - 30.0).abs())
}
