//! The engine's state and the work-buffer plumbing.
//!
//! The C plugin keeps its whole state in file-level statics: pointers into the
//! image-side work buffer (`workBuffer`, `objBuffer`, `getBuffer`,
//! `aetBuffer`), a pointer into the span `Bitmap`, plus `objUsed` and
//! `engineStopped`. All of it is (re)derived from the engine oop at the start
//! of every primitive, so this port gathers it into an [`Engine`] value built
//! per primitive call instead — same lifetime, no hidden globals.
//!
//! `objBuffer`/`getBuffer`/`aetBuffer` are *moving* pointers in the C
//! (`allocateGETEntry` does `aetBuffer += nSlots`). Here they are word
//! offsets into the work buffer, which keeps every access bounds-checked:
//! where the C would silently read or write outside the `Bitmap` on corrupt
//! input, this port panics, which the primitive wrapper turns into a clean
//! primitive failure (see the crate README).
//!
//! Function names follow the C (`aaFirstPixelFromto`, `objat`, ...) so the
//! two sources read side by side. Where the C's Slang generator inlined a
//! helper's body at the call site (marked `/* begin foo */`), this port calls
//! the helper — the bodies are identical.

#![allow(non_snake_case)]

use crate::consts::*;

/// The VM's `sqInt`: pointer-sized signed, matching the C build.
pub type SqInt = isize;

/// Services the engine core needs from its surroundings.
///
/// These are the only places the rasterizer touches anything outside the work
/// and span buffers. The VM-backed implementation lives in `lib.rs`; tests
/// provide their own.
pub trait Host {
    /// The oop half of `BalloonEnginePlugin>>#loadBitsFrom:`: answers the
    /// first indexable field and slot size of `(formArray at: xIndex+1) bits`,
    /// or `None` when `xIndex > slotSizeOf(formArray)` (the C's exact check,
    /// `>` and all). The pointer must stay valid for the primitive's duration.
    fn bits_of_form(&mut self, x_index: SqInt) -> Option<(*const i32, SqInt)>;

    /// `BalloonEngineBase>>#copyBitsFrom:to:at:` — the BitBlt hook fetched via
    /// `ioLoadFunctionFrom`. On a missing function the C tries to re-load the
    /// module and silently does nothing if that fails; implementations mirror
    /// that.
    fn copyBitsFromtoat(&mut self, x0: SqInt, x1: SqInt, y_value: SqInt);

    /// `ioMicroMSecs` for the profiling counters.
    fn ioMicroMSecs(&mut self) -> SqInt;
}

/// A do-nothing host for paths that cannot reach any host service.
pub struct NullHost;

impl Host for NullHost {
    fn bits_of_form(&mut self, _x_index: SqInt) -> Option<(*const i32, SqInt)> {
        None
    }
    fn copyBitsFromtoat(&mut self, _x0: SqInt, _x1: SqInt, _y_value: SqInt) {}
    fn ioMicroMSecs(&mut self) -> SqInt {
        0
    }
}

/// The engine state the C keeps in statics, plus the buffer views.
pub struct Engine<H> {
    pub host: H,
    /// `workBuffer`: first indexable field of the work-buffer Bitmap.
    wb: *mut i32,
    /// Slot count of that Bitmap; every access is checked against it.
    wb_len: usize,
    /// `spanBuffer`: first indexable field of the span Bitmap (unsigned).
    span: *mut u32,
    span_len: usize,
    /// `objBuffer` as a word offset into the work buffer.
    pub obj: usize,
    /// `getBuffer` as a word offset into the work buffer (moves on allocate).
    pub get: usize,
    /// `aetBuffer` as a word offset into the work buffer (moves on allocate).
    pub aet: usize,
    /// The C static `objUsed`, cached from the header on engine load and
    /// stored back by `storeEngineStateInto:`.
    pub objUsed: SqInt,
    /// The C static `engineStopped`.
    pub engineStopped: bool,
    /// The C static `doProfileStats` (copied from the plugin global).
    pub doProfileStats: bool,
    /// The C static `geProfileTime` (only ever used within one call).
    pub geProfileTime: SqInt,
}

impl<H: Host> Engine<H> {
    /// An engine with no buffers attached. Any buffer access before a
    /// successful load panics (in C it would chase a stale static pointer).
    pub fn new(host: H) -> Self {
        Engine {
            host,
            wb: core::ptr::null_mut(),
            wb_len: 0,
            span: core::ptr::null_mut(),
            span_len: 0,
            obj: 0,
            get: 0,
            aet: 0,
            objUsed: 0,
            engineStopped: false,
            doProfileStats: false,
            geProfileTime: 0,
        }
    }

    /// Points the engine at a work buffer (`workBufferPut:` in the C).
    ///
    /// # Safety
    ///
    /// `ptr` must point at `len` readable+writable `i32` words that stay valid
    /// (and un-moved) for as long as this engine is used.
    pub unsafe fn set_work_buffer(&mut self, ptr: *mut i32, len: usize) {
        self.wb = ptr;
        self.wb_len = len;
    }

    /// Points the engine at a span buffer.
    ///
    /// # Safety
    ///
    /// Same contract as [`Engine::set_work_buffer`], for `u32` words.
    pub unsafe fn set_span_buffer(&mut self, ptr: *mut u32, len: usize) {
        self.span = ptr;
        self.span_len = len;
    }

    // --- raw cells ----------------------------------------------------------

    #[inline]
    fn wb_cell(&self, i: SqInt) -> *mut i32 {
        let u = usize::try_from(i).expect("work buffer index negative");
        assert!(u < self.wb_len, "work buffer index out of range");
        // SAFETY: bounds just checked against the attached buffer.
        unsafe { self.wb.add(u) }
    }

    /// `workBuffer[i]`, sign-extended to `sqInt` as in the C.
    #[inline]
    pub fn wb_at(&self, i: SqInt) -> SqInt {
        // SAFETY: wb_cell checked the bounds.
        unsafe { *self.wb_cell(i) as SqInt }
    }

    /// `workBuffer[i]` as the raw 32-bit cell.
    #[inline]
    pub fn wb_i32(&self, i: SqInt) -> i32 {
        // SAFETY: wb_cell checked the bounds.
        unsafe { *self.wb_cell(i) }
    }

    /// `workBuffer[i] = v`, truncating to 32 bits as the C store does.
    #[inline]
    pub fn wb_put(&mut self, i: SqInt, v: SqInt) {
        // SAFETY: wb_cell checked the bounds.
        unsafe { *self.wb_cell(i) = v as i32 }
    }

    /// A header cell holding an IEEE float (edge/color transforms).
    #[inline]
    pub fn wb_f32(&self, i: SqInt) -> f32 {
        f32::from_bits(self.wb_i32(i) as u32)
    }

    #[inline]
    pub fn wb_f32_put(&mut self, i: SqInt, v: f32) {
        // SAFETY: wb_cell checked the bounds.
        unsafe { *self.wb_cell(i) = v.to_bits() as i32 }
    }

    /// `objBuffer[obj + index]` (`obj:at:` in the C).
    #[inline]
    pub fn objat(&self, obj: SqInt, index: SqInt) -> SqInt {
        self.wb_at(self.obj as SqInt + obj + index)
    }

    /// `objBuffer[obj + index] = value` (`obj:at:put:`).
    #[inline]
    pub fn objatput(&mut self, obj: SqInt, index: SqInt, value: SqInt) {
        self.wb_put(self.obj as SqInt + obj + index, value);
    }

    /// `getBuffer[i]`.
    #[inline]
    pub fn get_at(&self, i: SqInt) -> SqInt {
        self.wb_at(self.get as SqInt + i)
    }

    #[inline]
    pub fn get_put(&mut self, i: SqInt, v: SqInt) {
        self.wb_put(self.get as SqInt + i, v);
    }

    /// `aetBuffer[i]`.
    #[inline]
    pub fn aet_at(&self, i: SqInt) -> SqInt {
        self.wb_at(self.aet as SqInt + i)
    }

    #[inline]
    pub fn aet_put(&mut self, i: SqInt, v: SqInt) {
        self.wb_put(self.aet as SqInt + i, v);
    }

    #[inline]
    fn span_cell(&self, i: SqInt) -> *mut u32 {
        let u = usize::try_from(i).expect("span buffer index negative");
        assert!(u < self.span_len, "span buffer index out of range");
        // SAFETY: bounds just checked against the attached buffer.
        unsafe { self.span.add(u) }
    }

    /// `spanBuffer[i]` (unsigned, as in the C).
    #[inline]
    pub fn span_at(&self, i: SqInt) -> u32 {
        // SAFETY: span_cell checked the bounds.
        unsafe { *self.span_cell(i) }
    }

    #[inline]
    pub fn span_put(&mut self, i: SqInt, v: u32) {
        // SAFETY: span_cell checked the bounds.
        unsafe { *self.span_cell(i) = v }
    }

    /// A point slot in the header: `((int*)(workBuffer + idx))[0/1]`.
    #[inline]
    pub fn point_x(&self, idx: SqInt) -> i32 {
        self.wb_i32(idx)
    }

    #[inline]
    pub fn point_y(&self, idx: SqInt) -> i32 {
        self.wb_i32(idx + 1)
    }

    #[inline]
    pub fn point_put(&mut self, idx: SqInt, x: i32, y: i32) {
        self.wb_put(idx, x as SqInt);
        self.wb_put(idx + 1, y as SqInt);
    }

    // --- work buffer stack (grows downward from GWSize) ---------------------

    /// `wbStackClear`.
    pub fn wbStackClear(&mut self) {
        let size = self.wb_at(GWSize);
        self.wb_put(GWBufferTop, size);
    }

    /// `wbStackPop:`.
    pub fn wbStackPop(&mut self, n_items: SqInt) {
        let top = self.wb_at(GWBufferTop);
        self.wb_put(GWBufferTop, top + n_items);
    }

    /// `wbStackPush:` — answers false (and stops the engine) on overflow.
    pub fn wbStackPush(&mut self, n_items: SqInt) -> bool {
        if !self.needAvailableSpace(n_items) {
            return false;
        }
        let top = self.wb_at(GWBufferTop);
        self.wb_put(GWBufferTop, top - n_items);
        true
    }

    /// `wbStackSize`.
    #[inline]
    pub fn wbStackSize(&self) -> SqInt {
        self.wb_at(GWSize) - self.wb_at(GWBufferTop)
    }

    /// `wbStackValue:`.
    #[inline]
    pub fn wbStackValue(&self, index: SqInt) -> SqInt {
        self.wb_at(self.wb_at(GWBufferTop) + index)
    }

    /// `wbStackValue:put:`.
    #[inline]
    pub fn wbStackValueput(&mut self, index: SqInt, value: SqInt) {
        let top = self.wb_at(GWBufferTop);
        self.wb_put(top + index, value);
    }

    // --- fill stack entries (3 words each, on the wb stack) -----------------

    #[inline]
    pub fn stackFillValue(&self, index: SqInt) -> SqInt {
        self.wbStackValue(index)
    }

    #[inline]
    pub fn stackFillValueput(&mut self, index: SqInt, value: SqInt) {
        self.wbStackValueput(index, value);
    }

    #[inline]
    pub fn stackFillDepth(&self, index: SqInt) -> SqInt {
        self.wbStackValue(index + 1)
    }

    #[inline]
    pub fn stackFillDepthput(&mut self, index: SqInt, value: SqInt) {
        self.wbStackValueput(index + 1, value);
    }

    #[inline]
    pub fn stackFillRightX(&self, index: SqInt) -> SqInt {
        self.wbStackValue(index + 2)
    }

    #[inline]
    pub fn stackFillRightXput(&mut self, index: SqInt, value: SqInt) {
        self.wbStackValueput(index + 2, value);
    }

    /// `topFill` — zero when the fill stack is empty.
    pub fn topFill(&self) -> SqInt {
        if self.wbStackSize() == 0 {
            0
        } else {
            self.stackFillValue(self.wbStackSize() - StackFillEntryLength)
        }
    }

    /// `topDepth` — -1 when the fill stack is empty.
    pub fn topDepth(&self) -> SqInt {
        if self.wbStackSize() == 0 {
            -1
        } else {
            self.stackFillDepth(self.wbStackSize() - StackFillEntryLength)
        }
    }

    /// `topRightX` — the C's 999999999 sentinel when empty.
    pub fn topRightX(&self) -> SqInt {
        if self.wbStackSize() == 0 {
            999999999
        } else {
            self.stackFillRightX(self.wbStackSize() - StackFillEntryLength)
        }
    }

    // --- stopping -----------------------------------------------------------

    /// `stopBecauseOf:` — records the stop reason the image resumes off.
    pub fn stopBecauseOf(&mut self, stop_reason: SqInt) {
        self.wb_put(GWStopReason, stop_reason);
        self.engineStopped = true;
    }

    // --- allocation ---------------------------------------------------------

    /// `needAvailableSpace:` — false (and engine stopped) when the free gap
    /// between the tables and the downward stack cannot fit `nSlots`.
    pub fn needAvailableSpace(&mut self, n_slots: SqInt) -> bool {
        if GWHeaderSize + self.objUsed + self.wb_at(GWGETUsed) + self.wb_at(GWAETUsed) + n_slots
            > self.wb_at(GWBufferTop)
        {
            self.stopBecauseOf(GErrorNoMoreSpace);
            return false;
        }
        true
    }

    /// `allocateGETEntry:` — makes room by sliding the AET upward.
    pub fn allocateGETEntry(&mut self, n_slots: SqInt) -> bool {
        if !self.needAvailableSpace(n_slots) {
            return false;
        }
        if self.wb_at(GWAETUsed) != 0 {
            let mut src_index = self.wb_at(GWAETUsed);
            let mut dst_index = self.wb_at(GWAETUsed) + n_slots;
            for _ in 1..=self.wb_at(GWAETUsed) {
                dst_index -= 1;
                src_index -= 1;
                let v = self.aet_at(src_index);
                self.aet_put(dst_index, v);
            }
        }
        self.aet = (self.aet as SqInt + n_slots) as usize;
        true
    }

    /// `allocateObjEntry:` — makes room by sliding the GET (and AET) upward.
    pub fn allocateObjEntry(&mut self, n_slots: SqInt) -> bool {
        if !self.allocateGETEntry(n_slots) {
            return false;
        }
        if self.wb_at(GWGETUsed) != 0 {
            let mut src_index = self.wb_at(GWGETUsed);
            let mut dst_index = self.wb_at(GWGETUsed) + n_slots;
            for _ in 1..=self.wb_at(GWGETUsed) {
                dst_index -= 1;
                src_index -= 1;
                let v = self.get_at(src_index);
                self.get_put(dst_index, v);
            }
        }
        self.get = (self.get as SqInt + n_slots) as usize;
        true
    }

    /// `allocateBezier` — answers the new object's index, or 0 when stopped.
    pub fn allocateBezier(&mut self) -> SqInt {
        if !self.allocateObjEntry(GBBaseSize) {
            return 0;
        }
        let bezier = self.objUsed;
        self.objUsed = bezier + GBBaseSize;
        self.objatput(bezier, GEObjectType, GEPrimitiveBezier);
        self.objatput(bezier, GEObjectIndex, 0);
        self.objatput(bezier, GEObjectLength, GBBaseSize);
        bezier
    }

    /// `allocateWideBezier`.
    pub fn allocateWideBezier(&mut self) -> SqInt {
        if !self.allocateObjEntry(GBWideSize) {
            return 0;
        }
        let bezier = self.objUsed;
        self.objUsed = bezier + GBWideSize;
        self.objatput(bezier, GEObjectType, GEPrimitiveWideBezier);
        self.objatput(bezier, GEObjectIndex, 0);
        self.objatput(bezier, GEObjectLength, GBWideSize);
        bezier
    }

    /// `allocateLine`.
    pub fn allocateLine(&mut self) -> SqInt {
        if !self.allocateObjEntry(GLBaseSize) {
            return 0;
        }
        let line = self.objUsed;
        self.objUsed = line + GLBaseSize;
        self.objatput(line, GEObjectType, GEPrimitiveLine);
        self.objatput(line, GEObjectIndex, 0);
        self.objatput(line, GEObjectLength, GLBaseSize);
        line
    }

    /// `allocateWideLine`.
    pub fn allocateWideLine(&mut self) -> SqInt {
        if !self.allocateObjEntry(GLWideSize) {
            return 0;
        }
        let line = self.objUsed;
        self.objUsed = line + GLWideSize;
        self.objatput(line, GEObjectType, GEPrimitiveWideLine);
        self.objatput(line, GEObjectIndex, 0);
        self.objatput(line, GEObjectLength, GLWideSize);
        line
    }

    /// `allocateBezierStackEntry` — pushes 6 slots, answers the stack index
    /// (still answered even when the push failed; callers check
    /// `engineStopped`, exactly as the C does).
    pub fn allocateBezierStackEntry(&mut self) -> SqInt {
        self.wbStackPush(6);
        self.wbStackSize()
    }

    // --- object predicates --------------------------------------------------

    /// `objectTypeOf:`.
    #[inline]
    pub fn objectTypeOf(&self, obj: SqInt) -> SqInt {
        self.objat(obj, GEObjectType) & GEPrimitiveTypeMask
    }

    /// `isEdge:`.
    pub fn isEdge(&self, edge: SqInt) -> bool {
        let type_ = self.objectTypeOf(edge);
        if type_ > GEPrimitiveEdgeMask {
            return false;
        }
        (self.objectTypeOf(edge) & GEPrimitiveEdgeMask) != 0
    }

    /// `isWide:`.
    pub fn isWide(&self, object: SqInt) -> bool {
        (self.objectTypeOf(object) & GEPrimitiveWide) != 0
    }

    /// `isFillOkay:` — accepts 0, direct colors, and in-range fill objects.
    pub fn isFillOkay(&self, fill: SqInt) -> bool {
        (fill == 0)
            || ((fill & 0xFF000000u32 as SqInt) != 0)
            || (((fill >= 0) && (fill < self.objUsed))
                && (((fill & 0xFF000000u32 as SqInt) != 0)
                    || ((self.objectTypeOf(fill) & GEPrimitiveFillMask) != 0)))
    }

    /// `fillTypeOf:`.
    #[inline]
    pub fn fillTypeOf(&self, fill: SqInt) -> SqInt {
        (((self.objectTypeOf(fill) & GEPrimitiveFillMask) as usize) >> 8) as SqInt
    }

    /// `edgeTypeOf:` — the type without the wide flag.
    #[inline]
    pub fn edgeTypeOf(&self, edge: SqInt) -> SqInt {
        ((self.objectTypeOf(edge) as usize) >> 1) as SqInt
    }

    /// `edgeFillsValidate:`.
    pub fn edgeFillsValidate(&mut self, edge: SqInt) {
        let value = self.objectTypeOf(edge) & !GEEdgeFillsInvalid;
        self.objatput(edge, GEObjectType, value);
    }

    /// `edgeFillsInvalidate:`.
    pub fn edgeFillsInvalidate(&mut self, edge: SqInt) {
        let value = self.objectTypeOf(edge) | GEEdgeFillsInvalid;
        self.objatput(edge, GEObjectType, value);
    }

    // --- anti-aliasing ------------------------------------------------------

    /// `setAALevel:` — clamps to 1/2/4 and derives the shift/mask family.
    #[allow(clippy::manual_range_contains)] // keeps the C's comparison shape
    pub fn setAALevel(&mut self, level: SqInt) {
        let mut aa_level = 0;
        if level >= 4 {
            aa_level = 4;
        }
        if (level >= 2) && (level < 4) {
            aa_level = 2;
        }
        if level < 2 {
            aa_level = 1;
        }
        self.wb_put(GWAALevel, aa_level);
        if aa_level == 1 {
            self.wb_put(GWAAShift, 0);
            self.wb_put(GWAAColorMask, 0xFFFFFFFFu32 as i32 as SqInt);
            self.wb_put(GWAAScanMask, 0);
        }
        if aa_level == 2 {
            self.wb_put(GWAAShift, 1);
            self.wb_put(GWAAColorMask, 4244438268u32 as i32 as SqInt);
            self.wb_put(GWAAScanMask, 1);
        }
        if aa_level == 4 {
            self.wb_put(GWAAShift, 2);
            self.wb_put(GWAAColorMask, 4042322160u32 as i32 as SqInt);
            self.wb_put(GWAAScanMask, 3);
        }
        let shift = self.wb_at(GWAAShift);
        self.wb_put(GWAAColorShift, shift * 2);
        self.wb_put(GWAAHalfPixel, shift);
    }

    /// `aaFirstPixelFrom:to:` — first full pixel for AA drawing.
    ///
    /// The C masks with `(unsigned int)~(aaLevel - 1)`, i.e. a *32-bit*
    /// zero-extended mask; reproduced bit for bit.
    pub fn aaFirstPixelFromto(&self, left_x: SqInt, right_x: SqInt) -> SqInt {
        let mask = (!(self.wb_i32(GWAALevel).wrapping_sub(1))) as u32 as SqInt;
        let first_pixel = ((left_x + self.wb_at(GWAALevel)) - 1) & mask;
        if first_pixel > right_x {
            right_x
        } else {
            first_pixel
        }
    }

    /// `aaLastPixelFrom:to:` — last full pixel for AA drawing.
    pub fn aaLastPixelFromto(&self, _left_x: SqInt, right_x: SqInt) -> SqInt {
        let mask = (!(self.wb_i32(GWAALevel).wrapping_sub(1))) as u32 as SqInt;
        (right_x - 1) & mask
    }

    // --- statistics ---------------------------------------------------------

    /// `incrementStat:by:`.
    pub fn incrementStatby(&mut self, stat_index: SqInt, value: SqInt) {
        let v = self.wb_at(stat_index) + value;
        self.wb_put(stat_index, v);
    }

    /// `resetGraphicsEngineStats`.
    pub fn resetGraphicsEngineStats(&mut self) {
        for idx in [
            GWTimeInitializing,
            GWTimeFinishTest,
            GWTimeNextGETEntry,
            GWTimeAddAETEntry,
            GWTimeNextFillEntry,
            GWTimeMergeFill,
            GWTimeDisplaySpan,
            GWTimeNextAETEntry,
            GWTimeChangeAETEntry,
            GWCountInitializing,
            GWCountFinishTest,
            GWCountNextGETEntry,
            GWCountAddAETEntry,
            GWCountNextFillEntry,
            GWCountMergeFill,
            GWCountDisplaySpan,
            GWCountNextAETEntry,
            GWCountChangeAETEntry,
            GWBezierMonotonSubdivisions,
            GWBezierHeightSubdivisions,
            GWBezierOverflowSubdivisions,
            GWBezierLineConversions,
        ] {
            self.wb_put(idx, 0);
        }
    }

    // --- transforms ---------------------------------------------------------

    /// `initEdgeTransform` — identity, and marks the transform absent.
    pub fn initEdgeTransform(&mut self) {
        let t = GWEdgeTransform;
        self.wb_f32_put(t, 1.0);
        self.wb_f32_put(t + 1, 0.0);
        self.wb_f32_put(t + 2, 0.0);
        self.wb_f32_put(t + 3, 0.0);
        self.wb_f32_put(t + 4, 1.0);
        self.wb_f32_put(t + 5, 0.0);
        self.wb_put(GWHasEdgeTransform, 0);
    }

    /// `initColorTransform` — identity, and marks the transform absent.
    pub fn initColorTransform(&mut self) {
        let t = GWColorTransform;
        self.wb_f32_put(t, 1.0);
        self.wb_f32_put(t + 1, 0.0);
        self.wb_f32_put(t + 2, 1.0);
        self.wb_f32_put(t + 3, 0.0);
        self.wb_f32_put(t + 4, 1.0);
        self.wb_f32_put(t + 5, 0.0);
        self.wb_f32_put(t + 6, 1.0);
        self.wb_f32_put(t + 7, 0.0);
        self.wb_put(GWHasColorTransform, 0);
    }

    /// `transformPoint:` on one of the header point slots (the C always
    /// passes `workBuffer + GWPointN`).
    pub fn transformPoint(&mut self, point: SqInt) {
        if self.wb_at(GWHasEdgeTransform) != 0 {
            let x_value = self.point_x(point) as f64;
            let y_value = self.point_y(point) as f64;
            let t = GWEdgeTransform;
            let aa = self.wb_at(GWAALevel) as f64;
            let x = (((self.wb_f32(t) as f64 * x_value)
                + (self.wb_f32(t + 1) as f64 * y_value)
                + self.wb_f32(t + 2) as f64)
                * aa) as SqInt;
            let y = (((self.wb_f32(t + 3) as f64 * x_value)
                + (self.wb_f32(t + 4) as f64 * y_value)
                + self.wb_f32(t + 5) as f64)
                * aa) as SqInt;
            self.point_put(point, x as i32, y as i32);
        } else {
            // Multiply each component by aaLevel and add the dest offset.
            let x = (self.point_x(point) as SqInt + self.wb_at(GWDestOffsetX))
                .wrapping_mul(self.wb_at(GWAALevel));
            let y = (self.point_y(point) as SqInt + self.wb_at(GWDestOffsetY))
                .wrapping_mul(self.wb_at(GWAALevel));
            self.point_put(point, x as i32, y as i32);
        }
    }

    /// `transformPoints:` — transforms point1..pointN in place.
    pub fn transformPoints(&mut self, n: SqInt) {
        if n > 0 {
            self.transformPoint(GWPoint1);
        }
        if n > 1 {
            self.transformPoint(GWPoint2);
        }
        if n > 2 {
            self.transformPoint(GWPoint3);
        }
        if n > 3 {
            self.transformPoint(GWPoint4);
        }
    }

    /// `transformWidth:` — pushes a `w`-sized cross through the transform and
    /// takes the smaller resulting extent (minimum 1).
    pub fn transformWidth(&mut self, w: SqInt) -> SqInt {
        if w == 0 {
            return 0;
        }
        self.point_put(GWPoint1, 0, 0);
        self.point_put(GWPoint2, (w * 256) as i32, 0);
        self.point_put(GWPoint3, 0, (w * 256) as i32);
        self.transformPoints(3);
        let mut delta_x = (self.point_x(GWPoint2).wrapping_sub(self.point_x(GWPoint1))) as f64;
        let mut delta_y = (self.point_y(GWPoint2).wrapping_sub(self.point_y(GWPoint1))) as f64;
        let mut dst_width =
            ((((delta_x * delta_x) + (delta_y * delta_y)).sqrt() as SqInt) + 128) / 256;
        delta_x = (self.point_x(GWPoint3).wrapping_sub(self.point_x(GWPoint1))) as f64;
        delta_y = (self.point_y(GWPoint3).wrapping_sub(self.point_y(GWPoint1))) as f64;
        let dst_width2 =
            ((((delta_x * delta_x) + (delta_y * delta_y)).sqrt() as SqInt) + 128) / 256;
        if dst_width2 < dst_width {
            dst_width = dst_width2;
        }
        if dst_width == 0 {
            1
        } else {
            dst_width
        }
    }

    /// `transformColor:` — applies the color transform to a direct color.
    ///
    /// Also the place where a translucent color while `needsFlush` is set
    /// raises the `GErrorNeedFlush` stop, which the image depends on.
    pub fn transformColor(&mut self, fill_index: SqInt) -> SqInt {
        if !((fill_index == 0) || ((fill_index & 0xFF000000u32 as SqInt) != 0)) {
            // A fill *index*, not a color: answered untouched.
            return fill_index;
        }
        let mut b = fill_index & 0xFF;
        let mut g = ((fill_index as usize) >> 8) as SqInt & 0xFF;
        let mut r = ((fill_index as usize) >> 16) as SqInt & 0xFF;
        let mut a = ((fill_index as usize) >> 24) as SqInt & 0xFF;
        if self.wb_at(GWHasColorTransform) != 0 {
            let t = GWColorTransform;
            // With a == 0 the C divides by zero (double), yielding inf/NaN,
            // then hits C's undefined double->int conversion; Rust's `as`
            // saturates instead (NaN -> 0). Same inputs, defined outcome.
            let alpha_scale =
                ((a as f64 * self.wb_f32(t + 6) as f64) + self.wb_f32(t + 7) as f64) / a as f64;
            r = (((r as f64 * self.wb_f32(t) as f64) + self.wb_f32(t + 1) as f64) * alpha_scale)
                as SqInt;
            g = (((g as f64 * self.wb_f32(t + 2) as f64) + self.wb_f32(t + 3) as f64)
                * alpha_scale) as SqInt;
            b = (((b as f64 * self.wb_f32(t + 4) as f64) + self.wb_f32(t + 5) as f64)
                * alpha_scale) as SqInt;
            a = (a as f64 * alpha_scale) as SqInt;
            r = r.clamp(0, 0xFF);
            g = g.clamp(0, 0xFF);
            b = b.clamp(0, 0xFF);
            a = a.clamp(0, 0xFF);
        }
        if a < 1 {
            return 0;
        }
        if (a < 0xFF) && (self.wb_at(GWNeedsFlush) != 0) {
            self.stopBecauseOf(GErrorNeedFlush);
        }
        b + (g << 8) + (r << 16) + (a << 24)
    }

    /// `uncheckedTransformColor:` — the per-pixel variant used by fills; no
    /// flush check, and fully transparent (< 16) maps to 0.
    pub fn uncheckedTransformColor(&self, fill_index: SqInt) -> SqInt {
        if self.wb_at(GWHasColorTransform) == 0 {
            return fill_index;
        }
        let t = GWColorTransform;
        let mut b = fill_index & 0xFF;
        let mut g = ((fill_index as usize) >> 8) as SqInt & 0xFF;
        let mut r = ((fill_index as usize) >> 16) as SqInt & 0xFF;
        let mut a = ((fill_index as usize) >> 24) as SqInt & 0xFF;
        r = ((r as f64 * self.wb_f32(t) as f64) + self.wb_f32(t + 1) as f64) as SqInt;
        g = ((g as f64 * self.wb_f32(t + 2) as f64) + self.wb_f32(t + 3) as f64) as SqInt;
        b = ((b as f64 * self.wb_f32(t + 4) as f64) + self.wb_f32(t + 5) as f64) as SqInt;
        a = ((a as f64 * self.wb_f32(t + 6) as f64) + self.wb_f32(t + 7) as f64) as SqInt;
        r = r.clamp(0, 0xFF);
        g = g.clamp(0, 0xFF);
        b = b.clamp(0, 0xFF);
        a = a.clamp(0, 0xFF);
        if a < 16 {
            return 0;
        }
        b + (g << 8) + (r << 16) + (a << 24)
    }

    // --- small math ---------------------------------------------------------

    /// `computeSqrt:` — table below 32, rounded `sqrt` above.
    pub fn computeSqrt(&self, length2: SqInt) -> SqInt {
        if length2 < 32 {
            SMALL_SQRT_TABLE[length2 as usize] as SqInt
        } else {
            ((length2 as f64).sqrt() + 0.5) as SqInt
        }
    }

    /// `accurateLengthOf:with:`.
    pub fn accurateLengthOfwith(&self, delta_x: SqInt, delta_y: SqInt) -> SqInt {
        if delta_x == 0 {
            return delta_y.abs();
        }
        if delta_y == 0 {
            return delta_x.abs();
        }
        let length2 = (delta_x * delta_x) + (delta_y * delta_y);
        self.computeSqrt(length2)
    }

    /// `clampValue:max:`.
    pub fn clampValuemax(&self, value: SqInt, max_value: SqInt) -> SqInt {
        if value < 0 {
            0
        } else if value >= max_value {
            max_value - 1
        } else {
            value
        }
    }

    /// `repeatValue:max:`.
    pub fn repeatValuemax(&self, delta: SqInt, max_value: SqInt) -> SqInt {
        let mut new_delta = delta;
        while new_delta < 0 {
            new_delta += max_value;
        }
        while new_delta >= max_value {
            new_delta -= max_value;
        }
        new_delta
    }

    /// `assureValue:between:and:`.
    pub fn assureValuebetweenand(&self, val1: SqInt, val2: SqInt, val3: SqInt) -> SqInt {
        if val2 > val3 {
            if val1 > val2 {
                return val2;
            }
            if val1 < val3 {
                return val3;
            }
        } else {
            if val1 < val2 {
                return val2;
            }
            if val1 > val3 {
                return val3;
            }
        }
        val1
    }

    /// `offsetFromWidth:`.
    #[inline]
    pub fn offsetFromWidth(&self, line_width: SqInt) -> SqInt {
        line_width / 2
    }

    /// `makeRectFromPoints` — completes point2/point4 from point1/point3.
    pub fn makeRectFromPoints(&mut self) {
        let p1x = self.point_x(GWPoint1);
        let p1y = self.point_y(GWPoint1);
        let p3x = self.point_x(GWPoint3);
        let p3y = self.point_y(GWPoint3);
        self.point_put(GWPoint2, p3x, p1y);
        self.point_put(GWPoint4, p1x, p3y);
    }

    // --- buffer setup -------------------------------------------------------

    /// The content half of `loadWorkBufferFrom:` — validates the header and
    /// derives the obj/GET/AET offsets. `slot_size` is `slotSizeOf(wbOop)`.
    /// Answers 0 or a `GEF*` failure code.
    pub fn attach_work_buffer_checked(&mut self, slot_size: SqInt) -> SqInt {
        if self.wb_at(GWMagicIndex) != GWMagicNumber {
            return GEFWorkBufferBadMagic;
        }
        if self.wb_at(GWSize) != slot_size {
            return GEFWorkBufferWrongSize;
        }
        if self.wb_at(GWObjStart) != GWHeaderSize {
            return GEFWorkBufferStartWrong;
        }
        self.obj = self.wb_at(GWObjStart) as usize;
        self.get = self.obj + self.wb_at(GWObjUsed) as usize;
        self.aet = self.get + self.wb_at(GWGETUsed) as usize;
        if GWHeaderSize + self.wb_at(GWObjUsed) + self.wb_at(GWGETUsed) + self.wb_at(GWAETUsed)
            > self.wb_at(GWSize)
        {
            return GEFWorkTooBig;
        }
        0
    }

    /// The content half of `primitiveInitializeBuffer` — lays out a fresh
    /// work buffer of `size` slots (buffer already attached).
    pub fn initializeBuffer(&mut self, size: SqInt) {
        self.obj = GWHeaderSize as usize;
        self.wb_put(GWMagicIndex, GWMagicNumber);
        self.wb_put(GWSize, size);
        self.wb_put(GWBufferTop, size);
        self.wb_put(GWState, GEStateUnlocked);
        self.wb_put(GWObjStart, GWHeaderSize);
        self.wb_put(GWObjUsed, 4);
        // Object 0 is the reserved "no fill" record.
        self.objatput(0, GEObjectType, GEPrimitiveFill);
        self.objatput(0, GEObjectLength, 4);
        self.objatput(0, GEObjectIndex, 0);
        self.wb_put(GWGETStart, 0);
        self.wb_put(GWGETUsed, 0);
        self.wb_put(GWAETStart, 0);
        self.wb_put(GWAETUsed, 0);
        self.wb_put(GWStopReason, 0);
        self.wb_put(GWNeedsFlush, 0);
        self.wb_put(GWClipMinX, 0);
        self.wb_put(GWClipMaxX, 0);
        self.wb_put(GWClipMinY, 0);
        self.wb_put(GWClipMaxY, 0);
        self.wb_put(GWCurrentZ, 0);
        self.resetGraphicsEngineStats();
        self.initEdgeTransform();
        self.initColorTransform();
    }
}

/// `absoluteSquared8Dot24:` — `(value * value) >> 24` for an 8.24 fixed-point
/// value in [0, 1), computed in 32-bit halves exactly as the C does.
pub fn absoluteSquared8Dot24(value: SqInt) -> SqInt {
    let word1 = (value & 0xFFFF) as u32;
    let word2 = ((value as usize >> 16) & 0xFF) as u32;
    let sum = ((word1.wrapping_mul(word1) as usize) >> 16)
        .wrapping_add(word1.wrapping_mul(word2).wrapping_mul(2) as usize)
        .wrapping_add((word2.wrapping_mul(word2) as usize) << 16);
    (sum >> 8) as SqInt
}

/// `smallSqrtTable`.
pub const SMALL_SQRT_TABLE: [i32; 32] = [
    0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    6,
];

/// `rShiftTable` — indexed by bitmap depth.
pub const R_SHIFT_TABLE: [i32; 17] = [0, 5, 4, 0, 3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1];

/// `circleCosTable`.
#[allow(clippy::approx_constant)] // the C's literal digits are the ABI
pub const CIRCLE_COS_TABLE: [f64; 33] = [
    1.0,
    0.98078528040323,
    0.923879532511287,
    0.831469612302545,
    0.7071067811865475,
    0.555570233019602,
    0.38268343236509,
    0.1950903220161286,
    0.0,
    -0.1950903220161283,
    -0.3826834323650896,
    -0.555570233019602,
    -0.707106781186547,
    -0.831469612302545,
    -0.9238795325112865,
    -0.98078528040323,
    -1.0,
    -0.98078528040323,
    -0.923879532511287,
    -0.831469612302545,
    -0.707106781186548,
    -0.555570233019602,
    -0.3826834323650903,
    -0.1950903220161287,
    0.0,
    0.1950903220161282,
    0.38268343236509,
    0.555570233019602,
    0.707106781186547,
    0.831469612302545,
    0.9238795325112865,
    0.98078528040323,
    1.0,
];

/// `circleSinTable`.
#[allow(clippy::approx_constant)] // the C's literal digits are the ABI
pub const CIRCLE_SIN_TABLE: [f64; 33] = [
    0.0,
    0.1950903220161282,
    0.3826834323650897,
    0.555570233019602,
    0.707106781186547,
    0.831469612302545,
    0.923879532511287,
    0.98078528040323,
    1.0,
    0.98078528040323,
    0.923879532511287,
    0.831469612302545,
    0.7071067811865475,
    0.555570233019602,
    0.38268343236509,
    0.1950903220161286,
    0.0,
    -0.1950903220161283,
    -0.3826834323650896,
    -0.555570233019602,
    -0.707106781186547,
    -0.831469612302545,
    -0.9238795325112865,
    -0.98078528040323,
    -1.0,
    -0.98078528040323,
    -0.923879532511287,
    -0.831469612302545,
    -0.707106781186548,
    -0.555570233019602,
    -0.3826834323650903,
    -0.1950903220161287,
    0.0,
];
