use std::collections::HashMap;

use flint_core::device::{self, CaptureMode, DeviceInfo};
use flint_media::CaptureSettings;
use gpui::*;

pub struct CaptureRequest {
    pub source: String,
    pub name: String,
    pub settings: CaptureSettings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropdownKind {
    Device,
    Format,
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
        self.devices = device::scan_devices();
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
            },
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
