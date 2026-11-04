//! Playback engine for FlintPlayer built on the system libmpv runtime.
//!
//! The library is loaded with `dlopen` at runtime, so no mpv headers or
//! development package are required. Rendering uses the libmpv software
//! renderer, which lets a single-threaded UI (for example GPUI) poll for
//! finished BGRA frames.

mod mpv;
mod player;

pub use player::{ChapterInfo, MpvPlayer, PlayerState, TrackInfo};

/// Errors returned by the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// No usable libmpv shared object was found.
    #[error("could not load libmpv: {0}")]
    LibraryLoad(String),
    /// The loaded libmpv lacks a symbol this engine needs.
    #[error("libmpv is missing symbol {name}: {message}")]
    MissingSymbol { name: String, message: String },
    /// An mpv client API call returned a negative status code.
    #[error("libmpv error {code}: {message}")]
    Mpv { code: i32, message: String },
    /// `mpv_create` returned a null handle.
    #[error("mpv_create returned a null handle")]
    CreateFailed,
    /// The software render context could not be created.
    #[error("render context creation failed ({code}): {message}")]
    RenderContext { code: i32, message: String },
    /// `render_frame` was called before a render context existed.
    #[error("render context is not available")]
    RenderUnavailable,
    /// The requested frame size cannot be represented.
    #[error("invalid frame size {width}x{height}")]
    InvalidSize { width: u32, height: u32 },
    /// A string passed to mpv contained an interior NUL byte.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The event thread could not be spawned.
    #[error("failed to spawn mpv event thread: {0}")]
    ThreadSpawn(String),
}

pub(crate) fn invalid_argument(what: &str) -> EngineError {
    EngineError::InvalidArgument(what.to_string())
}
