//! Events: the raw queue, and a decoded record the image can read without
//! knowing SDL's union layout.
//!
//! Two primitives, deliberately:
//!
//! * `primitivePollEventRaw` copies the 128 bytes of `SDL_Event` verbatim.
//!   The union is explicitly padded to that size in the header so it does not
//!   vary by platform, and this is the escape hatch: anything this plugin does
//!   not decode, the image can still get at, exactly as its FFI binding does
//!   today.
//! * `primitivePollEvent` writes the fixed 64-byte record documented in the
//!   README. Nothing about SDL's field offsets leaks into the image, so an SDL
//!   that moves a field breaks this crate's tests rather than the image's
//!   event handling.
//!
//! `SDL_EVENT_TEXT_INPUT` is the one event whose payload does not fit: it
//! carries a `const char *` SDL owns and reuses. The decoder copies the text
//! into the plugin and `primitiveLastTextInput` answers it, which is why the
//! image must read it before pumping the next event.

use core::ffi::c_void;
use std::sync::Mutex;

use pharo_vm_plugin::poison;
use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    borrowed_str, sc, sdl, SDL_CommonEvent, SDL_KeyboardEvent, SDL_MouseButtonEvent,
    SDL_MouseMotionEvent, SDL_MouseWheelEvent, SDL_TextInputEvent, SDL_WindowEvent,
    EVENT_RECORD_SIZE, SDL_EVENT_KEY_DOWN, SDL_EVENT_KEY_UP, SDL_EVENT_MOUSE_BUTTON_DOWN,
    SDL_EVENT_MOUSE_BUTTON_UP, SDL_EVENT_MOUSE_MOTION, SDL_EVENT_MOUSE_WHEEL, SDL_EVENT_SIZE,
    SDL_EVENT_TEXT_INPUT, SDL_EVENT_WINDOW_FIRST, SDL_EVENT_WINDOW_LAST,
};

/// The text from the most recently decoded `SDL_EVENT_TEXT_INPUT`.
///
/// SDL's `text` pointer is only good until the next pump, so the text is
/// copied here as the event is decoded and answered separately.
///
/// Reached through [`poison::lock`] on both sides. The store is one
/// whole-value assignment, so there is nothing here a panic can tear; the
/// [`Section`](pharo_vm_plugin::Section) it opens is what puts this global
/// under the same module-wide fail-fast rule as every other one in the tree.
static LAST_TEXT_INPUT: Mutex<String> = Mutex::new(String::new());

/// A raw event, aligned as SDL's union is.
///
/// `u64` elements rather than `u8` because the union's alignment is 8 -- the
/// header says so explicitly -- and the decoder reads `u32`s and `f32`s out of
/// it at their natural offsets.
#[repr(C, align(8))]
struct RawEvent([u64; SDL_EVENT_SIZE / 8]);

impl RawEvent {
    const fn zeroed() -> Self {
        Self([0; SDL_EVENT_SIZE / 8])
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        core::ptr::addr_of_mut!(self.0).cast::<c_void>()
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: any bit pattern is a valid `u8`, and the array is exactly
        // SDL_EVENT_SIZE bytes long.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), SDL_EVENT_SIZE) }
    }

    /// Reinterprets the event as one of its members.
    ///
    /// # Safety
    ///
    /// `T` must be a member of `SDL_Event` -- every one is at most 128 bytes
    /// and 8-aligned, which this buffer satisfies -- and the caller must have
    /// checked the event's `type` field first.
    unsafe fn read<T: Copy>(&self) -> T {
        debug_assert!(core::mem::size_of::<T>() <= SDL_EVENT_SIZE);
        // SAFETY: delegated to this function's contract.
        unsafe { self.0.as_ptr().cast::<T>().read() }
    }

}

/// The plugin's own decoded event, laid out for the image.
///
/// The layout is documented in the README and answered by
/// `primitiveEventRecordSize`; it is deliberately all fixed-width fields at
/// fixed offsets, so the image reads it with plain integer accessors.
#[derive(Debug, Default, Clone, Copy)]
struct Record {
    event_type: u32,
    window_id: u32,
    timestamp_ns: u64,
    a: i32,
    b: i32,
    c: i32,
    d: i32,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
}

impl Record {
    fn to_bytes(self) -> [u8; EVENT_RECORD_SIZE] {
        let mut out = [0u8; EVENT_RECORD_SIZE];
        out[0..4].copy_from_slice(&self.event_type.to_ne_bytes());
        out[4..8].copy_from_slice(&self.window_id.to_ne_bytes());
        out[8..16].copy_from_slice(&self.timestamp_ns.to_ne_bytes());
        out[16..20].copy_from_slice(&self.a.to_ne_bytes());
        out[20..24].copy_from_slice(&self.b.to_ne_bytes());
        out[24..28].copy_from_slice(&self.c.to_ne_bytes());
        out[28..32].copy_from_slice(&self.d.to_ne_bytes());
        out[32..40].copy_from_slice(&self.x.to_ne_bytes());
        out[40..48].copy_from_slice(&self.y.to_ne_bytes());
        out[48..56].copy_from_slice(&self.dx.to_ne_bytes());
        out[56..64].copy_from_slice(&self.dy.to_ne_bytes());
        out
    }
}

/// Turns a raw event into the record the image reads.
///
/// An event this does not recognise still yields its type, window and
/// timestamp, with the payload fields left at zero -- so the image can route
/// on the type and fall back to `primitivePollEventRaw` for the rest, rather
/// than losing the event entirely.
fn decode(raw: &RawEvent) -> Record {
    // SAFETY throughout: each `read` is guarded by the type test that selects
    // it, which is what tells us which member of the union is live.
    let common = unsafe { raw.read::<SDL_CommonEvent>() };
    let mut r = Record {
        event_type: common.r#type,
        timestamp_ns: common.timestamp,
        ..Record::default()
    };

    match common.r#type {
        SDL_EVENT_KEY_DOWN | SDL_EVENT_KEY_UP => {
            let e = unsafe { raw.read::<SDL_KeyboardEvent>() };
            r.window_id = e.windowID;
            r.a = e.scancode as i32;
            r.b = e.key as i32;
            r.c = i32::from(e.r#mod);
            r.d = i32::from(e.repeat);
        }
        SDL_EVENT_MOUSE_MOTION => {
            let e = unsafe { raw.read::<SDL_MouseMotionEvent>() };
            r.window_id = e.windowID;
            r.a = e.state as i32;
            r.b = e.which as i32;
            r.x = f64::from(e.x);
            r.y = f64::from(e.y);
            r.dx = f64::from(e.xrel);
            r.dy = f64::from(e.yrel);
        }
        SDL_EVENT_MOUSE_BUTTON_DOWN | SDL_EVENT_MOUSE_BUTTON_UP => {
            let e = unsafe { raw.read::<SDL_MouseButtonEvent>() };
            r.window_id = e.windowID;
            r.a = i32::from(e.button);
            r.b = i32::from(e.clicks);
            r.c = e.which as i32;
            r.x = f64::from(e.x);
            r.y = f64::from(e.y);
        }
        SDL_EVENT_MOUSE_WHEEL => {
            let e = unsafe { raw.read::<SDL_MouseWheelEvent>() };
            r.window_id = e.windowID;
            r.a = e.direction as i32;
            r.b = e.which as i32;
            r.c = e.integer_x;
            r.d = e.integer_y;
            r.x = f64::from(e.x);
            r.y = f64::from(e.y);
            r.dx = f64::from(e.mouse_x);
            r.dy = f64::from(e.mouse_y);
        }
        SDL_EVENT_TEXT_INPUT => {
            let e = unsafe { raw.read::<SDL_TextInputEvent>() };
            r.window_id = e.windowID;
            // SAFETY: SDL guarantees `text` is NUL-terminated UTF-8 for a
            // TEXT_INPUT event; it is copied out before the next pump.
            let text = unsafe { borrowed_str(e.text) };
            r.a = i32::try_from(text.len()).unwrap_or(i32::MAX);
            if let Ok(mut slot) = poison::lock(&LAST_TEXT_INPUT) {
                *slot = text;
            }
        }
        t if (SDL_EVENT_WINDOW_FIRST..=SDL_EVENT_WINDOW_LAST).contains(&t) => {
            let e = unsafe { raw.read::<SDL_WindowEvent>() };
            r.window_id = e.windowID;
            r.a = e.data1;
            r.b = e.data2;
        }
        _ => {}
    }
    r
}

/// How many bytes `primitivePollEventRaw` writes.
#[pharo_primitive]
fn primitiveRawEventSize(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(SDL_EVENT_SIZE as isize)
}

/// How many bytes `primitivePollEvent` writes.
#[pharo_primitive]
fn primitiveEventRecordSize(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(EVENT_RECORD_SIZE as isize)
}

/// `SDL_PumpEvents`.
#[pharo_primitive]
fn primitivePumpEvents(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    let s = sdl()?;
    sc!(s, SDL_PumpEvents());
    Ok(())
}

/// `SDL_PollEvent` into a decoded record. Answers whether there was an event.
///
/// `record` must be a ByteArray of exactly `primitiveEventRecordSize` bytes.
/// It is left untouched when there is no event.
#[pharo_primitive]
fn primitivePollEvent(vm: &Interp, record: Oop) -> PrimResult<bool> {
    let s = sdl()?;
    if usize::try_from(vm.byte_size_of(record)?)? != EVENT_RECORD_SIZE {
        return Err(PrimErr::BadArgument);
    }
    let mut raw = RawEvent::zeroed();
    if !sc!(s, SDL_PollEvent(raw.as_mut_ptr())) {
        return Ok(false);
    }
    vm.write_bytes(record, 0, &decode(&raw).to_bytes())?;
    Ok(true)
}

/// `SDL_PollEvent` into the raw 128-byte `SDL_Event`.
///
/// For anything the decoded record does not carry. The layout is SDL's, so the
/// image takes on the same coupling its FFI binding already has.
#[pharo_primitive]
fn primitivePollEventRaw(vm: &Interp, event: Oop) -> PrimResult<bool> {
    let s = sdl()?;
    if usize::try_from(vm.byte_size_of(event)?)? != SDL_EVENT_SIZE {
        return Err(PrimErr::BadArgument);
    }
    let mut raw = RawEvent::zeroed();
    if !sc!(s, SDL_PollEvent(raw.as_mut_ptr())) {
        return Ok(false);
    }
    // Decoded as well, so `primitiveLastTextInput` works whichever poll the
    // image uses.
    let _ = decode(&raw);
    vm.write_bytes(event, 0, raw.bytes())?;
    Ok(true)
}

/// `SDL_WaitEventTimeout` into a decoded record.
///
/// Blocks the interpreter for up to `timeoutMilliseconds`, during which no
/// Smalltalk process runs -- the image should keep the timeout short, or poll.
#[pharo_primitive]
fn primitiveWaitEventTimeout(
    vm: &Interp,
    record: Oop,
    timeoutMilliseconds: sqInt,
) -> PrimResult<bool> {
    let s = sdl()?;
    if usize::try_from(vm.byte_size_of(record)?)? != EVENT_RECORD_SIZE {
        return Err(PrimErr::BadArgument);
    }
    let timeout = i32::try_from(timeoutMilliseconds).map_err(|_| PrimErr::BadArgument)?;
    let mut raw = RawEvent::zeroed();
    if !sc!(s, SDL_WaitEventTimeout(raw.as_mut_ptr(), timeout)) {
        return Ok(false);
    }
    vm.write_bytes(record, 0, &decode(&raw).to_bytes())?;
    Ok(true)
}

/// The text of the most recent `SDL_EVENT_TEXT_INPUT`.
///
/// Must be read before the next poll: SDL reuses the buffer, and this answers
/// what was copied out of it at decode time.
#[pharo_primitive]
fn primitiveLastTextInput(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(poison::lock(&LAST_TEXT_INPUT)?.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_from<T: Copy>(value: T) -> RawEvent {
        let mut raw = RawEvent::zeroed();
        assert!(core::mem::size_of::<T>() <= SDL_EVENT_SIZE);
        // SAFETY: writing a union member into a buffer big enough for the
        // whole union, which is what SDL does itself.
        unsafe { raw.0.as_mut_ptr().cast::<T>().write(value) };
        raw
    }

    #[test]
    fn a_record_serialises_at_the_documented_offsets() {
        let r = Record {
            event_type: 0x300,
            window_id: 7,
            timestamp_ns: 42,
            a: -1,
            b: 2,
            c: 3,
            d: 4,
            x: 1.5,
            y: 2.5,
            dx: 3.5,
            dy: 4.5,
        };
        let b = r.to_bytes();
        assert_eq!(b.len(), EVENT_RECORD_SIZE);
        assert_eq!(u32::from_ne_bytes(b[0..4].try_into().unwrap()), 0x300);
        assert_eq!(u32::from_ne_bytes(b[4..8].try_into().unwrap()), 7);
        assert_eq!(u64::from_ne_bytes(b[8..16].try_into().unwrap()), 42);
        assert_eq!(i32::from_ne_bytes(b[16..20].try_into().unwrap()), -1);
        assert_eq!(i32::from_ne_bytes(b[28..32].try_into().unwrap()), 4);
        assert_eq!(f64::from_ne_bytes(b[32..40].try_into().unwrap()), 1.5);
        assert_eq!(f64::from_ne_bytes(b[56..64].try_into().unwrap()), 4.5);
    }

    #[test]
    fn a_keyboard_event_decodes_into_the_documented_fields() {
        let raw = raw_from(SDL_KeyboardEvent {
            r#type: SDL_EVENT_KEY_DOWN,
            reserved: 0,
            timestamp: 99,
            windowID: 3,
            which: 0,
            scancode: 44,
            key: 32,
            r#mod: 0x0040,
            raw: 0,
            down: true,
            repeat: true,
        });
        let r = decode(&raw);
        assert_eq!(r.event_type, SDL_EVENT_KEY_DOWN);
        assert_eq!(r.window_id, 3);
        assert_eq!(r.timestamp_ns, 99);
        assert_eq!(r.a, 44, "scancode");
        assert_eq!(r.b, 32, "keycode");
        assert_eq!(r.c, 0x40, "modifiers");
        assert_eq!(r.d, 1, "repeat");
    }

    #[test]
    fn a_mouse_motion_event_keeps_position_and_movement_apart() {
        let raw = raw_from(SDL_MouseMotionEvent {
            r#type: SDL_EVENT_MOUSE_MOTION,
            reserved: 0,
            timestamp: 1,
            windowID: 2,
            which: 5,
            state: 0b101,
            x: 10.5,
            y: 20.25,
            xrel: -1.5,
            yrel: 2.0,
        });
        let r = decode(&raw);
        assert_eq!(r.a, 0b101, "button state");
        assert_eq!(r.b, 5, "mouse id");
        assert_eq!((r.x, r.y), (10.5, 20.25));
        assert_eq!((r.dx, r.dy), (-1.5, 2.0));
    }

    #[test]
    fn a_mouse_button_event_carries_button_and_click_count() {
        let raw = raw_from(SDL_MouseButtonEvent {
            r#type: SDL_EVENT_MOUSE_BUTTON_DOWN,
            reserved: 0,
            timestamp: 1,
            windowID: 2,
            which: 9,
            button: 3,
            down: true,
            clicks: 2,
            padding: 0,
            x: 4.0,
            y: 5.0,
        });
        let r = decode(&raw);
        assert_eq!(r.a, 3, "button");
        assert_eq!(r.b, 2, "clicks");
        assert_eq!(r.c, 9, "mouse id");
        assert_eq!((r.x, r.y), (4.0, 5.0));
    }

    #[test]
    fn a_wheel_event_carries_both_the_float_and_the_tick_counts() {
        let raw = raw_from(SDL_MouseWheelEvent {
            r#type: SDL_EVENT_MOUSE_WHEEL,
            reserved: 0,
            timestamp: 1,
            windowID: 2,
            which: 0,
            x: 0.5,
            y: -1.5,
            direction: 1,
            mouse_x: 7.0,
            mouse_y: 8.0,
            integer_x: 0,
            integer_y: -1,
        });
        let r = decode(&raw);
        assert_eq!(r.a, 1, "direction");
        assert_eq!((r.x, r.y), (0.5, -1.5));
        assert_eq!((r.c, r.d), (0, -1), "whole ticks");
        assert_eq!((r.dx, r.dy), (7.0, 8.0), "pointer position");
    }

    #[test]
    fn a_window_event_carries_its_two_data_words() {
        let raw = raw_from(SDL_WindowEvent {
            r#type: SDL_EVENT_WINDOW_FIRST + 5,
            reserved: 0,
            timestamp: 1,
            windowID: 11,
            data1: 640,
            data2: 480,
        });
        let r = decode(&raw);
        assert_eq!(r.window_id, 11);
        assert_eq!((r.a, r.b), (640, 480));
    }

    #[test]
    fn an_unknown_event_still_answers_its_type_and_timestamp() {
        // The image must be able to route on the type and reach for the raw
        // bytes, rather than seeing the event vanish.
        let raw = raw_from(SDL_CommonEvent {
            r#type: 0x7FFF,
            reserved: 0,
            timestamp: 1234,
        });
        let r = decode(&raw);
        assert_eq!(r.event_type, 0x7FFF);
        assert_eq!(r.timestamp_ns, 1234);
        assert_eq!((r.a, r.b, r.c, r.d), (0, 0, 0, 0));
    }

    #[test]
    fn a_text_input_event_stashes_its_text_out_of_band() {
        let text = std::ffi::CString::new("ão").unwrap();
        let raw = raw_from(SDL_TextInputEvent {
            r#type: SDL_EVENT_TEXT_INPUT,
            reserved: 0,
            timestamp: 1,
            windowID: 4,
            text: text.as_ptr(),
        });
        let r = decode(&raw);
        assert_eq!(r.window_id, 4);
        assert_eq!(r.a, 3, "length in bytes, not characters");
        assert_eq!(*LAST_TEXT_INPUT.lock().unwrap(), "ão");
    }

    #[test]
    fn a_text_input_event_with_no_text_does_not_crash() {
        let raw = raw_from(SDL_TextInputEvent {
            r#type: SDL_EVENT_TEXT_INPUT,
            reserved: 0,
            timestamp: 1,
            windowID: 4,
            text: core::ptr::null(),
        });
        assert_eq!(decode(&raw).a, 0);
    }

    #[test]
    fn the_raw_buffer_is_exactly_sdls_event_size_and_alignment() {
        assert_eq!(core::mem::size_of::<RawEvent>(), SDL_EVENT_SIZE);
        assert_eq!(core::mem::align_of::<RawEvent>(), 8);
        assert_eq!(RawEvent::zeroed().bytes().len(), SDL_EVENT_SIZE);
    }
}
