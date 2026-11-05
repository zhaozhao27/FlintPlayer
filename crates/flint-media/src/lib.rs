pub mod audio;
pub mod audio_dsp;
pub mod audio_record;
pub mod capture_stream;
pub mod capture_v4l2;
pub mod decoder;
pub mod effects;
pub mod embedded_subtitles;
pub mod latest_slot;
pub mod panorama;
pub mod recorder;
pub mod snapshot;
pub mod stereo;
pub mod subtitles;

use crossbeam_channel::bounded;
use flint_core::command::CaptureCommand;
use flint_core::{Stats, StatsSnapshot};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use audio_record::AudioRecordSummary;
pub use latest_slot::Frame;
pub use recorder::RecordSummary;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

pub const EOF_ERROR: &str = "输入流已结束";

pub const AUDIO_METHOD_AUTO: u8 = 0;
pub const AUDIO_METHOD_PULSE: u8 = 1;
pub const AUDIO_METHOD_NULL: u8 = 2;

pub fn audio_method_code(name: &str) -> u8 {
    match name {
        "pulse" => AUDIO_METHOD_PULSE,
        "null" => AUDIO_METHOD_NULL,
        _ => AUDIO_METHOD_AUTO,
    }
}

pub fn audio_method_name(code: u8) -> &'static str {
    match code {
        AUDIO_METHOD_PULSE => "pulse",
        AUDIO_METHOD_NULL => "null",
        _ => "auto",
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MediaError(String);

impl From<String> for MediaError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureSmoothing {
    None,
    #[default]
    System,
    Low,
    Normal,
    Medium,
    High,
}

impl CaptureSmoothing {
    pub fn from_key(key: &str) -> Self {
        match key.trim() {
            "none" => Self::None,
            "low" => Self::Low,
            "normal" => Self::Normal,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => Self::System,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::System => "system",
            Self::Low => "low",
            Self::Normal => "normal",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CaptureSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fourcc: Option<String>,
    pub input_format: Option<String>,
    pub pixel_format: Option<String>,
    pub smoothing: CaptureSmoothing,
    pub yuv_output: bool,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            fourcc: None,
            input_format: None,
            pixel_format: None,
            smoothing: CaptureSmoothing::default(),
            yuv_output: true,
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
    pub duration: f64,
    pub audio_tracks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChapterInfo {
    pub title: String,
    pub start: f64,
    pub end: f64,
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
    recorder: Mutex<Option<recorder::Recorder>>,
    audio_recorder: Mutex<Option<audio_record::WavRecorder>>,
    paused: AtomicBool,
    pending_steps: AtomicI32,
    last_pts: Mutex<f64>,
    seek_target: Mutex<Option<f64>>,
    video_renderer: AtomicU8,
    yuv_output: AtomicBool,
    speed: AtomicU32,
    volume: AtomicU32,
    muted: AtomicBool,
    audio_channel: AtomicU32,
    audio_swap: AtomicBool,
    audio_sync_ms: AtomicI32,
    audio_track: AtomicUsize,
    audio_output_rate: AtomicU32,
    audio_output_format: AtomicU8,
    audio_device: Mutex<Option<String>>,
    audio_device2: Mutex<Option<String>>,
    audio_passthrough_device: Mutex<Option<String>>,
    audio_output_method: AtomicU8,
    audio_effects: AtomicU32,
    audio_ac3_dts: AtomicBool,
    audio_ac3_dts_gain: AtomicI32,
    audio_eq: [AtomicI32; audio_dsp::EQ_BANDS.len()],
    external_audio_request: Mutex<Option<String>>,
    external_audio_label: Mutex<Option<String>>,
    external_audio_notice: Mutex<Option<String>>,
    chapters: Mutex<Arc<Vec<ChapterInfo>>>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            slot: latest_slot::LatestSlot::new(),
            stats: Stats::new(),
            stop: AtomicBool::new(false),
            status: Mutex::new(EngineStatus::default()),
            pixel_format: Mutex::new(None),
            recorder: Mutex::new(None),
            audio_recorder: Mutex::new(None),
            paused: AtomicBool::new(false),
            pending_steps: AtomicI32::new(0),
            last_pts: Mutex::new(0.0),
            seek_target: Mutex::new(None),
            video_renderer: AtomicU8::new(0),
            yuv_output: AtomicBool::new(false),
            speed: AtomicU32::new(1.0f32.to_bits()),
            volume: AtomicU32::new(1.0f32.to_bits()),
            muted: AtomicBool::new(false),
            audio_channel: AtomicU32::new(0),
            audio_swap: AtomicBool::new(false),
            audio_sync_ms: AtomicI32::new(0),
            audio_track: AtomicUsize::new(usize::MAX),
            audio_output_rate: AtomicU32::new(0),
            audio_output_format: AtomicU8::new(0),
            audio_device: Mutex::new(None),
            audio_device2: Mutex::new(None),
            audio_passthrough_device: Mutex::new(None),
            audio_output_method: AtomicU8::new(AUDIO_METHOD_AUTO),
            audio_effects: AtomicU32::new(0),
            audio_ac3_dts: AtomicBool::new(false),
            audio_ac3_dts_gain: AtomicI32::new(600),
            audio_eq: std::array::from_fn(|_| AtomicI32::new(0)),
            external_audio_request: Mutex::new(None),
            external_audio_label: Mutex::new(None),
            external_audio_notice: Mutex::new(None),
            chapters: Mutex::new(Arc::new(Vec::new())),
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
        self.paused.store(false, Ordering::Release);
        self.pending_steps.store(0, Ordering::Release);
        *self.last_pts.lock() = 0.0;
        *self.seek_target.lock() = None;
        self.speed.store(1.0f32.to_bits(), Ordering::Release);
        self.audio_sync_ms.store(0, Ordering::Release);
        self.audio_track.store(usize::MAX, Ordering::Release);
        *self.external_audio_request.lock() = None;
        *self.external_audio_label.lock() = None;
        *self.external_audio_notice.lock() = None;
        *self.chapters.lock() = Arc::new(Vec::new());
        let _ = self.stop_recording();
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Acquire))
    }

    pub fn set_speed(&self, speed: f32) {
        self.speed
            .store(speed.clamp(0.1, 4.0).to_bits(), Ordering::Release);
    }

    pub fn set_video_renderer(&self, name: &str) {
        self.video_renderer.store(
            match name {
                "null-compressed" => 1,
                "null-uncompressed" => 2,
                _ => 0,
            },
            Ordering::Release,
        );
    }

    pub fn video_renderer_null(&self) -> bool {
        self.video_renderer.load(Ordering::Acquire) != 0
    }

    pub fn video_renderer_compressed(&self) -> bool {
        self.video_renderer.load(Ordering::Acquire) == 1
    }

    pub fn set_yuv_output(&self, enabled: bool) {
        self.yuv_output.store(enabled, Ordering::Release);
    }

    pub fn yuv_output(&self) -> bool {
        self.yuv_output.load(Ordering::Acquire)
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Acquire))
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 5.0).to_bits(), Ordering::Release);
    }

    pub fn muted(&self) -> bool {
        self.muted.load(Ordering::Acquire)
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Release);
    }

    pub fn audio_gain(&self) -> f32 {
        if self.muted() {
            0.0
        } else {
            self.volume()
        }
    }

    pub fn audio_options(&self) -> audio::AudioOptions {
        audio::AudioOptions {
            gain: self.audio_gain(),
            channel: audio::Channel::from_code(self.audio_channel.load(Ordering::Acquire)),
            swap: self.audio_swap.load(Ordering::Acquire),
            sync_ms: self.audio_sync_ms.load(Ordering::Acquire),
        }
    }

    pub fn set_audio_channel(&self, channel: audio::Channel) {
        self.audio_channel.store(channel.code(), Ordering::Release);
    }

    pub fn set_audio_swap(&self, swap: bool) {
        self.audio_swap.store(swap, Ordering::Release);
    }

    pub fn audio_swap(&self) -> bool {
        self.audio_swap.load(Ordering::Acquire)
    }

    pub fn audio_effects(&self) -> u32 {
        self.audio_effects.load(Ordering::Acquire)
    }

    pub fn set_audio_effects(&self, flags: u32) {
        self.audio_effects
            .store(flags & audio_dsp::EFFECT_ALL, Ordering::Release);
    }

    pub fn audio_ac3_dts(&self) -> bool {
        self.audio_ac3_dts.load(Ordering::Acquire)
    }

    pub fn audio_ac3_dts_gain_db(&self) -> f32 {
        self.audio_ac3_dts_gain.load(Ordering::Acquire) as f32 / 100.0
    }

    pub fn set_audio_ac3_dts(&self, enabled: bool, gain_db: f32) {
        self.audio_ac3_dts.store(enabled, Ordering::Release);
        self.audio_ac3_dts_gain.store(
            (gain_db.clamp(0.0, 24.0) * 100.0).round() as i32,
            Ordering::Release,
        );
    }

    pub fn audio_eq_gains(&self) -> [f32; audio_dsp::EQ_BANDS.len()] {
        std::array::from_fn(|index| self.audio_eq[index].load(Ordering::Acquire) as f32 / 100.0)
    }

    pub fn set_audio_eq_gains(&self, gains: &[f32; audio_dsp::EQ_BANDS.len()]) {
        for (slot, gain) in self.audio_eq.iter().zip(gains) {
            slot.store(
                (gain.clamp(-12.0, 12.0) * 100.0).round() as i32,
                Ordering::Release,
            );
        }
    }

    pub fn audio_track(&self) -> Option<usize> {
        let value = self.audio_track.load(Ordering::Acquire);
        (value != usize::MAX).then_some(value)
    }

    pub fn set_audio_track(&self, track: Option<usize>) {
        self.audio_track
            .store(track.unwrap_or(usize::MAX), Ordering::Release);
    }

    pub fn request_external_audio(&self, path: &str) {
        *self.external_audio_request.lock() = Some(path.to_string());
    }

    pub fn clear_external_audio(&self) {
        *self.external_audio_request.lock() = Some(String::new());
    }

    pub fn take_external_audio_request(&self) -> Option<String> {
        self.external_audio_request.lock().take()
    }

    pub fn external_audio_label(&self) -> Option<String> {
        self.external_audio_label.lock().clone()
    }

    pub fn set_external_audio_label(&self, label: Option<String>) {
        *self.external_audio_label.lock() = label;
    }

    pub fn push_external_audio_notice(&self, notice: String) {
        *self.external_audio_notice.lock() = Some(notice);
    }

    pub fn take_external_audio_notice(&self) -> Option<String> {
        self.external_audio_notice.lock().take()
    }

    pub fn set_audio_sync_ms(&self, sync_ms: i32) {
        self.audio_sync_ms
            .store(sync_ms.clamp(-5000, 5000), Ordering::Release);
    }

    pub fn audio_output_rate(&self) -> u32 {
        match self.audio_output_rate.load(Ordering::Acquire) {
            0 => 48_000,
            rate => rate,
        }
    }

    pub fn set_audio_output_rate(&self, rate: u32) {
        self.audio_output_rate.store(rate, Ordering::Release);
    }

    pub fn audio_output_format(&self) -> audio::SampleFormat {
        audio::SampleFormat::from_code(u32::from(self.audio_output_format.load(Ordering::Acquire)))
    }

    pub fn set_audio_output_format(&self, format: audio::SampleFormat) {
        self.audio_output_format
            .store(format.code() as u8, Ordering::Release);
    }

    pub fn audio_device(&self) -> Option<String> {
        self.audio_device.lock().clone()
    }

    pub fn set_audio_device(&self, device: Option<String>) {
        let mut current = self.audio_device.lock();
        *current = device.filter(|name| !name.is_empty());
    }

    pub fn audio_device2(&self) -> Option<String> {
        self.audio_device2.lock().clone()
    }

    pub fn set_audio_device2(&self, device: Option<String>) {
        let mut current = self.audio_device2.lock();
        *current = device.filter(|name| !name.is_empty());
    }

    pub fn audio_passthrough_device(&self) -> Option<String> {
        self.audio_passthrough_device.lock().clone()
    }

    pub fn set_audio_passthrough_device(&self, device: Option<String>) {
        let mut current = self.audio_passthrough_device.lock();
        *current = device.filter(|name| !name.is_empty());
    }

    pub fn audio_output_method(&self) -> u8 {
        self.audio_output_method.load(Ordering::Acquire)
    }

    pub fn set_audio_output_method(&self, method: u8) {
        self.audio_output_method.store(method, Ordering::Release);
    }

    pub fn audio_sync_ms(&self) -> i32 {
        self.audio_sync_ms.load(Ordering::Acquire)
    }

    pub fn chapters(&self) -> Arc<Vec<ChapterInfo>> {
        self.chapters.lock().clone()
    }

    pub fn set_chapters(&self, chapters: Vec<ChapterInfo>) {
        *self.chapters.lock() = Arc::new(chapters);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
        if !paused {
            self.pending_steps.store(0, Ordering::Release);
        }
    }

    pub fn request_step(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        self.pending_steps.fetch_add(delta, Ordering::AcqRel);
    }

    pub fn take_steps(&self) -> i32 {
        self.pending_steps.swap(0, Ordering::AcqRel)
    }

    pub fn last_pts(&self) -> f64 {
        *self.last_pts.lock()
    }

    pub fn note_last_pts(&self, pts: f64) {
        *self.last_pts.lock() = pts;
    }

    pub fn request_seek(&self, seconds: f64) {
        *self.seek_target.lock() = Some(seconds.max(0.0));
    }

    pub fn take_seek(&self) -> Option<f64> {
        self.seek_target.lock().take()
    }

    pub fn start_recording(
        &self,
        path: &Path,
        width: usize,
        height: usize,
        fps: f64,
    ) -> Result<(), String> {
        let mut slot = self.recorder.lock();
        if slot.is_some() {
            return Err("当前面板已经在录制".to_string());
        }
        *slot = Some(recorder::Recorder::start(path, width, height, fps)?);
        Ok(())
    }

    pub fn start_recording_nv12(
        &self,
        path: &Path,
        width: usize,
        height: usize,
        fps: f64,
    ) -> Result<(), String> {
        let mut slot = self.recorder.lock();
        if slot.is_some() {
            return Err("当前面板已经在录制".to_string());
        }
        *slot = Some(recorder::Recorder::start_nv12(path, width, height, fps)?);
        Ok(())
    }

    pub fn stop_recording(&self) -> Option<Result<RecordSummary, String>> {
        self.recorder.lock().take().map(|recorder| recorder.stop())
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.lock().is_some()
    }

    pub fn start_audio_recording(&self, path: &Path, rate: u32) -> Result<(), String> {
        let mut slot = self.audio_recorder.lock();
        if slot.is_some() {
            return Err("当前面板已经在录制声音".to_string());
        }
        *slot = Some(audio_record::WavRecorder::start(path, rate, 2)?);
        Ok(())
    }

    pub fn stop_audio_recording(&self) -> Option<Result<AudioRecordSummary, String>> {
        self.audio_recorder
            .lock()
            .take()
            .map(|recorder| recorder.stop())
    }

    pub fn is_audio_recording(&self) -> bool {
        self.audio_recorder.lock().is_some()
    }

    pub fn write_audio_samples(&self, samples: &[i16]) {
        if let Some(recorder) = self.audio_recorder.lock().as_mut() {
            recorder.write(samples);
        }
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
        if let Some(recorder) = self.recorder.lock().as_ref() {
            if !frame.rgba.is_empty() {
                recorder.push(&frame.rgba);
            } else if let Some(planes) = frame.nv12.as_ref() {
                recorder.push_nv12(&planes.y, planes.y_stride, &planes.uv, planes.uv_stride);
            }
        }
        *self.last_pts.lock() = frame.pts;
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

    pub fn start(
        &mut self,
        source: &str,
        settings: CaptureSettings,
    ) -> Result<SourceInfo, MediaError> {
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
        self.shared.set_paused(false);
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("采集线程异常退出");
            }
        }
        self.shared.stop_recording();
        if let Some(Ok(summary)) = self.shared.stop_audio_recording() {
            log::info!(
                "声音录制已保存：{}（{:.1} 秒）",
                summary.path.display(),
                summary.seconds
            );
        }
        let mut status = self.shared.status.lock();
        status.running = false;
        status.started_at = None;
    }

    pub fn start_recording(&self, path: &Path) -> Result<(), MediaError> {
        let info = self
            .shared
            .status()
            .info
            .ok_or_else(|| MediaError::from("当前没有正在运行的画面".to_string()))?;
        let width = info.width as usize;
        let height = info.height as usize;
        let fps = info.nominal_fps;
        if self.shared.yuv_output() {
            self.shared.start_recording_nv12(path, width, height, fps)
        } else {
            self.shared.start_recording(path, width, height, fps)
        }
        .map_err(MediaError::from)
    }

    pub fn stop_recording(&self) -> Option<Result<RecordSummary, String>> {
        self.shared.stop_recording()
    }

    pub fn is_recording(&self) -> bool {
        self.shared.is_recording()
    }

    pub fn start_audio_recording(&self, path: &Path, rate: u32) -> Result<(), MediaError> {
        self.shared
            .start_audio_recording(path, rate)
            .map_err(MediaError::from)
    }

    pub fn stop_audio_recording(&self) -> Option<Result<AudioRecordSummary, String>> {
        self.shared.stop_audio_recording()
    }

    pub fn is_audio_recording(&self) -> bool {
        self.shared.is_audio_recording()
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.set_paused(paused);
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused()
    }

    pub fn step_frames(&self, delta: i32) {
        self.shared.request_step(delta);
    }

    pub fn seek_to(&self, seconds: f64) {
        self.shared.request_seek(seconds);
    }

    pub fn set_speed(&self, speed: f32) {
        self.shared.set_speed(speed);
    }

    pub fn set_video_renderer(&self, name: &str) {
        self.shared.set_video_renderer(name);
    }

    pub fn speed(&self) -> f32 {
        self.shared.speed()
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.set_muted(muted);
    }

    pub fn set_audio_channel(&self, channel: audio::Channel) {
        self.shared.set_audio_channel(channel);
    }

    pub fn set_audio_swap(&self, swap: bool) {
        self.shared.set_audio_swap(swap);
    }

    pub fn set_audio_sync_ms(&self, sync_ms: i32) {
        self.shared.set_audio_sync_ms(sync_ms);
    }

    pub fn request_external_audio(&self, path: &str) {
        self.shared.request_external_audio(path);
    }

    pub fn clear_external_audio(&self) {
        self.shared.clear_external_audio();
    }

    pub fn external_audio_label(&self) -> Option<String> {
        self.shared.external_audio_label()
    }

    pub fn set_audio_effects(&self, flags: u32) {
        self.shared.set_audio_effects(flags);
    }

    pub fn set_audio_ac3_dts(&self, enabled: bool, gain_db: f32) {
        self.shared.set_audio_ac3_dts(enabled, gain_db);
    }

    pub fn set_audio_eq_gains(&self, gains: &[f32; audio_dsp::EQ_BANDS.len()]) {
        self.shared.set_audio_eq_gains(gains);
    }

    pub fn set_audio_output_rate(&self, rate: u32) {
        self.shared.set_audio_output_rate(rate);
    }

    pub fn set_audio_output_format(&self, format: audio::SampleFormat) {
        self.shared.set_audio_output_format(format);
    }

    pub fn set_audio_device(&self, device: Option<String>) {
        self.shared.set_audio_device(device);
    }

    pub fn set_audio_device2(&self, device: Option<String>) {
        self.shared.set_audio_device2(device);
    }

    pub fn set_audio_passthrough_device(&self, device: Option<String>) {
        self.shared.set_audio_passthrough_device(device);
    }

    pub fn set_audio_output_method(&self, method: u8) {
        self.shared.set_audio_output_method(method);
    }

    pub fn position_seconds(&self) -> f64 {
        self.shared.last_pts()
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
