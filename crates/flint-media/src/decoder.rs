use crate::capture_stream::{self, StreamSource};
use crate::capture_v4l2::{CaptureCodec, RawFormat, V4l2Stream};
use crate::latest_slot::Frame;
use crate::{CaptureSettings, Shared, SourceInfo};
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
    Stream(StreamSource),
}

fn run(
    shared: Arc<Shared>,
    source: String,
    settings: CaptureSettings,
    ready: Sender<Result<SourceInfo, String>>,
) {
    let mut converter = Converter::new();
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
            Ok(()) => {}
            Err(error) => {
                break if shared.should_stop() { None } else { Some(error) };
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
        };
        Ok((Pipeline::Device(device), info))
    } else {
        let stream = capture_stream::open(
            source,
            settings.width,
            settings.height,
            settings.fps,
            Arc::clone(shared),
        )?;
        let info = SourceInfo {
            source: source.to_string(),
            transport: "Stream".into(),
            codec: stream.info.codec.clone(),
            width: stream.info.width,
            height: stream.info.height,
            pixel_format: stream.info.pixel_format.clone(),
            nominal_fps: stream.info.nominal_fps,
        };
        Ok((Pipeline::Stream(stream), info))
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
    let Some((_, packet)) = stream.input.packets().next() else {
        if shared.should_stop() {
            return Ok(());
        }
        return Err("输入流已结束".to_string());
    };
    if packet.stream() != stream.stream_index {
        return Ok(());
    }
    let Some(data) = packet.data() else {
        return Ok(());
    };
    let captured_at = Instant::now();
    let pts = packet
        .pts()
        .map(|value| value as f64 * stream.time_base)
        .unwrap_or(0.0);
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
    scaler_key: Option<(Pixel, u32, u32)>,
    output: Option<RawVideoFrame>,
    raw: Option<RawVideoFrame>,
    spare: Vec<u8>,
    reported_format: Option<Pixel>,
}

impl Converter {
    fn new() -> Self {
        Self {
            decoder: None,
            scaler: None,
            scaler_key: None,
            output: None,
            raw: None,
            spare: Vec::new(),
            reported_format: None,
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
                let result =
                    self.feed_compressed(&mut decoder, data, captured_at, pts, state);
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
        if self.reported_format != Some(frame.format()) {
            self.reported_format = Some(frame.format());
            let name = frame
                .format()
                .descriptor()
                .map(|descriptor| descriptor.name())
                .unwrap_or("unknown");
            shared.note_pixel_format(name);
        }
        self.ensure_scaler(frame.format(), width, height)?;
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
            pts,
            captured_at,
            serial: 0,
        });
        self.spare = recycled.unwrap_or_default();
        Ok(())
    }

    fn ensure_scaler(&mut self, format: Pixel, width: u32, height: u32) -> Result<(), String> {
        let key = (format, width, height);
        if self.scaler_key == Some(key) && self.scaler.is_some() {
            return Ok(());
        }
        let scaler = scaling::Context::get(
            format,
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            scaling::Flags::BILINEAR,
        )
        .map_err(|error| format!("创建 RGBA 缩放器失败：{error}"))?;
        self.scaler = Some(scaler);
        self.output = Some(RawVideoFrame::new(Pixel::RGBA, width, height));
        self.scaler_key = Some(key);
        Ok(())
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
                drain_frames(decoder, converter, failures, shared, captured_at, pts, started)?;
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
    drain_frames(decoder, converter, failures, shared, captured_at, pts, started)
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
    let stride = if stride == 0 {
        frame.stride(0)
    } else {
        stride
    };
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
