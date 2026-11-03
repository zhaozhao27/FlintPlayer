pub mod capture_stream;
pub mod capture_v4l2;
pub mod decoder;
pub mod latest_slot;

use crossbeam_channel::bounded;
use flint_core::command::CaptureCommand;
use flint_core::{Stats, StatsSnapshot};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use latest_slot::Frame;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MediaError(String);

impl From<String> for MediaError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

#[derive(Clone, Debug)]
pub struct CaptureSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fourcc: Option<String>,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            fourcc: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub source: String,
    pub transport: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub nominal_fps: f64,
}

#[derive(Clone, Debug, Default)]
pub struct EngineStatus {
    pub info: Option<SourceInfo>,
    pub error: Option<String>,
    pub running: bool,
    pub started_at: Option<Instant>,
}

pub struct Shared {
    pub slot: latest_slot::LatestSlot,
    pub stats: Stats,
    stop: AtomicBool,
    status: Mutex<EngineStatus>,
    pixel_format: Mutex<Option<String>>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            slot: latest_slot::LatestSlot::new(),
            stats: Stats::new(),
            stop: AtomicBool::new(false),
            status: Mutex::new(EngineStatus::default()),
            pixel_format: Mutex::new(None),
        }
    }

    pub fn note_pixel_format(&self, name: &str) {
        let mut current = self.pixel_format.lock();
        if current.as_deref() != Some(name) {
            *current = Some(name.to_string());
        }
    }

    pub fn pixel_format(&self) -> Option<String> {
        self.pixel_format.lock().clone()
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn prepare(&self) {
        self.stop.store(false, Ordering::Relaxed);
        self.slot.reset();
        self.stats.reset();
        *self.status.lock() = EngineStatus::default();
        *self.pixel_format.lock() = None;
    }

    pub fn begin(&self, info: SourceInfo) -> bool {
        let mut status = self.status.lock();
        if status.error.is_some() {
            return false;
        }
        status.info = Some(info);
        status.running = true;
        status.started_at = Some(Instant::now());
        true
    }

    pub fn finish(&self, error: Option<String>) {
        let mut status = self.status.lock();
        status.running = false;
        status.started_at = None;
        status.error = error;
    }

    pub fn clear_error(&self) {
        self.status.lock().error = None;
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().clone()
    }

    pub fn publish_frame(&self, frame: Frame) -> Option<Vec<u8>> {
        let before = self.slot.dropped();
        let recycled = self.slot.publish(frame);
        if self.slot.dropped() > before {
            self.stats.record_drop();
        }
        recycled
    }

    pub fn stats_snapshot(&self) -> StatsSnapshot {
        self.stats.snapshot(Instant::now())
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CaptureEngine {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    settings: CaptureSettings,
    source: Option<String>,
}

impl CaptureEngine {
    pub fn new() -> Self {
        // UVC 摄像头常在 MJPEG 帧内写入非标准 APPn 段，libavcodec 会按错误级别打印，但帧仍能正常解码。
        ffmpeg_next::log::set_level(ffmpeg_next::log::Level::Fatal);
        Self {
            shared: Arc::new(Shared::new()),
            worker: None,
            settings: CaptureSettings::default(),
            source: None,
        }
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub fn settings(&self) -> &CaptureSettings {
        &self.settings
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn is_running(&self) -> bool {
        self.worker.is_some() && self.shared.status().running
    }

    pub fn start(&mut self, source: &str, settings: CaptureSettings) -> Result<SourceInfo, MediaError> {
        self.stop();
        self.shared.prepare();
        let (sender, receiver) = bounded(1);
        let shared = self.shared.clone();
        let handle = decoder::spawn(shared, source.to_string(), settings.clone(), sender);
        self.worker = Some(handle);
        self.settings = settings;
        self.source = Some(source.to_string());

        match receiver.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(info)) => {
                if !self.shared.begin(info.clone()) {
                    self.stop();
                    let error = self
                        .shared
                        .status()
                        .error
                        .unwrap_or_else(|| "启动采集失败".to_string());
                    return Err(error.into());
                }
                log::info!("采集已启动：{} {}x{}", info.codec, info.width, info.height);
                Ok(info)
            }
            Ok(Err(error)) => {
                self.shared.finish(Some(error.clone()));
                self.stop();
                Err(error.into())
            }
            Err(_) => {
                let error = "启动采集超时".to_string();
                self.shared.finish(Some(error.clone()));
                self.stop();
                Err(error.into())
            }
        }
    }

    pub fn stop(&mut self) {
        self.shared.request_stop();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("采集线程异常退出");
            }
        }
        let mut status = self.shared.status.lock();
        status.running = false;
        status.started_at = None;
    }

    pub fn clear_error(&self) {
        self.shared.clear_error();
    }

    pub fn take_latest(&self) -> Option<Arc<Frame>> {
        self.shared.slot.take()
    }

    pub fn recycle(&self, frame: Arc<Frame>) {
        self.shared.slot.recycle(frame);
    }

    pub fn execute(&mut self, command: CaptureCommand) -> Result<(), MediaError> {
        match command {
            CaptureCommand::StartCapture(source) => {
                let settings = self.settings.clone();
                self.start(&source, settings).map(|_| ())
            }
            CaptureCommand::Stop => {
                self.stop();
                Ok(())
            }
            CaptureCommand::SetResolution(width, height) => {
                let source = self
                    .source
                    .clone()
                    .ok_or_else(|| MediaError::from("尚未选择采集设备".to_string()))?;
                let mut settings = self.settings.clone();
                settings.width = width;
                settings.height = height;
                self.start(&source, settings).map(|_| ())
            }
            CaptureCommand::Snapshot => Ok(()),
        }
    }
}

impl Default for CaptureEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CaptureEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
