//! Exercises the binding against a real SDL3, when there is one.
//!
//! These skip when SDL3 cannot be loaded, which on Linux is the normal case:
//! the VM bundle does not download SDL3 there. To run them, put an
//! `libSDL3.so.0` on the loader path -- `LD_LIBRARY_PATH=<dir> cargo test`.
//!
//! Video is not required. `SDL_VIDEODRIVER=dummy` gives a real window and
//! renderer with no display, which is enough to exercise every signature in
//! the table; the tests set it themselves so they behave the same headless.

use core::ffi::c_int;
use std::sync::{Mutex, MutexGuard, PoisonError};

use SDL3Plugin::ffi::{self, sdl};

/// `SDL_INIT_VIDEO`, which implies `SDL_INIT_EVENTS`.
const SDL_INIT_VIDEO: u32 = 0x20;
/// `SDL_WINDOW_HIDDEN`: no point mapping a window in a test.
const SDL_WINDOW_HIDDEN: u64 = 0x08;
/// `SDL_PIXELFORMAT_ARGB8888`, the shape a 32-bit Pharo Form is in.
const SDL_PIXELFORMAT_ARGB8888: u32 = 0x1636_2004;
/// `SDL_TEXTUREACCESS_STREAMING`.
const SDL_TEXTUREACCESS_STREAMING: c_int = 1;

macro_rules! skip_without_sdl {
    () => {
        if !ffi::load() {
            eprintln!("skipping: no SDL3 could be loaded");
            return;
        }
    };
}

/// Serialises the tests that touch SDL's video and event state.
///
/// `cargo test` runs test functions on several threads, and SDL requires
/// video and event calls on one -- the same one that initialised video. Two
/// tests creating windows at once would also see each other's events on the
/// shared queue.
static SDL_LOCK: Mutex<()> = Mutex::new(());

fn sdl_guard() -> MutexGuard<'static, ()> {
    SDL_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Brings SDL up with the dummy video driver, once per test process.
fn init_headless() -> bool {
    if !ffi::load() {
        return false;
    }
    // Safe to set: these tests are the only thing in this process.
    unsafe { std::env::set_var("SDL_VIDEODRIVER", "dummy") };
    let s = sdl().unwrap();
    // SAFETY: SDL_Init is safe to call repeatedly; it reference-counts.
    unsafe { (s.SDL_Init.unwrap())(SDL_INIT_VIDEO) }
}

#[test]
fn the_version_is_the_one_the_bundle_names() {
    skip_without_sdl!();
    let s = sdl().unwrap();
    // SAFETY: no arguments, and the entry point was resolved from SDL3.
    let v = unsafe { (s.SDL_GetVersion.unwrap())() };
    // SDL packs it as major * 1000000 + minor * 1000 + micro.
    assert_eq!(v / 1_000_000, 3, "this binding is SDL3-only");
}

#[test]
fn every_entry_point_this_plugin_declares_really_exists() {
    skip_without_sdl!();
    // The check that catches a misspelled symbol name, which would otherwise
    // be indistinguishable from an older SDL lacking the entry point.
    let missing = sdl().unwrap().missing_entry_points();
    assert!(missing.is_empty(), "not exported by this SDL3: {missing:?}");
}

#[test]
fn a_window_reports_the_geometry_it_was_asked_for() {
    let _guard = sdl_guard();
    if !init_headless() {
        eprintln!("skipping: SDL3 would not initialise");
        return;
    }
    let s = sdl().unwrap();
    unsafe {
        let w = (s.SDL_CreateWindow.unwrap())(c"probe".as_ptr(), 320, 240, SDL_WINDOW_HIDDEN);
        assert!(!w.is_null(), "{}", ffi::borrowed_str((s.SDL_GetError.unwrap())()));

        let (mut width, mut height) = (0, 0);
        assert!((s.SDL_GetWindowSize.unwrap())(w, &mut width, &mut height));
        assert_eq!((width, height), (320, 240));

        assert!((s.SDL_SetWindowTitle.unwrap())(w, c"renamed".as_ptr()));
        let title = ffi::borrowed_str((s.SDL_GetWindowTitle.unwrap())(w));
        assert_eq!(title, "renamed");

        assert!((s.SDL_GetWindowID.unwrap())(w) != 0);
        (s.SDL_DestroyWindow.unwrap())(w);
    }
}

#[test]
fn a_renderer_and_a_texture_take_pixels_from_a_buffer() {
    let _guard = sdl_guard();
    if !init_headless() {
        eprintln!("skipping: SDL3 would not initialise");
        return;
    }
    // The whole display path a Pharo Form would take, minus the display.
    let s = sdl().unwrap();
    unsafe {
        let w = (s.SDL_CreateWindow.unwrap())(c"probe".as_ptr(), 64, 64, SDL_WINDOW_HIDDEN);
        assert!(!w.is_null());
        let r = (s.SDL_CreateRenderer.unwrap())(w, core::ptr::null());
        assert!(!r.is_null(), "{}", ffi::borrowed_str((s.SDL_GetError.unwrap())()));

        let t = (s.SDL_CreateTexture.unwrap())(
            r,
            SDL_PIXELFORMAT_ARGB8888,
            SDL_TEXTUREACCESS_STREAMING,
            4,
            4,
        );
        assert!(!t.is_null(), "{}", ffi::borrowed_str((s.SDL_GetError.unwrap())()));

        let pixels = [0xFF00_FF00u32; 16];
        let rect = ffi::SDL_Rect { x: 0, y: 0, w: 4, h: 4 };
        assert!((s.SDL_UpdateTexture.unwrap())(t, &rect, pixels.as_ptr().cast(), 4 * 4));

        assert!((s.SDL_SetRenderDrawColor.unwrap())(r, 0, 0, 0, 255));
        assert!((s.SDL_RenderClear.unwrap())(r));
        assert!((s.SDL_RenderTexture.unwrap())(
            r,
            t,
            core::ptr::null(),
            core::ptr::null()
        ));
        assert!((s.SDL_RenderPresent.unwrap())(r));

        let (mut ow, mut oh) = (0, 0);
        assert!((s.SDL_GetRenderOutputSize.unwrap())(r, &mut ow, &mut oh));
        assert_eq!((ow, oh), (64, 64));

        (s.SDL_DestroyTexture.unwrap())(t);
        (s.SDL_DestroyRenderer.unwrap())(r);
        (s.SDL_DestroyWindow.unwrap())(w);
    }
}

#[test]
fn polling_a_drained_queue_answers_false() {
    let _guard = sdl_guard();
    if !init_headless() {
        eprintln!("skipping: SDL3 would not initialise");
        return;
    }
    let s = sdl().unwrap();
    unsafe {
        // Drain whatever the other tests' windows queued, then confirm the
        // queue reports itself empty. Note SDL does not promise to leave the
        // buffer untouched on a false answer, which is why the plugin copies
        // into the image object only after a true one.
        let mut buffer = [0u64; ffi::SDL_EVENT_SIZE / 8];
        while (s.SDL_PollEvent.unwrap())(buffer.as_mut_ptr().cast()) {}
        assert!(!(s.SDL_PollEvent.unwrap())(buffer.as_mut_ptr().cast()));
    }
}

#[test]
fn the_error_string_round_trips() {
    skip_without_sdl!();
    let s = sdl().unwrap();
    unsafe {
        assert!((s.SDL_ClearError.unwrap())());
        assert_eq!(ffi::borrowed_str((s.SDL_GetError.unwrap())()), "");
        // A call that must fail: a null window has no size.
        let (mut w, mut h) = (0, 0);
        assert!(!(s.SDL_GetWindowSize.unwrap())(core::ptr::null_mut(), &mut w, &mut h));
        assert!(
            !ffi::borrowed_str((s.SDL_GetError.unwrap())()).is_empty(),
            "SDL should have left a message behind"
        );
    }
}

#[test]
fn the_clock_moves_forward() {
    skip_without_sdl!();
    let s = sdl().unwrap();
    unsafe {
        let first = (s.SDL_GetTicks.unwrap())();
        (s.SDL_Delay.unwrap())(2);
        assert!((s.SDL_GetTicks.unwrap())() >= first);
    }
}

#[test]
fn a_keys_name_comes_back_as_text() {
    skip_without_sdl!();
    let s = sdl().unwrap();
    // SDLK_SPACE is the ASCII code point, as SDL3's keycodes are.
    let name = unsafe { ffi::borrowed_str((s.SDL_GetKeyName.unwrap())(32)) };
    assert_eq!(name, "Space");
}
