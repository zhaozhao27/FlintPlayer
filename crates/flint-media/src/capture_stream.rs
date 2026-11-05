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
    pub audio_tracks: Vec<String>,
}

pub struct StreamSource {
    pub input: ffmpeg::format::context::Input,
    pub decoder: ffmpeg::codec::decoder::Video,
    pub stream_index: usize,
    pub time_base: f64,
    pub info: StreamInfo,
    pub local: bool,
    pub duration: f64,
    pub frame_duration: f64,
    pub clock: Option<(std::time::Instant, f64, f32)>,
    pub audio: Option<AudioTrack>,
    pub active_audio: Option<usize>,
    pub external_audio: Option<ExternalAudio>,
}

pub struct ExternalAudio {
    pub input: ffmpeg::format::context::Input,
    pub track: AudioTrack,
    pub stream_index: usize,
    pub time_base: f64,
    pub next_pts: f64,
}

pub struct AudioTrack {
    decoder: ffmpeg::codec::decoder::Audio,
    resampler: ffmpeg::software::resampling::Context,
    sink: crate::audio::AudioSink,
    secondary: Option<crate::audio::AudioSink>,
    shifter: crate::audio::SyncShifter,
    effects: crate::audio_dsp::AudioEffects,
    pub codec: String,
    pub rate: u32,
    pub format: crate::audio::SampleFormat,
    pub device: Option<String>,
    pub device2: Option<String>,
    pub passthrough: Option<String>,
    pub method: u8,
    pub stream_index: usize,
}

impl AudioTrack {
    pub fn push(
        &mut self,
        data: &[u8],
        options: crate::audio::AudioOptions,
        shared: &Shared,
    ) -> Result<(), String> {
        let packet = ffmpeg::Packet::copy(data);
        self.decoder
            .send_packet(&packet)
            .map_err(|error| format!("音频包解码失败：{error}"))?;
        let mut frame = ffmpeg::frame::Audio::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let mut output = ffmpeg::frame::Audio::empty();
            if self.resampler.run(&frame, &mut output).is_err() {
                continue;
            }
            let expected = output.samples() * usize::from(output.channels()) * 2;
            let plane = output.data(0);
            let mut samples: Vec<i16> = plane[..expected.min(plane.len())]
                .chunks_exact(2)
                .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                .collect();
            if options.channel != crate::audio::Channel::Stereo || options.swap {
                crate::audio::apply_channel(&mut samples, options.channel, options.swap);
            }
            self.effects.process(
                &mut samples,
                shared.audio_effects(),
                &shared.audio_eq_gains(),
            );
            if options.sync_ms != 0 || !self.shifter.is_idle() {
                samples = self.shifter.apply(&samples, options.sync_ms, self.rate);
            }
            let gain = options.gain
                * if matches!(self.codec.as_str(), "AC3" | "EAC3" | "DTS" | "TRUEHD")
                    && shared.audio_ac3_dts()
                {
                    10f32.powf(shared.audio_ac3_dts_gain_db() / 20.0)
                } else {
                    1.0
                };
            let output: Vec<i16> = if gain <= 0.0 {
                vec![0i16; samples.len()]
            } else if (gain - 1.0).abs() > f32::EPSILON {
                samples
                    .iter()
                    .map(|sample| (*sample as f32 * gain).clamp(-32768.0, 32767.0) as i16)
                    .collect()
            } else {
                samples
            };
            self.sink.write(&output);
            if let Some(secondary) = &self.secondary {
                secondary.write(&output);
            }
            shared.write_audio_samples(&output);
        }
        Ok(())
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.sink.flush();
    }
}

pub fn audio_track_labels(input: &ffmpeg::format::context::Input) -> Vec<String> {
    input
        .streams()
        .filter(|stream| stream.parameters().medium() == Type::Audio)
        .enumerate()
        .map(|(index, stream)| {
            let codec = stream.parameters().id().name().to_uppercase();
            let language = stream
                .metadata()
                .get("language")
                .unwrap_or_default()
                .to_string();
            let (rate, channels) =
                ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                    .ok()
                    .and_then(|context| context.decoder().audio().ok())
                    .map(|decoder| {
                        let layout = decoder.channel_layout();
                        let channels = if layout.bits() == 0 {
                            0
                        } else {
                            layout.channels()
                        };
                        (decoder.rate(), channels)
                    })
                    .unwrap_or((0, 0));
            let mut label = format!("音轨 {}: {}", index + 1, codec);
            if rate > 0 {
                label.push_str(&format!(" {rate}Hz"));
            }
            if channels > 0 {
                label.push_str(&format!(" {channels}声道"));
            }
            if !language.is_empty() {
                label.push_str(&format!(" [{language}]"));
            }
            label
        })
        .collect()
}

fn nth_audio<'a>(
    input: &'a ffmpeg::format::context::Input,
    index: usize,
) -> Option<ffmpeg::format::stream::Stream<'a>> {
    input
        .streams()
        .filter(|stream| stream.parameters().medium() == Type::Audio)
        .nth(index)
}

pub(crate) struct AudioOutputConfig {
    pub rate: u32,
    pub format: crate::audio::SampleFormat,
    pub device: Option<String>,
    pub device2: Option<String>,
    pub passthrough: Option<String>,
    pub method: u8,
}

pub(crate) fn open_audio(
    input: &ffmpeg::format::context::Input,
    track: Option<usize>,
    output: AudioOutputConfig,
) -> Option<(AudioTrack, usize)> {
    let AudioOutputConfig {
        rate: output_rate,
        format,
        device,
        device2,
        passthrough: passthrough_device,
        method,
    } = output;
    let (stream, ordinal) = match track.and_then(|index| nth_audio(input, index)) {
        Some(stream) => (stream, track.unwrap_or(0)),
        None => {
            let best = input.streams().best(Type::Audio)?;
            let ordinal = input
                .streams()
                .filter(|stream| stream.parameters().medium() == Type::Audio)
                .position(|stream| stream.index() == best.index())
                .unwrap_or(0);
            (best, ordinal)
        }
    };
    let stream_index = stream.index();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters()).ok()?;
    let decoder = context.decoder().audio().ok()?;
    let codec = decoder.id().name().to_uppercase();
    let layout = decoder.channel_layout();
    let layout = if layout.bits() == 0 {
        ffmpeg::channel_layout::ChannelLayout::STEREO
    } else {
        layout
    };
    let rate = decoder.rate();
    if rate == 0 {
        return None;
    }
    let null_output = method == crate::AUDIO_METHOD_NULL;
    let passthrough = passthrough_device.filter(|name| !name.is_empty());
    let primary_device = if matches!(codec.as_str(), "AC3" | "EAC3" | "DTS" | "TRUEHD") {
        passthrough.clone().or_else(|| device.clone())
    } else {
        device.clone()
    };
    if primary_device != device {
        log::info!(
            "直通输出设备：{}",
            primary_device.as_deref().unwrap_or("默认")
        );
    }
    let sink = crate::audio::AudioSink::open(
        output_rate,
        2,
        format,
        primary_device.as_deref(),
        null_output,
    )?;
    let secondary = device2
        .as_deref()
        .filter(|name| device.as_deref() != Some(*name))
        .and_then(|name| {
            crate::audio::AudioSink::open(output_rate, 2, format, Some(name), null_output)
        });
    let resampler = ffmpeg::software::resampling::Context::get(
        decoder.format(),
        layout,
        rate,
        ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed),
        ffmpeg::channel_layout::ChannelLayout::STEREO,
        output_rate,
    )
    .ok()?;
    log::info!("音频流：{} {}Hz", decoder.id().name().to_uppercase(), rate);
    Some((
        AudioTrack {
            decoder,
            resampler,
            sink,
            secondary,
            shifter: crate::audio::SyncShifter::default(),
            effects: crate::audio_dsp::AudioEffects::new(output_rate),
            codec,
            rate: output_rate,
            format,
            device,
            device2,
            passthrough,
            method,
            stream_index,
        },
        ordinal,
    ))
}

pub fn is_device(source: &str) -> bool {
    source.starts_with("/dev/video")
}

pub fn screen_display(source: &str) -> Option<&str> {
    source
        .strip_prefix("screen1:")
        .or_else(|| source.strip_prefix("screen2:"))
}

pub fn is_screen(source: &str) -> bool {
    screen_display(source).is_some()
}

fn window_spec(spec: &str) -> Option<u32> {
    let value = spec.strip_prefix("window:")?;
    u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

#[cfg(have_avdevice)]
extern "C" {
    fn avdevice_register_all();
}

fn register_devices() -> bool {
    #[cfg(have_avdevice)]
    {
        unsafe { avdevice_register_all() };
        return true;
    }
    #[allow(unreachable_code)]
    false
}

pub fn open(
    source: &str,
    settings: &crate::CaptureSettings,
    shared: Arc<Shared>,
) -> Result<StreamSource, String> {
    let width = settings.width;
    let height = settings.height;
    let fps = settings.fps;
    ffmpeg::init().map_err(|error| format!("FFmpeg 初始化失败：{error}"))?;

    let screen_spec = screen_display(source).map(str::to_string);
    let window_id = screen_spec.as_deref().and_then(window_spec);
    let screen = screen_spec.as_ref().map(|spec| match window_spec(spec) {
        Some(_) => std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()),
        None => spec.clone(),
    });
    if screen.is_some() && !register_devices() {
        return Err("系统缺少 libavdevice，无法进行屏幕采集".to_string());
    }
    let input_name = screen.as_deref().unwrap_or(source);
    let forced_format = screen.is_some().then_some("x11grab");
    let input_format = forced_format.or(settings.input_format.as_deref());

    let local = !source.contains("://") && screen.is_none();
    let mut options = ffmpeg::Dictionary::new();
    if !local {
        options.set("fflags", "nobuffer");
        options.set("flags", "low_delay");
        options.set("probesize", "32768");
        options.set("analyzeduration", "0");
        options.set("max_delay", "500000");
        options.set("reorder_queue_size", "0");
    }
    if let Some(window) = window_id {
        options.set("window_id", &window.to_string());
    } else if width > 0 && height > 0 {
        options.set("video_size", &format!("{width}x{height}"));
    } else if screen.is_some() {
        options.set("video_size", "1280x720");
    }
    if fps > 0 {
        options.set("framerate", &fps.to_string());
    } else if screen.is_some() {
        options.set("framerate", "30");
    }
    if let Some(pixel_format) = settings.pixel_format.as_deref() {
        options.set("pix_fmt", pixel_format);
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

    let interrupt_shared = shared.clone();
    let input = open_input(
        input_name,
        input_format,
        options,
        Box::new(move || interrupt_shared.should_stop()),
    )?;
    shared.set_chapters(collect_chapters(&input));
    let duration = {
        let micros = input.duration();
        if micros > 0 {
            micros as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
        } else {
            0.0
        }
    };

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
        if !local {
            context.set_flags(ffmpeg::codec::Flags::LOW_DELAY);
            context.set_threading(ffmpeg::codec::threading::Config::count(1));
        }
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
            audio_tracks: audio_track_labels(&input),
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

    let (audio, active_audio) = match open_audio(
        &input,
        shared.audio_track(),
        AudioOutputConfig {
            rate: shared.audio_output_rate(),
            format: shared.audio_output_format(),
            device: shared.audio_device(),
            device2: shared.audio_device2(),
            passthrough: shared.audio_passthrough_device(),
            method: shared.audio_output_method(),
        },
    ) {
        Some((track, ordinal)) => {
            shared.set_audio_track(Some(ordinal));
            (Some(track), Some(ordinal))
        }
        None => (None, None),
    };

    Ok(StreamSource {
        input,
        decoder,
        stream_index,
        time_base,
        local: !source.contains("://"),
        duration,
        frame_duration: 1.0 / info.nominal_fps.max(0.1),
        clock: None,
        audio,
        active_audio,
        external_audio: None,
        info,
    })
}

fn collect_chapters(input: &ffmpeg::format::context::Input) -> Vec<crate::ChapterInfo> {
    let mut chapters: Vec<crate::ChapterInfo> = input
        .chapters()
        .enumerate()
        .map(|(index, chapter)| {
            let time_base = chapter.time_base();
            let scale = if time_base.denominator() == 0 {
                0.0
            } else {
                f64::from(time_base.numerator()) / f64::from(time_base.denominator())
            };
            let title = chapter
                .metadata()
                .get("title")
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("章节 {}", index + 1));
            crate::ChapterInfo {
                title,
                start: chapter.start() as f64 * scale,
                end: chapter.end() as f64 * scale,
            }
        })
        .filter(|chapter| chapter.end > chapter.start && chapter.start >= 0.0)
        .collect();
    chapters.sort_by(|left, right| left.start.total_cmp(&right.start));
    chapters
}

fn open_input<F: FnMut() -> bool>(
    source: &str,
    input_format: Option<&str>,
    options: ffmpeg::Dictionary,
    interrupted: Box<F>,
) -> Result<ffmpeg::format::context::Input, String> {
    let path = CString::new(source).map_err(|_| "输入地址包含无效字符".to_string())?;
    let format_name = match input_format {
        Some(name) => {
            let name = CString::new(name).map_err(|_| "输入格式名包含无效字符".to_string())?;
            let format = unsafe { ffmpeg::ffi::av_find_input_format(name.as_ptr()) };
            if format.is_null() {
                return Err(format!(
                    "FFmpeg 不支持输入格式 {}",
                    input_format.unwrap_or("")
                ));
            }
            format
        }
        None => ptr::null(),
    };
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
            format_name,
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
