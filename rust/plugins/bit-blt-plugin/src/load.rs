//! The interpreter-facing half: reading the BitBlt object's instance
//! variables into [`BitBlt`] (`loadBitBltFrom:warping:`), OS-surface
//! locking through SurfacePlugin, and the composite operations
//! (`copyBits`, `warpBits`, `drawLoopX:Y:`).
//!
//! Control flow mirrors the C exactly, including its habit of continuing
//! after a failed accessor and consulting `failed()` at fixed points -- see
//! [`crate::vmcalls`].

use pharo_vm_plugin::{sqInt, Interp};

use crate::state::{BitBlt, CmTable, LockSurfaceFn, QuerySurfaceFn, UnlockSurfaceFn};
use crate::state::{
    BB_CLIP_HEIGHT_INDEX, BB_CLIP_WIDTH_INDEX, BB_CLIP_X_INDEX, BB_CLIP_Y_INDEX,
    BB_COLOR_MAP_INDEX, BB_DEST_FORM_INDEX, BB_DEST_X_INDEX, BB_DEST_Y_INDEX,
    BB_HALFTONE_FORM_INDEX, BB_HEIGHT_INDEX, BB_RULE_INDEX, BB_SOURCE_FORM_INDEX,
    BB_SOURCE_X_INDEX, BB_SOURCE_Y_INDEX, BB_WARP_BASE, BB_WIDTH_INDEX, BE_BIT_BLT_INDEX,
    COLOR_MAP_FIXED_PART, COLOR_MAP_INDEXED_PART, COLOR_MAP_NEW_STYLE, COLOR_MAP_PRESENT,
    FORM_BITS_INDEX, FORM_DEPTH_INDEX, FORM_HEIGHT_INDEX, FORM_WIDTH_INDEX, OP_TABLE_SIZE,
};
use crate::vmcalls as vmc;
use crate::warp::deltaFromtonSteps;

/// `PrimErrObjectMoved` / `PrimErrCallbackError` (interp.h).
pub const PRIM_ERR_OBJECT_MOVED: sqInt = 18;
pub const PRIM_ERR_CALLBACK_ERROR: sqInt = 20;

/// `fetchIntOrFloat:ofObject:` -- integer value of a field, truncating a
/// Float; fails the primitive on anything else or out-of-range Floats.
fn fetchIntOrFloatofObject(vm: &Interp, fieldIndex: sqInt, objectPointer: sqInt) -> sqInt {
    let fieldOop = vmc::fetchPointerofObject(vm, fieldIndex, objectPointer);
    if vmc::isIntegerObject(vm, fieldOop) {
        return vmc::integerValueOf(vm, fieldOop);
    }
    let floatValue = vmc::floatValueOf(vm, fieldOop);
    if !(-2.147_483_648e9..=2.147_483_647e9).contains(&floatValue) {
        vmc::primitiveFail(vm);
        return 0;
    }
    floatValue as sqInt
}

/// `fetchIntOrFloat:ofObject:ifNil:`.
fn fetchIntOrFloatofObjectifNil(
    vm: &Interp,
    fieldIndex: sqInt,
    objectPointer: sqInt,
    defaultValue: sqInt,
) -> sqInt {
    let fieldOop = vmc::fetchPointerofObject(vm, fieldIndex, objectPointer);
    if vmc::isIntegerObject(vm, fieldOop) {
        return vmc::integerValueOf(vm, fieldOop);
    }
    if fieldOop == vmc::nilObject(vm) {
        return defaultValue;
    }
    let floatValue = vmc::floatValueOf(vm, fieldOop);
    if !(-2.147_483_648e9..=2.147_483_647e9).contains(&floatValue) {
        vmc::primitiveFail(vm);
        return 0;
    }
    floatValue as sqInt
}

/// `ignoreSourceOrHalftone:`.
fn ignoreSourceOrHalftone(vm: &Interp, bb: &BitBlt, formPointer: sqInt) -> bool {
    if formPointer == vmc::nilObject(vm) {
        return true;
    }
    matches!(bb.combinationRule, 0 | 5 | 10 | 15)
}

/// `loadSurfacePlugin` -- resolve the SurfacePlugin entry points through
/// `ioLoadFunctionFrom`, caching them in the state (cleared again by
/// `moduleUnloaded`).
fn loadSurfacePlugin(vm: &Interp, bb: &mut BitBlt) -> bool {
    let query = vmc::ioLoadFunctionFrom(vm, b"ioGetSurfaceFormat\0", b"SurfacePlugin\0");
    let lock = vmc::ioLoadFunctionFrom(vm, b"ioLockSurface\0", b"SurfacePlugin\0");
    let unlock = vmc::ioLoadFunctionFrom(vm, b"ioUnlockSurface\0", b"SurfacePlugin\0");
    // SAFETY: the addresses come from SurfacePlugin's export table; the
    // signatures are its published ABI, unchanged since it exists.
    bb.querySurfaceFn = if query == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<usize, QuerySurfaceFn>(query) })
    };
    bb.lockSurfaceFn = if lock == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<usize, LockSurfaceFn>(lock) })
    };
    bb.unlockSurfaceFn = if unlock == 0 {
        None
    } else {
        Some(unsafe { core::mem::transmute::<usize, UnlockSurfaceFn>(unlock) })
    };
    query != 0 && lock != 0 && unlock != 0
}

/// Guard against depths the C would feed into `32 / depth` or
/// `pitch = width / ppw` with a zero divisor (SIGFPE). Divergence: the load
/// fails instead of crashing the VM; see the README.
fn depthUsable(depth: i32) -> bool {
    (1..=32).contains(&depth)
}

/// `loadBitBltDestForm` -- answer false if anything is wrong.
fn loadBitBltDestForm(vm: &Interp, bb: &mut BitBlt) -> bool {
    if !(vmc::isPointers(vm, bb.destForm) && vmc::slotSizeOf(vm, bb.destForm) >= 4) {
        return false;
    }
    let destBitsOop = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, bb.destForm);
    bb.destWidth = vmc::fetchIntegerofObject(vm, FORM_WIDTH_INDEX, bb.destForm) as i32;
    bb.destHeight = vmc::fetchIntegerofObject(vm, FORM_HEIGHT_INDEX, bb.destForm) as i32;
    if !(bb.destWidth >= 0 && bb.destHeight >= 0) {
        return false;
    }
    bb.destDepth = vmc::fetchIntegerofObject(vm, FORM_DEPTH_INDEX, bb.destForm) as i32;
    bb.destMSB = (bb.destDepth > 0) as i32;
    if bb.destMSB == 0 {
        bb.destDepth = -bb.destDepth;
    }
    if vmc::isIntegerObject(vm, destBitsOop) {
        // Query for actual surface dimensions
        if bb.querySurfaceFn.is_none() && !loadSurfacePlugin(vm, bb) {
            return false;
        }
        let query = bb.querySurfaceFn.expect("checked above");
        // SAFETY: resolved from SurfacePlugin with this signature.
        let ok = unsafe {
            query(
                vmc::integerValueOf(vm, destBitsOop),
                &mut bb.destWidth,
                &mut bb.destHeight,
                &mut bb.destDepth,
                &mut bb.destMSB,
            )
        };
        if ok == 0 {
            vmc::primitiveFailFor(vm, PRIM_ERR_CALLBACK_ERROR);
            return false;
        }
        if !depthUsable(bb.destDepth) {
            return false;
        }
        bb.destPPW = 32 / bb.destDepth as sqInt;
        bb.destBits = 0;
        bb.destPitch = 0;
    } else {
        if !vmc::isWordsOrBytes(vm, destBitsOop) {
            return false;
        }
        if !depthUsable(bb.destDepth) {
            return false;
        }
        bb.destPPW = 32 / bb.destDepth as sqInt;
        bb.destPitch = ((bb.destWidth as sqInt + (bb.destPPW - 1)) / bb.destPPW * 4) as i32;
        let destBitsSize = vmc::byteSizeOf(vm, destBitsOop);
        if destBitsSize < bb.destPitch as sqInt * bb.destHeight as sqInt {
            return false;
        }
        bb.destBits = vmc::firstIndexableFieldAddr(vm, destBitsOop);
    }
    true
}

/// `loadBitBltSourceForm`.
fn loadBitBltSourceForm(vm: &Interp, bb: &mut BitBlt) -> bool {
    if !(vmc::isPointers(vm, bb.sourceForm) && vmc::slotSizeOf(vm, bb.sourceForm) >= 4) {
        return false;
    }
    let sourceBitsOop = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, bb.sourceForm);
    bb.sourceWidth = fetchIntOrFloatofObject(vm, FORM_WIDTH_INDEX, bb.sourceForm) as i32;
    bb.sourceHeight = fetchIntOrFloatofObject(vm, FORM_HEIGHT_INDEX, bb.sourceForm) as i32;
    if !(bb.sourceWidth >= 0 && bb.sourceHeight >= 0) {
        return false;
    }
    bb.sourceDepth = vmc::fetchIntegerofObject(vm, FORM_DEPTH_INDEX, bb.sourceForm) as i32;
    bb.sourceMSB = (bb.sourceDepth > 0) as i32;
    if bb.sourceMSB == 0 {
        bb.sourceDepth = -bb.sourceDepth;
    }
    if vmc::isIntegerObject(vm, sourceBitsOop) {
        // Query for actual surface dimensions
        if bb.querySurfaceFn.is_none() && !loadSurfacePlugin(vm, bb) {
            return false;
        }
        let query = bb.querySurfaceFn.expect("checked above");
        // SAFETY: resolved from SurfacePlugin with this signature.
        let ok = unsafe {
            query(
                vmc::integerValueOf(vm, sourceBitsOop),
                &mut bb.sourceWidth,
                &mut bb.sourceHeight,
                &mut bb.sourceDepth,
                &mut bb.sourceMSB,
            )
        };
        if ok == 0 {
            vmc::primitiveFailFor(vm, PRIM_ERR_CALLBACK_ERROR);
            return false;
        }
        if !depthUsable(bb.sourceDepth) {
            return false;
        }
        bb.sourcePPW = 32 / bb.sourceDepth as sqInt;
        bb.sourceBits = 0;
        bb.sourcePitch = 0;
    } else {
        if !vmc::isWordsOrBytes(vm, sourceBitsOop) {
            return false;
        }
        if !depthUsable(bb.sourceDepth) {
            return false;
        }
        bb.sourcePPW = 32 / bb.sourceDepth as sqInt;
        bb.sourcePitch = ((bb.sourceWidth as sqInt + (bb.sourcePPW - 1)) / bb.sourcePPW * 4) as i32;
        let sourceBitsSize = vmc::byteSizeOf(vm, sourceBitsOop);
        if sourceBitsSize < bb.sourcePitch as sqInt * bb.sourceHeight as sqInt {
            return false;
        }
        bb.sourceBits = vmc::firstIndexableFieldAddr(vm, sourceBitsOop);
    }
    true
}

/// `loadColorMapShiftOrMaskFrom:`.
fn loadColorMapShiftOrMaskFrom(vm: &Interp, mapOop: sqInt) -> CmTable {
    if mapOop == vmc::nilObject(vm) {
        return CmTable::Null;
    }
    if !(vmc::isWords(vm, mapOop) && vmc::slotSizeOf(vm, mapOop) == 4) {
        vmc::primitiveFail(vm);
        return CmTable::Null;
    }
    CmTable::Image(vmc::firstIndexableFieldAddr(vm, mapOop))
}

/// `loadColorMap` -- ColorMap, if not nil, must be longWords, and 2^N long,
/// where N = sourceDepth for 1, 2, 4, 8 bits, or N = 9, 12, or 15 (3, 4, 5
/// bits per color) for 16 or 32 bits.
fn loadColorMap(vm: &Interp, bb: &mut BitBlt) -> bool {
    bb.cmFlags = 0;
    bb.cmMask = 0;
    bb.cmBitsPerColor = 0;
    bb.cmShiftTable = CmTable::Null;
    bb.cmMaskTable = CmTable::Null;
    bb.cmLookupTable = 0;
    let cmOop = vmc::fetchPointerofObject(vm, BB_COLOR_MAP_INDEX, bb.bitBltOop);
    if cmOop == vmc::nilObject(vm) {
        return true;
    }
    // even if identity or somesuch - may be cleared later
    bb.cmFlags = COLOR_MAP_PRESENT;
    let mut oldStyle = false;
    let cmSize: sqInt;
    if vmc::isWords(vm, cmOop) {
        // This is an old-style color map (indexed only, with implicit RGBA
        // conversion)
        cmSize = vmc::slotSizeOf(vm, cmOop);
        bb.cmLookupTable = vmc::firstIndexableFieldAddr(vm, cmOop);
        oldStyle = true;
    } else {
        // A new-style color map (fully qualified)
        if !(vmc::isPointers(vm, cmOop) && vmc::slotSizeOf(vm, cmOop) >= 3) {
            return false;
        }
        bb.cmShiftTable =
            loadColorMapShiftOrMaskFrom(vm, vmc::fetchPointerofObject(vm, 0, cmOop));
        bb.cmMaskTable =
            loadColorMapShiftOrMaskFrom(vm, vmc::fetchPointerofObject(vm, 1, cmOop));
        let oop = vmc::fetchPointerofObject(vm, 2, cmOop);
        if oop == vmc::nilObject(vm) {
            cmSize = 0;
        } else {
            if !vmc::isWords(vm, oop) {
                return false;
            }
            cmSize = vmc::slotSizeOf(vm, oop);
            bb.cmLookupTable = vmc::firstIndexableFieldAddr(vm, oop);
        }
        bb.cmFlags |= COLOR_MAP_NEW_STYLE;
    }
    if cmSize & (cmSize - 1) != 0 {
        return false;
    }
    bb.cmMask = cmSize - 1;
    bb.cmBitsPerColor = match cmSize {
        512 => 3,
        4096 => 4,
        32768 => 5,
        _ => 0,
    };
    if cmSize == 0 {
        bb.cmLookupTable = 0;
        bb.cmMask = 0;
    } else {
        bb.cmFlags |= COLOR_MAP_INDEXED_PART;
    }
    if oldStyle {
        // needs implicit conversion
        bb.setupColorMasks();
    }
    if bb.isIdentityMapwith() {
        bb.cmMaskTable = CmTable::Null;
        bb.cmShiftTable = CmTable::Null;
    } else {
        bb.cmFlags |= COLOR_MAP_FIXED_PART;
    }
    true
}

/// `loadHalftoneForm`.
fn loadHalftoneForm(vm: &Interp, bb: &mut BitBlt) -> bool {
    if bb.noHalftone {
        bb.halftoneBase = 0;
        return true;
    }
    let halftoneBits: sqInt;
    if vmc::isPointers(vm, bb.halftoneForm) && vmc::slotSizeOf(vm, bb.halftoneForm) >= 4 {
        // Old-style 32xN monochrome halftone Forms
        halftoneBits = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, bb.halftoneForm);
        bb.halftoneHeight = vmc::fetchIntegerofObject(vm, FORM_HEIGHT_INDEX, bb.halftoneForm);
        if !vmc::isWords(vm, halftoneBits) {
            bb.noHalftone = true;
        }
    } else {
        // New spec accepts, basically, a word array
        if !vmc::isWords(vm, bb.halftoneForm) {
            return false;
        }
        halftoneBits = bb.halftoneForm;
        bb.halftoneHeight = vmc::slotSizeOf(vm, halftoneBits);
    }
    bb.halftoneBase = vmc::firstIndexableFieldAddr(vm, halftoneBits);
    // Divergence: a non-positive height would make the C's copy loops take
    // `y % halftoneHeight` with a zero divisor (SIGFPE); fail the load.
    if !bb.noHalftone && bb.halftoneHeight <= 0 {
        return false;
    }
    true
}

/// `loadBitBltFrom:warping:` -- load context from the BitBlt instance.
/// Answers false if anything is amiss.
pub fn loadBitBltFromwarping(vm: &Interp, bb: &mut BitBlt, bbObj: sqInt, aBool: bool) -> bool {
    bb.bitBltOop = bbObj;
    bb.isWarping = aBool;
    bb.bitBltIsReceiver = bbObj == vmc::stackValue(vm, vmc::methodArgumentCount(vm));
    bb.numGCsOnInvocation = vmc::statNumGCs(vm);
    bb.combinationRule = vmc::fetchIntegerofObject(vm, BB_RULE_INDEX, bb.bitBltOop);
    if vmc::failed(vm) || bb.combinationRule < 0 || bb.combinationRule > OP_TABLE_SIZE - 2 {
        return false;
    }
    if bb.combinationRule >= 16 && bb.combinationRule <= 17 {
        return false;
    }
    bb.sourceForm = vmc::fetchPointerofObject(vm, BB_SOURCE_FORM_INDEX, bb.bitBltOop);
    bb.noSource = ignoreSourceOrHalftone(vm, bb, bb.sourceForm);
    bb.halftoneForm = vmc::fetchPointerofObject(vm, BB_HALFTONE_FORM_INDEX, bb.bitBltOop);
    bb.noHalftone = ignoreSourceOrHalftone(vm, bb, bb.halftoneForm);
    bb.destForm = vmc::fetchPointerofObject(vm, BB_DEST_FORM_INDEX, bbObj);
    if !loadBitBltDestForm(vm, bb) {
        return false;
    }
    bb.destX = fetchIntOrFloatofObjectifNil(vm, BB_DEST_X_INDEX, bb.bitBltOop, 0);
    bb.destY = fetchIntOrFloatofObjectifNil(vm, BB_DEST_Y_INDEX, bb.bitBltOop, 0);
    bb.width = fetchIntOrFloatofObjectifNil(vm, BB_WIDTH_INDEX, bb.bitBltOop, bb.destWidth as sqInt);
    bb.height =
        fetchIntOrFloatofObjectifNil(vm, BB_HEIGHT_INDEX, bb.bitBltOop, bb.destHeight as sqInt);
    if vmc::failed(vm) {
        return false;
    }
    if bb.noSource {
        bb.sourceX = 0;
        bb.sourceY = 0;
    } else {
        if !loadBitBltSourceForm(vm, bb) {
            return false;
        }
        if !loadColorMap(vm, bb) {
            return false;
        }
        if bb.cmFlags & COLOR_MAP_NEW_STYLE == 0 {
            bb.setupColorMasks();
        }
        bb.sourceX = fetchIntOrFloatofObjectifNil(vm, BB_SOURCE_X_INDEX, bb.bitBltOop, 0);
        bb.sourceY = fetchIntOrFloatofObjectifNil(vm, BB_SOURCE_Y_INDEX, bb.bitBltOop, 0);
    }
    if !loadHalftoneForm(vm, bb) {
        return false;
    }
    bb.clipX = fetchIntOrFloatofObjectifNil(vm, BB_CLIP_X_INDEX, bb.bitBltOop, 0);
    bb.clipY = fetchIntOrFloatofObjectifNil(vm, BB_CLIP_Y_INDEX, bb.bitBltOop, 0);
    bb.clipWidth =
        fetchIntOrFloatofObjectifNil(vm, BB_CLIP_WIDTH_INDEX, bb.bitBltOop, bb.destWidth as sqInt);
    bb.clipHeight = fetchIntOrFloatofObjectifNil(
        vm,
        BB_CLIP_HEIGHT_INDEX,
        bb.bitBltOop,
        bb.destHeight as sqInt,
    );
    if vmc::failed(vm) {
        return false;
    }
    if bb.clipX < 0 {
        bb.clipWidth += bb.clipX;
        bb.clipX = 0;
    }
    if bb.clipY < 0 {
        bb.clipHeight += bb.clipY;
        bb.clipY = 0;
    }
    if bb.clipX + bb.clipWidth > bb.destWidth as sqInt {
        bb.clipWidth = bb.destWidth as sqInt - bb.clipX;
    }
    if bb.clipY + bb.clipHeight > bb.destHeight as sqInt {
        bb.clipHeight = bb.destHeight as sqInt - bb.clipY;
    }
    if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
        // querySurface could be a callback in loadSourceForm/loadDestForm
        vmc::primitiveFailFor(vm, PRIM_ERR_OBJECT_MOVED);
        return false;
    }
    true
}

/// `reloadDestAndSourceForms` -- a GC has occurred; refetch the form oops
/// from the receiver (or the Balloon engine's BitBlt).
fn reloadDestAndSourceForms(vm: &Interp, bb: &mut BitBlt) {
    let mut receiver = vmc::stackValue(vm, vmc::methodArgumentCount(vm));
    if !bb.bitBltIsReceiver {
        receiver = vmc::fetchPointerofObject(vm, BE_BIT_BLT_INDEX, receiver);
    }
    bb.destForm = vmc::fetchPointerofObject(vm, BB_DEST_FORM_INDEX, receiver);
    bb.sourceForm = vmc::fetchPointerofObject(vm, BB_SOURCE_FORM_INDEX, receiver);
}

/// `showDisplayBits` / `ensureDestAndSourceFormsAreValid`.
pub fn showDisplayBits(vm: &Interp, bb: &mut BitBlt) {
    if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
        reloadDestAndSourceForms(vm, bb);
    }
}

/// `lockSurfaces` -- get a pointer to the bits of any OS surfaces. See the
/// long comment in the C for the locking rules.
pub fn lockSurfaces(vm: &Interp, bb: &mut BitBlt) -> bool {
    bb.hasSurfaceLock = false;
    if bb.destBits == 0 {
        // Blitting *to* OS surface
        if bb.lockSurfaceFn.is_none() && !loadSurfacePlugin(vm, bb) {
            return false;
        }
        let lock = bb.lockSurfaceFn.expect("checked above");
        let destHandle = vmc::fetchIntegerofObject(vm, FORM_BITS_INDEX, bb.destForm);
        if !(bb.sourceBits != 0 || bb.noSource) {
            // Handle the special case of equal source and dest handles
            let sourceHandle = vmc::fetchIntegerofObject(vm, FORM_BITS_INDEX, bb.sourceForm);
            if sourceHandle == destHandle {
                // If we have overlapping source/dest we lock the entire area
                // so that there is only one area transmitted. (The generated
                // C's comments here are swapped relative to its branches;
                // the branches are mirrored as compiled.)
                if bb.isWarping {
                    let l = bb.sx.min(bb.dx);
                    let r = bb.sx.max(bb.dx) + bb.bbW;
                    let t = bb.sy.min(bb.dy);
                    let b = bb.sy.max(bb.dy) + bb.bbH;
                    // SAFETY: SurfacePlugin ABI, as loadSurfacePlugin.
                    bb.sourceBits = unsafe {
                        lock(sourceHandle, &mut bb.sourcePitch, l, t, r - l, b - t)
                    } as usize;
                } else {
                    // SAFETY: as above.
                    bb.sourceBits = unsafe {
                        lock(sourceHandle, &mut bb.sourcePitch, 0, 0, bb.sourceWidth, bb.sourceHeight)
                    } as usize;
                }
                bb.destBits = bb.sourceBits;
                bb.destPitch = bb.sourcePitch;
                bb.hasSurfaceLock = true;
                if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
                    unlockSurfaces(vm, bb);
                    vmc::primitiveFailFor(vm, PRIM_ERR_OBJECT_MOVED);
                    return false;
                }
                if bb.destBits == 0 {
                    unlockSurfaces(vm, bb);
                    vmc::primitiveFailFor(vm, PRIM_ERR_CALLBACK_ERROR);
                    return false;
                }
                bb.endOfSource = bb
                    .sourceBits
                    .wrapping_add_signed(bb.sourcePitch as sqInt * bb.sourceHeight as sqInt);
                bb.endOfDestination = bb.endOfSource;
                return true;
            }
        }
        // SAFETY: as above.
        bb.destBits =
            unsafe { lock(destHandle, &mut bb.destPitch, bb.dx, bb.dy, bb.bbW, bb.bbH) } as usize;
        bb.hasSurfaceLock = true;
        if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
            unlockSurfaces(vm, bb);
            vmc::primitiveFailFor(vm, PRIM_ERR_OBJECT_MOVED);
            return false;
        }
        if bb.destBits == 0 {
            vmc::primitiveFailFor(vm, PRIM_ERR_CALLBACK_ERROR);
        }
    }
    if !(bb.sourceBits != 0 || bb.noSource) {
        // Blitting *from* OS surface
        let sourceHandle = vmc::fetchIntegerofObject(vm, FORM_BITS_INDEX, bb.sourceForm);
        if vmc::failed(vm) {
            return false;
        }
        if bb.lockSurfaceFn.is_none() && !loadSurfacePlugin(vm, bb) {
            return false;
        }
        let lock = bb.lockSurfaceFn.expect("checked above");
        if bb.isWarping {
            // When warping we always need the entire surface for the source
            // SAFETY: as above.
            bb.sourceBits = unsafe {
                lock(sourceHandle, &mut bb.sourcePitch, 0, 0, bb.sourceWidth, bb.sourceHeight)
            } as usize;
        } else {
            // SAFETY: as above.
            bb.sourceBits = unsafe {
                lock(sourceHandle, &mut bb.sourcePitch, bb.sx, bb.sy, bb.bbW, bb.bbH)
            } as usize;
        }
        bb.hasSurfaceLock = true;
        if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
            unlockSurfaces(vm, bb);
            vmc::primitiveFailFor(vm, PRIM_ERR_OBJECT_MOVED);
            return false;
        }
        if bb.sourceBits == 0 {
            vmc::primitiveFailFor(vm, PRIM_ERR_CALLBACK_ERROR);
        }
    }
    bb.endOfSource = if bb.noSource || bb.sourceBits == 0 {
        0
    } else {
        bb.sourceBits
            .wrapping_add_signed(bb.sourcePitch as sqInt * bb.sourceHeight as sqInt)
    };
    bb.endOfDestination = bb
        .destBits
        .wrapping_add_signed(bb.destPitch as sqInt * bb.destHeight as sqInt);
    bb.destBits != 0 && (bb.sourceBits != 0 || bb.noSource)
}

/// `unlockSurfaces` -- unlock the bits of any OS surfaces.
pub fn unlockSurfaces(vm: &Interp, bb: &mut BitBlt) {
    if !bb.hasSurfaceLock {
        return;
    }
    if bb.unlockSurfaceFn.is_none() && !loadSurfacePlugin(vm, bb) {
        return;
    }
    let unlock = bb.unlockSurfaceFn.expect("checked above");
    if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
        reloadDestAndSourceForms(vm, bb);
    }
    let mut destLocked = false;
    let destHandle = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, bb.destForm);
    if vmc::isIntegerObject(vm, destHandle) {
        // The destBits are always assumed to be dirty
        // SAFETY: SurfacePlugin ABI, as loadSurfacePlugin.
        unsafe {
            unlock(
                vmc::integerValueOf(vm, destHandle),
                bb.affectedL as i32,
                bb.affectedT as i32,
                (bb.affectedR - bb.affectedL) as i32,
                (bb.affectedB - bb.affectedT) as i32,
            )
        };
        bb.destBits = 0;
        bb.destPitch = 0;
        destLocked = true;
    }
    if !bb.noSource {
        if bb.numGCsOnInvocation != vmc::statNumGCs(vm) {
            reloadDestAndSourceForms(vm, bb);
        }
        let sourceHandle = vmc::fetchPointerofObject(vm, FORM_BITS_INDEX, bb.sourceForm);
        if vmc::isIntegerObject(vm, sourceHandle) {
            // Only unlock sourceHandle if different from destHandle
            if !(destLocked && sourceHandle == destHandle) {
                // SAFETY: as above.
                unsafe { unlock(vmc::integerValueOf(vm, sourceHandle), 0, 0, 0, 0) };
            }
            bb.sourceBits = 0;
            bb.sourcePitch = 0;
        }
    }
    bb.hasSurfaceLock = false;
}

/// `copyBitsRule41Test` -- fetch the rule-41 parameters from the stack.
fn copyBitsRule41Test(vm: &Interp, bb: &mut BitBlt) {
    if bb.combinationRule == 41 {
        // fetch the forecolor into componentAlphaModeColor.
        bb.componentAlphaModeAlpha = 0xFF;
        bb.componentAlphaModeColor = 0xFFFFFF;
        bb.gammaLookupTable = 0;
        bb.ungammaLookupTable = 0;
        let argc = vmc::methodArgumentCount(vm);
        if argc >= 2 {
            bb.componentAlphaModeAlpha = vmc::stackIntegerValue(vm, argc - 2);
            if vmc::failed(vm) {
                vmc::primitiveFail(vm);
                return;
            }
            bb.componentAlphaModeColor = vmc::stackIntegerValue(vm, argc - 1);
            if vmc::failed(vm) {
                vmc::primitiveFail(vm);
                return;
            }
            if argc == 4 {
                let gammaLookupTableOop = vmc::stackObjectValue(vm, 1);
                if vmc::isBytes(vm, gammaLookupTableOop) {
                    bb.gammaLookupTable = vmc::firstIndexableFieldAddr(vm, gammaLookupTableOop);
                }
                let ungammaLookupTableOop = vmc::stackObjectValue(vm, 0);
                if vmc::isBytes(vm, ungammaLookupTableOop) {
                    bb.ungammaLookupTable =
                        vmc::firstIndexableFieldAddr(vm, ungammaLookupTableOop);
                }
            }
        } else if argc == 1 {
            bb.componentAlphaModeColor = vmc::stackIntegerValue(vm, 0);
            if vmc::failed(vm) {
                vmc::primitiveFail(vm);
            }
        } else {
            vmc::primitiveFail(vm);
        }
    }
}

/// `copyBitsLockedAndClipped` -- the actual copyBits operation; assumes
/// surfaces have been locked and clipping was performed.
pub fn copyBitsLockedAndClipped(vm: &Interp, bb: &mut BitBlt) {
    copyBitsRule41Test(vm, bb);
    if vmc::failed(vm) {
        vmc::primitiveFail(vm);
        return;
    }
    // The C fetches the rule-30/31 alpha argument after its inlined
    // tryCopyingBitsQuickly; the quick path never applies to those rules, so
    // fetching before dispatch is order-equivalent.
    if bb.combinationRule >= 30 && bb.combinationRule <= 31 {
        // Check and fetch source alpha parameter for alpha blend
        if vmc::methodArgumentCount(vm) != 1 {
            vmc::primitiveFail(vm);
            return;
        }
        bb.sourceAlpha = vmc::stackIntegerValue(vm, 0);
        if vmc::failed(vm) || !(0..=0xFF).contains(&bb.sourceAlpha) {
            vmc::primitiveFail(vm);
            return;
        }
    }
    // SAFETY: loadBitBltFrom:warping: validated the bitmap geometry and
    // lockSurfaces pinned any OS surfaces, which is the engine's contract.
    unsafe { bb.copyBitsDispatch() };
}

/// `copyBits` -- clip, lock, blit, unlock. Exported for the Balloon engine
/// through the `copyBits` symbol in `lib.rs`.
pub fn copyBits(vm: &Interp, bb: &mut BitBlt) {
    bb.clipRange();
    if bb.bbW <= 0 || bb.bbH <= 0 {
        // zero width or height; noop
        bb.affectedL = 0;
        bb.affectedR = 0;
        bb.affectedT = 0;
        bb.affectedB = 0;
        return;
    }
    if !lockSurfaces(vm, bb) {
        vmc::primitiveFail(vm);
        return;
    }
    copyBitsLockedAndClipped(vm, bb);
    unlockSurfaces(vm, bb);
}

/// The interpreter half of `warpLoop`: fetch the quad points and smoothing
/// arguments, then run [`BitBlt::warpLoopBody`].
fn warpLoop(vm: &Interp, bb: &mut BitBlt) {
    if vmc::slotSizeOf(vm, bb.bitBltOop) < BB_WARP_BASE + 12 {
        vmc::primitiveFail(vm);
        return;
    }
    let mut nSteps = bb.height - 1;
    if nSteps <= 0 {
        nSteps = 1;
    }
    let mut pAx = fetchIntOrFloatofObject(vm, BB_WARP_BASE, bb.bitBltOop);
    let mut words = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 3, bb.bitBltOop);
    let deltaP12x = deltaFromtonSteps(pAx, words, nSteps);
    if deltaP12x < 0 {
        pAx = words - nSteps * deltaP12x;
    }
    let mut pAy = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 1, bb.bitBltOop);
    words = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 4, bb.bitBltOop);
    let deltaP12y = deltaFromtonSteps(pAy, words, nSteps);
    if deltaP12y < 0 {
        pAy = words - nSteps * deltaP12y;
    }
    let mut pBx = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 9, bb.bitBltOop);
    words = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 6, bb.bitBltOop);
    let deltaP43x = deltaFromtonSteps(pBx, words, nSteps);
    if deltaP43x < 0 {
        pBx = words - nSteps * deltaP43x;
    }
    let mut pBy = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 10, bb.bitBltOop);
    words = fetchIntOrFloatofObject(vm, BB_WARP_BASE + 7, bb.bitBltOop);
    let deltaP43y = deltaFromtonSteps(pBy, words, nSteps);
    if deltaP43y < 0 {
        pBy = words - nSteps * deltaP43y;
    }
    if vmc::failed(vm) {
        return;
    }
    let smoothingCount: sqInt;
    let mut sourceMap: usize = 0;
    if vmc::methodArgumentCount(vm) == 2 {
        smoothingCount = vmc::stackIntegerValue(vm, 1);
        let sourceMapOop = vmc::stackValue(vm, 0);
        if sourceMapOop == vmc::nilObject(vm) {
            if bb.sourceDepth < 16 {
                // color map is required to smooth non-RGB dest
                vmc::primitiveFail(vm);
                return;
            }
        } else {
            // The C writes `1U << sourceDepth`; for sourceDepth = 32 that is
            // UB which x86 resolves to 1 (shift count masked) -- mirrored
            // with a wrapping shift.
            if vmc::slotSizeOf(vm, sourceMapOop)
                < 1u32.wrapping_shl(bb.sourceDepth as u32) as sqInt
            {
                // sourceMap must be long enough for sourceDepth
                vmc::primitiveFail(vm);
                return;
            }
            sourceMap = vmc::firstIndexableFieldAddr(vm, sourceMapOop);
        }
    } else {
        smoothingCount = 1;
    }
    if smoothingCount < 1 {
        // Divergence: warpPickSmoothPixels would divide by the smoothing
        // count; the C has undefined behaviour (SIGFPE) for counts < 1.
        vmc::primitiveFail(vm);
        return;
    }
    // SAFETY: as copyBitsLockedAndClipped.
    unsafe {
        bb.warpLoopBody(
            pAx, pAy, pBx, pBy, deltaP12x, deltaP12y, deltaP43x, deltaP43y, smoothingCount,
            sourceMap,
        )
    };
}

/// `warpBits`.
pub fn warpBits(vm: &Interp, bb: &mut BitBlt) {
    let ns = bb.noSource;
    bb.noSource = true;
    bb.clipRange();
    bb.noSource = ns;
    if bb.noSource || bb.bbW <= 0 || bb.bbH <= 0 {
        // zero width or height; noop
        bb.affectedL = 0;
        bb.affectedR = 0;
        bb.affectedT = 0;
        bb.affectedB = 0;
        return;
    }
    if !lockSurfaces(vm, bb) {
        vmc::primitiveFail(vm);
        return;
    }
    bb.destMaskAndPointerInit();
    warpLoop(vm, bb);
    if bb.hDir > 0 {
        bb.affectedL = bb.dx as sqInt;
        bb.affectedR = bb.dx as sqInt + bb.bbW as sqInt;
    } else {
        bb.affectedL = (bb.dx as sqInt - bb.bbW as sqInt) + 1;
        bb.affectedR = bb.dx as sqInt + 1;
    }
    if bb.vDir > 0 {
        bb.affectedT = bb.dy as sqInt;
        bb.affectedB = bb.dy as sqInt + bb.bbH as sqInt;
    } else {
        bb.affectedT = (bb.dy as sqInt - bb.bbH as sqInt) + 1;
        bb.affectedB = bb.dy as sqInt + 1;
    }
    unlockSurfaces(vm, bb);
}

/// `drawLoopX:Y:` -- the primitive implementation of the line-drawing loop.
pub fn drawLoopXY(vm: &Interp, bb: &mut BitBlt, xDelta: sqInt, yDelta: sqInt) {
    let dx1: sqInt = xDelta.signum();
    let dy1: sqInt = yDelta.signum();
    let px = yDelta.abs();
    let py = xDelta.abs();
    // init null rectangle
    let mut affL: sqInt = 9999;
    let mut affT: sqInt = 9999;
    let mut affR: sqInt = -9999;
    let mut affB: sqInt = -9999;
    if py > px {
        // more horizontal
        let mut p = py / 2;
        for i in 1..=py {
            bb.destX += dx1;
            p -= px;
            if p < 0 {
                bb.destY += dy1;
                p += py;
            }
            if i < py {
                copyBits(vm, bb);
                if vmc::failed(vm) {
                    return;
                }
                if bb.affectedL < bb.affectedR && bb.affectedT < bb.affectedB {
                    // Affected rectangle grows along the line
                    affL = affL.min(bb.affectedL);
                    affR = affR.max(bb.affectedR);
                    affT = affT.min(bb.affectedT);
                    affB = affB.max(bb.affectedB);
                    if (affR - affL) * (affB - affT) > 4000 {
                        // If affected rectangle gets large, update it in
                        // chunks
                        bb.affectedL = affL;
                        bb.affectedR = affR;
                        bb.affectedT = affT;
                        bb.affectedB = affB;
                        showDisplayBits(vm, bb);
                        // init null rectangle
                        affL = 9999;
                        affT = 9999;
                        affR = -9999;
                        affB = -9999;
                    }
                }
            }
        }
    } else {
        // more vertical
        let mut p = px / 2;
        for i in 1..=px {
            bb.destY += dy1;
            p -= py;
            if p < 0 {
                bb.destX += dx1;
                p += px;
            }
            if i < px {
                copyBits(vm, bb);
                if vmc::failed(vm) {
                    return;
                }
                if bb.affectedL < bb.affectedR && bb.affectedT < bb.affectedB {
                    affL = affL.min(bb.affectedL);
                    affR = affR.max(bb.affectedR);
                    affT = affT.min(bb.affectedT);
                    affB = affB.max(bb.affectedB);
                    if (affR - affL) * (affB - affT) > 4000 {
                        bb.affectedL = affL;
                        bb.affectedR = affR;
                        bb.affectedT = affT;
                        bb.affectedB = affB;
                        showDisplayBits(vm, bb);
                        affL = 9999;
                        affT = 9999;
                        affR = -9999;
                        affB = -9999;
                    }
                }
            }
        }
    }
    bb.affectedL = affL;
    bb.affectedR = affR;
    bb.affectedT = affT;
    bb.affectedB = affB;
    // store destX, Y back
    vmc::storeIntegerofObjectwithValue(vm, BB_DEST_X_INDEX, bb.bitBltOop, bb.destX);
    vmc::storeIntegerofObjectwithValue(vm, BB_DEST_Y_INDEX, bb.bitBltOop, bb.destY);
}
