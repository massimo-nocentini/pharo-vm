//! Edge machinery: the global and active edge tables, and the fixed-point
//! stepping of lines, beziers and their wide variants.
//!
//! Mirrors `BalloonEngineBase`/`BalloonEnginePlugin` edge methods from the
//! generated C. Local variables keep the C's declared widths: `sqInt` locals
//! are `SqInt`, `int` locals are `i32` with wrapping arithmetic (the C relies
//! on 32-bit wraparound in the fixed-point update loops).

#![allow(non_snake_case)]

use crate::consts::*;
use crate::engine::{absoluteSquared8Dot24, Engine, Host, SqInt};

impl<H: Host> Engine<H> {
    // --- global edge table --------------------------------------------------

    /// `addEdgeToGET:`.
    pub fn addEdgeToGET(&mut self, edge: SqInt) {
        if !self.allocateGETEntry(1) {
            return;
        }
        let used = self.wb_at(GWGETUsed);
        self.get_put(used, edge);
        self.wb_put(GWGETUsed, used + 1);
    }

    /// `checkedAddLineToGET:` — clip-rejects before adding.
    pub fn checkedAddLineToGET(&mut self, line: SqInt) {
        let line_width = if self.isWide(line) {
            self.objat(line, GLWideExtent)
        } else {
            0
        };
        if self.objat(line, GLEndY) + line_width < self.wb_at(GWFillMinY) {
            return;
        }
        if (self.objat(line, GEXValue) - line_width >= self.wb_at(GWFillMaxX))
            && (self.objat(line, GLEndX) - line_width >= self.wb_at(GWFillMaxX))
        {
            return;
        }
        self.addEdgeToGET(line);
    }

    /// `checkedAddBezierToGET:`.
    pub fn checkedAddBezierToGET(&mut self, bezier: SqInt) {
        let line_width = if self.isWide(bezier) {
            self.objat(bezier, GBWideExtent)
        } else {
            0
        };
        if self.objat(bezier, GBEndY) + line_width < self.wb_at(GWFillMinY) {
            return;
        }
        if (self.objat(bezier, GEXValue) - line_width >= self.wb_at(GWFillMaxX))
            && (self.objat(bezier, GBEndX) - line_width >= self.wb_at(GWFillMaxX))
        {
            return;
        }
        self.addEdgeToGET(bezier);
    }

    /// `checkedAddEdgeToGET:`.
    pub fn checkedAddEdgeToGET(&mut self, edge: SqInt) {
        let type_ = self.objectTypeOf(edge);
        if (type_ & GEPrimitiveWideMask) == GEPrimitiveLine {
            self.checkedAddLineToGET(edge);
            return;
        }
        if (type_ & GEPrimitiveWideMask) == GEPrimitiveBezier {
            self.checkedAddBezierToGET(edge);
            return;
        }
        self.addEdgeToGET(edge);
    }

    /// `createGlobalEdgeTable`.
    ///
    /// Walks the object buffer record by record. Like the C, a corrupt record
    /// with length 0 would loop forever; the object buffer is engine-built,
    /// so lengths are always positive.
    pub fn createGlobalEdgeTable(&mut self) {
        let mut object = 0;
        let end = self.objUsed;
        while object < end {
            // addEdgeToGET: may fail on insufficient space, but that is not a
            // problem here (the C says so too).
            if self.isEdge(object) {
                // Only add edges that start above fillMaxY.
                if self.objat(object, GEYValue) < self.wb_at(GWFillMaxY) {
                    self.checkedAddEdgeToGET(object);
                }
            }
            object += self.objat(object, GEObjectLength);
        }
    }

    /// `getSorts:before:` — should `edge1` sort before `edge2`?
    pub fn getSortsbefore(&self, edge1: SqInt, edge2: SqInt) -> bool {
        if edge1 == edge2 {
            return true;
        }
        let diff = self.objat(edge1, GEYValue) - self.objat(edge2, GEYValue);
        if diff != 0 {
            return diff < 0;
        }
        let diff = self.objat(edge1, GEXValue) - self.objat(edge2, GEXValue);
        diff < 0
    }

    /// `quickSortGlobalEdgeTable:from:to:` — always on `getBuffer`.
    pub fn quickSortGlobalEdgeTablefromto(&mut self, i: SqInt, j: SqInt) {
        let n = (j + 1) - i;
        if n <= 1 {
            return;
        }
        let mut di = self.get_at(i) as i32;
        let mut dj = self.get_at(j) as i32;
        let mut before = self.getSortsbefore(di as SqInt, dj as SqInt);
        if !before {
            let tmp = self.get_at(i);
            let tj = self.get_at(j);
            self.get_put(i, tj);
            self.get_put(j, tmp);
            core::mem::swap(&mut di, &mut dj);
        }
        if n <= 2 {
            return;
        }
        let ij = (i + j) / 2;
        let mut dij = self.get_at(ij) as i32;
        before = self.getSortsbefore(di as SqInt, dij as SqInt);
        if before {
            before = self.getSortsbefore(dij as SqInt, dj as SqInt);
            if !before {
                let tmp = self.get_at(j);
                let tij = self.get_at(ij);
                self.get_put(j, tij);
                self.get_put(ij, tmp);
                dij = dj;
            }
        } else {
            let tmp = self.get_at(i);
            let tij = self.get_at(ij);
            self.get_put(i, tij);
            self.get_put(ij, tmp);
            dij = di;
        }
        if n <= 3 {
            return;
        }
        let mut k = i;
        let mut l = j;
        let mut again = true;
        while again {
            before = true;
            while before {
                l -= 1;
                if k <= l {
                    let tmp = self.get_at(l) as i32;
                    before = self.getSortsbefore(dij as SqInt, tmp as SqInt);
                } else {
                    before = false;
                }
            }
            before = true;
            while before {
                k += 1;
                if k <= l {
                    let tmp = self.get_at(k) as i32;
                    before = self.getSortsbefore(tmp as SqInt, dij as SqInt);
                } else {
                    before = false;
                }
            }
            again = k <= l;
            if again {
                let tmp = self.get_at(k);
                let tl = self.get_at(l);
                self.get_put(k, tl);
                self.get_put(l, tmp);
            }
        }
        self.quickSortGlobalEdgeTablefromto(i, l);
        self.quickSortGlobalEdgeTablefromto(k, j);
    }

    // --- active edge table --------------------------------------------------

    /// `indexForInsertingIntoAET:`.
    pub fn indexForInsertingIntoAET(&self, edge: SqInt) -> SqInt {
        let initial_x = self.objat(edge, GEXValue);
        let mut index = 0;
        while (index < self.wb_at(GWAETUsed))
            && (self.objat(self.aet_at(index), GEXValue) < initial_x)
        {
            index += 1;
        }
        while (index < self.wb_at(GWAETUsed))
            && ((self.objat(self.aet_at(index), GEXValue) == initial_x)
                && self.getSortsbefore(self.aet_at(index), edge))
        {
            index += 1;
        }
        index
    }

    /// `insertToAET:beforeIndex:`.
    pub fn insertToAETbeforeIndex(&mut self, edge: SqInt, index: SqInt) {
        if !self.needAvailableSpace(1) {
            return;
        }
        let mut i = self.wb_at(GWAETUsed) - 1;
        while i >= index {
            let v = self.aet_at(i);
            self.aet_put(i + 1, v);
            i -= 1;
        }
        self.aet_put(index, edge);
        let used = self.wb_at(GWAETUsed);
        self.wb_put(GWAETUsed, used + 1);
    }

    /// `insertEdgeIntoAET:`.
    pub fn insertEdgeIntoAET(&mut self, edge: SqInt) {
        if self.objat(edge, GENumLines) <= 0 {
            return;
        }
        let index = self.indexForInsertingIntoAET(edge);
        self.insertToAETbeforeIndex(edge, index);
    }

    /// `moveAETEntryFrom:edge:x:` — bubble the entry left to its position.
    pub fn moveAETEntryFromedgex(&mut self, index: SqInt, edge: SqInt, x_value: SqInt) {
        let mut new_index = index;
        while (new_index > 0) && (self.objat(self.aet_at(new_index - 1), GEXValue) > x_value) {
            let v = self.aet_at(new_index - 1);
            self.aet_put(new_index, v);
            new_index -= 1;
        }
        self.aet_put(new_index, edge);
    }

    /// `removeFirstAETEntry`.
    pub fn removeFirstAETEntry(&mut self) {
        let mut index = self.wb_at(GWAETStart);
        let value = self.wb_at(GWAETUsed) - 1;
        self.wb_put(GWAETUsed, value);
        while index < self.wb_at(GWAETUsed) {
            let v = self.aet_at(index + 1);
            self.aet_put(index, v);
            index += 1;
        }
    }

    /// `resortFirstAETEntry`.
    pub fn resortFirstAETEntry(&mut self) {
        if self.wb_at(GWAETStart) == 0 {
            return;
        }
        let edge = self.aet_at(self.wb_at(GWAETStart)) as i32;
        let x_value = self.objat(edge as SqInt, GEXValue);
        let left_edge = self.aet_at(self.wb_at(GWAETStart) - 1) as i32;
        if self.objat(left_edge as SqInt, GEXValue) <= x_value {
            return;
        }
        let start = self.wb_at(GWAETStart);
        self.moveAETEntryFromedgex(start, edge as SqInt, x_value);
    }

    // --- line stepping ------------------------------------------------------

    /// `stepToFirstLineIn:at:` — Bresenham setup, then catch up to `yValue`.
    pub fn stepToFirstLineInat(&mut self, line: SqInt, y_value: SqInt) {
        // Quick check if there is anything at all to do.
        if !self.isWide(line) && (y_value >= self.objat(line, GLEndY)) {
            self.objatput(line, GENumLines, 0);
            return;
        }
        let delta_x = self.objat(line, GLEndX) - self.objat(line, GEXValue);
        let delta_y = self.objat(line, GLEndY) - self.objat(line, GEYValue);
        let (x_dir, width_x, mut error);
        if delta_x >= 0 {
            x_dir = 1;
            width_x = delta_x;
            error = 0;
        } else {
            x_dir = -1;
            width_x = 0 - delta_x;
            error = 1 - delta_y;
        }
        let (x_inc, error_adj_up);
        if delta_y == 0 {
            // No error for horizontal edges; xInc encodes width and direction.
            error = 0;
            x_inc = delta_x;
            error_adj_up = 0;
        } else if delta_y > width_x {
            // y-major. (The C notes the '>' instead of '>=' could matter.)
            x_inc = 0;
            error_adj_up = width_x;
        } else {
            x_inc = (width_x / delta_y) * x_dir;
            error_adj_up = width_x % delta_y;
        }
        self.objatput(line, GENumLines, delta_y);
        self.objatput(line, GLXDirection, x_dir);
        self.objatput(line, GLXIncrement, x_inc);
        self.objatput(line, GLError, error);
        self.objatput(line, GLErrorAdjUp, error_adj_up);
        self.objatput(line, GLErrorAdjDown, delta_y);
        let start_y = self.objat(line, GEYValue);
        if start_y != y_value {
            for _i in start_y..y_value {
                self.stepToNextLineInat(line, 0);
            }
            self.objatput(line, GENumLines, delta_y - (y_value - start_y));
        }
    }

    /// `stepToNextLineIn:at:` — one Bresenham step (yValue is unused in the C
    /// too).
    pub fn stepToNextLineInat(&mut self, line: SqInt, _y_value: SqInt) {
        let mut x = self.objat(line, GEXValue) + self.objat(line, GLXIncrement);
        let mut err = self.objat(line, GLError) + self.objat(line, GLErrorAdjUp);
        if err > 0 {
            x += self.objat(line, GLXDirection);
            err -= self.objat(line, GLErrorAdjDown);
        }
        self.objatput(line, GLError, err);
        self.objatput(line, GEXValue, x);
    }

    // --- bezier stepping ----------------------------------------------------

    /// `stepToNextBezierForward:at:` — the shared forward-differencing step.
    /// `ud` is the object-buffer index of the 6-word update block
    /// (`bezier + GBUpdateData` or `bezier + GBWideUpdateData`). Answers the
    /// new x value (`lastX >> 8`).
    pub fn stepToNextBezierForwardat(&mut self, ud: SqInt, y_value: SqInt) -> SqInt {
        let mut last_x = self.objat(ud, GBUpdateX) as i32;
        let mut last_y = self.objat(ud, GBUpdateY) as i32;
        let mut fw_dx = self.objat(ud, GBUpdateDX) as i32;
        let mut fw_dy = self.objat(ud, GBUpdateDY) as i32;
        // Step until minY, and only while fwDy steps downward; the C notes the
        // fwDy test should not be needed in theory but is insurance.
        let min_y = y_value * 256;
        while (min_y > last_y as SqInt) && (fw_dy >= 0) {
            last_x = last_x.wrapping_add(fw_dx.wrapping_add(32768) >> 16);
            last_y = last_y.wrapping_add(fw_dy.wrapping_add(32768) >> 16);
            fw_dx = fw_dx.wrapping_add(self.objat(ud, GBUpdateDDX) as i32);
            fw_dy = fw_dy.wrapping_add(self.objat(ud, GBUpdateDDY) as i32);
        }
        self.objatput(ud, GBUpdateX, last_x as SqInt);
        self.objatput(ud, GBUpdateY, last_y as SqInt);
        self.objatput(ud, GBUpdateDX, fw_dx as SqInt);
        self.objatput(ud, GBUpdateDY, fw_dy as SqInt);
        (last_x >> 8) as SqInt
    }

    /// `stepToNextBezierIn:at:`.
    pub fn stepToNextBezierInat(&mut self, bezier: SqInt, y_value: SqInt) {
        let x_value = self.stepToNextBezierForwardat(bezier + GBUpdateData, y_value);
        self.objatput(bezier, GEXValue, x_value);
    }

    /// `stepToFirstBezierIn:at:` — integer forward-differencing setup.
    pub fn stepToFirstBezierInat(&mut self, bezier: SqInt, y_value: SqInt) {
        // Quick check if there is anything at all to do.
        if !self.isWide(bezier) && (y_value >= self.objat(bezier, GBEndY)) {
            self.objatput(bezier, GENumLines, 0);
            return;
        }
        let start_x = self.objat(bezier, GEXValue);
        let start_y = self.objat(bezier, GEYValue);
        let via_x = self.objat(bezier, GBViaX);
        let via_y = self.objat(bezier, GBViaY);
        let end_x = self.objat(bezier, GBEndX);
        let end_y = self.objat(bezier, GBEndY);
        let delta_y = end_y - start_y;
        let fw_x1 = (via_x - start_x) * 2;
        let fw_x2 = (start_x + end_x) - (via_x * 2);
        let fw_y1 = (via_y - start_y) * 2;
        let fw_y2 = (start_y + end_y) - (via_y * 2);
        let mut max_steps = delta_y * 2;
        if max_steps < 2 {
            max_steps = 2;
        }
        let scaled_step_size = 0x1000000 / max_steps;
        let squared_step_size = absoluteSquared8Dot24(scaled_step_size);
        let mut fw_dx = fw_x1 * scaled_step_size;
        let fw_ddx = (fw_x2 * squared_step_size) * 2;
        fw_dx += fw_ddx / 2;
        let mut fw_dy = fw_y1 * scaled_step_size;
        let fw_ddy = (fw_y2 * squared_step_size) * 2;
        fw_dy += fw_ddy / 2;
        self.objatput(bezier, GENumLines, delta_y);
        let ud = bezier + GBUpdateData;
        self.objatput(ud, GBUpdateX, start_x * 256);
        self.objatput(ud, GBUpdateY, start_y * 256);
        self.objatput(ud, GBUpdateDX, fw_dx);
        self.objatput(ud, GBUpdateDY, fw_dy);
        self.objatput(ud, GBUpdateDDX, fw_ddx);
        self.objatput(ud, GBUpdateDDY, fw_ddy);
        let start_y = self.objat(bezier, GEYValue);
        if start_y != y_value {
            self.stepToNextBezierInat(bezier, y_value);
            self.objatput(bezier, GENumLines, delta_y - (y_value - start_y));
        }
    }

    // --- wide line stepping -------------------------------------------------

    /// `adjustWideLine:afterSteppingFrom:to:` — simulates a rectangular
    /// brush by adjusting width and start position.
    pub fn adjustWideLineafterSteppingFromto(&mut self, line: SqInt, last_x: SqInt, next_x: SqInt) {
        let y_entry = self.objat(line, GLWideEntry);
        let y_exit = self.objat(line, GLWideExit);
        let base_width = self.objat(line, GLWideExtent);
        let line_offset = base_width / 2;
        let mut line_width = self.objat(line, GLWideWidth);
        let x_dir = self.objat(line, GLXDirection);
        let delta_x = next_x - last_x;
        if y_entry < base_width {
            if x_dir < 0 {
                line_width -= delta_x; // effectively adding
            } else {
                line_width += delta_x;
                self.objatput(line, GEXValue, last_x);
            }
        }
        if (y_exit + line_offset) == 0 {
            if x_dir > 0 {
                line_width -= self.objat(line, GLXIncrement);
            } else {
                line_width += self.objat(line, GLXIncrement); // effectively subtracting
                self.objatput(line, GEXValue, last_x);
            }
        }
        if (y_exit + line_offset) > 0 {
            if x_dir < 0 {
                line_width += delta_x; // effectively subtracting
                self.objatput(line, GEXValue, last_x);
            } else {
                line_width -= delta_x;
            }
        }
        self.objatput(line, GLWideWidth, line_width);
    }

    /// `stepToNextWideLineIn:at:`.
    pub fn stepToNextWideLineInat(&mut self, line: SqInt, _y_value: SqInt) {
        let y_entry = self.objat(line, GLWideEntry) + 1;
        let y_exit = self.objat(line, GLWideExit) + 1;
        self.objatput(line, GLWideEntry, y_entry);
        self.objatput(line, GLWideExit, y_exit);
        let line_width = self.objat(line, GLWideExtent);
        let line_offset = line_width / 2;
        if y_entry >= line_offset {
            self.edgeFillsValidate(line);
        }
        if y_exit >= 0 {
            self.edgeFillsInvalidate(line);
        }
        let last_x = self.objat(line, GEXValue);
        self.stepToNextLineInat(line, 0);
        let next_x = self.objat(line, GEXValue);
        if (y_entry <= line_width) || ((y_exit + line_offset) >= 0) {
            self.adjustWideLineafterSteppingFromto(line, last_x, next_x);
        }
    }

    /// `stepToFirstWideLineIn:at:`.
    pub fn stepToFirstWideLineInat(&mut self, line: SqInt, y_value: SqInt) {
        let line_width = self.objat(line, GLWideExtent);
        let line_offset = line_width / 2;
        let start_x = self.objat(line, GEXValue);
        let start_y = self.objat(line, GEYValue);
        self.stepToFirstLineInat(line, start_y);
        let n_lines = self.objat(line, GENumLines);
        let x_dir = self.objat(line, GLXDirection);
        self.objatput(line, GEXValue, start_x - line_offset);
        self.objatput(line, GENumLines, n_lines + line_width);
        if x_dir > 0 {
            let w = self.objat(line, GLXIncrement) + line_width;
            self.objatput(line, GLWideWidth, w);
        } else {
            let w = line_width - self.objat(line, GLXIncrement);
            self.objatput(line, GLWideWidth, w);
            let v = self.objat(line, GEXValue) + self.objat(line, GLXIncrement);
            self.objatput(line, GEXValue, v);
        }
        let y_entry = 0; // turned on at lineOffset
        let y_exit = (0 - n_lines) - line_offset; // turned off at zero
        self.objatput(line, GLWideEntry, y_entry);
        self.objatput(line, GLWideExit, y_exit);
        if (y_entry >= line_offset) && (y_exit < 0) {
            self.edgeFillsValidate(line);
        } else {
            self.edgeFillsInvalidate(line);
        }
        if start_y != y_value {
            for _i in start_y..y_value {
                self.stepToNextWideLineInat(line, 0);
            }
            let v = self.objat(line, GENumLines) - (y_value - start_y);
            self.objatput(line, GENumLines, v);
        }
    }

    // --- wide bezier stepping -----------------------------------------------

    /// `computeFinalWideBezierValues:width:`.
    pub fn computeFinalWideBezierValueswidth(&mut self, bezier: SqInt, line_width: SqInt) {
        let mut left_x = (self.objat(bezier + GBUpdateData, GBUpdateX) as i32) / 256;
        let mut right_x = (self.objat(bezier + GBWideUpdateData, GBUpdateX) as i32) / 256;
        if left_x > right_x {
            core::mem::swap(&mut left_x, &mut right_x);
        }
        self.objatput(bezier, GEXValue, left_x as SqInt);
        if (right_x - left_x) as SqInt > line_width {
            self.objatput(bezier, GBWideWidth, (right_x - left_x) as SqInt);
        } else {
            self.objatput(bezier, GBWideWidth, line_width);
        }
    }

    /// `adjustWideBezierLeft:width:offset:endX:` (dx < 0).
    pub fn adjustWideBezierLeftwidthoffsetendX(
        &mut self,
        bezier: SqInt,
        line_width: SqInt,
        line_offset: SqInt,
        end_x: SqInt,
    ) {
        let v = self.objat(bezier + GBUpdateData, GBUpdateX) - (line_offset * 256);
        self.objatput(bezier + GBUpdateData, GBUpdateX, v);
        let last_x = self.objat(bezier + GBWideUpdateData, GBUpdateX) as i32;
        self.objatput(
            bezier + GBWideUpdateData,
            GBUpdateX,
            last_x as SqInt + ((line_width - line_offset) * 256),
        );
        let last_y = self.objat(bezier + GBWideUpdateData, GBUpdateY) as i32;
        self.objatput(
            bezier + GBWideUpdateData,
            GBUpdateY,
            last_y as SqInt + (line_width * 256),
        );
        self.objatput(bezier, GBFinalX, end_x - line_offset);
    }

    /// `adjustWideBezierRight:width:offset:endX:` (dx >= 0).
    pub fn adjustWideBezierRightwidthoffsetendX(
        &mut self,
        bezier: SqInt,
        line_width: SqInt,
        line_offset: SqInt,
        end_x: SqInt,
    ) {
        let v = self.objat(bezier + GBUpdateData, GBUpdateX) + (line_offset * 256);
        self.objatput(bezier + GBUpdateData, GBUpdateX, v);
        let last_x = self.objat(bezier + GBWideUpdateData, GBUpdateX) as i32;
        self.objatput(
            bezier + GBWideUpdateData,
            GBUpdateX,
            last_x as SqInt - ((line_width - line_offset) * 256),
        );
        // Set lineWidth pixels down.
        let last_y = self.objat(bezier + GBWideUpdateData, GBUpdateY) as i32;
        self.objatput(
            bezier + GBWideUpdateData,
            GBUpdateY,
            last_y as SqInt + (line_width * 256),
        );
        self.objatput(bezier, GBFinalX, (end_x - line_offset) + line_width);
    }

    /// `stepToNextWideBezierIn:at:`.
    pub fn stepToNextWideBezierInat(&mut self, bezier: SqInt, y_value: SqInt) {
        let line_width = self.objat(bezier, GBWideExtent);
        let line_offset = line_width / 2;
        let y_entry = self.objat(bezier, GBWideEntry) + 1;
        let y_exit = self.objat(bezier, GBWideExit) + 1;
        self.objatput(bezier, GBWideEntry, y_entry);
        self.objatput(bezier, GBWideExit, y_exit);
        if y_entry >= line_offset {
            self.edgeFillsValidate(bezier);
        }
        if y_exit >= 0 {
            self.edgeFillsInvalidate(bezier);
        }
        if (y_exit + line_offset) < 0 {
            self.stepToNextBezierForwardat(bezier + GBUpdateData, y_value);
        } else {
            // Adjust the last x value to the final x recorded previously.
            let v = self.objat(bezier, GBFinalX) * 256;
            self.objatput(bezier + GBUpdateData, GBUpdateX, v);
        }
        self.stepToNextBezierForwardat(bezier + GBWideUpdateData, y_value);
        self.computeFinalWideBezierValueswidth(bezier, line_width);
    }

    /// `stepToNextWideBezier` — the AET-based no-argument variant used by the
    /// update dispatch.
    pub fn stepToNextWideBezier(&mut self) {
        let bezier = self.aet_at(self.wb_at(GWAETStart));
        let y = self.wb_at(GWCurrentY);
        self.stepToNextWideBezierInat(bezier, y);
    }

    /// `stepToFirstWideBezierIn:at:`.
    pub fn stepToFirstWideBezierInat(&mut self, bezier: SqInt, y_value: SqInt) {
        let line_width = self.objat(bezier, GBWideExtent);
        let line_offset = line_width / 2;
        let end_x = self.objat(bezier, GBEndX);
        let start_y = self.objat(bezier, GEYValue);
        self.stepToFirstBezierInat(bezier, start_y);
        let n_lines = self.objat(bezier, GENumLines);
        for i in 0..=5 {
            let v = self.objat(bezier + GBUpdateData, i);
            self.objatput(bezier + GBWideUpdateData, i, v);
        }
        let mut x_dir = self.objat(bezier + GBUpdateData, GBUpdateDX) as i32;
        if x_dir == 0 {
            x_dir = self.objat(bezier + GBUpdateData, GBUpdateDDX) as i32;
        }
        x_dir = if x_dir >= 0 { 1 } else { -1 };
        if x_dir < 0 {
            self.adjustWideBezierLeftwidthoffsetendX(bezier, line_width, line_offset, end_x);
        } else {
            self.adjustWideBezierRightwidthoffsetendX(bezier, line_width, line_offset, end_x);
        }
        if n_lines == 0 {
            let v = self.objat(bezier, GBFinalX) * 256;
            self.objatput(bezier + GBUpdateData, GBUpdateX, v);
        }
        self.objatput(bezier, GENumLines, n_lines + line_width);
        let y_entry = 0; // turned on at lineOffset
        let y_exit = (0 - n_lines) - line_offset; // turned off at zero
        self.objatput(bezier, GBWideEntry, y_entry);
        self.objatput(bezier, GBWideExit, y_exit);
        if (y_entry >= line_offset) && (y_exit < 0) {
            self.edgeFillsValidate(bezier);
        } else {
            self.edgeFillsInvalidate(bezier);
        }
        self.computeFinalWideBezierValueswidth(bezier, line_width);
        if start_y != y_value {
            // Must single-step here so that entry/exit works.
            for i in start_y..y_value {
                self.stepToNextWideBezierInat(bezier, i);
            }
            let v = self.objat(bezier, GENumLines) - (y_value - start_y);
            self.objatput(bezier, GENumLines, v);
        }
    }
}
