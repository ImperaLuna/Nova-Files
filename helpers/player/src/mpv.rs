#![expect(unsafe_code, reason = "the binding to libmpv, loaded at run time")]

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;
use std::{mem, ptr};

use libloading::Library;

const LIBRARY_NAMES: [&str; 2] = ["libmpv.so.2", "libmpv.so"];
const SOFTWARE: &CStr = c"sw";
const PIXEL_FORMAT: &CStr = c"rgb0";

// https://github.com/mpv-player/mpv/blob/v0.37.0/libmpv/render.h
const PARAM_INVALID: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_SW_SIZE: c_int = 17;
const PARAM_SW_FORMAT: c_int = 18;
const PARAM_SW_STRIDE: c_int = 19;
const PARAM_SW_POINTER: c_int = 20;
const UPDATE_FRAME: u64 = 1;
const BYTES_PER_PIXEL: usize = 4;

// https://github.com/mpv-player/mpv/blob/v0.37.0/libmpv/client.h
pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_DOUBLE: c_int = 5;
const EVENT_NONE: c_int = 0;
const EVENT_SHUTDOWN: c_int = 1;
const EVENT_END_FILE: c_int = 7;
const EVENT_VIDEO_RECONFIG: c_int = 17;
const EVENT_PROPERTY_CHANGE: c_int = 22;

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct RawProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

type Create = unsafe extern "C" fn() -> *mut c_void;
type Initialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type SetOption = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type CommandString = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type Command = unsafe extern "C" fn(*mut c_void, *mut *const c_char) -> c_int;
type GetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
type Observe = unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int) -> c_int;
type WaitEvent = unsafe extern "C" fn(*mut c_void, c_double) -> *mut RawEvent;
type Destroy = unsafe extern "C" fn(*mut c_void);
type RenderCreate = unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int;
type UpdateCallback = unsafe extern "C" fn(*mut c_void);
type SetUpdateCallback = unsafe extern "C" fn(*mut c_void, Option<UpdateCallback>, *mut c_void);
type RenderUpdate = unsafe extern "C" fn(*mut c_void) -> u64;
type Render = unsafe extern "C" fn(*mut c_void, *mut RenderParam) -> c_int;
type RenderFree = unsafe extern "C" fn(*mut c_void);

struct Functions {
    create: Create,
    initialize: Initialize,
    set_option: SetOption,
    command_string: CommandString,
    command: Command,
    get_property: GetProperty,
    observe: Observe,
    wait_event: WaitEvent,
    destroy: Destroy,
    render_create: RenderCreate,
    set_update_callback: SetUpdateCallback,
    render_update: RenderUpdate,
    render: Render,
    render_free: RenderFree,
}

pub struct Mpv {
    functions: Functions,
    handle: *mut c_void,
    render: *mut c_void,
    woken: Arc<Woken>,
    _library: Library,
}

#[derive(Default)]
pub struct Woken {
    frame: Mutex<bool>,
    ready: Condvar,
}

pub enum Event {
    Property { name: String, value: Value },
    Resized,
    Ended,
    Shutdown,
}

pub enum Value {
    Number(f64),
    Flag(bool),
    Text(String),
    Gone,
}

#[derive(Clone, Copy)]
pub struct Commander {
    command_string: CommandString,
    handle: *mut c_void,
}

impl Mpv {
    pub fn load() -> Option<Self> {
        let library = LIBRARY_NAMES.iter().find_map(|name| {
            // SAFETY: libmpv's initialisers only set up its own state.
            let library = unsafe { Library::new(name) };
            library.ok()
        })?;
        let functions = Functions::from(&library)?;
        // SAFETY: mpv_create takes nothing and returns a new handle or null.
        let handle = unsafe { (functions.create)() };
        if handle.is_null() {
            return None;
        }
        Some(Self {
            functions,
            handle,
            render: ptr::null_mut(),
            woken: Arc::new(Woken::default()),
            _library: library,
        })
    }

    pub fn set_option(&self, name: &str, value: &str) -> bool {
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return false;
        };
        // SAFETY: the handle is live and both strings are NUL-terminated
        // copies that outlive the call.
        let status =
            unsafe { (self.functions.set_option)(self.handle, name.as_ptr(), value.as_ptr()) };
        status >= 0
    }

    pub fn start(&mut self) -> bool {
        // SAFETY: the handle is live and not yet initialised.
        let initialized = unsafe { (self.functions.initialize)(self.handle) };
        if initialized < 0 {
            return false;
        }
        let mut params = [
            RenderParam {
                kind: PARAM_API_TYPE,
                data: SOFTWARE.as_ptr().cast_mut().cast::<c_void>(),
            },
            RenderParam {
                kind: PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: the parameter list ends with PARAM_INVALID and the API
        // name is a static C string, as mpv_render_context_create asks.
        let created = unsafe {
            (self.functions.render_create)(&raw mut self.render, self.handle, params.as_mut_ptr())
        };
        if created < 0 {
            return false;
        }
        let woken = Arc::as_ptr(&self.woken).cast_mut().cast::<c_void>();
        // SAFETY: `woken` lives in an Arc this Mpv holds until after the
        // render context is freed in drop, so the callback never sees a
        // dangling pointer.
        unsafe {
            (self.functions.set_update_callback)(self.render, Some(frame_ready), woken);
        }
        true
    }

    pub fn commander(&self) -> Commander {
        Commander {
            command_string: self.functions.command_string,
            handle: self.handle,
        }
    }

    pub fn load_file(&self, file: &CStr) -> bool {
        let mut args = [c"loadfile".as_ptr(), file.as_ptr(), ptr::null()];
        // SAFETY: the argument list is NULL-terminated and its strings
        // outlive the call.
        let status = unsafe { (self.functions.command)(self.handle, args.as_mut_ptr()) };
        status >= 0
    }

    pub fn observe(&self, name: &CStr, format: c_int) {
        // SAFETY: the handle is live and the name is a C string.
        unsafe {
            (self.functions.observe)(self.handle, 0, name.as_ptr(), format);
        }
    }

    pub fn number(&self, name: &CStr) -> Option<f64> {
        let mut value: c_double = 0.0;
        // SAFETY: MPV_FORMAT_DOUBLE writes one double into `value`.
        let status = unsafe {
            (self.functions.get_property)(
                self.handle,
                name.as_ptr(),
                FORMAT_DOUBLE,
                (&raw mut value).cast(),
            )
        };
        (status >= 0).then_some(value)
    }

    pub fn wait_for_frame(&self, longest: Duration) -> bool {
        let woken = &self.woken;
        mem::take(
            &mut *woken
                .ready
                .wait_timeout_while(
                    woken.frame.lock().unwrap_or_else(PoisonError::into_inner),
                    longest,
                    |frame| !*frame,
                )
                .unwrap_or_else(PoisonError::into_inner)
                .0,
        )
    }

    pub fn next_event(&self) -> Option<Event> {
        loop {
            // SAFETY: mpv_wait_event returns an event that stays valid until
            // the next call on this handle, which is only made after it is
            // read here.
            let event = unsafe { &*(self.functions.wait_event)(self.handle, 0.0) };
            match event.event_id {
                EVENT_NONE => return None,
                EVENT_SHUTDOWN => return Some(Event::Shutdown),
                EVENT_END_FILE => return Some(Event::Ended),
                EVENT_VIDEO_RECONFIG => return Some(Event::Resized),
                EVENT_PROPERTY_CHANGE if !event.data.is_null() => {
                    // SAFETY: a property-change event's data is an
                    // mpv_event_property.
                    let property = unsafe { &*event.data.cast::<RawProperty>() };
                    if let Some(event) = property_event(property) {
                        return Some(event);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn has_new_frame(&self) -> bool {
        // SAFETY: the render context is live; this is the render thread.
        let flags = unsafe { (self.functions.render_update)(self.render) };
        flags & UPDATE_FRAME != 0
    }

    pub fn draw(&self, width: c_int, height: c_int, pixels: &mut [u8]) -> bool {
        let stride = usize::try_from(width).unwrap_or(0) * BYTES_PER_PIXEL;
        if pixels.len() < stride * usize::try_from(height).unwrap_or(0) || stride == 0 {
            return false;
        }
        let mut size = [width, height];
        let mut stride = stride;
        let mut params = [
            RenderParam {
                kind: PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast(),
            },
            RenderParam {
                kind: PARAM_SW_FORMAT,
                data: PIXEL_FORMAT.as_ptr().cast_mut().cast(),
            },
            RenderParam {
                kind: PARAM_SW_STRIDE,
                data: (&raw mut stride).cast(),
            },
            RenderParam {
                kind: PARAM_SW_POINTER,
                data: pixels.as_mut_ptr().cast(),
            },
            RenderParam {
                kind: PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: `pixels` holds at least stride * height bytes, and every
        // parameter points at a live value of the type render.h names.
        let status = unsafe { (self.functions.render)(self.render, params.as_mut_ptr()) };
        status >= 0
    }
}

impl Commander {
    pub fn run(self, command: &str) -> bool {
        let Ok(command) = CString::new(command) else {
            return false;
        };
        // SAFETY: libmpv's client calls are thread-safe and the handle
        // outlives the threads that hold a Commander.
        let status = unsafe { (self.command_string)(self.handle, command.as_ptr()) };
        status >= 0
    }
}

// SAFETY: client.h: "all functions are thread-safe" except where noted;
// mpv_command_string is not among the exceptions.
unsafe impl Send for Commander {}

impl Drop for Mpv {
    fn drop(&mut self) {
        if !self.render.is_null() {
            // SAFETY: the render context is live and freed once, before the
            // handle it belongs to.
            unsafe { (self.functions.render_free)(self.render) };
        }
        // SAFETY: the handle is live and destroyed once.
        unsafe { (self.functions.destroy)(self.handle) };
    }
}

unsafe extern "C" fn frame_ready(woken: *mut c_void) {
    // SAFETY: `woken` is the Arc<Woken> pointer Mpv::start passed, alive
    // until the render context is freed.
    let woken = unsafe { &*woken.cast::<Woken>() };
    *woken.frame.lock().unwrap_or_else(PoisonError::into_inner) = true;
    woken.ready.notify_one();
}

fn property_event(property: &RawProperty) -> Option<Event> {
    if property.name.is_null() {
        return None;
    }
    // SAFETY: a property's name is a C string while the event is valid.
    let name = unsafe { CStr::from_ptr(property.name) };
    let name = name.to_string_lossy().into_owned();
    let value = if property.data.is_null() {
        Value::Gone
    } else {
        match property.format {
            FORMAT_DOUBLE => {
                // SAFETY: MPV_FORMAT_DOUBLE data points at a double.
                let number = unsafe { *property.data.cast::<c_double>() };
                Value::Number(number)
            }
            FORMAT_FLAG => {
                // SAFETY: MPV_FORMAT_FLAG data points at an int.
                let flag = unsafe { *property.data.cast::<c_int>() };
                Value::Flag(flag != 0)
            }
            FORMAT_STRING => {
                // SAFETY: MPV_FORMAT_STRING data points at a char*.
                let text = unsafe { *property.data.cast::<*const c_char>() };
                if text.is_null() {
                    Value::Gone
                } else {
                    // SAFETY: the char* is a C string while the event is
                    // valid.
                    let text = unsafe { CStr::from_ptr(text) };
                    Value::Text(text.to_string_lossy().into_owned())
                }
            }
            _ => Value::Gone,
        }
    };
    Some(Event::Property { name, value })
}

impl Functions {
    fn from(library: &Library) -> Option<Self> {
        macro_rules! symbol {
            ($name:literal) => {{
                // SAFETY: each type matches the function's declaration in
                // client.h or render.h of libmpv 2.
                let symbol = unsafe { library.get($name) };
                *symbol.ok()?
            }};
        }
        Some(Self {
            create: symbol!(b"mpv_create\0"),
            initialize: symbol!(b"mpv_initialize\0"),
            set_option: symbol!(b"mpv_set_option_string\0"),
            command_string: symbol!(b"mpv_command_string\0"),
            command: symbol!(b"mpv_command\0"),
            get_property: symbol!(b"mpv_get_property\0"),
            observe: symbol!(b"mpv_observe_property\0"),
            wait_event: symbol!(b"mpv_wait_event\0"),
            destroy: symbol!(b"mpv_terminate_destroy\0"),
            render_create: symbol!(b"mpv_render_context_create\0"),
            set_update_callback: symbol!(b"mpv_render_context_set_update_callback\0"),
            render_update: symbol!(b"mpv_render_context_update\0"),
            render: symbol!(b"mpv_render_context_render\0"),
            render_free: symbol!(b"mpv_render_context_free\0"),
        })
    }
}
