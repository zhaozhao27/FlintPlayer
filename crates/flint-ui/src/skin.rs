use slint::Color;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub name: String,
    pub accent: Color,
    pub menu_bg: Color,
    pub menu_hover: Color,
    pub video_mode: bool,
    pub source: Option<PathBuf>,
}

impl Skin {
    pub fn id(&self) -> String {
        match &self.source {
            Some(path) => path.to_string_lossy().to_string(),
            None => self.name.clone(),
        }
    }
}

pub const BUILTIN_SKINS: &[(&str, &str, &str, &str)] = &[
    ("默认皮肤", "4aa3ff", "444444", "1c1c1c"),
    ("暗黑皮肤", "9a9a9a", "2b2b2b", "141414"),
    ("琥珀皮肤", "e5a100", "3d342a", "241d14"),
    ("青绿皮肤", "2fbf9f", "33413d", "1b2623"),
];

pub const SKIN_EXTENSION: &str = "fskin";

pub fn skin_dir() -> PathBuf {
    flint_core::config::config_dir().join("skins")
}

pub fn builtin_skins() -> Vec<Skin> {
    BUILTIN_SKINS
        .iter()
        .map(|(name, accent, menu_bg, menu_hover)| Skin {
            name: (*name).to_string(),
            accent: parse_color(accent).unwrap_or(Color::from_rgb_u8(0x4a, 0xa3, 0xff)),
            menu_bg: parse_color(menu_bg).unwrap_or(Color::from_rgb_u8(0x44, 0x44, 0x44)),
            menu_hover: parse_color(menu_hover).unwrap_or(Color::from_rgb_u8(0x1c, 0x1c, 0x1c)),
            video_mode: false,
            source: None,
        })
        .collect()
}

pub fn external_skins(dir: &Path) -> Vec<Skin> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skins: Vec<Skin> = read
        .flatten()
        .map(|item| item.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case(SKIN_EXTENSION))
                .unwrap_or(false)
        })
        .filter_map(|path| load_skin(&path))
        .collect();
    skins.sort_by_key(|skin| skin.name.to_lowercase());
    skins
}

pub fn load_library() -> Vec<Skin> {
    let mut skins = builtin_skins();
    skins.extend(external_skins(&skin_dir()));
    skins
}

pub fn load_skin(path: &Path) -> Option<Skin> {
    let text = fs::read_to_string(path).ok()?;
    let mut name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "外部皮肤".to_string());
    let defaults = builtin_skins();
    let mut skin = Skin {
        name: name.clone(),
        accent: defaults[0].accent,
        menu_bg: defaults[0].menu_bg,
        menu_hover: defaults[0].menu_hover,
        video_mode: false,
        source: Some(path.to_path_buf()),
    };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "name" if !value.is_empty() => name = value.to_string(),
            "accent" => {
                if let Some(color) = parse_color(value) {
                    skin.accent = color;
                }
            }
            "menu_bg" | "menu-background" => {
                if let Some(color) = parse_color(value) {
                    skin.menu_bg = color;
                }
            }
            "menu_hover" | "menu-highlight" => {
                if let Some(color) = parse_color(value) {
                    skin.menu_hover = color;
                }
            }
            "mode" | "interface-mode" => {
                skin.video_mode = value.eq_ignore_ascii_case("video");
            }
            _ => {}
        }
    }
    skin.name = name;
    Some(skin)
}

pub fn parse_color(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let hex = &hex[hex.len() - 6..];
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb_u8(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

pub fn find_skin<'a>(skins: &'a [Skin], id: &str) -> Option<&'a Skin> {
    skins.iter().find(|skin| skin.id() == id).or_else(|| {
        let path = Path::new(id);
        path.is_file()
            .then(|| {
                skins
                    .iter()
                    .find(|skin| skin.source.as_deref() == Some(path))
            })
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skin(dir: &Path, file: &str, body: &str) -> PathBuf {
        let path = dir.join(file);
        fs::write(&path, body).unwrap();
        path
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flint-skin-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_colors_with_and_without_hash() {
        assert_eq!(
            parse_color("#112233").unwrap(),
            Color::from_rgb_u8(0x11, 0x22, 0x33)
        );
        assert_eq!(
            parse_color("112233").unwrap(),
            Color::from_rgb_u8(0x11, 0x22, 0x33)
        );
        assert_eq!(
            parse_color("#ff112233").unwrap(),
            Color::from_rgb_u8(0x11, 0x22, 0x33)
        );
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn loads_skin_keys_and_defaults() {
        let dir = scratch("load");
        let path = write_skin(
            &dir,
            "ocean.fskin",
            "name=海洋\naccent=#123456\nmenu_bg=#202020\nmenu_hover=#101010\nignored=1\n",
        );
        let skin = load_skin(&path).unwrap();
        assert_eq!(skin.name, "海洋");
        assert_eq!(skin.accent, Color::from_rgb_u8(0x12, 0x34, 0x56));
        assert_eq!(skin.menu_bg, Color::from_rgb_u8(0x20, 0x20, 0x20));
        assert_eq!(skin.menu_hover, Color::from_rgb_u8(0x10, 0x10, 0x10));
        assert_eq!(skin.source.as_deref(), Some(path.as_path()));

        let fallback = write_skin(&dir, "plain.fskin", "accent=bad\n");
        let skin = load_skin(&fallback).unwrap();
        assert_eq!(skin.name, "plain");
        assert_eq!(skin.accent, builtin_skins()[0].accent);
        assert!(load_skin(&dir.join("missing.fskin")).is_none());
    }

    #[test]
    fn scans_external_skins_only_from_extension() {
        let dir = scratch("scan");
        write_skin(&dir, "b.fskin", "name=B\n");
        write_skin(&dir, "a.FSKIN", "name=A\n");
        write_skin(&dir, "notes.txt", "name=ignored\n");
        fs::create_dir_all(dir.join("nested.fskin")).unwrap();
        let skins = external_skins(&dir);
        let names: Vec<&str> = skins.iter().map(|skin| skin.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
        assert!(external_skins(&dir.join("absent")).is_empty());
    }

    #[test]
    fn finds_skins_by_name_and_path() {
        let dir = scratch("find");
        let path = write_skin(&dir, "custom.fskin", "name=自定义\n");
        let mut library = builtin_skins();
        library.extend(external_skins(&dir));
        assert_eq!(find_skin(&library, "默认皮肤").unwrap().name, "默认皮肤");
        assert_eq!(
            find_skin(&library, &path.to_string_lossy()).unwrap().name,
            "自定义"
        );
        assert!(find_skin(&library, "不存在").is_none());
    }
}
