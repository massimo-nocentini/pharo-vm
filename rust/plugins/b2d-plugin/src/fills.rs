//! Fills: the depth-sorted fill stack, span filling, gradients and bitmap
//! fills.
//!
//! The fill stack lives at the top of the work buffer and grows downward;
//! entries are three words (`fill`, `depth`, `rightX`). Span filling writes
//! 32-bit pixels into the span buffer, accumulating fractional coverage when
//! anti-aliasing is on.
//!
//! Numeric widths follow the C's local declarations exactly: the radial
//! gradients step `ds`/`dt` as 32-bit ints (wrapping), the span accumulation
//! adds a 64-bit `sqInt` into a 32-bit unsigned cell (truncating), and the AA
//! masks are the sign-/zero-extensions the C's mixed int/unsigned expressions
//! produce on a 64-bit build.

#![allow(non_snake_case)]

use crate::consts::*;
use crate::engine::{Engine, Host, SqInt, R_SHIFT_TABLE};

impl<H: Host> Engine<H> {
    // --- fill stack ---------------------------------------------------------

    /// `fillSorts:before:` — deeper first, then lower (unsigned) fill value.
    pub fn fillSortsbefore(&self, fill_entry1: SqInt, fill_entry2: SqInt) -> bool {
        let diff = self.stackFillDepth(fill_entry1) - self.stackFillDepth(fill_entry2);
        if diff != 0 {
            return diff > 0;
        }
        // Unsigned 32-bit comparison of the raw cells, as in the C.
        let top = self.wb_at(GWBufferTop);
        (self.wb_i32(top + fill_entry1) as u32) < (self.wb_i32(top + fill_entry2) as u32)
    }

    /// `findStackFill:depth:` — the entry's stack index, or -1.
    pub fn findStackFilldepth(&self, fill_index: SqInt, depth: SqInt) -> SqInt {
        let mut index = 0;
        while (index < self.wbStackSize())
            && ((self.stackFillValue(index) != fill_index)
                || (self.stackFillDepth(index) != depth))
        {
            index += StackFillEntryLength;
        }
        if index >= self.wbStackSize() {
            -1
        } else {
            index
        }
    }

    /// `freeStackFillEntry`.
    pub fn freeStackFillEntry(&mut self) {
        self.wbStackPop(StackFillEntryLength);
    }

    /// `hideFill:depth:` — answers whether the fill was found (and removed).
    pub fn hideFilldepth(&mut self, fill_index: SqInt, depth: SqInt) -> bool {
        let mut index = self.findStackFilldepth(fill_index, depth);
        if index == -1 {
            return false;
        }
        if index == 0 {
            self.freeStackFillEntry();
            return true;
        }
        // Move the most recent entry (index 0) into the hole, then pop it.
        let v = self.stackFillValue(0);
        self.stackFillValueput(index, v);
        let v = self.stackFillDepth(0);
        self.stackFillDepthput(index, v);
        let v = self.stackFillRightX(0);
        self.stackFillRightXput(index, v);
        self.freeStackFillEntry();
        if self.wbStackSize() <= StackFillEntryLength {
            return true;
        }
        // Re-establish the topmost entry: find the entry that sorts first and
        // swap it into the top slot.
        let mut new_top_index = 0;
        index = StackFillEntryLength;
        while index < self.wbStackSize() {
            if self.fillSortsbefore(index, new_top_index) {
                new_top_index = index;
            }
            index += StackFillEntryLength;
        }
        if new_top_index + StackFillEntryLength == self.wbStackSize() {
            return true;
        }
        let top_slot = self.wbStackSize() - StackFillEntryLength;
        let new_top = self.stackFillValue(new_top_index);
        let v = self.stackFillValue(top_slot);
        self.stackFillValueput(new_top_index, v);
        self.stackFillValueput(top_slot, new_top);
        let new_depth = self.stackFillDepth(new_top_index);
        let v = self.stackFillDepth(top_slot);
        self.stackFillDepthput(new_top_index, v);
        self.stackFillDepthput(top_slot, new_depth);
        let new_right_x = self.stackFillRightX(new_top_index);
        let v = self.stackFillRightX(top_slot);
        self.stackFillRightXput(new_top_index, v);
        self.stackFillRightXput(top_slot, new_right_x);
        true
    }

    /// `showFill:depth:rightX:`.
    pub fn showFilldepthrightX(&mut self, fill_index: SqInt, depth: SqInt, right_x: SqInt) {
        if !self.wbStackPush(StackFillEntryLength) {
            return;
        }
        self.stackFillValueput(0, fill_index);
        self.stackFillDepthput(0, depth);
        self.stackFillRightXput(0, right_x);
        if self.wbStackSize() == StackFillEntryLength {
            return;
        }
        let top_slot = self.wbStackSize() - StackFillEntryLength;
        if self.fillSortsbefore(0, top_slot) {
            // New top fill: swap the fresh entry with the current top slot.
            let v = self.stackFillValue(top_slot);
            self.stackFillValueput(0, v);
            let v = self.stackFillDepth(top_slot);
            self.stackFillDepthput(0, v);
            let v = self.stackFillRightX(top_slot);
            self.stackFillRightXput(0, v);
            self.stackFillValueput(top_slot, fill_index);
            self.stackFillDepthput(top_slot, depth);
            self.stackFillRightXput(top_slot, right_x);
        }
    }

    /// `toggleFill:depth:rightX:`.
    pub fn toggleFilldepthrightX(&mut self, fill_index: SqInt, depth: SqInt, right_x: SqInt) {
        if self.wbStackSize() == 0 {
            if self.wbStackPush(StackFillEntryLength) {
                let slot = self.wbStackSize() - StackFillEntryLength;
                self.stackFillValueput(slot, fill_index);
                self.stackFillDepthput(slot, depth);
                self.stackFillRightXput(slot, right_x);
            }
        } else {
            let hidden = self.hideFilldepth(fill_index, depth);
            if !hidden {
                self.showFilldepthrightX(fill_index, depth, right_x);
            }
        }
    }

    /// `toggleFillsOf:`.
    pub fn toggleFillsOf(&mut self, edge: SqInt) {
        if !self.needAvailableSpace(StackFillEntryLength * 2) {
            return;
        }
        let depth = ((self.objat(edge, GEZValue) as usize) << 1) as SqInt;
        let fill_index = self.objat(edge, GEFillIndexLeft);
        if fill_index != 0 {
            self.toggleFilldepthrightX(fill_index, depth, 999999999);
        }
        let fill_index = self.objat(edge, GEFillIndexRight);
        if fill_index != 0 {
            self.toggleFilldepthrightX(fill_index, depth, 999999999);
        }
        let x = self.objat(edge, GEXValue);
        self.quickRemoveInvalidFillsAt(x);
    }

    /// `toggleWideFillOf:`.
    pub fn toggleWideFillOf(&mut self, edge: SqInt) {
        let type_ = self.edgeTypeOf(edge);
        // Dispatch as the C's switch does. For types 0/1 (external edges) the
        // C reads a stale static `dispatchReturnValue`, which is unspecified;
        // this port uses 0 there (see README).
        let (line_width, fill) = match type_ {
            2 => (
                self.objat(edge, GLWideWidth),
                self.objat(edge, GLWideFill),
            ),
            3 => (
                self.objat(edge, GBWideWidth),
                self.objat(edge, GBWideFill),
            ),
            _ => (0, 0),
        };
        if fill == 0 {
            return;
        }
        if !self.needAvailableSpace(StackFillEntryLength) {
            return;
        }
        // +1 so lines sort before interior fills.
        let depth = (((self.objat(edge, GEZValue) as usize) << 1) as SqInt) + 1;
        let right_x = self.objat(edge, GEXValue) + line_width;
        let index = self.findStackFilldepth(fill, depth);
        if index == -1 {
            self.showFilldepthrightX(fill, depth, right_x);
        } else if self.stackFillRightX(index) < right_x {
            self.stackFillRightXput(index, right_x);
        }
        let x = self.objat(edge, GEXValue);
        self.quickRemoveInvalidFillsAt(x);
    }

    /// `quickRemoveInvalidFillsAt:`.
    pub fn quickRemoveInvalidFillsAt(&mut self, left_x: SqInt) {
        if self.wbStackSize() == 0 {
            return;
        }
        while self.topRightX() <= left_x {
            let fill = self.topFill();
            let depth = self.topDepth();
            self.hideFilldepth(fill, depth);
            if self.wbStackSize() == 0 {
                return;
            }
        }
    }

    // --- span buffer --------------------------------------------------------

    /// `clearSpanBuffer` — only the part the previous scan line used.
    pub fn clearSpanBuffer(&mut self) {
        let shift = self.wb_at(GWAAShift) as usize;
        let mut x0 = ((self.wb_at(GWSpanStart) as usize) >> shift) as SqInt;
        let mut x1 = (((self.wb_at(GWSpanEnd) as usize) >> shift) + 1) as SqInt;
        if x0 < 0 {
            x0 = 0;
        }
        if x1 > self.wb_at(GWSpanSize) {
            x1 = self.wb_at(GWSpanSize);
        }
        while x0 < x1 {
            self.span_put(x0, 0);
            x0 += 1;
        }
        let size = self.wb_at(GWSpanSize);
        self.wb_put(GWSpanStart, size);
        self.wb_put(GWSpanEnd, 0);
    }

    /// `fillColorSpan:from:to:` — solid color, no clipping (callers clip).
    pub fn fillColorSpanfromto(&mut self, pixel_value32: SqInt, left_x: SqInt, right_x: SqInt) {
        if self.wb_at(GWAALevel) != 1 {
            self.fillColorSpanAAx0x1(pixel_value32, left_x, right_x);
            return;
        }
        // The C unrolls this four-wide; the stores are identical.
        let mut x0 = left_x;
        while x0 < right_x {
            self.span_put(x0, pixel_value32 as u32);
            x0 += 1;
        }
    }

    /// `fillColorSpanAA:x0:x1:` — the three-part anti-aliased color fill.
    pub fn fillColorSpanAAx0x1(&mut self, pixel_value32: SqInt, left_x: SqInt, right_x: SqInt) {
        let first_pixel = self.aaFirstPixelFromto(left_x, right_x);
        let last_pixel = self.aaLastPixelFromto(left_x, right_x);
        let aa_level = self.wb_at(GWAALevel);
        let base_shift = self.wb_at(GWAAShift) as usize;
        let mut x = left_x;
        // Part a: sub-pixels before the first full pixel.
        if x < first_pixel {
            let pv32 = (((pixel_value32 & self.wb_at(GWAAColorMask)) as usize)
                >> (self.wb_at(GWAAColorShift) as usize)) as SqInt;
            while x < first_pixel {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + pv32) as u32;
                self.span_put(idx, v);
                x += 1;
            }
        }
        // Part b: aaLevel pixels at a time between the full pixels.
        if x < last_pixel {
            let color_mask = (((self.wb_at(GWAAColorMask) as usize)
                >> (self.wb_at(GWAAShift) as usize))
                | 4042322160usize) as SqInt;
            let pv32 =
                (((pixel_value32 & color_mask) as usize) >> (self.wb_at(GWAAShift) as usize)) as SqInt;
            while x < last_pixel {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + pv32) as u32;
                self.span_put(idx, v);
                x += aa_level;
            }
        }
        // Part c: sub-pixels in the last full pixel.
        if x < right_x {
            let pv32 = (((pixel_value32 & self.wb_at(GWAAColorMask)) as usize)
                >> (self.wb_at(GWAAColorShift) as usize)) as SqInt;
            while x < right_x {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + pv32) as u32;
                self.span_put(idx, v);
                x += 1;
            }
        }
    }

    /// `fillSpan:from:to:` — clip, then dispatch on the fill kind. Answers
    /// true when the fill must be handled by Smalltalk code.
    ///
    /// The parameter is `unsigned int` in the C; callers truncate.
    pub fn fillSpanfromto(&mut self, fill: u32, left_x: SqInt, right_x: SqInt) -> bool {
        if fill == 0 {
            return false;
        }
        let mut x0 = if left_x < self.wb_at(GWSpanEndAA) {
            self.wb_at(GWSpanEndAA)
        } else {
            left_x
        };
        let span_limit =
            ((self.wb_at(GWSpanSize) as usize) << (self.wb_at(GWAAShift) as usize)) as SqInt;
        let mut x1 = if right_x > span_limit { span_limit } else { right_x };
        if x0 < self.wb_at(GWFillMinX) {
            x0 = self.wb_at(GWFillMinX);
        }
        if x1 > self.wb_at(GWFillMaxX) {
            x1 = self.wb_at(GWFillMaxX);
        }
        if x0 < self.wb_at(GWSpanStart) {
            self.wb_put(GWSpanStart, x0);
        }
        if x1 > self.wb_at(GWSpanEnd) {
            self.wb_put(GWSpanEnd, x1);
        }
        if x1 > self.wb_at(GWSpanEndAA) {
            self.wb_put(GWSpanEndAA, x1);
        }
        if x0 >= x1 {
            return false;
        }
        if (fill & 0xFF000000u32) != 0 {
            self.fillColorSpanfromto(fill as SqInt, x0, x1);
        } else {
            // Store the values for the dispatch (and for the image, which
            // reads them back through primitiveNextFillEntry).
            self.wb_put(GWLastExportedFill, fill as SqInt);
            self.wb_put(GWLastExportedLeftX, x0);
            self.wb_put(GWLastExportedRightX, x1);
            let type_ = self.fillTypeOf(fill as SqInt);
            if type_ <= 1 {
                return true;
            }
            let f = self.wb_at(GWLastExportedFill);
            let lx = self.wb_at(GWLastExportedLeftX);
            let rx = self.wb_at(GWLastExportedRightX);
            let y = self.wb_at(GWCurrentY);
            match type_ {
                2 => {
                    self.fillLinearGradientfromtoat(f, lx, rx, y);
                }
                3 => {
                    self.fillRadialGradientfromtoat(f, lx, rx, y);
                }
                4 | 5 => {
                    self.fillBitmapSpanfromtoat(f, lx, rx, y);
                }
                _ => {}
            }
        }
        false
    }

    /// `fillAllFrom:to:` — walk the fill stack over [leftX, rightX). Answers
    /// true when an external fill was hit (the C's callers discard this when
    /// inlined; see `findNextExternalFillFromAET`).
    pub fn fillAllFromto(&mut self, left_x: SqInt, right_x: SqInt) -> bool {
        // (The C reads topFill here too, but overwrites it before use.)
        let mut fill;
        let mut start_x = left_x;
        let mut stop_x = self.topRightX();
        while stop_x < right_x {
            fill = self.topFill();
            if fill != 0 && self.fillSpanfromto(fill as u32, start_x, stop_x) {
                return true;
            }
            self.quickRemoveInvalidFillsAt(stop_x);
            start_x = stop_x;
            stop_x = self.topRightX();
        }
        fill = self.topFill();
        if fill != 0 {
            return self.fillSpanfromto(fill as u32, start_x, right_x);
        }
        false
    }

    // --- linear gradients ---------------------------------------------------

    /// `fillLinearGradient:from:to:at:`.
    pub fn fillLinearGradientfromtoat(
        &mut self,
        fill: SqInt,
        left_x: SqInt,
        right_x: SqInt,
        y_value: SqInt,
    ) {
        let ramp = fill + GFRampOffset;
        let ramp_size = self.objat(fill, GFRampLength);
        let ds_x = self.objat(fill, GFDirectionX);
        let mut ds = ((left_x - self.objat(fill, GFOriginX)) * ds_x)
            + ((y_value - self.objat(fill, GFOriginY)) * self.objat(fill, GFDirectionY));
        let x0 = left_x;
        let mut x = left_x;
        let x1 = right_x;
        // Part one: everything outside the left boundary.
        let mut ramp_index;
        loop {
            ramp_index = ds / 65536;
            if !(((ramp_index < 0) || (ramp_index >= ramp_size)) && (x < x1)) {
                break;
            }
            x += 1;
            ds += ds_x;
        }
        if x > x0 {
            let mut ri = ramp_index;
            if ri < 0 {
                ri = 0;
            }
            if ri >= ramp_size {
                ri = ramp_size - 1;
            }
            let pixel = self.objat(ramp, ri);
            self.fillColorSpanfromto(pixel, x0, x);
        }
        if self.wb_at(GWAALevel) == 1 {
            // Fast version without anti-aliasing.
            loop {
                ramp_index = ds / 65536;
                if !(((ramp_index < ramp_size) && (ramp_index >= 0)) && (x < x1)) {
                    break;
                }
                let v = self.objat(ramp, ramp_index) as u32;
                self.span_put(x, v);
                x += 1;
                ds += ds_x;
            }
        } else {
            x = self.fillLinearGradientAArampdsdsXfromto(fill, ds, ds_x, x, right_x);
        }
        if x < x1 {
            let mut ri = ramp_index;
            if ri < 0 {
                ri = 0;
            }
            if ri >= ramp_size {
                ri = ramp_size - 1;
            }
            let pixel = self.objat(ramp, ri);
            self.fillColorSpanfromto(pixel, x, x1);
        }
    }

    /// `fillLinearGradientAA:ramp:ds:dsX:from:to:` — answers the final x.
    pub fn fillLinearGradientAArampdsdsXfromto(
        &mut self,
        fill: SqInt,
        delta_s: SqInt,
        ds_x: SqInt,
        left_x: SqInt,
        right_x: SqInt,
    ) -> SqInt {
        let ramp = fill + GFRampOffset;
        let aa_level = self.wb_at(GWAALevel);
        let base_shift = self.wb_at(GWAAShift) as usize;
        let ramp_size = self.objat(fill, GFRampLength);
        let mut ds = delta_s;
        let mut x = left_x;
        let mut ramp_index = ds / 65536;
        let first_pixel = self.aaFirstPixelFromto(left_x, right_x);
        let last_pixel = self.aaLastPixelFromto(left_x, right_x);
        let mut color_mask = self.wb_at(GWAAColorMask);
        let mut color_shift = self.wb_at(GWAAColorShift);
        while (x < first_pixel) && ((ramp_index < ramp_size) && (ramp_index >= 0)) {
            let ramp_value =
                (((self.objat(ramp, ramp_index) & color_mask) as usize) >> color_shift as usize)
                    as SqInt;
            while (x < first_pixel) && ((ds / 65536) == ramp_index) {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + ramp_value) as u32;
                self.span_put(idx, v);
                x += 1;
                ds += ds_x;
            }
            ramp_index = ds / 65536;
        }
        color_mask = (((self.wb_at(GWAAColorMask) as usize) >> (self.wb_at(GWAAShift) as usize))
            | 4042322160usize) as SqInt;
        color_shift = self.wb_at(GWAAShift);
        while (x < last_pixel) && ((ramp_index < ramp_size) && (ramp_index >= 0)) {
            let ramp_value =
                (((self.objat(ramp, ramp_index) & color_mask) as usize) >> color_shift as usize)
                    as SqInt;
            while (x < last_pixel) && ((ds / 65536) == ramp_index) {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + ramp_value) as u32;
                self.span_put(idx, v);
                x += aa_level;
                ds += ((ds_x as usize) << color_shift as usize) as SqInt;
            }
            ramp_index = ds / 65536;
        }
        color_mask = self.wb_at(GWAAColorMask);
        color_shift = self.wb_at(GWAAColorShift);
        while (x < right_x) && ((ramp_index < ramp_size) && (ramp_index >= 0)) {
            let ramp_value =
                (((self.objat(ramp, ramp_index) & color_mask) as usize) >> color_shift as usize)
                    as SqInt;
            while (x < right_x) && ((ds / 65536) == ramp_index) {
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + ramp_value) as u32;
                self.span_put(idx, v);
                x += 1;
                ds += ds_x;
            }
            ramp_index = ds / 65536;
        }
        x
    }

    // --- radial gradients ---------------------------------------------------

    /// The C steps radial `ds`/`dt` as 32-bit ints; this helper is that
    /// squared-distance expression, computed with i32 wraparound.
    #[inline]
    fn radial_len2_i32(ds: i32, dt: i32) -> SqInt {
        let q = ds / 65536;
        let r = dt / 65536;
        q.wrapping_mul(q).wrapping_add(r.wrapping_mul(r)) as SqInt
    }

    /// `fillRadialGradient:from:to:at:`.
    pub fn fillRadialGradientfromtoat(
        &mut self,
        fill: SqInt,
        left_x: SqInt,
        right_x: SqInt,
        y_value: SqInt,
    ) {
        let ramp = fill + GFRampOffset;
        let ramp_size = self.objat(fill, GFRampLength);
        let delta_x = left_x - self.objat(fill, GFOriginX);
        let delta_y = y_value - self.objat(fill, GFOriginY);
        let ds_x = self.objat(fill, GFDirectionX);
        let dt_x = self.objat(fill, GFNormalX);
        let mut ds = (delta_x * ds_x) + (delta_y * self.objat(fill, GFDirectionY));
        let mut dt = (delta_x * dt_x) + (delta_y * self.objat(fill, GFNormalY));
        let mut x = left_x;
        let x1 = right_x;
        // Part one: everything outside the outer radius (the upper bound).
        let length2 = (ramp_size - 1) * (ramp_size - 1);
        while ((((ds / 65536) * (ds / 65536)) + ((dt / 65536) * (dt / 65536))) >= length2)
            && (x < x1)
        {
            x += 1;
            ds += ds_x;
            dt += dt_x;
        }
        if x > left_x {
            let pixel = self.objat(ramp, ramp_size - 1);
            self.fillColorSpanfromto(pixel, left_x, x);
        }
        // deltaST is kept in the point1 header slot, as 32-bit ints.
        self.wb_put(GWPoint1, ds);
        self.wb_put(GWPoint1 + 1, dt);
        if x < self.objat(fill, GFOriginX) {
            // Draw the decreasing part of the ramp.
            x = if self.wb_at(GWAALevel) == 1 {
                self.fillRadialDecreasingrampdeltaSTdsXdtXfromto(fill, ds_x, dt_x, x, x1)
            } else {
                self.fillRadialDecreasingAArampdeltaSTdsXdtXfromto(fill, ds_x, dt_x, x, x1)
            };
        }
        if x < x1 {
            // Draw the increasing part of the ramp.
            x = if self.wb_at(GWAALevel) == 1 {
                self.fillRadialIncreasingrampdeltaSTdsXdtXfromto(fill, ds_x, dt_x, x, x1)
            } else {
                self.fillRadialIncreasingAArampdeltaSTdsXdtXfromto(fill, ds_x, dt_x, x, x1)
            };
        }
        if x < right_x {
            let pixel = self.objat(ramp, ramp_size - 1);
            self.fillColorSpanfromto(pixel, x, right_x);
        }
    }

    /// `fillRadialDecreasing:ramp:deltaST:dsX:dtX:from:to:`.
    fn fillRadialDecreasingrampdeltaSTdsXdtXfromto(
        &mut self,
        fill: SqInt,
        ds_x: SqInt,
        dt_x: SqInt,
        left_x: SqInt,
        right_x: SqInt,
    ) -> SqInt {
        let ramp = fill + GFRampOffset;
        let mut ds = self.wb_i32(GWPoint1);
        let mut dt = self.wb_i32(GWPoint1 + 1);
        let mut ramp_index =
            self.accurateLengthOfwith((ds / 65536) as SqInt, (dt / 65536) as SqInt);
        let mut ramp_value = self.objat(ramp, ramp_index);
        let mut length2 = (ramp_index - 1) * (ramp_index - 1);
        let mut x = left_x;
        let mut x1 = right_x;
        if x1 > self.objat(fill, GFOriginX) {
            x1 = self.objat(fill, GFOriginX);
        }
        while x < x1 {
            // Try to copy the current value more than just once.
            while (x < x1) && (Self::radial_len2_i32(ds, dt) >= length2) {
                self.span_put(x, ramp_value as u32);
                x += 1;
                ds = (ds as SqInt + ds_x) as i32;
                dt = (dt as SqInt + dt_x) as i32;
            }
            let next_length = Self::radial_len2_i32(ds, dt);
            while next_length < length2 {
                ramp_index -= 1;
                ramp_value = self.objat(ramp, ramp_index);
                length2 = (ramp_index - 1) * (ramp_index - 1);
            }
        }
        self.wb_put(GWPoint1, ds as SqInt);
        self.wb_put(GWPoint1 + 1, dt as SqInt);
        x
    }

    /// `fillRadialIncreasing:ramp:deltaST:dsX:dtX:from:to:`.
    fn fillRadialIncreasingrampdeltaSTdsXdtXfromto(
        &mut self,
        fill: SqInt,
        ds_x: SqInt,
        dt_x: SqInt,
        left_x: SqInt,
        right_x: SqInt,
    ) -> SqInt {
        let ramp = fill + GFRampOffset;
        let mut ds = self.wb_i32(GWPoint1);
        let mut dt = self.wb_i32(GWPoint1 + 1);
        let mut ramp_index =
            self.accurateLengthOfwith((ds / 65536) as SqInt, (dt / 65536) as SqInt);
        let mut ramp_value = self.objat(ramp, ramp_index);
        let ramp_size = self.objat(fill, GFRampLength);
        // This is the upper bound.
        let length2 = (ramp_size - 1) * (ramp_size - 1);
        let mut next_length = (ramp_index + 1) * (ramp_index + 1);
        let mut last_length = Self::radial_len2_i32(ds, dt);
        let mut x = left_x;
        let x1 = right_x;
        while (x < x1) && (last_length < length2) {
            // Try to copy the current value more than once.
            while (x < x1) && (Self::radial_len2_i32(ds, dt) <= next_length) {
                self.span_put(x, ramp_value as u32);
                x += 1;
                ds = (ds as SqInt + ds_x) as i32;
                dt = (dt as SqInt + dt_x) as i32;
            }
            last_length = Self::radial_len2_i32(ds, dt);
            while last_length > next_length {
                ramp_index += 1;
                ramp_value = self.objat(ramp, ramp_index);
                next_length = (ramp_index + 1) * (ramp_index + 1);
            }
        }
        self.wb_put(GWPoint1, ds as SqInt);
        self.wb_put(GWPoint1 + 1, dt as SqInt);
        x
    }

    /// `fillRadialDecreasingAA:ramp:deltaST:dsX:dtX:from:to:`.
    fn fillRadialDecreasingAArampdeltaSTdsXdtXfromto(
        &mut self,
        fill: SqInt,
        ds_x: SqInt,
        dt_x: SqInt,
        left_x: SqInt,
        right_x: SqInt,
    ) -> SqInt {
        let ramp = fill + GFRampOffset;
        let mut ds = self.wb_i32(GWPoint1);
        let mut dt = self.wb_i32(GWPoint1 + 1);
        let aa_level = self.wb_at(GWAALevel);
        let base_shift = self.wb_at(GWAAShift) as usize;
        let mut ramp_index =
            self.accurateLengthOfwith((ds / 65536) as SqInt, (dt / 65536) as SqInt);
        let mut length2 = (ramp_index - 1) * (ramp_index - 1);
        let mut x = left_x;
        let mut x1 = self.objat(fill, GFOriginX);
        if x1 > right_x {
            x1 = right_x;
        }
        let first_pixel = self.aaFirstPixelFromto(left_x, x1);
        let last_pixel = self.aaLastPixelFromto(left_x, x1);
        if x < first_pixel {
            let color_mask = self.wb_at(GWAAColorMask);
            let color_shift = self.wb_at(GWAAColorShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while x < first_pixel {
                while (x < first_pixel) && (Self::radial_len2_i32(ds, dt) >= length2) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += 1;
                    ds = (ds as SqInt + ds_x) as i32;
                    dt = (dt as SqInt + dt_x) as i32;
                }
                let next_length = Self::radial_len2_i32(ds, dt);
                while next_length < length2 {
                    ramp_index -= 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    length2 = (ramp_index - 1) * (ramp_index - 1);
                }
            }
        }
        if x < last_pixel {
            let color_mask = (((self.wb_at(GWAAColorMask) as usize)
                >> (self.wb_at(GWAAShift) as usize))
                | 4042322160usize) as SqInt;
            let color_shift = self.wb_at(GWAAShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while x < last_pixel {
                while (x < last_pixel) && (Self::radial_len2_i32(ds, dt) >= length2) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += aa_level;
                    ds = (ds as SqInt + (((ds_x as usize) << color_shift as usize) as SqInt)) as i32;
                    dt = (dt as SqInt + (((dt_x as usize) << color_shift as usize) as SqInt)) as i32;
                }
                let next_length = Self::radial_len2_i32(ds, dt);
                while next_length < length2 {
                    ramp_index -= 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    length2 = (ramp_index - 1) * (ramp_index - 1);
                }
            }
        }
        if x < x1 {
            let color_mask = self.wb_at(GWAAColorMask);
            let color_shift = self.wb_at(GWAAColorShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while x < x1 {
                while (x < x1) && (Self::radial_len2_i32(ds, dt) >= length2) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += 1;
                    ds = (ds as SqInt + ds_x) as i32;
                    dt = (dt as SqInt + dt_x) as i32;
                }
                let next_length = Self::radial_len2_i32(ds, dt);
                while next_length < length2 {
                    ramp_index -= 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    length2 = (ramp_index - 1) * (ramp_index - 1);
                }
            }
        }
        self.wb_put(GWPoint1, ds as SqInt);
        self.wb_put(GWPoint1 + 1, dt as SqInt);
        x
    }

    /// `fillRadialIncreasingAA:ramp:deltaST:dsX:dtX:from:to:`.
    fn fillRadialIncreasingAArampdeltaSTdsXdtXfromto(
        &mut self,
        fill: SqInt,
        ds_x: SqInt,
        dt_x: SqInt,
        left_x: SqInt,
        right_x: SqInt,
    ) -> SqInt {
        let ramp = fill + GFRampOffset;
        let mut ds = self.wb_i32(GWPoint1);
        let mut dt = self.wb_i32(GWPoint1 + 1);
        let aa_level = self.wb_at(GWAALevel);
        let base_shift = self.wb_at(GWAAShift) as usize;
        let mut ramp_index =
            self.accurateLengthOfwith((ds / 65536) as SqInt, (dt / 65536) as SqInt);
        let ramp_size = self.objat(fill, GFRampLength);
        // This is the upper bound.
        let length2 = (ramp_size - 1) * (ramp_size - 1);
        let mut next_length = (ramp_index + 1) * (ramp_index + 1);
        let mut last_length = Self::radial_len2_i32(ds, dt);
        let mut x = left_x;
        let first_pixel = self.aaFirstPixelFromto(left_x, right_x);
        let last_pixel = self.aaLastPixelFromto(left_x, right_x);
        if (x < first_pixel) && (last_length < length2) {
            let color_mask = self.wb_at(GWAAColorMask);
            let color_shift = self.wb_at(GWAAColorShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while (x < first_pixel) && (last_length < length2) {
                while (x < first_pixel) && (Self::radial_len2_i32(ds, dt) <= next_length) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += 1;
                    ds = (ds as SqInt + ds_x) as i32;
                    dt = (dt as SqInt + dt_x) as i32;
                }
                last_length = Self::radial_len2_i32(ds, dt);
                while last_length > next_length {
                    ramp_index += 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    next_length = (ramp_index + 1) * (ramp_index + 1);
                }
            }
        }
        if (x < last_pixel) && (last_length < length2) {
            let color_mask = (((self.wb_at(GWAAColorMask) as usize)
                >> (self.wb_at(GWAAShift) as usize))
                | 4042322160usize) as SqInt;
            let color_shift = self.wb_at(GWAAShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while (x < last_pixel) && (last_length < length2) {
                while (x < last_pixel) && (Self::radial_len2_i32(ds, dt) <= next_length) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += aa_level;
                    ds = (ds as SqInt + (((ds_x as usize) << color_shift as usize) as SqInt)) as i32;
                    dt = (dt as SqInt + (((dt_x as usize) << color_shift as usize) as SqInt)) as i32;
                }
                last_length = Self::radial_len2_i32(ds, dt);
                while last_length > next_length {
                    ramp_index += 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    next_length = (ramp_index + 1) * (ramp_index + 1);
                }
            }
        }
        if (x < right_x) && (last_length < length2) {
            let color_mask = self.wb_at(GWAAColorMask);
            let color_shift = self.wb_at(GWAAColorShift);
            let mut ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                >> color_shift as usize) as SqInt;
            while (x < right_x) && (last_length < length2) {
                while (x < right_x) && (Self::radial_len2_i32(ds, dt) <= next_length) {
                    let index = ((x as usize) >> base_shift) as SqInt;
                    let v = (self.span_at(index) as SqInt + ramp_value) as u32;
                    self.span_put(index, v);
                    x += 1;
                    ds = (ds as SqInt + ds_x) as i32;
                    dt = (dt as SqInt + dt_x) as i32;
                }
                last_length = Self::radial_len2_i32(ds, dt);
                while last_length > next_length {
                    ramp_index += 1;
                    ramp_value = (((self.objat(ramp, ramp_index) & color_mask) as usize)
                        >> color_shift as usize) as SqInt;
                    next_length = (ramp_index + 1) * (ramp_index + 1);
                }
            }
        }
        self.wb_put(GWPoint1, ds as SqInt);
        self.wb_put(GWPoint1 + 1, dt as SqInt);
        x
    }

    // --- bitmap fills -------------------------------------------------------

    /// The oop half of `loadBitsFrom:` plus the engine-side size check.
    fn loadBitsFrom(&mut self, bm_fill: SqInt) -> Option<(*const i32, SqInt)> {
        let x_index = self.objat(bm_fill, GEObjectIndex);
        let (bits, bits_len) = self.host.bits_of_form(x_index)?;
        if bits_len != self.objat(bm_fill, GBBitmapSize) {
            return None;
        }
        Some((bits, bits_len))
    }

    /// A checked read of the form's bits (`bits[index]` in the C).
    #[inline]
    fn bits_at(bits: (*const i32, SqInt), index: SqInt) -> i32 {
        let u = usize::try_from(index).expect("bitmap bits index negative");
        assert!(u < bits.1 as usize, "bitmap bits index out of range");
        // SAFETY: bounds just checked; the host guarantees the pointer covers
        // `bits.1` words for the primitive's duration.
        unsafe { *bits.0.add(u) }
    }

    /// `bitmapValue:bits:atX:y:` — one pixel, converted to 32 bits and color
    /// transformed. Answers the C's sign-extended `sqInt`.
    pub fn bitmapValuebitsatXy(
        &self,
        bm_fill: SqInt,
        bits: (*const i32, SqInt),
        xp: SqInt,
        yp: SqInt,
    ) -> SqInt {
        let bm_depth = self.objat(bm_fill, GBBitmapDepth);
        let bm_raster = self.objat(bm_fill, GBBitmapRaster);
        if bm_depth == 32 {
            let mut value = Self::bits_at(bits, (bm_raster * yp) + xp);
            if (value != 0) && ((value as u32 & 0xFF000000u32) == 0) {
                value = (value as u32 | 0xFF000000u32) as i32;
            }
            return self.uncheckedTransformColor(value as SqInt);
        }
        let mut r_shift = R_SHIFT_TABLE[bm_depth as usize];
        // cMask masks the pixel out of the word; rShift moves it to the lowest
        // bit position.
        let mut value = Self::bits_at(
            bits,
            (bm_raster * yp) + (((xp as usize) >> r_shift) as SqInt),
        );
        let c_mask = (1u32 << bm_depth) - 1;
        r_shift = (32 - bm_depth as i32) - (((xp as i32) & ((1i32 << r_shift) - 1)) * bm_depth as i32);
        value = (((value as SqInt as usize) >> r_shift) as u32 & c_mask) as i32;
        if bm_depth == 16 {
            // Must convert by expanding bits.
            if value != 0 {
                let mut b = ((value as SqInt & 0x1F) as usize) << 3;
                b += b >> 5;
                let mut g = (((value as SqInt as usize) >> 5) & 0x1F) << 3;
                g += g >> 5;
                let mut r = (((value as SqInt as usize) >> 10) & 0x1F) << 3;
                r += r >> 5;
                let a = 0xFFusize;
                value = (b + (g << 8) + (r << 16) + (a << 24)) as i32;
            }
        } else {
            // Must convert by using the color map.
            if self.objat(bm_fill, GBColormapSize) == 0 {
                value = 0;
            } else {
                value = self.objat(bm_fill, GBColormapOffset + value as SqInt) as i32;
            }
        }
        self.uncheckedTransformColor(value as SqInt)
    }

    /// Shared per-pixel coordinate step for the bitmap fills: tiling or
    /// clamping, then the in-bounds test.
    #[inline]
    fn bitmap_coords(
        &self,
        tile_flag: bool,
        ds: &mut SqInt,
        dt: &mut SqInt,
        bm_width: SqInt,
        bm_height: SqInt,
    ) -> Option<(SqInt, SqInt)> {
        if tile_flag {
            *ds = self.repeatValuemax(*ds, ((bm_width as usize) << 16) as SqInt);
            *dt = self.repeatValuemax(*dt, ((bm_height as usize) << 16) as SqInt);
        }
        let mut xp = *ds / 65536;
        let mut yp = *dt / 65536;
        if !tile_flag {
            xp = self.clampValuemax(xp, bm_width);
            yp = self.clampValuemax(yp, bm_height);
        }
        if (xp >= 0) && (yp >= 0) && (xp < bm_width) && (yp < bm_height) {
            Some((xp, yp))
        } else {
            None
        }
    }

    /// `fillBitmapSpan:from:to:at:`.
    pub fn fillBitmapSpanfromtoat(
        &mut self,
        bm_fill: SqInt,
        left_x: SqInt,
        right_x: SqInt,
        y_value: SqInt,
    ) {
        if self.wb_at(GWAALevel) != 1 {
            self.fillBitmapSpanAAfromtoat(bm_fill, left_x, right_x, y_value);
            return;
        }
        let bits = match self.loadBitsFrom(bm_fill) {
            Some(b) => b,
            None => return,
        };
        let bm_width = self.objat(bm_fill, GBBitmapWidth);
        let bm_height = self.objat(bm_fill, GBBitmapHeight);
        let tile_flag = self.objat(bm_fill, GBTileFlag) == 1;
        let delta_x = left_x - self.objat(bm_fill, GFOriginX);
        let delta_y = y_value - self.objat(bm_fill, GFOriginY);
        let ds_x = self.objat(bm_fill, GFDirectionX);
        let dt_x = self.objat(bm_fill, GFNormalX);
        let mut ds = (delta_x * ds_x) + (delta_y * self.objat(bm_fill, GFDirectionY));
        let mut dt = (delta_x * dt_x) + (delta_y * self.objat(bm_fill, GFNormalY));
        let mut x = left_x;
        let x1 = right_x;
        while x < x1 {
            if let Some((xp, yp)) = self.bitmap_coords(tile_flag, &mut ds, &mut dt, bm_width, bm_height)
            {
                let fill_value = self.bitmapValuebitsatXy(bm_fill, bits, xp, yp);
                self.span_put(x, fill_value as u32);
            }
            ds += ds_x;
            dt += dt_x;
            x += 1;
        }
    }

    /// `fillBitmapSpanAA:from:to:at:` — the three-part anti-aliased variant.
    pub fn fillBitmapSpanAAfromtoat(
        &mut self,
        bm_fill: SqInt,
        left_x: SqInt,
        right_x: SqInt,
        y_value: SqInt,
    ) {
        let bits = match self.loadBitsFrom(bm_fill) {
            Some(b) => b,
            None => return,
        };
        let bm_width = self.objat(bm_fill, GBBitmapWidth);
        let bm_height = self.objat(bm_fill, GBBitmapHeight);
        let tile_flag = self.objat(bm_fill, GBTileFlag) == 1;
        let delta_x = left_x - self.objat(bm_fill, GFOriginX);
        let delta_y = y_value - self.objat(bm_fill, GFOriginY);
        let ds_x = self.objat(bm_fill, GFDirectionX);
        let dt_x = self.objat(bm_fill, GFNormalX);
        let mut ds = (delta_x * ds_x) + (delta_y * self.objat(bm_fill, GFDirectionY));
        let mut dt = (delta_x * dt_x) + (delta_y * self.objat(bm_fill, GFNormalY));
        let aa_level = self.wb_at(GWAALevel);
        let first_pixel = self.aaFirstPixelFromto(left_x, right_x);
        let last_pixel = self.aaLastPixelFromto(left_x, right_x);
        let base_shift = self.wb_at(GWAAShift) as usize;
        let mut c_mask = self.wb_at(GWAAColorMask);
        let mut c_shift = self.wb_at(GWAAColorShift);
        let mut x = left_x;
        while x < first_pixel {
            if let Some((xp, yp)) = self.bitmap_coords(tile_flag, &mut ds, &mut dt, bm_width, bm_height)
            {
                let mut fill_value = self.bitmapValuebitsatXy(bm_fill, bits, xp, yp);
                fill_value = (((fill_value & c_mask) as usize) >> c_shift as usize) as SqInt;
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + fill_value) as u32;
                self.span_put(idx, v);
            }
            ds += ds_x;
            dt += dt_x;
            x += 1;
        }
        c_mask = (((self.wb_at(GWAAColorMask) as usize) >> (self.wb_at(GWAAShift) as usize))
            | 4042322160usize) as SqInt;
        c_shift = self.wb_at(GWAAShift);
        while x < last_pixel {
            if let Some((xp, yp)) = self.bitmap_coords(tile_flag, &mut ds, &mut dt, bm_width, bm_height)
            {
                let mut fill_value = self.bitmapValuebitsatXy(bm_fill, bits, xp, yp);
                fill_value = (((fill_value & c_mask) as usize) >> c_shift as usize) as SqInt;
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + fill_value) as u32;
                self.span_put(idx, v);
            }
            ds += ((ds_x as usize) << c_shift as usize) as SqInt;
            dt += ((dt_x as usize) << c_shift as usize) as SqInt;
            x += aa_level;
        }
        c_mask = self.wb_at(GWAAColorMask);
        c_shift = self.wb_at(GWAAColorShift);
        while x < right_x {
            if let Some((xp, yp)) = self.bitmap_coords(tile_flag, &mut ds, &mut dt, bm_width, bm_height)
            {
                let mut fill_value = self.bitmapValuebitsatXy(bm_fill, bits, xp, yp);
                fill_value = (((fill_value & c_mask) as usize) >> c_shift as usize) as SqInt;
                let idx = ((x as usize) >> base_shift) as SqInt;
                let v = (self.span_at(idx) as SqInt + fill_value) as u32;
                self.span_put(idx, v);
            }
            ds += ds_x;
            dt += dt_x;
            x += 1;
        }
    }

    /// `fillBitmapSpan:from:to:` — a raw bitmap handed in by
    /// `primitiveMergeFillFrom`; always starts reading at bit index 0.
    pub fn fillBitmapSpanfromto(&mut self, bits: &[i32], left_x: SqInt, right_x: SqInt) {
        let mut x0 = left_x;
        let x1 = right_x;
        // "Hack for pre-increment" in the C: bitX starts at -1.
        let mut bit_x: SqInt = -1;
        if self.wb_at(GWAALevel) == 1 {
            // Speedy version for no anti-aliasing.
            while x0 < x1 {
                bit_x += 1;
                let fill_value = bits[bit_x as usize] as SqInt;
                self.span_put(x0, fill_value as u32);
                x0 += 1;
            }
        } else {
            // Generic version with anti-aliasing.
            let color_mask = self.wb_at(GWAAColorMask);
            let color_shift = self.wb_at(GWAAColorShift);
            let base_shift = self.wb_at(GWAAShift) as usize;
            while x0 < x1 {
                let x = ((x0 as usize) >> base_shift) as SqInt;
                bit_x += 1;
                let mut fill_value = bits[bit_x as usize] as SqInt;
                fill_value = (((fill_value & color_mask) as usize) >> color_shift as usize) as SqInt;
                let v = (self.span_at(x) as SqInt + fill_value) as u32;
                self.span_put(x, v);
                x0 += 1;
            }
        }
        if x1 > self.wb_at(GWSpanEnd) {
            self.wb_put(GWSpanEnd, x1);
        }
        if x1 > self.wb_at(GWSpanEndAA) {
            self.wb_put(GWSpanEndAA, x1);
        }
    }
}
