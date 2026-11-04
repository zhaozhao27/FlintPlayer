use std::sync::Arc;
use std::time::Duration;

use flint_media::{CaptureEngine, CaptureSettings, CaptureSmoothing, Shared, SourceInfo};
use gpui::*;

pub struct CaptureController {
    engine: Option<CaptureEngine>,
    shared: Option<Arc<Shared>>,
    info: Option<SourceInfo>,
    device_name: String,
    source: Option<String>,
    settings: CaptureSettings,
    texture: Option<DynamicTexture>,
    texture_size: Option<(u32, u32)>,
    frame_size: Option<(u32, u32)>,
    scratch: Vec<u8>,
    dirty: bool,
    error: Option<String>,
    ticker_started: bool,
}

impl CaptureController {
    pub fn new() -> Self {
        Self {
            engine: None,
            shared: None,
            info: None,
            device_name: String::new(),
            source: None,
            settings: CaptureSettings::default(),
            texture: None,
            texture_size: None,
            frame_size: None,
            scratch: Vec::new(),
            dirty: false,
            error: None,
            ticker_started: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.info.is_some()
    }

    pub fn info(&self) -> Option<&SourceInfo> {
        self.info.as_ref()
    }

    pub fn label(&self) -> String {
        let Some(info) = &self.info else {
            return String::new();
        };
        if self.device_name.is_empty() {
            info.source.clone()
        } else {
            self.device_name.clone()
        }
    }

    pub fn stats(&self) -> Option<flint_core::StatsSnapshot> {
        self.shared.as_ref().map(|shared| shared.stats_snapshot())
    }

    pub fn decoded_pixel_format(&self) -> Option<String> {
        self.shared.as_ref().and_then(|shared| shared.pixel_format())
    }

    pub fn frame_size(&self) -> Option<(u32, u32)> {
        self.frame_size
    }

    pub fn elapsed(&self) -> f64 {
        self.shared
            .as_ref()
            .and_then(|shared| shared.status().started_at)
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn texture(&self) -> Option<DynamicTexture> {
        self.texture
    }

    pub fn texture_size(&self) -> Option<(u32, u32)> {
        self.texture_size
    }

    pub fn display_info(&self) -> String {
        let Some(info) = &self.info else {
            return crate::i18n::tr("未打开媒体");
        };
        let name = if self.device_name.is_empty() {
            info.source.clone()
        } else {
            self.device_name.clone()
        };
        format!(
            "{} · {} · {}×{}",
            name, info.pixel_format, info.width, info.height
        )
    }

    pub fn start(&mut self, source: String, name: String, cx: &mut Context<Self>) {
        self.start_with(source, name, CaptureSettings::default(), cx);
    }

    pub fn start_with(
        &mut self,
        source: String,
        name: String,
        settings: CaptureSettings,
        cx: &mut Context<Self>,
    ) {
        self.stop(cx);
        self.device_name = name;
        self.source = Some(source.clone());
        self.settings = settings.clone();
        self.error = None;
        let task = cx.background_executor().spawn(async move {
            let mut engine = CaptureEngine::new();
            engine
                .start(&source, settings)
                .map(|info| (engine, info))
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((engine, info)) => {
                        this.shared = Some(engine.shared());
                        this.engine = Some(engine);
                        this.info = Some(info);
                        this.error = None;
                        if !this.ticker_started {
                            this.ticker_started = true;
                            this.start_ticker(cx);
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_smoothing(&mut self, smoothing: CaptureSmoothing, cx: &mut Context<Self>) {
        self.settings.smoothing = smoothing;
        let Some(source) = self.source.clone().filter(|_| self.is_active()) else {
            return;
        };
        self.start_with(source, self.device_name.clone(), self.settings.clone(), cx);
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(mut engine) = self.engine.take() {
            engine.stop();
        }
        self.shared = None;
        self.info = None;
        self.frame_size = None;
        self.dirty = false;
        self.error = None;
        cx.notify();
    }

    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let updated = this.update(cx, |this, cx| {
                this.poll_frame();
                cx.notify();
            });
            if updated.is_err() {
                break;
            }
        })
        .detach();
    }

    fn poll_frame(&mut self) {
        let Some(shared) = &self.shared else {
            return;
        };
        let Some(frame) = shared.slot.take() else {
            return;
        };
        let (width, height) = (frame.width as u32, frame.height as u32);
        if width == 0 || height == 0 || frame.rgba.len() < (width as usize * height as usize * 4) {
            return;
        }
        self.scratch.resize(width as usize * height as usize * 4, 0);
        for (destination, source) in self
            .scratch
            .chunks_exact_mut(4)
            .zip(frame.rgba.chunks_exact(4))
        {
            destination[0] = source[2];
            destination[1] = source[1];
            destination[2] = source[0];
            destination[3] = 255;
        }
        self.frame_size = Some((width, height));
        self.dirty = true;
    }

    pub fn upload_frame(&mut self, window: &mut Window) {
        if !self.dirty {
            return;
        }
        let Some((width, height)) = self.frame_size else {
            return;
        };
        self.dirty = false;
        let stride = width * 4;
        let dimensions = Size::new(DevicePixels(width as i32), DevicePixels(height as i32));
        let bounds = Bounds::new(Point::new(DevicePixels(0), DevicePixels(0)), dimensions);

        if let Some(texture) = self.texture {
            if window
                .update_dynamic_texture(texture, bounds, &self.scratch, stride)
                .is_ok()
            {
                self.texture_size = Some((width, height));
                return;
            }
            window.remove_dynamic_texture(texture);
            self.texture = None;
        }

        match window.create_dynamic_texture(dimensions, &self.scratch, stride) {
            Ok(texture) => {
                self.texture = Some(texture);
                self.texture_size = Some((width, height));
            }
            Err(error) => self.error = Some(format!("创建采集纹理失败：{error}")),
        }
    }
}
