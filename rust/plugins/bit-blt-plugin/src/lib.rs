//! `BitBltPlugin`, in Rust: the VM's 2D blitter.
//!
//! Port of `plugins/BitBltPlugin/src/common/BitBltPlugin.c` (Slang-generated
//! from `BitBltSimulation`, VMMaker.oscog-eem.2493): `primitiveCopyBits`,
//! `primitiveWarpBits`, `primitiveDrawLoop`, `primitiveDisplayString`,
//! `primitivePixelValueAt`, `primitiveCompareColors`, and the `copyBits` /
//! `copyBitsFromtoat` / `loadBitBltFrom` / `moduleUnloaded` entry points the
//! Balloon engine and the module loader call directly.
//!
//! The port keeps the C's names and structure so the two can be read side by
//! side: [`state`] holds the C's file statics, [`rules`] the combination
//! rules, [`engine`] the copy loops, [`warp`] WarpBlt, and [`load`] the
//! interpreter-facing loading/locking half. The optional ARM SIMD fast paths
//! (`BitBltArm*`, `ENABLE_FAST_BLT`) are **not** ported: they are an
//! acceleration over the generic paths, which are complete on their own, and
//! the C plugin as built for Pharo compiles without them.

// The crate is named for the shared library the VM loads (libBitBltPlugin.so),
// and the port keeps the C's identifiers.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

mod engine;
mod load;
mod rules;
mod state;
mod vmcalls;
mod warp;

#[cfg(test)]
mod tests;

use std::ffi::CStr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard};

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use state::{BitBlt, BB_DEST_X_INDEX, FORM_BITS_INDEX, FORM_DEPTH_INDEX, FORM_HEIGHT_INDEX,
    FORM_WIDTH_INDEX};
use vmcalls as vmc;

// The C's external module name; the VM compares the prefix against
// "BitBltPlugin".
pharo_plugin!("BitBltPlugin VMMaker.oscog-eem.2493 (e)", init = initialiseModule_hook);

/// `initialiseModule` -- the C builds `opTable` and `dither8Lookup` here;
/// both are compile-time constants in this port, so there is nothing to do.
fn initialiseModule_hook() -> bool {
    true
}

/// The C's file-scope statics, as one lockable value. The VM calls
/// primitives from its single interpreter thread; the mutex only makes the
/// global sound Rust.
fn state() -> MutexGuard<'static, BitBlt> {
    static STATE: Mutex<BitBlt> = Mutex::new(BitBlt::new());
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The interpreter handle for the exported non-primitive entry points
/// (`copyBits` & co.), which the VM or another plugin calls outside the
/// `#[pharo_primitive]` machinery.
fn with_vm<R>(f: impl FnOnce(&Interp) -> R) -> Option<R> {
    let vt = pharo_vm_plugin::__private::INTERP.load(core::sync::atomic::Ordering::Acquire);
    if vt.is_null() {
        return None;
    }
    // SAFETY: set by the generated setInterpreter with the VM's own table.
    let vm = unsafe { Interp::from_raw(vt) };
    Some(f(&vm))
}

/// Mirrors the C paths that end with the failure flag raised: re-raise the
/// same code through the SDK's `Err` path (`primitiveFailFor` is idempotent
/// for an already-set code).
fn errFromFlag(vm: &Interp) -> PrimErr {
    vmc::currentFailure(vm)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// `primitiveCopyBits` -- invoke the copyBits operation. Answers `bitCount`
/// for the two diff rules (22, 32), the receiver otherwise.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveCopyBits(vm: &Interp) -> PrimResult<Oop> {
    let bb = &mut *state();
    let rcvr = vmc::stackValue(vm, vmc::methodArgumentCount(vm));
    if !load::loadBitBltFromwarping(vm, bb, rcvr, false) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    load::copyBits(vm, bb);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    load::showDisplayBits(vm, bb);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    if bb.combinationRule == 22 || bb.combinationRule == 32 {
        // methodReturnInteger in the C: unchecked SmallInteger tagging.
        Ok(Oop(vmc::integerObjectOf(vm, bb.bitCount)))
    } else {
        Ok(Oop(rcvr))
    }
}

/// `primitiveWarpBits`.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveWarpBits(vm: &Interp) -> PrimResult<()> {
    let bb = &mut *state();
    let rcvr = vmc::stackValue(vm, vmc::methodArgumentCount(vm));
    if !load::loadBitBltFromwarping(vm, bb, rcvr, true) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    load::warpBits(vm, bb);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    load::showDisplayBits(vm, bb);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    Ok(())
}

/// `primitiveDrawLoop` -- Bresenham line drawing by repeated copyBits.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveDrawLoop(vm: &Interp) -> PrimResult<()> {
    let bb = &mut *state();
    let rcvr = vmc::stackValue(vm, 2);
    let xDelta = vmc::stackIntegerValue(vm, 1);
    let yDelta = vmc::stackIntegerValue(vm, 0);
    if !load::loadBitBltFromwarping(vm, bb, rcvr, false) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    if !vmc::failed(vm) {
        load::drawLoopXY(vm, bb, xDelta, yDelta);
        load::showDisplayBits(vm, bb);
    }
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    // The C pops its 2 arguments; answering the receiver does the same.
    Ok(())
}

/// `primitiveDisplayString` -- glyph-by-glyph blitting of a byte string.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveDisplayString(vm: &Interp) -> PrimResult<()> {
    let bb = &mut *state();
    if vmc::methodArgumentCount(vm) != 6 {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    let kernDelta = vmc::stackIntegerValue(vm, 0);
    let xTable = vmc::stackValue(vm, 1);
    let glyphMap = vmc::stackValue(vm, 2);
    let stopIndex = vmc::stackIntegerValue(vm, 3);
    let startIndex = vmc::stackIntegerValue(vm, 4);
    let sourceString = vmc::stackValue(vm, 5);
    let bbObj = vmc::stackObjectValue(vm, 6);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    if !(vmc::isArray(vm, xTable)
        && vmc::isArray(vm, glyphMap)
        && vmc::slotSizeOf(vm, glyphMap) == 256
        && vmc::isBytes(vm, sourceString)
        && startIndex > 0
        && stopIndex >= 0
        && stopIndex <= vmc::byteSizeOf(vm, sourceString)
        && load::loadBitBltFromwarping(vm, bb, bbObj, false)
        && bb.combinationRule != 30
        && bb.combinationRule != 31)
    {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    if stopIndex == 0 {
        return Ok(()); // the C pops its 6 arguments
    }
    // See if we can go directly into copyLoopPixMap (usually we can)
    let maxGlyph = vmc::slotSizeOf(vm, xTable) - 2;
    // no point using slower version
    let quickBlt = bb.destBits != 0
        && bb.sourceBits != 0
        && !bb.noSource
        && bb.sourceForm != bb.destForm
        && (bb.cmFlags != 0 || bb.sourceMSB != bb.destMSB || bb.sourceDepth != bb.destDepth);
    if quickBlt {
        bb.endOfSource = bb
            .sourceBits
            .wrapping_add_signed(bb.sourcePitch as sqInt * bb.sourceHeight as sqInt);
        bb.endOfDestination = bb
            .destBits
            .wrapping_add_signed(bb.destPitch as sqInt * bb.destHeight as sqInt);
    } else if !load::lockSurfaces(vm, bb) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    let left = bb.destX;
    let sourcePtr = vmc::firstIndexableFieldAddr(vm, sourceString);
    for charIndex in startIndex..=stopIndex {
        // SAFETY: charIndex is within 1..=byteSizeOf(sourceString), checked
        // above, and sourcePtr is the string's first byte.
        let ascii =
            unsafe { state::byteAtPointer(sourcePtr + charIndex as usize - 1) } as sqInt;
        let glyphIndex = vmc::fetchIntegerofObject(vm, ascii, glyphMap);
        if glyphIndex < 0 || glyphIndex > maxGlyph {
            // As in the C: fails without unlocking the surfaces.
            vmc::primitiveFail(vm);
            return Err(errFromFlag(vm));
        }
        bb.sourceX = vmc::fetchIntegerofObject(vm, glyphIndex, xTable);
        bb.width = vmc::fetchIntegerofObject(vm, glyphIndex + 1, xTable) - bb.sourceX;
        if vmc::failed(vm) {
            return Err(errFromFlag(vm));
        }
        bb.clipRange();
        if bb.bbW > 0 && bb.bbH > 0 {
            if quickBlt {
                bb.destMaskAndPointerInit();
                // SAFETY: geometry validated by loadBitBltFrom:warping:.
                unsafe { bb.copyLoopPixMap() };
                bb.affectedL = bb.dx as sqInt;
                bb.affectedR = bb.dx as sqInt + bb.bbW as sqInt;
                bb.affectedT = bb.dy as sqInt;
                bb.affectedB = bb.dy as sqInt + bb.bbH as sqInt;
            } else {
                load::copyBitsLockedAndClipped(vm, bb);
            }
        }
        if vmc::failed(vm) {
            return Err(errFromFlag(vm));
        }
        bb.destX = bb.destX + bb.width + kernDelta;
    }
    bb.affectedL = left;
    if !quickBlt {
        load::unlockSurfaces(vm, bb);
    }
    load::showDisplayBits(vm, bb);
    vmc::storeIntegerofObjectwithValue(vm, BB_DEST_X_INDEX, bbObj, bb.destX);
    // The C pops its 6 arguments; answering the receiver does the same.
    Ok(())
}

/// `primitivePixelValueAtX:y:` -- the single pixel at x@y of the receiver
/// Form. Coordinates outside the form answer 0.
#[pharo_primitive(accessor_depth = 1)]
fn primitivePixelValueAt(vm: &Interp) -> PrimResult<Oop> {
    if !(vmc::isIntegerObject(vm, vmc::stackValue(vm, 1))
        && vmc::isIntegerObject(vm, vmc::stackValue(vm, 0)))
    {
        vmc::primitiveFailFor(vm, PrimErr::BadArgument.code());
        return Err(PrimErr::BadArgument);
    }
    let xVal = vmc::stackIntegerValue(vm, 1);
    let yVal = vmc::stackIntegerValue(vm, 0);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    if xVal < 0 || yVal < 0 {
        return Ok(Oop(vmc::integerObjectOf(vm, 0)));
    }
    let rcvr = vmc::stackValue(vm, vmc::methodArgumentCount(vm));
    if !(vmc::isPointers(vm, rcvr) && vmc::slotSizeOf(vm, rcvr) >= 4) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    let bitmap = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, rcvr);
    if !vmc::isWordsOrBytes(vm, bitmap) {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    let width = vmc::fetchIntegerofObject(vm, FORM_WIDTH_INDEX, rcvr);
    let height = vmc::fetchIntegerofObject(vm, FORM_HEIGHT_INDEX, rcvr);
    // if width/height/depth are not integer, fail
    let depth = vmc::fetchIntegerofObject(vm, FORM_DEPTH_INDEX, rcvr);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    if xVal >= width || yVal >= height {
        return Ok(Oop(vmc::integerObjectOf(vm, 0)));
    }
    if depth < 0 {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    // Divergence: depth 0 or > 32 puts a zero divisor into the C's
    // `32 / depth` / stride computation (SIGFPE); fail instead.
    if depth == 0 || depth > 32 {
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    // pixels in each word / how many words per row of pixels
    let ppW = 32 / depth;
    let stride = (width + (ppW - 1)) / ppW;
    let bitsSize = vmc::byteSizeOf(vm, bitmap);
    if bitsSize < stride * height * 4 {
        // bytes per word
        vmc::primitiveFail(vm);
        return Err(errFromFlag(vm));
    }
    // load the word that contains our target
    let word = vmc::fetchLong32ofObject(vm, yVal * stride + xVal / ppW, bitmap);
    // make a mask to isolate the pixel within that word
    let mask = 0xFFFF_FFFFu32 >> (32 - depth);
    // this is the tricky MSB part - we mask the xVal to find how far into the
    // word we need, then add 1 for the pixel we're looking for, then * depth
    // to get the bit shift
    let shift = 32 - ((xVal & (ppW - 1)) + 1) * depth;
    // shift, mask and dim the lights
    let pixel = state::shr32(word as u32, shift) & mask;
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    let result = vmc::positive32BitIntegerFor(vm, pixel);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    Ok(Oop(result))
}

/// `primitiveCompareColors` -- only functional with `ENABLE_FAST_BLT`, which
/// this port (like the Pharo build of the C) does not compile; after
/// validating its arguments it fails, exactly as the C does.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveCompareColors(vm: &Interp) -> PrimResult<()> {
    if !(vmc::isPositiveMachineIntegerObject(vm, vmc::stackValue(vm, 2))
        && vmc::isPositiveMachineIntegerObject(vm, vmc::stackValue(vm, 1))
        && vmc::isIntegerObject(vm, vmc::stackValue(vm, 0)))
    {
        vmc::primitiveFailFor(vm, PrimErr::BadArgument.code());
        return Err(PrimErr::BadArgument);
    }
    let _colorA = if core::mem::size_of::<sqInt>() == 4 {
        vmc::positive32BitValueOf(vm, vmc::stackValue(vm, 2)) as u64
    } else {
        vmc::positive64BitValueOf(vm, vmc::stackValue(vm, 2))
    };
    let _colorB = if core::mem::size_of::<sqInt>() == 4 {
        vmc::positive32BitValueOf(vm, vmc::stackValue(vm, 1)) as u64
    } else {
        vmc::positive64BitValueOf(vm, vmc::stackValue(vm, 1))
    };
    let _testID = vmc::stackIntegerValue(vm, 0);
    let _rcvr = vmc::stackValue(vm, 3);
    if vmc::failed(vm) {
        return Err(errFromFlag(vm));
    }
    vmc::primitiveFail(vm);
    Err(PrimErr::GenericFailure)
}

// ---------------------------------------------------------------------------
// Non-primitive exports (Balloon engine, module loader)
// ---------------------------------------------------------------------------

/// Accessor depth for the exported `copyBits`, as the C exports it.
#[used]
#[no_mangle]
pub static copyBitsAccessorDepth: core::ffi::c_schar = 3;

/// `copyBits` -- exported for the Balloon engine, operating on the state a
/// prior `loadBitBltFrom` established.
#[no_mangle]
pub extern "C" fn copyBits() -> sqInt {
    catch_unwind(AssertUnwindSafe(|| {
        with_vm(|vm| {
            let bb = &mut *state();
            load::copyBits(vm, bb);
        });
    }))
    .ok();
    0
}

/// `copyBitsFrom:to:at:` -- support for the Balloon engine: blit one span.
#[no_mangle]
pub extern "C" fn copyBitsFromtoat(startX: sqInt, stopX: sqInt, yValue: sqInt) -> sqInt {
    catch_unwind(AssertUnwindSafe(|| {
        with_vm(|vm| {
            let bb = &mut *state();
            bb.destX = startX;
            bb.destY = yValue;
            bb.sourceX = startX;
            bb.width = stopX - startX;
            load::copyBits(vm, bb);
            load::showDisplayBits(vm, bb);
        });
    }))
    .ok();
    0
}

/// `loadBitBltFrom` -- exported for the Balloon engine.
#[no_mangle]
pub extern "C" fn loadBitBltFrom(bbObj: sqInt) -> sqInt {
    catch_unwind(AssertUnwindSafe(|| {
        with_vm(|vm| {
            let bb = &mut *state();
            load::loadBitBltFromwarping(vm, bb, bbObj, false) as sqInt
        })
        .unwrap_or(0)
    }))
    .unwrap_or(0)
}

/// `moduleUnloaded:` -- drop the cached SurfacePlugin entry points when that
/// module goes away.
///
/// # Safety
///
/// `aModuleName` must be null or a NUL-terminated string, which is the VM's
/// calling convention for this hook.
#[no_mangle]
pub unsafe extern "C" fn moduleUnloaded(aModuleName: *const core::ffi::c_char) -> sqInt {
    catch_unwind(AssertUnwindSafe(|| {
        if aModuleName.is_null() {
            return;
        }
        // SAFETY: the VM passes a NUL-terminated module name.
        let name = unsafe { CStr::from_ptr(aModuleName) };
        if name.to_bytes() == b"SurfacePlugin" {
            // The surface plugin just shut down. How nasty.
            let bb = &mut *state();
            bb.querySurfaceFn = None;
            bb.lockSurfaceFn = None;
            bb.unlockSurfaceFn = None;
        }
    }))
    .ok();
    0
}
