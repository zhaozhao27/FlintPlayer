use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub source: String,
    pub sources: Vec<String>,
    pub favorites: Vec<String>,
    pub bookmarks: Vec<(String, Vec<f64>)>,
    pub resume: Vec<(String, f64)>,
    pub capture_selections: Vec<(String, String, u32, u32)>,
    pub capture_smoothing: String,
    pub capture_audio_source: String,
    pub remember_position: bool,
    pub show_markers: bool,
    pub seek_step_seconds: f64,
    pub subtitle_scale: f32,
    pub subtitle_color: String,
    pub subtitle_top: bool,
    pub subtitle_font: String,
    pub subtitle_bold: bool,
    pub subtitle_shadow: bool,
    pub ass_styles: bool,
    pub subtitle_graphic_outside: bool,
    pub subtitle_text_inside: bool,
    pub subtitle_bottom: bool,
    pub subtitle_border_opaque: bool,
    pub subtitle_smooth_vmr: bool,
    pub subtitle_3d: String,
    pub subtitle_language: String,
    pub stereo_mode: String,
    pub video_renderer: String,
    pub avisynth_enabled: bool,
    pub avisynth_script: String,
    pub vapoursynth_enabled: bool,
    pub vapoursynth_script: String,
    pub pano_enabled: bool,
    pub pano_mode: String,
    pub pano_yaw: f32,
    pub pano_pitch: f32,
    pub pano_fov: f32,
    pub tts_enabled: bool,
    pub translate_enabled: bool,
    pub brightness: i16,
    pub contrast: i16,
    pub saturation: i16,
    pub hue: i16,
    pub rotation: u16,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub screen1_width: u32,
    pub screen1_height: u32,
    pub screen2_width: u32,
    pub screen2_height: u32,
    pub window_custom_width: u32,
    pub window_custom_height: u32,
    pub osd_visible: bool,
    pub always_on_top: bool,
    pub topmost_mode: String,
    pub autohide_controls: bool,
    pub fullscreen_no_cover: bool,
    pub ignore_shortcuts: bool,
    pub favorites_memory_points: bool,
    pub similar_mode: String,
    pub touch_enabled: bool,
    pub move_only_titlebar: bool,
    pub skin_opacity: f32,
    pub skin: String,
    pub menu_accent: String,
    pub logo_file: String,
    pub logo_play_file: String,
    pub logo_show: bool,
    pub logo_play_show: bool,
    pub skin_layer_output: bool,
    pub popup_menu_type: String,
    pub menu_output_mode: String,
    pub touch_skin: bool,
    pub video_skin_start: bool,
    pub menu_show_names: bool,
    pub enlarge_skin_menu: bool,
    pub keep_ui_mode: bool,
    pub separate_geometry: bool,
    pub language: String,
    pub shutdown_mode: String,
    pub remember_post_action: bool,
    pub remember_window: bool,
    pub snapshot_dir: PathBuf,
    pub snapshot_format: String,
    pub snapshot_name: String,
    pub snapshot_play_folder: bool,
    pub snapshot_interval_ms: u64,
    pub snapshot_count: i32,
    pub thumbnail_width: u32,
    pub post_action: String,
    pub window_w: f32,
    pub window_h: f32,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub video_window_w: Option<f32>,
    pub video_window_h: Option<f32>,
    pub video_window_x: Option<f32>,
    pub video_window_y: Option<f32>,
    pub audio_window_w: Option<f32>,
    pub audio_window_h: Option<f32>,
    pub audio_window_x: Option<f32>,
    pub audio_window_y: Option<f32>,
    pub audio_device: String,
    pub audio_device2: String,
    pub audio_passthrough_device: String,
    pub audio_output_method: String,
    pub audio_output_rate: u32,
    pub audio_output_format: u32,
    pub audio_multi_channel_volume: bool,
    pub audio_max_volume: u32,
    pub audio_ac3_dts: bool,
    pub audio_ac3_dts_gain_db: f32,
    pub remember_frame: bool,
    pub frame_scale_x: i32,
    pub frame_scale_y: i32,
    pub frame_shift_x: i32,
    pub frame_shift_y: i32,
    pub volume_control: String,
    pub video_record_dir: PathBuf,
    pub video_record_play_folder: bool,
    pub audio_record_dir: PathBuf,
    pub audio_record_play_folder: bool,
    pub audio_processing: bool,
    pub audio_effect_normalize: bool,
    pub audio_effect_crystallize: bool,
    pub audio_effect_denoise: bool,
    pub audio_effect_reverb: bool,
    pub audio_effect_vocal_remove: bool,
    pub audio_effect_vocal_enhance: bool,
    pub audio_equalizer: bool,
    pub audio_eq_preset: String,
    pub audio_sync_mode: String,
    pub audio_sync_reset_exit: bool,
    pub audio_sync_ms: i32,
    pub audio_language: String,
    pub audio_select_language: bool,
    pub default_open_action: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: String::new(),
            sources: Vec::new(),
            favorites: Vec::new(),
            bookmarks: Vec::new(),
            resume: Vec::new(),
            capture_selections: Vec::new(),
            capture_smoothing: "system".into(),
            capture_audio_source: String::new(),
            remember_position: true,
            show_markers: true,
            seek_step_seconds: 5.0,
            subtitle_scale: 1.0,
            subtitle_color: "white".into(),
            subtitle_top: false,
            subtitle_font: String::new(),
            subtitle_bold: false,
            subtitle_shadow: true,
            ass_styles: true,
            subtitle_graphic_outside: false,
            subtitle_text_inside: true,
            subtitle_bottom: true,
            subtitle_border_opaque: false,
            subtitle_smooth_vmr: false,
            subtitle_3d: "off".into(),
            subtitle_language: "auto".into(),
            stereo_mode: "off".into(),
            video_renderer: "auto".into(),
            avisynth_enabled: false,
            avisynth_script: String::new(),
            vapoursynth_enabled: false,
            vapoursynth_script: String::new(),
            pano_enabled: false,
            pano_mode: "off".to_string(),
            pano_yaw: 0.0,
            pano_pitch: 0.0,
            pano_fov: 90.0,
            tts_enabled: false,
            translate_enabled: false,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            hue: 0,
            rotation: 0,
            flip_horizontal: false,
            flip_vertical: false,
            width: 1280,
            height: 720,
            fps: 30,
            screen1_width: 0,
            screen1_height: 0,
            screen2_width: 0,
            screen2_height: 0,
            window_custom_width: 1280,
            window_custom_height: 720,
            osd_visible: false,
            always_on_top: false,
            topmost_mode: "never".into(),
            autohide_controls: true,
            fullscreen_no_cover: false,
            ignore_shortcuts: false,
            favorites_memory_points: false,
            similar_mode: "selected".into(),
            touch_enabled: true,
            move_only_titlebar: true,
            skin_opacity: 1.0,
            skin: "默认皮肤".into(),
            menu_accent: String::new(),
            logo_file: String::new(),
            logo_play_file: String::new(),
            logo_show: true,
            logo_play_show: true,
            skin_layer_output: true,
            popup_menu_type: "skin".into(),
            menu_output_mode: "auto".into(),
            touch_skin: false,
            video_skin_start: false,
            menu_show_names: true,
            enlarge_skin_menu: true,
            keep_ui_mode: true,
            separate_geometry: false,
            language: "auto".into(),
            shutdown_mode: "poweroff".into(),
            remember_post_action: true,
            remember_window: true,
            snapshot_dir: dirs::picture_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("FlintPlayer"),
            snapshot_format: "png".into(),
            snapshot_name: "time".into(),
            snapshot_play_folder: false,
            snapshot_interval_ms: 1000,
            snapshot_count: 10,
            thumbnail_width: 320,
            post_action: "none".into(),
            window_w: 1280.0,
            window_h: 800.0,
            window_x: None,
            window_y: None,
            video_window_w: None,
            video_window_h: None,
            video_window_x: None,
            video_window_y: None,
            audio_window_w: None,
            audio_window_h: None,
            audio_window_x: None,
            audio_window_y: None,
            audio_device: String::new(),
            audio_device2: String::new(),
            audio_passthrough_device: String::new(),
            audio_output_method: "auto".to_string(),
            audio_output_rate: 0,
            audio_output_format: 0,
            audio_multi_channel_volume: false,
            audio_max_volume: 100,
            audio_ac3_dts: false,
            audio_ac3_dts_gain_db: 6.0,
            remember_frame: false,
            frame_scale_x: 100,
            frame_scale_y: 100,
            frame_shift_x: 0,
            frame_shift_y: 0,
            volume_control: "default".into(),
            video_record_dir: dirs::video_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("FlintPlayer"),
            video_record_play_folder: false,
            audio_record_dir: dirs::audio_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("FlintPlayer"),
            audio_record_play_folder: false,
            audio_processing: false,
            audio_effect_normalize: false,
            audio_effect_crystallize: false,
            audio_effect_denoise: false,
            audio_effect_reverb: false,
            audio_effect_vocal_remove: false,
            audio_effect_vocal_enhance: false,
            audio_equalizer: false,
            audio_eq_preset: "flat".into(),
            audio_sync_mode: "none".into(),
            audio_sync_reset_exit: false,
            audio_sync_ms: 0,
            audio_language: "auto".into(),
            audio_select_language: false,
            default_open_action: "file".into(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match fs::read_to_string(config_path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            match key.trim() {
                "source" => cfg.source = value.to_string(),
                "sources" => cfg.sources = decode_sources(value),
                "favorites" => cfg.favorites = decode_sources(value),
                "bookmarks" => {
                    cfg.bookmarks = decode_timed_paths(value)
                        .into_iter()
                        .filter_map(|(path, times)| {
                            let times: Vec<f64> = times
                                .split(',')
                                .filter_map(|time| time.trim().parse::<f64>().ok())
                                .filter(|time| time.is_finite() && *time >= 0.0)
                                .collect();
                            (!times.is_empty()).then_some((path, times))
                        })
                        .collect()
                }
                "resume" => {
                    cfg.resume = decode_timed_paths(value)
                        .into_iter()
                        .filter_map(|(path, position)| {
                            let position = position.trim().parse::<f64>().ok()?;
                            (position.is_finite() && position >= 0.0).then_some((path, position))
                        })
                        .collect()
                }
                "capture_selections" => {
                    cfg.capture_selections = decode_sources(value)
                        .into_iter()
                        .filter_map(|entry| {
                            let mut parts = entry.split('\t');
                            let device = parts.next()?.to_string();
                            let fourcc = parts.next()?.to_string();
                            let width = parts.next()?.parse::<u32>().ok()?;
                            let height = parts.next()?.parse::<u32>().ok()?;
                            (!device.is_empty()).then_some((device, fourcc, width, height))
                        })
                        .collect()
                }
                "capture_smoothing" => {
                    if matches!(value, "none" | "system" | "low" | "normal" | "medium" | "high") {
                        cfg.capture_smoothing = value.to_string();
                    }
                }
                "capture_audio_source" => cfg.capture_audio_source = value.trim().to_string(),
                "remember_position" => cfg.remember_position = value == "true" || value == "1",
                "show_markers" => cfg.show_markers = value == "true" || value == "1",
                "seek_step_seconds" => {
                    if let Ok(seconds) = value.parse::<f64>() {
                        if seconds.is_finite() {
                            cfg.seek_step_seconds = seconds.clamp(1.0, 600.0);
                        }
                    }
                }
                "subtitle_scale" => {
                    cfg.subtitle_scale = value.parse().unwrap_or(cfg.subtitle_scale).clamp(0.6, 2.0)
                }
                "subtitle_color" => {
                    if matches!(value, "white" | "yellow" | "cyan" | "green") {
                        cfg.subtitle_color = value.to_string();
                    }
                }
                "subtitle_top" => cfg.subtitle_top = value == "true" || value == "1",
                "subtitle_font" => cfg.subtitle_font = value.to_string(),
                "subtitle_bold" => cfg.subtitle_bold = value == "true" || value == "1",
                "subtitle_shadow" => cfg.subtitle_shadow = value == "true" || value == "1",
                "ass_styles" => cfg.ass_styles = value == "true" || value == "1",
                "subtitle_graphic_outside" => {
                    cfg.subtitle_graphic_outside = value == "true" || value == "1"
                }
                "subtitle_text_inside" => {
                    cfg.subtitle_text_inside = value == "true" || value == "1"
                }
                "subtitle_bottom" => cfg.subtitle_bottom = value == "true" || value == "1",
                "subtitle_border_opaque" => {
                    cfg.subtitle_border_opaque = value == "true" || value == "1"
                }
                "subtitle_smooth_vmr" => cfg.subtitle_smooth_vmr = value == "true" || value == "1",
                "avisynth_enabled" => cfg.avisynth_enabled = value == "true" || value == "1",
                "avisynth_script" => cfg.avisynth_script = value.to_string(),
                "vapoursynth_enabled" => cfg.vapoursynth_enabled = value == "true" || value == "1",
                "vapoursynth_script" => cfg.vapoursynth_script = value.to_string(),
                "subtitle_3d" => {
                    if matches!(value, "off" | "sbs" | "tab" | "anaglyph") {
                        cfg.subtitle_3d = value.to_string();
                    }
                }
                "subtitle_language" => {
                    if matches!(value, "auto" | "chi" | "eng" | "jpn" | "kor") {
                        cfg.subtitle_language = value.to_string();
                    }
                }
                "stereo_mode" => {
                    if matches!(
                        value,
                        "off"
                            | "anaglyph-sbs"
                            | "anaglyph-tab"
                            | "left-sbs"
                            | "right-sbs"
                            | "left-tab"
                            | "right-tab"
                    ) {
                        cfg.stereo_mode = value.to_string();
                    }
                }
                "video_renderer" => {
                    if matches!(
                        value,
                        "auto" | "opengl" | "null-compressed" | "null-uncompressed"
                    ) {
                        cfg.video_renderer = value.to_string();
                    }
                }
                "pano_enabled" => cfg.pano_enabled = value == "true" || value == "1",
                "pano_mode" => {
                    cfg.pano_mode = value.to_string();
                    cfg.pano_enabled = cfg.pano_mode != "off";
                }
                "pano_yaw" => {
                    cfg.pano_yaw = value.parse().unwrap_or(cfg.pano_yaw);
                    if !cfg.pano_yaw.is_finite() {
                        cfg.pano_yaw = 0.0;
                    }
                }
                "pano_pitch" => {
                    cfg.pano_pitch = value.parse().unwrap_or(cfg.pano_pitch).clamp(-89.0, 89.0);
                }
                "pano_fov" => {
                    cfg.pano_fov = value.parse().unwrap_or(cfg.pano_fov).clamp(30.0, 120.0);
                }
                "tts_enabled" => cfg.tts_enabled = value == "true" || value == "1",
                "translate_enabled" => cfg.translate_enabled = value == "true" || value == "1",
                "brightness" => {
                    cfg.brightness = value.parse().unwrap_or(cfg.brightness).clamp(-100, 100)
                }
                "contrast" => cfg.contrast = value.parse().unwrap_or(cfg.contrast).clamp(-100, 100),
                "saturation" => {
                    cfg.saturation = value.parse().unwrap_or(cfg.saturation).clamp(-100, 100)
                }
                "hue" => cfg.hue = value.parse().unwrap_or(cfg.hue).clamp(-100, 100),
                "rotation" => {
                    if matches!(value, "0" | "90" | "180" | "270") {
                        cfg.rotation = value.parse().unwrap_or(0);
                    }
                }
                "flip_horizontal" => cfg.flip_horizontal = value == "true" || value == "1",
                "flip_vertical" => cfg.flip_vertical = value == "true" || value == "1",
                "width" => cfg.width = value.parse().unwrap_or(cfg.width),
                "height" => cfg.height = value.parse().unwrap_or(cfg.height),
                "fps" => cfg.fps = value.parse().unwrap_or(cfg.fps),
                "screen1_width" => {
                    cfg.screen1_width = value.parse().unwrap_or(cfg.screen1_width);
                }
                "screen1_height" => {
                    cfg.screen1_height = value.parse().unwrap_or(cfg.screen1_height);
                }
                "screen2_width" => {
                    cfg.screen2_width = value.parse().unwrap_or(cfg.screen2_width);
                }
                "screen2_height" => {
                    cfg.screen2_height = value.parse().unwrap_or(cfg.screen2_height);
                }
                "window_custom_width" => {
                    cfg.window_custom_width = value.parse().unwrap_or(cfg.window_custom_width);
                }
                "window_custom_height" => {
                    cfg.window_custom_height = value.parse().unwrap_or(cfg.window_custom_height);
                }
                "osd_visible" => cfg.osd_visible = value == "true" || value == "1",
                "always_on_top" => {
                    cfg.always_on_top = value == "true" || value == "1";
                    if cfg.always_on_top {
                        cfg.topmost_mode = "always".into();
                    }
                }
                "topmost_mode" => {
                    if matches!(value, "never" | "always" | "playing" | "fullscreen") {
                        cfg.topmost_mode = value.to_string();
                        cfg.always_on_top = value == "always";
                    }
                }
                "autohide_controls" => cfg.autohide_controls = value == "true" || value == "1",
                "fullscreen_no_cover" => cfg.fullscreen_no_cover = value == "true" || value == "1",
                "ignore_shortcuts" => cfg.ignore_shortcuts = value == "true" || value == "1",
                "favorites_memory_points" => {
                    cfg.favorites_memory_points = value == "true" || value == "1"
                }
                "similar_mode" => {
                    if matches!(value, "selected" | "similar" | "all") {
                        cfg.similar_mode = value.to_string();
                    }
                }
                "touch_enabled" => cfg.touch_enabled = value == "true" || value == "1",
                "move_only_titlebar" => cfg.move_only_titlebar = value == "true" || value == "1",
                "skin_opacity" => {
                    cfg.skin_opacity = value.parse().unwrap_or(cfg.skin_opacity).clamp(0.4, 1.0)
                }
                "skin" => cfg.skin = value.to_string(),
                "logo_file" => cfg.logo_file = value.to_string(),
                "logo_play_file" => cfg.logo_play_file = value.to_string(),
                "logo_show" => cfg.logo_show = value == "true" || value == "1",
                "logo_play_show" => cfg.logo_play_show = value == "true" || value == "1",
                "skin_layer_output" => cfg.skin_layer_output = value == "true" || value == "1",
                "popup_menu_type" => {
                    if matches!(value, "skin" | "system") {
                        cfg.popup_menu_type = value.to_string();
                    }
                }
                "menu_output_mode" => {
                    if matches!(value, "auto" | "dark") {
                        cfg.menu_output_mode = value.to_string();
                    }
                }
                "touch_skin" => cfg.touch_skin = value == "true" || value == "1",
                "video_skin_start" => cfg.video_skin_start = value == "true" || value == "1",
                "menu_show_names" => cfg.menu_show_names = value == "true" || value == "1",
                "enlarge_skin_menu" => cfg.enlarge_skin_menu = value == "true" || value == "1",
                "keep_ui_mode" => cfg.keep_ui_mode = value == "true" || value == "1",
                "separate_geometry" => cfg.separate_geometry = value == "true" || value == "1",
                "language" => {
                    if matches!(value, "auto" | "zh" | "en") {
                        cfg.language = value.to_string();
                    }
                }
                "menu_accent" => {
                    if matches!(
                        value,
                        "" | "blue" | "red" | "green" | "purple" | "orange" | "cyan" | "silver"
                    ) {
                        cfg.menu_accent = value.to_string();
                    }
                }
                "shutdown_mode" => {
                    if matches!(value, "poweroff" | "suspend" | "hibernate") {
                        cfg.shutdown_mode = value.to_string();
                    }
                }
                "remember_post_action" => {
                    cfg.remember_post_action = value == "true" || value == "1"
                }
                "remember_window" => cfg.remember_window = value == "true" || value == "1",
                "snapshot_dir" => cfg.snapshot_dir = PathBuf::from(value),
                "snapshot_format" => {
                    if matches!(value, "png" | "jpeg" | "bmp") {
                        cfg.snapshot_format = value.to_string();
                    }
                }
                "snapshot_name" => {
                    if matches!(value, "time" | "playback") {
                        cfg.snapshot_name = value.to_string();
                    }
                }
                "snapshot_play_folder" => {
                    cfg.snapshot_play_folder = value == "true" || value == "1"
                }
                "snapshot_interval_ms" => {
                    cfg.snapshot_interval_ms = value
                        .parse()
                        .unwrap_or(cfg.snapshot_interval_ms)
                        .clamp(100, 60000)
                }
                "snapshot_count" => {
                    cfg.snapshot_count = value.parse().unwrap_or(cfg.snapshot_count).clamp(1, 100)
                }
                "thumbnail_width" => {
                    cfg.thumbnail_width =
                        value.parse().unwrap_or(cfg.thumbnail_width).clamp(64, 3840)
                }
                "post_action" => {
                    if matches!(
                        value,
                        "none" | "exit-current" | "exit-all" | "off-current" | "off-all"
                    ) {
                        cfg.post_action = value.to_string();
                    }
                }
                "window_w" => cfg.window_w = value.parse().unwrap_or(cfg.window_w).max(720.0),
                "window_h" => cfg.window_h = value.parse().unwrap_or(cfg.window_h).max(480.0),
                "window_x" => cfg.window_x = value.parse().ok(),
                "window_y" => cfg.window_y = value.parse().ok(),
                "video_window_w" => {
                    cfg.video_window_w = value.parse::<f32>().ok().map(|v| v.max(720.0))
                }
                "video_window_h" => {
                    cfg.video_window_h = value.parse::<f32>().ok().map(|v| v.max(480.0))
                }
                "video_window_x" => cfg.video_window_x = value.parse().ok(),
                "video_window_y" => cfg.video_window_y = value.parse().ok(),
                "audio_window_w" => {
                    cfg.audio_window_w = value.parse::<f32>().ok().map(|v| v.max(720.0))
                }
                "audio_window_h" => {
                    cfg.audio_window_h = value.parse::<f32>().ok().map(|v| v.max(480.0))
                }
                "audio_window_x" => cfg.audio_window_x = value.parse().ok(),
                "audio_window_y" => cfg.audio_window_y = value.parse().ok(),
                "audio_device" => cfg.audio_device = value.to_string(),
                "audio_device2" => cfg.audio_device2 = value.to_string(),
                "audio_passthrough_device" => cfg.audio_passthrough_device = value.to_string(),
                "audio_multi_channel_volume" => {
                    cfg.audio_multi_channel_volume = value == "true" || value == "1"
                }
                "audio_max_volume" => {
                    cfg.audio_max_volume = value
                        .parse()
                        .unwrap_or(cfg.audio_max_volume)
                        .clamp(100, 500)
                }
                "audio_ac3_dts" => cfg.audio_ac3_dts = value == "true" || value == "1",
                "audio_ac3_dts_gain_db" => {
                    cfg.audio_ac3_dts_gain_db = value
                        .parse()
                        .unwrap_or(cfg.audio_ac3_dts_gain_db)
                        .clamp(0.0, 24.0)
                }
                "audio_output_method" => {
                    cfg.audio_output_method = match value {
                        "pulse" | "null" | "auto" => value.to_string(),
                        _ => cfg.audio_output_method.clone(),
                    }
                }
                "audio_output_rate" => {
                    cfg.audio_output_rate = value.parse().unwrap_or(cfg.audio_output_rate)
                }
                "audio_output_format" => {
                    cfg.audio_output_format = value.parse().unwrap_or(cfg.audio_output_format)
                }
                "remember_frame" => cfg.remember_frame = value == "true" || value == "1",
                "frame_scale_x" => {
                    cfg.frame_scale_x = value.parse().unwrap_or(cfg.frame_scale_x).clamp(10, 400)
                }
                "frame_scale_y" => {
                    cfg.frame_scale_y = value.parse().unwrap_or(cfg.frame_scale_y).clamp(10, 400)
                }
                "frame_shift_x" => {
                    cfg.frame_shift_x = value.parse().unwrap_or(cfg.frame_shift_x).clamp(-50, 50)
                }
                "frame_shift_y" => {
                    cfg.frame_shift_y = value.parse().unwrap_or(cfg.frame_shift_y).clamp(-50, 50)
                }
                "volume_control" => {
                    if matches!(value, "default" | "system-wave" | "system-master") {
                        cfg.volume_control = value.to_string();
                    }
                }
                "video_record_dir" => cfg.video_record_dir = PathBuf::from(value),
                "video_record_play_folder" => {
                    cfg.video_record_play_folder = value == "true" || value == "1"
                }
                "audio_record_dir" => cfg.audio_record_dir = PathBuf::from(value),
                "audio_record_play_folder" => {
                    cfg.audio_record_play_folder = value == "true" || value == "1"
                }
                "audio_processing" => cfg.audio_processing = value == "true" || value == "1",
                "audio_effect_normalize" => {
                    cfg.audio_effect_normalize = value == "true" || value == "1"
                }
                "audio_effect_crystallize" => {
                    cfg.audio_effect_crystallize = value == "true" || value == "1"
                }
                "audio_effect_denoise" => {
                    cfg.audio_effect_denoise = value == "true" || value == "1"
                }
                "audio_effect_reverb" => cfg.audio_effect_reverb = value == "true" || value == "1",
                "audio_effect_vocal_remove" => {
                    cfg.audio_effect_vocal_remove = value == "true" || value == "1"
                }
                "audio_effect_vocal_enhance" => {
                    cfg.audio_effect_vocal_enhance = value == "true" || value == "1"
                }
                "audio_equalizer" => cfg.audio_equalizer = value == "true" || value == "1",
                "audio_eq_preset" => {
                    if matches!(
                        value,
                        "flat"
                            | "pop"
                            | "rock"
                            | "classical"
                            | "dance"
                            | "bass"
                            | "treble"
                            | "vocal"
                    ) {
                        cfg.audio_eq_preset = value.to_string();
                    }
                }
                "audio_sync_mode" if value == "none" || value == "always" => {
                    cfg.audio_sync_mode = value.to_string();
                }
                "audio_sync_reset_exit" => {
                    cfg.audio_sync_reset_exit = value == "true" || value == "1"
                }
                "audio_sync_ms" => cfg.audio_sync_ms = value.trim().parse().unwrap_or(0),
                "audio_language" if matches!(value, "auto" | "chi" | "eng" | "jpn" | "kor") => {
                    cfg.audio_language = value.to_string();
                }
                "audio_select_language" => {
                    cfg.audio_select_language = value == "true" || value == "1"
                }
                "default_open_action"
                    if matches!(value, "file" | "url" | "folder" | "navigate") =>
                {
                    cfg.default_open_action = value.to_string();
                }
                _ => {}
            }
        }
        if !cfg.remember_frame {
            cfg.frame_scale_x = 100;
            cfg.frame_scale_y = 100;
            cfg.frame_shift_x = 0;
            cfg.frame_shift_y = 0;
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let sanitize = |value: &str| value.replace(['\n', '\r'], " ");
        let text = format!(
            "# FlintPlayer\n\
             source={}\n\
             sources={}\n\
             favorites={}\n\
             bookmarks={}\n\
             resume={}\n\
             capture_selections={}\n\
             capture_smoothing={}\n\
             capture_audio_source={}\n\
             remember_position={}\n\
             show_markers={}\n\
             seek_step_seconds={}\n\
             subtitle_scale={}\n\
             subtitle_color={}\n\
             subtitle_top={}\n\
             subtitle_font={}\n\
             subtitle_bold={}\n\
             subtitle_shadow={}\n\
             ass_styles={}\n\
             subtitle_graphic_outside={}\n\
             subtitle_text_inside={}\n\
             subtitle_bottom={}\n\
             subtitle_border_opaque={}\n\
             subtitle_smooth_vmr={}\n\
             subtitle_3d={}\n\
             subtitle_language={}\n\
             stereo_mode={}\n\
             video_renderer={}\n\
             avisynth_enabled={}\n\
             avisynth_script={}\n\
             vapoursynth_enabled={}\n\
             vapoursynth_script={}\n\
             pano_enabled={}\n\
             pano_mode={}\n\
             pano_yaw={}\n\
             pano_pitch={}\n\
             pano_fov={}\n\
             tts_enabled={}\n\
             translate_enabled={}\n\
             brightness={}\n\
             contrast={}\n\
             saturation={}\n\
             hue={}\n\
             rotation={}\n\
             flip_horizontal={}\n\
             flip_vertical={}\n\
             width={}\n\
             height={}\n\
             fps={}\n\
             screen1_width={}\n\
             screen1_height={}\n\
             screen2_width={}\n\
             screen2_height={}\n\
             window_custom_width={}\n\
             window_custom_height={}\n\
             osd_visible={}\n\
             always_on_top={}\n\
             topmost_mode={}\n\
             autohide_controls={}\n\
             fullscreen_no_cover={}\n\
             ignore_shortcuts={}\n\
             favorites_memory_points={}\n\
             similar_mode={}\n\
             touch_enabled={}\n\
             move_only_titlebar={}\n\
             skin_opacity={}\n\
             skin={}\n\
             menu_accent={}\n\
             logo_file={}\n\
             logo_play_file={}\n\
             logo_show={}\n\
             logo_play_show={}\n\
             skin_layer_output={}\n\
             popup_menu_type={}\n\
             menu_output_mode={}\n\
             touch_skin={}\n\
             video_skin_start={}\n\
             language={}\n\
             shutdown_mode={}\n\
             remember_post_action={}\n\
             remember_window={}\n\
             snapshot_dir={}\n\
             snapshot_format={}\n\
             snapshot_name={}\n\
             snapshot_play_folder={}\n\
             snapshot_interval_ms={}\n\
             snapshot_count={}\n\
             thumbnail_width={}\n\
             post_action={}\n\
             window_w={}\n\
             window_h={}\n\
             window_x={}\n\
             window_y={}\n\
             menu_show_names={}\n\
             enlarge_skin_menu={}\n\
             keep_ui_mode={}\n\
             separate_geometry={}\n\
             video_window_w={}\n\
             video_window_h={}\n\
             video_window_x={}\n\
             video_window_y={}\n\
             audio_window_w={}\n\
             audio_window_h={}\n\
             audio_window_x={}\n\
             audio_window_y={}\n\
             audio_device={}\n\
             audio_device2={}\n\
             audio_passthrough_device={}\n\
             audio_multi_channel_volume={}\n\
             audio_max_volume={}\n\
             audio_ac3_dts={}\n\
             audio_ac3_dts_gain_db={}\n\
             audio_output_method={}\n\
             audio_output_rate={}\n\
             audio_output_format={}\n\
             remember_frame={}\n\
             frame_scale_x={}\n\
             frame_scale_y={}\n\
             frame_shift_x={}\n\
             frame_shift_y={}\n\
             volume_control={}\n\
             video_record_dir={}\n\
             video_record_play_folder={}\n\
             audio_record_dir={}\n\
             audio_record_play_folder={}\n\
             audio_processing={}\n\
             audio_effect_normalize={}\n\
             audio_effect_crystallize={}\n\
             audio_effect_denoise={}\n\
             audio_effect_reverb={}\n\
             audio_effect_vocal_remove={}\n\
             audio_effect_vocal_enhance={}\n\
             audio_equalizer={}\n\
             audio_eq_preset={}\n\
             audio_sync_mode={}\n\
             audio_sync_reset_exit={}\n\
             audio_sync_ms={}\n\
             audio_language={}\n\
             audio_select_language={}\n\
             default_open_action={}\n",
            sanitize(&self.source),
            sanitize(&encode_sources(&self.sources)),
            sanitize(&encode_sources(&self.favorites)),
            sanitize(&encode_timed_paths(
                &self
                    .bookmarks
                    .iter()
                    .map(|(path, times)| {
                        (
                            path.clone(),
                            times
                                .iter()
                                .map(|time| format!("{time:.3}"))
                                .collect::<Vec<_>>()
                                .join(","),
                        )
                    })
                    .collect::<Vec<_>>()
            )),
            sanitize(&encode_timed_paths(
                &self
                    .resume
                    .iter()
                    .map(|(path, position)| (path.clone(), format!("{position:.3}")))
                    .collect::<Vec<_>>()
            )),
            sanitize(&encode_capture_selections(&self.capture_selections)),
            sanitize(&self.capture_smoothing),
            sanitize(&self.capture_audio_source),
            self.remember_position,
            self.show_markers,
            self.seek_step_seconds,
            self.subtitle_scale,
            sanitize(&self.subtitle_color),
            self.subtitle_top,
            sanitize(&self.subtitle_font),
            self.subtitle_bold,
            self.subtitle_shadow,
            self.ass_styles,
            self.subtitle_graphic_outside,
            self.subtitle_text_inside,
            self.subtitle_bottom,
            self.subtitle_border_opaque,
            self.subtitle_smooth_vmr,
            sanitize(&self.subtitle_3d),
            sanitize(&self.subtitle_language),
            sanitize(&self.stereo_mode),
            sanitize(&self.video_renderer),
            self.avisynth_enabled,
            sanitize(&self.avisynth_script),
            self.vapoursynth_enabled,
            sanitize(&self.vapoursynth_script),
            self.pano_enabled,
            sanitize(&self.pano_mode),
            self.pano_yaw,
            self.pano_pitch,
            self.pano_fov,
            self.tts_enabled,
            self.translate_enabled,
            self.brightness,
            self.contrast,
            self.saturation,
            self.hue,
            self.rotation,
            self.flip_horizontal,
            self.flip_vertical,
            self.width,
            self.height,
            self.fps,
            self.screen1_width,
            self.screen1_height,
            self.screen2_width,
            self.screen2_height,
            self.window_custom_width,
            self.window_custom_height,
            self.osd_visible,
            self.always_on_top,
            sanitize(&self.topmost_mode),
            self.autohide_controls,
            self.fullscreen_no_cover,
            self.ignore_shortcuts,
            self.favorites_memory_points,
            sanitize(&self.similar_mode),
            self.touch_enabled,
            self.move_only_titlebar,
            self.skin_opacity,
            sanitize(&self.skin),
            sanitize(&self.menu_accent),
            sanitize(&self.logo_file),
            sanitize(&self.logo_play_file),
            self.logo_show,
            self.logo_play_show,
            self.skin_layer_output,
            sanitize(&self.popup_menu_type),
            sanitize(&self.menu_output_mode),
            self.touch_skin,
            self.video_skin_start,
            sanitize(&self.language),
            sanitize(&self.shutdown_mode),
            self.remember_post_action,
            self.remember_window,
            sanitize(&self.snapshot_dir.display().to_string()),
            sanitize(&self.snapshot_format),
            sanitize(&self.snapshot_name),
            self.snapshot_play_folder,
            self.snapshot_interval_ms,
            self.snapshot_count,
            self.thumbnail_width,
            sanitize(&self.post_action),
            self.window_w,
            self.window_h,
            self.window_x.map(|v| v.to_string()).unwrap_or_default(),
            self.window_y.map(|v| v.to_string()).unwrap_or_default(),
            self.menu_show_names,
            self.enlarge_skin_menu,
            self.keep_ui_mode,
            self.separate_geometry,
            self.video_window_w
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.video_window_h
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.video_window_x
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.video_window_y
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.audio_window_w
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.audio_window_h
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.audio_window_x
                .map(|v| v.to_string())
                .unwrap_or_default(),
            self.audio_window_y
                .map(|v| v.to_string())
                .unwrap_or_default(),
            sanitize(&self.audio_device),
            sanitize(&self.audio_device2),
            sanitize(&self.audio_passthrough_device),
            self.audio_multi_channel_volume,
            self.audio_max_volume,
            self.audio_ac3_dts,
            self.audio_ac3_dts_gain_db,
            sanitize(&self.audio_output_method),
            self.audio_output_rate,
            self.audio_output_format,
            self.remember_frame,
            self.frame_scale_x,
            self.frame_scale_y,
            self.frame_shift_x,
            self.frame_shift_y,
            sanitize(&self.volume_control),
            sanitize(&self.video_record_dir.display().to_string()),
            self.video_record_play_folder,
            sanitize(&self.audio_record_dir.display().to_string()),
            self.audio_record_play_folder,
            self.audio_processing,
            self.audio_effect_normalize,
            self.audio_effect_crystallize,
            self.audio_effect_denoise,
            self.audio_effect_reverb,
            self.audio_effect_vocal_remove,
            self.audio_effect_vocal_enhance,
            self.audio_equalizer,
            sanitize(&self.audio_eq_preset),
            sanitize(&self.audio_sync_mode),
            self.audio_sync_reset_exit,
            self.audio_sync_ms,
            sanitize(&self.audio_language),
            self.audio_select_language,
            sanitize(&self.default_open_action),
        );
        fs::write(path, text)
    }
}

fn encode_sources(sources: &[String]) -> String {
    sources
        .iter()
        .map(|source| source.replace('\\', "\\\\").replace('|', "\\|"))
        .collect::<Vec<_>>()
        .join("|")
}

fn decode_sources(value: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '|' {
            entries.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    entries.push(current);
    entries
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn encode_timed_paths(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(path, times)| {
            let path = path
                .replace('\\', "\\\\")
                .replace('|', "\\|")
                .replace('\t', " ");
            format!("{path}\t{times}")
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn decode_timed_paths(value: &str) -> Vec<(String, String)> {
    decode_sources(value)
        .into_iter()
        .filter_map(|entry| {
            let (path, times) = entry.split_once('\t')?;
            Some((path.to_string(), times.to_string()))
        })
        .collect()
}

fn encode_capture_selections(entries: &[(String, String, u32, u32)]) -> String {
    entries
        .iter()
        .map(|(device, fourcc, width, height)| {
            let escape = |value: &str| {
                value
                    .replace('\\', "\\\\")
                    .replace('|', "\\|")
                    .replace('\t', " ")
            };
            format!(
                "{}\t{}\t{width}\t{height}",
                escape(device),
                escape(fourcc)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flintplayer")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.ini")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capture_values() {
        let text = "source=/dev/video0\nsources=/dev/video0|/dev/video2\nwidth=1920\nheight=1080\nfps=60\n\
                    osd_visible=true\nremember_window=false\nsnapshot_dir=/tmp/shots\n";
        let parsed = Config::parse(text);
        assert_eq!(parsed.source, "/dev/video0");
        assert_eq!(parsed.sources, vec!["/dev/video0", "/dev/video2"]);
        assert_eq!((parsed.width, parsed.height, parsed.fps), (1920, 1080, 60));
        assert!(parsed.osd_visible);
        assert!(!parsed.remember_window);
        assert_eq!(parsed.snapshot_dir, PathBuf::from("/tmp/shots"));
    }

    #[test]
    fn parses_frame_geometry_memory() {
        let text = "remember_frame=true\nframe_scale_x=150\nframe_scale_y=120\n\
                    frame_shift_x=-5\nframe_shift_y=7\n";
        let parsed = Config::parse(text);
        assert!(parsed.remember_frame);
        assert_eq!(
            (
                parsed.frame_scale_x,
                parsed.frame_scale_y,
                parsed.frame_shift_x,
                parsed.frame_shift_y
            ),
            (150, 120, -5, 7)
        );
        let clamped = Config::parse("remember_frame=true\nframe_scale_x=9999\nframe_shift_y=-999\n");
        assert_eq!(clamped.frame_scale_x, 400);
        assert_eq!(clamped.frame_shift_y, -50);
        let unremembered = Config::parse("frame_scale_x=150\nframe_scale_y=120\nframe_shift_x=-5\nframe_shift_y=7\n");
        assert_eq!(
            (
                unremembered.frame_scale_x,
                unremembered.frame_scale_y,
                unremembered.frame_shift_x,
                unremembered.frame_shift_y
            ),
            (100, 100, 0, 0)
        );
    }

    #[test]
    fn parses_volume_control_mode() {
        assert_eq!(
            Config::parse("volume_control=system-master\n").volume_control,
            "system-master"
        );
        assert_eq!(
            Config::parse("volume_control=bogus\n").volume_control,
            "default"
        );
    }

    #[test]
    fn parses_recording_directories() {
        let parsed = Config::parse(
            "video_record_dir=/tmp/videos\nvideo_record_play_folder=true\n\
             audio_record_dir=/tmp/audio\naudio_record_play_folder=true\n",
        );
        assert_eq!(parsed.video_record_dir, PathBuf::from("/tmp/videos"));
        assert!(parsed.video_record_play_folder);
        assert_eq!(parsed.audio_record_dir, PathBuf::from("/tmp/audio"));
        assert!(parsed.audio_record_play_folder);
        let defaults = Config::parse("");
        assert!(!defaults.video_record_play_folder);
        assert!(!defaults.audio_record_play_folder);
    }

    #[test]
    fn parses_audio_effects() {
        let parsed = Config::parse(
            "audio_processing=true\naudio_effect_normalize=true\n\
             audio_effect_crystallize=1\naudio_effect_denoise=true\n\
             audio_effect_reverb=true\naudio_effect_vocal_remove=true\n\
             audio_effect_vocal_enhance=true\n",
        );
        assert!(parsed.audio_processing);
        assert!(parsed.audio_effect_normalize);
        assert!(parsed.audio_effect_crystallize);
        assert!(parsed.audio_effect_denoise);
        assert!(parsed.audio_effect_reverb);
        assert!(parsed.audio_effect_vocal_remove);
        assert!(parsed.audio_effect_vocal_enhance);
        let defaults = Config::parse("");
        assert!(!defaults.audio_processing);
        assert!(!defaults.audio_effect_normalize);
    }

    #[test]
    fn parses_equalizer_settings() {
        let parsed = Config::parse("audio_equalizer=true\naudio_eq_preset=rock\n");
        assert!(parsed.audio_equalizer);
        assert_eq!(parsed.audio_eq_preset, "rock");
        let defaults = Config::parse("");
        assert!(!defaults.audio_equalizer);
        assert_eq!(defaults.audio_eq_preset, "flat");
        assert_eq!(
            Config::parse("audio_eq_preset=bogus\n").audio_eq_preset,
            "flat"
        );
    }

    #[test]
    fn parses_audio_sync_settings() {
        let parsed = Config::parse(
            "audio_sync_mode=always\naudio_sync_reset_exit=true\naudio_sync_ms=-150\n",
        );
        assert_eq!(parsed.audio_sync_mode, "always");
        assert!(parsed.audio_sync_reset_exit);
        assert_eq!(parsed.audio_sync_ms, -150);
        let defaults = Config::parse("");
        assert_eq!(defaults.audio_sync_mode, "none");
        assert!(!defaults.audio_sync_reset_exit);
        assert_eq!(defaults.audio_sync_ms, 0);
        assert_eq!(
            Config::parse("audio_sync_mode=bogus\n").audio_sync_mode,
            "none"
        );
    }

    #[test]
    fn parses_audio_language_settings() {
        let parsed = Config::parse("audio_language=jpn\naudio_select_language=true\n");
        assert_eq!(parsed.audio_language, "jpn");
        assert!(parsed.audio_select_language);
        let defaults = Config::parse("");
        assert_eq!(defaults.audio_language, "auto");
        assert!(!defaults.audio_select_language);
        assert_eq!(
            Config::parse("audio_language=bogus\n").audio_language,
            "auto"
        );
    }

    #[test]
    fn parses_default_open_action() {
        let parsed = Config::parse("default_open_action=navigate\n");
        assert_eq!(parsed.default_open_action, "navigate");
        assert_eq!(Config::parse("").default_open_action, "file");
        assert_eq!(
            Config::parse("default_open_action=bogus\n").default_open_action,
            "file"
        );
    }

    #[test]
    fn keeps_source_url_intact() {
        let parsed = Config::parse("source=rtsp://cam.local/live?channel=1&x=2\n");
        assert_eq!(parsed.source, "rtsp://cam.local/live?channel=1&x=2");
    }

    #[test]
    fn round_trips_sources_with_separator_characters() {
        let text = "sources=rtsp://cam/live\\|main|/dev/video2|C:\\\\share\n";
        let parsed = Config::parse(text);
        assert_eq!(
            parsed.sources,
            vec!["rtsp://cam/live|main", "/dev/video2", "C:\\share"]
        );
        assert_eq!(
            encode_sources(&parsed.sources),
            "rtsp://cam/live\\|main|/dev/video2|C:\\\\share"
        );
    }

    #[test]
    fn round_trips_favorites() {
        let text = "favorites=/home/u/a.mkv|/home/u/番剧|C:\\\\share\n";
        let parsed = Config::parse(text);
        assert_eq!(
            parsed.favorites,
            vec!["/home/u/a.mkv", "/home/u/番剧", "C:\\share"]
        );
        assert_eq!(
            encode_sources(&parsed.favorites),
            "/home/u/a.mkv|/home/u/番剧|C:\\\\share"
        );
    }

    #[test]
    fn round_trips_bookmarks_and_resume() {
        let text = "bookmarks=/home/u/番剧 ep1.mkv\t12.500,45.000|C:\\\\share\\\\b.mkv\t3.000\n\
                    resume=/home/u/番剧 ep1.mkv\t120.500\n\
                    remember_position=false\nshow_markers=false\n";
        let parsed = Config::parse(text);
        assert_eq!(
            parsed.bookmarks,
            vec![
                ("/home/u/番剧 ep1.mkv".to_string(), vec![12.5, 45.0]),
                ("C:\\share\\b.mkv".to_string(), vec![3.0]),
            ]
        );
        assert_eq!(
            parsed.resume,
            vec![("/home/u/番剧 ep1.mkv".to_string(), 120.5)]
        );
        assert!(!parsed.remember_position);
        assert!(!parsed.show_markers);
        assert_eq!(parsed.subtitle_scale, 1.0);
        assert_eq!(parsed.subtitle_color, "white");
        assert!(!parsed.subtitle_top);
        assert_eq!(parsed.subtitle_font, "");
        assert!(!parsed.subtitle_bold);
        assert!(parsed.subtitle_shadow);
        assert!(parsed.ass_styles);
        assert_eq!(parsed.subtitle_3d, "off");

        let encoded = encode_timed_paths(&[
            (
                "/home/u/番剧 ep1.mkv".to_string(),
                "12.500,45.000".to_string(),
            ),
            ("C:\\share|b.mkv".to_string(), "3.000".to_string()),
        ]);
        let decoded = decode_timed_paths(&encoded);
        assert_eq!(
            decoded,
            vec![
                (
                    "/home/u/番剧 ep1.mkv".to_string(),
                    "12.500,45.000".to_string()
                ),
                ("C:\\share|b.mkv".to_string(), "3.000".to_string()),
            ]
        );
    }

    #[test]
    fn round_trips_capture_selections() {
        let text =
            "capture_selections=/dev/video0\tMJPG\t1280\t720|/dev/video2\tYUYV\t640\t480\n";
        let parsed = Config::parse(text);
        assert_eq!(
            parsed.capture_selections,
            vec![
                ("/dev/video0".to_string(), "MJPG".to_string(), 1280, 720),
                ("/dev/video2".to_string(), "YUYV".to_string(), 640, 480),
            ]
        );
        assert_eq!(
            encode_capture_selections(&parsed.capture_selections),
            "/dev/video0\tMJPG\t1280\t720|/dev/video2\tYUYV\t640\t480"
        );
    }

    #[test]
    fn round_trips_capture_smoothing() {
        let parsed = Config::parse("capture_smoothing=high\n");
        assert_eq!(parsed.capture_smoothing, "high");
        assert_eq!(Config::parse("capture_smoothing=bogus\n").capture_smoothing, "system");
        assert_eq!(Config::parse("").capture_smoothing, "system");
        let path = std::env::temp_dir().join(format!(
            "flint-config-smoothing-{}.ini",
            std::process::id()
        ));
        Config {
            capture_smoothing: "medium".into(),
            ..Config::default()
        }
        .save_to(&path)
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(saved.contains("capture_smoothing=medium\n"));
        assert_eq!(Config::parse(&saved).capture_smoothing, "medium");
    }

    #[test]
    fn round_trips_capture_audio_source() {
        let parsed = Config::parse("capture_audio_source=alsa_input.mic\n");
        assert_eq!(parsed.capture_audio_source, "alsa_input.mic");
        assert_eq!(Config::parse("").capture_audio_source, "");
        let path = std::env::temp_dir().join(format!(
            "flint-config-audio-{}.ini",
            std::process::id()
        ));
        Config {
            capture_audio_source: "alsa_input.mic".into(),
            ..Config::default()
        }
        .save_to(&path)
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(saved.contains("capture_audio_source=alsa_input.mic\n"));
        assert_eq!(Config::parse(&saved).capture_audio_source, "alsa_input.mic");
    }

    #[test]
    fn parses_subtitle_style() {
        let text =
            "subtitle_scale=1.3\nsubtitle_color=green\nsubtitle_top=true\n\
             subtitle_font=Noto Serif CJK SC\nsubtitle_bold=true\nsubtitle_shadow=false\nass_styles=false\nsubtitle_3d=tab\n";
        let parsed = Config::parse(text);
        assert_eq!(parsed.subtitle_scale, 1.3);
        assert_eq!(parsed.subtitle_color, "green");
        assert!(parsed.subtitle_top);
        assert_eq!(parsed.subtitle_font, "Noto Serif CJK SC");
        assert!(parsed.subtitle_bold);
        assert!(!parsed.subtitle_shadow);
        assert!(!parsed.ass_styles);
        assert_eq!(parsed.subtitle_3d, "tab");
        assert_eq!(parsed.subtitle_language, "auto");
        assert_eq!(
            Config::parse("subtitle_language=eng\n").subtitle_language,
            "eng"
        );
        assert_eq!(
            Config::parse("subtitle_language=bogus\n").subtitle_language,
            "auto"
        );
    }

    #[test]
    fn parses_panorama_view() {
        let parsed = Config::parse(
            "pano_enabled=true\npano_yaw=45\npano_pitch=200\npano_fov=10\ntts_enabled=true\n",
        );
        assert!(parsed.pano_enabled);
        assert_eq!(parsed.pano_yaw, 45.0);
        assert_eq!(parsed.pano_pitch, 89.0);
        assert_eq!(parsed.pano_fov, 30.0);
        assert!(parsed.tts_enabled);
    }

    #[test]
    fn parses_video_adjustments() {
        let parsed = Config::parse(
            "brightness=25\ncontrast=200\nsaturation=-40\nrotation=90\n\
             flip_horizontal=true\nflip_vertical=1\n",
        );
        assert_eq!(parsed.brightness, 25);
        assert_eq!(parsed.contrast, 100);
        assert_eq!(parsed.saturation, -40);
        assert_eq!(parsed.rotation, 90);
        assert!(parsed.flip_horizontal);
        assert!(parsed.flip_vertical);
        assert_eq!(Config::parse("rotation=45\n").rotation, 0);
    }

    #[test]
    fn parses_topmost_mode() {
        assert_eq!(
            Config::parse("topmost_mode=playing\n").topmost_mode,
            "playing"
        );
        assert_eq!(Config::parse("topmost_mode=bogus\n").topmost_mode, "never");
        let legacy = Config::parse("always_on_top=true\n");
        assert!(legacy.always_on_top);
        assert_eq!(legacy.topmost_mode, "always");
        let legacy_off = Config::parse("always_on_top=false\n");
        assert!(!legacy_off.always_on_top);
        assert_eq!(legacy_off.topmost_mode, "never");
    }

    #[test]
    fn parses_skin_toggles() {
        let parsed = Config::parse(
            "autohide_controls=false\nfullscreen_no_cover=true\n\
             ignore_shortcuts=true\nfavorites_memory_points=1\nsimilar_mode=all\n\
             touch_enabled=false\nmove_only_titlebar=false\n\
             shutdown_mode=suspend\nremember_post_action=false\n",
        );
        assert!(!parsed.autohide_controls);
        assert!(parsed.fullscreen_no_cover);
        assert!(parsed.ignore_shortcuts);
        assert!(parsed.favorites_memory_points);
        assert_eq!(parsed.similar_mode, "all");
        assert!(!parsed.touch_enabled);
        assert!(!parsed.move_only_titlebar);
        let defaults = Config::parse("");
        assert!(defaults.move_only_titlebar);
        assert!(defaults.autohide_controls);
        assert!(!defaults.fullscreen_no_cover);
        assert!(!defaults.ignore_shortcuts);
        assert!(!defaults.favorites_memory_points);
        assert_eq!(defaults.similar_mode, "selected");
        assert!(defaults.touch_enabled);
        assert_eq!(parsed.shutdown_mode, "suspend");
        assert!(!parsed.remember_post_action);
        assert_eq!(defaults.shutdown_mode, "poweroff");
        assert!(defaults.remember_post_action);
        assert_eq!(
            Config::parse("similar_mode=bogus\n").similar_mode,
            "selected"
        );
    }

    #[test]
    fn parses_skin_opacity() {
        assert!((Config::parse("skin_opacity=0.75\n").skin_opacity - 0.75).abs() < 1e-6);
        assert_eq!(Config::parse("").skin_opacity, 1.0);
        assert_eq!(Config::parse("skin_opacity=9\n").skin_opacity, 1.0);
        assert_eq!(Config::parse("skin_opacity=0.1\n").skin_opacity, 0.4);
    }

    #[test]
    fn parses_skin_selection() {
        assert_eq!(Config::parse("").skin, "默认皮肤");
        assert_eq!(Config::parse("skin=琥珀皮肤\n").skin, "琥珀皮肤");
        assert_eq!(
            Config::parse("skin=/home/user/.config/flintplayer/skins/ocean.fskin\n").skin,
            "/home/user/.config/flintplayer/skins/ocean.fskin"
        );
        assert_eq!(Config::parse("").menu_accent, "");
        assert_eq!(Config::parse("menu_accent=purple\n").menu_accent, "purple");
        assert_eq!(Config::parse("menu_accent=bogus\n").menu_accent, "");
        assert_eq!(Config::parse("").logo_file, "");
        assert!(Config::parse("").logo_show);
        assert!(!Config::parse("logo_show=false\nlogo_play_show=0\n").logo_show);
        assert!(!Config::parse("logo_show=false\nlogo_play_show=0\n").logo_play_show);
        assert_eq!(
            Config::parse("logo_file=/tmp/a.png\nlogo_play_file=/tmp/b.png\n").logo_play_file,
            "/tmp/b.png"
        );
    }

    #[test]
    fn parses_skin_output_options() {
        let defaults = Config::parse("");
        assert!(defaults.skin_layer_output);
        assert_eq!(defaults.popup_menu_type, "skin");
        assert_eq!(defaults.menu_output_mode, "auto");
        assert!(!defaults.touch_skin);
        assert!(!defaults.video_skin_start);

        let parsed = Config::parse(
            "skin_layer_output=false\npopup_menu_type=system\nmenu_output_mode=dark\ntouch_skin=true\nvideo_skin_start=1\n",
        );
        assert!(!parsed.skin_layer_output);
        assert_eq!(parsed.popup_menu_type, "system");
        assert_eq!(parsed.menu_output_mode, "dark");
        assert!(parsed.touch_skin);
        assert!(parsed.video_skin_start);

        let invalid = Config::parse("popup_menu_type=bogus\nmenu_output_mode=bogus\n");
        assert_eq!(invalid.popup_menu_type, "skin");
        assert_eq!(invalid.menu_output_mode, "auto");
    }

    #[test]
    fn parses_skin_menu_layout_options() {
        let defaults = Config::parse("");
        assert!(defaults.menu_show_names);
        assert!(defaults.enlarge_skin_menu);
        assert!(defaults.keep_ui_mode);
        assert!(!defaults.separate_geometry);
        assert_eq!(defaults.video_window_w, None);
        assert_eq!(defaults.audio_window_x, None);

        let parsed = Config::parse(
            "menu_show_names=false\nenlarge_skin_menu=0\nkeep_ui_mode=false\nseparate_geometry=1\nvideo_window_w=1600\nvideo_window_h=900\nvideo_window_x=40\nvideo_window_y=60\naudio_window_w=900\naudio_window_h=600\n",
        );
        assert!(!parsed.menu_show_names);
        assert!(!parsed.enlarge_skin_menu);
        assert!(!parsed.keep_ui_mode);
        assert!(parsed.separate_geometry);
        assert_eq!(parsed.video_window_w, Some(1600.0));
        assert_eq!(parsed.video_window_h, Some(900.0));
        assert_eq!(parsed.video_window_x, Some(40.0));
        assert_eq!(parsed.video_window_y, Some(60.0));
        assert_eq!(parsed.audio_window_w, Some(900.0));
        assert_eq!(parsed.audio_window_h, Some(600.0));

        let clamped = Config::parse("video_window_w=100\nvideo_window_h=100\n");
        assert_eq!(clamped.video_window_w, Some(720.0));
        assert_eq!(clamped.video_window_h, Some(480.0));
    }

    #[test]
    fn round_trips_skin_layout_options() {
        let config = Config::parse(
            "menu_show_names=false\nenlarge_skin_menu=false\nkeep_ui_mode=false\nseparate_geometry=true\nvideo_window_w=1600\nvideo_window_h=900\nvideo_window_x=40\nvideo_window_y=60\naudio_window_w=900\naudio_window_h=600\naudio_window_x=12\naudio_window_y=34\n",
        );
        let path = std::env::temp_dir().join(format!(
            "flint-config-{}-{}.ini",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        config.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let parsed = Config::parse(&text);
        assert!(!parsed.menu_show_names);
        assert!(!parsed.enlarge_skin_menu);
        assert!(!parsed.keep_ui_mode);
        assert!(parsed.separate_geometry);
        assert_eq!(parsed.video_window_w, config.video_window_w);
        assert_eq!(parsed.video_window_h, config.video_window_h);
        assert_eq!(parsed.video_window_x, config.video_window_x);
        assert_eq!(parsed.video_window_y, config.video_window_y);
        assert_eq!(parsed.audio_window_w, config.audio_window_w);
        assert_eq!(parsed.audio_window_h, config.audio_window_h);
        assert_eq!(parsed.audio_window_x, config.audio_window_x);
        assert_eq!(parsed.audio_window_y, config.audio_window_y);
    }

    #[test]
    fn round_trips_subtitle_style_and_synth_options() {
        let defaults = Config::parse("");
        assert!(!defaults.subtitle_graphic_outside);
        assert!(defaults.subtitle_text_inside);
        assert!(defaults.subtitle_bottom);
        assert!(!defaults.subtitle_border_opaque);
        assert!(!defaults.subtitle_smooth_vmr);
        assert!(!defaults.avisynth_enabled);
        assert!(!defaults.vapoursynth_enabled);
        assert!(defaults.avisynth_script.is_empty());

        let config = Config::parse(
            "subtitle_graphic_outside=true\nsubtitle_text_inside=false\nsubtitle_bottom=false\nsubtitle_border_opaque=true\nsubtitle_smooth_vmr=1\navisynth_enabled=true\navisynth_script=/tmp/filter.avs\nvapoursynth_enabled=1\nvapoursynth_script=/tmp/filter.vpy\n",
        );
        assert!(config.subtitle_graphic_outside);
        assert!(!config.subtitle_text_inside);
        assert!(!config.subtitle_bottom);
        assert!(config.subtitle_border_opaque);
        assert!(config.subtitle_smooth_vmr);
        assert!(config.avisynth_enabled);
        assert_eq!(config.avisynth_script, "/tmp/filter.avs");
        assert!(config.vapoursynth_enabled);
        assert_eq!(config.vapoursynth_script, "/tmp/filter.vpy");

        let path = std::env::temp_dir().join(format!(
            "flint-config-synth-{}-{}.ini",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        config.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let parsed = Config::parse(&text);
        assert!(parsed.subtitle_graphic_outside);
        assert!(!parsed.subtitle_text_inside);
        assert!(!parsed.subtitle_bottom);
        assert!(parsed.subtitle_border_opaque);
        assert!(parsed.subtitle_smooth_vmr);
        assert!(parsed.avisynth_enabled);
        assert_eq!(parsed.avisynth_script, "/tmp/filter.avs");
        assert!(parsed.vapoursynth_enabled);
        assert_eq!(parsed.vapoursynth_script, "/tmp/filter.vpy");
    }

    #[test]
    fn parses_language() {
        assert_eq!(Config::parse("").language, "auto");
        assert_eq!(Config::parse("language=zh\n").language, "zh");
        assert_eq!(Config::parse("language=en\n").language, "en");
        assert_eq!(Config::parse("language=bogus\n").language, "auto");
    }

    #[test]
    fn parses_post_action() {
        assert_eq!(
            Config::parse("post_action=off-current\n").post_action,
            "off-current"
        );
        assert_eq!(
            Config::parse("post_action=off-all\n").post_action,
            "off-all"
        );
        assert_eq!(
            Config::parse("post_action=exit-all\n").post_action,
            "exit-all"
        );
        assert_eq!(Config::parse("post_action=bogus\n").post_action, "none");
    }

    #[test]
    fn parses_snapshot_burst() {
        let parsed = Config::parse("snapshot_interval_ms=250\nsnapshot_count=5\n");
        assert_eq!(parsed.snapshot_interval_ms, 250);
        assert_eq!(parsed.snapshot_count, 5);
        let clamped = Config::parse("snapshot_interval_ms=10\nsnapshot_count=999\n");
        assert_eq!(clamped.snapshot_interval_ms, 100);
        assert_eq!(clamped.snapshot_count, 100);
        let defaults = Config::parse("");
        assert_eq!(defaults.snapshot_interval_ms, 1000);
        assert_eq!(defaults.snapshot_count, 10);
    }

    #[test]
    fn parses_thumbnail_width() {
        assert_eq!(Config::parse("thumbnail_width=640\n").thumbnail_width, 640);
        assert_eq!(Config::parse("thumbnail_width=1\n").thumbnail_width, 64);
        assert_eq!(
            Config::parse("thumbnail_width=99999\n").thumbnail_width,
            3840
        );
        assert_eq!(Config::parse("").thumbnail_width, 320);
    }

    #[test]
    fn clamps_window_size() {
        let parsed = Config::parse("window_w=100\nwindow_h=100\n");
        assert!(parsed.window_w >= 720.0);
        assert!(parsed.window_h >= 480.0);
    }
}
