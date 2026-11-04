use std::collections::HashMap;

use flint_core::device::{self, DeviceInfo, FormatInfo, Resolution};
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
    formats: Vec<FormatInfo>,
    format_index: Option<usize>,
    resolutions: Vec<Resolution>,
    resolution_index: Option<usize>,
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
            formats: Vec::new(),
            format_index: None,
            resolutions: Vec::new(),
            resolution_index: None,
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

    pub fn formats(&self) -> &[FormatInfo] {
        &self.formats
    }

    pub fn format_index(&self) -> Option<usize> {
        self.format_index
    }

    pub fn resolutions(&self) -> &[Resolution] {
        &self.resolutions
    }

    pub fn resolution_index(&self) -> Option<usize> {
        self.resolution_index
    }

    pub fn open(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.tab = tab.min(4);
        self.devices = device::scan_devices();
        self.open = true;
        self.dropdown = None;
        if self.device_index.map_or(true, |index| index >= self.devices.len()) {
            self.device_index = (!self.devices.is_empty()).then_some(0);
        }
        self.refresh_formats();
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
        self.refresh_formats();
        cx.notify();
    }

    pub fn select_format(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(format) = self.formats.get(index) else {
            return;
        };
        let key = self.device_key();
        let fourcc = format.fourcc.clone();
        let Some(key) = key else {
            return;
        };
        let entry = self.saved.entry(key).or_default();
        entry.fourcc = Some(fourcc);
        let (width, height) = (entry.width, entry.height);
        self.format_index = Some(index);
        self.dropdown = None;
        self.sync_selections();
        self.refresh_resolutions(width, height);
        cx.notify();
    }

    pub fn select_resolution(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(resolution) = self.resolutions.get(index) else {
            return;
        };
        let width = resolution.width;
        let height = resolution.height;
        let Some(key) = self.device_key() else {
            return;
        };
        let entry = self.saved.entry(key).or_default();
        entry.width = Some(width);
        entry.height = Some(height);
        self.resolution_index = Some(index);
        self.sync_selections();
        cx.notify();
    }

    pub fn open_selected(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.device_index.and_then(|index| self.devices.get(index)) else {
            return;
        };
        let source = device.path.display().to_string();
        let name = device.name.clone();
        let fourcc = self
            .format_index
            .and_then(|index| self.formats.get(index))
            .map(|format| format.fourcc.clone());
        let (width, height) = self
            .resolution_index
            .and_then(|index| self.resolutions.get(index))
            .map(|resolution| (resolution.width, resolution.height))
            .unwrap_or((1280, 720));
        self.request = Some(CaptureRequest {
            source,
            name,
            settings: CaptureSettings {
                width,
                height,
                fps: 30,
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

    fn refresh_formats(&mut self) {
        self.formats.clear();
        self.format_index = None;
        self.resolutions.clear();
        self.resolution_index = None;

        let (key, formats) = match self.device_index.and_then(|index| self.devices.get(index)) {
            Some(device) => (
                device.path.display().to_string(),
                device.formats.clone(),
            ),
            None => return,
        };
        let saved = self.saved.get(&key).cloned().unwrap_or_default();
        self.formats = formats;
        self.format_index = saved
            .fourcc
            .as_deref()
            .and_then(|fourcc| {
                self.formats
                    .iter()
                    .position(|format| format.fourcc == fourcc)
            })
            .or_else(|| preferred_format(&self.formats));
        self.refresh_resolutions(saved.width, saved.height);
    }

    fn refresh_resolutions(&mut self, width: Option<u32>, height: Option<u32>) {
        self.resolutions.clear();
        self.resolution_index = None;

        let Some(device) = self.device_index.and_then(|index| self.devices.get(index)) else {
            return;
        };
        let Some(fourcc) = self
            .format_index
            .and_then(|index| self.formats.get(index))
            .map(|format| format.fourcc.clone())
        else {
            return;
        };
        self.resolutions = device::resolutions(&device.path, &fourcc);
        self.resolutions.reverse();
        self.resolution_index = match (width, height) {
            (Some(width), Some(height)) => self
                .resolutions
                .iter()
                .position(|resolution| resolution.width == width && resolution.height == height),
            _ => None,
        }
        .or_else(|| preferred_resolution(&self.resolutions));
    }
}

fn preferred_format(formats: &[FormatInfo]) -> Option<usize> {
    const ORDER: [&str; 10] = [
        "MJPG", "JPEG", "H264", "HEVC", "YUYV", "NV12", "UYVY", "RGB3", "BGR3", "GREY",
    ];
    ORDER
        .iter()
        .find_map(|wanted| formats.iter().position(|format| format.fourcc == *wanted))
        .or(if formats.is_empty() { None } else { Some(0) })
}

fn preferred_resolution(resolutions: &[Resolution]) -> Option<usize> {
    if let Some(index) = resolutions
        .iter()
        .position(|resolution| resolution.width == 1280 && resolution.height == 720)
    {
        return Some(index);
    }
    resolutions
        .iter()
        .enumerate()
        .filter(|(_, resolution)| resolution.width <= 1920 && resolution.height <= 1080)
        .max_by_key(|(_, resolution)| resolution.width * resolution.height)
        .map(|(index, _)| index)
        .or_else(|| resolutions.len().checked_sub(1))
}
