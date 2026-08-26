//! Windows: creating, sizing, moving and showing them.

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimResult};

use crate::ffi::{check, sc, sdl};
use crate::resources::{
    as_c_int, as_c_int_positive, as_u64, destroy_window, with_window, Window, WINDOWS,
};

/// `SDL_CreateWindow`. Answers a window handle.
///
/// `flags` is an `SDL_WindowFlags` bit mask. SDL requires the video subsystem
/// to have been initialised first, on the thread that will pump events -- for
/// the VM that is the interpreter thread, which is where every primitive runs.
#[pharo_primitive]
fn primitiveCreateWindow(
    vm: &Interp,
    title: Oop,
    width: sqInt,
    height: sqInt,
    flags: sqInt,
) -> PrimResult<sqInt> {
    let s = sdl()?;
    let title = vm.c_string_value(title)?;
    let ptr = sc!(
        s,
        SDL_CreateWindow(
            title.as_ptr(),
            as_c_int_positive(width)?,
            as_c_int_positive(height)?,
            as_u64(flags)?,
        )
    );
    WINDOWS.insert(Window::adopt(ptr)?)
}

/// `SDL_DestroyWindow`.
///
/// Also invalidates the handles on this window's renderer and that renderer's
/// textures, which SDL destroys along with it.
#[pharo_primitive]
fn primitiveDestroyWindow(_vm: &Interp, window: sqInt) -> PrimResult<()> {
    destroy_window(window)
}

/// Is this still a live window handle?
#[pharo_primitive]
fn primitiveWindowIsLive(_vm: &Interp, window: sqInt) -> PrimResult<bool> {
    Ok(WINDOWS.is_live(window))
}

/// `SDL_GetWindowID`.
#[pharo_primitive]
fn primitiveGetWindowID(_vm: &Interp, window: sqInt) -> PrimResult<isize> {
    let s = sdl()?;
    with_window(window, |w| Ok(sc!(s, SDL_GetWindowID(w)) as isize))
}

/// `SDL_SetWindowTitle`.
#[pharo_primitive]
fn primitiveSetWindowTitle(vm: &Interp, window: sqInt, title: Oop) -> PrimResult<()> {
    let s = sdl()?;
    let title = vm.c_string_value(title)?;
    with_window(window, |w| {
        check(sc!(s, SDL_SetWindowTitle(w, title.as_ptr())))
    })
}

/// `SDL_GetWindowTitle`.
#[pharo_primitive]
fn primitiveGetWindowTitle(_vm: &Interp, window: sqInt) -> PrimResult<String> {
    let s = sdl()?;
    with_window(window, |w| {
        let ptr = sc!(s, SDL_GetWindowTitle(w));
        // SAFETY: SDL answers a NUL-terminated string it owns, valid until the
        // title is next set; it is copied out immediately.
        Ok(unsafe { crate::ffi::borrowed_str(ptr) })
    })
}

/// Declares a primitive answering a window's two-integer geometry as a Point.
///
/// A `Point` rather than an out-parameter because these are small and the
/// image already thinks in points; the doubles-in-a-ByteArray convention the
/// Cairo plugin uses is for things that come in fives and sixes.
macro_rules! window_point {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(vm: &Interp, window: sqInt) -> PrimResult<Oop> {
            let s = sdl()?;
            let (mut x, mut y) = (0, 0);
            with_window(window, |w| check(sc!(s, $entry(w, &mut x, &mut y))))?;
            vm.point(x as sqInt, y as sqInt)
        }
    };
}

window_point!(
    /// `SDL_GetWindowSize`, as `width @ height`.
    primitiveGetWindowSize => SDL_GetWindowSize
);
window_point!(
    /// `SDL_GetWindowPosition`, as `x @ y`.
    primitiveGetWindowPosition => SDL_GetWindowPosition
);

/// `SDL_SetWindowSize`.
#[pharo_primitive]
fn primitiveSetWindowSize(
    _vm: &Interp,
    window: sqInt,
    width: sqInt,
    height: sqInt,
) -> PrimResult<()> {
    let s = sdl()?;
    let (w_, h_) = (as_c_int_positive(width)?, as_c_int_positive(height)?);
    with_window(window, |w| check(sc!(s, SDL_SetWindowSize(w, w_, h_))))
}

/// `SDL_SetWindowPosition`. Negative coordinates are legal on a multi-monitor
/// desktop, so these are not range-restricted the way sizes are.
#[pharo_primitive]
fn primitiveSetWindowPosition(_vm: &Interp, window: sqInt, x: sqInt, y: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let (x_, y_) = (as_c_int(x)?, as_c_int(y)?);
    with_window(window, |w| check(sc!(s, SDL_SetWindowPosition(w, x_, y_))))
}

/// Declares a primitive that acts on a window and answers nothing.
macro_rules! window_action {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(_vm: &Interp, window: sqInt) -> PrimResult<()> {
            let s = sdl()?;
            with_window(window, |w| check(sc!(s, $entry(w))))
        }
    };
}

window_action!(
    /// `SDL_ShowWindow`.
    primitiveShowWindow => SDL_ShowWindow
);
window_action!(
    /// `SDL_HideWindow`.
    primitiveHideWindow => SDL_HideWindow
);
window_action!(
    /// `SDL_RaiseWindow`.
    primitiveRaiseWindow => SDL_RaiseWindow
);
window_action!(
    /// `SDL_SyncWindow`, which waits for pending changes to take effect.
    primitiveSyncWindow => SDL_SyncWindow
);

/// Declares a primitive that sets a boolean window property.
macro_rules! window_flag {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(_vm: &Interp, window: sqInt, value: bool) -> PrimResult<()> {
            let s = sdl()?;
            with_window(window, |w| check(sc!(s, $entry(w, value))))
        }
    };
}

window_flag!(
    /// `SDL_SetWindowFullscreen`.
    primitiveSetWindowFullscreen => SDL_SetWindowFullscreen
);
window_flag!(
    /// `SDL_SetWindowResizable`.
    primitiveSetWindowResizable => SDL_SetWindowResizable
);
