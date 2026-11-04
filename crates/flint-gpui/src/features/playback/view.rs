use gpui::*;

use super::controller::PlaybackController;
use crate::features::player_frame::PlayerFrameController;
use crate::theme;

pub fn video_canvas(
    playback: &Entity<PlaybackController>,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let (texture, size, error, aspect) = {
        let playback = playback.read(cx);
        (
            playback.texture(),
            playback.texture_size(),
            playback.error().map(str::to_string),
            playback.aspect().to_string(),
        )
    };

    if let (Some(texture), Some((width, height))) = (texture, size) {
        let ratio = aspect_ratio(&aspect);
        let stretch = aspect == "0";
        return canvas(
            move |bounds, _, _| {
                if stretch {
                    bounds
                } else {
                    fit_bounds(bounds, width, height, ratio)
                }
            },
            move |_bounds, image_bounds, window, _| {
                let _ = window.paint_dynamic_texture(image_bounds, texture);
            },
        )
        .size_full()
        .into_any_element();
    }

    let message = error.unwrap_or_else(|| "拖入媒体文件即可播放".to_string());
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(theme::TITLEBAR_TEXT_DIM))
        .child(crate::i18n::tr(&message))
        .into_any_element()
}

fn fit_bounds(
    bounds: Bounds<Pixels>,
    width: u32,
    height: u32,
    ratio: Option<f32>,
) -> Bounds<Pixels> {
    let available_width = f32::from(bounds.size.width);
    let available_height = f32::from(bounds.size.height);
    if width == 0 || height == 0 || available_width <= 0.0 || available_height <= 0.0 {
        return bounds;
    }
    let (fit_width, fit_height) = match ratio {
        Some(ratio) => {
            let scale = (available_width / ratio).min(available_height);
            (ratio * scale, scale)
        }
        None => {
            let scale = (available_width / width as f32).min(available_height / height as f32);
            (width as f32 * scale, height as f32 * scale)
        }
    };
    Bounds::new(
        point(
            bounds.origin.x + px((available_width - fit_width) / 2.0),
            bounds.origin.y + px((available_height - fit_height) / 2.0),
        ),
        Size::new(px(fit_width), px(fit_height)),
    )
}

fn aspect_ratio(value: &str) -> Option<f32> {
    if value.is_empty() || value == "no" || value == "0" {
        return None;
    }
    let (numerator, denominator) = value.split_once(':')?;
    let numerator = numerator.parse::<f32>().ok()?;
    let denominator = denominator.parse::<f32>().ok()?;
    if numerator <= 0.0 || denominator <= 0.0 {
        return None;
    }
    Some(numerator / denominator)
}
