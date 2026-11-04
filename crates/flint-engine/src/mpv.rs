use std::ffi::{c_char, c_int, c_ulong, c_void, CStr, CString};
use std::sync::Arc;

use libloading::Library;

use crate::{invalid_argument, EngineError};

pub(crate) type MpvHandle = c_void;
pub(crate) type MpvRenderContext = c_void;

/// Mirrors `mpv_event` from client.h.
#[repr(C)]
pub(crate) struct MpvEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

/// Mirrors `mpv_event_end_file` from client.h.
#[repr(C)]
pub(crate) struct MpvEventEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

/// Mirrors `mpv_render_param` from render.h.
#[repr(C)]
pub(crate) struct MpvRenderParam {
    pub type_: c_int,
    pub data: *mut c_void,
}

pub(crate) const MPV_EVENT_SHUTDOWN: c_int = 1;
pub(crate) const MPV_EVENT_END_FILE: c_int = 7;
pub(crate) const MPV_EVENT_FILE_LOADED: c_int = 8;
pub(crate) const MPV_EVENT_IDLE: c_int = 11;

pub(crate) const MPV_END_FILE_REASON_ERROR: c_int = 4;

pub(crate) const MPV_RENDER_PARAM_INVALID: c_int = 0;
pub(crate) const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
pub(crate) const MPV_RENDER_PARAM_SW_SIZE: c_int = 17;
pub(crate) const MPV_RENDER_PARAM_SW_FORMAT: c_int = 18;
pub(crate) const MPV_RENDER_PARAM_SW_STRIDE: c_int = 19;
pub(crate) const MPV_RENDER_PARAM_SW_POINTER: c_int = 20;

pub(crate) const MPV_RENDER_UPDATE_FRAME: u64 = 1 << 0;

pub(crate) const MPV_RENDER_API_TYPE_SW: &[u8; 3] = b"sw\0";

type FnCreate = unsafe extern "C" fn() -> *mut MpvHandle;
type FnInitialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type FnSetOptionString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type FnSetPropertyString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type FnCommandString = unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> c_int;
type FnGetPropertyString = unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> *mut c_char;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnWaitEvent = unsafe extern "C" fn(*mut MpvHandle, f64) -> *mut MpvEvent;
type FnWakeup = unsafe extern "C" fn(*mut MpvHandle);
type FnDestroy = unsafe extern "C" fn(*mut MpvHandle);
type FnClientApiVersion = unsafe extern "C" fn() -> c_ulong;
type FnErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type FnRenderContextCreate =
    unsafe extern "C" fn(*mut *mut MpvRenderContext, *mut MpvHandle, *mut MpvRenderParam) -> c_int;
type FnRenderContextUpdate = unsafe extern "C" fn(*mut MpvRenderContext) -> u64;
type FnRenderContextRender =
    unsafe extern "C" fn(*mut MpvRenderContext, *mut MpvRenderParam) -> c_int;
type FnRenderContextFree = unsafe extern "C" fn(*mut MpvRenderContext);

/// Resolved function table for the libmpv shared object.
///
/// Kept in an `Arc` so the event thread can hold the function pointers while
/// the `Library` stays loaded for the player's lifetime.
pub(crate) struct MpvApi {
    _library: Library,
    pub(crate) create: FnCreate,
    pub(crate) initialize: FnInitialize,
    pub(crate) set_option_string: FnSetOptionString,
    pub(crate) set_property_string: FnSetPropertyString,
    pub(crate) command_string: FnCommandString,
    pub(crate) get_property_string: FnGetPropertyString,
    pub(crate) free: FnFree,
    pub(crate) wait_event: FnWaitEvent,
    pub(crate) wakeup: FnWakeup,
    pub(crate) destroy: FnDestroy,
    pub(crate) client_api_version: FnClientApiVersion,
    pub(crate) error_string: FnErrorString,
    pub(crate) render_context_create: FnRenderContextCreate,
    pub(crate) render_context_update: FnRenderContextUpdate,
    pub(crate) render_context_render: FnRenderContextRender,
    pub(crate) render_context_free: FnRenderContextFree,
}

impl MpvApi {
    pub(crate) fn load() -> Result<Arc<Self>, EngineError> {
        let library = load_library()?;

        // SAFETY: the symbol names and signatures below are taken from the
        // libmpv 0.37 client.h / render.h ABI.
        unsafe {
            Ok(Arc::new(Self {
                create: symbol(&library, b"mpv_create")?,
                initialize: symbol(&library, b"mpv_initialize")?,
                set_option_string: symbol(&library, b"mpv_set_option_string")?,
                set_property_string: symbol(&library, b"mpv_set_property_string")?,
                command_string: symbol(&library, b"mpv_command_string")?,
                get_property_string: symbol(&library, b"mpv_get_property_string")?,
                free: symbol(&library, b"mpv_free")?,
                wait_event: symbol(&library, b"mpv_wait_event")?,
                wakeup: symbol(&library, b"mpv_wakeup")?,
                destroy: symbol(&library, b"mpv_destroy")?,
                client_api_version: symbol(&library, b"mpv_client_api_version")?,
                error_string: symbol(&library, b"mpv_error_string")?,
                render_context_create: symbol(&library, b"mpv_render_context_create")?,
                render_context_update: symbol(&library, b"mpv_render_context_update")?,
                render_context_render: symbol(&library, b"mpv_render_context_render")?,
                render_context_free: symbol(&library, b"mpv_render_context_free")?,
                _library: library,
            }))
        }
    }

    pub(crate) fn error_message(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static, NULL-terminated string.
        unsafe {
            let text = (self.error_string)(code);
            if text.is_null() {
                format!("error {code}")
            } else {
                CStr::from_ptr(text).to_string_lossy().into_owned()
            }
        }
    }

    pub(crate) fn error(&self, code: c_int) -> EngineError {
        EngineError::Mpv {
            code,
            message: self.error_message(code),
        }
    }

    pub(crate) fn check(&self, code: c_int) -> Result<(), EngineError> {
        if code < 0 {
            Err(self.error(code))
        } else {
            Ok(())
        }
    }
}

fn load_library() -> Result<Library, EngineError> {
    let mut failures = Vec::new();
    for name in ["libmpv.so.2", "libmpv.so"] {
        // SAFETY: loading a shared library can run its initializers; that is
        // the intended behaviour of the dlopen-based binding.
        match unsafe { Library::new(name) } {
            Ok(library) => return Ok(library),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    Err(EngineError::LibraryLoad(failures.join("; ")))
}

/// # Safety
/// `T` must be the exact C signature of `name`.
unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, EngineError> {
    let symbol = library
        .get::<T>(name)
        .map_err(|error| EngineError::MissingSymbol {
            name: String::from_utf8_lossy(name).into_owned(),
            message: error.to_string(),
        })?;
    Ok(*symbol)
}

pub(crate) fn set_option_string(
    api: &MpvApi,
    handle: *mut MpvHandle,
    name: &str,
    value: &str,
) -> Result<(), EngineError> {
    let name = CString::new(name).map_err(|_| invalid_argument("option name contains NUL"))?;
    let value = CString::new(value).map_err(|_| invalid_argument("option value contains NUL"))?;
    // SAFETY: both C strings outlive the call and the handle is valid.
    let code = unsafe { (api.set_option_string)(handle, name.as_ptr(), value.as_ptr()) };
    api.check(code)
}

pub(crate) fn set_property_string(
    api: &MpvApi,
    handle: *mut MpvHandle,
    name: &str,
    value: &str,
) -> Result<(), EngineError> {
    let name = CString::new(name).map_err(|_| invalid_argument("property name contains NUL"))?;
    let value = CString::new(value).map_err(|_| invalid_argument("property value contains NUL"))?;
    // SAFETY: both C strings outlive the call and the handle is valid.
    let code = unsafe { (api.set_property_string)(handle, name.as_ptr(), value.as_ptr()) };
    api.check(code)
}

pub(crate) fn command_string(
    api: &MpvApi,
    handle: *mut MpvHandle,
    command: &str,
) -> Result<(), EngineError> {
    let command = CString::new(command).map_err(|_| invalid_argument("command contains NUL"))?;
    // SAFETY: the C string outlives the call and the handle is valid.
    let code = unsafe { (api.command_string)(handle, command.as_ptr()) };
    api.check(code)
}

pub(crate) fn get_property_string(
    api: &MpvApi,
    handle: *mut MpvHandle,
    name: &str,
) -> Option<String> {
    let name = CString::new(name).ok()?;
    // SAFETY: mpv_get_property_string returns either NULL or a heap string
    // that must be released with mpv_free.
    unsafe {
        let text = (api.get_property_string)(handle, name.as_ptr());
        if text.is_null() {
            return None;
        }
        let value = CStr::from_ptr(text).to_string_lossy().into_owned();
        (api.free)(text.cast());
        Some(value)
    }
}
