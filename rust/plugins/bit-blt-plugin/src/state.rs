//! The blitter's state and lookup tables.
//!
//! The C plugin keeps everything in file-scope statics (`plugins/BitBltPlugin/
//! src/common/BitBltPlugin.c`). This port gathers those statics into one
//! [`BitBlt`] struct so the engine can be driven from unit tests without a VM,
//! and keeps the C's names (`bbW`, `destPPW`, `cmFlags`, ...) so the two
//! sources can be read side by side.
//!
//! Field types mirror the C declarations: what the C declares `int` is `i32`
//! here, `sqInt`/`usqInt` are `isize`/`usize`, and 32-bit pixel words are
//! `u32`. That matters: the C relies on `int` truncation and unsigned 32-bit
//! wraparound in several places, and a wider type would change results.
//!
//! # Trust model
//!
//! Like the C plugin, the engine reads and writes raw memory through byte
//! addresses (`sourceBits`, `destBits`, `halftoneBase`, `cmLookupTable`).
//! Those addresses come from `loadBitBltFrom:warping:`, which validates the
//! image-side `Form` geometry (bitmap size >= pitch * height) exactly as the
//! C does, or from unit tests that point them at Rust-owned buffers. There is
//! no per-access bounds check beyond the C's own `assert`s, which become
//! `debug_assert!`s here.

use pharo_vm_plugin::sqInt;

/// `usqInt` in the C: the unsigned pointer-sized integer.
pub type UsqInt = usize;

// --- Constants (the C's #defines) --------------------------------------

pub const ALL_ONES: u32 = 0xFFFF_FFFF;
pub const ALPHA_INDEX: usize = 3;
pub const BB_CLIP_HEIGHT_INDEX: sqInt = 13;
pub const BB_CLIP_WIDTH_INDEX: sqInt = 12;
pub const BB_CLIP_X_INDEX: sqInt = 10;
pub const BB_CLIP_Y_INDEX: sqInt = 11;
pub const BB_COLOR_MAP_INDEX: sqInt = 14;
pub const BB_DEST_FORM_INDEX: sqInt = 0;
pub const BB_DEST_X_INDEX: sqInt = 4;
pub const BB_DEST_Y_INDEX: sqInt = 5;
pub const BB_HALFTONE_FORM_INDEX: sqInt = 2;
pub const BB_HEIGHT_INDEX: sqInt = 7;
pub const BB_RULE_INDEX: sqInt = 3;
pub const BB_SOURCE_FORM_INDEX: sqInt = 1;
pub const BB_SOURCE_X_INDEX: sqInt = 8;
pub const BB_SOURCE_Y_INDEX: sqInt = 9;
pub const BB_WARP_BASE: sqInt = 15;
pub const BB_WIDTH_INDEX: sqInt = 6;
pub const BE_BIT_BLT_INDEX: sqInt = 2;
pub const BINARY_POINT: sqInt = 14;
pub const BLUE_INDEX: usize = 2;
pub const COLOR_MAP_FIXED_PART: sqInt = 2;
pub const COLOR_MAP_INDEXED_PART: sqInt = 4;
pub const COLOR_MAP_NEW_STYLE: sqInt = 8;
pub const COLOR_MAP_PRESENT: sqInt = 1;
pub const FIXED_PT1: sqInt = 0x4000;
pub const FORM_BITS_INDEX: sqInt = 0;
pub const FORM_DEPTH_INDEX: sqInt = 3;
pub const FORM_HEIGHT_INDEX: sqInt = 2;
pub const FORM_WIDTH_INDEX: sqInt = 1;
pub const GREEN_INDEX: usize = 1;
pub const OP_TABLE_SIZE: sqInt = 43;
pub const RED_INDEX: usize = 0;

/// `maskTable` in the C: mask for the low `n` bits, defined only for the
/// depths BitBlt supports (other entries are 0, as in the C; entry 32 is the
/// C's `-1` read as unsigned).
pub const MASK_TABLE: [u32; 33] = [
    0, 1, 3, 0, 15, 31, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 65535, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFFFF_FFFF,
];

pub const DITHER_MATRIX_4X4: [i32; 16] = [
    0, 8, 2, 10, //
    12, 4, 14, 6, //
    3, 11, 1, 9, //
    15, 7, 13, 5,
];

pub const DITHER_THRESHOLDS_16: [i32; 8] = [0, 2, 4, 6, 8, 12, 14, 16];

pub const DITHER_VALUES_16: [i32; 32] = [
    0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, //
    15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
];

/// `expensiveDither32To16:threshold:` -- the C's slow ditherer, kept because
/// it defines `DITHER8_LOOKUP` and because `initDither8Lookup` calls it with
/// `srcWord` = one byte (so only the low component contributes).
pub const fn expensive_dither32_to16_threshold(src_word: u32, dither_value: i32) -> u32 {
    let mut pv = src_word & 0xFF;
    let mut threshold = DITHER_THRESHOLDS_16[(pv & 7) as usize];
    let mut value = DITHER_VALUES_16[(pv >> 3) as usize];
    let mut out: i32 = if dither_value < threshold { value + 1 } else { value };
    pv = (src_word >> 8) & 0xFF;
    threshold = DITHER_THRESHOLDS_16[(pv & 7) as usize];
    value = DITHER_VALUES_16[(pv >> 3) as usize];
    if dither_value < threshold {
        out |= (value + 1) << 5;
    } else {
        out |= value << 5;
    }
    pv = (src_word >> 16) & 0xFF;
    threshold = DITHER_THRESHOLDS_16[(pv & 7) as usize];
    value = DITHER_VALUES_16[(pv >> 3) as usize];
    if dither_value < threshold {
        out |= (value + 1) << 10;
    } else {
        out |= value << 10;
    }
    out as u32
}

/// `dither8Lookup`, indexed `(threshold << 8) + byte`.
///
/// The C fills this at `initialiseModule` time; being pure, it is a
/// compile-time constant here.
pub const DITHER8_LOOKUP: [u8; 4096] = {
    let mut table = [0u8; 4096];
    let mut b: usize = 0;
    while b <= 0xFF {
        let mut t: usize = 0;
        while t <= 15 {
            // As in the C: the whole triple computation runs, but with a
            // one-byte input only the low component is non-zero.
            let value = expensive_dither32_to16_threshold(b as u32, t as i32);
            table[(t << 8) + b] = value as u8;
            t += 1;
        }
        b += 1;
    }
    table
};

/// `default8To32Table` / the `theTable` statics: the default translation from
/// 8-bit indexed colors to 32-bit ARGB.
pub const DEFAULT_8_TO_32_TABLE: [u32; 256] = [
    0x0, 0xFF000001, 0xFFFFFFFF, 0xFF808080, 0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFF00FFFF,
    0xFFFFFF00, 0xFFFF00FF, 0xFF202020, 0xFF404040, 0xFF606060, 0xFF9F9F9F, 0xFFBFBFBF, 0xFFDFDFDF,
    0xFF080808, 0xFF101010, 0xFF181818, 0xFF282828, 0xFF303030, 0xFF383838, 0xFF484848, 0xFF505050,
    0xFF585858, 0xFF686868, 0xFF707070, 0xFF787878, 0xFF878787, 0xFF8F8F8F, 0xFF979797, 0xFFA7A7A7,
    0xFFAFAFAF, 0xFFB7B7B7, 0xFFC7C7C7, 0xFFCFCFCF, 0xFFD7D7D7, 0xFFE7E7E7, 0xFFEFEFEF, 0xFFF7F7F7,
    0xFF000001, 0xFF003300, 0xFF006600, 0xFF009900, 0xFF00CC00, 0xFF00FF00, 0xFF000033, 0xFF003333,
    0xFF006633, 0xFF009933, 0xFF00CC33, 0xFF00FF33, 0xFF000066, 0xFF003366, 0xFF006666, 0xFF009966,
    0xFF00CC66, 0xFF00FF66, 0xFF000099, 0xFF003399, 0xFF006699, 0xFF009999, 0xFF00CC99, 0xFF00FF99,
    0xFF0000CC, 0xFF0033CC, 0xFF0066CC, 0xFF0099CC, 0xFF00CCCC, 0xFF00FFCC, 0xFF0000FF, 0xFF0033FF,
    0xFF0066FF, 0xFF0099FF, 0xFF00CCFF, 0xFF00FFFF, 0xFF330000, 0xFF333300, 0xFF336600, 0xFF339900,
    0xFF33CC00, 0xFF33FF00, 0xFF330033, 0xFF333333, 0xFF336633, 0xFF339933, 0xFF33CC33, 0xFF33FF33,
    0xFF330066, 0xFF333366, 0xFF336666, 0xFF339966, 0xFF33CC66, 0xFF33FF66, 0xFF330099, 0xFF333399,
    0xFF336699, 0xFF339999, 0xFF33CC99, 0xFF33FF99, 0xFF3300CC, 0xFF3333CC, 0xFF3366CC, 0xFF3399CC,
    0xFF33CCCC, 0xFF33FFCC, 0xFF3300FF, 0xFF3333FF, 0xFF3366FF, 0xFF3399FF, 0xFF33CCFF, 0xFF33FFFF,
    0xFF660000, 0xFF663300, 0xFF666600, 0xFF669900, 0xFF66CC00, 0xFF66FF00, 0xFF660033, 0xFF663333,
    0xFF666633, 0xFF669933, 0xFF66CC33, 0xFF66FF33, 0xFF660066, 0xFF663366, 0xFF666666, 0xFF669966,
    0xFF66CC66, 0xFF66FF66, 0xFF660099, 0xFF663399, 0xFF666699, 0xFF669999, 0xFF66CC99, 0xFF66FF99,
    0xFF6600CC, 0xFF6633CC, 0xFF6666CC, 0xFF6699CC, 0xFF66CCCC, 0xFF66FFCC, 0xFF6600FF, 0xFF6633FF,
    0xFF6666FF, 0xFF6699FF, 0xFF66CCFF, 0xFF66FFFF, 0xFF990000, 0xFF993300, 0xFF996600, 0xFF999900,
    0xFF99CC00, 0xFF99FF00, 0xFF990033, 0xFF993333, 0xFF996633, 0xFF999933, 0xFF99CC33, 0xFF99FF33,
    0xFF990066, 0xFF993366, 0xFF996666, 0xFF999966, 0xFF99CC66, 0xFF99FF66, 0xFF990099, 0xFF993399,
    0xFF996699, 0xFF999999, 0xFF99CC99, 0xFF99FF99, 0xFF9900CC, 0xFF9933CC, 0xFF9966CC, 0xFF9999CC,
    0xFF99CCCC, 0xFF99FFCC, 0xFF9900FF, 0xFF9933FF, 0xFF9966FF, 0xFF9999FF, 0xFF99CCFF, 0xFF99FFFF,
    0xFFCC0000, 0xFFCC3300, 0xFFCC6600, 0xFFCC9900, 0xFFCCCC00, 0xFFCCFF00, 0xFFCC0033, 0xFFCC3333,
    0xFFCC6633, 0xFFCC9933, 0xFFCCCC33, 0xFFCCFF33, 0xFFCC0066, 0xFFCC3366, 0xFFCC6666, 0xFFCC9966,
    0xFFCCCC66, 0xFFCCFF66, 0xFFCC0099, 0xFFCC3399, 0xFFCC6699, 0xFFCC9999, 0xFFCCCC99, 0xFFCCFF99,
    0xFFCC00CC, 0xFFCC33CC, 0xFFCC66CC, 0xFFCC99CC, 0xFFCCCCCC, 0xFFCCFFCC, 0xFFCC00FF, 0xFFCC33FF,
    0xFFCC66FF, 0xFFCC99FF, 0xFFCCCCFF, 0xFFCCFFFF, 0xFFFF0000, 0xFFFF3300, 0xFFFF6600, 0xFFFF9900,
    0xFFFFCC00, 0xFFFFFF00, 0xFFFF0033, 0xFFFF3333, 0xFFFF6633, 0xFFFF9933, 0xFFFFCC33, 0xFFFFFF33,
    0xFFFF0066, 0xFFFF3366, 0xFFFF6666, 0xFFFF9966, 0xFFFFCC66, 0xFFFFFF66, 0xFFFF0099, 0xFFFF3399,
    0xFFFF6699, 0xFFFF9999, 0xFFFFCC99, 0xFFFFFF99, 0xFFFF00CC, 0xFFFF33CC, 0xFFFF66CC, 0xFFFF99CC,
    0xFFFFCCCC, 0xFFFFFFCC, 0xFFFF00FF, 0xFFFF33FF, 0xFFFF66FF, 0xFFFF99FF, 0xFFFFCCFF, 0xFFFFFFFF,
];

// --- Raw memory access (memoryAccess.h) ---------------------------------

/// `long32At`: reads the native-endian 32-bit word at a byte address.
///
/// # Safety
///
/// `addr` must be within a live buffer, which `loadBitBltFrom:warping:`'s
/// size checks (or the test harness) guarantee.
#[inline(always)]
pub unsafe fn long32At(addr: usize) -> u32 {
    unsafe { (addr as *const u32).read_unaligned() }
}

/// `long32Atput`: writes the native-endian 32-bit word at a byte address.
///
/// # Safety
///
/// As [`long32At`], plus the buffer must be writable.
#[inline(always)]
pub unsafe fn long32Atput(addr: usize, value: u32) {
    unsafe { (addr as *mut u32).write_unaligned(value) }
}

/// `byteAtPointer`: reads the byte at a byte address.
///
/// # Safety
///
/// As [`long32At`].
#[inline(always)]
pub unsafe fn byteAtPointer(addr: usize) -> u8 {
    unsafe { (addr as *const u8).read() }
}

// --- Shift helpers -------------------------------------------------------
//
// C shifts by >= the operand width are undefined behaviour; the generated
// code stays within range on the inputs the image can produce, but a
// corrupted BitBlt object could push a shift count out of range. These
// helpers pin that down: an out-of-range count yields 0, which is also what
// the C's 64-bit `(usqInt)x << n` followed by truncation to 32 bits gives
// for n in 32..64. This is the port's one systematic UB-removal (see the
// README).

/// `x << n` on a 32-bit word, 0 when the count is out of range.
#[inline(always)]
pub fn shl32(value: u32, n: sqInt) -> u32 {
    if (0..32).contains(&n) {
        value << n
    } else {
        0
    }
}

/// `x >> n` (logical) on a 32-bit word, 0 when the count is out of range.
#[inline(always)]
pub fn shr32(value: u32, n: sqInt) -> u32 {
    if (0..32).contains(&n) {
        value >> n
    } else {
        0
    }
}

/// The C's `(shift < 0) ? x >> -shift : x << shift` idiom.
#[inline(always)]
pub fn shift32(value: u32, n: sqInt) -> u32 {
    if n < 0 {
        shr32(value, -n)
    } else {
        shl32(value, n)
    }
}

/// `sqInt`-width shift with the same out-of-range policy, for the spots the
/// C computes in `usqInt` before truncating.
#[inline(always)]
pub fn shift_sq(value: sqInt, n: sqInt) -> sqInt {
    const BITS: sqInt = UsqInt::BITS as sqInt;
    if n < 0 {
        if -n >= BITS {
            0
        } else {
            ((value as UsqInt) >> -n) as sqInt
        }
    } else if n >= BITS {
        0
    } else {
        ((value as UsqInt) << n) as sqInt
    }
}

// --- Color-map table indirection ----------------------------------------

/// Where `cmShiftTable` / `cmMaskTable` point.
///
/// The C uses bare pointers that are either null, aimed into an image-side
/// words object (new-style color map), or aimed at function-local statics
/// filled by `setupColorMasksFrom:to:`. The "local statics" live inside
/// [`BitBlt`] here (`cmLocalShifts` / `cmLocalMasks`), so this enum replaces
/// the self-referential pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmTable {
    /// The C's null pointer.
    Null,
    /// Pointer into image memory (a 4-slot words object), stored as address.
    Image(usize),
    /// The `setupColorMasksFrom:to:` static arrays, held in the state.
    Local,
}

/// The surface-plugin entry points, resolved through `ioLoadFunctionFrom`
/// exactly as the C does (see `loadSurfacePlugin`). Signatures from
/// `SurfacePlugin.c`.
pub type QuerySurfaceFn =
    unsafe extern "C" fn(isize, *mut i32, *mut i32, *mut i32, *mut i32) -> i32;
pub type LockSurfaceFn = unsafe extern "C" fn(isize, *mut i32, i32, i32, i32, i32) -> isize;
pub type UnlockSurfaceFn = unsafe extern "C" fn(isize, i32, i32, i32, i32) -> i32;

/// Every file-scope static of the C plugin, under its C name.
///
/// One instance lives behind a `Mutex` in `lib.rs`; the VM only ever calls
/// primitives from its interpreter thread, so the lock is uncontended and
/// exists to make the global state sound Rust rather than to arbitrate.
#[derive(Debug)]
pub struct BitBlt {
    pub affectedB: sqInt,
    pub affectedL: sqInt,
    pub affectedR: sqInt,
    pub affectedT: sqInt,
    pub bbH: i32,
    pub bbW: i32,
    pub bitBltIsReceiver: bool,
    pub bitBltOop: sqInt,
    pub bitCount: sqInt,
    pub clipHeight: sqInt,
    pub clipWidth: sqInt,
    pub clipX: sqInt,
    pub clipY: sqInt,
    pub cmBitsPerColor: sqInt,
    pub cmFlags: sqInt,
    /// Address of the lookup words (image memory), 0 for the C's null.
    pub cmLookupTable: usize,
    pub cmMask: sqInt,
    pub cmMaskTable: CmTable,
    pub cmShiftTable: CmTable,
    /// Backing store for `CmTable::Local` (the C's function statics in
    /// `setupColorMasksFrom:to:`).
    pub cmLocalMasks: [u32; 4],
    pub cmLocalShifts: [i32; 4],
    pub combinationRule: sqInt,
    pub componentAlphaModeAlpha: sqInt,
    pub componentAlphaModeColor: sqInt,
    /// Byte address of the destination bitmap (0 = OS surface not locked yet).
    pub destBits: usize,
    pub destDelta: sqInt,
    pub destDepth: i32,
    pub destForm: sqInt,
    pub destHeight: i32,
    pub destIndex: UsqInt,
    pub destMask: u32,
    /// Kept as `i32` (not `bool`) because `querySurfaceFn` writes it through
    /// an `int *`.
    pub destMSB: i32,
    pub destPitch: i32,
    pub destPPW: sqInt,
    pub destWidth: i32,
    pub destX: sqInt,
    pub destY: sqInt,
    pub dstBitShift: sqInt,
    pub dx: i32,
    pub dy: i32,
    pub endOfDestination: UsqInt,
    pub endOfSource: UsqInt,
    /// Address of the rule-41 gamma table bytes, 0 for null.
    pub gammaLookupTable: usize,
    pub halftoneBase: usize,
    pub halftoneForm: sqInt,
    pub halftoneHeight: sqInt,
    pub hasSurfaceLock: bool,
    pub hDir: sqInt,
    pub height: sqInt,
    pub isWarping: bool,
    pub lockSurfaceFn: Option<LockSurfaceFn>,
    pub mask1: u32,
    pub mask2: u32,
    pub noHalftone: bool,
    pub noSource: bool,
    pub numGCsOnInvocation: sqInt,
    pub nWords: sqInt,
    pub preload: bool,
    pub querySurfaceFn: Option<QuerySurfaceFn>,
    pub skew: sqInt,
    pub sourceAlpha: sqInt,
    /// Byte address of the source bitmap (0 = OS surface not locked yet).
    pub sourceBits: usize,
    pub sourceDelta: sqInt,
    pub sourceDepth: i32,
    pub sourceForm: sqInt,
    pub sourceHeight: i32,
    pub sourceIndex: UsqInt,
    pub sourceMSB: i32,
    pub sourcePitch: i32,
    pub sourcePPW: sqInt,
    pub sourceWidth: i32,
    pub sourceX: sqInt,
    pub sourceY: sqInt,
    pub srcBitShift: sqInt,
    pub sx: i32,
    pub sy: i32,
    pub ungammaLookupTable: usize,
    pub unlockSurfaceFn: Option<UnlockSurfaceFn>,
    pub vDir: sqInt,
    pub warpAlignMask: sqInt,
    pub warpAlignShift: sqInt,
    pub warpBitShiftTable: [i32; 32],
    pub warpSrcMask: sqInt,
    pub warpSrcShift: sqInt,
    pub width: sqInt,
}

impl BitBlt {
    /// All-zero state, matching the C's zero-initialised statics.
    pub const fn new() -> Self {
        Self {
            affectedB: 0,
            affectedL: 0,
            affectedR: 0,
            affectedT: 0,
            bbH: 0,
            bbW: 0,
            bitBltIsReceiver: false,
            bitBltOop: 0,
            bitCount: 0,
            clipHeight: 0,
            clipWidth: 0,
            clipX: 0,
            clipY: 0,
            cmBitsPerColor: 0,
            cmFlags: 0,
            cmLookupTable: 0,
            cmMask: 0,
            cmMaskTable: CmTable::Null,
            cmShiftTable: CmTable::Null,
            cmLocalMasks: [0; 4],
            cmLocalShifts: [0; 4],
            combinationRule: 0,
            componentAlphaModeAlpha: 0,
            componentAlphaModeColor: 0,
            destBits: 0,
            destDelta: 0,
            destDepth: 0,
            destForm: 0,
            destHeight: 0,
            destIndex: 0,
            destMask: 0,
            destMSB: 0,
            destPitch: 0,
            destPPW: 0,
            destWidth: 0,
            destX: 0,
            destY: 0,
            dstBitShift: 0,
            dx: 0,
            dy: 0,
            endOfDestination: 0,
            endOfSource: 0,
            gammaLookupTable: 0,
            halftoneBase: 0,
            halftoneForm: 0,
            halftoneHeight: 0,
            hasSurfaceLock: false,
            hDir: 0,
            height: 0,
            isWarping: false,
            lockSurfaceFn: None,
            mask1: 0,
            mask2: 0,
            noHalftone: false,
            noSource: false,
            numGCsOnInvocation: 0,
            nWords: 0,
            preload: false,
            querySurfaceFn: None,
            skew: 0,
            sourceAlpha: 0,
            sourceBits: 0,
            sourceDelta: 0,
            sourceDepth: 0,
            sourceForm: 0,
            sourceHeight: 0,
            sourceIndex: 0,
            sourceMSB: 0,
            sourcePitch: 0,
            sourcePPW: 0,
            sourceWidth: 0,
            sourceX: 0,
            sourceY: 0,
            srcBitShift: 0,
            sx: 0,
            sy: 0,
            ungammaLookupTable: 0,
            unlockSurfaceFn: None,
            vDir: 0,
            warpAlignMask: 0,
            warpAlignShift: 0,
            warpBitShiftTable: [0; 32],
            warpSrcMask: 0,
            warpSrcShift: 0,
            width: 0,
        }
    }

    /// `cmShiftTable[i]` through the [`CmTable`] indirection.
    ///
    /// Callers guard with [`BitBlt::hasCmShiftTable`], as the C guards
    /// against null.
    #[inline]
    pub fn cmShiftAt(&self, i: usize) -> i32 {
        match self.cmShiftTable {
            // SAFETY: `Image` addresses come from a 4-slot words object
            // validated in loadColorMapShiftOrMaskFrom, and i is 0..4.
            CmTable::Image(addr) => unsafe { long32At(addr + 4 * i) as i32 },
            CmTable::Local => self.cmLocalShifts[i],
            CmTable::Null => 0,
        }
    }

    /// `cmMaskTable[i]` through the [`CmTable`] indirection.
    #[inline]
    pub fn cmMaskAt(&self, i: usize) -> u32 {
        match self.cmMaskTable {
            // SAFETY: as cmShiftAt.
            CmTable::Image(addr) => unsafe { long32At(addr + 4 * i) },
            CmTable::Local => self.cmLocalMasks[i],
            CmTable::Null => 0,
        }
    }

    /// `cmLookupTable[index & cmMask]`.
    ///
    /// # Safety
    ///
    /// `cmLookupTable` must point at at least `cmMask + 1` words, which
    /// `loadColorMap` establishes (table size is a power of two and
    /// `cmMask = size - 1`).
    #[inline]
    pub unsafe fn cmLookupAt(&self, index: sqInt) -> u32 {
        unsafe { long32At(self.cmLookupTable + 4 * ((index & self.cmMask) as usize)) }
    }

    /// `cmLookupTable[index & cmMask] := value` (used by the tally rules).
    ///
    /// # Safety
    ///
    /// As [`BitBlt::cmLookupAt`], plus the table must be writable.
    #[inline]
    pub unsafe fn cmLookupAtPut(&mut self, index: sqInt, value: u32) {
        unsafe { long32Atput(self.cmLookupTable + 4 * ((index & self.cmMask) as usize), value) }
    }
}

impl Default for BitBlt {
    fn default() -> Self {
        Self::new()
    }
}
