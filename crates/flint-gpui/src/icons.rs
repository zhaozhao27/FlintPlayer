use gpui::*;

pub const PLAY: u8 = 0;
pub const PAUSE: u8 = 1;
pub const STOP: u8 = 2;
pub const NEXT: u8 = 3;
pub const PREV: u8 = 4;
pub const RATIO: u8 = 6;
pub const MENU: u8 = 9;
pub const VOLUME: u8 = 10;
pub const VOLUME_MUTED: u8 = 11;
pub const MINIMIZE: u8 = 13;
pub const MAXIMIZE: u8 = 14;
pub const CLOSE: u8 = 17;
pub const CHEVRON_DOWN: u8 = 24;
pub const SIDEBAR: u8 = 27;
pub const PIN: u8 = 28;
pub const OPEN: u8 = 30;
pub const SETTINGS: u8 = 32;

fn bytes(kind: u8) -> &'static [u8] {
    match kind {
        0 => include_bytes!("../assets/icons/kind00.svg"),
        1 => include_bytes!("../assets/icons/kind01.svg"),
        2 => include_bytes!("../assets/icons/kind02.svg"),
        3 => include_bytes!("../assets/icons/kind03.svg"),
        4 => include_bytes!("../assets/icons/kind04.svg"),
        6 => include_bytes!("../assets/icons/kind06.svg"),
        9 => include_bytes!("../assets/icons/kind09.svg"),
        10 => include_bytes!("../assets/icons/kind10.svg"),
        11 => include_bytes!("../assets/icons/kind11.svg"),
        13 => include_bytes!("../assets/icons/kind13.svg"),
        14 => include_bytes!("../assets/icons/kind14.svg"),
        17 => include_bytes!("../assets/icons/kind17.svg"),
        24 => include_bytes!("../assets/icons/kind24.svg"),
        27 => include_bytes!("../assets/icons/kind27.svg"),
        28 => include_bytes!("../assets/icons/kind28.svg"),
        30 => include_bytes!("../assets/icons/kind30.svg"),
        31 => include_bytes!("../assets/icons/kind31.svg"),
        32 => include_bytes!("../assets/icons/kind32.svg"),
        _ => include_bytes!("../assets/icons/kind00.svg"),
    }
}

pub fn icon(kind: u8, size: f32, tint: u32) -> Svg {
    svg()
        .data(bytes(kind))
        .w(px(size))
        .h(px(size))
        .text_color(rgb(tint))
}
