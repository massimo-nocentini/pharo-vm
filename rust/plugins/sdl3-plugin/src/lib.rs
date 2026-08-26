//! SDL3Plugin: SDL3 as Pharo named primitives.
//!
//! See `README.md` for the image-side contract: which primitive corresponds to
//! which `SDL_*` entry point, the handle rules, and the layout of the decoded
//! event record.

#![allow(non_snake_case)] // primitive names follow the image's pragmas
#![deny(unsafe_op_in_unsafe_fn)]

pub mod events;
pub mod ffi;
pub mod input;
pub mod render;
pub mod resources;
pub mod window;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimResult};

use resources::as_u32;

pharo_plugin!("SDL3Plugin", init = init, shutdown = shutdown);

/// Loads SDL3, and declines the module if it is not there.
///
/// Declining matters more here than for Cairo: on Linux the VM bundle does not
/// download SDL3 at all yet -- `cmake/importSDL2.cmake` fetches `SDL3-3.4.10`
/// only for Windows and macOS -- so a Linux image must be able to find out
/// that this backend is unavailable and keep using SDL2 through its FFI
/// binding.
fn init() -> bool {
    ffi::load()
}

/// Destroys every texture, renderer and window still registered.
///
/// `SDL_Quit` is deliberately *not* called: the image may have initialised SDL
/// for its own purposes through another route, and quitting the subsystem out
/// from under it would be worse than leaving it running in a process that is
/// ending anyway.
fn shutdown() -> bool {
    resources::release_all();
    true
}

/// Answers whether SDL3 is loaded. Never fails, so the image can ask before
/// committing to this backend.
#[pharo_primitive]
fn primitiveIsAvailable(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(ffi::sdl().is_ok())
}

/// The file the plugin loaded SDL3 from.
#[pharo_primitive]
fn primitiveLibraryPath(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(ffi::sdl()?.path.clone())
}

/// `SDL_GetVersion`, as `major * 1000000 + minor * 1000 + micro`.
#[pharo_primitive]
fn primitiveVersion(vm: &Interp) -> PrimResult<i32> {
    vm.expect_argument_count(0)?;
    let s = ffi::sdl()?;
    Ok(ffi::sc!(s, SDL_GetVersion()))
}

/// `SDL_Init`. Answers nothing; fails if SDL could not initialise, and the
/// reason is then in `primitiveGetError`.
///
/// SDL wants the video subsystem initialised on the thread that will pump
/// events. Every primitive runs on the interpreter thread, so as long as the
/// image only drives SDL through this plugin, that holds by construction.
#[pharo_primitive]
fn primitiveInit(_vm: &Interp, flags: sqInt) -> PrimResult<()> {
    let s = ffi::sdl()?;
    ffi::check(ffi::sc!(s, SDL_Init(as_u32(flags)?)))
}

/// `SDL_WasInit`, answering the mask of subsystems that are up.
#[pharo_primitive]
fn primitiveWasInit(_vm: &Interp, flags: sqInt) -> PrimResult<isize> {
    let s = ffi::sdl()?;
    Ok(ffi::sc!(s, SDL_WasInit(as_u32(flags)?)) as isize)
}

/// `SDL_Quit`.
///
/// Destroys everything this plugin is holding first, because SDL frees the
/// underlying objects and their handles must not outlive them.
#[pharo_primitive]
fn primitiveQuit(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    let s = ffi::sdl()?;
    resources::release_all();
    ffi::sc!(s, SDL_Quit());
    Ok(())
}

/// `SDL_GetError`, the message behind the last failed primitive.
#[pharo_primitive]
fn primitiveGetError(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let s = ffi::sdl()?;
    let ptr = ffi::sc!(s, SDL_GetError());
    // SAFETY: SDL answers a NUL-terminated string it owns, valid until the
    // next SDL call on this thread; it is copied out immediately.
    Ok(unsafe { ffi::borrowed_str(ptr) })
}

/// `SDL_ClearError`.
#[pharo_primitive]
fn primitiveClearError(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    let s = ffi::sdl()?;
    ffi::check(ffi::sc!(s, SDL_ClearError()))
}

/// `SDL_GetCurrentVideoDriver`, or an empty string when video is not up.
#[pharo_primitive]
fn primitiveGetCurrentVideoDriver(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let s = ffi::sdl()?;
    let ptr = ffi::sc!(s, SDL_GetCurrentVideoDriver());
    // SAFETY: SDL answers a static NUL-terminated string, or NULL.
    Ok(unsafe { ffi::borrowed_str(ptr) })
}

/// How many windows, renderers and textures the image is holding, as an Array
/// of three integers. For leak-hunting from the image side.
#[pharo_primitive]
fn primitiveLiveResourceCounts(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let counts = [
        resources::WINDOWS.len(),
        resources::RENDERERS.len(),
        resources::TEXTURES.len(),
    ];
    let array = vm.instantiate(vm.class_array()?, 3)?;
    for (i, n) in counts.iter().enumerate() {
        let oop = vm.integer_checked(isize::try_from(*n).unwrap_or(isize::MAX))?;
        vm.store_pointer(isize::try_from(i).unwrap_or(0), array, oop)?;
    }
    Ok(array)
}
