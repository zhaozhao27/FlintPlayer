use gpui::*;

use super::controller::CaptureController;
use crate::features::player_frame::PlayerFrameController;
use crate::theme;

pub fn video_canvas(
    capture: &Entity<CaptureController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let (texture, size, error) = {
        let capture = capture.read(cx);
        (
            capture.texture(),
            capture.texture_size(),
            capture.error().map(str::to_string),
        )
    };

    if let (Some(texture), Some((width, height))) = (texture, size) {
        return canvas(
            move |bounds, _, _| fit_bounds(bounds, width, height),
            move |_bounds, image_bounds, window, _| {
                let _ = window.paint_dynamic_texture(image_bounds, texture);
            },
        )
        .size_full()
        .into_any_element();
    }

    let message = error.unwrap_or_else(|| "正在打开采集设备...".to_string());
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(theme::TITLEBAR_TEXT_DIM))
        .child(message)
        .into_any_element()
}

fn fit_bounds(bounds: Bounds<Pixels>, width: u32, height: u32) -> Bounds<Pixels> {
    let available_width = f32::from(bounds.size.width);
    let available_height = f32::from(bounds.size.height);
    if width == 0 || height == 0 || available_width <= 0.0 || available_height <= 0.0 {
        return bounds;
    }
    let scale = (available_width / width as f32).min(available_height / height as f32);
    let fit_width = width as f32 * scale;
    let fit_height = height as f32 * scale;
    Bounds::new(
        point(
            bounds.origin.x + px((available_width - fit_width) / 2.0),
            bounds.origin.y + px((available_height - fit_height) / 2.0),
        ),
        Size::new(px(fit_width), px(fit_height)),
    )
}
