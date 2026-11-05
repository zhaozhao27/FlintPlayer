use std::ffi::CString;
use std::fs;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};

const VIDIOC_QUERYCAP: libc::c_ulong = 0x8068_5600;
const VIDIOC_ENUM_FMT: libc::c_ulong = 0xc040_5602;
const VIDIOC_ENUM_FRAMESIZES: libc::c_ulong = 0xc02c_564a;
const VIDIOC_ENUM_FRAMEINTERVALS: libc::c_ulong = 0xc034_564b;

const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;
const V4L2_FMT_FLAG_COMPRESSED: u32 = 0x0000_0004;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// A format/resolution/frame-rate combination exposed by a capture device.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureMode {
    pub fourcc: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

#[derive(Clone, Debug)]
pub struct FormatInfo {
    pub fourcc: String,
    pub description: String,
    pub compressed: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub name: String,
    pub driver: String,
    pub bus: String,
    pub formats: Vec<FormatInfo>,
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
#[derive(Clone, Copy)]
struct FmtDesc {
    index: u32,
    type_: u32,
    flags: u32,
    description: [u8; 32],
    pixelformat: u32,
    mbus_code: u32,
    reserved: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrmSizeUnion {
    data: [u32; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrmSizeEnum {
    index: u32,
    pixel_format: u32,
    type_: u32,
    size: FrmSizeUnion,
    reserved: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrmIvalUnion {
    data: [u32; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrmIvalEnum {
    index: u32,
    pixel_format: u32,
    width: u32,
    height: u32,
    type_: u32,
    interval: FrmIvalUnion,
    reserved: [u32; 2],
}

struct Fd(RawFd);

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

fn open_device(path: &Path) -> Result<Fd, String> {
    let name = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| format!("设备路径无效：{}", path.display()))?;
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "无法打开 {}：{}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(Fd(fd))
}

fn ioctl<T>(fd: RawFd, request: libc::c_ulong, value: &mut T) -> Result<(), String> {
    let rc = unsafe { libc::ioctl(fd, request, value as *mut T) };
    if rc < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

pub fn fourcc_string(value: u32) -> String {
    String::from_utf8_lossy(&value.to_le_bytes()).to_string()
}

pub fn query_device(path: &Path) -> Result<DeviceInfo, String> {
    let fd = open_device(path)?;
    let mut capability: Capability = unsafe { std::mem::zeroed() };
    ioctl(fd.0, VIDIOC_QUERYCAP, &mut capability)
        .map_err(|error| format!("{} 能力查询失败：{error}", path.display()))?;

    let caps = if capability.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
        capability.device_caps
    } else {
        capability.capabilities
    };
    if caps & V4L2_CAP_VIDEO_CAPTURE == 0 {
        return Err(format!("{} 不是视频采集节点", path.display()));
    }

    let mut formats = Vec::new();
    for index in 0..32u32 {
        let mut desc = FmtDesc {
            index,
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
            flags: 0,
            description: [0; 32],
            pixelformat: 0,
            mbus_code: 0,
            reserved: [0; 3],
        };
        if ioctl(fd.0, VIDIOC_ENUM_FMT, &mut desc).is_err() {
            break;
        }
        formats.push(FormatInfo {
            fourcc: fourcc_string(desc.pixelformat),
            description: bytes_to_string(&desc.description),
            compressed: desc.flags & V4L2_FMT_FLAG_COMPRESSED != 0,
        });
    }

    let card = bytes_to_string(&capability.card);
    let name = if card.is_empty() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    } else {
        card
    };

    Ok(DeviceInfo {
        path: path.to_path_buf(),
        name,
        driver: bytes_to_string(&capability.driver),
        bus: bytes_to_string(&capability.bus_info),
        formats,
    })
}

pub fn scan_devices() -> Vec<DeviceInfo> {
    let Ok(entries) = fs::read_dir("/dev") else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.strip_prefix("video")
                        .map(|digits| {
                            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .collect();
    paths.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name[5..].parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });

    paths
        .iter()
        .filter_map(|path| query_device(path).ok())
        .collect()
}

pub fn resolutions(path: &Path, fourcc: &str) -> Vec<Resolution> {
    let Ok(fd) = open_device(path) else {
        return Vec::new();
    };
    resolutions_for(fd.0, pixel_format_value(fourcc))
}

pub fn modes(path: &Path, fourcc: &str) -> Vec<CaptureMode> {
    let Ok(fd) = open_device(path) else {
        return Vec::new();
    };
    let pixel_format = pixel_format_value(fourcc);
    let mut result = Vec::new();
    for resolution in resolutions_for(fd.0, pixel_format) {
        let rates = frame_intervals(fd.0, pixel_format, resolution.width, resolution.height);
        let rates = if rates.is_empty() {
            vec![30.0]
        } else {
            rates
        };
        for fps in rates {
            result.push(CaptureMode {
                fourcc: fourcc.to_string(),
                width: resolution.width,
                height: resolution.height,
                fps,
            });
        }
    }
    result
}

fn pixel_format_value(fourcc: &str) -> u32 {
    fourcc
        .as_bytes()
        .iter()
        .copied()
        .enumerate()
        .fold(0u32, |acc, (index, byte)| {
            acc | (byte as u32) << (index * 8)
        })
}

fn resolutions_for(fd: RawFd, pixel_format: u32) -> Vec<Resolution> {
    let mut result = Vec::new();
    for index in 0..64u32 {
        let mut size = FrmSizeEnum {
            index,
            pixel_format,
            type_: 0,
            size: FrmSizeUnion { data: [0; 6] },
            reserved: [0; 2],
        };
        if ioctl(fd, VIDIOC_ENUM_FRAMESIZES, &mut size).is_err() {
            break;
        }
        let values = size.size.data;
        match size.type_ {
            1 => result.push(Resolution {
                width: values[0],
                height: values[1],
            }),
            2 | 3 => {
                let (min_width, max_width, min_height, max_height) =
                    (values[0], values[1], values[3], values[4]);
                for (width, height) in [(640, 480), (1280, 720), (1920, 1080), (3840, 2160)] {
                    if width >= min_width
                        && width <= max_width
                        && height >= min_height
                        && height <= max_height
                    {
                        result.push(Resolution { width, height });
                    }
                }
                result.push(Resolution {
                    width: max_width,
                    height: max_height,
                });
            }
            _ => break,
        }
    }
    result.retain(|resolution| resolution.width > 0 && resolution.height > 0);
    result.sort_by(|left, right| {
        (right.width * right.height).cmp(&(left.width * left.height))
    });
    result.dedup();
    result
}

fn frame_intervals(fd: RawFd, pixel_format: u32, width: u32, height: u32) -> Vec<f64> {
    const STANDARD_FPS: [f64; 9] = [60.0, 50.0, 30.0, 25.0, 24.0, 20.0, 15.0, 10.0, 5.0];

    let mut rates = Vec::new();
    for index in 0..64u32 {
        let mut interval = FrmIvalEnum {
            index,
            pixel_format,
            width,
            height,
            type_: 0,
            interval: FrmIvalUnion { data: [0; 6] },
            reserved: [0; 2],
        };
        if ioctl(fd, VIDIOC_ENUM_FRAMEINTERVALS, &mut interval).is_err() {
            break;
        }
        let values = interval.interval.data;
        match interval.type_ {
            1 => {
                let seconds = fraction_seconds(values[0], values[1]);
                if seconds > 0.0 {
                    rates.push(1.0 / seconds);
                }
            }
            2 => {
                let min_seconds = fraction_seconds(values[0], values[1]);
                let max_seconds = fraction_seconds(values[2], values[3]);
                if min_seconds > 0.0 && max_seconds > 0.0 {
                    let highest = 1.0 / min_seconds;
                    let lowest = 1.0 / max_seconds;
                    rates.extend(
                        STANDARD_FPS
                            .iter()
                            .copied()
                            .filter(|fps| *fps <= highest * 1.01 && *fps >= lowest * 0.99),
                    );
                    rates.push(highest);
                    rates.push(lowest);
                }
            }
            _ => break,
        }
    }
    rates.retain(|fps| fps.is_finite() && *fps >= 1.0 && *fps <= 1000.0);
    rates.sort_by(|left, right| right.total_cmp(left));
    rates.dedup_by(|left, right| (*left - *right).abs() < 0.01);
    rates
}

fn fraction_seconds(numerator: u32, denominator: u32) -> f64 {
    if numerator == 0 || denominator == 0 {
        return 0.0;
    }
    f64::from(numerator) / f64::from(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_fourcc() {
        let mjpeg = u32::from_le_bytes(*b"MJPG");
        assert_eq!(fourcc_string(mjpeg), "MJPG");
        assert_eq!(fourcc_string(u32::from_le_bytes(*b"YUYV")), "YUYV");
    }

    #[test]
    fn ioctl_layouts_match_kernel_sizes() {
        assert_eq!(std::mem::size_of::<Capability>(), 104);
        assert_eq!(std::mem::size_of::<FmtDesc>(), 64);
        assert_eq!(std::mem::size_of::<FrmSizeEnum>(), 44);
        assert_eq!(std::mem::size_of::<FrmIvalEnum>(), 52);
    }
}
