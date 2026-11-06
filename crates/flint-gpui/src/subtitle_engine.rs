pub const MODES: [(&'static str, &'static str, &'static str); 3] = [
    ("file", "使用字幕所含样式", "no"),
    ("basic", "仅使用基本(默认)样式", "yes"),
    ("custom", "使用用户自定义的样式", "force"),
];

pub fn normalize(key: &str) -> &'static str {
    match key {
        "file" => "file",
        "custom" => "custom",
        _ => "basic",
    }
}

pub fn mpv_value(key: &str) -> &'static str {
    match normalize(key) {
        "file" => "no",
        "custom" => "force",
        _ => "yes",
    }
}

pub fn label(key: &str) -> &'static str {
    match normalize(key) {
        "file" => MODES[0].1,
        "custom" => MODES[2].1,
        _ => MODES[1].1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_known_modes() {
        assert_eq!(normalize("file"), "file");
        assert_eq!(normalize("basic"), "basic");
        assert_eq!(normalize("custom"), "custom");
    }

    #[test]
    fn unknown_modes_fall_back_to_basic() {
        assert_eq!(normalize(""), "basic");
        assert_eq!(normalize("bogus"), "basic");
    }

    #[test]
    fn maps_modes_to_sub_ass_override_values() {
        assert_eq!(mpv_value("file"), "no");
        assert_eq!(mpv_value("basic"), "yes");
        assert_eq!(mpv_value("custom"), "force");
    }
}
