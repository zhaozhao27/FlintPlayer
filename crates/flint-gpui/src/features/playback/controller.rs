use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use flint_engine::{ChapterInfo, EngineError, MpvPlayer, PlayerState, TrackInfo};
use flint_media::panorama;
use flint_media::recorder::{RecordSummary, Recorder};
use flint_media::stereo::{self, StereoMode};
use flint_media::Frame;
use gpui::*;

#[derive(Clone, Debug, Default)]
pub struct RawInput {
    pub demuxer: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
}

pub struct PlaybackController {
    player: Option<MpvPlayer>,
    state: PlayerState,
    error: Option<String>,
    texture: Option<DynamicTexture>,
    texture_size: Option<(u32, u32)>,
    progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_before_mute: f64,
    aspect: String,
    stereo: StereoMode,
    pano_view: Option<panorama::View>,
    pano_map: Option<panorama::Map>,
    pano_buf: Vec<u8>,
    display_buf: Vec<u8>,
    crop_buf: Vec<u8>,
    crop_ratio: Option<f32>,
    recorder: Option<Recorder>,
    raw_input: Option<RawInput>,
    analysis: bool,
    frame_index: i64,
    audio_device: String,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            player: None,
            state: PlayerState::default(),
            error: None,
            texture: None,
            texture_size: None,
            progress_bounds: Rc::new(Cell::new(Bounds::default())),
            volume_before_mute: 100.0,
            aspect: "no".to_string(),
            stereo: StereoMode::Off,
            pano_view: None,
            pano_map: None,
            pano_buf: Vec::new(),
            display_buf: Vec::new(),
            crop_buf: Vec::new(),
            crop_ratio: None,
            recorder: None,
            raw_input: None,
            analysis: false,
            frame_index: 0,
            audio_device: "auto".to_string(),
        }
    }

    pub fn set_raw_input(&mut self, raw: RawInput) {
        self.raw_input = Some(raw);
    }

    pub fn set_analysis_mode(&mut self, enabled: bool) {
        self.analysis = enabled;
    }

    pub fn analysis_mode(&self) -> bool {
        self.analysis
    }

    pub fn raw_pixel_format(&self) -> Option<&str> {
        self.raw_input
            .as_ref()
            .map(|raw| raw.pixel_format.as_str())
            .filter(|format| !format.is_empty())
    }

    pub fn pixel_format(&self) -> Option<String> {
        self.player.as_ref().and_then(|player| player.pixel_format())
    }

    pub fn frame_number(&self) -> i64 {
        self.player
            .as_ref()
            .and_then(|player| player.frame_number())
            .filter(|number| *number > 0)
            .unwrap_or(self.frame_index)
    }

    pub fn analysis_fps(&self) -> f64 {
        self.raw_input
            .as_ref()
            .and_then(|raw| raw.fps)
            .or_else(|| {
                self.tracks()
                    .into_iter()
                    .find(|track| track.kind == "video" && track.fps > 0.0)
                    .map(|track| track.fps)
            })
            .unwrap_or(0.0)
    }

    pub fn state(&self) -> &PlayerState {
        &self.state
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_loaded(&self) -> bool {
        self.state.loaded
    }

    pub fn texture(&self) -> Option<DynamicTexture> {
        self.texture
    }

    pub fn texture_size(&self) -> Option<(u32, u32)> {
        self.texture_size
    }

    pub fn progress_bounds(&self) -> Rc<Cell<Bounds<Pixels>>> {
        self.progress_bounds.clone()
    }

    pub fn open(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.player.is_none() {
            match MpvPlayer::new() {
                Ok(player) => self.player = Some(player),
                Err(error) => {
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }
        self.frame_index = 0;
        let path = path.to_string_lossy();
        let analysis = self.analysis;
        let result = match self.player.as_ref() {
            Some(player) => apply_raw_input(player, self.raw_input.as_ref())
                .and_then(|()| {
                    let _ = player.set_property("audio-device", &self.audio_device);
                    player.open(&path)
                })
                .and_then(|()| {
                    if analysis {
                        player.pause()
                    } else {
                        player.play()
                    }
                }),
            None => Ok(()),
        };
        log::debug!("open media {path}: {result:?}");
        match result {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub fn configure_audio_device(&mut self, device: &str) {
        self.audio_device = device.to_string();
        if let Some(player) = self.player.as_ref() {
            let _ = player.set_property("audio-device", device);
        }
    }

    pub fn set_audio_device(&mut self, device: &str, cx: &mut Context<Self>) {
        self.configure_audio_device(device);
        cx.notify();
    }

    pub fn step_frame(&mut self, delta: i64, cx: &mut Context<Self>) {
        let result = self.player.as_ref().map(|player| {
            player.pause()?;
            player.command(if delta < 0 {
                "frame-back-step"
            } else {
                "frame-step"
            })
        });
        match result {
            Some(Ok(())) => {
                self.error = None;
                self.frame_index = (self.frame_index + delta).max(0);
            }
            Some(Err(error)) => self.error = Some(error.to_string()),
            None => {}
        }
        self.refresh();
        cx.notify();
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let result = self.player.as_ref().map(|player| player.toggle_pause());
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let result = self.player.as_ref().map(|player| player.stop());
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn seek_relative(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let result = self
            .player
            .as_ref()
            .map(|player| player.seek_relative(seconds));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn seek_fraction(&mut self, fraction: f64, cx: &mut Context<Self>) {
        if self.state.duration <= 0.0 {
            return;
        }
        let seconds = (fraction.clamp(0.0, 1.0) * self.state.duration).min(self.state.duration);
        let result = self
            .player
            .as_ref()
            .map(|player| player.seek_absolute(seconds));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn set_volume(&mut self, volume: f64, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, 100.0);
        let result = self.player.as_ref().map(|player| player.set_volume(volume));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.state.volume = volume;
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        let current = self.state.volume;
        if current > 0.0 {
            self.volume_before_mute = current;
            self.set_volume(0.0, cx);
        } else {
            let restored = if self.volume_before_mute > 0.0 {
                self.volume_before_mute
            } else {
                100.0
            };
            self.set_volume(restored, cx);
        }
    }

    pub fn chapters(&self) -> Vec<ChapterInfo> {
        self.player
            .as_ref()
            .map(MpvPlayer::chapters)
            .unwrap_or_default()
    }

    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.player
            .as_ref()
            .map(MpvPlayer::tracks)
            .unwrap_or_default()
    }

    pub fn speed(&self) -> f64 {
        self.player.as_ref().map(MpvPlayer::speed).unwrap_or(1.0)
    }

    pub fn seek_to(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let target = seconds.clamp(0.0, self.state.duration.max(0.0));
        let result = self
            .player
            .as_ref()
            .map(|player| player.seek_absolute(target));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn seek_chapter(&mut self, delta: i64, cx: &mut Context<Self>) {
        let result = self
            .player
            .as_ref()
            .map(|player| player.seek_chapter(delta));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn set_speed(&mut self, speed: f64, cx: &mut Context<Self>) {
        let speed = speed.clamp(0.1, 16.0);
        let result = self.player.as_ref().map(|player| player.set_speed(speed));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub fn set_property(&mut self, name: &str, value: &str, cx: &mut Context<Self>) {
        let result = self
            .player
            .as_ref()
            .map(|player| player.set_property(name, value));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub fn command(&mut self, command: &str, cx: &mut Context<Self>) {
        let result = self
            .player
            .as_ref()
            .map(|player| player.command(command));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub fn set_track(&mut self, kind: &str, id: i64, cx: &mut Context<Self>) {        let result = self
            .player
            .as_ref()
            .map(|player| player.set_track(kind, id));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        self.refresh();
        cx.notify();
    }

    pub fn set_repeat(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let result = self.player.as_ref().map(|player| player.set_repeat(enabled));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub fn set_shuffle(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let result = self
            .player
            .as_ref()
            .map(|player| player.set_shuffle(enabled));
        if let Some(Err(error)) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub fn stereo(&self) -> StereoMode {
        self.stereo
    }

    pub fn set_stereo(&mut self, mode: StereoMode, cx: &mut Context<Self>) {
        self.stereo = mode;
        self.pano_map = None;
        cx.notify();
    }

    pub fn set_pano(&mut self, view: Option<panorama::View>, cx: &mut Context<Self>) {
        self.pano_view = view;
        self.pano_map = None;
        cx.notify();
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn start_recording(&mut self, path: &Path, fps: f64) -> Result<(), String> {
        if self.recorder.is_some() {
            return Err("已在录制中".to_string());
        }
        if self.state.width == 0 || self.state.height == 0 {
            return Err("当前没有可录制的画面".to_string());
        }
        let recorder = Recorder::start(
            path,
            self.state.width as usize,
            self.state.height as usize,
            fps,
        )?;
        self.recorder = Some(recorder);
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Option<Result<RecordSummary, String>> {
        self.recorder.take().map(Recorder::stop)
    }

    pub fn aspect(&self) -> &str {
        &self.aspect
    }

    pub fn crop_ratio(&self) -> Option<f32> {
        self.crop_ratio
    }

    pub fn set_aspect(&mut self, value: &str, cx: &mut Context<Self>) {
        self.aspect = value.to_string();
        self.crop_ratio = None;
        cx.notify();
    }

    pub fn set_crop(&mut self, ratio: Option<f32>, cx: &mut Context<Self>) {
        self.crop_ratio = ratio;
        self.aspect = "no".to_string();
        cx.notify();
    }

    pub fn try_command(&mut self, command: &str) -> Result<(), String> {
        match &self.player {
            Some(player) => player.command(command).map_err(|error| error.to_string()),
            None => Err("没有正在播放的文件".to_string()),
        }
    }

    pub fn refresh(&mut self) {
        if let Some(player) = &self.player {
            let state = player.state();
            if state.loaded != self.state.loaded {
                log::debug!(
                    "playback loaded={} size={}x{} duration={:.2}",
                    state.loaded,
                    state.width,
                    state.height,
                    state.duration
                );
            }
            self.state = state;
        }
    }

    pub fn upload_frame(&mut self, window: &mut Window) {
        let (width, height) = (self.state.width, self.state.height);
        if width == 0 || height == 0 {
            return;
        }
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let frame = match player.render_frame(width, height) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let mut pixels = frame;
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        if let Some(view) = self.pano_view {
            let width_usize = width as usize;
            let height_usize = height as usize;
            let valid = self
                .pano_map
                .as_ref()
                .is_some_and(|map| map.matches(width_usize, height_usize, view));
            if !valid {
                self.pano_map = panorama::Map::build(width_usize, height_usize, view);
            }
            if let Some(map) = &self.pano_map {
                let frame = Frame {
                    width: width_usize,
                    height: height_usize,
                    rgba: pixels,
                    nv12: None,
                    pts: 0.0,
                    captured_at: std::time::Instant::now(),
                    serial: 0,
                };
                let rendered = map.render(&frame, &mut self.pano_buf);
                pixels = frame.rgba;
                if !rendered {
                    log::debug!("全景渲染失败");
                }
            }
        }
        let mut source: &[u8] = &pixels;
        if stereo::transform_rgba(
            width as usize,
            height as usize,
            source,
            self.stereo,
            &mut self.display_buf,
        ) {
            source = &self.display_buf;
        }
        if let Some(recorder) = &self.recorder {
            recorder.push(source);
        }
        let (mut out_width, mut out_height) = (width, height);
        if let Some(ratio) = self.crop_ratio {
            if let Some((cropped_width, cropped_height)) =
                crop_rgba(source, width as usize, height as usize, ratio, &mut self.crop_buf)
            {
                out_width = cropped_width as u32;
                out_height = cropped_height as u32;
                source = &self.crop_buf;
            }
        }
        if self.texture.is_some() && self.texture_size != Some((out_width, out_height)) {
            if let Some(texture) = self.texture.take() {
                window.remove_dynamic_texture(texture);
            }
        }
        let stride = out_width * 4;
        let dimensions = Size::new(
            DevicePixels(out_width as i32),
            DevicePixels(out_height as i32),
        );
        let bounds = Bounds::new(Point::new(DevicePixels(0), DevicePixels(0)), dimensions);

        if let Some(texture) = self.texture {
            match window.update_dynamic_texture(texture, bounds, source, stride) {
                Ok(()) => {
                    self.texture_size = Some((out_width, out_height));
                    return;
                }
                Err(error) => {
                    log::warn!("更新视频纹理失败：{error}");
                    window.remove_dynamic_texture(texture);
                    self.texture = None;
                }
            }
        }

        match window.create_dynamic_texture(dimensions, source, stride) {
            Ok(texture) => {
                self.texture = Some(texture);
                self.texture_size = Some((out_width, out_height));
            }
            Err(error) => self.error = Some(format!("创建视频纹理失败：{error}")),
        }
    }
}

fn apply_raw_input(player: &MpvPlayer, raw: Option<&RawInput>) -> Result<(), EngineError> {
    let Some(raw) = raw else {
        return Ok(());
    };
    if !raw.demuxer.is_empty() {
        player.set_property("demuxer-lavf-format", &raw.demuxer)?;
    }
    let mut options = Vec::new();
    if !raw.pixel_format.is_empty() {
        options.push(format!("pixel_format={}", raw.pixel_format));
    }
    if raw.width > 0 && raw.height > 0 {
        options.push(format!("video_size={}x{}", raw.width, raw.height));
    }
    if let Some(fps) = raw.fps {
        options.push(format!("framerate={fps}"));
    }
    if !options.is_empty() {
        player.set_property("demuxer-lavf-o", &options.join(","))?;
    }
    Ok(())
}

fn crop_rgba(
    source: &[u8],
    width: usize,
    height: usize,
    ratio: f32,
    buffer: &mut Vec<u8>,
) -> Option<(usize, usize)> {
    if width == 0 || height == 0 || !ratio.is_finite() || ratio <= 0.0 {
        return None;
    }
    let source_ratio = width as f32 / height as f32;
    let (crop_width, crop_height) = if source_ratio > ratio {
        ((height as f32 * ratio).round() as usize, height)
    } else {
        (width, (width as f32 / ratio).round() as usize)
    };
    let crop_width = crop_width.clamp(1, width);
    let crop_height = crop_height.clamp(1, height);
    if (crop_width, crop_height) == (width, height) {
        return None;
    }
    let offset_x = (width - crop_width) / 2;
    let offset_y = (height - crop_height) / 2;
    buffer.clear();
    buffer.reserve(crop_width * crop_height * 4);
    for row in offset_y..offset_y + crop_height {
        let start = (row * width + offset_x) * 4;
        buffer.extend_from_slice(&source[start..start + crop_width * 4]);
    }
    Some((crop_width, crop_height))
}
