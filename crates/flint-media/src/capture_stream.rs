use crate::Shared;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

pub struct StreamInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub nominal_fps: f64,
}

pub struct StreamSource {
    pub input: ffmpeg::format::context::Input,
    pub decoder: ffmpeg::codec::decoder::Video,
    pub stream_index: usize,
    pub time_base: f64,
    pub info: StreamInfo,
}

pub fn is_device(source: &str) -> bool {
    source.starts_with("/dev/video")
}

pub fn open(
    source: &str,
    width: u32,
    height: u32,
    fps: u32,
    shared: Arc<Shared>,
) -> Result<StreamSource, String> {
    ffmpeg::init().map_err(|error| format!("FFmpeg 初始化失败：{error}"))?;

    let mut options = ffmpeg::Dictionary::new();
    options.set("fflags", "nobuffer");
    options.set("flags", "low_delay");
    options.set("probesize", "32768");
    options.set("analyzeduration", "0");
    options.set("max_delay", "500000");
    options.set("reorder_queue_size", "0");
    if width > 0 && height > 0 {
        options.set("video_size", &format!("{width}x{height}"));
    }
    if fps > 0 {
        options.set("framerate", &fps.to_string());
    }
    if source.contains("://") {
        options.set("rw_timeout", "5000000");
    }
    if source.starts_with("rtsp://") {
        options.set("rtsp_transport", "tcp");
        options.set("stimeout", "5000000");
    }
    if source.starts_with("udp://") || source.starts_with("rtp://") {
        options.set("buffer_size", "65536");
    }

    let interrupt_shared = shared;
    let input = open_input(source, options, Box::new(move || interrupt_shared.should_stop()))?;

    let (stream_index, time_base, info, decoder) = {
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| "输入中没有视频流".to_string())?;
        let time_base = stream.time_base();
        let time_base = if time_base.denominator() == 0 {
            0.0
        } else {
            time_base.numerator() as f64 / time_base.denominator() as f64
        };
        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|error| format!("读取视频参数失败：{error}"))?;
        context.set_flags(ffmpeg::codec::Flags::LOW_DELAY);
        context.set_threading(ffmpeg::codec::threading::Config::count(1));
        let decoder = context
            .decoder()
            .video()
            .map_err(|error| format!("视频解码器不可用：{error}"))?;

        let nominal_fps = decoder
            .frame_rate()
            .map(rational)
            .filter(|value| *value > 0.1 && *value < 1000.0)
            .or_else(|| {
                let value = rational(stream.avg_frame_rate());
                (value > 0.1 && value < 1000.0).then_some(value)
            })
            .unwrap_or(if fps > 0 { fps as f64 } else { 30.0 });

        let pixel = decoder.format();
        let pixel_format = pixel
            .descriptor()
            .map(|descriptor| descriptor.name().to_string())
            .unwrap_or_else(|| format!("{pixel:?}"));

        let info = StreamInfo {
            codec: decoder.id().name().to_uppercase(),
            width: decoder.width(),
            height: decoder.height(),
            pixel_format,
            nominal_fps,
        };
        (stream.index(), time_base, info, decoder)
    };

    log::info!(
        "流已打开：{} {}x{} {} fps={:.2}",
        info.codec,
        info.width,
        info.height,
        info.pixel_format,
        info.nominal_fps
    );

    Ok(StreamSource {
        input,
        decoder,
        stream_index,
        time_base,
        info,
    })
}

fn open_input<F: FnMut() -> bool>(
    source: &str,
    options: ffmpeg::Dictionary,
    interrupted: Box<F>,
) -> Result<ffmpeg::format::context::Input, String> {
    let path = CString::new(source).map_err(|_| "输入地址包含无效字符".to_string())?;
    unsafe {
        let mut context = ffmpeg::ffi::avformat_alloc_context();
        if context.is_null() {
            return Err("无法分配输入上下文".to_string());
        }
        (*context).interrupt_callback = ffmpeg::util::interrupt::new(interrupted).interrupt;
        let mut options = options.disown();
        let result = ffmpeg::ffi::avformat_open_input(
            &mut context,
            path.as_ptr(),
            ptr::null_mut(),
            &mut options,
        );
        ffmpeg::Dictionary::own(options);
        if result < 0 {
            return Err(format!("打开输入失败：{}", ffmpeg::Error::from(result)));
        }
        if ffmpeg::ffi::avformat_find_stream_info(context, ptr::null_mut()) < 0 {
            ffmpeg::ffi::avformat_close_input(&mut context);
            return Err("读取输入流信息失败".to_string());
        }
        Ok(ffmpeg::format::context::Input::wrap(context))
    }
}

fn rational(value: ffmpeg::Rational) -> f64 {
    if value.denominator() == 0 {
        0.0
    } else {
        value.numerator() as f64 / value.denominator() as f64
    }
}
