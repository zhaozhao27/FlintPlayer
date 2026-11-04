use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueStyle {
    pub color: Option<u32>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub top: Option<bool>,
    pub font: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub style: CueStyle,
}

#[derive(Clone, Debug, Default)]
pub struct SubtitleTrack {
    pub path: PathBuf,
    pub cues: Vec<Cue>,
}

impl SubtitleTrack {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("读取字幕失败：{error}"))?;
        let text = decode_text(&bytes);
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let cues = match extension.as_str() {
            "ass" | "ssa" => parse_ass(&text),
            "vtt" => parse_cues(&text),
            "srt" => parse_cues(&text),
            _ => {
                let cues = parse_cues(&text);
                if cues.is_empty() {
                    parse_ass(&text)
                } else {
                    cues
                }
            }
        };
        if cues.is_empty() {
            return Err("字幕文件中没有可显示的文本字幕".to_string());
        }
        Ok(Self {
            path: path.to_path_buf(),
            cues,
        })
    }

    pub fn cue_at(&self, seconds: f64) -> Option<&Cue> {
        let index = self.cues.partition_point(|cue| cue.start <= seconds);
        if index == 0 {
            return None;
        }
        let cue = &self.cues[index - 1];
        (seconds < cue.end).then_some(cue)
    }

    pub fn text_at(&self, seconds: f64) -> &str {
        self.cue_at(seconds)
            .map(|cue| cue.text.as_str())
            .unwrap_or("")
    }

    pub fn to_srt(&self) -> String {
        let mut output = String::new();
        for (index, cue) in self.cues.iter().enumerate() {
            output.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                index + 1,
                format_srt_time(cue.start),
                format_srt_time(cue.end),
                cue.text
            ));
        }
        output
    }

    pub fn to_ass(&self) -> String {
        let mut output = String::from(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 384\nPlayResY: 288\n\n\
             [V4+ Styles]\n\
             Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
             Style: Default,Noto Sans CJK SC,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n\n\
             [Events]\n\
             Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        );
        for cue in &self.cues {
            output.push_str(&format!(
                "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
                format_ass_time(cue.start),
                format_ass_time(cue.end),
                cue.text.replace('\n', "\\N")
            ));
        }
        output
    }
}

fn format_srt_time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        total / 3_600_000,
        total / 60_000 % 60,
        total / 1000 % 60,
        total % 1000
    )
}

fn format_ass_time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        total / 360_000,
        total / 6000 % 60,
        total / 100 % 60,
        total % 100
    )
}

fn decode_text(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_string();
    }
    let (text, _, _) = encoding_rs::GB18030.decode(bytes);
    text.trim_start_matches('\u{feff}').to_string()
}

fn parse_cues(text: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut lines = text.lines().map(str::trim_end).peekable();
    while let Some(line) = lines.next() {
        if !line.contains("-->") {
            continue;
        }
        let mut range = line.split("-->");
        let Some(start) = range.next().and_then(parse_time) else {
            continue;
        };
        let Some(end) = range
            .next()
            .and_then(|value| value.split_whitespace().next())
            .and_then(parse_time)
        else {
            continue;
        };
        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() || next.contains("-->") {
                break;
            }
            let next = lines.next().unwrap_or_default();
            body.push(clean_text(next, true));
        }
        let text = body.join("\n").trim().to_string();
        if !text.is_empty() && end > start {
            cues.push(Cue {
                start,
                end,
                text,
                style: CueStyle::default(),
            });
        }
    }
    cues.sort_by(|left, right| left.start.total_cmp(&right.start));
    cues
}

fn parse_ass(text: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut styles: Vec<(String, CueStyle)> = Vec::new();
    let mut section = String::new();
    let mut fields: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_ascii_lowercase();
            fields.clear();
            continue;
        }
        match section.as_str() {
            "[v4+ styles]" | "[v4 styles]" => {
                if let Some(rest) = line.strip_prefix("Format:") {
                    fields = rest
                        .split(',')
                        .map(|field| field.trim().to_ascii_lowercase())
                        .collect();
                } else if let Some(rest) = line.strip_prefix("Style:") {
                    if let Some((name, style)) = parse_ass_style(rest, &fields) {
                        styles.push((name, style));
                    }
                }
            }
            "[events]" => {
                if let Some(rest) = line.strip_prefix("Format:") {
                    fields = rest
                        .split(',')
                        .map(|field| field.trim().to_ascii_lowercase())
                        .collect();
                    continue;
                }
                let Some(rest) = line.strip_prefix("Dialogue:") else {
                    continue;
                };
                if fields.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = rest.splitn(fields.len(), ',').map(str::trim).collect();
                if parts.len() < fields.len() {
                    continue;
                }
                let field = |name: &str| {
                    fields
                        .iter()
                        .position(|value| value == name)
                        .and_then(|index| parts.get(index).copied())
                };
                let (Some(start), Some(end), Some(body)) =
                    (field("start"), field("end"), field("text"))
                else {
                    continue;
                };
                let (Some(start), Some(end)) = (parse_time(start), parse_time(end)) else {
                    continue;
                };
                let mut style = field("style")
                    .and_then(|name| {
                        styles
                            .iter()
                            .find(|(key, _)| key == name)
                            .map(|(_, style)| style.clone())
                    })
                    .unwrap_or_default();
                apply_ass_overrides(body, &mut style);
                let text = clean_text(body, false).trim().to_string();
                if !text.is_empty() && end > start {
                    cues.push(Cue {
                        start,
                        end,
                        text,
                        style,
                    });
                }
            }
            _ => {}
        }
    }
    cues.sort_by(|left, right| left.start.total_cmp(&right.start));
    cues
}

fn parse_ass_style(rest: &str, fields: &[String]) -> Option<(String, CueStyle)> {
    let parts: Vec<&str> = rest.splitn(fields.len(), ',').map(str::trim).collect();
    if parts.len() < fields.len() {
        return None;
    }
    let value = |name: &str| {
        fields
            .iter()
            .position(|field| field == name)
            .and_then(|index| parts.get(index).copied())
    };
    let name = value("name")?.to_string();
    let mut style = CueStyle::default();
    if let Some(font) = value("fontname") {
        if !font.is_empty() {
            style.font = Some(font.to_string());
        }
    }
    if let Some(size) = value("fontsize").and_then(|size| size.parse::<f32>().ok()) {
        style.font_size = Some(size);
    }
    if let Some(color) = value("primarycolour").and_then(parse_ass_color) {
        style.color = Some(color);
    }
    if let Some(bold) = value("bold") {
        style.bold = Some(bold.trim() == "1" || bold.eq_ignore_ascii_case("-1"));
    }
    if let Some(alignment) = value("alignment").and_then(|value| value.parse::<u32>().ok()) {
        style.top = Some((7..=9).contains(&alignment));
    }
    Some((name, style))
}

fn apply_ass_overrides(body: &str, style: &mut CueStyle) {
    let mut tags = String::new();
    let mut depth = 0;
    for character in body.chars() {
        match character {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            other if depth > 0 => tags.push(other),
            _ => {}
        }
    }
    for tag in tags.split('\\').skip(1) {
        if let Some(rest) = tag.strip_prefix("an") {
            if let Ok(alignment) = rest
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<u32>()
            {
                style.top = Some((7..=9).contains(&alignment));
            }
        } else if let Some(rest) = tag.strip_prefix("fs") {
            if let Ok(size) = rest.parse::<f32>() {
                if size > 0.0 {
                    style.font_size = Some(size);
                }
            }
        } else if let Some(rest) = tag.strip_prefix('b') {
            if rest.starts_with('0') {
                style.bold = Some(false);
            } else if rest.starts_with('1') || rest.is_empty() {
                style.bold = Some(true);
            }
        } else if let Some(rest) = tag.strip_prefix("fn") {
            let name = rest.trim_end_matches(|c: char| c == '&' || c.is_whitespace());
            if !name.is_empty() {
                style.font = Some(name.to_string());
            }
        } else if let Some(color) = tag
            .strip_prefix("c")
            .or_else(|| tag.strip_prefix("1c"))
            .and_then(parse_ass_color)
        {
            style.color = Some(color);
        }
    }
}

fn parse_ass_color(text: &str) -> Option<u32> {
    let hex = text
        .trim()
        .trim_start_matches(['&', 'H', 'h'])
        .trim_end_matches('&');
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    let (b, g, r) = ((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF);
    Some((r << 16) | (g << 8) | b)
}

fn parse_time(value: &str) -> Option<f64> {
    let value = value.trim().replace(',', ".");
    let parts: Vec<&str> = value.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [hours, minutes, seconds] => (
            hours.parse::<f64>().ok()?,
            minutes.parse::<f64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        [minutes, seconds] => (
            0.0,
            minutes.parse::<f64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

pub(crate) fn clean_text(text: &str, strip_html: bool) -> String {
    let mut output = String::with_capacity(text.len());
    let mut override_depth = 0;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' => override_depth += 1,
            '}' if override_depth > 0 => override_depth -= 1,
            '\\' if override_depth == 0 => match chars.next() {
                Some('N') | Some('n') => output.push('\n'),
                Some('h') => output.push(' '),
                Some(other) => output.push(other),
                None => {}
            },
            '<' if strip_html && override_depth == 0 => {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
            }
            other if override_depth == 0 => output.push(other),
            _ => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let text = "1\n00:00:01,500 --> 00:00:03,000\nHello <i>world</i>\nsecond line\n\n2\n00:01:00,000 --> 00:01:02,250\n再见\n";
        let cues = parse_cues(text);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1.5);
        assert_eq!(cues[0].end, 3.0);
        assert_eq!(cues[0].text, "Hello world\nsecond line");
        assert_eq!(cues[1].text, "再见");
    }

    #[test]
    fn parses_ass_with_overrides() {
        let text = "[Script Info]\nTitle: test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:05.00,0:00:07.50,Default,,0,0,0,,{\\an8}Top line\\Nsecond, with comma\n";
        let cues = parse_ass(text);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, 5.0);
        assert_eq!(cues[0].end, 7.5);
        assert_eq!(cues[0].text, "Top line\nsecond, with comma");
    }

    #[test]
    fn parses_ass_styles_and_inline_overrides() {
        let text = "[Script Info]\n\n[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n\
Style: Top,Yellow,32,&H0000FFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,1,0,8,10,10,10,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,plain\n\
Dialogue: 0,0:00:03.00,0:00:04.00,Top,,0,0,0,,styled text\n\
Dialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,{\\c&H0000FF&\\fs40\\b1\\an8\\fnNoto Sans}inline\n";
        let cues = parse_ass(text);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].style.font_size, Some(20.0));
        assert_eq!(cues[0].style.color, Some(0xFFFFFF));
        assert_eq!(cues[0].style.bold, Some(false));
        assert_eq!(cues[0].style.top, Some(false));

        let top = &cues[1].style;
        assert_eq!(top.color, Some(0xFFFF00));
        assert_eq!(top.font_size, Some(32.0));
        assert_eq!(top.bold, Some(true));
        assert_eq!(top.top, Some(true));

        let inline = &cues[2].style;
        assert_eq!(inline.color, Some(0xFF0000));
        assert_eq!(inline.font_size, Some(40.0));
        assert_eq!(inline.bold, Some(true));
        assert_eq!(inline.top, Some(true));
        assert_eq!(inline.font.as_deref(), Some("Noto Sans"));
        assert_eq!(cues[2].text, "inline");
    }

    #[test]
    fn finds_cue_at_time() {
        let track = SubtitleTrack {
            path: PathBuf::new(),
            cues: vec![Cue {
                start: 1.0,
                end: 2.0,
                text: "a".into(),
                style: CueStyle {
                    color: Some(0x00FF00),
                    ..CueStyle::default()
                },
            }],
        };
        assert!(track.cue_at(0.5).is_none());
        assert_eq!(track.cue_at(1.5).unwrap().style.color, Some(0x00FF00));
        assert!(track.cue_at(2.0).is_none());
    }

    #[test]
    fn looks_up_text_by_time() {
        let track = SubtitleTrack {
            path: PathBuf::new(),
            cues: vec![
                Cue {
                    start: 1.0,
                    end: 2.0,
                    text: "a".into(),
                    style: CueStyle::default(),
                },
                Cue {
                    start: 3.0,
                    end: 4.0,
                    text: "b".into(),
                    style: CueStyle::default(),
                },
            ],
        };
        assert_eq!(track.text_at(0.5), "");
        assert_eq!(track.text_at(1.5), "a");
        assert_eq!(track.text_at(2.5), "");
        assert_eq!(track.text_at(3.2), "b");
    }

    #[test]
    fn exports_srt_and_ass() {
        let track = SubtitleTrack {
            path: PathBuf::new(),
            cues: vec![
                Cue {
                    start: 1.5,
                    end: 3.0,
                    text: "第一行\n第二行".into(),
                    style: CueStyle::default(),
                },
                Cue {
                    start: 3661.25,
                    end: 3662.5,
                    text: "late".into(),
                    style: CueStyle::default(),
                },
            ],
        };
        let srt = track.to_srt();
        assert!(srt.contains("00:00:01,500 --> 00:00:03,000\n第一行\n第二行"));
        assert!(srt.contains("01:01:01,250 --> 01:01:02,500\nlate"));
        let ass = track.to_ass();
        assert!(ass.contains("Dialogue: 0,0:00:01.50,0:00:03.00,Default,,0,0,0,,第一行\\N第二行"));
        assert!(ass.contains("Dialogue: 0,1:01:01.25,1:01:02.50,Default,,0,0,0,,late"));
    }

    #[test]
    fn parses_vtt_timestamps() {
        let text = "WEBVTT\n\n00:00:02.000 --> 00:00:04.000\ncaption\n";
        let cues = parse_cues(text);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, 2.0);
        assert_eq!(cues[0].text, "caption");
    }
}
