//! Geometry loading: bezier subdivision on the work-buffer stack, and the
//! line/bezier/shape/polygon/oval/rectangle/fill constructors.
//!
//! Beziers are subdivided until they are monotone in Y (and X, when wide) and
//! at most 255 scan lines tall; the pieces live on the work-buffer stack as
//! 6-word (startX, startY, viaX, viaY, endX, endY) entries addressed by the
//! C's opaque "stack index" (distance from the top).
//!
//! Point sources come in two layouts (`PointArray` int pairs and
//! `ShortPointArray` int16 pairs); [`PointsRef`] reproduces the C's pointer
//! reinterpretation, including its little-endian halfword order.

#![allow(non_snake_case)]

use crate::consts::*;
use crate::engine::{Engine, Host, SqInt, CIRCLE_COS_TABLE, CIRCLE_SIN_TABLE};

/// A borrowed view of a words object holding points (`int*` in the C, also
/// reinterpreted as `short*` for ShortPointArrays / ShortRunArrays).
#[derive(Clone, Copy)]
pub struct PointsRef {
    ptr: *const i32,
    /// Length in 32-bit words.
    words: usize,
}

impl PointsRef {
    /// Wraps `words` 32-bit cells at `ptr`.
    ///
    /// # Safety
    ///
    /// The memory must stay valid (and un-moved) while this view is used.
    pub unsafe fn new(ptr: *const i32, words: usize) -> Self {
        PointsRef { ptr, words }
    }

    /// A view over a slice (for tests).
    pub fn from_slice(s: &[i32]) -> Self {
        PointsRef {
            ptr: s.as_ptr(),
            words: s.len(),
        }
    }

    /// `((int*)points)[index]` (`loadPointIntAt:from:`).
    #[inline]
    pub fn int_at(&self, index: SqInt) -> i32 {
        let u = usize::try_from(index).expect("point index negative");
        assert!(u < self.words, "point index out of range");
        // SAFETY: bounds just checked.
        unsafe { *self.ptr.add(u) }
    }

    /// `((short*)points)[index]` (`loadPointShortAt:from:`) — the low half of
    /// the word first, as on the little-endian targets this VM builds for.
    #[inline]
    pub fn short_at(&self, index: SqInt) -> i16 {
        let u = usize::try_from(index).expect("point index negative");
        assert!(u / 2 < self.words, "point index out of range");
        let word = // SAFETY: bounds just checked.
            unsafe { *self.ptr.add(u / 2) } as u32;
        if u % 2 == 0 {
            (word & 0xFFFF) as i16
        } else {
            (word >> 16) as i16
        }
    }
}

impl<H: Host> Engine<H> {
    // --- the bezier stack ---------------------------------------------------

    /// The bezier stack accessors `bzStartX:` ... `bzEndY:put:` all address
    /// `wbStackValue: stackSize - index + k`; this is that common index.
    #[inline]
    fn bz_slot(&self, index: SqInt, k: SqInt) -> SqInt {
        (self.wbStackSize() - index) + k
    }

    #[inline]
    fn bz_at(&self, index: SqInt, k: SqInt) -> SqInt {
        self.wbStackValue(self.bz_slot(index, k))
    }

    #[inline]
    fn bz_put(&mut self, index: SqInt, k: SqInt, value: SqInt) {
        let slot = self.bz_slot(index, k);
        self.wbStackValueput(slot, value);
    }

    /// `computeBezierSplitAtHalf:` — split the entry at `index`, answering the
    /// new entry's stack index (0 when the engine stopped).
    pub fn computeBezierSplitAtHalf(&mut self, index: SqInt) -> SqInt {
        let new_index = self.allocateBezierStackEntry();
        if self.engineStopped {
            return 0;
        }
        let start_x = self.bz_at(index, 0);
        let start_y = self.bz_at(index, 1);
        let via_x = self.bz_at(index, 2);
        let via_y = self.bz_at(index, 3);
        let end_x = self.bz_at(index, 4);
        let end_y = self.bz_at(index, 5);
        let mut left_via_x = start_x;
        let mut left_via_y = start_y;
        let mut right_via_x = via_x;
        let mut right_via_y = via_y;
        left_via_x += (via_x - start_x) / 2;
        left_via_y += (via_y - start_y) / 2;
        right_via_x += (end_x - via_x) / 2;
        right_via_y += (end_y - via_y) / 2;
        let mut shared_x = right_via_x;
        let mut shared_y = right_via_y;
        shared_x += (left_via_x - right_via_x) / 2;
        shared_y += (left_via_y - right_via_y) / 2;
        // Store the first part back.
        self.bz_put(index, 2, left_via_x);
        self.bz_put(index, 3, left_via_y);
        self.bz_put(index, 4, shared_x);
        self.bz_put(index, 5, shared_y);
        self.bz_put(new_index, 0, shared_x);
        self.bz_put(new_index, 1, shared_y);
        self.bz_put(new_index, 2, right_via_x);
        self.bz_put(new_index, 3, right_via_y);
        self.bz_put(new_index, 4, end_x);
        self.bz_put(new_index, 5, end_y);
        new_index
    }

    /// `computeBezier:splitAt:` — split at a parametric value, clamping the
    /// new via points so the halves stay monotone in Y.
    pub fn computeBeziersplitAt(&mut self, index: SqInt, param: f64) -> SqInt {
        let start_x = self.bz_at(index, 0);
        let start_y = self.bz_at(index, 1);
        let via_x = self.bz_at(index, 2);
        let via_y = self.bz_at(index, 3);
        let end_x = self.bz_at(index, 4);
        let end_y = self.bz_at(index, 5);
        let mut left_via_x = start_x;
        let mut left_via_y = start_y;
        let mut right_via_x = via_x;
        let mut right_via_y = via_y;
        left_via_x += (((via_x - start_x) as f64) * param) as SqInt;
        left_via_y += (((via_y - start_y) as f64) * param) as SqInt;
        let mut shared_x = left_via_x;
        let mut shared_y = left_via_y;
        right_via_x += (((end_x - via_x) as f64) * param) as SqInt;
        right_via_y += (((end_y - via_y) as f64) * param) as SqInt;
        shared_x += (((right_via_x - left_via_x) as f64) * param) as SqInt;
        shared_y += (((right_via_y - left_via_y) as f64) * param) as SqInt;
        // Check the new via points.
        left_via_y = self.assureValuebetweenand(left_via_y, start_y, shared_y);
        right_via_y = self.assureValuebetweenand(right_via_y, shared_y, end_y);
        let new_index = self.allocateBezierStackEntry();
        if self.engineStopped {
            return 0;
        }
        self.bz_put(index, 2, left_via_x);
        self.bz_put(index, 3, left_via_y);
        self.bz_put(index, 4, shared_x);
        self.bz_put(index, 5, shared_y);
        self.bz_put(new_index, 0, shared_x);
        self.bz_put(new_index, 1, shared_y);
        self.bz_put(new_index, 2, right_via_x);
        self.bz_put(new_index, 3, right_via_y);
        self.bz_put(new_index, 4, end_x);
        self.bz_put(new_index, 5, end_y);
        new_index
    }

    /// `subdivideBezier:` — split when taller than 255 lines or much wider
    /// than tall (the overflow guard).
    pub fn subdivideBezier(&mut self, index: SqInt) -> SqInt {
        let start_y = self.bz_at(index, 1);
        let end_y = self.bz_at(index, 5);
        if end_y == start_y {
            return index;
        }
        let mut delta_y = end_y - start_y;
        if delta_y < 0 {
            delta_y = 0 - delta_y;
        }
        if delta_y > 0xFF {
            self.incrementStatby(GWBezierHeightSubdivisions, 1);
            return self.computeBezierSplitAtHalf(index);
        }
        let start_x = self.bz_at(index, 0);
        let end_x = self.bz_at(index, 4);
        let mut delta_x = end_x - start_x;
        if delta_x < 0 {
            delta_x = 0 - delta_x;
        }
        if (delta_y * 32) < delta_x {
            self.incrementStatby(GWBezierOverflowSubdivisions, 1);
            return self.computeBezierSplitAtHalf(index);
        }
        index
    }

    /// `subdivideBezierFrom:` — recursive subdivision; answers the maximum
    /// stack index used.
    pub fn subdivideBezierFrom(&mut self, index: SqInt) -> SqInt {
        let other_index = self.subdivideBezier(index);
        if other_index != index {
            let index1 = self.subdivideBezierFrom(index);
            if self.engineStopped {
                return 0;
            }
            let index2 = self.subdivideBezierFrom(other_index);
            if self.engineStopped {
                return 0;
            }
            return if index1 >= index2 { index1 } else { index2 };
        }
        index
    }

    /// `subdivideToBeMonotonInX:`.
    pub fn subdivideToBeMonotonInX(&mut self, index: SqInt) -> SqInt {
        let start_x = self.bz_at(index, 0);
        let via_x = self.bz_at(index, 2);
        let end_x = self.bz_at(index, 4);
        let dx1 = via_x - start_x;
        let dx2 = end_x - via_x;
        if (dx1 * dx2) >= 0 {
            return index;
        }
        self.incrementStatby(GWBezierMonotonSubdivisions, 1);
        let mut denom = dx2 - dx1;
        let mut num = dx1;
        if num < 0 {
            num = 0 - num;
        }
        if denom < 0 {
            denom = 0 - denom;
        }
        self.computeBeziersplitAt(index, num as f64 / denom as f64)
    }

    /// `subdivideToBeMonotonInY:`.
    pub fn subdivideToBeMonotonInY(&mut self, index: SqInt) -> SqInt {
        let start_y = self.bz_at(index, 1);
        let via_y = self.bz_at(index, 3);
        let end_y = self.bz_at(index, 5);
        let dy1 = via_y - start_y;
        let dy2 = end_y - via_y;
        if (dy1 * dy2) >= 0 {
            return index;
        }
        self.incrementStatby(GWBezierMonotonSubdivisions, 1);
        let mut denom = dy2 - dy1;
        let mut num = dy1;
        if num < 0 {
            num = 0 - num;
        }
        if denom < 0 {
            denom = 0 - denom;
        }
        self.computeBeziersplitAt(index, num as f64 / denom as f64)
    }

    /// `subdivideToBeMonoton:inX:`.
    pub fn subdivideToBeMonotoninX(&mut self, base: SqInt, do_test_x: bool) -> SqInt {
        let base2 = self.subdivideToBeMonotonInY(base);
        let mut index1 = base2;
        let mut index2 = base2;
        if do_test_x {
            index1 = self.subdivideToBeMonotonInX(base);
        }
        if index1 > index2 {
            index2 = index1;
        }
        if (base != base2) && do_test_x {
            index1 = self.subdivideToBeMonotonInX(base2);
        }
        if index1 > index2 {
            index2 = index1;
        }
        index2
    }

    /// `loadAndSubdivideBezierFrom:via:to:isWide:` — pushes the curve read
    /// from the header point slots and subdivides; answers the segment count.
    pub fn loadAndSubdivideBezierFromviatoisWide(
        &mut self,
        point1: SqInt,
        point2: SqInt,
        point3: SqInt,
        wide_flag: bool,
    ) -> SqInt {
        let bz1 = self.allocateBezierStackEntry();
        if self.engineStopped {
            return 0;
        }
        let (p1x, p1y) = (self.point_x(point1), self.point_y(point1));
        let (p2x, p2y) = (self.point_x(point2), self.point_y(point2));
        let (p3x, p3y) = (self.point_x(point3), self.point_y(point3));
        self.bz_put(bz1, 0, p1x as SqInt);
        self.bz_put(bz1, 1, p1y as SqInt);
        self.bz_put(bz1, 2, p2x as SqInt);
        self.bz_put(bz1, 3, p2y as SqInt);
        self.bz_put(bz1, 4, p3x as SqInt);
        self.bz_put(bz1, 5, p3y as SqInt);
        let bz2 = self.subdivideToBeMonotoninX(bz1, wide_flag);
        let mut index2 = bz2;
        let mut index = bz1;
        while index <= bz2 {
            let index1 = self.subdivideBezierFrom(index);
            if index1 > index2 {
                index2 = index1;
            }
            if self.engineStopped {
                return 0;
            }
            index += 6;
        }
        index2 / 6
    }

    // --- lines and beziers into the object buffer ---------------------------

    /// `loadBezier:segment:leftFill:rightFill:offset:` — one subdivided
    /// segment from the stack into a bezier object, top-to-bottom order.
    pub fn loadBeziersegmentleftFillrightFilloffset(
        &mut self,
        bezier: SqInt,
        index: SqInt,
        left_fill_index: SqInt,
        right_fill_index: SqInt,
        y_offset: SqInt,
    ) {
        if self.bz_at(index, 5) >= self.bz_at(index, 1) {
            // Top to bottom.
            let v = self.bz_at(index, 0);
            self.objatput(bezier, GEXValue, v);
            let v = self.bz_at(index, 1) - y_offset;
            self.objatput(bezier, GEYValue, v);
            let v = self.bz_at(index, 2);
            self.objatput(bezier, GBViaX, v);
            let v = self.bz_at(index, 3) - y_offset;
            self.objatput(bezier, GBViaY, v);
            let v = self.bz_at(index, 4);
            self.objatput(bezier, GBEndX, v);
            let v = self.bz_at(index, 5) - y_offset;
            self.objatput(bezier, GBEndY, v);
        } else {
            let v = self.bz_at(index, 4);
            self.objatput(bezier, GEXValue, v);
            let v = self.bz_at(index, 5) - y_offset;
            self.objatput(bezier, GEYValue, v);
            let v = self.bz_at(index, 2);
            self.objatput(bezier, GBViaX, v);
            let v = self.bz_at(index, 3) - y_offset;
            self.objatput(bezier, GBViaY, v);
            let v = self.bz_at(index, 0);
            self.objatput(bezier, GBEndX, v);
            let v = self.bz_at(index, 1) - y_offset;
            self.objatput(bezier, GBEndY, v);
        }
        let v = self.wb_at(GWCurrentZ);
        self.objatput(bezier, GEZValue, v);
        self.objatput(bezier, GEFillIndexLeft, left_fill_index);
        self.objatput(bezier, GEFillIndexRight, right_fill_index);
    }

    /// `loadWideBezier:lineFill:leftFill:rightFill:n:` — all stacked segments
    /// into (possibly wide) bezier objects; clears the stack afterwards.
    pub fn loadWideBezierlineFillleftFillrightFilln(
        &mut self,
        line_width: SqInt,
        line_fill: SqInt,
        left_fill: SqInt,
        right_fill: SqInt,
        n_segments: SqInt,
    ) {
        let (wide, offset) = if (line_width == 0) || (line_fill == 0) {
            (false, 0)
        } else {
            (true, line_width / 2)
        };
        let mut index = n_segments * 6;
        while index > 0 {
            let bezier = if wide {
                self.allocateWideBezier()
            } else {
                self.allocateBezier()
            };
            if self.engineStopped {
                return;
            }
            self.loadBeziersegmentleftFillrightFilloffset(
                bezier, index, left_fill, right_fill, offset,
            );
            if wide {
                self.objatput(bezier, GBWideFill, line_fill);
                self.objatput(bezier, GBWideWidth, line_width);
                self.objatput(bezier, GBWideExtent, line_width);
            }
            index -= 6;
        }
        self.wbStackClear();
    }

    /// `loadLine:from:to:offset:leftFill:rightFill:` — the shared line setup;
    /// `p1`/`p2` are header point slots.
    pub fn loadLinefromtooffsetleftFillrightFill(
        &mut self,
        line: SqInt,
        p1: SqInt,
        p2: SqInt,
        y_offset: SqInt,
        left_fill: SqInt,
        right_fill: SqInt,
    ) {
        let (p1, p2, y_dir) = if self.point_y(p1) <= self.point_y(p2) {
            (p1, p2, 1)
        } else {
            (p2, p1, -1)
        };
        let v = self.point_x(p1) as SqInt;
        self.objatput(line, GEXValue, v);
        let v = self.point_y(p1) as SqInt - y_offset;
        self.objatput(line, GEYValue, v);
        let v = self.wb_at(GWCurrentZ);
        self.objatput(line, GEZValue, v);
        self.objatput(line, GEFillIndexLeft, left_fill);
        self.objatput(line, GEFillIndexRight, right_fill);
        let v = self.point_x(p2) as SqInt;
        self.objatput(line, GLEndX, v);
        let v = self.point_y(p2) as SqInt - y_offset;
        self.objatput(line, GLEndY, v);
        self.objatput(line, GLYDirection, y_dir);
    }

    /// `loadWideLine:from:to:lineFill:leftFill:rightFill:`.
    pub fn loadWideLinefromtolineFillleftFillrightFill(
        &mut self,
        line_width: SqInt,
        p1: SqInt,
        p2: SqInt,
        line_fill: SqInt,
        left_fill: SqInt,
        right_fill: SqInt,
    ) {
        let (line, offset) = if (line_width == 0) || (line_fill == 0) {
            (self.allocateLine(), 0)
        } else {
            (self.allocateWideLine(), line_width / 2)
        };
        if self.engineStopped {
            return;
        }
        self.loadLinefromtooffsetleftFillrightFill(line, p1, p2, offset, left_fill, right_fill);
        if self.isWide(line) {
            self.objatput(line, GLWideFill, line_fill);
            self.objatput(line, GLWideWidth, line_width);
            self.objatput(line, GLWideExtent, line_width);
        }
    }

    // --- shapes -------------------------------------------------------------

    /// The body shared by `loadShape...` and `loadCompressedShape...`:
    /// `loadCompressedSegment:from:short:leftFill:rightFill:lineWidth:lineColor:`.
    #[allow(clippy::too_many_arguments)]
    pub fn loadCompressedSegmentfromshortleftFillrightFilllineWidthlineColor(
        &mut self,
        segment_index: SqInt,
        points: PointsRef,
        points_short: bool,
        left_fill: SqInt,
        right_fill: SqInt,
        line_width: SqInt,
        line_fill: SqInt,
    ) {
        // Check if we have anything to do at all.
        if (left_fill == right_fill) && ((line_width == 0) || (line_fill == 0)) {
            return;
        }
        // 3 points with x/y each.
        let index = segment_index * 6;
        let (x0, y0, x1, y1, x2, y2);
        if points_short {
            x0 = points.short_at(index) as SqInt;
            y0 = points.short_at(index + 1) as SqInt;
            x1 = points.short_at(index + 2) as SqInt;
            y1 = points.short_at(index + 3) as SqInt;
            x2 = points.short_at(index + 4) as SqInt;
            y2 = points.short_at(index + 5) as SqInt;
        } else {
            x0 = points.int_at(index) as SqInt;
            y0 = points.int_at(index + 1) as SqInt;
            x1 = points.int_at(index + 2) as SqInt;
            y1 = points.int_at(index + 3) as SqInt;
            x2 = points.int_at(index + 4) as SqInt;
            y2 = points.int_at(index + 5) as SqInt;
        }
        if ((x0 == x1) && (y0 == y1)) || ((x1 == x2) && (y1 == y2)) {
            // We can use a line from x0/y0 to x2/y2.
            if (x0 == x2) && (y0 == y2) {
                return;
            }
            self.point_put(GWPoint1, x0 as i32, y0 as i32);
            self.point_put(GWPoint2, x2 as i32, y2 as i32);
            self.transformPoints(2);
            self.loadWideLinefromtolineFillleftFillrightFill(
                line_width, GWPoint1, GWPoint2, line_fill, left_fill, right_fill,
            );
            return;
        }
        self.point_put(GWPoint1, x0 as i32, y0 as i32);
        self.point_put(GWPoint2, x1 as i32, y1 as i32);
        self.point_put(GWPoint3, x2 as i32, y2 as i32);
        self.transformPoints(3);
        let segs = self.loadAndSubdivideBezierFromviatoisWide(
            GWPoint1,
            GWPoint2,
            GWPoint3,
            (line_width != 0) && (line_fill != 0),
        );
        if self.engineStopped {
            return;
        }
        self.loadWideBezierlineFillleftFillrightFilln(
            line_width, line_fill, left_fill, right_fill, segs,
        );
    }

    /// `loadShape:nSegments:fill:lineWidth:lineFill:pointsShort:`.
    pub fn loadShapenSegmentsfilllineWidthlineFillpointsShort(
        &mut self,
        points: PointsRef,
        n_segments: SqInt,
        fill_index: SqInt,
        line_width: SqInt,
        line_fill: SqInt,
        points_short: bool,
    ) {
        for i in 1..=n_segments {
            self.loadCompressedSegmentfromshortleftFillrightFilllineWidthlineColor(
                i - 1,
                points,
                points_short,
                fill_index,
                0,
                line_width,
                line_fill,
            );
            if self.engineStopped {
                return;
            }
        }
    }

    /// `loadCompressedShape:segments:leftFills:rightFills:lineWidths:lineFills:fillIndexList:pointShort:`.
    #[allow(clippy::too_many_arguments)]
    pub fn loadCompressedShapesegmentsleftFillsrightFillslineWidthslineFillsfillIndexListpointShort(
        &mut self,
        points: PointsRef,
        n_segments: SqInt,
        left_fills: PointsRef,
        right_fills: PointsRef,
        line_widths: PointsRef,
        line_fills: PointsRef,
        fill_index_list: PointsRef,
        points_short: bool,
    ) {
        if n_segments == 0 {
            return;
        }
        let mut left_run: SqInt = -1;
        let mut right_run: SqInt = -1;
        let mut width_run: SqInt = -1;
        let mut line_fill_run: SqInt = -1;
        let mut left_length: SqInt = 1;
        let mut right_length: SqInt = 1;
        let mut width_length: SqInt = 1;
        let mut line_fill_length: SqInt = 1;
        let mut left_value: SqInt = 0;
        let mut right_value: SqInt = 0;
        let mut width_value: SqInt = 0;
        let mut line_fill_value: SqInt = 0;
        for i in 1..=n_segments {
            // Decrement the current run lengths and load new stuff.
            left_length -= 1;
            if left_length <= 0 {
                left_run += 1;
                let w = left_fills.int_at(left_run);
                left_length = ((w as SqInt as usize) >> 16) as SqInt;
                left_value = (w & 0xFFFF) as SqInt;
                if left_value != 0 {
                    left_value = fill_index_list.int_at(left_value - 1) as SqInt;
                    left_value = self.transformColor(left_value);
                    if self.engineStopped {
                        return;
                    }
                }
            }
            right_length -= 1;
            if right_length <= 0 {
                right_run += 1;
                let w = right_fills.int_at(right_run);
                right_length = ((w as SqInt as usize) >> 16) as SqInt;
                right_value = (w & 0xFFFF) as SqInt;
                if right_value != 0 {
                    right_value = fill_index_list.int_at(right_value - 1) as SqInt;
                    right_value = self.transformColor(right_value);
                }
            }
            width_length -= 1;
            if width_length <= 0 {
                width_run += 1;
                let w = line_widths.int_at(width_run);
                width_length = ((w as SqInt as usize) >> 16) as SqInt;
                width_value = (w & 0xFFFF) as SqInt;
                if width_value != 0 {
                    width_value = self.transformWidth(width_value);
                }
            }
            line_fill_length -= 1;
            if line_fill_length <= 0 {
                line_fill_run += 1;
                let w = line_fills.int_at(line_fill_run);
                line_fill_length = ((w as SqInt as usize) >> 16) as SqInt;
                line_fill_value = (w & 0xFFFF) as SqInt;
                if line_fill_value != 0 {
                    line_fill_value = fill_index_list.int_at(line_fill_value - 1) as SqInt;
                }
            }
            self.loadCompressedSegmentfromshortleftFillrightFilllineWidthlineColor(
                i - 1,
                points,
                points_short,
                left_value,
                right_value,
                width_value,
                line_fill_value,
            );
            if self.engineStopped {
                return;
            }
        }
    }

    /// `loadPolygon:nPoints:fill:lineWidth:lineFill:pointsShort:`.
    pub fn loadPolygonnPointsfilllineWidthlineFillpointsShort(
        &mut self,
        points: PointsRef,
        n_points: SqInt,
        fill_index: SqInt,
        line_width: SqInt,
        line_fill: SqInt,
        is_short: bool,
    ) {
        let (mut x0, mut y0);
        if is_short {
            x0 = points.short_at(0) as SqInt;
            y0 = points.short_at(1) as SqInt;
        } else {
            x0 = points.int_at(0) as SqInt;
            y0 = points.int_at(1) as SqInt;
        }
        for i in 1..n_points {
            let (x1, y1);
            if is_short {
                x1 = points.short_at(i * 2) as SqInt;
                y1 = points.short_at((i * 2) + 1) as SqInt;
            } else {
                x1 = points.int_at(i * 2) as SqInt;
                y1 = points.int_at((i * 2) + 1) as SqInt;
            }
            self.point_put(GWPoint1, x0 as i32, y0 as i32);
            self.point_put(GWPoint2, x1 as i32, y1 as i32);
            self.transformPoints(2);
            self.loadWideLinefromtolineFillleftFillrightFill(
                line_width, GWPoint1, GWPoint2, line_fill, fill_index, 0,
            );
            if self.engineStopped {
                return;
            }
            x0 = x1;
            y0 = y1;
        }
    }

    // --- ovals and rectangles -----------------------------------------------

    /// `loadOvalSegment:w:h:cx:cy:` — one of 16 quadratic segments into the
    /// header point slots.
    pub fn loadOvalSegmentwhcxcy(&mut self, seg: SqInt, w: SqInt, h: SqInt, cx: SqInt, cy: SqInt) {
        let s = seg as usize;
        // Load start point of segment.
        let x0 = ((CIRCLE_COS_TABLE[s * 2] * w as f64) + cx as f64) as SqInt;
        let y0 = ((CIRCLE_SIN_TABLE[s * 2] * h as f64) + cy as f64) as SqInt;
        self.point_put(GWPoint1, x0 as i32, y0 as i32);
        let x2 = ((CIRCLE_COS_TABLE[s * 2 + 2] * w as f64) + cx as f64) as SqInt;
        let y2 = ((CIRCLE_SIN_TABLE[s * 2 + 2] * h as f64) + cy as f64) as SqInt;
        self.point_put(GWPoint3, x2 as i32, y2 as i32);
        // NOTE (from the C): the intermediate point is the point ON the curve
        // and not yet the control point (which is OFF the curve).
        let mut x1 = ((CIRCLE_COS_TABLE[s * 2 + 1] * w as f64) + cx as f64) as SqInt;
        let mut y1 = ((CIRCLE_SIN_TABLE[s * 2 + 1] * h as f64) + cy as f64) as SqInt;
        x1 = (x1 * 2) - ((x0 + x2) / 2);
        y1 = (y1 * 2) - ((y0 + y2) / 2);
        self.point_put(GWPoint2, x1 as i32, y1 as i32);
    }

    /// `loadOval:lineFill:leftFill:rightFill:` — 16 bezier segments from the
    /// rectangle in point1/point2.
    pub fn loadOvallineFillleftFillrightFill(
        &mut self,
        line_width: SqInt,
        line_fill: SqInt,
        left_fill: SqInt,
        right_fill: SqInt,
    ) {
        let w = (self.point_x(GWPoint2).wrapping_sub(self.point_x(GWPoint1))) / 2;
        let h = (self.point_y(GWPoint2).wrapping_sub(self.point_y(GWPoint1))) / 2;
        let cx = (self.point_x(GWPoint2).wrapping_add(self.point_x(GWPoint1))) / 2;
        let cy = (self.point_y(GWPoint2).wrapping_add(self.point_y(GWPoint1))) / 2;
        for i in 0..=15 {
            self.loadOvalSegmentwhcxcy(i, w as SqInt, h as SqInt, cx as SqInt, cy as SqInt);
            self.transformPoints(3);
            let n_segments = self.loadAndSubdivideBezierFromviatoisWide(
                GWPoint1,
                GWPoint2,
                GWPoint3,
                (line_width != 0) && (line_fill != 0),
            );
            if self.engineStopped {
                return;
            }
            self.loadWideBezierlineFillleftFillrightFilln(
                line_width, line_fill, left_fill, right_fill, n_segments,
            );
            if self.engineStopped {
                return;
            }
        }
    }

    /// `loadRectangle:lineFill:leftFill:rightFill:` — four lines from the
    /// transformed corner points.
    pub fn loadRectanglelineFillleftFillrightFill(
        &mut self,
        line_width: SqInt,
        line_fill: SqInt,
        left_fill: SqInt,
        right_fill: SqInt,
    ) {
        self.loadWideLinefromtolineFillleftFillrightFill(
            line_width, GWPoint1, GWPoint2, line_fill, left_fill, right_fill,
        );
        self.loadWideLinefromtolineFillleftFillrightFill(
            line_width, GWPoint2, GWPoint3, line_fill, left_fill, right_fill,
        );
        self.loadWideLinefromtolineFillleftFillrightFill(
            line_width, GWPoint3, GWPoint4, line_fill, left_fill, right_fill,
        );
        self.loadWideLinefromtolineFillleftFillrightFill(
            line_width, GWPoint4, GWPoint1, line_fill, left_fill, right_fill,
        );
    }

    // --- oriented fills -----------------------------------------------------

    /// `loadFillOrientation:from:along:normal:width:height:` — transforms the
    /// three points in place and derives the fixed-point direction/normal.
    pub fn loadFillOrientationfromalongnormalwidthheight(
        &mut self,
        fill: SqInt,
        point1: SqInt,
        point2: SqInt,
        point3: SqInt,
        fill_width: SqInt,
        fill_height: SqInt,
    ) {
        let v = self.point_x(point2).wrapping_add(self.point_x(point1));
        let w = self.point_y(point2).wrapping_add(self.point_y(point1));
        self.point_put(point2, v, w);
        let v = self.point_x(point3).wrapping_add(self.point_x(point1));
        let w = self.point_y(point3).wrapping_add(self.point_y(point1));
        self.point_put(point3, v, w);
        self.transformPoint(point1);
        self.transformPoint(point2);
        self.transformPoint(point3);
        let dir_x = self.point_x(point2).wrapping_sub(self.point_x(point1));
        let dir_y = self.point_y(point2).wrapping_sub(self.point_y(point1));
        let nrm_x = self.point_x(point3).wrapping_sub(self.point_x(point1));
        let nrm_y = self.point_y(point3).wrapping_sub(self.point_y(point1));
        // Compute the scale from direction/normal into ramp size, in 32-bit
        // ints as the C declares them.
        let ds_length2 = dir_x.wrapping_mul(dir_x).wrapping_add(dir_y.wrapping_mul(dir_y));
        let (ds_x, ds_y) = if ds_length2 > 0 {
            (
                ((dir_x as f64 * fill_width as f64 * 65536.0) / ds_length2 as f64) as SqInt,
                ((dir_y as f64 * fill_width as f64 * 65536.0) / ds_length2 as f64) as SqInt,
            )
        } else {
            (0, 0)
        };
        let dt_length2 = nrm_x.wrapping_mul(nrm_x).wrapping_add(nrm_y.wrapping_mul(nrm_y));
        let (dt_x, dt_y) = if dt_length2 > 0 {
            (
                ((nrm_x as f64 * fill_height as f64 * 65536.0) / dt_length2 as f64) as SqInt,
                ((nrm_y as f64 * fill_height as f64 * 65536.0) / dt_length2 as f64) as SqInt,
            )
        } else {
            (0, 0)
        };
        let v = self.point_x(point1) as SqInt;
        self.objatput(fill, GFOriginX, v);
        let v = self.point_y(point1) as SqInt;
        self.objatput(fill, GFOriginY, v);
        self.objatput(fill, GFDirectionX, ds_x);
        self.objatput(fill, GFDirectionY, ds_y);
        self.objatput(fill, GFNormalX, dt_x);
        self.objatput(fill, GFNormalY, dt_y);
    }

    /// `allocateGradientFill:rampWidth:isRadial:` — copies the (possibly
    /// color-transformed) ramp into the new fill object.
    pub fn allocateGradientFillrampWidthisRadial(
        &mut self,
        ramp: PointsRef,
        ramp_width: SqInt,
        is_radial: bool,
    ) -> SqInt {
        let fill_size = GGBaseSize + ramp_width;
        if !self.allocateObjEntry(fill_size) {
            return 0;
        }
        let fill = self.objUsed;
        self.objUsed = fill + fill_size;
        self.objatput(
            fill,
            GEObjectType,
            if is_radial {
                GEPrimitiveRadialGradientFill
            } else {
                GEPrimitiveLinearGradientFill
            },
        );
        self.objatput(fill, GEObjectIndex, 0);
        self.objatput(fill, GEObjectLength, fill_size);
        if self.wb_at(GWHasColorTransform) != 0 {
            for i in 0..ramp_width {
                let v = self.transformColor(ramp.int_at(i) as SqInt);
                self.objatput(fill, GFRampOffset + i, v);
            }
        } else {
            for i in 0..ramp_width {
                self.objatput(fill, GFRampOffset + i, ramp.int_at(i) as SqInt);
            }
        }
        self.objatput(fill, GFRampLength, ramp_width);
        fill
    }

    /// `allocateBitmapFill:colormap:` — copies the (possibly transformed)
    /// colormap into the new fill object. `cm` is empty for no colormap.
    pub fn allocateBitmapFillcolormap(&mut self, cm_size: SqInt, cm: Option<PointsRef>) -> SqInt {
        let fill_size = GBMBaseSize + cm_size;
        if !self.allocateObjEntry(fill_size) {
            return 0;
        }
        let fill = self.objUsed;
        self.objUsed = fill + fill_size;
        self.objatput(fill, GEObjectType, GEPrimitiveClippedBitmapFill);
        self.objatput(fill, GEObjectIndex, 0);
        self.objatput(fill, GEObjectLength, fill_size);
        if let Some(cm) = cm {
            if self.wb_at(GWHasColorTransform) != 0 {
                for i in 0..cm_size {
                    let v = self.transformColor(cm.int_at(i) as SqInt);
                    self.objatput(fill, GBColormapOffset + i, v);
                }
            } else {
                for i in 0..cm_size {
                    self.objatput(fill, GBColormapOffset + i, cm.int_at(i) as SqInt);
                }
            }
        }
        self.objatput(fill, GBColormapSize, cm_size);
        fill
    }
}
