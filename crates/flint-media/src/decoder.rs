use crate::capture_stream::{self, StreamSource};
use crate::capture_v4l2::{CaptureCodec, RawFormat, V4l2Stream};
use crate::latest_slot::{Frame, Nv12Planes};
use crate::{CaptureSettings, CaptureSmoothing, Shared, SourceInfo};
use crossbeam_channel::Sender;
use ffmpeg::codec::decoder::Video as VideoDecoder;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame::video::Video as RawVideoFrame;
use ffmpeg_next as ffmpeg;
use std::path::Path;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(120);
const PAUSE_POLL: Duration = Duration::from_millis(15);
const MAX_DECODE_FAILURES: u32 = 30;
const FAILURE_WINDOW: Duration = Duration::from_secs(2);
const MAX_SEND_ATTEMPTS: u32 = 3;

pub fn spawn(
    shared: Arc<Shared>,
    source: String,
    settings: CaptureSettings,
    ready: Sender<Result<SourceInfo, String>>,
) -> JoinHandle<()> {
    thread::spawn(move || run(shared, source, settings, ready))
}

enum Pipeline {
    Device(V4l2Stream),
    Stream(Box<StreamSource>),
}

fn run(
    shared: Arc<Shared>,
    source: String,
    settings: CaptureSettings,
    ready: Sender<Result<SourceInfo, String>>,
) {
    let mut converter = Converter::new(settings.smoothing);
    let opened = open_pipeline(&source, &settings, &shared);
    let mut pipeline = match opened {
        Ok((pipeline, info)) => {
            if ready.send(Ok(info)).is_err() {
                return;
            }
            pipeline
        }
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            shared.finish(Some(error));
            return;
        }
    };

    let mut decode_budget = DecodeBudget::new();
    let mut ended = false;
    let error = loop {
        if shared.should_stop() {
            break None;
        }
        let outcome = match &mut pipeline {
            Pipeline::Device(device) => {
                pump_device(device, &mut converter, &mut decode_budget, &shared)
            }
            Pipeline::Stream(stream) => {
                pump_stream(stream, &mut converter, &mut decode_budget, &shared)
            }
        };
        match outcome {
            Ok(()) => {
                ended = false;
            }
            Err(error) => {
                if shared.should_stop() {
                    break None;
                }
                if matches!(&pipeline, Pipeline::Stream(_)) && error == crate::EOF_ERROR {
                    if !ended {
                        ended = true;
                        shared.finish(Some(error));
                    }
                    thread::sleep(PAUSE_POLL);
                } else {
                    break Some(error);
                }
            }
        }
    };

    match &error {
        Some(message) => log::warn!("采集结束 {source}：{message}"),
        None => log::info!("采集结束 {source}"),
    }
    shared.finish(error);
}

fn open_pipeline(
    source: &str,
    settings: &CaptureSettings,
    shared: &Arc<Shared>,
) -> Result<(Pipeline, SourceInfo), String> {
    if capture_stream::is_device(source) {
        let device = V4l2Stream::open(
            Path::new(source),
            settings.fourcc.as_deref(),
            settings.width,
            settings.height,
            settings.fps,
        )?;
        let info = SourceInfo {
            source: source.to_string(),
            transport: "V4L2".into(),
            codec: device.codec().label().to_string(),
            width: device.width(),
            height: device.height(),
            pixel_format: device.fourcc().to_string(),
            nominal_fps: device.fps(),
            duration: 0.0,
            audio_tracks: Vec::new(),
        };
        Ok((Pipeline::Device(device), info))
    } else {
        let stream = capture_stream::open(source, settings, Arc::clone(shared))?;
        let transport = if capture_stream::is_screen(source) {
            "X11"
        } else {
            "Stream"
        };
        let info = SourceInfo {
            source: source.to_string(),
            transport: transport.into(),
            codec: stream.info.codec.clone(),
            width: stream.info.width,
            height: stream.info.height,
            pixel_format: stream.info.pixel_format.clone(),
            nominal_fps: stream.info.nominal_fps,
            duration: stream.duration,
            audio_tracks: stream.info.audio_tracks.clone(),
        };
        Ok((Pipeline::Stream(Box::new(stream)), info))
    }
}

fn pump_device(
    device: &mut V4l2Stream,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
) -> Result<(), String> {
    let Some(dequeued) = device.dequeue(POLL_INTERVAL)? else {
        return Ok(());
    };
    let captured_at = Instant::now();
    let bytes_used = dequeued.bytes_used;
    let width = device.width();
    let height = device.height();
    let stride = device.stride();
    let codec = device.codec();
    {
        let mapping = device.bytes(dequeued.index);
        let data = &mapping[..bytes_used.min(mapping.len())];
        shared.stats.record_bytes(captured_at, data.len() as u64);
        let mut state = FeedState { shared, failures };
        converter.feed(
            codec,
            data,
            FrameShape {
                width,
                height,
                stride,
            },
            captured_at,
            dequeued.timestamp,
            &mut state,
        )?;
    }
    device.requeue(dequeued.index)
}

fn pump_stream(
    stream: &mut StreamSource,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
) -> Result<(), String> {
    if shared.should_stop() {
        return Ok(());
    }
    handle_external_audio_request(stream, shared);
    let requested = shared.audio_track();
    let output_rate = shared.audio_output_rate();
    let output_format = shared.audio_output_format();
    let output_device = shared.audio_device();
    let output_device2 = shared.audio_device2();
    let output_passthrough = shared.audio_passthrough_device();
    let output_method = shared.audio_output_method();
    let stale = stream.audio.as_ref().is_some_and(|track| {
        track.rate != output_rate
            || track.format != output_format
            || track.device != output_device
            || track.device2 != output_device2
            || track.passthrough != output_passthrough
            || track.method != output_method
    });
    if requested != stream.active_audio || stale {
        if stream.external_audio.is_some() {
            stream.audio = None;
            stream.active_audio = None;
        } else {
            let (audio, active) = match capture_stream::open_audio(
                &stream.input,
                requested,
                capture_stream::AudioOutputConfig {
                    rate: output_rate,
                    format: output_format,
                    device: output_device,
                    device2: output_device2,
                    passthrough: output_passthrough,
                    method: output_method,
                },
            ) {
                Some((track, ordinal)) => (Some(track), Some(ordinal)),
                None => (None, None),
            };
            stream.audio = audio;
            stream.active_audio = active;
            shared.set_audio_track(active);
        }
    }
    if let Some(target) = shared.take_seek() {
        seek_to(stream, target)?;
        decode_until(stream, converter, failures, shared, target)?;
        shared.note_last_pts(target);
        return Ok(());
    }
    if shared.paused() {
        let steps = shared.take_steps();
        if steps == 0 {
            thread::sleep(PAUSE_POLL);
            return Ok(());
        }
        stream.clock = None;
        return decode_steps(stream, converter, failures, shared, steps);
    }
    pump_external_audio(stream, shared)?;
    let Some((_, packet)) = stream.input.packets().next() else {
        if shared.should_stop() {
            return Ok(());
        }
        return Err(crate::EOF_ERROR.to_string());
    };
    if packet.stream() != stream.stream_index {
        if stream.external_audio.is_none() {
            if let Some(audio) = stream.audio.as_mut() {
                if packet.stream() == audio.stream_index {
                    if let Some(data) = packet.data() {
                        let mut options = shared.audio_options();
                        if (shared.speed() - 1.0).abs() >= f32::EPSILON {
                            options.gain = 0.0;
                        }
                        audio.push(data, options, shared)?;
                    }
                }
            }
        }
        return Ok(());
    }
    let Some(data) = packet.data() else {
        return Ok(());
    };
    let captured_at = Instant::now();
    let pts = packet_pts(&packet, stream.time_base);
    if stream.local {
        pace(stream, pts, shared);
    }
    shared.stats.record_bytes(captured_at, data.len() as u64);
    let packet = ffmpeg::Packet::copy(data);
    submit_packet(
        &mut stream.decoder,
        converter,
        failures,
        shared,
        &packet,
        captured_at,
        pts,
    )
}

fn decode_steps(
    stream: &mut StreamSource,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
    steps: i32,
) -> Result<(), String> {
    let target = steps.unsigned_abs() as u64;
    if steps < 0 {
        let back = target as f64 * stream.frame_duration;
        let target_pts = (shared.last_pts() - back).max(0.0);
        seek_back(stream, target_pts)?;
        return decode_until(stream, converter, failures, shared, target_pts);
    }
    let start_frames = shared.stats_snapshot().frames;
    let mut packets = 0u64;
    let limit = target * 8 + 8;
    while shared.stats_snapshot().frames.saturating_sub(start_frames) < target {
        if shared.should_stop() {
            return Ok(());
        }
        packets += 1;
        if packets > limit {
            break;
        }
        let Some((_, packet)) = stream.input.packets().next() else {
            shared.set_paused(false);
            return Err(crate::EOF_ERROR.to_string());
        };
        if packet.stream() != stream.stream_index {
            continue;
        }
        let Some(data) = packet.data() else {
            continue;
        };
        let captured_at = Instant::now();
        let pts = packet_pts(&packet, stream.time_base);
        shared.stats.record_bytes(captured_at, data.len() as u64);
        let packet = ffmpeg::Packet::copy(data);
        submit_packet(
            &mut stream.decoder,
            converter,
            failures,
            shared,
            &packet,
            captured_at,
            pts,
        )?;
    }
    Ok(())
}

fn seek_to(stream: &mut StreamSource, seconds: f64) -> Result<(), String> {
    let timestamp = (seconds.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
    stream
        .input
        .seek(timestamp, ..)
        .map_err(|error| format!("定位视频失败：{error}"))?;
    stream.decoder.flush();
    stream.clock = None;
    if let Some(audio) = stream.audio.as_mut() {
        audio.flush();
    }
    seek_external_audio(stream, seconds);
    Ok(())
}

fn seek_back(stream: &mut StreamSource, target: f64) -> Result<(), String> {
    let timestamp = (target.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
    stream
        .input
        .seek(timestamp, ..)
        .map_err(|error| format!("定位视频帧失败：{error}"))?;
    stream.decoder.flush();
    if let Some(audio) = stream.audio.as_mut() {
        audio.flush();
    }
    seek_external_audio(stream, target);
    Ok(())
}

fn seek_external_audio(stream: &mut StreamSource, seconds: f64) {
    let Some(external) = stream.external_audio.as_mut() else {
        return;
    };
    let timestamp = (seconds.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
    if external.input.seek(timestamp, ..).is_err() {
        log::warn!("外部音频定位失败：{seconds:.2} 秒");
    }
    external.track.flush();
    external.next_pts = seconds.max(0.0);
}

const EXTERNAL_AUDIO_LOOKAHEAD: f64 = 0.25;

fn handle_external_audio_request(stream: &mut StreamSource, shared: &Shared) {
    let Some(request) = shared.take_external_audio_request() else {
        return;
    };
    if request.is_empty() {
        if stream.external_audio.take().is_some() {
            stream.audio = None;
            stream.active_audio = None;
            shared.set_external_audio_label(None);
            shared.push_external_audio_notice("已恢复内置音频".to_string());
        }
        return;
    }
    stream.audio = None;
    stream.active_audio = None;
    match open_external_audio(&request, shared, shared.last_pts()) {
        Some(external) => {
            let label = Path::new(&request)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| request.clone());
            stream.external_audio = Some(external);
            shared.set_external_audio_label(Some(label.clone()));
            shared.push_external_audio_notice(format!("已载入外部音频：{label}"));
        }
        None => {
            shared.set_external_audio_label(None);
            shared.push_external_audio_notice(format!("无法载入外部音频：{request}"));
        }
    }
}

fn open_external_audio(
    path: &str,
    shared: &Shared,
    start: f64,
) -> Option<capture_stream::ExternalAudio> {
    let mut input = ffmpeg::format::input(&Path::new(path)).ok()?;
    let (stream_index, time_base) = {
        let stream = input.streams().best(ffmpeg::media::Type::Audio)?;
        (stream.index(), f64::from(stream.time_base()))
    };
    let (track, _) = capture_stream::open_audio(
        &input,
        None,
        capture_stream::AudioOutputConfig {
            rate: shared.audio_output_rate(),
            format: shared.audio_output_format(),
            device: shared.audio_device(),
            device2: shared.audio_device2(),
            passthrough: shared.audio_passthrough_device(),
            method: shared.audio_output_method(),
        },
    )?;
    let timestamp = (start.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
    let _ = input.seek(timestamp, ..);
    Some(capture_stream::ExternalAudio {
        input,
        track,
        stream_index,
        time_base,
        next_pts: start.max(0.0),
    })
}

fn pump_external_audio(stream: &mut StreamSource, shared: &Shared) -> Result<(), String> {
    let video_pts = shared.last_pts();
    let Some(external) = stream.external_audio.as_mut() else {
        return Ok(());
    };
    let mut budget = 0;
    while external.next_pts <= video_pts + EXTERNAL_AUDIO_LOOKAHEAD && budget < 512 {
        let Some((_, packet)) = external.input.packets().next() else {
            break;
        };
        budget += 1;
        if packet.stream() != external.stream_index {
            continue;
        }
        let Some(data) = packet.data() else {
            continue;
        };
        external.next_pts = packet_pts(&packet, external.time_base);
        let mut options = shared.audio_options();
        if (shared.speed() - 1.0).abs() >= f32::EPSILON {
            options.gain = 0.0;
        }
        external.track.push(data, options, shared)?;
    }
    Ok(())
}

// Demuxer seeks land on the preceding keyframe; decoding forward reproduces
// PotPlayer's accurate seek and single-frame stepping.
fn decode_until(
    stream: &mut StreamSource,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
    target: f64,
) -> Result<(), String> {
    const MAX_SEEK_PACKETS: u64 = 8192;
    let tolerance = stream.frame_duration * 0.5;
    let mut published = f64::NEG_INFINITY;
    let mut packets = 0u64;
    while published + tolerance < target {
        if shared.should_stop() {
            return Ok(());
        }
        packets += 1;
        if packets > MAX_SEEK_PACKETS {
            break;
        }
        let Some((_, packet)) = stream.input.packets().next() else {
            return Ok(());
        };
        if packet.stream() != stream.stream_index {
            continue;
        }
        let Some(data) = packet.data() else {
            continue;
        };
        let captured_at = Instant::now();
        let pts = packet_pts(&packet, stream.time_base);
        let frames_before = shared.stats_snapshot().frames;
        shared.stats.record_bytes(captured_at, data.len() as u64);
        let packet = ffmpeg::Packet::copy(data);
        submit_packet(
            &mut stream.decoder,
            converter,
            failures,
            shared,
            &packet,
            captured_at,
            pts,
        )?;
        if shared.stats_snapshot().frames > frames_before {
            published = shared.last_pts();
        } else if shared.last_pts() > published {
            published = shared.last_pts();
        }
    }
    Ok(())
}

fn pace(stream: &mut StreamSource, pts: f64, shared: &Shared) {
    const MAX_SLEEP: Duration = Duration::from_millis(100);
    let speed = shared.speed().clamp(0.1, 4.0);
    let (anchor_at, anchor_pts, anchor_speed) =
        *stream.clock.get_or_insert((Instant::now(), pts, speed));
    if pts < anchor_pts || (speed - anchor_speed).abs() > f32::EPSILON {
        stream.clock = Some((Instant::now(), pts, speed));
        return;
    }
    let Some(wait) = (anchor_at + Duration::from_secs_f64((pts - anchor_pts) / f64::from(speed)))
        .checked_duration_since(Instant::now())
    else {
        return;
    };
    let deadline = Instant::now() + wait.min(MAX_SLEEP);
    while Instant::now() < deadline {
        if shared.should_stop() || shared.paused() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn packet_pts(packet: &ffmpeg::Packet, time_base: f64) -> f64 {
    packet
        .pts()
        .map(|value| value as f64 * time_base)
        .unwrap_or(0.0)
}

#[derive(Clone, Copy)]
struct FrameShape {
    width: u32,
    height: u32,
    stride: usize,
}

struct FeedState<'a> {
    shared: &'a Shared,
    failures: &'a mut DecodeBudget,
}

struct DecodeBudget {
    failures: u32,
    since: Option<Instant>,
}

impl DecodeBudget {
    fn new() -> Self {
        Self {
            failures: 0,
            since: None,
        }
    }

    fn clear(&mut self) {
        self.failures = 0;
        self.since = None;
    }

    fn record(&mut self, message: String) -> Result<(), String> {
        let now = Instant::now();
        let since = *self.since.get_or_insert(now);
        self.failures += 1;
        if self.failures >= MAX_DECODE_FAILURES
            && now.saturating_duration_since(since) >= FAILURE_WINDOW
        {
            return Err(message);
        }
        log::warn!("跳过无法解码的数据包：{message}");
        Ok(())
    }
}

struct Converter {
    decoder: Option<VideoDecoder>,
    scaler: Option<scaling::Context>,
    scaler_key: Option<(Pixel, u32, u32, scaling::Flags)>,
    scaler_flags: scaling::Flags,
    output: Option<RawVideoFrame>,
    raw: Option<RawVideoFrame>,
    spare: Vec<u8>,
    reported_format: Option<Pixel>,
    output_format: Pixel,
    yuv_unsupported: bool,
    yuv_logged: bool,
}

impl Converter {
    fn new(smoothing: CaptureSmoothing) -> Self {
        Self {
            decoder: None,
            scaler: None,
            scaler_key: None,
            scaler_flags: smoothing_flags(smoothing),
            output: None,
            raw: None,
            spare: Vec::new(),
            reported_format: None,
            output_format: Pixel::RGBA,
            yuv_unsupported: false,
            yuv_logged: false,
        }
    }

    fn feed(
        &mut self,
        codec: CaptureCodec,
        data: &[u8],
        shape: FrameShape,
        captured_at: Instant,
        pts: f64,
        state: &mut FeedState<'_>,
    ) -> Result<(), String> {
        if data.is_empty() || shape.width == 0 || shape.height == 0 {
            return Ok(());
        }
        match codec {
            CaptureCodec::Raw(raw) => {
                let format = pixel_format(raw);
                let mut frame = match self.raw.take() {
                    Some(frame)
                        if frame.format() == format
                            && frame.width() == shape.width
                            && frame.height() == shape.height =>
                    {
                        frame
                    }
                    _ => RawVideoFrame::new(format, shape.width, shape.height),
                };
                let result = fill_raw_frame(
                    &mut frame,
                    raw,
                    data,
                    shape.stride,
                    shape.width,
                    shape.height,
                )
                .and_then(|()| {
                    let started = Instant::now();
                    self.convert_with_latency(&frame, state.shared, captured_at, pts, started)
                });
                self.raw = Some(frame);
                result
            }
            CaptureCodec::Mjpeg | CaptureCodec::H264 | CaptureCodec::Hevc => {
                self.ensure_decoder(codec)?;
                let mut decoder = self
                    .decoder
                    .take()
                    .ok_or_else(|| "解码器未初始化".to_string())?;
                let result = self.feed_compressed(&mut decoder, data, captured_at, pts, state);
                self.decoder = Some(decoder);
                result
            }
        }
    }

    fn feed_compressed(
        &mut self,
        decoder: &mut VideoDecoder,
        data: &[u8],
        captured_at: Instant,
        pts: f64,
        state: &mut FeedState<'_>,
    ) -> Result<(), String> {
        let packet = ffmpeg::Packet::copy(data);
        submit_packet(
            decoder,
            self,
            state.failures,
            state.shared,
            &packet,
            captured_at,
            pts,
        )
    }

    fn ensure_decoder(&mut self, codec: CaptureCodec) -> Result<(), String> {
        if self.decoder.is_some() {
            return Ok(());
        }
        let id = match codec {
            CaptureCodec::Mjpeg => ffmpeg::codec::Id::MJPEG,
            CaptureCodec::H264 => ffmpeg::codec::Id::H264,
            CaptureCodec::Hevc => ffmpeg::codec::Id::HEVC,
            CaptureCodec::Raw(_) => unreachable!(),
        };
        let codec = ffmpeg::decoder::find(id)
            .ok_or_else(|| format!("系统缺少 {} 解码器", codec.label()))?;
        let mut context = ffmpeg::codec::Context::new();
        context.set_flags(ffmpeg::codec::Flags::LOW_DELAY);
        context.set_threading(ffmpeg::codec::threading::Config::count(1));
        let decoder = context
            .decoder()
            .open_as(codec)
            .and_then(|opened| opened.video())
            .map_err(|error| format!("打开 {} 解码器失败：{error}", codec.name()))?;
        self.decoder = Some(decoder);
        Ok(())
    }

    fn convert_with_latency(
        &mut self,
        frame: &RawVideoFrame,
        shared: &Shared,
        captured_at: Instant,
        pts: f64,
        started: Instant,
    ) -> Result<(), String> {
        let decode_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.convert(frame, shared, captured_at, pts)?;
        shared.stats.record_frame(Instant::now(), decode_ms);
        Ok(())
    }

    fn convert(
        &mut self,
        frame: &RawVideoFrame,
        shared: &Shared,
        captured_at: Instant,
        pts: f64,
    ) -> Result<(), String> {
        let width = frame.width();
        let height = frame.height();
        if width == 0 || height == 0 {
            return Ok(());
        }
        if shared.video_renderer_null() {
            shared.note_last_pts(pts);
            return Ok(());
        }
        if self.reported_format != Some(frame.format()) {
            self.reported_format = Some(frame.format());
            let name = frame
                .format()
                .descriptor()
                .map(|descriptor| descriptor.name())
                .unwrap_or("unknown");
            shared.note_pixel_format(name);
        }
        if shared.yuv_output() && !self.yuv_unsupported && width % 2 == 0 && height % 2 == 0 {
            match self.convert_nv12(frame, shared, captured_at, pts) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    log::warn!("NV12 直出不可用，回退 RGBA：{error}");
                    self.yuv_unsupported = true;
                    shared.set_yuv_output(false);
                }
            }
        }

        self.ensure_scaler(frame.format(), width, height, Pixel::RGBA)?;
        let scaler = self
            .scaler
            .as_mut()
            .ok_or_else(|| "缩放器未初始化".to_string())?;
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| "输出帧未初始化".to_string())?;
        scaler
            .run(frame, output)
            .map_err(|error| format!("RGBA 转换失败：{error}"))?;

        if self.spare.is_empty() {
            if let Some(buffer) = shared.slot.acquire_buffer() {
                self.spare = buffer;
            }
        }
        let row = width as usize * 4;
        let stride = output.stride(0);
        let plane = output.data(0);
        self.spare.resize(row * height as usize, 0);
        for y in 0..height as usize {
            let start = y * stride;
            self.spare[y * row..(y + 1) * row].copy_from_slice(&plane[start..start + row]);
        }

        let rgba = std::mem::take(&mut self.spare);
        let recycled = shared.publish_frame(Frame {
            width: width as usize,
            height: height as usize,
            rgba,
            nv12: None,
            pts,
            captured_at,
            serial: 0,
        });
        self.spare = recycled.unwrap_or_default();
        Ok(())
    }

    fn convert_nv12(
        &mut self,
        frame: &RawVideoFrame,
        shared: &Shared,
        captured_at: Instant,
        pts: f64,
    ) -> Result<(), String> {
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        self.ensure_scaler(frame.format(), width as u32, height as u32, Pixel::NV12)?;
        let scaler = self
            .scaler
            .as_mut()
            .ok_or_else(|| "缩放器未初始化".to_string())?;
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| "输出帧未初始化".to_string())?;
        scaler
            .run(frame, output)
            .map_err(|error| format!("NV12 转换失败：{error}"))?;

        let chroma_height = height.div_ceil(2);
        let y_row = width;
        let uv_row = width.div_ceil(2) * 2;
        let mut planes = shared.slot.acquire_planes().unwrap_or_else(|| Nv12Planes {
            y: Vec::new(),
            uv: Vec::new(),
            y_stride: 0,
            uv_stride: 0,
        });
        planes.y.resize(y_row * height, 0);
        planes.uv.resize(uv_row * chroma_height, 0);

        let luma_stride = output.stride(0);
        let luma = output.data(0);
        for row in 0..height {
            let start = row * luma_stride;
            planes.y[row * y_row..(row + 1) * y_row].copy_from_slice(&luma[start..start + y_row]);
        }
        let chroma_stride = output.stride(1);
        let chroma = output.data(1);
        for row in 0..chroma_height {
            let start = row * chroma_stride;
            planes.uv[row * uv_row..(row + 1) * uv_row]
                .copy_from_slice(&chroma[start..start + uv_row]);
        }
        planes.y_stride = y_row;
        planes.uv_stride = uv_row;
        if !self.yuv_logged {
            self.yuv_logged = true;
            log::info!("采集 NV12 直出已启用：{width}x{height}");
        }

        shared.publish_frame(Frame {
            width,
            height,
            rgba: Vec::new(),
            nv12: Some(planes),
            pts,
            captured_at,
            serial: 0,
        });
        Ok(())
    }

    fn ensure_scaler(
        &mut self,
        format: Pixel,
        width: u32,
        height: u32,
        target: Pixel,
    ) -> Result<(), String> {
        let key = (format, width, height, self.scaler_flags);
        if self.scaler_key == Some(key) && self.scaler.is_some() && self.output_format == target {
            return Ok(());
        }
        let scaler = scaling::Context::get(
            format,
            width,
            height,
            target,
            width,
            height,
            self.scaler_flags,
        )
        .map_err(|error| format!("创建缩放器失败：{error}"))?;
        self.scaler = Some(scaler);
        self.output = Some(RawVideoFrame::new(target, width, height));
        self.scaler_key = Some(key);
        self.output_format = target;
        Ok(())
    }
}

fn smoothing_flags(smoothing: CaptureSmoothing) -> scaling::Flags {
    match smoothing {
        CaptureSmoothing::None => scaling::Flags::POINT,
        CaptureSmoothing::System => scaling::Flags::empty(),
        CaptureSmoothing::Low => scaling::Flags::FAST_BILINEAR,
        CaptureSmoothing::Normal => scaling::Flags::BILINEAR,
        CaptureSmoothing::Medium => scaling::Flags::BICUBIC,
        CaptureSmoothing::High => scaling::Flags::LANCZOS,
    }
}

fn submit_packet(
    decoder: &mut VideoDecoder,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
    packet: &ffmpeg::Packet,
    captured_at: Instant,
    pts: f64,
) -> Result<(), String> {
    if shared.video_renderer_compressed() {
        shared.note_last_pts(pts);
        return Ok(());
    }
    let started = Instant::now();
    let mut delivered = false;
    for _ in 0..MAX_SEND_ATTEMPTS {
        match decoder.send_packet(packet) {
            Ok(()) => {
                delivered = true;
                break;
            }
            // FFmpeg only accepts a packet after pending output has been received,
            // so EAGAIN here means "drain, then resubmit", never "drop and return".
            Err(error) if is_eagain(&error) => {
                drain_frames(
                    decoder,
                    converter,
                    failures,
                    shared,
                    captured_at,
                    pts,
                    started,
                )?;
            }
            Err(error) => {
                return record_decode_failure(
                    failures,
                    format!("提交解码数据失败：{}", ffmpeg_message(&error)),
                );
            }
        }
    }
    if !delivered {
        return record_decode_failure(failures, "解码器持续拒绝数据".to_string());
    }
    drain_frames(
        decoder,
        converter,
        failures,
        shared,
        captured_at,
        pts,
        started,
    )
}

fn drain_frames(
    decoder: &mut VideoDecoder,
    converter: &mut Converter,
    failures: &mut DecodeBudget,
    shared: &Shared,
    captured_at: Instant,
    pts: f64,
    started: Instant,
) -> Result<(), String> {
    let mut frame = RawVideoFrame::empty();
    loop {
        match decoder.receive_frame(&mut frame) {
            Ok(()) => {
                failures.clear();
                converter.convert_with_latency(&frame, shared, captured_at, pts, started)?;
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => break,
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => {
                return record_decode_failure(
                    failures,
                    format!("解码失败：{}", ffmpeg_message(&error)),
                );
            }
        }
    }
    Ok(())
}

fn record_decode_failure(failures: &mut DecodeBudget, message: String) -> Result<(), String> {
    failures.record(message)
}

fn is_eagain(error: &ffmpeg::Error) -> bool {
    matches!(error, ffmpeg::Error::Other { errno } if *errno == ffmpeg::util::error::EAGAIN)
}

fn ffmpeg_message(error: &ffmpeg::Error) -> String {
    let text = error.to_string();
    if text.is_empty() {
        format!("{error:?}")
    } else {
        text
    }
}

fn pixel_format(raw: RawFormat) -> Pixel {
    match raw {
        RawFormat::Yuyv422 => Pixel::YUYV422,
        RawFormat::Uyvy422 => Pixel::UYVY422,
        RawFormat::Nv12 => Pixel::NV12,
        RawFormat::Rgb24 => Pixel::RGB24,
        RawFormat::Bgr24 => Pixel::BGR24,
        RawFormat::Gray8 => Pixel::GRAY8,
    }
}

fn fill_raw_frame(
    frame: &mut RawVideoFrame,
    raw: RawFormat,
    data: &[u8],
    stride: usize,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let stride = if stride == 0 { frame.stride(0) } else { stride };
    match raw {
        RawFormat::Nv12 => {
            let uv_offset = stride
                .checked_mul(height as usize)
                .ok_or_else(|| "NV12 平面偏移溢出".to_string())?;
            copy_plane(frame, 0, data, 0, stride, height as usize, width as usize)?;
            copy_plane(
                frame,
                1,
                data,
                uv_offset,
                stride,
                height as usize / 2,
                width as usize,
            )?;
        }
        RawFormat::Gray8 => {
            copy_plane(frame, 0, data, 0, stride, height as usize, width as usize)?;
        }
        RawFormat::Rgb24 | RawFormat::Bgr24 => {
            copy_plane(
                frame,
                0,
                data,
                0,
                stride,
                height as usize,
                width as usize * 3,
            )?;
        }
        RawFormat::Yuyv422 | RawFormat::Uyvy422 => {
            copy_plane(
                frame,
                0,
                data,
                0,
                stride,
                height as usize,
                width as usize * 2,
            )?;
        }
    }
    Ok(())
}

fn copy_plane(
    frame: &mut RawVideoFrame,
    plane: usize,
    data: &[u8],
    offset: usize,
    src_stride: usize,
    rows: usize,
    row_bytes: usize,
) -> Result<(), String> {
    let dst_stride = frame.stride(plane);
    let dst = frame.data_mut(plane);
    for row in 0..rows {
        let start = offset + row * src_stride;
        if start >= data.len() {
            break;
        }
        let len = row_bytes.min(src_stride).min(data.len() - start);
        let target = row * dst_stride;
        if target + len > dst.len() {
            break;
        }
        dst[target..target + len].copy_from_slice(&data[start..start + len]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_yuyv_publishes_nv12_when_enabled() {
        let shared = Shared::new();
        shared.set_yuv_output(true);
        let mut converter = Converter::new(CaptureSmoothing::None);
        let mut budget = DecodeBudget::new();
        let mut state = FeedState {
            shared: &shared,
            failures: &mut budget,
        };
        let mut data = vec![128u8; 8 * 4];
        for byte in data.iter_mut().step_by(2) {
            *byte = 100;
        }
        converter
            .feed(
                CaptureCodec::Raw(RawFormat::Yuyv422),
                &data,
                FrameShape {
                    width: 4,
                    height: 4,
                    stride: 8,
                },
                Instant::now(),
                0.0,
                &mut state,
            )
            .expect("raw frame converts");

        let frame = shared.slot.take().expect("frame published");
        assert!(frame.rgba.is_empty());
        let planes = frame.nv12.as_ref().expect("nv12 planes");
        assert_eq!((frame.width, frame.height), (4, 4));
        assert_eq!((planes.y_stride, planes.uv_stride), (4, 4));
        assert_eq!(planes.y.len(), 16);
        assert_eq!(planes.uv.len(), 8);
        assert!(planes.y.iter().all(|&value| value == planes.y[0]));
    }

    #[test]
    fn odd_dimensions_fall_back_to_rgba() {
        let shared = Shared::new();
        shared.set_yuv_output(true);
        let mut converter = Converter::new(CaptureSmoothing::None);
        let mut budget = DecodeBudget::new();
        let mut state = FeedState {
            shared: &shared,
            failures: &mut budget,
        };
        converter
            .feed(
                CaptureCodec::Raw(RawFormat::Gray8),
                &[64u8; 5 * 3],
                FrameShape {
                    width: 5,
                    height: 3,
                    stride: 5,
                },
                Instant::now(),
                0.0,
                &mut state,
            )
            .expect("raw frame converts");

        let frame = shared.slot.take().expect("frame published");
        assert!(frame.nv12.is_none());
        assert_eq!(frame.rgba.len(), 5 * 3 * 4);
    }
}
