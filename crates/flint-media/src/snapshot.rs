use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub enum SnapshotSource<'a> {
    Rgba(&'a [u8]),
    Nv12 {
        y: &'a [u8],
        y_stride: usize,
        uv: &'a [u8],
        uv_stride: usize,
    },
}

pub fn save_snapshot(
    path: &Path,
    width: u32,
    height: u32,
    source: SnapshotSource<'_>,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("截图尺寸无效".to_string());
    }
    let (pix_fmt, data) = match source {
        SnapshotSource::Rgba(rgba) => {
            let expected = width as usize * height as usize * 4;
            if rgba.len() != expected {
                return Err("截图数据尺寸不匹配".to_string());
            }
            ("rgba", rgba.to_vec())
        }
        SnapshotSource::Nv12 {
            y,
            y_stride,
            uv,
            uv_stride,
        } => {
            let packed = pack_nv12(width as usize, height as usize, y, y_stride, uv, uv_stride)
                .ok_or_else(|| "截图数据尺寸不匹配".to_string())?;
            ("nv12", packed)
        }
    };
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", pix_fmt])
        .args(["-s", &format!("{width}x{height}")])
        .args(["-i", "pipe:0", "-frames:v", "1", "-update", "1"])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("启动 ffmpeg 失败：{error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法打开 ffmpeg 输入管道".to_string())?;
    let written = stdin.write_all(&data);
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 ffmpeg 结束失败：{error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "截图编码失败：{}",
            message.lines().last().unwrap_or("ffmpeg 退出异常")
        ));
    }
    written.map_err(|error| format!("写入截图数据失败：{error}"))?;
    Ok(())
}

pub(crate) fn pack_nv12(
    width: usize,
    height: usize,
    y: &[u8],
    y_stride: usize,
    uv: &[u8],
    uv_stride: usize,
) -> Option<Vec<u8>> {
    let uv_row = width.div_ceil(2) * 2;
    let chroma_height = height.div_ceil(2);
    if width == 0 || height == 0 || y_stride < width || uv_stride < uv_row {
        return None;
    }
    if y.len() < y_stride * height || uv.len() < uv_stride * chroma_height {
        return None;
    }
    let mut buffer = vec![0u8; width * height + uv_row * chroma_height];
    let (luma, chroma) = buffer.split_at_mut(width * height);
    // Plane stride can be wider than the visible row, so pack each visible row separately.
    for row in 0..height {
        let start = row * y_stride;
        luma[row * width..(row + 1) * width].copy_from_slice(&y[start..start + width]);
    }
    for row in 0..chroma_height {
        let start = row * uv_stride;
        chroma[row * uv_row..(row + 1) * uv_row].copy_from_slice(&uv[start..start + uv_row]);
    }
    Some(buffer)
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
    fn saves_nv12_snapshot_png() {
        if !ffmpeg_available() {
            eprintln!("跳过：系统 PATH 中没有 ffmpeg");
            return;
        }
        let dir = std::env::temp_dir().join(format!("flint-snapshot-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("snapshot.png");
        let width = 32u32;
        let height = 32u32;
        let y = vec![128u8; width as usize * height as usize];
        let uv = vec![128u8; width as usize * height as usize / 2];
        save_snapshot(
            &path,
            width,
            height,
            SnapshotSource::Nv12 {
                y: &y,
                y_stride: width as usize,
                uv: &uv,
                uv_stride: width as usize,
            },
        )
        .expect("保存截图");
        let bytes = std::fs::read(&path).expect("截图文件存在");
        assert!(bytes.starts_with(b"\x89PNG"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
