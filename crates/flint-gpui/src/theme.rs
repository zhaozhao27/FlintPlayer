use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const THEME_FILE: &str = "theme.ini";

trait Token: Sized {
    fn parse(value: &str) -> Option<Self>;
    fn render(&self) -> String;
}

impl Token for u32 {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        match value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .or_else(|| value.strip_prefix('#'))
        {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => value.parse().ok(),
        }
    }

    fn render(&self) -> String {
        if *self > 0xff_ffff {
            format!("0x{self:08x}")
        } else {
            format!("0x{self:06x}")
        }
    }
}

impl Token for f32 {
    fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }

    fn render(&self) -> String {
        format!("{self}")
    }
}

macro_rules! theme_tokens {
    ($($field:ident : $ty:ty = $default:expr, $key:literal;)*) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct Theme {
            $(pub $field: $ty,)*
        }

        impl Default for Theme {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }

        $(pub fn $field() -> $ty {
            current().$field
        })*

        impl Theme {
            fn apply(&mut self, key: &str, value: &str) -> bool {
                match key {
                    $($key => match <$ty as Token>::parse(value) {
                        Some(parsed) => {
                            self.$field = parsed;
                            true
                        }
                        None => false,
                    },)*
                    _ => false,
                }
            }

            pub fn entries(&self) -> impl Iterator<Item = (&'static str, String)> + '_ {
                [$(($key, Token::render(&self.$field)),)*].into_iter()
            }
        }
    };
}

theme_tokens! {
    titlebar_height: f32 = 40.0, "titlebar_height";
    menu_row_height: f32 = 32.0, "menu_row_height";
    control_bar_height: f32 = 62.0, "control_bar_height";

    bg_primary: u32 = 0x1d1d1d, "bg_primary";
    accent: u32 = 0xead300, "accent";

    titlebar_bg: u32 = 0x1d1d1d, "titlebar_bg";
    titlebar_text: u32 = 0xd0d0d0, "titlebar_text";
    titlebar_text_dim: u32 = 0x8a8a8a, "titlebar_text_dim";
    titlebar_hover: u32 = 0x3a3a3a, "titlebar_hover";
    titlebar_close_hover: u32 = 0xc42b1c, "titlebar_close_hover";

    title_bg: u32 = 0x1e1e1e, "title_bg";
    title_text: u32 = 0xd8d8d8, "title_text";
    title_meta: u32 = 0x9a9a9a, "title_meta";
    title_source: u32 = 0xb8b8b8, "title_source";
    title_chevron: u32 = 0xa8a8a8, "title_chevron";
    title_button_tint: u32 = 0xc8c8c8, "title_button_tint";
    title_pin_active: u32 = 0xfae100, "title_pin_active";
    title_divider: u32 = 0x3a3a3a, "title_divider";
    title_button_hover: u32 = 0x232323, "title_button_hover";

    menu_bg: u32 = 0x3b3b3b, "menu_bg";
    menu_border: u32 = 0x5d5d5d, "menu_border";
    menu_hover: u32 = 0x1c1c1c, "menu_hover";
    menu_text: u32 = 0xe6e6e6, "menu_text";
    menu_text_dim: u32 = 0x7a7a7a, "menu_text_dim";
    menu_text_disabled: u32 = 0x6a6a6a, "menu_text_disabled";
    menu_shortcut: u32 = 0x9a9a9a, "menu_shortcut";
    menu_separator: u32 = 0x5d5d5d, "menu_separator";
    menu_check: u32 = 0xead300, "menu_check";

    control_bar_bg: u32 = 0x1b1b1b, "control_bar_bg";
    control_bar_text: u32 = 0xc8c8c8, "control_bar_text";
    control_bar_hover: u32 = 0x232323, "control_bar_hover";

    bar_border: u32 = 0x3a3a3a, "bar_border";
    bar_track: u32 = 0x4a4a4a, "bar_track";
    bar_fill: u32 = 0xe5c100, "bar_fill";
    bar_thumb: u32 = 0xffffff, "bar_thumb";
    bar_chapter: u32 = 0x9a9a9a, "bar_chapter";
    bar_bookmark: u32 = 0x4aa3ff, "bar_bookmark";
    bar_button_tint: u32 = 0xd0d0d0, "bar_button_tint";
    bar_button_disabled: u32 = 0x5a5a5a, "bar_button_disabled";
    bar_button_divider: u32 = 0x333333, "bar_button_divider";
    bar_chip_bg: u32 = 0x262626, "bar_chip_bg";
    bar_chip_border: u32 = 0x3a3a3a, "bar_chip_border";
    bar_chip_text: u32 = 0xa8a8a8, "bar_chip_text";
    bar_cluster_tint: u32 = 0xcfcfcf, "bar_cluster_tint";
    bar_sidebar_active: u32 = 0xf0c93c, "bar_sidebar_active";

    video_bg: u32 = 0x000000, "video_bg";
    panel_border: u32 = 0x3a3a3a, "panel_border";
    notice_bg: u32 = 0x000000c0, "notice_bg";
    notice_text: u32 = 0xfafafa, "notice_text";
}

static CURRENT: OnceLock<Theme> = OnceLock::new();

pub fn init() {
    ensure_file();
    let _ = current();
}

pub fn file_path() -> PathBuf {
    flint_core::config::config_dir().join(THEME_FILE)
}

pub fn load_from(path: &Path) -> Theme {
    let mut theme = Theme::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        return theme;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            log::warn!("忽略无效主题行：{line}");
            continue;
        };
        if !theme.apply(key.trim(), value.trim()) {
            log::warn!("忽略无效主题项：{line}");
        }
    }
    theme
}

fn current() -> &'static Theme {
    CURRENT.get_or_init(|| load_from(&file_path()))
}

fn ensure_file() {
    let path = file_path();
    if path.exists() {
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    let mut text = String::new();
    for (key, value) in Theme::default().entries() {
        text.push_str(&format!("{key} = {value}\n"));
    }
    if let Err(error) = std::fs::create_dir_all(parent) {
        log::warn!("无法创建主题目录 {}：{error}", parent.display());
        return;
    }
    if let Err(error) = std::fs::write(&path, text) {
        log::warn!("无法写入主题文件 {}：{error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::{Theme, Token};

    #[test]
    fn defaults_match_original_palette() {
        let theme = Theme::default();
        assert_eq!(theme.bg_primary, 0x1d1d1d);
        assert_eq!(theme.menu_check, 0xead300);
        assert_eq!(theme.notice_bg, 0x000000c0);
        assert_eq!(theme.control_bar_height, 62.0);
        assert_eq!(theme.entries().count(), 49);
    }

    #[test]
    fn parses_hex_and_decimal_colors() {
        assert_eq!(u32::parse("0x1d1d1d"), Some(0x1d1d1d));
        assert_eq!(u32::parse("#EAD300"), Some(0xead300));
        assert_eq!(u32::parse("123456"), Some(123456));
        assert_eq!(u32::parse("zzz"), None);
        assert_eq!(f32::parse("48.5"), Some(48.5));
    }

    #[test]
    fn renders_round_trippable_values() {
        assert_eq!(0x1d1d1du32.render(), "0x1d1d1d");
        assert_eq!(0x000000c0u32.render(), "0x0000c0");
        assert_eq!(u32::parse(&0x000000c0u32.render()), Some(0x000000c0));
        assert_eq!(40.0f32.render(), "40");
        assert_eq!(f32::parse(&40.0f32.render()), Some(40.0));
    }

    #[test]
    fn applies_overrides_and_ignores_unknown() {
        let mut theme = Theme::default();
        assert!(theme.apply("accent", "#ff0000"));
        assert_eq!(theme.accent, 0xff0000);
        assert!(theme.apply("menu_row_height", "28"));
        assert_eq!(theme.menu_row_height, 28.0);
        assert!(!theme.apply("nonexistent", "1"));
        assert!(!theme.apply("accent", "not-a-color"));
        assert_eq!(theme.accent, 0xff0000);
    }

    #[test]
    fn loads_ini_file_with_comments_and_blanks() {
        let dir = std::env::temp_dir().join(format!("flint-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("theme.ini");
        std::fs::write(
            &path,
            "; comment\n# another\n\naccent = 0x102030\nmenu_row_height = 30\nbogus = 1\n",
        )
        .unwrap();
        let theme = super::load_from(&path);
        assert_eq!(theme.accent, 0x102030);
        assert_eq!(theme.menu_row_height, 30.0);
        assert_eq!(theme.bg_primary, Theme::default().bg_primary);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
