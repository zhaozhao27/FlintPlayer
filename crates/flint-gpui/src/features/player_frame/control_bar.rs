use std::cell::Cell;
use std::rc::Rc;

use gpui::*;

use super::controller::{PlayerFrameController, ToolbarMenu};
use crate::icons::{self, icon};
use crate::i18n::tr;
use crate::theme;

pub fn control_bar(
    frame: &PlayerFrameController,
    capturing: bool,
    width: f32,
    height: f32,
    cx: &mut Context<PlayerFrameController>,
) -> AnyElement {
    let (
        running,
        paused,
        position,
        duration,
        volume,
        muted,
        chapters,
        bookmarks,
        codec,
        size,
        rate,
        bitrate,
    ) = {
        let playback = frame.playback.read(cx);
        let state = playback.state();
        let capture = frame.capture.read(cx);
        let running = if capturing {
            capture.is_active()
        } else {
            state.loaded
        };
        let paused = !capturing && state.paused;
        let duration = if capturing { 0.0 } else { state.duration };
        let position = if capturing { 0.0 } else { state.position };
        let volume = if capturing {
            1.0f32
        } else {
            (state.volume / 100.0).clamp(0.0, 1.0) as f32
        };
        let muted = !capturing && state.volume <= 0.0;
        let chapters = if capturing || duration <= 0.0 {
            Vec::new()
        } else {
            playback
                .chapters()
                .iter()
                .filter(|chapter| chapter.start > 0.0)
                .map(|chapter| (chapter.start / duration).clamp(0.0, 1.0) as f32)
                .collect()
        };
        let current = {
            let playlist = frame.playlist.read(cx);
            playlist
                .current()
                .and_then(|index| playlist.items().get(index).cloned())
        };
        let bookmarks = if capturing || duration <= 0.0 {
            Vec::new()
        } else {
            let key = current
                .as_ref()
                .map(|path| path.to_string_lossy().to_string());
            key.and_then(|key| {
                frame
                    .config
                    .bookmarks
                    .iter()
                    .find(|(file, _)| file == &key)
                    .map(|(_, times)| {
                        times
                            .iter()
                            .map(|time| (time / duration).clamp(0.0, 1.0) as f32)
                            .collect()
                    })
            })
            .unwrap_or_default()
        };
        let (codec, size, rate, bitrate) = if capturing {
            match capture.info() {
                Some(info) => (
                    info.codec.clone(),
                    format!("{}×{}", info.width, info.height),
                    if info.nominal_fps > 0.0 {
                        format!("{:.2}", info.nominal_fps)
                    } else {
                        String::new()
                    },
                    String::new(),
                ),
                None => (String::new(), String::new(), String::new(), String::new()),
            }
        } else {
            let video_track = playback
                .tracks()
                .into_iter()
                .find(|track| track.kind == "video" && track.selected);
            let codec = video_track
                .as_ref()
                .map(|track| track.codec.to_ascii_uppercase())
                .unwrap_or_default();
            let size = if state.width > 0 && state.height > 0 {
                format!("{}×{}", state.width, state.height)
            } else {
                String::new()
            };
            let rate = video_track
                .as_ref()
                .filter(|track| track.fps > 0.0)
                .map(|track| format!("{:.2}", track.fps))
                .unwrap_or_default();
            let bitrate = video_track
                .as_ref()
                .filter(|track| track.bitrate > 0.0)
                .map(|track| format!("{:.2}M", track.bitrate / 1_000_000.0))
                .unwrap_or_default();
            (codec, size, rate, bitrate)
        };
        (
            running,
            paused,
            position,
            duration,
            volume,
            muted,
            chapters,
            bookmarks,
            codec,
            size,
            rate,
            bitrate,
        )
    };

    let (analysis, frame_position, frame_total) = {
        let playback = frame.playback.read(cx);
        if !capturing && playback.analysis_mode() {
            (
                true,
                playback.frame_number(),
                frame.analysis_frame_total(&playback),
            )
        } else {
            (false, 0, None)
        }
    };
    let frame_fraction = frame_total
        .map(|total| (frame_position as f32 / (total - 1) as f32).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let fraction = if analysis {
        frame.analysis_seek.unwrap_or(frame_fraction)
    } else if duration > 0.0 {
        (position / duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let seek_width = (width - 96.0).max(0.0);

    let seek_bounds = frame.playback.read(cx).progress_bounds();
    let seek_down_bounds = seek_bounds.clone();
    let seek_move_bounds = seek_bounds.clone();
    let seek_drag_bounds = seek_bounds.clone();
    let analysis_down = analysis;
    let analysis_move = analysis;

    let mut seek_area = div()
        .id("control-seek")
        .relative()
        .w(px(seek_width))
        .h_full()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let fraction = seek_fraction(event.position, seek_down_bounds.get());
                if analysis_down {
                    this.begin_analysis_seek(fraction as f32, cx);
                } else {
                    this.playback
                        .update(cx, |playback, cx| playback.seek_fraction(fraction, cx));
                }
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if this.volume_drag.get() || event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let fraction = seek_fraction(event.position, seek_move_bounds.get());
            if analysis_move {
                this.drag_analysis_seek(fraction as f32, cx);
            } else {
                this.playback
                    .update(cx, |playback, cx| playback.seek_fraction(fraction, cx));
            }
        }))
        .child(
            canvas(
                move |bounds, _, _| seek_bounds.set(bounds),
                |_, _, _, _| {},
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(9.0))
                .w(px(seek_width))
                .h(px(2.0))
                .bg(rgb(theme::bar_track())),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(9.0))
                .w(px(seek_width * fraction))
                .h(px(2.0))
                .bg(rgb(theme::bar_fill())),
        );

    for marker in chapters {
        seek_area = seek_area.child(
            div()
                .absolute()
                .left(px(seek_width * marker - 1.0))
                .top(px(5.0))
                .w(px(2.0))
                .h(px(10.0))
                .bg(rgb(theme::bar_chapter())),
        );
    }
    for marker in bookmarks {
        seek_area = seek_area.child(
            div()
                .absolute()
                .left(px(seek_width * marker - 1.0))
                .top(px(4.0))
                .w(px(3.0))
                .h(px(12.0))
                .bg(rgb(theme::bar_bookmark())),
        );
    }
    seek_area = seek_area.child(
        div()
            .absolute()
            .left(px(seek_width * fraction - 5.0))
            .top(px(5.0))
            .w(px(10.0))
            .h(px(10.0))
            .rounded(px(5.0))
            .bg(rgb(theme::bar_thumb())),
    );

    let mute_area = div()
        .id("control-mute")
        .w(px(20.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| {
            this.playback
                .update(cx, |playback, cx| playback.toggle_mute(cx));
        }))
        .child(icon(
            if muted {
                icons::VOLUME_MUTED
            } else {
                icons::VOLUME
            },
            13.0,
            theme::control_bar_text(),
        ));

    let volume_bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let volume_down_bounds = volume_bounds.clone();

    let volume_area = div()
        .id("control-volume")
        .cursor_pointer()
        .relative()
        .w(px(48.0))
        .h_full()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.volume_drag.set(true);
                if let Some(fraction) = pointer_fraction(event.position, volume_down_bounds.get()) {
                    this.playback.update(cx, |playback, cx| {
                        playback.set_volume(fraction as f64 * 100.0, cx)
                    });
                }
            }),
        )
        .child(
            canvas(
                {
                    let tracked = volume_bounds.clone();
                    move |bounds, _, _| tracked.set(bounds)
                },
                |_, _, _, _| {},
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(9.0))
                .w(px(48.0))
                .h(px(2.0))
                .bg(rgb(theme::bar_track())),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(9.0))
                .w(px(48.0 * volume))
                .h(px(2.0))
                .bg(rgb(theme::bar_fill())),
        )
        .child(
            div()
                .absolute()
                .left(px((48.0 * volume - 4.0).clamp(0.0, 40.0)))
                .top(px(6.0))
                .w(px(8.0))
                .h(px(8.0))
                .rounded(px(4.0))
                .bg(rgb(theme::bar_thumb())),
        );

    let play_pause = bar_button(
        "bar-play",
        if paused { icons::PLAY } else { icons::PAUSE },
        running,
    )
    .on_click(cx.listener(move |this, _, _, cx| {
        if !running {
            return;
        }
        this.playback
            .update(cx, |playback, cx| playback.toggle_pause(cx));
    }));

    let stop = bar_button("bar-stop", icons::STOP, running).on_click(cx.listener(
        move |this, _, _, cx| {
            if !running {
                return;
            }
            if capturing {
                this.stop_capture(cx);
            } else {
                this.playback.update(cx, |playback, cx| playback.stop(cx));
            }
        },
    ));

    let previous = bar_button("bar-prev", icons::PREV, true).on_click(
        cx.listener(|this, _, _, cx| this.handle_action("prev-panel".to_string(), cx)),
    );
    let next = bar_button("bar-next", icons::NEXT, true)
        .on_click(cx.listener(|this, _, _, cx| this.handle_action("next-panel".to_string(), cx)));
    let open = bar_button("bar-open", icons::OPEN, true)
        .on_click(cx.listener(|this, _, _, cx| this.handle_action("open-file".to_string(), cx)));

    let (elapsed, duration_text) = if analysis {
        let shown = frame_total
            .map(|total| (fraction * (total - 1) as f32).round() as i64)
            .unwrap_or(frame_position);
        let last = frame_total
            .map(|total| (total - 1).to_string())
            .unwrap_or_else(|| "--".to_string());
        (format!("{} {}", tr("帧号"), shown), last)
    } else {
        (format_clock(position), format_clock(duration))
    };

    let mut row = div()
        .absolute()
        .left_0()
        .top(px(20.0))
        .w_full()
        .h(px(42.0))
        .pr(px(4.0))
        .flex()
        .items_center()
        .child(play_pause)
        .child(stop)
        .child(previous)
        .child(next)
        .child(open)
        .child(div().w(px(10.0)).flex_shrink_0())
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(12.0))
                .text_color(rgb(theme::control_bar_text()))
                .child(format!("{elapsed} / {duration_text}")),
        )
        .child(div().w(px(12.0)).flex_shrink_0())
        .child(chip("chip-sw", tr("S/W"), 34.0))
        .child(div().w(px(4.0)).flex_shrink_0());

    if !codec.is_empty() {
        row = row.child(chip("chip-codec", codec, 46.0));
    }
    if !size.is_empty() && width > 600.0 {
        row = row.child(chip("chip-size", size, 74.0));
    }
    if !rate.is_empty() && width > 660.0 {
        row = row.child(chip("chip-rate", rate, 52.0));
    }
    if !bitrate.is_empty() && width > 740.0 {
        row = row.child(chip("chip-bitrate", bitrate, 62.0));
    }

    let sidebar_tint = if frame.is_sidebar_open() {
        theme::bar_sidebar_active()
    } else {
        theme::bar_cluster_tint()
    };

    row = row
        .child(div().flex_1())
        .child(
            cluster_label("cluster-360", "360°")
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme::control_bar_hover())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_toolbar_menu(
                        ToolbarMenu::Panorama,
                        width - 196.0,
                        height - theme::control_bar_height() - 4.0,
                        cx,
                    );
                })),
        )
        .child(
            cluster_label("cluster-3d", "3D")
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme::control_bar_hover())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_toolbar_menu(
                        ToolbarMenu::Stereo,
                        width - 164.0,
                        height - theme::control_bar_height() - 4.0,
                        cx,
                    );
                })),
        )
        .child(cluster_button(
            "cluster-ratio",
            icons::RATIO,
            theme::bar_cluster_tint(),
            false,
        ))
        .child(
            cluster_button("cluster-sidebar", icons::SIDEBAR, sidebar_tint, true).on_click(
                cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)),
            ),
        )
        .child(
            cluster_button(
                "cluster-settings",
                icons::SETTINGS,
                theme::bar_cluster_tint(),
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_control_window(cx);
            })),
        )
        .child(
            cluster_button("cluster-menu", icons::MENU, theme::bar_cluster_tint(), true)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx))),
        );

    let top_row = div()
        .absolute()
        .left_0()
        .top_0()
        .w_full()
        .h(px(20.0))
        .flex()
        .items_center()
        .child(div().w(px(8.0)).flex_shrink_0())
        .child(seek_area)
        .child(div().w(px(4.0)).flex_shrink_0())
        .child(mute_area)
        .child(div().w(px(4.0)).flex_shrink_0())
        .child(volume_area)
        .child(div().w(px(12.0)).flex_shrink_0());

    let volume_drag_listener = {
        let bounds = volume_bounds.clone();
        let active = frame.volume_drag.clone();
        let target = cx.entity().downgrade();
        div()
            .absolute()
            .left_0()
            .top_0()
            .w_full()
            .h(px(20.0))
            .child(canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let bounds = bounds.clone();
                    let active = active.clone();
                    let target = target.clone();
                    let released = active.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
                        if phase != DispatchPhase::Bubble || !active.get() {
                            return;
                        }
                        if event.pressed_button != Some(MouseButton::Left) {
                            active.set(false);
                            return;
                        }
                        if let Some(fraction) = pointer_fraction(event.position, bounds.get()) {
                            let _ = target.update(cx, |this, cx| {
                                this.playback.update(cx, |playback, cx| {
                                    playback.set_volume(fraction as f64 * 100.0, cx)
                                });
                            });
                        }
                    });
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, _cx| {
                        if phase == DispatchPhase::Bubble {
                            released.set(false);
                        }
                    });
                },
            ))
    };

    let seek_drag_listener = {
        let bounds = seek_drag_bounds.clone();
        let target = cx.entity().downgrade();
        div()
            .absolute()
            .left_0()
            .top_0()
            .w_full()
            .h(px(20.0))
            .child(canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let bounds = bounds.clone();
                    let move_target = target.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
                        if phase != DispatchPhase::Bubble
                            || event.pressed_button != Some(MouseButton::Left)
                        {
                            return;
                        }
                        if let Some(fraction) = pointer_fraction(event.position, bounds.get()) {
                            let _ = move_target
                                .update(cx, |this, cx| this.drag_analysis_seek(fraction, cx));
                        }
                    });
                    let up_target = target.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
                        if phase == DispatchPhase::Bubble {
                            let _ = up_target.update(cx, |this, cx| this.commit_analysis_seek(cx));
                        }
                    });
                },
            ))
    };

    div()
        .relative()
        .h(px(theme::control_bar_height()))
        .w_full()
        .overflow_hidden()
        .bg(rgb(theme::control_bar_bg()))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w_full()
                .h(px(1.0))
                .bg(rgb(theme::bar_border())),
        )
        .child(top_row)
        .child(volume_drag_listener)
        .child(seek_drag_listener)
        .child(row)
        .into_any_element()
}

fn bar_button(id: &'static str, kind: u8, enabled: bool) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .relative()
        .h_full()
        .w(px(38.0))
        .flex()
        .items_center()
        .justify_center();
    if enabled {
        button = button.hover(|style| style.bg(rgb(theme::control_bar_hover())));
    }
    button.child(icon(
        kind,
        13.0,
        if enabled {
            theme::bar_button_tint()
        } else {
            theme::bar_button_disabled()
        },
    ))
    .child(
        div()
            .absolute()
            .right_0()
            .top(px(9.0))
            .w(px(1.0))
            .h(px(24.0))
            .bg(rgb(theme::bar_button_divider())),
    )
}

fn cluster_button(id: &'static str, kind: u8, tint: u32, enabled: bool) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .relative()
        .h_full()
        .w(px(32.0))
        .flex()
        .items_center()
        .justify_center();
    if enabled {
        button = button.hover(|style| style.bg(rgb(theme::control_bar_hover())));
    }
    button.child(icon(
        kind,
        15.0,
        if enabled {
            tint
        } else {
            theme::bar_button_disabled()
        },
    ))
}

fn cluster_label(id: &'static str, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .w(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(12.0))
        .text_color(rgb(theme::bar_button_disabled()))
        .child(tr(label))
}

fn chip(id: &'static str, text: String, width: f32) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(width))
        .h(px(18.0))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded(px(2.0))
        .bg(rgb(theme::bar_chip_bg()))
        .border_1()
        .border_color(rgb(theme::bar_chip_border()))
        .text_size(px(11.0))
        .text_color(rgb(theme::bar_chip_text()))
        .child(text)
}

fn pointer_fraction(position: Point<Pixels>, bounds: Bounds<Pixels>) -> Option<f32> {
    let width = f32::from(bounds.size.width);
    if width <= 0.0 {
        return None;
    }
    Some(((f32::from(position.x) - f32::from(bounds.origin.x)) / width).clamp(0.0, 1.0))
}

fn seek_fraction(position: Point<Pixels>, bounds: Bounds<Pixels>) -> f64 {
    pointer_fraction(position, bounds).unwrap_or(0.0) as f64
}

fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}
