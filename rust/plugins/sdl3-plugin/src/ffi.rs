//! The SDL3 binding: types, structures and the function table.
//!
//! Bound at runtime for the same reason Cairo is (see the sibling crate):
//! `cmake/importSDL2.cmake` downloads SDL as a ready-made binary, so there are
//! no headers to compile against and no `.pc` file to link by. On Linux it
//! does not download SDL3 at all yet -- only Windows and macOS get
//! `SDL3-3.4.10` -- which makes tolerating a missing library not a nicety but
//! the common case on the platform this was written on.
//!
//! Signatures and structure layouts are transcribed from the SDL 3.4.10
//! headers. Unlike Cairo's, the structures here are read field by field out of
//! an `SDL_Event`, so a layout error would be a silent misread rather than a
//! crash. `tests/sdl3_live.rs` checks them against a real SDL3.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_float, c_int, c_void};
use std::sync::OnceLock;

use pharo_vm_plugin::dylib;
use pharo_vm_plugin::{PrimErr, PrimResult};

/// A window. Opaque: only SDL dereferences one.
#[repr(C)]
pub struct SDL_Window {
    _private: [u8; 0],
}

/// A 2D rendering context for a window.
#[repr(C)]
pub struct SDL_Renderer {
    _private: [u8; 0],
}

/// An image the renderer can draw.
#[repr(C)]
pub struct SDL_Texture {
    _private: [u8; 0],
}

/// An integer rectangle, as `SDL_UpdateTexture` takes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SDL_Rect {
    pub x: c_int,
    pub y: c_int,
    pub w: c_int,
    pub h: c_int,
}

/// A floating-point rectangle, as the render calls take.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SDL_FRect {
    pub x: c_float,
    pub y: c_float,
    pub w: c_float,
    pub h: c_float,
}

/// The fields every event begins with.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_CommonEvent {
    pub r#type: u32,
    pub reserved: u32,
    /// Nanoseconds, from `SDL_GetTicksNS`.
    pub timestamp: u64,
}

/// `SDL_EVENT_WINDOW_*`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_WindowEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    pub data1: i32,
    pub data2: i32,
}

/// `SDL_EVENT_KEY_DOWN` / `SDL_EVENT_KEY_UP`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_KeyboardEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    pub which: u32,
    pub scancode: u32,
    pub key: u32,
    /// `SDL_Keymod` is a `Uint16`, and `raw` follows it in the same word.
    pub r#mod: u16,
    pub raw: u16,
    pub down: bool,
    pub repeat: bool,
}

/// `SDL_EVENT_TEXT_INPUT`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_TextInputEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    /// UTF-8, owned by SDL and valid only until the next event is pumped.
    pub text: *const c_char,
}

/// `SDL_EVENT_MOUSE_MOTION`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_MouseMotionEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    pub which: u32,
    pub state: u32,
    pub x: c_float,
    pub y: c_float,
    pub xrel: c_float,
    pub yrel: c_float,
}

/// `SDL_EVENT_MOUSE_BUTTON_DOWN` / `_UP`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_MouseButtonEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    pub which: u32,
    pub button: u8,
    pub down: bool,
    pub clicks: u8,
    pub padding: u8,
    pub x: c_float,
    pub y: c_float,
}

/// `SDL_EVENT_MOUSE_WHEEL`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SDL_MouseWheelEvent {
    pub r#type: u32,
    pub reserved: u32,
    pub timestamp: u64,
    pub windowID: u32,
    pub which: u32,
    pub x: c_float,
    pub y: c_float,
    pub direction: u32,
    pub mouse_x: c_float,
    pub mouse_y: c_float,
    pub integer_x: i32,
    pub integer_y: i32,
}

/// How many bytes one `SDL_Event` occupies.
///
/// The union is explicitly padded to this in the header
/// (`Uint8 padding[128]`, with a compile-time assertion beside it) precisely
/// so that it does not vary between platforms -- which is what makes
/// `primitivePollEventRaw` safe to hand to the image as bytes.
pub const SDL_EVENT_SIZE: usize = 128;

/// How many bytes the plugin's own decoded event record occupies.
///
/// See the README for the field layout. Fixed here, and answered to the image
/// by `primitiveEventRecordSize` so nothing has to hard-code it twice.
pub const EVENT_RECORD_SIZE: usize = 64;

/// `SDL_EVENT_QUIT`.
pub const SDL_EVENT_QUIT: u32 = 0x100;
/// First `SDL_EVENT_WINDOW_*`, `SDL_EVENT_WINDOW_SHOWN`.
pub const SDL_EVENT_WINDOW_FIRST: u32 = 0x202;
/// Last `SDL_EVENT_WINDOW_*`.
pub const SDL_EVENT_WINDOW_LAST: u32 = 0x22F;
/// `SDL_EVENT_KEY_DOWN`.
pub const SDL_EVENT_KEY_DOWN: u32 = 0x300;
/// `SDL_EVENT_KEY_UP`.
pub const SDL_EVENT_KEY_UP: u32 = 0x301;
/// `SDL_EVENT_TEXT_INPUT`.
pub const SDL_EVENT_TEXT_INPUT: u32 = 0x303;
/// `SDL_EVENT_MOUSE_MOTION`.
pub const SDL_EVENT_MOUSE_MOTION: u32 = 0x400;
/// `SDL_EVENT_MOUSE_BUTTON_DOWN`.
pub const SDL_EVENT_MOUSE_BUTTON_DOWN: u32 = 0x401;
/// `SDL_EVENT_MOUSE_BUTTON_UP`.
pub const SDL_EVENT_MOUSE_BUTTON_UP: u32 = 0x402;
/// `SDL_EVENT_MOUSE_WHEEL`.
pub const SDL_EVENT_MOUSE_WHEEL: u32 = 0x403;

/// Declares the function table, one `Option` per symbol.
macro_rules! sdl_api {
    ($( fn $name:ident ( $($arg:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        /// SDL3's entry points, resolved once when the module loads.
        #[allow(non_snake_case)]
        pub struct Sdl {
            /// Where the library was found, for `primitiveLibraryPath`.
            pub path: String,
            $(
                #[allow(missing_docs)]
                pub $name: Option<unsafe extern "C" fn($($arg),*) $(-> $ret)?>,
            )*
        }

        impl Sdl {
            /// # Safety
            ///
            /// `lib` must be SDL3 and must outlive every pointer taken from it.
            unsafe fn resolve(lib: &'static dylib::Library, path: String) -> Self {
                Self {
                    path,
                    // SAFETY: each name is an SDL3 entry point whose signature
                    // is transcribed from the 3.4.10 headers just above.
                    $( $name: unsafe { dylib::symbol(lib, stringify!($name)) }, )*
                }
            }

            /// How many entry points this plugin knows about.
            #[must_use]
            pub fn declared_count(&self) -> usize {
                let mut n = 0;
                $( { let _ = &self.$name; n += 1; } )*
                n
            }

            /// Names this library did not export -- a misspelling here would
            /// otherwise be indistinguishable from an older SDL.
            #[must_use]
            pub fn missing_entry_points(&self) -> Vec<&'static str> {
                let mut missing = Vec::new();
                $( if self.$name.is_none() { missing.push(stringify!($name)); } )*
                missing
            }
        }
    };
}

sdl_api! {
    // ---- the library itself ----
    fn SDL_Init(u32) -> bool;
    fn SDL_Quit();
    fn SDL_WasInit(u32) -> u32;
    fn SDL_GetError() -> *const c_char;
    fn SDL_ClearError() -> bool;
    fn SDL_GetVersion() -> c_int;
    fn SDL_GetCurrentVideoDriver() -> *const c_char;
    fn SDL_free(*mut c_void);

    // ---- windows ----
    fn SDL_CreateWindow(*const c_char, c_int, c_int, u64) -> *mut SDL_Window;
    fn SDL_DestroyWindow(*mut SDL_Window);
    fn SDL_GetWindowID(*mut SDL_Window) -> u32;
    fn SDL_SetWindowTitle(*mut SDL_Window, *const c_char) -> bool;
    fn SDL_GetWindowTitle(*mut SDL_Window) -> *const c_char;
    fn SDL_GetWindowSize(*mut SDL_Window, *mut c_int, *mut c_int) -> bool;
    fn SDL_SetWindowSize(*mut SDL_Window, c_int, c_int) -> bool;
    fn SDL_GetWindowPosition(*mut SDL_Window, *mut c_int, *mut c_int) -> bool;
    fn SDL_SetWindowPosition(*mut SDL_Window, c_int, c_int) -> bool;
    fn SDL_ShowWindow(*mut SDL_Window) -> bool;
    fn SDL_HideWindow(*mut SDL_Window) -> bool;
    fn SDL_RaiseWindow(*mut SDL_Window) -> bool;
    fn SDL_SyncWindow(*mut SDL_Window) -> bool;
    fn SDL_SetWindowFullscreen(*mut SDL_Window, bool) -> bool;
    fn SDL_SetWindowResizable(*mut SDL_Window, bool) -> bool;

    // ---- renderers ----
    fn SDL_CreateRenderer(*mut SDL_Window, *const c_char) -> *mut SDL_Renderer;
    fn SDL_DestroyRenderer(*mut SDL_Renderer);
    fn SDL_SetRenderDrawColor(*mut SDL_Renderer, u8, u8, u8, u8) -> bool;
    fn SDL_RenderClear(*mut SDL_Renderer) -> bool;
    fn SDL_RenderPresent(*mut SDL_Renderer) -> bool;
    fn SDL_RenderTexture(
        *mut SDL_Renderer, *mut SDL_Texture, *const SDL_FRect, *const SDL_FRect
    ) -> bool;
    fn SDL_GetRenderOutputSize(*mut SDL_Renderer, *mut c_int, *mut c_int) -> bool;
    fn SDL_SetRenderVSync(*mut SDL_Renderer, c_int) -> bool;
    fn SDL_GetRenderVSync(*mut SDL_Renderer, *mut c_int) -> bool;

    // ---- textures ----
    fn SDL_CreateTexture(*mut SDL_Renderer, u32, c_int, c_int, c_int) -> *mut SDL_Texture;
    fn SDL_DestroyTexture(*mut SDL_Texture);
    fn SDL_UpdateTexture(*mut SDL_Texture, *const SDL_Rect, *const c_void, c_int) -> bool;
    fn SDL_SetTextureBlendMode(*mut SDL_Texture, u32) -> bool;
    fn SDL_SetTextureScaleMode(*mut SDL_Texture, c_int) -> bool;

    // ---- events ----
    fn SDL_PumpEvents();
    fn SDL_PollEvent(*mut c_void) -> bool;
    fn SDL_WaitEventTimeout(*mut c_void, i32) -> bool;

    // ---- input ----
    fn SDL_GetMouseState(*mut c_float, *mut c_float) -> u32;
    fn SDL_GetModState() -> u16;
    fn SDL_GetKeyName(u32) -> *const c_char;
    fn SDL_StartTextInput(*mut SDL_Window) -> bool;
    fn SDL_StopTextInput(*mut SDL_Window) -> bool;
    fn SDL_ShowCursor() -> bool;
    fn SDL_HideCursor() -> bool;

    // ---- clipboard ----
    fn SDL_GetClipboardText() -> *mut c_char;
    fn SDL_SetClipboardText(*const c_char) -> bool;
    fn SDL_HasClipboardText() -> bool;

    // ---- time ----
    fn SDL_GetTicks() -> u64;
    fn SDL_Delay(u32);
}

static SDL: OnceLock<Option<Sdl>> = OnceLock::new();

/// File names SDL3 might be under.
fn library_names() -> Vec<String> {
    dylib::library_names("SDL3", "0")
}

/// Loads SDL3, once. Answers whether it is now available.
pub fn load() -> bool {
    SDL.get_or_init(|| {
        let names: Vec<String> = library_names();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        // SAFETY: these names denote SDL3, and every signature declared above
        // is transcribed from the 3.4.10 headers.
        let (lib, path) = unsafe { dylib::open_first(&refs) }?;
        let lib = dylib::leak(lib);
        // SAFETY: `lib` is 'static, so the pointers stay valid for the process.
        let sdl = unsafe { Sdl::resolve(lib, path) };
        // A library exporting neither of these is not SDL3.
        if sdl.SDL_Init.is_none() || sdl.SDL_PollEvent.is_none() {
            return None;
        }
        Some(sdl)
    })
    .is_some()
}

/// The loaded library, or `Unsupported` if it never loaded.
pub fn sdl() -> PrimResult<&'static Sdl> {
    SDL.get().and_then(Option::as_ref).ok_or(PrimErr::Unsupported)
}

/// Calls an SDL entry point, failing cleanly when the library lacks it.
macro_rules! sc {
    ($sdl:expr, $f:ident ( $($arg:expr),* $(,)? )) => {{
        let f = $sdl.$f.ok_or(::pharo_vm_plugin::PrimErr::Unsupported)?;
        // SAFETY: the pointer came out of the SDL3 we loaded, and the
        // signature is the one declared in `sdl_api!` from the headers.
        unsafe { f($($arg),*) }
    }};
}
pub(crate) use sc;

/// Reads a NUL-terminated string SDL owns and does not expect back.
///
/// # Safety
///
/// `ptr` must be NUL-terminated and valid for the call.
pub unsafe fn borrowed_str(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: delegated to this function's contract.
    unsafe { core::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Turns an SDL `false` into a primitive failure, with SDL's message discarded.
///
/// SDL reports failure as `false` and leaves the detail in `SDL_GetError`,
/// which the image can read for itself with `primitiveGetError`. There is
/// nowhere to put a message in a primitive failure, only a code.
pub fn check(ok: bool) -> PrimResult<()> {
    if ok {
        Ok(())
    } else {
        Err(PrimErr::OperationFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layouts this crate reads events through.
    ///
    /// Every number here was taken from a C program compiled against the SDL
    /// 3.4.10 headers (`sizeof` and `offsetof` for each member), not inferred.
    /// A mismatch means a field was omitted, reordered or given the wrong
    /// width, and every field after it would be silently misread -- the one
    /// failure mode this crate's `dlopen` binding cannot get a compiler error
    /// for.
    #[test]
    fn event_structs_have_the_headers_layout() {
        use core::mem::{align_of, offset_of, size_of};

        assert_eq!(size_of::<SDL_CommonEvent>(), 16);
        assert_eq!(align_of::<SDL_CommonEvent>(), 8);

        assert_eq!(size_of::<SDL_WindowEvent>(), 32);
        assert_eq!(offset_of!(SDL_WindowEvent, data1), 20);

        assert_eq!(size_of::<SDL_KeyboardEvent>(), 40);
        assert_eq!(offset_of!(SDL_KeyboardEvent, windowID), 16);
        assert_eq!(offset_of!(SDL_KeyboardEvent, scancode), 24);
        assert_eq!(offset_of!(SDL_KeyboardEvent, key), 28);
        // `mod` is a Uint16 and `raw` shares its word; getting this wrong
        // would shift `down` and `repeat` and invert every key event.
        assert_eq!(offset_of!(SDL_KeyboardEvent, r#mod), 32);
        assert_eq!(offset_of!(SDL_KeyboardEvent, raw), 34);
        assert_eq!(offset_of!(SDL_KeyboardEvent, down), 36);
        assert_eq!(offset_of!(SDL_KeyboardEvent, repeat), 37);

        assert_eq!(size_of::<SDL_TextInputEvent>(), 32);
        assert_eq!(offset_of!(SDL_TextInputEvent, text), 24);

        assert_eq!(size_of::<SDL_MouseMotionEvent>(), 48);
        assert_eq!(offset_of!(SDL_MouseMotionEvent, state), 24);
        assert_eq!(offset_of!(SDL_MouseMotionEvent, x), 28);
        assert_eq!(offset_of!(SDL_MouseMotionEvent, yrel), 40);

        assert_eq!(size_of::<SDL_MouseButtonEvent>(), 40);
        assert_eq!(offset_of!(SDL_MouseButtonEvent, button), 24);
        assert_eq!(offset_of!(SDL_MouseButtonEvent, clicks), 26);
        assert_eq!(offset_of!(SDL_MouseButtonEvent, x), 28);

        assert_eq!(size_of::<SDL_MouseWheelEvent>(), 56);
        assert_eq!(offset_of!(SDL_MouseWheelEvent, x), 24);
        assert_eq!(offset_of!(SDL_MouseWheelEvent, direction), 32);
        assert_eq!(offset_of!(SDL_MouseWheelEvent, mouse_x), 36);
        assert_eq!(offset_of!(SDL_MouseWheelEvent, integer_x), 44);
        assert_eq!(offset_of!(SDL_MouseWheelEvent, integer_y), 48);

        // Every member fits inside the union's fixed 128 bytes.
        assert!(size_of::<SDL_MouseWheelEvent>() <= SDL_EVENT_SIZE);
    }

    #[test]
    fn rects_are_four_packed_values() {
        assert_eq!(core::mem::size_of::<SDL_Rect>(), 16);
        assert_eq!(core::mem::size_of::<SDL_FRect>(), 16);
    }

    #[test]
    fn a_failed_sdl_call_becomes_a_primitive_failure() {
        assert!(check(true).is_ok());
        assert_eq!(check(false), Err(PrimErr::OperationFailed));
    }

    #[test]
    fn library_names_are_platform_shaped() {
        let names = library_names();
        assert!(!names.is_empty());
        if cfg!(target_os = "linux") {
            assert_eq!(names[0], "libSDL3.so.0");
        }
    }

    #[test]
    fn sdl_is_unsupported_until_it_loads() {
        match sdl() {
            Ok(s) => assert!(s.declared_count() > 40),
            Err(e) => assert_eq!(e, PrimErr::Unsupported),
        }
    }
}
