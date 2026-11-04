use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

impl Language {
    pub fn from_code(code: &str) -> Self {
        match code {
            "zh" => Language::Zh,
            "en" => Language::En,
            _ => Language::from_locale(),
        }
    }

    fn from_locale() -> Self {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("LC_MESSAGES")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if locale.starts_with("zh") {
            Language::Zh
        } else {
            Language::En
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    CURRENT.store(
        match language {
            Language::Zh => 0,
            Language::En => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn language() -> Language {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Language::En
    } else {
        Language::Zh
    }
}

fn english() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../i18n/en.tsv")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('\t'))
            .collect()
    })
}

pub fn tr(text: &str) -> String {
    if language() == Language::En {
        if let Some(value) = english().get(text) {
            return (*value).to_string();
        }
        if let Some(rest) = text.strip_prefix('　') {
            if let Some(value) = english().get(rest) {
                return format!("　{value}");
            }
        }
    }
    text.to_string()
}

pub fn tr_template(template: &str, args: &[(&str, &str)]) -> String {
    let mut output = tr(template);
    for (name, value) in args {
        output = output.replace(&format!("{{{name}}}"), value);
    }
    output
}

struct MessageTemplate {
    segments: Vec<&'static str>,
    translated: &'static str,
}

fn message_templates() -> &'static [MessageTemplate] {
    static TEMPLATES: OnceLock<Vec<MessageTemplate>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let mut templates = Vec::new();
        for (source, translated) in english().iter() {
            if !source.contains('{') {
                continue;
            }
            let segments = split_placeholders(source);
            if segments.len() > 1 {
                templates.push(MessageTemplate {
                    segments,
                    translated,
                });
            }
        }
        templates.sort_by(|a, b| {
            b.segments
                .iter()
                .map(|s| s.len())
                .sum::<usize>()
                .cmp(&a.segments.iter().map(|s| s.len()).sum::<usize>())
        });
        templates
    })
}

fn split_placeholders(template: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = template.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if let Some(end) = template[index..].find('}') {
                segments.push(&template[start..index]);
                index += end + 1;
                start = index;
                continue;
            }
        }
        index += 1;
    }
    segments.push(&template[start..]);
    segments
}

fn match_template<'a>(text: &'a str, template: &MessageTemplate) -> Option<Vec<&'a str>> {
    let segments = &template.segments;
    let first = segments.first()?;
    let last = segments.last()?;
    if !text.starts_with(first) || !text.ends_with(last) {
        return None;
    }
    if text.len() < first.len() + last.len() {
        return None;
    }
    let mut captures = Vec::with_capacity(segments.len().saturating_sub(1));
    let mut position = first.len();
    let end = text.len() - last.len();
    for segment in &segments[1..segments.len() - 1] {
        let found = text[position..end].find(segment)?;
        let capture_end = position + found;
        captures.push(&text[position..capture_end]);
        position = capture_end + segment.len();
    }
    captures.push(&text[position..end]);
    Some(captures)
}

fn fill_placeholders(template: &str, captures: &[&str]) -> String {
    let mut output = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut copy_from = 0;
    let mut index = 0;
    let mut capture_index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if let Some(end) = template[index..].find('}') {
                if capture_index < captures.len() {
                    output.push_str(&template[copy_from..index]);
                    output.push_str(captures[capture_index]);
                    copy_from = index + end + 1;
                    capture_index += 1;
                    index = copy_from;
                    continue;
                }
            }
        }
        index += 1;
    }
    output.push_str(&template[copy_from..]);
    output
}

pub fn tr_notice(text: &str) -> String {
    if language() != Language::En {
        return text.to_string();
    }
    if let Some(value) = english().get(text) {
        return (*value).to_string();
    }
    for template in message_templates() {
        if let Some(captures) = match_template(text, template) {
            let translated: Vec<String> =
                captures.iter().map(|capture| tr_notice(capture)).collect();
            let borrowed: Vec<&str> = translated.iter().map(String::as_str).collect();
            return fill_placeholders(template.translated, &borrowed);
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_notices_in_english() {
        set_language(Language::En);
        assert_eq!(tr_notice("字幕延迟：+0.5 秒"), "Subtitle delay: +0.5 s");
        assert_eq!(tr_notice("已取消关机"), "Cancelled Shut down");
        assert_eq!(tr_notice("缩放：按 4:3 拉伸"), "Scaling: Stretch to 4:3");
        assert_eq!(
            tr_notice("帧大小：宽 110% 高 110%"),
            "Frame size: width 110% height 110%"
        );
        assert_eq!(
            tr_notice("声音输出设备：alsa_output.pci"),
            "Audio output device: alsa_output.pci"
        );
        assert_eq!(tr_notice("系统音量：72%"), "System volume: 72%");
        assert_eq!(tr_notice("系统静音：开"), "System mute: On");
        assert_eq!(
            tr_notice("音量控制方式：系统主音量控制"),
            "Volume control: System master volume control"
        );
        assert_eq!(
            tr_notice("声音录制已保存：/tmp/a.wav（3.2 秒）"),
            "Audio recording saved: /tmp/a.wav (3.2 s)"
        );
        assert_eq!(
            tr_notice("开始录制声音：/tmp/a.wav"),
            "Started audio recording: /tmp/a.wav"
        );
        assert_eq!(
            tr_notice("声音录制储存在播放文件夹：开"),
            "Save audio recordings in the play folder: On"
        );
        assert_eq!(tr_notice("语音消除：开"), "Vocal removal: On");
        assert_eq!(tr_notice("声音处理：关"), "Audio processing: Off");
        assert_eq!(tr_notice("均衡器：开"), "Equalizer: On");
        assert_eq!(
            tr_notice("均衡器预置：低音增强"),
            "Equalizer preset: Bass boost"
        );
        assert_eq!(
            tr_notice("声音输出方式：空设备(静音)"),
            "Audio output mode: Null device (muted)"
        );
        assert_eq!(
            tr_notice("音量控制设置：最大 135%，AC3/DTS 放大 6.0 dB"),
            "Volume control settings: max 135%, AC3/DTS boost 6.0 dB"
        );
        assert_eq!(
            tr_notice("AC3/DTS 音量处理：开（放大 6.0 dB）"),
            "AC3/DTS volume processing: On (boost 6.0 dB)"
        );
        assert_eq!(
            tr_notice("声音同步：滞后 100ms"),
            "Audio sync: Delay 100 ms"
        );
        assert_eq!(
            tr_notice("声音同步：超前 50ms"),
            "Audio sync: Advance 50 ms"
        );
        assert_eq!(
            tr_notice("音频同步保持：始终保持声音同步"),
            "Keep audio sync: Always keep audio sync"
        );
        assert_eq!(
            tr_notice("退出时初始化声音同步：开"),
            "Reset audio sync on exit: On"
        );
        assert_eq!(
            tr_notice("默认打开：打开文件"),
            "Default open: Open file"
        );
        assert_eq!(tr_notice("未知消息"), "未知消息");
        set_language(Language::Zh);
    }
}
