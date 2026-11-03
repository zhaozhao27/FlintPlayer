use flint_core::device;
use std::ffi::CString;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("FlintPlayer 的 V4L2 结构布局仅适配 64 位目标");

const VIDIOC_QUERYCAP: libc::c_ulong = 0x8068_5600;
const VIDIOC_S_FMT: libc::c_ulong = 0xc0d0_5605;
const VIDIOC_G_FMT: libc::c_ulong = 0xc0d0_5604;
const VIDIOC_REQBUFS: libc::c_ulong = 0xc014_5608;
const VIDIOC_QUERYBUF: libc::c_ulong = 0xc058_5609;
const VIDIOC_QBUF: libc::c_ulong = 0xc058_560f;
const VIDIOC_DQBUF: libc::c_ulong = 0xc058_5611;
const VIDIOC_STREAMON: libc::c_ulong = 0x4004_5612;
const VIDIOC_STREAMOFF: libc::c_ulong = 0x4004_5613;
const VIDIOC_G_PARM: libc::c_ulong = 0xc0cc_5615;
const VIDIOC_S_PARM: libc::c_ulong = 0xc0cc_5616;

const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_MEMORY_MMAP: u32 = 1;
const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;
const BUFFER_COUNT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawFormat {
    Yuyv422,
    Uyvy422,
    Nv12,
    Rgb24,
    Bgr24,
    Gray8,
}

impl RawFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Yuyv422 => "YUYV 4:2:2",
            Self::Uyvy422 => "UYVY 4:2:2",
            Self::Nv12 => "NV12",
            Self::Rgb24 => "RGB24",
            Self::Bgr24 => "BGR24",
            Self::Gray8 => "GRAY8",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureCodec {
    Mjpeg,
    H264,
    Hevc,
    Raw(RawFormat),
}

impl CaptureCodec {
    pub fn classify(fourcc: &str) -> Option<Self> {
        match fourcc {
            "MJPG" | "JPEG" | "jpeg" => Some(Self::Mjpeg),
            "H264" | "AVC1" => Some(Self::H264),
            "HEVC" | "H265" => Some(Self::Hevc),
            "YUYV" | "YUY2" => Some(Self::Raw(RawFormat::Yuyv422)),
            "UYVY" => Some(Self::Raw(RawFormat::Uyvy422)),
            "NV12" => Some(Self::Raw(RawFormat::Nv12)),
            "RGB3" => Some(Self::Raw(RawFormat::Rgb24)),
            "BGR3" => Some(Self::Raw(RawFormat::Bgr24)),
            "GREY" => Some(Self::Raw(RawFormat::Gray8)),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mjpeg => "MJPEG",
            Self::H264 => "H.264",
            Self::Hevc => "H.265",
            Self::Raw(raw) => raw.label(),
        }
    }

    pub fn is_compressed(self) -> bool {
        !matches!(self, Self::Raw(_))
    }

    fn rank(self) -> u8 {
        match self {
            Self::Mjpeg => 0,
            Self::H264 => 1,
            Self::Hevc => 2,
            Self::Raw(RawFormat::Yuyv422) => 3,
            Self::Raw(RawFormat::Nv12) => 4,
            Self::Raw(RawFormat::Uyvy422) => 5,
            Self::Raw(RawFormat::Rgb24) => 6,
            Self::Raw(RawFormat::Bgr24) => 7,
            Self::Raw(RawFormat::Gray8) => 8,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Format {
    type_: u32,
    _pad: u32,
    pix: PixFormat,
    _tail: [u8; 152],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StreamParm {
    type_: u32,
    capability: u32,
    capturemode: u32,
    numerator: u32,
    denominator: u32,
    extendedmode: u32,
    readbuffers: u32,
    reserved: [u32; 4],
    _tail: [u8; 160],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RequestBuffers {
    count: u32,
    type_: u32,
    memory: u32,
    capabilities: u32,
    reserved: [u32; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Timeval {
    seconds: i64,
    microseconds: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Buffer {
    index: u32,
    type_: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    _pad: u32,
    timestamp: Timeval,
    timecode: [u8; 16],
    sequence: u32,
    memory: u32,
    m: u64,
    length: u32,
    reserved2: u32,
    request_fd: i32,
    _end_pad: u32,
}

impl Default for Format {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl Default for StreamParm {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

struct Fd(RawFd);

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

struct Mapping {
    ptr: *mut u8,
    length: usize,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.cast(), self.length);
        }
    }
}

pub struct Dequeued {
    pub index: usize,
    pub bytes_used: usize,
    pub timestamp: f64,
}

pub struct V4l2Stream {
    fd: Fd,
    mappings: Vec<Mapping>,
    codec: CaptureCodec,
    fourcc: String,
    width: u32,
    height: u32,
    stride: usize,
    fps: f64,
    streaming: bool,
    path: PathBuf,
}

fn ioctl<T>(fd: RawFd, request: libc::c_ulong, value: &mut T) -> Result<(), String> {
    let rc = unsafe { libc::ioctl(fd, request, value as *mut T) };
    if rc < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn fourcc_value(text: &str) -> u32 {
    let mut bytes = [0u8; 4];
    for (index, byte) in text.bytes().take(4).enumerate() {
        bytes[index] = byte;
    }
    u32::from_le_bytes(bytes)
}

fn fourcc_text(value: u32) -> String {
    String::from_utf8_lossy(&value.to_le_bytes()).to_string()
}

impl V4l2Stream {
    pub fn open(
        path: &Path,
        preferred: Option<&str>,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self, String> {
        let info = device::query_device(path)?;
        let (fourcc, _) = choose_format(&info, preferred)?;

        let name = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| format!("设备路径无效：{}", path.display()))?;
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(format!("无法打开 {}：{}", path.display(), std::io::Error::last_os_error()));
        }
        let fd = Fd(fd);

        let mut capability: Capability = unsafe { std::mem::zeroed() };
        ioctl(fd.0, VIDIOC_QUERYCAP, &mut capability)
            .map_err(|error| format!("设备能力查询失败：{error}"))?;
        let caps = if capability.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
            capability.device_caps
        } else {
            capability.capabilities
        };
        if caps & V4L2_CAP_VIDEO_CAPTURE == 0 {
            return Err("设备不支持单平面视频采集".into());
        }

        let supported = device::resolutions(path, &fourcc);
        let (width, height) = choose_size(&supported, width, height);
        let mut format = Format {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            ..Format::default()
        };
        format.pix.width = width;
        format.pix.height = height;
        format.pix.pixelformat = fourcc_value(&fourcc);
        format.pix.field = 0;
        format.pix.bytesperline = 0;
        format.pix.sizeimage = 0;
        ioctl(fd.0, VIDIOC_S_FMT, &mut format)
            .map_err(|error| format!("设置采集格式失败：{error}"))?;
        ioctl(fd.0, VIDIOC_G_FMT, &mut format)
            .map_err(|error| format!("读取采集格式失败：{error}"))?;

        let actual_fourcc = fourcc_text(format.pix.pixelformat);
        let codec = CaptureCodec::classify(&actual_fourcc)
            .ok_or_else(|| format!("驱动返回了不支持的像素格式 {actual_fourcc}"))?;
        let width = format.pix.width;
        let height = format.pix.height;
        if width == 0 || height == 0 {
            return Err("驱动返回了无效的画面尺寸".into());
        }
        let stride = if format.pix.bytesperline > 0 {
            format.pix.bytesperline as usize
        } else {
            default_stride(codec, width)
        };

        if fps > 0 {
            let mut parm = StreamParm {
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                numerator: 1,
                denominator: fps,
                ..StreamParm::default()
            };
            if let Err(error) = ioctl(fd.0, VIDIOC_S_PARM, &mut parm) {
                log::debug!("设定帧率失败，沿用驱动默认值：{error}");
            }
        }
        let mut negotiated_fps = if fps > 0 { fps as f64 } else { 30.0 };
        let mut parm = StreamParm {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            ..StreamParm::default()
        };
        if ioctl(fd.0, VIDIOC_G_PARM, &mut parm).is_ok()
            && parm.numerator > 0
            && parm.denominator > 0
        {
            negotiated_fps = parm.denominator as f64 / parm.numerator as f64;
        }

        let mut request = RequestBuffers {
            count: BUFFER_COUNT,
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            memory: V4L2_MEMORY_MMAP,
            capabilities: 0,
            reserved: [0],
        };
        ioctl(fd.0, VIDIOC_REQBUFS, &mut request)
            .map_err(|error| format!("申请采集缓冲区失败：{error}"))?;
        if request.count == 0 {
            return Err("驱动没有提供采集缓冲区".into());
        }

        let mut mappings = Vec::new();
        for index in 0..request.count {
            let mut buffer = Buffer {
                index,
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                memory: V4L2_MEMORY_MMAP,
                ..Buffer::default()
            };
            ioctl(fd.0, VIDIOC_QUERYBUF, &mut buffer)
                .map_err(|error| format!("查询采集缓冲区失败：{error}"))?;
            let length = buffer.length as usize;
            if length == 0 {
                return Err("采集缓冲区长度为 0".into());
            }
            let pointer = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    length,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd.0,
                    (buffer.m & 0xffff_ffff) as i64,
                )
            };
            if pointer == libc::MAP_FAILED {
                return Err(format!("映射采集缓冲区失败：{}", std::io::Error::last_os_error()));
            }
            mappings.push(Mapping {
                ptr: pointer.cast(),
                length,
            });
            queue_buffer(fd.0, index)?;
        }

        let mut stream = Self {
            fd,
            mappings,
            codec,
            fourcc: actual_fourcc,
            width,
            height,
            stride,
            fps: negotiated_fps,
            streaming: false,
            path: path.to_path_buf(),
        };

        let mut type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        ioctl(stream.fd.0, VIDIOC_STREAMON, &mut type_)
            .map_err(|error| format!("启动视频流失败：{error}"))?;
        stream.streaming = true;
        log::info!(
            "V4L2 开始采集 {}：{} {}x{} stride={} fps={}",
            stream.path.display(),
            stream.codec.label(),
            width,
            height,
            stride,
            stream.fps
        );
        Ok(stream)
    }

    pub fn codec(&self) -> CaptureCodec {
        self.codec
    }

    pub fn fourcc(&self) -> &str {
        &self.fourcc
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }

    pub fn dequeue(&mut self, timeout: Duration) -> Result<Option<Dequeued>, String> {
        let mut poll_fd = libc::pollfd {
            fd: self.fd.0,
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            let rc = unsafe { libc::poll(&mut poll_fd, 1, timeout.as_millis() as libc::c_int) };
            if rc < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("等待视频帧失败：{error}"));
            }
            if rc == 0 {
                return Ok(None);
            }
            if poll_fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err("设备断开".into());
            }
            break;
        }

        let mut buffer = Buffer {
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            memory: V4L2_MEMORY_MMAP,
            ..Buffer::default()
        };
        let rc = loop {
            let rc = unsafe { libc::ioctl(self.fd.0, VIDIOC_DQBUF, &mut buffer as *mut Buffer) };
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break rc;
        };
        if rc < 0 {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::EAGAIN) => Ok(None),
                Some(libc::ENODEV) | Some(libc::EIO) => Err("设备断开".into()),
                _ => Err(format!("取出视频帧失败：{error}")),
            };
        }
        let index = buffer.index as usize;
        if index >= self.mappings.len() {
            return Err("驱动返回了越界的缓冲区索引".into());
        }
        let length = self.mappings[index].length;
        let bytes_used = if buffer.bytesused == 0 {
            length
        } else {
            (buffer.bytesused as usize).min(length)
        };
        Ok(Some(Dequeued {
            index,
            bytes_used,
            timestamp: buffer.timestamp.seconds as f64
                + buffer.timestamp.microseconds as f64 / 1_000_000.0,
        }))
    }

    pub fn bytes(&self, index: usize) -> &[u8] {
        let mapping = &self.mappings[index];
        unsafe { std::slice::from_raw_parts(mapping.ptr, mapping.length) }
    }

    pub fn requeue(&mut self, index: usize) -> Result<(), String> {
        queue_buffer(self.fd.0, index as u32)
    }
}

impl Drop for V4l2Stream {
    fn drop(&mut self) {
        if self.streaming {
            let mut type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            unsafe {
                libc::ioctl(self.fd.0, VIDIOC_STREAMOFF, &mut type_);
            }
            self.streaming = false;
        }
        self.mappings.clear();
    }
}

fn queue_buffer(fd: RawFd, index: u32) -> Result<(), String> {
    let mut buffer = Buffer {
        index,
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
        memory: V4L2_MEMORY_MMAP,
        ..Buffer::default()
    };
    ioctl(fd, VIDIOC_QBUF, &mut buffer).map_err(|error| format!("归还采集缓冲区失败：{error}"))
}

fn default_stride(codec: CaptureCodec, width: u32) -> usize {
    let width = width as usize;
    match codec {
        CaptureCodec::Raw(RawFormat::Yuyv422) | CaptureCodec::Raw(RawFormat::Uyvy422) => width * 2,
        CaptureCodec::Raw(RawFormat::Rgb24) | CaptureCodec::Raw(RawFormat::Bgr24) => width * 3,
        CaptureCodec::Raw(RawFormat::Nv12) | CaptureCodec::Raw(RawFormat::Gray8) => width,
        _ => width * 2,
    }
}

fn choose_format(
    info: &device::DeviceInfo,
    preferred: Option<&str>,
) -> Result<(String, CaptureCodec), String> {
    if let Some(preferred) = preferred {
        if let Some(codec) = CaptureCodec::classify(preferred) {
            if info.formats.iter().any(|format| format.fourcc == preferred) {
                return Ok((preferred.to_string(), codec));
            }
        }
    }
    info.formats
        .iter()
        .filter_map(|format| {
            CaptureCodec::classify(&format.fourcc)
                .map(|codec| (format.fourcc.clone(), codec))
        })
        .min_by_key(|(_, codec)| codec.rank())
        .ok_or_else(|| format!("{} 没有可用的视频格式", info.path.display()))
}

fn choose_size(supported: &[device::Resolution], width: u32, height: u32) -> (u32, u32) {
    if supported.is_empty() {
        return if width > 0 && height > 0 {
            (width, height)
        } else {
            (1280, 720)
        };
    }
    let target_w = if width > 0 { width } else { 1280 };
    let target_h = if height > 0 { height } else { 720 };
    supported
        .iter()
        .min_by_key(|resolution| {
            resolution.width.abs_diff(target_w) as u64 * 2
                + resolution.height.abs_diff(target_h) as u64
        })
        .map(|resolution| (resolution.width, resolution.height))
        .unwrap_or((target_w, target_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_fourcc() {
        assert_eq!(CaptureCodec::classify("MJPG"), Some(CaptureCodec::Mjpeg));
        assert_eq!(
            CaptureCodec::classify("YUYV"),
            Some(CaptureCodec::Raw(RawFormat::Yuyv422))
        );
        assert_eq!(CaptureCodec::classify("NV12"), Some(CaptureCodec::Raw(RawFormat::Nv12)));
        assert!(CaptureCodec::classify("HEVC").unwrap().is_compressed());
        assert!(CaptureCodec::classify("XXXX").is_none());
    }

    #[test]
    fn ioctl_layouts_match_kernel() {
        assert_eq!(std::mem::size_of::<Format>(), 208);
        assert_eq!(std::mem::size_of::<StreamParm>(), 204);
        assert_eq!(std::mem::size_of::<RequestBuffers>(), 20);
        assert_eq!(std::mem::size_of::<Buffer>(), 88);
        assert_eq!(std::mem::size_of::<Capability>(), 104);
    }

    #[test]
    fn picks_nearest_resolution() {
        let sizes = vec![
            device::Resolution { width: 640, height: 480 },
            device::Resolution { width: 1280, height: 720 },
            device::Resolution { width: 1920, height: 1080 },
        ];
        assert_eq!(choose_size(&sizes, 1300, 700), (1280, 720));
        assert_eq!(choose_size(&sizes, 0, 0), (1280, 720));
    }
}
