use std::path::Path;

use gpui::*;

use super::controller::PlaylistController;
use crate::features::player_frame::PlayerFrameController;
use crate::theme;

pub const PLAYLIST_WIDTH: f32 = 280.0;
const HEADER_HEIGHT: f32 = 30.0;
const ROW_HEIGHT: f32 = 21.0;

pub fn playlist_panel(
    playlist: &Entity<PlaylistController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let (items, current, count) = {
        let playlist = playlist.read(cx);
        let items: Vec<(usize, String)> = playlist
            .items()
            .iter()
            .enumerate()
            .map(|(index, path)| (index, display_name(path)))
            .collect();
        (items, playlist.current(), playlist.len())
    };

    let header = div()
        .h(px(HEADER_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .px_2()
        .bg(rgb(0x252525))
        .border_b_1()
        .border_color(rgb(theme::PANEL_BORDER))
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(rgb(theme::TITLEBAR_TEXT))
                .child(format!("播放列表 · {count}")),
        )
        .child(
            div()
                .id("playlist-close")
                .w(px(22.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .text_color(rgb(theme::TITLEBAR_TEXT_DIM))
                .child("✕")
                .hover(|style| style.bg(rgb(theme::CONTROL_BAR_HOVER)))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
        );

    let rows: Vec<AnyElement> = if items.is_empty() {
        vec![div()
            .px_3()
            .pt_3()
            .text_size(px(12.0))
            .text_color(rgb(theme::MENU_TEXT_DIM))
            .child("播放列表为空")
            .into_any_element()]
    } else {
        items
            .into_iter()
            .map(|(index, name)| {
                let selected = current == Some(index);
                let background = if selected {
                    theme::MENU_HOVER
                } else {
                    0x2d2d2d
                };
                div()
                    .id(("playlist-row", index))
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .gap_2()
                    .bg(rgb(background))
                    .text_size(px(12.0))
                    .text_color(rgb(if selected {
                        theme::ACCENT
                    } else {
                        theme::MENU_TEXT
                    }))
                    .hover(|style| style.bg(rgb(theme::TITLEBAR_HOVER)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_playlist_index(index, cx);
                    }))
                    .child(
                        div()
                            .w(px(30.0))
                            .flex_shrink_0()
                            .text_color(rgb(theme::MENU_TEXT_DIM))
                            .child(format!("{index:02}")),
                    )
                    .child(div().flex_1().truncate().child(name))
                    .into_any_element()
            })
            .collect()
    };

    div()
        .w(px(PLAYLIST_WIDTH))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(rgb(0x2d2d2d))
        .border_l_1()
        .border_color(rgb(theme::TITLEBAR_BG))
        .child(header)
        .child(
            div()
                .id("playlist-scroll")
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .children(rows),
        )
        .into_any_element()
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}
