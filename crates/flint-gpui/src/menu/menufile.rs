use std::path::{Path, PathBuf};

use roxmltree::Node;

use super::{generated, EntryKind, MenuEntry};

pub const MENU_FILE: &str = "menu.xml";
const INDENT: &str = "  ";

pub fn file_path() -> PathBuf {
    flint_core::config::config_dir().join(MENU_FILE)
}

pub fn ensure_file() {
    let path = file_path();
    if path.exists() {
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(parent) {
        log::warn!("无法创建菜单目录 {}：{error}", parent.display());
        return;
    }
    if let Err(error) = std::fs::write(&path, render(&super::catalog::main_menu())) {
        log::warn!("无法写入菜单文件 {}：{error}", path.display());
    }
}

pub fn load(path: &Path) -> Option<Vec<MenuEntry>> {
    let text = std::fs::read_to_string(path).ok()?;
    match parse(&text) {
        Ok(entries) if !entries.is_empty() => Some(entries),
        Ok(_) => {
            log::warn!("菜单文件没有可用项：{}", path.display());
            None
        }
        Err(error) => {
            log::warn!("菜单文件解析失败 {}：{error}", path.display());
            None
        }
    }
}

pub fn parse(text: &str) -> Result<Vec<MenuEntry>, roxmltree::Error> {
    let document = roxmltree::Document::parse(text)?;
    Ok(child_entries(document.root_element()))
}

fn child_entries(node: Node<'_, '_>) -> Vec<MenuEntry> {
    let mut entries = Vec::new();
    for child in node.children().filter(|child| child.is_element()) {
        entries.extend(entry_from(child));
    }
    entries
}

fn entry_from(node: Node<'_, '_>) -> Vec<MenuEntry> {
    match node.tag_name().name() {
        "action" => action_from(node).into_iter().collect(),
        "submenu" => submenu_from(node).into_iter().collect(),
        "separator" => vec![MenuEntry::separator()],
        "generated" => generated_from(node),
        other => {
            log::warn!("忽略未知菜单节点 <{other}>");
            Vec::new()
        }
    }
}

fn action_from(node: Node<'_, '_>) -> Option<MenuEntry> {
    let title = node.attribute("title").unwrap_or_default();
    if title.is_empty() {
        log::warn!("忽略缺少 title 的动作项");
        return None;
    }
    let mut entry = MenuEntry::action(title, node.attribute("cmd").unwrap_or_default());
    entry.shortcut = text_attr(node, "shortcut");
    entry.icon = text_attr(node, "icon");
    entry.checked = flag_attr(node, "checked");
    if flag_attr(node, "disabled") == Some(true) {
        entry.enabled = false;
    }
    Some(entry)
}

fn submenu_from(node: Node<'_, '_>) -> Option<MenuEntry> {
    let title = node.attribute("title").unwrap_or_default();
    if title.is_empty() {
        log::warn!("忽略缺少 title 的子菜单");
        return None;
    }
    let mut entry = MenuEntry::submenu(title, child_entries(node));
    entry.dynamic = text_attr(node, "dynamic");
    if flag_attr(node, "disabled") == Some(true) {
        entry.enabled = false;
    }
    Some(entry)
}

fn generated_from(node: Node<'_, '_>) -> Vec<MenuEntry> {
    let Some(name) = node.attribute("name") else {
        log::warn!("忽略缺少 name 的生成器节点");
        return Vec::new();
    };
    match name {
        "pixel-shaders" => generated::pixel_shaders(),
        "hdr-curves" => generated::hdr_curves(),
        "peak-presets" => generated::peak_presets(),
        "ass-engines" => generated::ass_engines(),
        other => {
            log::warn!("忽略未知菜单生成器 {other}");
            Vec::new()
        }
    }
}

fn text_attr(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn flag_attr(node: Node<'_, '_>, name: &str) -> Option<bool> {
    node.attribute(name).map(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        )
    })
}

pub fn render(entries: &[MenuEntry]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<menu>\n");
    for entry in entries {
        render_entry(entry, 1, &mut out);
    }
    out.push_str("</menu>\n");
    out
}

fn render_entry(entry: &MenuEntry, depth: usize, out: &mut String) {
    let indent = INDENT.repeat(depth);
    match entry.kind {
        EntryKind::Separator => {
            out.push_str(&indent);
            out.push_str("<separator/>\n");
        }
        EntryKind::Action => {
            out.push_str(&indent);
            out.push_str("<action");
            attribute(out, "title", &entry.title);
            if let Some(action) = entry.action.as_deref().filter(|value| !value.is_empty()) {
                attribute(out, "cmd", action);
            }
            if let Some(shortcut) = entry.shortcut.as_deref().filter(|value| !value.is_empty()) {
                attribute(out, "shortcut", shortcut);
            }
            if let Some(icon) = entry.icon.as_deref().filter(|value| !value.is_empty()) {
                attribute(out, "icon", icon);
            }
            if let Some(checked) = entry.checked {
                out.push_str(if checked {
                    " checked=\"true\""
                } else {
                    " checked=\"false\""
                });
            }
            if !entry.enabled {
                out.push_str(" disabled=\"true\"");
            }
            out.push_str("/>\n");
        }
        EntryKind::Submenu => {
            out.push_str(&indent);
            out.push_str("<submenu");
            attribute(out, "title", &entry.title);
            if let Some(name) = entry.dynamic.as_deref().filter(|value| !value.is_empty()) {
                attribute(out, "dynamic", name);
            }
            if !entry.enabled {
                out.push_str(" disabled=\"true\"");
            }
            let generated = entry.generated.as_deref().filter(|value| !value.is_empty());
            if generated.is_none() && entry.children.is_empty() {
                out.push_str("/>\n");
                return;
            }
            out.push_str(">\n");
            match generated {
                Some(name) => {
                    out.push_str(&INDENT.repeat(depth + 1));
                    out.push_str("<generated");
                    attribute(out, "name", name);
                    out.push_str("/>\n");
                }
                None => {
                    for child in &entry.children {
                        render_entry(child, depth + 1, out);
                    }
                }
            }
            out.push_str(&indent);
            out.push_str("</submenu>\n");
        }
    }
}

fn attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&escape(value));
    out.push('"');
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{parse, render};
    use crate::menu::MenuEntry;

    #[test]
    fn parses_actions_submenus_and_separators() {
        let entries = parse(
            r#"<menu>
                 <action title="打开文件..." cmd="open-file" shortcut="Ctrl+O"/>
                 <separator/>
                 <submenu title="播放" dynamic="play">
                   <action title="暂停" cmd="play-pause" checked="true"/>
                   <action title="占位" disabled="true"/>
                 </submenu>
               </menu>"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].action.as_deref(), Some("open-file"));
        assert_eq!(entries[0].shortcut.as_deref(), Some("Ctrl+O"));
        assert!(entries[1].is_separator());
        assert_eq!(entries[2].dynamic.as_deref(), Some("play"));
        assert_eq!(entries[2].children.len(), 2);
        assert_eq!(entries[2].children[0].checked, Some(true));
        assert!(!entries[2].children[1].enabled);
    }

    #[test]
    fn expands_generated_nodes() {
        let entries = parse(r#"<menu><submenu title="像素着色"><generated name="pixel-shaders"/></submenu></menu>"#).unwrap();
        assert!(entries[0].children.iter().any(|entry| entry.action.as_deref() == Some("pixel-shader-cycle")));
        let curves = parse(r#"<menu><generated name="hdr-curves"/></menu>"#).unwrap();
        assert_eq!(curves[0].title, "色调映射类型：");
        assert_eq!(curves[0].children.len(), crate::hdr::CURVES.len());
    }

    #[test]
    fn skips_unknown_and_malformed_nodes() {
        let entries = parse(r#"<menu><bogus/><action/><generated name="nope"/><action title="ok" cmd="x"/></menu>"#).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "ok");
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(parse("<menu><action title=\"x\"></menu>").is_err());
    }

    #[test]
    fn render_round_trips_through_parser() {
        let original = vec![
            MenuEntry::action("打开文件...", "open-file").with_shortcut("Ctrl+O"),
            MenuEntry::separator(),
            MenuEntry::submenu("播放", Vec::new()).dynamic("play").disabled(),
            MenuEntry::submenu("字幕", vec![MenuEntry::action("a & b", "x")])
                .generated("ass-engines"),
        ];
        let text = render(&original);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].title, "打开文件...");
        assert_eq!(parsed[0].shortcut.as_deref(), Some("Ctrl+O"));
        assert!(parsed[1].is_separator());
        assert_eq!(parsed[2].dynamic.as_deref(), Some("play"));
        assert!(!parsed[2].enabled);
        assert_eq!(parsed[3].title, "字幕");
        assert_eq!(parsed[3].children[0].title, "ASS/SSA 输出引擎：");
    }

    #[test]
    fn escapes_attribute_values() {
        let original = vec![MenuEntry::action("a & b \"c\" <d>", "x")];
        let text = render(&original);
        assert!(text.contains("title=\"a &amp; b &quot;c&quot; &lt;d&gt;\""));
        assert_eq!(parse(&text).unwrap()[0].title, "a & b \"c\" <d>");
    }

    #[test]
    fn default_menu_renders_and_parses() {
        let entries = crate::menu::main_menu();
        let text = render(&entries);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.len(), entries.len());
        assert_eq!(parsed[0].title, entries[0].title);
        assert!(text.contains("<generated name=\"pixel-shaders\"/>"));
        assert!(text.contains("dynamic=\"play\""));
    }
}
