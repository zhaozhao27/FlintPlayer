use super::MenuEntry;

pub const PEAKS: [u32; 8] = [0, 100, 200, 300, 500, 1000, 1500, 2000];

pub fn pixel_shaders() -> Vec<MenuEntry> {
    let mut items = vec![
        MenuEntry::action("调整尺寸前的着色切换", "pixel-shader-cycle").with_shortcut("Shift+P"),
        MenuEntry::action("    不使用", "pixel-shader:"),
    ];
    items.extend(crate::shaders::SHADERS.iter().map(|spec| {
        MenuEntry::action(
            format!("    {}", spec.label),
            format!("pixel-shader:{}", spec.key),
        )
    }));
    items.extend([
        MenuEntry::action("    组合", "").disabled(),
        MenuEntry::action("    组合编辑...", "").disabled(),
        MenuEntry::action("调整尺寸后的着色切换", "")
            .with_shortcut("Ctrl+Alt+P")
            .disabled(),
        MenuEntry::action("    不使用", "").disabled(),
        MenuEntry::action("    组合", "").disabled(),
        MenuEntry::action("    组合编辑...", "").disabled(),
        MenuEntry::action("重载着色文件", "").disabled(),
        MenuEntry::action("打开着色文件夹", "").disabled(),
        MenuEntry::action("显示器色域校正(ICC)", "").disabled(),
        MenuEntry::action("YCgCo 色彩空间自动校正", "").disabled(),
        MenuEntry::action("HDR SMPTE ST 2084 自动校正", "hdr-tone-map-toggle"),
        MenuEntry::action("HDR SMPTE ST 2086 自动校正", "hdr-metadata-toggle"),
    ]);
    items.extend(hdr_curves());
    items.extend(peak_presets());
    items
}

pub fn hdr_curves() -> Vec<MenuEntry> {
    vec![MenuEntry::submenu(
        "色调映射类型：",
        crate::hdr::CURVES
            .iter()
            .map(|(key, _)| {
                MenuEntry::action(crate::hdr::label(key), format!("tone-mapping:{key}"))
            })
            .collect(),
    )]
}

pub fn peak_presets() -> Vec<MenuEntry> {
    vec![MenuEntry::submenu(
        "源峰值亮度(尼特)：",
        PEAKS
            .iter()
            .map(|peak| {
                let label = if *peak == 0 {
                    "自动".to_string()
                } else {
                    format!("{peak} 尼特")
                };
                MenuEntry::action(label, format!("peak:{peak}"))
            })
            .collect(),
    )]
}

pub fn ass_engines() -> Vec<MenuEntry> {
    vec![MenuEntry::submenu(
        "ASS/SSA 输出引擎：",
        crate::subtitle_engine::MODES
            .iter()
            .map(|(key, label, _)| MenuEntry::action(*label, format!("ass-engine:{key}")))
            .collect(),
    )]
}
