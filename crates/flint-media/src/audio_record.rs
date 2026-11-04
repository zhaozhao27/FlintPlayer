use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct AudioRecordSummary {
    pub path: PathBuf,
    pub frames: u64,
    pub seconds: f64,
}

pub struct WavRecorder {
    writer: BufWriter<File>,
    path: PathBuf,
    rate: u32,
    channels: u16,
    data_bytes: u64,
    frames: u64,
}

impl WavRecorder {
    pub fn start(path: &Path, rate: u32, channels: u16) -> Result<Self, String> {
        let rate = rate.max(1);
        let channels = channels.max(1);
        let file = File::create(path).map_err(|error| format!("创建录音文件失败：{error}"))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&wav_header(rate, channels, 0))
            .map_err(|error| format!("写入录音文件失败：{error}"))?;
        Ok(Self {
            writer,
            path: path.to_path_buf(),
            rate,
            channels,
            data_bytes: 0,
            frames: 0,
        })
    }

    pub fn write(&mut self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if self.writer.write_all(&bytes).is_ok() {
            self.data_bytes += bytes.len() as u64;
            self.frames += samples.len() as u64 / u64::from(self.channels);
        }
    }

    pub fn stop(mut self) -> Result<AudioRecordSummary, String> {
        self.writer
            .flush()
            .map_err(|error| format!("写入录音文件失败：{error}"))?;
        let mut file = self
            .writer
            .into_inner()
            .map_err(|error| format!("写入录音文件失败：{error}"))?;
        let data_bytes = self.data_bytes.min(u64::from(u32::MAX) - 36) as u32;
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(&wav_header(self.rate, self.channels, data_bytes)))
            .map_err(|error| format!("写入录音文件失败：{error}"))?;
        Ok(AudioRecordSummary {
            path: self.path,
            frames: self.frames,
            seconds: self.frames as f64 / f64::from(self.rate),
        })
    }
}

fn wav_header(rate: u32, channels: u16, data_bytes: u32) -> [u8; 44] {
    let bits: u16 = 16;
    let block_align = channels * bits / 8;
    let byte_rate = rate * u32::from(block_align);
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&channels.to_le_bytes());
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&bits.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_valid_wav_header_and_data() {
        let dir = std::env::temp_dir().join(format!("flint-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("capture.wav");
        let mut recorder = WavRecorder::start(&path, 48000, 2).unwrap();
        recorder.write(&[1000, -1000, 2000, -2000]);
        let summary = recorder.stop().unwrap();
        assert_eq!(summary.frames, 2);
        assert!((summary.seconds - 2.0 / 48000.0).abs() < 1e-9);
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[36..40], b"data");
        assert_eq!(u32::from_le_bytes(data[40..44].try_into().unwrap()), 8);
        assert_eq!(data.len(), 52);
        assert_eq!(i16::from_le_bytes(data[44..46].try_into().unwrap()), 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
