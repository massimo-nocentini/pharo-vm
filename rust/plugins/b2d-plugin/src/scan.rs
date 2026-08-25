//! The scanline loop: GET/AET scanning, span display, and the resumable
//! rendering state machine.
//!
//! The engine renders a scan line in four phases (add from GET, scan AET
//! fills, blit the span buffer, update edges). Whenever it meets an entity it
//! cannot handle itself — an external edge or fill registered by the image —
//! it records a stop reason (`GErrorGETEntry` and friends) and returns to the
//! image, which services the entity and resumes through the `primitiveNext*`
//! calls. That stop-reason protocol is ABI and is mirrored exactly.

#![allow(non_snake_case)]

use crate::consts::*;
use crate::engine::{Engine, Host, SqInt};

impl<H: Host> Engine<H> {
    /// `initializeGETProcessing` — clip setup, GET creation and sorting.
    pub fn initializeGETProcessing(&mut self) {
        let level = self.wb_at(GWAALevel);
        self.setAALevel(level);
        if self.wb_at(GWClipMinX) < 0 {
            self.wb_put(GWClipMinX, 0);
        }
        if self.wb_at(GWClipMaxX) > self.wb_at(GWSpanSize) {
            let v = self.wb_at(GWSpanSize);
            self.wb_put(GWClipMaxX, v);
        }
        let shift = self.wb_at(GWAAShift) as usize;
        let v = ((self.wb_at(GWClipMinX) as usize) << shift) as SqInt;
        self.wb_put(GWFillMinX, v);
        let v = ((self.wb_at(GWClipMinY) as usize) << shift) as SqInt;
        self.wb_put(GWFillMinY, v);
        let v = ((self.wb_at(GWClipMaxX) as usize) << shift) as SqInt;
        self.wb_put(GWFillMaxX, v);
        let v = ((self.wb_at(GWClipMaxY) as usize) << shift) as SqInt;
        self.wb_put(GWFillMaxY, v);
        self.wb_put(GWGETUsed, 0);
        self.wb_put(GWAETUsed, 0);
        // Both tables start right after the objects.
        self.get = self.obj + self.objUsed as usize;
        self.aet = self.obj + self.objUsed as usize;
        self.createGlobalEdgeTable();
        if self.engineStopped {
            return;
        }
        if self.wb_at(GWGETUsed) == 0 {
            // Nothing to do.
            let v = self.wb_at(GWFillMaxY);
            self.wb_put(GWCurrentY, v);
            return;
        }
        let hi = self.wb_at(GWGETUsed) - 1;
        self.quickSortGlobalEdgeTablefromto(0, hi);
        let v = self.objat(self.get_at(0), GEYValue);
        self.wb_put(GWCurrentY, v);
        if self.wb_at(GWCurrentY) < self.wb_at(GWFillMinY) {
            let v = self.wb_at(GWFillMinY);
            self.wb_put(GWCurrentY, v);
        }
        self.wb_put(GWSpanStart, 0);
        let v = (((self.wb_at(GWSpanSize) as usize) << shift) as SqInt) - 1;
        self.wb_put(GWSpanEnd, v);
        self.clearSpanBuffer();
    }

    /// `findNextExternalEntryFromGET` — true when an external edge is next.
    pub fn findNextExternalEntryFromGET(&mut self) -> bool {
        let y_value = self.wb_at(GWCurrentY);
        while self.wb_at(GWGETStart) < self.wb_at(GWGETUsed) {
            let edge = self.get_at(self.wb_at(GWGETStart)) as i32 as SqInt;
            if self.objat(edge, GEYValue) > y_value {
                return false;
            }
            let type_ = self.objectTypeOf(edge);
            if (type_ & GEPrimitiveWideMask) == GEPrimitiveEdge {
                return true;
            }
            if !self.needAvailableSpace(1) {
                return false;
            }
            let e = self.get_at(self.wb_at(GWGETStart));
            let y = self.wb_at(GWCurrentY);
            match type_ {
                4 => self.stepToFirstLineInat(e, y),
                5 => self.stepToFirstWideLineInat(e, y),
                6 => self.stepToFirstBezierInat(e, y),
                7 => self.stepToFirstWideBezierInat(e, y),
                _ => {} // errorWrongIndex: ignored, as in the C
            }
            self.insertEdgeIntoAET(edge);
            let v = self.wb_at(GWGETStart) + 1;
            self.wb_put(GWGETStart, v);
        }
        false
    }

    /// `findNextExternalFillFromAET`.
    ///
    /// Note: the Slang inlining of `fillAllFrom:to:` discards its "external
    /// fill" answer here — an external fill records its state and rendering
    /// simply continues — so this function only ever answers false, exactly
    /// like the generated C.
    pub fn findNextExternalFillFromAET(&mut self) -> bool {
        let mut right_x = self.wb_at(GWFillMaxX);
        while self.wb_at(GWAETStart) < self.wb_at(GWAETUsed) {
            let left_edge = self.aet_at(self.wb_at(GWAETStart)) as i32 as SqInt;
            let left_x = self.objat(left_edge, GEXValue);
            right_x = left_x;
            if left_x >= self.wb_at(GWFillMaxX) {
                return false;
            }
            self.quickRemoveInvalidFillsAt(left_x);
            if self.isWide(left_edge) {
                self.toggleWideFillOf(left_edge);
            }
            if (self.objat(left_edge, GEObjectType) & GEEdgeFillsInvalid) == 0 {
                self.toggleFillsOf(left_edge);
                if self.engineStopped {
                    return false;
                }
            }
            let v = self.wb_at(GWAETStart) + 1;
            self.wb_put(GWAETStart, v);
            if self.wb_at(GWAETStart) < self.wb_at(GWAETUsed) {
                let right_edge = self.aet_at(self.wb_at(GWAETStart)) as i32 as SqInt;
                right_x = self.objat(right_edge, GEXValue);
                if right_x >= self.wb_at(GWFillMinX) {
                    // The visible portion; the "needs Smalltalk" answer is
                    // discarded (see above).
                    self.fillAllFromto(left_x, right_x);
                }
            }
        }
        if right_x < self.wb_at(GWFillMaxX) {
            let max_x = self.wb_at(GWFillMaxX);
            self.fillAllFromto(right_x, max_x);
        }
        false
    }

    /// `findNextExternalUpdateFromAET` — true when an external edge needs its
    /// update run by the image.
    pub fn findNextExternalUpdateFromAET(&mut self) -> bool {
        while self.wb_at(GWAETStart) < self.wb_at(GWAETUsed) {
            let edge = self.aet_at(self.wb_at(GWAETStart)) as i32 as SqInt;
            let count = self.objat(edge, GENumLines) - 1;
            if count == 0 {
                // Edge at end — remove it.
                self.removeFirstAETEntry();
            } else {
                // Store remaining lines back.
                self.objatput(edge, GENumLines, count);
                let type_ = self.objectTypeOf(edge);
                if (type_ & GEPrimitiveWideMask) == GEPrimitiveEdge {
                    return true;
                }
                let e = self.aet_at(self.wb_at(GWAETStart));
                let y = self.wb_at(GWCurrentY);
                match type_ {
                    4 => self.stepToNextLineInat(e, y),
                    5 => self.stepToNextWideLineInat(e, y),
                    6 => self.stepToNextBezierInat(e, y),
                    7 => self.stepToNextWideBezier(),
                    _ => {} // errorWrongIndex: ignored, as in the C
                }
                self.resortFirstAETEntry();
                let v = self.wb_at(GWAETStart) + 1;
                self.wb_put(GWAETStart, v);
            }
        }
        false
    }

    /// `displaySpanBufferAt:` — clip the span to the target and blit it.
    pub fn displaySpanBufferAt(&mut self, y: SqInt) {
        let shift = self.wb_at(GWAAShift) as usize;
        let mut target_x0 = ((self.wb_at(GWSpanStart) as usize) >> shift) as SqInt;
        if target_x0 < self.wb_at(GWClipMinX) {
            target_x0 = self.wb_at(GWClipMinX);
        }
        let mut target_x1 =
            ((((self.wb_at(GWSpanEnd) + self.wb_at(GWAALevel)) - 1) as usize) >> shift) as SqInt;
        if target_x1 > self.wb_at(GWClipMaxX) {
            target_x1 = self.wb_at(GWClipMaxX);
        }
        let target_y = ((y as usize) >> shift) as SqInt;
        if (target_y < self.wb_at(GWClipMinY))
            || (target_y >= self.wb_at(GWClipMaxY))
            || (target_x1 < self.wb_at(GWClipMinX))
            || (target_x0 >= self.wb_at(GWClipMaxX))
        {
            return;
        }
        self.host.copyBitsFromtoat(target_x0, target_x1, target_y);
    }

    /// `postDisplayAction` — decide whether rendering is complete.
    pub fn postDisplayAction(&mut self) {
        if (self.wb_at(GWGETStart) >= self.wb_at(GWGETUsed)) && (self.wb_at(GWAETUsed) == 0) {
            // No more entries to process.
            self.wb_put(GWState, GEStateCompleted);
        }
        if self.wb_at(GWCurrentY) >= self.wb_at(GWFillMaxY) {
            // Out of clipping range.
            self.wb_put(GWState, GEStateCompleted);
        }
    }

    /// `finishedProcessing`.
    pub fn finishedProcessing(&self) -> bool {
        self.wb_at(GWState) == GEStateCompleted
    }

    /// `proceedRenderingImage` — render until completed or stopped.
    pub fn proceedRenderingImage(&mut self) {
        while self.wb_at(GWState) != GEStateCompleted {
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            let external = self.findNextExternalEntryFromGET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextGETEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextGETEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateAddingFromGET);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingForEdge);
                self.stopBecauseOf(GErrorGETEntry);
                return;
            }
            self.wb_put(GWAETStart, 0);
            self.wbStackClear();
            self.wb_put(GWClearSpanBuffer, 1);
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            if (self.wb_at(GWClearSpanBuffer) != 0)
                && ((self.wb_at(GWCurrentY) & self.wb_at(GWAAScanMask)) == 0)
            {
                self.clearSpanBuffer();
            }
            self.wb_put(GWClearSpanBuffer, 0);
            let external = self.findNextExternalFillFromAET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextFillEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextFillEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateScanningAET);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingForFill);
                self.stopBecauseOf(GErrorFillEntry);
                return;
            }
            self.wbStackClear();
            self.wb_put(GWSpanEndAA, 0);
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            if (self.wb_at(GWCurrentY) & self.wb_at(GWAAScanMask)) == self.wb_at(GWAAScanMask) {
                let y = self.wb_at(GWCurrentY);
                self.displaySpanBufferAt(y);
                self.postDisplayAction();
            }
            if self.doProfileStats {
                self.incrementStatby(GWCountDisplaySpan, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeDisplaySpan, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateBlitBuffer);
                return;
            }
            if self.wb_at(GWState) == GEStateCompleted {
                return;
            }
            self.wb_put(GWAETStart, 0);
            let v = self.wb_at(GWCurrentY) + 1;
            self.wb_put(GWCurrentY, v);
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            let external = self.findNextExternalUpdateFromAET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextAETEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextAETEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateUpdateEdges);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingChange);
                self.stopBecauseOf(GErrorAETEntry);
                return;
            }
        }
    }

    /// `proceedRenderingScanline` — one scan line of the same state machine,
    /// resumable from any of its states.
    pub fn proceedRenderingScanline(&mut self) {
        let mut state = self.wb_at(GWState);
        if state == GEStateUnlocked {
            self.initializeGETProcessing();
            if self.engineStopped {
                return;
            }
            state = GEStateAddingFromGET;
        }
        if state == GEStateAddingFromGET {
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            let external = self.findNextExternalEntryFromGET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextGETEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextGETEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateAddingFromGET);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingForEdge);
                self.stopBecauseOf(GErrorGETEntry);
                return;
            }
            self.wb_put(GWAETStart, 0);
            self.wbStackClear();
            self.wb_put(GWClearSpanBuffer, 1);
            state = GEStateScanningAET;
        }
        if state == GEStateScanningAET {
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            if (self.wb_at(GWClearSpanBuffer) != 0)
                && ((self.wb_at(GWCurrentY) & self.wb_at(GWAAScanMask)) == 0)
            {
                self.clearSpanBuffer();
            }
            self.wb_put(GWClearSpanBuffer, 0);
            let external = self.findNextExternalFillFromAET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextFillEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextFillEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateScanningAET);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingForFill);
                self.stopBecauseOf(GErrorFillEntry);
                return;
            }
            state = GEStateBlitBuffer;
            self.wbStackClear();
            self.wb_put(GWSpanEndAA, 0);
        }
        if state == GEStateBlitBuffer {
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            if (self.wb_at(GWCurrentY) & self.wb_at(GWAAScanMask)) == self.wb_at(GWAAScanMask) {
                let y = self.wb_at(GWCurrentY);
                self.displaySpanBufferAt(y);
                self.postDisplayAction();
            }
            if self.doProfileStats {
                self.incrementStatby(GWCountDisplaySpan, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeDisplaySpan, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateBlitBuffer);
                return;
            }
            if self.wb_at(GWState) == GEStateCompleted {
                return;
            }
            state = GEStateUpdateEdges;
            self.wb_put(GWAETStart, 0);
            let v = self.wb_at(GWCurrentY) + 1;
            self.wb_put(GWCurrentY, v);
        }
        if state == GEStateUpdateEdges {
            if self.doProfileStats {
                self.geProfileTime = self.host.ioMicroMSecs();
            }
            let external = self.findNextExternalUpdateFromAET();
            if self.doProfileStats {
                self.incrementStatby(GWCountNextAETEntry, 1);
                let dt = self.host.ioMicroMSecs() - self.geProfileTime;
                self.incrementStatby(GWTimeNextAETEntry, dt);
            }
            if self.engineStopped {
                self.wb_put(GWState, GEStateUpdateEdges);
                return;
            }
            if external {
                self.wb_put(GWState, GEStateWaitingChange);
                self.stopBecauseOf(GErrorAETEntry);
                return;
            }
            self.wb_put(GWState, GEStateAddingFromGET);
        }
    }
}
