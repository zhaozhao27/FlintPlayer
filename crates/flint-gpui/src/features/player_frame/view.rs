use gpui::*;

use super::control_bar::control_bar;
use super::osd;
use super::controller::{
    FrameAction, HoldButton, OsdMode, PLAYER_KEY_CONTEXT, PlayerFrameController, ToggleOsd,
    ToggleOsdBrief, WindowCommand,
};
use crate::features::{capture, playback, settings};
use crate::icons::{self, icon};
use crate::menu::MenuEntry;
use crate::theme;

const PANEL_WIDTH: f32 = 300.0;
const PANEL_PADDING: f32 = 4.0;
const SEPARATOR_HEIGHT: f32 = 9.0;

fn menu_panel_height(entries: &[MenuEntry], row_height: f32) -> f32 {
    let rows = entries.iter().filter(|entry| !entry.is_separator()).count() as f32;
    let separators = entries.len() as f32 - rows;
    rows * row_height + separators * SEPARATOR_HEIGHT + 2.0 * PANEL_PADDING
}

fn menu_row_height(entries: &[MenuEntry], available: f32) -> f32 {
    let rows = entries.iter().filter(|entry| !entry.is_separator()).count() as f32;
    let separators = entries.len() as f32 - rows;
    if rows == 0.0 {
        return theme::menu_row_height();
    }
    let needed = rows * theme::menu_row_height() + separators * SEPARATOR_HEIGHT;
    let space = (available - 2.0 * PANEL_PADDING).max(0.0);
    if needed <= space {
        theme::menu_row_height()
    } else {
        ((space - separators * SEPARATOR_HEIGHT) / rows)
            .clamp(16.0, theme::menu_row_height())
    }
}

impl PlayerFrameController {
    fn render_titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let capturing = self.capture.read(cx).is_active();
        let (format, source) = if capturing {
            (String::new(), self.capture.read(cx).display_info())
        } else {
            let playlist = self.playlist.read(cx);
            let format = playlist
                .current()
                .and_then(|index| playlist.items().get(index))
                .and_then(|path| path.extension())
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_uppercase())
                .unwrap_or_default();
            let playback = self.playback.read(cx);
            let source = if playback.state().loaded {
                playback.state().title.clone()
            } else {
                crate::i18n::tr("未打开媒体")
            };
            (format, source)
        };
        let has_format = !format.is_empty();
        let pinned = self.config.always_on_top;

        let menu_opener = div()
            .id("titlebar-menu")
            .absolute()
            .left_0()
            .top_0()
            .w(px(94.0))
            .h_full()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx)))
            .child(
                div()
                    .absolute()
                    .left(px(12.0))
                    .top_0()
                    .w(px(66.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(rgb(theme::title_text()))
                    .child("FlintPlayer"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(80.0))
                    .top(px(15.5))
                    .child(icon(
                        icons::CHEVRON_DOWN,
                        9.0,
                        theme::title_chevron(),
                    )),
            );

        let mut bar = div()
            .relative()
            .h(px(theme::titlebar_height()))
            .w_full()
            .bg(rgb(theme::title_bg()))
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                window.start_window_move();
            })
            .child(menu_opener)
            .child(
                div()
                    .absolute()
                    .left(px(94.0))
                    .top(px(8.0))
                    .w(px(1.0))
                    .h(px(theme::titlebar_height() - 16.0))
                    .bg(rgb(theme::title_divider())),
            );

        if has_format {
            bar = bar
                .child(
                    div()
                        .absolute()
                        .left(px(102.0))
                        .top_0()
                        .w(px(28.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .text_size(px(11.0))
                        .text_color(rgb(theme::title_meta()))
                        .child(format),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(132.0))
                        .top(px(8.0))
                        .w(px(1.0))
                        .h(px(theme::titlebar_height() - 16.0))
                        .bg(rgb(theme::title_divider())),
                );
        }

        let source_left = if has_format { 140.0 } else { 102.0 };

        let pin = icon_button(
            "titlebar-pin",
            96.0,
            icons::PIN,
            if pinned {
                theme::title_pin_active()
            } else {
                theme::title_button_tint()
            },
            false,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.handle_action("topmost-toggle".to_string(), cx)
        }));

        let minimize = icon_button(
            "titlebar-minimize",
            66.0,
            icons::MINIMIZE,
            theme::title_button_tint(),
            false,
        )
        .on_click(cx.listener(|_, _, window, _| window.minimize_window()));

        let maximize = icon_button(
            "titlebar-maximize",
            36.0,
            icons::MAXIMIZE,
            theme::title_button_tint(),
            false,
        )
        .on_click(cx.listener(|_, _, window, _| window.zoom_window()));

        let close = icon_button(
            "titlebar-close",
            6.0,
            icons::CLOSE,
            theme::title_button_tint(),
            true,
        )
        .on_click(cx.listener(|this, _, _, cx| this.quit(cx)));

        bar.child(
            div()
                .absolute()
                .left(px(source_left))
                .right(px(110.0))
                .top_0()
                .h_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.0))
                .text_color(rgb(theme::title_source()))
                .child(source),
        )
        .child(pin)
        .child(minimize)
        .child(maximize)
        .child(close)
    }

    fn render_menu_level(
        &self,
        level: usize,
        max_bottom: f32,
        max_right: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entries = self.entries_at(level);
        if entries.is_empty() {
            return None;
        }
        let x = self.menu_level_left(level, max_right);
        let mut y = self.menu_level_top(level, max_bottom);
        let mut available = (max_bottom - y).max(120.0);
        let mut row_height = theme::menu_row_height();
        let content = menu_panel_height(entries, row_height);
        if (level > 0 || self.menu_anchor().is_some()) && y + content > max_bottom {
            y = (max_bottom - content).max(theme::titlebar_height() + PANEL_PADDING);
            available = (max_bottom - y).max(120.0);
            row_height = menu_row_height(entries, available);
        }

        let mut panel = div()
            .id(("menu-panel", level))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(PANEL_WIDTH))
            .max_h(px(available))
            .py(px(PANEL_PADDING))
            .bg(rgb(theme::menu_bg()))
            .border_1()
            .border_color(rgb(theme::menu_border()))
            .occlude()
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()));

        for (index, entry) in entries.iter().enumerate() {
            panel = panel.child(self.render_menu_row(level, index, entry, row_height, cx));
        }
        Some(panel.into_any())
    }

    fn menu_level_left(&self, level: usize, max_right: f32) -> f32 {
        let base = self.menu_anchor().map(|(x, _)| x).unwrap_or(8.0);
        let natural = base + level as f32 * PANEL_WIDTH;
        let limit = (max_right - PANEL_WIDTH - 8.0).max(8.0);
        if natural <= limit {
            return natural;
        }
        if level > 0 {
            let flipped = self.menu_level_left(level - 1, max_right) - PANEL_WIDTH;
            if flipped >= 8.0 {
                return flipped;
            }
        }
        limit
    }

    fn menu_level_top(&self, level: usize, max_bottom: f32) -> f32 {
        let mut top = self
            .menu_anchor()
            .map(|(_, y)| y.max(theme::titlebar_height() + PANEL_PADDING))
            .unwrap_or(theme::titlebar_height() + PANEL_PADDING);
        for depth in 0..level {
            let entries = self.entries_at(depth);
            let content = menu_panel_height(entries, theme::menu_row_height());
            if (depth > 0 || self.menu_anchor().is_some()) && top + content > max_bottom {
                top = (max_bottom - content).max(theme::titlebar_height() + PANEL_PADDING);
            }
            let row_height = menu_row_height(entries, (max_bottom - top).max(120.0));
            let Some(&index) = self.open_path().get(depth) else {
                break;
            };
            let mut offset = 0.0;
            for entry in entries.iter().take(index) {
                offset += if entry.is_separator() {
                    SEPARATOR_HEIGHT
                } else {
                    row_height
                };
            }
            top += PANEL_PADDING + offset;
        }
        top
    }

    fn render_menu_row(
        &self,
        level: usize,
        index: usize,
        entry: &MenuEntry,
        row_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if entry.is_separator() {
            return div()
                .h(px(SEPARATOR_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .child(div().h(px(1.0)).w_full().bg(rgb(theme::menu_separator())))
                .into_any();
        }

        let has_children = !entry.children.is_empty();
        let check_width = 18.0;
        let check = if entry.checked == Some(true) {
            "✓"
        } else {
            ""
        };
        let title_color = if entry.enabled {
            theme::menu_text()
        } else {
            theme::menu_text_disabled()
        };
        let shortcut = entry.shortcut.clone().unwrap_or_default();
        let action = entry.action.clone();
        let enabled = entry.enabled;
        let arrow = if has_children { "▸" } else { "" };

        let mut row = div()
            .id(("menu-row", level * 1000 + index))
            .h(px(row_height))
            .w_full()
            .flex()
            .items_center()
            .px(px(6.0))
            .text_color(rgb(title_color))
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                if *hovered {
                    this.hover_row(level, index, has_children, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if !enabled || has_children {
                    return;
                }
                match this.activate(action.as_deref(), cx) {
                    Some(FrameAction::Exit) => cx.quit(),
                    Some(FrameAction::ToggleFullscreen) => window.toggle_fullscreen(),
                    Some(FrameAction::Action(action)) => this.handle_action(action, cx),
                    None => {}
                }
            }));

        row = row
            .hover(|style| style.bg(rgb(theme::menu_hover())))
            .child(
                div()
                    .w(px(check_width))
                    .flex_shrink_0()
                    .text_color(rgb(theme::menu_check()))
                    .child(check),
            )
            .child(div().flex_1().child(entry.title.clone()))
            .child(
                div()
                    .flex_shrink_0()
                    .pr(px(4.0))
                    .text_color(rgb(theme::menu_shortcut()))
                    .child(shortcut),
            )
            .child(
                div()
                    .w(px(14.0))
                    .flex_shrink_0()
                    .text_color(rgb(theme::menu_shortcut()))
                    .child(arrow),
            );

        row.into_any()
    }

    fn render_notice(&self) -> Option<AnyElement> {
        let notice = self.notice()?;
        Some(
            div()
                .absolute()
                .bottom(px(theme::control_bar_height() + 16.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px_3()
                        .py(px(6.0))
                        .bg(rgb(theme::notice_bg()))
                        .text_color(rgb(theme::notice_text()))
                        .opacity(self.notice_opacity())
                        .child(crate::i18n::tr(notice)),
                )
                .into_any(),
        )
    }

    fn render_osd(
        &self,
        area_width: f32,
        area_height: f32,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let mode = self.osd_mode()?;
        let width = (area_width - 36.0).max(0.0);
        let playback = self.playback.read(cx);
        let state = playback.state();
        let position = osd::format_clock(state.position);
        let duration = osd::format_clock(state.duration);
        let title = &state.title;

        match mode {
            OsdMode::Brief => Some(osd::brief_osd(
                width,
                &format!("{title}   {position} / {duration}"),
            )),
            OsdMode::Playback => {
                let status = if !state.loaded || state.eof {
                    crate::i18n::tr("已停止")
                } else if state.paused {
                    crate::i18n::tr("暂停")
                } else {
                    crate::i18n::tr("播放中")
                };
                let mute_suffix = if state.volume <= 0.0 {
                    crate::i18n::tr(" (静音)")
                } else {
                    String::new()
                };
                let text = format!(
                    "{}: {title}\n{}: {status}   {}: {:.1}x   {}: {:.0}%{}\n{}: {position} / {duration}",
                    crate::i18n::tr("文件名"),
                    crate::i18n::tr("状态"),
                    crate::i18n::tr("速度"),
                    playback.speed(),
                    crate::i18n::tr("音量"),
                    state.volume,
                    mute_suffix,
                    crate::i18n::tr("时间"),
                );
                Some(osd::playback_osd(width, &text))
            }
            OsdMode::Full => {
                let text_size = (area_height * 0.03).min(area_width / 40.0).clamp(11.0, 30.0);
                let capture = self.capture.read(cx);
                let data = if let Some(info) = capture.info() {
                    let stats = capture.stats().unwrap_or_default();
                    let decoded = capture
                        .decoded_pixel_format()
                        .unwrap_or_else(|| info.pixel_format.to_lowercase());
                    let (output_width, output_height) = capture
                        .frame_size()
                        .unwrap_or((info.width, info.height));
                    osd::FullData::Capture(osd::CaptureData {
                        label: capture.label(),
                        clock: flint_core::sysinfo::local_hms(),
                        timeline: osd::format_clock(capture.elapsed()),
                        frames: stats.frames,
                        cpu: self.cpu_percent(),
                        codec: info.codec.clone(),
                        pixel_format: info.pixel_format.clone(),
                        width: info.width,
                        height: info.height,
                        nominal_fps: info.nominal_fps,
                        bitrate_mbps: stats.bitrate_mbps,
                        decoded,
                        display_format: if capture.yuv_frame().is_some() {
                            "NV12".to_string()
                        } else {
                            "RGBA".to_string()
                        },
                        output_width,
                        output_height,
                        actual_fps: stats.actual_fps,
                        dropped: stats.dropped,
                        drop_percent: stats.drop_percent,
                        decode_ms: stats.decode_ms,
                        latency_ms: capture.latency_ms(),
                        audio_codec: None,
                    })
                } else {
                    let tracks = playback.tracks();
                    let video = tracks
                        .iter()
                        .find(|track| track.kind == "video" && track.selected);
                    let audio = tracks
                        .iter()
                        .find(|track| track.kind == "audio" && track.selected);
                    let playlist = self.playlist.read(cx);
                    let container = playlist
                        .current()
                        .and_then(|index| playlist.items().get(index))
                        .and_then(|path| path.extension())
                        .and_then(|extension| extension.to_str())
                        .map(|extension| extension.to_ascii_uppercase())
                        .unwrap_or_default();
                    osd::FullData::Playback(osd::PlaybackData {
                        title: title.clone(),
                        clock: flint_core::sysinfo::local_hms(),
                        position,
                        frames: playback.frame_number(),
                        cpu: self.cpu_percent(),
                        codec: video
                            .map(|track| track.codec.to_ascii_uppercase())
                            .unwrap_or_default(),
                        container,
                        fps: video.map(|track| track.fps).unwrap_or(0.0),
                        bitrate: video.map(|track| track.bitrate).unwrap_or(0.0),
                        width: state.width,
                        height: state.height,
                        audio_codec: audio.map(|track| track.codec.to_ascii_uppercase()),
                    })
                };
                Some(osd::full_osd(width, text_size, &data))
            }
        }
    }

    fn render_analysis(&self, area_width: f32, cx: &Context<Self>) -> Option<AnyElement> {
        let playback = self.playback.read(cx);
        if !playback.analysis_mode() {
            return None;
        }
        let state = playback.state();
        let frame = playback.frame_number();
        let seconds = if state.position > 0.0 {
            state.position
        } else {
            let fps = playback.analysis_fps();
            if fps > 0.0 {
                frame as f64 / fps
            } else {
                0.0
            }
        };
        let pixel_format = playback
            .raw_pixel_format()
            .map(|format| format.to_ascii_uppercase())
            .or_else(|| {
                playback
                    .pixel_format()
                    .map(|format| format.to_ascii_uppercase())
            })
            .unwrap_or_else(|| "-".to_string());
        let text = format!(
            "{}   {} {}   {}   {}×{}   {}   [ , / . ] {}",
            crate::i18n::tr("单帧分析"),
            crate::i18n::tr("帧号"),
            frame,
            osd::format_timestamp(seconds),
            state.width,
            state.height,
            pixel_format,
            crate::i18n::tr("上一帧/下一帧"),
        );
        Some(osd::analysis_overlay((area_width - 36.0).max(0.0), &text))
    }
}

impl Render for PlayerFrameController {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !window.is_window_active() {
            self.release_hold_speed(cx);
        }
        if !self.focus_handle().is_focused(window) {
            window.focus(self.focus_handle(), cx);
        }
        if !self.ticker_started {
            self.ticker_started = true;
            self.start_ticker(cx);
        }
        if !self.control_started {
            self.control_started = true;
            self.start_control_loop(cx);
        }
        if let Some(display) = window.display(cx) {
            let size = display.visible_bounds().size;
            self.display_size = (
                f32::from(size.width).max(1.0) as u32,
                f32::from(size.height).max(1.0) as u32,
            );
        }
        if let Some(path) = self.pending_open.take() {
            self.open_media(&path, cx);
        }
        if let Some(command) = self.pending_window.take() {
            match command {
                WindowCommand::ToggleMaximize => window.zoom_window(),
                WindowCommand::ToggleFullscreen => window.toggle_fullscreen(),
                WindowCommand::Activate => window.activate_window(),
                WindowCommand::Minimize => window.minimize_window(),
                WindowCommand::Resize(width, height) => {
                    window.resize(size(px(width), px(height)))
                }
                WindowCommand::Scale(factor) => {
                    let current = window.bounds().size;
                    window.resize(size(current.width * factor, current.height * factor));
                }
                WindowCommand::Fraction(fraction) => {
                    if let Some(display) = window.display(cx) {
                        let current = display.visible_bounds().size;
                        window.resize(size(
                            current.width * fraction,
                            current.height * fraction,
                        ));
                    }
                }
            }
        }
        if let Some(mut request) = self.settings.update(cx, |settings, _| settings.take_request()) {
            request.settings.smoothing =
                flint_media::CaptureSmoothing::from_key(&self.config.capture_smoothing);
            let target = request.source.clone();
            if request.split && self.capture.read(cx).source() != Some(target.as_str()) {
                let extra = cx.new(|_| crate::features::capture::CaptureController::new());
                extra.update(cx, |capture, cx| {
                    capture.start_with(request.source, request.name, request.settings, cx)
                });
                self.extras.push(extra);
            } else {
                self.playback.update(cx, |playback, cx| playback.stop(cx));
                self.stop_extras(cx);
                self.capture.update(cx, |capture, cx| {
                    capture.start_with(request.source, request.name, request.settings, cx)
                });
            }
        }
        if let Some(selections) = self
            .settings
            .update(cx, |settings, _| settings.take_selection_update())
        {
            self.config.capture_selections = selections;
            self.save_config();
        }
        self.playback
            .update(cx, |playback, _| playback.upload_frame(window));
        self.capture
            .update(cx, |capture, _| capture.upload_frame(window));
        for extra in &self.extras {
            extra.update(cx, |capture, _| capture.upload_frame(window));
        }

        let target = cx.entity().downgrade();
        let drop_handler = move |paths: &ExternalPaths, _: &mut Window, cx: &mut App| {
            if let Some(path) = paths.paths().first() {
                let _ = target.update(cx, |this, cx| this.open_media(path, cx));
            }
        };

        let capturing = self.capture.read(cx).is_active();
        let canvas = if capturing {
            let entities = self.capture_entities();
            if entities.len() <= 1 {
                capture::view::video_canvas(&self.capture, cx)
            } else {
                let mut grid = div().size_full().flex().flex_col().gap(px(2.0));
                for chunk in entities.chunks(2) {
                    let mut row = div().flex_1().w_full().flex().gap(px(2.0));
                    for entity in chunk {
                        row = row.child(
                            div()
                                .flex_1()
                                .h_full()
                                .overflow_hidden()
                                .child(capture::view::video_canvas(entity, cx)),
                        );
                    }
                    if chunk.len() == 1 {
                        row = row.child(div().flex_1());
                    }
                    grid = grid.child(row);
                }
                grid.into_any_element()
            }
        } else {
            playback::view::video_canvas(&self.playback, cx)
        };
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);
        let controls = control_bar(self, capturing, viewport_width, viewport_height, cx);

        let scale_x = (self.config.frame_scale_x.clamp(10, 400) as f32) / 100.0;
        let scale_y = (self.config.frame_scale_y.clamp(10, 400) as f32) / 100.0;
        let has_subtitle = self
            .playback
            .read(cx)
            .tracks()
            .iter()
            .any(|track| track.kind == "sub");
        let margin_visible = self.margin > 0
            && (self.margin_force
                || if self.margin_auto { has_subtitle } else { true });
        let margin_fraction = if margin_visible {
            (self.margin as f32 / 100.0).min(0.8)
        } else {
            0.0
        };
        let stage = div()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(rgb(theme::video_bg()))
            .on_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if this.is_menu_open() {
                    return;
                }
                let button = match event.button {
                    MouseButton::Left => HoldButton::Left,
                    MouseButton::Right => HoldButton::Right,
                    MouseButton::Middle => HoldButton::Middle,
                    _ => return,
                };
                if this.begin_hold_speed(button, cx) && button == HoldButton::Right {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .ml(px(self.config.frame_shift_x as f32))
                    .mt(px(self.config.frame_shift_y as f32))
                    .w(relative(scale_x))
                    .h(relative(scale_y * (1.0 - margin_fraction)))
                    .bg(rgb(theme::video_bg()))
                    .child(canvas),
            );

        let mut content = div()
            .flex_1()
            .w_full()
            .flex()
            .bg(rgb(theme::video_bg()))
            .child(stage);
        if self.is_sidebar_open() {
            content = content.child(crate::features::playlist::view::playlist_panel(
                &self.playlist,
                cx,
            ));
        }

        let mut root = div()
            .id("player-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::bg_primary()))
            .text_size(px(12.0))
            .text_color(rgb(theme::titlebar_text()))
            .track_focus(self.focus_handle())
            .key_context(PLAYER_KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &ToggleOsd, _, cx| {
                this.toggle_osd_shortcut(OsdMode::Full, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ToggleOsdBrief, _, cx| {
                this.toggle_osd_shortcut(OsdMode::Brief, cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key(event, window, cx);
            }))
            .on_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if event.button == MouseButton::Right {
                    if this.is_menu_open() {
                        this.close_menu(cx);
                    } else if !this.is_holding_right() {
                        this.open_menu_at(
                            f32::from(event.position.x),
                            f32::from(event.position.y),
                            cx,
                        );
                    }
                    cx.stop_propagation();
                    return;
                }
                let delta = match event.button {
                    MouseButton::Navigate(NavigationDirection::Back) => -1.0,
                    MouseButton::Navigate(NavigationDirection::Forward) => 1.0,
                    _ => return,
                };
                if this.playback.read(cx).analysis_mode() {
                    this.playback
                        .update(cx, |playback, cx| playback.step_frame(delta as i64, cx));
                } else {
                    this.playback
                        .update(cx, |playback, cx| playback.seek_relative(delta * 5.0, cx));
                }
                cx.stop_propagation();
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                this.finish_hold_speed(event.button, event.position, true, cx);
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.finish_hold_speed(event.button, event.position, false, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.finish_hold_speed(event.button, event.position, false, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.finish_hold_speed(event.button, event.position, false, cx);
                }),
            )
            .on_drop::<ExternalPaths>(drop_handler)
            .child(self.render_titlebar(cx))
            .child(content)
            .child(controls);

        if let Some(notice) = self.render_notice() {
            root = root.child(notice);
        }

        let sidebar_width = if self.is_sidebar_open() {
            crate::features::playlist::view::PLAYLIST_WIDTH
        } else {
            0.0
        };
        let osd_area_width = viewport_width - sidebar_width;
        let osd_area_height = viewport_height - theme::titlebar_height() - theme::control_bar_height();
        if let Some(osd) = self.render_osd(osd_area_width, osd_area_height, cx) {
            root = root.child(osd);
        }
        if let Some(analysis) = self.render_analysis(osd_area_width, cx) {
            root = root.child(analysis);
        }

        if self.is_control_open() {
            root = root.child(super::control_window::control_window(self, cx));
        }

        if self.is_menu_open() {
            let backdrop = div()
                .id("menu-backdrop")
                .absolute()
                .top(px(theme::titlebar_height()))
                .left_0()
                .right_0()
                .bottom_0()
                .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)));
            root = root.child(backdrop);
            let menu_bottom = if self.toolbar_menu().is_some() {
                viewport_height - theme::control_bar_height() - 4.0
            } else {
                viewport_height - 8.0
            };
            let levels = self.open_path().len() + 1;
            for level in 0..levels {
                if let Some(panel) =
                    self.render_menu_level(level, menu_bottom, viewport_width, cx)
                {
                    root = root.child(panel);
                }
            }
        }

        if self.settings.read(cx).is_open() {
            root = root.child(settings::view::settings_overlay(&self.settings, cx));
        }

        root
    }
}

fn icon_button(
    id: &'static str,
    right: f32,
    kind: u8,
    tint: u32,
    danger: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .right(px(right))
        .top(px(3.0))
        .w(px(28.0))
        .h(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(move |style| {
            style.bg(rgb(if danger {
                theme::titlebar_close_hover()
            } else {
                theme::title_button_hover()
            }))
        })
        .child(icon(kind, 16.0, tint))
}
