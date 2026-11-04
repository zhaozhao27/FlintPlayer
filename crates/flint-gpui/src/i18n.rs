use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

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
            _ => Self::from_locale(),
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

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_language(language: Language) {
    ENGLISH.store(language == Language::En, Ordering::Relaxed);
}

pub fn language() -> Language {
    if ENGLISH.load(Ordering::Relaxed) {
        Language::En
    } else {
        Language::Zh
    }
}

fn table() -> &'static HashMap<&'static str, &'static str> {
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
    if language() != Language::En {
        return text.to_string();
    }
    if let Some(value) = table().get(text) {
        return (*value).to_string();
    }
    if let Some(rest) = text.strip_prefix('\u{3000}') {
        if let Some(value) = table().get(rest) {
            return format!("\u{3000}{value}");
        }
    }
    text.to_string()
}

pub fn trf(template: &str, values: &[&dyn std::fmt::Display]) -> String {
    let translated = tr(template);
    let mut output = String::with_capacity(translated.len());
    let mut values = values.iter();
    let mut rest = translated.as_str();
    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else {
            break;
        };
        output.push_str(&rest[..start]);
        match values.next() {
            Some(value) => output.push_str(&value.to_string()),
            None => output.push_str(&rest[start..start + length + 1]),
        }
        rest = &rest[start + length + 1..];
    }
    output.push_str(rest);
    output
}
