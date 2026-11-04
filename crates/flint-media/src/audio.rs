use libc::{c_char, c_int, c_void, size_t};
use std::ffi::CString;
use std::ptr;
use std::sync::OnceLock;

const PA_SAMPLE_S16LE: c_int = 3;
const PA_SAMPLE_FLOAT32LE: c_int = 5;
const PA_SAMPLE_S24LE: c_int = 9;
const PA_STREAM_PLAYBACK: c_int = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    S16,
    S24,
    F32,
}

impl SampleFormat {
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::S24,
            2 => Self::F32,
            _ => Self::S16,
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Self::S16 => 0,
            Self::S24 => 1,
            Self::F32 => 2,
        }
    }

    fn pulse_code(self) -> c_int {
        match self {
            Self::S16 => PA_SAMPLE_S16LE,
            Self::S24 => PA_SAMPLE_S24LE,
            Self::F32 => PA_SAMPLE_FLOAT32LE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stereo,
    Left,
    Right,
}

impl Channel {
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Left,
            2 => Self::Right,
            _ => Self::Stereo,
        }
    }

    pub fn code(self) -> u32 {
        match self {
            Self::Stereo => 0,
            Self::Left => 1,
            Self::Right => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioOptions {
    pub gain: f32,
    pub channel: Channel,
    pub swap: bool,
    pub sync_ms: i32,
}

impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            gain: 1.0,
            channel: Channel::Stereo,
            swap: false,
            sync_ms: 0,
        }
    }
}

impl AudioOptions {
    pub fn is_plain(self) -> bool {
        (self.gain - 1.0).abs() < f32::EPSILON
            && self.channel == Channel::Stereo
            && !self.swap
            && self.sync_ms == 0
    }
}

pub fn apply_channel(samples: &mut [i16], channel: Channel, swap: bool) {
    for frame in samples.chunks_exact_mut(2) {
        let (mut left, mut right) = match channel {
            Channel::Stereo => (frame[0], frame[1]),
            Channel::Left => (frame[0], frame[0]),
            Channel::Right => (frame[1], frame[1]),
        };
        if swap {
            std::mem::swap(&mut left, &mut right);
        }
        frame[0] = left;
        frame[1] = right;
    }
}

fn encode_samples(samples: &[i16], format: SampleFormat) -> Vec<u8> {
    match format {
        SampleFormat::S16 => samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect(),
        SampleFormat::S24 => samples
            .iter()
            .flat_map(|sample| {
                let bytes = (i32::from(*sample) << 8).to_le_bytes();
                [bytes[0], bytes[1], bytes[2]]
            })
            .collect(),
        SampleFormat::F32 => samples
            .iter()
            .flat_map(|sample| (*sample as f32 / 32768.0).to_le_bytes())
            .collect(),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SyncShifter {
    applied_samples: i64,
}

impl SyncShifter {
    pub fn reset(&mut self) {
        self.applied_samples = 0;
    }

    pub fn is_idle(&self) -> bool {
        self.applied_samples == 0
    }

    pub fn applied_ms(&self, rate: u32) -> i32 {
        if rate == 0 {
            return 0;
        }
        (self.applied_samples * 1000 / i64::from(rate)) as i32
    }

    pub fn apply(&mut self, samples: &[i16], sync_ms: i32, rate: u32) -> Vec<i16> {
        if rate == 0 || samples.len() < 2 {
            return samples.to_vec();
        }
        let target = i64::from(sync_ms) * i64::from(rate) / 1000;
        let frames = samples.len() / 2;
        if target > self.applied_samples {
            let silence_frames = (target - self.applied_samples) as usize;
            let mut out = vec![0i16; silence_frames * 2];
            out.extend_from_slice(samples);
            self.applied_samples = target;
            out
        } else if target < self.applied_samples {
            let requested = (self.applied_samples - target) as usize;
            let dropped = requested.min(frames);
            self.applied_samples -= dropped as i64;
            samples[dropped * 2..].to_vec()
        } else {
            samples.to_vec()
        }
    }
}

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

type SimpleNew = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const SampleSpec,
    *const c_void,
    *const c_void,
    *mut c_int,
) -> *mut c_void;
type SimpleWrite = unsafe extern "C" fn(*mut c_void, *const c_void, size_t, *mut c_int) -> c_int;
type SimpleFlush = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
type SimpleFree = unsafe extern "C" fn(*mut c_void);

struct Api {
    new: SimpleNew,
    write: SimpleWrite,
    flush: SimpleFlush,
    free: SimpleFree,
}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let library = CString::new("libpulse-simple.so.0").ok()?;
        let handle = libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            log::info!("未找到 libpulse-simple，音频输出不可用");
            return None;
        }
        let symbol = |name: &str| -> Option<*mut c_void> {
            let name = CString::new(name).ok()?;
            let address = libc::dlsym(handle, name.as_ptr());
            (!address.is_null()).then_some(address)
        };
        Some(Api {
            new: std::mem::transmute::<*mut c_void, SimpleNew>(symbol("pa_simple_new")?),
            write: std::mem::transmute::<*mut c_void, SimpleWrite>(symbol("pa_simple_write")?),
            flush: std::mem::transmute::<*mut c_void, SimpleFlush>(symbol("pa_simple_flush")?),
            free: std::mem::transmute::<*mut c_void, SimpleFree>(symbol("pa_simple_free")?),
        })
    })
    .as_ref()
}

pub struct AudioSink {
    stream: *mut c_void,
    format: SampleFormat,
    null: bool,
}

unsafe impl Send for AudioSink {}

impl AudioSink {
    pub fn open(
        rate: u32,
        channels: u8,
        format: SampleFormat,
        device: Option<&str>,
        null_output: bool,
    ) -> Option<Self> {
        if null_output {
            return Some(Self {
                stream: ptr::null_mut(),
                format,
                null: true,
            });
        }
        let api = api()?;
        let spec = SampleSpec {
            format: format.pulse_code(),
            rate,
            channels,
        };
        let application = CString::new("FlintPlayer").ok()?;
        let stream_name = CString::new("Playback").ok()?;
        let device = device
            .filter(|name| !name.is_empty())
            .and_then(|name| CString::new(name).ok());
        let mut error: c_int = 0;
        let stream = unsafe {
            (api.new)(
                ptr::null(),
                application.as_ptr(),
                PA_STREAM_PLAYBACK,
                device
                    .as_ref()
                    .map_or(ptr::null(), |device| device.as_ptr()),
                stream_name.as_ptr(),
                &spec,
                ptr::null(),
                ptr::null(),
                &mut error,
            )
        };
        if stream.is_null() {
            log::info!("PulseAudio 输出打开失败（错误码 {error}），音频输出不可用");
            return None;
        }
        Some(Self {
            stream,
            format,
            null: false,
        })
    }

    pub fn write(&self, samples: &[i16]) {
        if samples.is_empty() || self.null {
            return;
        }
        let Some(api) = api() else {
            return;
        };
        let encoded = encode_samples(samples, self.format);
        let mut error: c_int = 0;
        unsafe {
            (api.write)(
                self.stream,
                encoded.as_ptr().cast::<c_void>(),
                encoded.len(),
                &mut error,
            );
        }
    }

    pub fn flush(&self) {
        if self.null {
            return;
        }
        let Some(api) = api() else {
            return;
        };
        let mut error: c_int = 0;
        unsafe {
            (api.flush)(self.stream, &mut error);
        }
    }
}

impl Drop for AudioSink {
    fn drop(&mut self) {
        if self.null || self.stream.is_null() {
            return;
        }
        if let Some(api) = api() {
            unsafe {
                (api.free)(self.stream);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_samples_for_pulse_formats() {
        assert_eq!(
            encode_samples(&[1, -1], SampleFormat::S16),
            vec![1, 0, 255, 255]
        );
        assert_eq!(
            encode_samples(&[1, -1], SampleFormat::S24),
            vec![0, 1, 0, 0, 255, 255]
        );
        let float = encode_samples(&[16384], SampleFormat::F32);
        assert_eq!(
            f32::from_le_bytes([float[0], float[1], float[2], float[3]]),
            0.5
        );
    }

    #[test]
    fn left_channel_duplicates_to_both_sides() {
        let mut samples = vec![1000, 2000, 3000, 4000];
        apply_channel(&mut samples, Channel::Left, false);
        assert_eq!(samples, vec![1000, 1000, 3000, 3000]);
    }

    #[test]
    fn right_channel_duplicates_to_both_sides() {
        let mut samples = vec![1000, 2000];
        apply_channel(&mut samples, Channel::Right, false);
        assert_eq!(samples, vec![2000, 2000]);
    }

    #[test]
    fn swap_exchanges_stereo_pairs() {
        let mut samples = vec![1, 2, 3, 4];
        apply_channel(&mut samples, Channel::Stereo, true);
        assert_eq!(samples, vec![2, 1, 4, 3]);
    }

    #[test]
    fn positive_sync_prepends_silence_once() {
        let mut shifter = SyncShifter::default();
        let first = shifter.apply(&[5, 6], 100, 1000);
        assert_eq!(first.len(), 2 + 100 * 2);
        assert!(first[..200].iter().all(|sample| *sample == 0));
        assert_eq!(&first[200..], &[5, 6]);
        let second = shifter.apply(&[7, 8], 100, 1000);
        assert_eq!(second, vec![7, 8]);
    }

    #[test]
    fn negative_sync_drops_frames() {
        let mut shifter = SyncShifter::default();
        let samples: Vec<i16> = (0..100).collect();
        let out = shifter.apply(&samples, -10, 1000);
        assert_eq!(out.len(), 100 - 20);
        assert_eq!(out[0], 20);
    }

    #[test]
    fn sync_returns_to_plain_when_reset() {
        let mut shifter = SyncShifter::default();
        let _ = shifter.apply(&[1i16; 200], 50, 1000);
        let out = shifter.apply(&[7i16; 200], 0, 1000);
        assert_eq!(out, vec![7i16; 100]);
        let next = shifter.apply(&[3, 4], 0, 1000);
        assert_eq!(next, vec![3, 4]);
    }
}
