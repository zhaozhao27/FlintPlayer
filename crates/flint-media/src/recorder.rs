use crossbeam_channel::{bounded, Sender, TrySendError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const QUEUE_FRAMES: usize = 8;

pub struct RecordSummary {
    pub path: PathBuf,
    pub frames: u64,
    pub dropped: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputFormat {
    Rgba,
    Nv12,
}

pub struct Recorder {
    tx: Option<Sender<Vec<u8>>>,
    worker: Option<JoinHandle<Result<PathBuf, String>>>,
    path: PathBuf,
    frames: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    format: InputFormat,
    width: usize,
    height: usize,
    frame_bytes: usize,
}

impl Recorder {
    pub fn start(path: &Path, width: usize, height: usize, fps: f64) -> Result<Self, String> {
        Self::start_with(path, width, height, fps, InputFormat::Rgba)
    }

    pub fn start_nv12(path: &Path, width: usize, height: usize, fps: f64) -> Result<Self, String> {
        Self::start_with(path, width, height, fps, InputFormat::Nv12)
    }

    fn start_with(
        path: &Path,
        width: usize,
        height: usize,
        fps: f64,
        format: InputFormat,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("录制需要有效的画面尺寸".to_string());
        }
        let fps = if fps.is_finite() && fps > 0.1 && fps <= 240.0 {
            fps
        } else {
            30.0
        };
        let pix_fmt = match format {
            InputFormat::Rgba => "rgba",
            InputFormat::Nv12 => "nv12",
        };
        let frame_bytes = match format {
            InputFormat::Rgba => width * height * 4,
            InputFormat::Nv12 => width * height + width.div_ceil(2) * 2 * height.div_ceil(2),
        };
        let mut command = Command::new("ffmpeg");
        command
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", pix_fmt])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-framerate", &format!("{fps:.3}")])
            .args(["-i", "pipe:0", "-an"])
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("启动 ffmpeg 失败：{error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "无法打开 ffmpeg 输入管道".to_string())?;
        let (tx, rx) = bounded::<Vec<u8>>(QUEUE_FRAMES);
        let frames = Arc::new(AtomicU64::new(0));
        let dropped = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&frames);
        let target = path.to_path_buf();
        let worker = thread::spawn(move || {
            for buffer in rx {
                if let Err(error) = stdin.write_all(&buffer) {
                    return Err(format!("写入录制流失败：{error}"));
                }
                counter.fetch_add(1, Ordering::Relaxed);
            }
            drop(stdin);
            let output = child
                .wait_with_output()
                .map_err(|error| format!("等待 ffmpeg 结束失败：{error}"))?;
            if !output.status.success() {
                let message = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "录制编码失败：{}",
                    message.lines().last().unwrap_or("ffmpeg 退出异常")
                ));
            }
            Ok(target)
        });
        Ok(Self {
            tx: Some(tx),
            worker: Some(worker),
            path: path.to_path_buf(),
            frames,
            dropped,
            format,
            width,
            height,
            frame_bytes,
        })
    }

    pub fn push(&self, rgba: &[u8]) {
        if self.format != InputFormat::Rgba || rgba.len() != self.frame_bytes {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.enqueue(rgba.to_vec());
    }

    pub fn push_nv12(&self, y: &[u8], y_stride: usize, uv: &[u8], uv_stride: usize) {
        if self.format != InputFormat::Nv12 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let Some(buffer) =
            crate::snapshot::pack_nv12(self.width, self.height, y, y_stride, uv, uv_stride)
        else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.enqueue(buffer);
    }

    fn enqueue(&self, buffer: Vec<u8>) {
        let Some(tx) = self.tx.as_ref() else {
            return;
        };
        match tx.try_send(buffer) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn stop(mut self) -> Result<RecordSummary, String> {
        self.tx.take();
        let result = self
            .worker
            .take()
            .map(|worker| worker.join().map_err(|_| "录制线程异常退出".to_string())?)
            .unwrap_or_else(|| Ok(self.path.clone()));
        let frames = self.frames.load(Ordering::Relaxed);
        let dropped = self.dropped.load(Ordering::Relaxed);
        result.map(|path| RecordSummary {
            path,
            frames,
            dropped,
        })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn records_raw_frames_to_mp4() {
        if !ffmpeg_available() {
            eprintln!("跳过：系统 PATH 中没有 ffmpeg");
            return;
        }
        let dir = std::env::temp_dir().join(format!("flint-rec-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.mp4");
        let recorder = Recorder::start(&path, 64, 64, 30.0).expect("启动录制");
        let frame = vec![128u8; 64 * 64 * 4];
        for _ in 0..30 {
            recorder.push(&frame);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let summary = recorder.stop().expect("完成录制");
        let size = std::fs::metadata(&summary.path)
            .expect("录制文件存在")
            .len();
        assert!(size > 0);
        assert!(summary.frames > 0);
        assert_eq!(summary.frames + summary.dropped, 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_nv12_frames_to_mp4() {
        if !ffmpeg_available() {
            eprintln!("跳过：系统 PATH 中没有 ffmpeg");
            return;
        }
        let dir = std::env::temp_dir().join(format!("flint-rec-nv12-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.mp4");
        let recorder = Recorder::start_nv12(&path, 64, 64, 30.0).expect("启动录制");
        let y = vec![128u8; 64 * 64];
        let uv = vec![128u8; 64 * 32];
        for _ in 0..30 {
            recorder.push_nv12(&y, 64, &uv, 64);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let summary = recorder.stop().expect("完成录制");
        let size = std::fs::metadata(&summary.path)
            .expect("录制文件存在")
            .len();
        assert!(size > 0);
        assert!(summary.frames > 0);
        assert_eq!(summary.frames + summary.dropped, 30);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
