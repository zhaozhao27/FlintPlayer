use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use parking_lot::Mutex;

use crate::mpv::{
    self, get_property_string, MpvApi, MpvEvent, MpvEventEndFile, MpvHandle, MpvRenderContext,
    MpvRenderParam, MPV_END_FILE_REASON_ERROR, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED,
    MPV_EVENT_IDLE, MPV_EVENT_SHUTDOWN, MPV_RENDER_API_TYPE_SW, MPV_RENDER_PARAM_API_TYPE,
    MPV_RENDER_PARAM_INVALID, MPV_RENDER_PARAM_SW_FORMAT, MPV_RENDER_PARAM_SW_POINTER,
    MPV_RENDER_PARAM_SW_SIZE, MPV_RENDER_PARAM_SW_STRIDE, MPV_RENDER_UPDATE_FRAME,
};
use crate::EngineError;

const EVENT_TIMEOUT_SECS: f64 = 0.1;
const SW_FORMAT_BGR0: &[u8; 5] = b"bgr0\0";

/// Snapshot of the player state, refreshed by the background event thread.
#[derive(Clone, Debug, Default)]
pub struct PlayerState {
    /// Playback position in seconds.
    pub position: f64,
    /// Media duration in seconds.
    pub duration: f64,
    /// Whether playback is paused.
    pub paused: bool,
    /// Whether the current file reached its end.
    pub eof: bool,
    /// Title reported by mpv's `media-title` property.
    pub title: String,
    /// Video width in pixels.
    pub width: u32,
    /// Video height in pixels.
    pub height: u32,
    /// Volume as an mpv percentage (100 = normal).
    pub volume: f64,
    /// Whether a file is currently loaded.
    pub loaded: bool,
    /// Video gamma reported by mpv (`pq`, `hlg`, or a transfer function name).
    pub video_gamma: String,
    /// Mastering peak luminance in nits reported by mpv, 0.0 when unknown.
    pub sig_peak: f64,
    /// Last playback error reported by mpv, if any.
    pub error: Option<String>,
}

/// A chapter exposed by the current media.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChapterInfo {
    /// Chapter title as reported by mpv.
    pub title: String,
    /// Start time in seconds.
    pub start: f64,
}

/// An audio, video, or subtitle track of the current media.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackInfo {
    /// mpv track id used with `set_track`.
    pub id: i64,
    /// Track kind (`audio`, `video`, or `sub`).
    pub kind: String,
    /// Track title, empty when the container does not provide one.
    pub title: String,
    /// ISO language code, empty when unknown.
    pub lang: String,
    /// Whether mpv currently uses this track.
    pub selected: bool,
    /// Whether the track comes from an external file.
    pub external: bool,
    /// Codec name reported by mpv, empty when unknown.
    pub codec: String,
    /// Nominal frame rate from the demuxer, zero for non-video tracks.
    pub fps: f64,
    /// Demuxed bitrate in bits per second, zero when unknown.
    pub bitrate: f64,
}

/// A libmpv playback session.
///
/// The handle may be moved between threads (the libmpv client API serializes
/// calls internally), but [`MpvPlayer::render_frame`] must only be called from
/// one thread because the render context is bound to its creating thread.
pub struct MpvPlayer {
    api: Arc<MpvApi>,
    handle: *mut MpvHandle,
    state: Arc<Mutex<PlayerState>>,
    stop: Arc<AtomicBool>,
    event_thread: Option<JoinHandle<()>>,
    render: Option<RenderTarget>,
}

struct RenderTarget {
    context: *mut MpvRenderContext,
    width: u32,
    height: u32,
    buffer: Vec<u8>,
}

// SAFETY: libmpv documents the client API as fully thread-safe; the event
// thread only uses the handle through the API and is joined before destroy.
unsafe impl Send for MpvPlayer {}

impl MpvPlayer {
    /// Creates a player and starts its event thread.
    pub fn new() -> Result<Self, EngineError> {
        let api = MpvApi::load()?;

        // SAFETY: mpv_create returns a new handle owned by us.
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err(EngineError::CreateFailed);
        }

        let options = [
            ("vo", "libmpv"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("pause", "yes"),
            ("terminal", "no"),
            ("input-default-bindings", "no"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("audio-client-name", "FlintPlayer"),
        ];
        for (name, value) in options {
            if let Err(error) = mpv::set_option_string(&api, handle, name, value) {
                // SAFETY: the handle was created above and is not shared yet.
                unsafe { (api.destroy)(handle) };
                return Err(error);
            }
        }

        // SAFETY: the handle is valid and options were set before initialize.
        let code = unsafe { (api.initialize)(handle) };
        if let Err(error) = api.check(code) {
            // SAFETY: mpv_destroy accepts initialized and uninitialized handles.
            unsafe { (api.destroy)(handle) };
            return Err(error);
        }

        // SAFETY: mpv_client_api_version has no preconditions.
        let version = unsafe { (api.client_api_version)() };
        log::info!(
            "libmpv client API {}.{} loaded",
            version >> 16,
            version & 0xffff
        );

        // vo=libmpv aborts video output initialization unless a render context
        // already exists, so the client must not wait for the first frame.
        let render_context = match create_render_context(&api, handle) {
            Ok(context) => context,
            Err(error) => {
                // SAFETY: no event thread uses the initialized handle yet.
                unsafe { (api.destroy)(handle) };
                return Err(error);
            }
        };

        let state = Arc::new(Mutex::new(PlayerState {
            paused: true,
            volume: 100.0,
            ..PlayerState::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_handle = SendHandle(handle);
        let thread_api = Arc::clone(&api);
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let event_thread = match thread::Builder::new()
            .name("mpv-event".to_string())
            .spawn(move || event_loop(thread_api, thread_handle, thread_state, thread_stop))
        {
            Ok(thread) => thread,
            Err(error) => {
                // SAFETY: no other thread is using the handle yet.
                unsafe { (api.destroy)(handle) };
                return Err(EngineError::ThreadSpawn(error.to_string()));
            }
        };

        Ok(Self {
            api,
            handle,
            state,
            stop,
            event_thread: Some(event_thread),
            render: Some(RenderTarget {
                context: render_context,
                width: 0,
                height: 0,
                buffer: Vec::new(),
            }),
        })
    }

    /// Loads `path` and replaces the current playlist entry.
    pub fn open(&self, path: &str) -> Result<(), EngineError> {
        {
            let mut state = self.state.lock();
            state.loaded = false;
            state.eof = false;
            state.error = None;
            state.position = 0.0;
            state.duration = 0.0;
            state.width = 0;
            state.height = 0;
            state.title.clear();
        }
        let command = format!("loadfile {} replace", quote_argument(path));
        mpv::command_string(&self.api, self.handle, &command)
    }

    /// Starts or resumes playback.
    pub fn play(&self) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, "pause", "no")
    }

    /// Pauses playback.
    pub fn pause(&self) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, "pause", "yes")
    }

    /// Sets an arbitrary mpv property from its string representation.
    pub fn set_property(&self, name: &str, value: &str) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, name, value)
    }

    /// Runs an mpv command line such as `screenshot-to-file "a.png" video`.
    pub fn command(&self, command: &str) -> Result<(), EngineError> {
        mpv::command_string(&self.api, self.handle, command)
    }

    /// Flips the pause state.
    pub fn toggle_pause(&self) -> Result<(), EngineError> {
        let paused = self.state.lock().paused;
        if paused {
            self.play()
        } else {
            self.pause()
        }
    }

    /// Seeks to an absolute position in seconds.
    pub fn seek_absolute(&self, seconds: f64) -> Result<(), EngineError> {
        self.seek(seconds, "absolute")
    }

    /// Seeks by a relative offset in seconds.
    pub fn seek_relative(&self, seconds: f64) -> Result<(), EngineError> {
        self.seek(seconds, "relative")
    }

    /// Sets the volume as an mpv percentage (100 = normal).
    pub fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, "volume", &volume.to_string())
    }

    /// Returns the chapters of the current media.
    pub fn chapters(&self) -> Vec<ChapterInfo> {
        let count = get_u32(&self.api, self.handle, "chapter-list/count").unwrap_or(0);
        (0..count)
            .map(|index| ChapterInfo {
                title: get_property_string(&self.api, self.handle, &format!("chapter-list/{index}/title"))
                    .unwrap_or_default(),
                start: get_f64(&self.api, self.handle, &format!("chapter-list/{index}/time"))
                    .unwrap_or(0.0),
            })
            .collect()
    }

    /// Returns the tracks of the current media.
    pub fn tracks(&self) -> Vec<TrackInfo> {
        let count = get_u32(&self.api, self.handle, "track-list/count").unwrap_or(0);
        (0..count)
            .map(|index| TrackInfo {
                id: get_f64(&self.api, self.handle, &format!("track-list/{index}/id"))
                    .unwrap_or(0.0) as i64,
                kind: get_property_string(&self.api, self.handle, &format!("track-list/{index}/type"))
                    .unwrap_or_default(),
                title: get_property_string(&self.api, self.handle, &format!("track-list/{index}/title"))
                    .unwrap_or_default(),
                lang: get_property_string(&self.api, self.handle, &format!("track-list/{index}/lang"))
                    .unwrap_or_default(),
                selected: get_flag(&self.api, self.handle, &format!("track-list/{index}/selected"))
                    .unwrap_or(false),
                external: get_flag(&self.api, self.handle, &format!("track-list/{index}/external"))
                    .unwrap_or(false),
                codec: get_property_string(&self.api, self.handle, &format!("track-list/{index}/codec"))
                    .unwrap_or_default(),
                fps: get_f64(&self.api, self.handle, &format!("track-list/{index}/demux-fps"))
                    .unwrap_or(0.0),
                bitrate: get_f64(
                    &self.api,
                    self.handle,
                    &format!("track-list/{index}/demux-bitrate"),
                )
                .unwrap_or(0.0),
            })
            .collect()
    }

    /// Frame index currently displayed, as estimated by the demuxer.
    pub fn frame_number(&self) -> Option<i64> {
        get_property_string(&self.api, self.handle, "estimated-frame-number")
            .and_then(|value| value.parse().ok())
    }

    /// Decoded video pixel format reported by the video output.
    pub fn pixel_format(&self) -> Option<String> {
        get_property_string(&self.api, self.handle, "video-params/pixelformat")
    }

    /// Selects the track with `id`, or disables the track kind when `id` is negative.
    pub fn set_track(&self, kind: &str, id: i64) -> Result<(), EngineError> {
        let property = match kind {
            "audio" => "aid",
            "sub" => "sid",
            "video" => "vid",
            _ => return Err(EngineError::InvalidArgument(format!("unknown track kind: {kind}"))),
        };
        let value = if id < 0 { "no".to_string() } else { id.to_string() };
        mpv::set_property_string(&self.api, self.handle, property, &value)
    }

    /// Returns the playback speed multiplier.
    pub fn speed(&self) -> f64 {
        get_f64(&self.api, self.handle, "speed").unwrap_or(1.0)
    }

    /// Sets the playback speed multiplier.
    pub fn set_speed(&self, speed: f64) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, "speed", &speed.to_string())
    }

    /// Seeks by `delta` chapters, clamped by mpv at either end.
    pub fn seek_chapter(&self, delta: i64) -> Result<(), EngineError> {
        mpv::command_string(&self.api, self.handle, &format!("add chapter {delta}"))
    }

    /// Enables or disables looping over the playlist.
    pub fn set_repeat(&self, enabled: bool) -> Result<(), EngineError> {
        let value = if enabled { "inf" } else { "no" };
        mpv::set_property_string(&self.api, self.handle, "loop-playlist", value)
    }

    /// Enables or disables shuffle playback order.
    pub fn set_shuffle(&self, enabled: bool) -> Result<(), EngineError> {
        let value = if enabled { "yes" } else { "no" };
        mpv::set_property_string(&self.api, self.handle, "shuffle", value)
    }

    /// Overrides the display aspect ratio (`no` keeps the container's, `0` stretches).
    pub fn set_video_aspect(&self, value: &str) -> Result<(), EngineError> {
        mpv::set_property_string(&self.api, self.handle, "video-aspect-override", value)
    }

    /// Stops playback and unloads the current file.
    pub fn stop(&self) -> Result<(), EngineError> {
        mpv::command_string(&self.api, self.handle, "stop")
    }

    /// Returns the most recent state snapshot.
    pub fn state(&self) -> PlayerState {
        self.state.lock().clone()
    }

    /// Renders the current video frame into a BGRA buffer.
    ///
    /// Returns `Ok(None)` when no new frame is pending, in which case the
    /// caller should keep displaying the previous buffer.
    pub fn render_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<u8>>, EngineError> {
        self.ensure_buffer(width, height)?;
        let Some(target) = self.render.as_mut() else {
            return Err(EngineError::RenderUnavailable);
        };

        // SAFETY: the context belongs to this thread and is alive.
        let flags = unsafe { (self.api.render_context_update)(target.context) };
        if flags & MPV_RENDER_UPDATE_FRAME == 0 {
            return Ok(None);
        }

        let mut size = [width as c_int, height as c_int];
        let mut stride = target.stride();
        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast::<c_void>(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_FORMAT,
                data: SW_FORMAT_BGR0.as_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_STRIDE,
                data: (&mut stride as *mut usize).cast::<c_void>(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_SW_POINTER,
                data: target.buffer.as_mut_ptr().cast::<c_void>(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        // SAFETY: the parameter array is terminated and the target buffer has
        // room for stride * height bytes.
        let code = unsafe { (self.api.render_context_render)(target.context, params.as_mut_ptr()) };
        self.api.check(code)?;

        Ok(Some(target.buffer.clone()))
    }

    fn seek(&self, seconds: f64, mode: &str) -> Result<(), EngineError> {
        let command = format!("seek {seconds} {mode} exact");
        mpv::command_string(&self.api, self.handle, &command)
    }

    fn ensure_buffer(&mut self, width: u32, height: u32) -> Result<(), EngineError> {
        let Some(target) = self.render.as_mut() else {
            return Err(EngineError::RenderUnavailable);
        };
        if target.width == width && target.height == height && !target.buffer.is_empty() {
            return Ok(());
        }

        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(EngineError::InvalidSize { width, height })?;
        if len == 0 {
            return Err(EngineError::InvalidSize { width, height });
        }

        target.buffer = vec![0; len];
        target.width = width;
        target.height = height;
        Ok(())
    }

    fn free_render_target(&mut self) {
        if let Some(target) = self.render.take() {
            // SAFETY: the context was created by mpv_render_context_create and
            // is freed at most once.
            unsafe { (self.api.render_context_free)(target.context) };
        }
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // SAFETY: mpv_wakeup only interrupts the event thread's wait; it is
        // explicitly safe to call from another thread.
        unsafe { (self.api.wakeup)(self.handle) };
        if let Some(thread) = self.event_thread.take() {
            let _ = thread.join();
        }
        self.free_render_target();
        // SAFETY: the event thread is joined and the render context is freed,
        // so the handle is no longer used.
        unsafe { (self.api.destroy)(self.handle) };
    }
}

impl RenderTarget {
    fn stride(&self) -> usize {
        self.width as usize * 4
    }
}

fn create_render_context(
    api: &MpvApi,
    handle: *mut MpvHandle,
) -> Result<*mut MpvRenderContext, EngineError> {
    let mut api_type = *MPV_RENDER_API_TYPE_SW;
    let mut params = [
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_API_TYPE,
            data: api_type.as_mut_ptr().cast::<c_void>(),
        },
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        },
    ];
    let mut context: *mut MpvRenderContext = ptr::null_mut();
    // SAFETY: mpv_render_context_create writes a new context into `context`
    // and the parameter array is terminated by MPV_RENDER_PARAM_INVALID.
    let code = unsafe { (api.render_context_create)(&mut context, handle, params.as_mut_ptr()) };
    if code < 0 {
        return Err(EngineError::RenderContext {
            code,
            message: api.error_message(code),
        });
    }
    Ok(context)
}

struct SendHandle(*mut MpvHandle);

// SAFETY: the handle is only used by the single event thread through the
// thread-safe libmpv client API.
unsafe impl Send for SendHandle {}

fn event_loop(
    api: Arc<MpvApi>,
    handle: SendHandle,
    state: Arc<Mutex<PlayerState>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: this is the only thread calling mpv_wait_event for the handle.
        let event = unsafe { (api.wait_event)(handle.0, EVENT_TIMEOUT_SECS) };
        if !event.is_null() {
            // SAFETY: the event pointer is valid until the next wait_event call.
            let event = unsafe { &*event };
            if handle_event(&api, event, &state) {
                break;
            }
        }
        poll_properties(&api, handle.0, &mut state.lock());
    }
}

/// Returns true when the player is shutting down and the loop must stop.
fn handle_event(api: &MpvApi, event: &MpvEvent, state: &Mutex<PlayerState>) -> bool {
    match event.event_id {
        MPV_EVENT_SHUTDOWN => {
            let mut state = state.lock();
            state.loaded = false;
            state.error = Some("mpv core shut down".to_string());
            true
        }
        MPV_EVENT_END_FILE if !event.data.is_null() => {
            // SAFETY: END_FILE carries an mpv_event_end_file, valid for the
            // duration of this event iteration.
            let end = unsafe { &*(event.data as *const MpvEventEndFile) };
            let mut state = state.lock();
            if end.reason == MPV_END_FILE_REASON_ERROR {
                state.eof = true;
                state.error = Some(api.error_message(end.error));
            }
            false
        }
        MPV_EVENT_FILE_LOADED => {
            let mut state = state.lock();
            state.loaded = true;
            state.eof = false;
            state.error = None;
            false
        }
        MPV_EVENT_IDLE => {
            state.lock().loaded = false;
            false
        }
        _ => false,
    }
}

fn poll_properties(api: &MpvApi, handle: *mut MpvHandle, state: &mut PlayerState) {
    if let Some(idle) = get_flag(api, handle, "idle-active") {
        state.loaded = !idle;
    }
    if let Some(position) = get_f64(api, handle, "time-pos") {
        state.position = position;
    } else if !state.loaded {
        state.position = 0.0;
    }
    if let Some(duration) = get_f64(api, handle, "duration") {
        state.duration = duration;
    } else if !state.loaded {
        state.duration = 0.0;
    }
    if let Some(paused) = get_flag(api, handle, "pause") {
        state.paused = paused;
    }
    if let Some(eof) = get_flag(api, handle, "eof-reached") {
        state.eof = eof;
    }
    if let Some(volume) = get_f64(api, handle, "volume") {
        state.volume = volume;
    }
    if let Some(width) = get_u32(api, handle, "width") {
        state.width = width;
    }
    if let Some(height) = get_u32(api, handle, "height") {
        state.height = height;
    }
    if let Some(title) = get_property_string(api, handle, "media-title") {
        state.title = title;
    }
    if state.loaded {
        if let Some(gamma) = get_property_string(api, handle, "video-params/gamma") {
            state.video_gamma = gamma;
        }
        if let Some(peak) = get_f64(api, handle, "video-params/sig-peak") {
            state.sig_peak = peak;
        }
    } else {
        state.video_gamma.clear();
        state.sig_peak = 0.0;
    }
}

fn get_flag(api: &MpvApi, handle: *mut MpvHandle, name: &str) -> Option<bool> {
    match get_property_string(api, handle, name)?.as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn get_f64(api: &MpvApi, handle: *mut MpvHandle, name: &str) -> Option<f64> {
    get_property_string(api, handle, name)?.parse().ok()
}

fn get_u32(api: &MpvApi, handle: *mut MpvHandle, name: &str) -> Option<u32> {
    get_property_string(api, handle, name)?.parse().ok()
}

fn quote_argument(argument: &str) -> String {
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    for character in argument.chars() {
        if matches!(character, '\\' | '"') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}
