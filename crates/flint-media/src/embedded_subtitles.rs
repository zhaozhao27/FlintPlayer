use crate::subtitles::{clean_text, Cue, SubtitleTrack};
use ffmpeg_next as ffmpeg;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleStreamInfo {
    pub index: usize,
    pub codec: String,
    pub language: String,
    pub title: String,
}

impl SubtitleStreamInfo {
    pub fn label(&self) -> String {
        let language = match self.language.as_str() {
            "" | "und" => "未知".to_string(),
            other => other.to_ascii_uppercase(),
        };
        if self.title.is_empty() {
            format!("内嵌字幕 {}（{}）", self.index, language)
        } else {
            format!("内嵌字幕 {}：{}（{}）", self.index, self.title, language)
        }
    }
}

fn open_input(path: &Path) -> Result<ffmpeg::format::context::Input, String> {
    let path = path.to_path_buf();
    ffmpeg::format::input(&path).map_err(|error| format!("打开文件失败：{error}"))
}

pub fn subtitle_streams(path: &Path) -> Result<Vec<SubtitleStreamInfo>, String> {
    let context = open_input(path)?;
    let mut streams = Vec::new();
    for stream in context.streams() {
        if stream.parameters().medium() != ffmpeg::media::Type::Subtitle {
            continue;
        }
        let metadata = stream.metadata();
        streams.push(SubtitleStreamInfo {
            index: stream.index(),
            codec: stream.parameters().id().name().to_string(),
            language: metadata.get("language").unwrap_or("und").to_string(),
            title: metadata.get("title").unwrap_or_default().to_string(),
        });
    }
    Ok(streams)
}

pub fn load_subtitle_stream(path: &Path, index: usize) -> Result<SubtitleTrack, String> {
    let mut context = open_input(path)?;
    let stream = context
        .streams()
        .find(|stream| {
            stream.index() == index && stream.parameters().medium() == ffmpeg::media::Type::Subtitle
        })
        .ok_or_else(|| "文件中没有该字幕轨".to_string())?;
    let time_base = {
        let value = stream.time_base();
        if value.denominator() == 0 {
            0.0
        } else {
            value.numerator() as f64 / value.denominator() as f64
        }
    };
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|error| format!("读取字幕参数失败：{error}"))?
        .decoder()
        .subtitle()
        .map_err(|error| format!("字幕解码器不可用：{error}"))?;

    let mut cues = Vec::new();
    for (stream, packet) in context.packets() {
        if stream.index() != index {
            continue;
        }
        let base = packet
            .pts()
            .or_else(|| packet.dts())
            .map(|pts| pts as f64 * time_base);
        let duration = packet.duration().max(0) as f64 * time_base;
        let mut subtitle = ffmpeg::Subtitle::new();
        let decoded = decoder.decode(&packet, &mut subtitle).unwrap_or(false);
        if decoded {
            let text = subtitle_text(&subtitle);
            if !text.is_empty() {
                let start = base.unwrap_or(0.0) + f64::from(subtitle.start()) / 1000.0;
                let end = if subtitle.end() > 0 {
                    base.unwrap_or(0.0) + f64::from(subtitle.end()) / 1000.0
                } else if duration > 0.01 {
                    start + duration
                } else {
                    start + 2.0
                };
                if end > start {
                    cues.push(Cue {
                        start,
                        end,
                        text,
                        style: Default::default(),
                    });
                }
            }
        }
        // avcodec_decode_subtitle2 分配的字幕矩形需要调用方释放，ffmpeg-next 的 Subtitle 没有 Drop。
        unsafe {
            ffmpeg::ffi::avsubtitle_free(subtitle.as_mut_ptr());
        }
    }

    if cues.is_empty() {
        return Err("该字幕轨没有可显示的文本字幕".to_string());
    }
    cues.sort_by(|left, right| left.start.total_cmp(&right.start));
    Ok(SubtitleTrack {
        path: path.to_path_buf(),
        cues,
    })
}

fn subtitle_text(subtitle: &ffmpeg::Subtitle) -> String {
    let mut lines = Vec::new();
    for rect in subtitle.rects() {
        match rect {
            ffmpeg::subtitle::Rect::Text(text) => lines.push(clean_text(text.get(), true)),
            ffmpeg::subtitle::Rect::Ass(ass) => {
                let payload = ass.get();
                let text = payload.splitn(9, ',').nth(8).unwrap_or(payload);
                lines.push(clean_text(text, true));
            }
            _ => {}
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_streams() {
        let info = SubtitleStreamInfo {
            index: 2,
            codec: "ass".into(),
            language: "chi".into(),
            title: "简体".into(),
        };
        assert_eq!(info.label(), "内嵌字幕 2：简体（CHI）");
        let unnamed = SubtitleStreamInfo {
            index: 3,
            codec: "subrip".into(),
            language: "und".into(),
            title: String::new(),
        };
        assert_eq!(unnamed.label(), "内嵌字幕 3（未知）");
    }
}
