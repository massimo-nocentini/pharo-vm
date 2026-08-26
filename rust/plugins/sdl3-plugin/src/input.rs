//! Mouse, keyboard, clipboard and the clock.

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{borrowed_str, check, sc, sdl};
use crate::resources::{as_u32, with_window};

/// `SDL_GetMouseState`. Answers the button bit mask and writes the pointer
/// position, as two doubles, into the 16-byte ByteArray given.
///
/// SDL reports the position as single-precision floats; they are widened here
/// because everything else the image reads from these plugins is a double, and
/// nothing is lost by it.
#[pharo_primitive]
fn primitiveGetMouseState(vm: &Interp, position: Oop) -> PrimResult<isize> {
    let s = sdl()?;
    if usize::try_from(vm.byte_size_of(position)?)? != 16 {
        return Err(PrimErr::BadArgument);
    }
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let buttons = sc!(s, SDL_GetMouseState(&mut x, &mut y));
    vm.write_f64s(position, &[f64::from(x), f64::from(y)])?;
    Ok(buttons as isize)
}

/// `SDL_GetModState`, the current keyboard modifier bit mask.
#[pharo_primitive]
fn primitiveGetModState(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    Ok(sc!(s, SDL_GetModState()) as sqInt)
}

/// `SDL_GetKeyName`. Answers an empty string for a key with no name, as SDL
/// does.
#[pharo_primitive]
fn primitiveGetKeyName(_vm: &Interp, keycode: sqInt) -> PrimResult<String> {
    let s = sdl()?;
    let ptr = sc!(s, SDL_GetKeyName(as_u32(keycode)?));
    // SAFETY: SDL answers a NUL-terminated string it owns, valid until the
    // next call; it is copied out immediately.
    Ok(unsafe { borrowed_str(ptr) })
}

/// `SDL_StartTextInput`, which begins delivering `SDL_EVENT_TEXT_INPUT`.
#[pharo_primitive]
fn primitiveStartTextInput(_vm: &Interp, window: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    with_window(window, |w| check(sc!(s, SDL_StartTextInput(w))))
}

/// `SDL_StopTextInput`.
#[pharo_primitive]
fn primitiveStopTextInput(_vm: &Interp, window: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    with_window(window, |w| check(sc!(s, SDL_StopTextInput(w))))
}

/// `SDL_ShowCursor`.
#[pharo_primitive]
fn primitiveShowCursor(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    check(sc!(s, SDL_ShowCursor()))
}

/// `SDL_HideCursor`.
#[pharo_primitive]
fn primitiveHideCursor(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    check(sc!(s, SDL_HideCursor()))
}

/// `SDL_GetClipboardText`.
///
/// SDL hands back memory it expects returned; it is freed here before the
/// string reaches the image, which is a leak the image-side FFI binding has to
/// remember to avoid for itself.
#[pharo_primitive]
fn primitiveGetClipboardText(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    let ptr = sc!(s, SDL_GetClipboardText());
    if ptr.is_null() {
        return Ok(String::new());
    }
    // SAFETY: SDL answers NUL-terminated UTF-8 it allocated for us.
    let text = unsafe { borrowed_str(ptr) };
    sc!(s, SDL_free(ptr.cast()));
    Ok(text)
}

/// `SDL_SetClipboardText`.
#[pharo_primitive]
fn primitiveSetClipboardText(vm: &Interp, text: Oop) -> PrimResult<()> {
    let s = sdl()?;
    let text = vm.c_string_value(text)?;
    check(sc!(s, SDL_SetClipboardText(text.as_ptr())))
}

/// `SDL_HasClipboardText`.
#[pharo_primitive]
fn primitiveHasClipboardText(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    Ok(sc!(s, SDL_HasClipboardText()))
}

/// `SDL_GetTicks`, milliseconds since SDL was initialised.
#[pharo_primitive]
fn primitiveGetTicks(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    isize::try_from(sc!(s, SDL_GetTicks())).map_err(|_| PrimErr::LimitExceeded)
}

/// `SDL_Delay`.
///
/// Blocks the interpreter, and with it every Smalltalk process. Here because
/// the SDL API has it; the image should almost always use its own delays.
#[pharo_primitive]
fn primitiveDelay(_vm: &Interp, milliseconds: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    sc!(s, SDL_Delay(as_u32(milliseconds)?));
    Ok(())
}
