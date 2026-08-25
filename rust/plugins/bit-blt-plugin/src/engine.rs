//! The blitter core: clipping, mask/skew setup, and the three copy loops,
//! plus the rule-34 (alpha blend) and rule-41 (component alpha) fast paths.
//!
//! Everything here is a mechanical port of the matching C function (named in
//! each doc comment) and touches no VM state, so it can be driven directly
//! from unit tests. The `unsafe fn`s read and write raw bitmap memory through
//! the addresses in [`BitBlt`]; their shared contract is the one stated on
//! [`crate::state`]: the loader's (or the test harness's) geometry checks
//! make every access land inside the bitmaps.

use pharo_vm_plugin::sqInt;

use crate::state::{
    long32At, long32Atput, shift32, shl32, shr32, BitBlt, ALL_ONES, COLOR_MAP_NEW_STYLE,
    COLOR_MAP_PRESENT, DEFAULT_8_TO_32_TABLE, DITHER_MATRIX_4X4, MASK_TABLE,
};
use crate::rules::{alphaBlendScaledwith, dither32To16threshold};

impl BitBlt {
    /// `clipRange` -- clip and adjust source origin and extent appropriately.
    pub fn clipRange(&mut self) {
        // first in x
        if self.destX >= self.clipX {
            self.sx = self.sourceX as i32;
            self.dx = self.destX as i32;
            self.bbW = self.width as i32;
        } else {
            self.sx = (self.sourceX + (self.clipX - self.destX)) as i32;
            self.bbW = (self.width - (self.clipX - self.destX)) as i32;
            self.dx = self.clipX as i32;
        }
        if (self.dx as sqInt + self.bbW as sqInt) > self.clipX + self.clipWidth {
            self.bbW -=
                ((self.dx as sqInt + self.bbW as sqInt) - (self.clipX + self.clipWidth)) as i32;
        }
        // then in y
        if self.destY >= self.clipY {
            self.sy = self.sourceY as i32;
            self.dy = self.destY as i32;
            self.bbH = self.height as i32;
        } else {
            self.sy = ((self.sourceY + self.clipY) - self.destY) as i32;
            self.bbH = (self.height - (self.clipY - self.destY)) as i32;
            self.dy = self.clipY as i32;
        }
        if (self.dy as sqInt + self.bbH as sqInt) > self.clipY + self.clipHeight {
            self.bbH -=
                ((self.dy as sqInt + self.bbH as sqInt) - (self.clipY + self.clipHeight)) as i32;
        }
        if self.noSource {
            return;
        }
        if self.sx < 0 {
            self.dx -= self.sx;
            self.bbW += self.sx;
            self.sx = 0;
        }
        if self.sx + self.bbW > self.sourceWidth {
            self.bbW -= (self.sx + self.bbW) - self.sourceWidth;
        }
        if self.sy < 0 {
            self.dy -= self.sy;
            self.bbH += self.sy;
            self.sy = 0;
        }
        if self.sy + self.bbH > self.sourceHeight {
            self.bbH -= (self.sy + self.bbH) - self.sourceHeight;
        }
    }

    /// `destMaskAndPointerInit` -- compute masks for left and right
    /// destination words, the word count, and the starting index/delta.
    pub fn destMaskAndPointerInit(&mut self) {
        // A mask, assuming power of two
        let pixPerM1 = self.destPPW - 1;
        // how many pixels in first word / last word
        let startBits = self.destPPW - (self.dx as sqInt & pixPerM1);
        let endBits = (((self.dx as sqInt + self.bbW as sqInt) - 1) & pixPerM1) + 1;
        if self.destMSB != 0 {
            self.mask1 = shr32(ALL_ONES, 32 - startBits * self.destDepth as sqInt);
            self.mask2 = shl32(ALL_ONES, 32 - endBits * self.destDepth as sqInt);
        } else {
            self.mask1 = shl32(ALL_ONES, 32 - startBits * self.destDepth as sqInt);
            self.mask2 = shr32(ALL_ONES, 32 - endBits * self.destDepth as sqInt);
        }
        if (self.bbW as sqInt) < startBits {
            self.mask1 &= self.mask2;
            self.mask2 = 0;
            self.nWords = 1;
        } else {
            self.nWords = ((self.bbW as sqInt - startBits) + pixPerM1) / self.destPPW + 1;
        }
        // defaults for no overlap with source; note pitch is bytes and
        // nWords is longs, not bytes
        self.hDir = 1;
        self.vDir = 1;
        self.destIndex = self.destBits.wrapping_add_signed(
            self.dy as sqInt * self.destPitch as sqInt + (self.dx as sqInt / self.destPPW) * 4,
        );
        // byte addr delta
        self.destDelta =
            self.destPitch as sqInt * self.vDir - 4 * (self.nWords * self.hDir);
    }

    /// `checkSourceOverlap` -- flip the copy direction when source and
    /// destination are the same form and the regions overlap.
    pub fn checkSourceOverlap(&mut self) {
        if self.sourceForm == self.destForm && self.dy >= self.sy {
            if self.dy > self.sy {
                // have to start at bottom
                self.vDir = -1;
                self.sy = self.sy + self.bbH - 1;
                self.dy = self.dy + self.bbH - 1;
            } else if self.dy == self.sy && self.dx > self.sx {
                // y's are equal, but x's are backward
                self.hDir = -1;
                // start at right
                self.sx = self.sx + self.bbW - 1;
                // and fix up masks
                self.dx = self.dx + self.bbW - 1;
                if self.nWords > 1 {
                    core::mem::swap(&mut self.mask1, &mut self.mask2);
                }
            }
            self.destIndex = self.destBits.wrapping_add_signed(
                self.dy as sqInt * self.destPitch as sqInt
                    + (self.dx as sqInt / self.destPPW) * 4,
            );
            self.destDelta =
                self.destPitch as sqInt * self.vDir - 4 * (self.nWords * self.hDir);
        }
    }

    /// `sourceSkewAndPointerInit` -- only for the same-depth barrel-shift
    /// loop.
    pub fn sourceSkewAndPointerInit(&mut self) {
        debug_assert!(
            self.destPPW == self.sourcePPW
                && self.destMSB == self.sourceMSB
                && self.destDepth == self.sourceDepth
        );
        // A mask, assuming power of two
        let pixPerM1 = self.destPPW - 1;
        let sxLowBits = self.sx as sqInt & pixPerM1;
        // how many pixels in first word
        let dxLowBits = self.dx as sqInt & pixPerM1;
        let startBits = if self.hDir > 0 {
            self.sourcePPW - (self.sx as sqInt & pixPerM1)
        } else {
            (((self.sx as sqInt + self.bbW as sqInt) - 1) & pixPerM1) + 1
        };
        let m1 = if self.destMSB != 0 {
            shr32(ALL_ONES, 32 - startBits * self.destDepth as sqInt)
        } else {
            shl32(ALL_ONES, 32 - startBits * self.destDepth as sqInt)
        };
        // i.e. there are some missing bits
        self.preload = (m1 & self.mask1) != self.mask1;
        // calculate right-shift skew from source to dest; -32..32
        self.skew = self.destDepth as sqInt
            * (if self.sourceMSB != 0 {
                sxLowBits - dxLowBits
            } else {
                dxLowBits - sxLowBits
            });
        if self.preload {
            self.skew = if self.skew < 0 {
                self.skew + 32
            } else {
                self.skew - 32
            };
        }
        // calculate increments from end of 1 line to start of next
        self.sourceIndex = self.sourceBits.wrapping_add_signed(
            self.sy as sqInt * self.sourcePitch as sqInt
                + (self.sx as sqInt / (32 / self.sourceDepth as sqInt)) * 4,
        );
        self.sourceDelta =
            self.sourcePitch as sqInt * self.vDir - 4 * (self.nWords * self.hDir);
        if self.preload {
            // Compensate for extra source word fetched
            self.sourceDelta -= 4 * self.hDir;
        }
        // The C asserts skew in -31..31 here, but its asserts are compiled
        // out in production and skew = -32 is reachable (word-aligned
        // reversed overlap): the LP64 64-bit shifts then make skewMask 0 and
        // the rotate degenerate to prevWord, which is exactly right for the
        // preloaded reverse copy. The shift helpers reproduce that.
        debug_assert!((-32..=32).contains(&self.skew));
    }

    /// `performCopyLoop` -- choose and perform the appropriate inner loop.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn performCopyLoop(&mut self) {
        self.destMaskAndPointerInit();
        if self.noSource {
            // Simple fill loop
            unsafe { self.copyLoopNoSource() };
        } else {
            self.checkSourceOverlap();
            if self.sourceDepth != self.destDepth
                || self.cmFlags != 0
                || self.sourceMSB != self.destMSB
            {
                // If we must convert between pixel depths or use color
                // lookups or swap pixels use the general version
                unsafe { self.copyLoopPixMap() };
            } else {
                // Otherwise we simply copy pixels and can use a faster version
                self.sourceSkewAndPointerInit();
                unsafe { self.copyLoop() };
            }
        }
    }

    /// `copyLoop` -- the same-depth loop; assumes `noSource` is false.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn copyLoop(&mut self) {
        // unskew is a bitShift and MUST remain signed, while skewMask is
        // unsigned. See sourceSkewAndPointerInit on the reachable range.
        let skew = self.skew;
        debug_assert!((-32..=32).contains(&skew));
        // Byte delta
        let hInc = self.hDir * 4;
        let (unskew, skewMask): (sqInt, u32) = if skew < 0 {
            (skew + 32, shl32(ALL_ONES, -skew))
        } else if skew == 0 {
            (0, ALL_ONES)
        } else {
            (skew - 32, shr32(ALL_ONES, skew))
        };
        let notSkewMask = !skewMask;
        // 32-bit rotate, as the C spells it at every use site
        let rotate = |prevWord: u32, thisWord: u32| -> u32 {
            shift32(prevWord & notSkewMask, unskew) | shift32(thisWord & skewMask, skew)
        };
        let mut halftoneWord: u32;
        if self.noHalftone {
            halftoneWord = ALL_ONES;
            self.halftoneHeight = 0;
        } else {
            // (0 % halftoneHeight) * 4 == 0
            halftoneWord = unsafe { long32At(self.halftoneBase) };
        }
        // Here is the vertical loop, in two versions, one for the
        // combinationRule = 3 copy mode, one for the general case.
        let mut y = self.dy as sqInt;
        if self.combinationRule == 3 {
            for _i in 1..=self.bbH {
                if self.halftoneHeight > 1 {
                    // Otherwise, its always the same
                    halftoneWord = unsafe {
                        long32At(
                            self.halftoneBase
                                .wrapping_add_signed((y % self.halftoneHeight) * 4),
                        )
                    };
                    y += self.vDir;
                }
                let mut prevWord: u32 = if self.preload {
                    // load the 64-bit shifter
                    debug_assert!(self.sourceIndex < self.endOfSource);
                    let w = unsafe { long32At(self.sourceIndex) };
                    self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                    w
                } else {
                    0
                };
                self.destMask = self.mask1;
                // pick up next word
                debug_assert!(self.sourceIndex < self.endOfSource);
                let mut thisWord = unsafe { long32At(self.sourceIndex) };
                self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                let mut skewWord = rotate(prevWord, thisWord);
                prevWord = thisWord;
                debug_assert!(self.destIndex < self.endOfDestination);
                let mut destWord = unsafe { long32At(self.destIndex) };
                destWord =
                    (self.destMask & (skewWord & halftoneWord)) | (destWord & !self.destMask);
                unsafe { long32Atput(self.destIndex, destWord) };
                self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                self.destMask = ALL_ONES;
                if skew == 0 && halftoneWord == ALL_ONES {
                    // Very special inner loop for STORE mode with no skew --
                    // just move words
                    if self.preload && self.hDir == 1 {
                        for _word in 2..self.nWords {
                            // Note loop starts with prevWord loaded (due to
                            // preload)
                            unsafe { long32Atput(self.destIndex, prevWord) };
                            self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                            debug_assert!(self.sourceIndex < self.endOfSource);
                            prevWord = unsafe { long32At(self.sourceIndex) };
                            self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                        }
                    } else {
                        for _word in 2..self.nWords {
                            debug_assert!(self.sourceIndex < self.endOfSource);
                            thisWord = unsafe { long32At(self.sourceIndex) };
                            self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                            unsafe { long32Atput(self.destIndex, thisWord) };
                            self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                        }
                        prevWord = thisWord;
                    }
                } else {
                    for _word in 2..self.nWords {
                        debug_assert!(self.sourceIndex < self.endOfSource);
                        thisWord = unsafe { long32At(self.sourceIndex) };
                        self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                        skewWord = rotate(prevWord, thisWord);
                        prevWord = thisWord;
                        unsafe { long32Atput(self.destIndex, skewWord & halftoneWord) };
                        self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                    }
                }
                if self.nWords > 1 {
                    self.destMask = self.mask2;
                    // pick up next word
                    debug_assert!(self.sourceIndex < self.endOfSource);
                    thisWord = unsafe { long32At(self.sourceIndex) };
                    self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                    skewWord = rotate(prevWord, thisWord);
                    debug_assert!(self.destIndex < self.endOfDestination);
                    destWord = unsafe { long32At(self.destIndex) };
                    destWord =
                        (self.destMask & (skewWord & halftoneWord)) | (destWord & !self.destMask);
                    unsafe { long32Atput(self.destIndex, destWord) };
                    self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                }
                self.sourceIndex = self.sourceIndex.wrapping_add_signed(self.sourceDelta);
                self.destIndex = self.destIndex.wrapping_add_signed(self.destDelta);
            }
        } else {
            for _i in 1..=self.bbH {
                // here is the vertical loop for the general case
                if self.halftoneHeight > 1 {
                    halftoneWord = unsafe {
                        long32At(
                            self.halftoneBase
                                .wrapping_add_signed((y % self.halftoneHeight) * 4),
                        )
                    };
                    y += self.vDir;
                }
                let mut prevWord: u32 = if self.preload {
                    debug_assert!(self.sourceIndex < self.endOfSource);
                    let w = unsafe { long32At(self.sourceIndex) };
                    self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                    w
                } else {
                    0
                };
                self.destMask = self.mask1;
                // pick up next word
                debug_assert!(self.sourceIndex < self.endOfSource);
                let mut thisWord = unsafe { long32At(self.sourceIndex) };
                self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                let mut skewWord = rotate(prevWord, thisWord);
                prevWord = thisWord;
                debug_assert!(self.destIndex < self.endOfDestination);
                let mut destWord = unsafe { long32At(self.destIndex) };
                let mut mergeWord =
                    unsafe { self.mergeFnwith(skewWord & halftoneWord, destWord) };
                destWord = (self.destMask & mergeWord) | (destWord & !self.destMask);
                unsafe { long32Atput(self.destIndex, destWord) };
                self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                self.destMask = ALL_ONES;
                for _word in 2..self.nWords {
                    // Normal inner loop does merge; pick up next word
                    debug_assert!(self.sourceIndex < self.endOfSource);
                    thisWord = unsafe { long32At(self.sourceIndex) };
                    self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                    skewWord = rotate(prevWord, thisWord);
                    prevWord = thisWord;
                    debug_assert!(self.destIndex < self.endOfDestination);
                    let dw = unsafe { long32At(self.destIndex) };
                    mergeWord = unsafe { self.mergeFnwith(skewWord & halftoneWord, dw) };
                    unsafe { long32Atput(self.destIndex, mergeWord) };
                    self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                }
                if self.nWords > 1 {
                    self.destMask = self.mask2;
                    debug_assert!(self.sourceIndex < self.endOfSource);
                    thisWord = unsafe { long32At(self.sourceIndex) };
                    self.sourceIndex = self.sourceIndex.wrapping_add_signed(hInc);
                    skewWord = rotate(prevWord, thisWord);
                    debug_assert!(self.destIndex < self.endOfDestination);
                    destWord = unsafe { long32At(self.destIndex) };
                    mergeWord = unsafe { self.mergeFnwith(skewWord & halftoneWord, destWord) };
                    destWord = (self.destMask & mergeWord) | (destWord & !self.destMask);
                    unsafe { long32Atput(self.destIndex, destWord) };
                    self.destIndex = self.destIndex.wrapping_add_signed(hInc);
                }
                self.sourceIndex = self.sourceIndex.wrapping_add_signed(self.sourceDelta);
                self.destIndex = self.destIndex.wrapping_add_signed(self.destDelta);
            }
        }
    }

    /// `copyLoopNoSource` -- fill loop; `hDir`/`vDir` are both positive, and
    /// preload and skew are unused.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn copyLoopNoSource(&mut self) {
        let mut halftoneWord: u32 = 0;
        if self.noHalftone {
            halftoneWord = ALL_ONES;
        }
        for i in 1..=self.bbH as sqInt {
            // here is the vertical loop
            if !self.noHalftone {
                halftoneWord = unsafe {
                    long32At(self.halftoneBase.wrapping_add_signed(
                        (((self.dy as sqInt + i) - 1) % self.halftoneHeight) * 4,
                    ))
                };
            }
            self.destMask = self.mask1;
            debug_assert!(self.destIndex < self.endOfDestination);
            let mut destWord = unsafe { long32At(self.destIndex) };
            let mut mergeWord = unsafe { self.mergeFnwith(halftoneWord, destWord) };
            destWord = (self.destMask & mergeWord) | (destWord & !self.destMask);
            unsafe { long32Atput(self.destIndex, destWord) };
            self.destIndex = self.destIndex.wrapping_add(4);
            self.destMask = ALL_ONES;
            if self.combinationRule == 3 {
                // Special inner loop for STORE
                destWord = halftoneWord;
                for _word in 2..self.nWords {
                    unsafe { long32Atput(self.destIndex, destWord) };
                    self.destIndex = self.destIndex.wrapping_add(4);
                }
            } else {
                // Normal inner loop does merge
                for _word in 2..self.nWords {
                    debug_assert!(self.destIndex < self.endOfDestination);
                    destWord = unsafe { long32At(self.destIndex) };
                    mergeWord = unsafe { self.mergeFnwith(halftoneWord, destWord) };
                    unsafe { long32Atput(self.destIndex, mergeWord) };
                    self.destIndex = self.destIndex.wrapping_add(4);
                }
            }
            if self.nWords > 1 {
                self.destMask = self.mask2;
                debug_assert!(self.destIndex < self.endOfDestination);
                destWord = unsafe { long32At(self.destIndex) };
                mergeWord = unsafe { self.mergeFnwith(halftoneWord, destWord) };
                destWord = (self.destMask & mergeWord) | (destWord & !self.destMask);
                unsafe { long32Atput(self.destIndex, destWord) };
                self.destIndex = self.destIndex.wrapping_add(4);
            }
            self.destIndex = self.destIndex.wrapping_add_signed(self.destDelta);
        }
    }

    /// `pickSourcePixels:flags:srcMask:destMask:srcShiftInc:dstShiftInc:` --
    /// pick `nPixels` source pixels, map them, and pack them into one
    /// destination word.
    ///
    /// Reads and advances `sourceIndex`/`srcBitShift`; reads `dstBitShift`
    /// without storing it back (the caller resets it), as the C does.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn pickSourcePixels(
        &mut self,
        nPixels: sqInt,
        mapperFlags: sqInt,
        srcMask: u32,
        dstMask: u32,
        srcShiftInc: sqInt,
        dstShiftInc: sqInt,
    ) -> u32 {
        let mut destWord: u32 = 0;
        let mut srcShift = self.srcBitShift; // Hint: Keep in register
        let mut dstShift = self.dstBitShift; // Hint: Keep in register
        let mut nPix = nPixels; // always > 0
        debug_assert!(nPix > 0);
        if mapperFlags == (COLOR_MAP_PRESENT | crate::state::COLOR_MAP_INDEXED_PART) {
            // a little optimization for (pretty crucial) blits using indexed
            // lookups only
            loop {
                // grab, colormap and mix in pixel
                debug_assert!(self.sourceIndex < self.endOfSource);
                let sourceWord = unsafe { long32At(self.sourceIndex) };
                let sourcePix = (shr32(sourceWord, srcShift) & srcMask) as sqInt;
                let destPix = unsafe { self.cmLookupAt(sourcePix) };
                // adjust dest pix index
                destWord |= shl32(destPix & dstMask, dstShift);
                // adjust source pix index
                dstShift += dstShiftInc;
                srcShift += srcShiftInc;
                // the C spells this `(srcShift & 0xFFFFFFE0) != 0`
                if !(0..32).contains(&srcShift) {
                    srcShift = if self.sourceMSB != 0 {
                        srcShift + 32
                    } else {
                        srcShift - 32
                    };
                    self.sourceIndex = self.sourceIndex.wrapping_add(4);
                }
                nPix -= 1;
                if nPix == 0 {
                    break;
                }
            }
        } else {
            loop {
                // grab, colormap and mix in pixel
                debug_assert!(self.sourceIndex < self.endOfSource);
                let sourceWord = unsafe { long32At(self.sourceIndex) };
                let sourcePix = (shr32(sourceWord, srcShift) & srcMask) as sqInt;
                let destPix = unsafe { self.mapPixelflags(sourcePix, mapperFlags) };
                // adjust dest pix index
                destWord |= shl32(destPix as u32 & dstMask, dstShift);
                // adjust source pix index
                dstShift += dstShiftInc;
                srcShift += srcShiftInc;
                if !(0..32).contains(&srcShift) {
                    srcShift = if self.sourceMSB != 0 {
                        srcShift + 32
                    } else {
                        srcShift - 32
                    };
                    self.sourceIndex = self.sourceIndex.wrapping_add(4);
                }
                nPix -= 1;
                if nPix == 0 {
                    break;
                }
            }
        }
        // Store back
        self.srcBitShift = srcShift;
        destWord
    }

    /// `copyLoopPixMap` -- the general loop: depth conversion, color lookup,
    /// and MSB/LSB swapping. Preload, skew and skewMask are all overlooked,
    /// since pickSourcePixels delivers its destination word already properly
    /// aligned.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn copyLoopPixMap(&mut self) {
        self.sourcePPW = 32 / self.sourceDepth as sqInt;
        let sourcePixMask = MASK_TABLE[self.sourceDepth as usize];
        let destPixMask = MASK_TABLE[self.destDepth as usize];
        let mapperFlags = self.cmFlags & !COLOR_MAP_NEW_STYLE;
        self.sourceIndex = self.sourceBits.wrapping_add_signed(
            self.sy as sqInt * self.sourcePitch as sqInt
                + (self.sx as sqInt / self.sourcePPW) * 4,
        );
        let scrStartBits = self.sourcePPW - (self.sx as sqInt & (self.sourcePPW - 1));
        let nSourceIncs = if (self.bbW as sqInt) < scrStartBits {
            0
        } else {
            (self.bbW as sqInt - scrStartBits) / self.sourcePPW + 1
        };
        // Note following two items were already calculated in destmask setup!
        self.sourceDelta = self.sourcePitch as sqInt - nSourceIncs * 4;
        let mut startBits = self.destPPW - (self.dx as sqInt & (self.destPPW - 1));
        let endBits = (((self.dx as sqInt + self.bbW as sqInt) - 1) & (self.destPPW - 1)) + 1;
        if (self.bbW as sqInt) < startBits {
            startBits = self.bbW as sqInt;
        }
        let mut srcShift = (self.sx as sqInt & (self.sourcePPW - 1)) * self.sourceDepth as sqInt;
        let mut dstShift = (self.dx as sqInt & (self.destPPW - 1)) * self.destDepth as sqInt;
        let mut srcShiftInc = self.sourceDepth as sqInt;
        let mut dstShiftInc = self.destDepth as sqInt;
        let mut dstShiftLeft: sqInt = 0;
        if self.sourceMSB != 0 {
            srcShift = (32 - self.sourceDepth as sqInt) - srcShift;
            srcShiftInc = -srcShiftInc;
        }
        if self.destMSB != 0 {
            dstShift = (32 - self.destDepth as sqInt) - dstShift;
            dstShiftInc = -dstShiftInc;
            dstShiftLeft = 32 - self.destDepth as sqInt;
        }
        let mut halftoneWord: u32 = 0;
        if self.noHalftone {
            halftoneWord = ALL_ONES;
        }
        for i in 1..=self.bbH as sqInt {
            // here is the vertical loop
            if !self.noHalftone {
                halftoneWord = unsafe {
                    long32At(self.halftoneBase.wrapping_add_signed(
                        (((self.dy as sqInt + i) - 1) % self.halftoneHeight) * 4,
                    ))
                };
            }
            self.srcBitShift = srcShift;
            self.dstBitShift = dstShift;
            self.destMask = self.mask1;
            // Here is the horizontal loop...
            let mut nPix = startBits;
            let mut words = self.nWords;
            loop {
                let skewWord = unsafe {
                    self.pickSourcePixels(
                        nPix,
                        mapperFlags,
                        sourcePixMask,
                        destPixMask,
                        srcShiftInc,
                        dstShiftInc,
                    )
                };
                self.dstBitShift = dstShiftLeft;
                if self.destMask == ALL_ONES {
                    // avoid read-modify-write
                    debug_assert!(self.destIndex < self.endOfDestination);
                    let dw = unsafe { long32At(self.destIndex) };
                    let mergeWord = unsafe { self.mergeFnwith(skewWord & halftoneWord, dw) };
                    unsafe { long32Atput(self.destIndex, self.destMask & mergeWord) };
                } else {
                    // General version using dest masking
                    debug_assert!(self.destIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(self.destIndex) };
                    let mergeWord = unsafe {
                        self.mergeFnwith(skewWord & halftoneWord, destWord & self.destMask)
                    };
                    destWord = (self.destMask & mergeWord) | (destWord & !self.destMask);
                    unsafe { long32Atput(self.destIndex, destWord) };
                }
                self.destIndex = self.destIndex.wrapping_add(4);
                if words == 2 {
                    // e.g., is the next word the last word?
                    // set mask for last word in this row
                    self.destMask = self.mask2;
                    nPix = endBits;
                } else {
                    // use fullword mask for inner loop
                    self.destMask = ALL_ONES;
                    nPix = self.destPPW;
                }
                words -= 1;
                if words == 0 {
                    break;
                }
            }
            self.sourceIndex = self.sourceIndex.wrapping_add_signed(self.sourceDelta);
            self.destIndex = self.destIndex.wrapping_add_signed(self.destDelta);
        }
    }

    /// Set the affected rectangle from `dx`/`dy`/`bbW`/`bbH`, as the quick
    /// paths do at every exit.
    fn setAffectedToBlitRect(&mut self) {
        self.affectedL = self.dx as sqInt;
        self.affectedR = self.dx as sqInt + self.bbW as sqInt;
        self.affectedT = self.dy as sqInt;
        self.affectedB = self.dy as sqInt + self.bbH as sqInt;
    }

    /// `tryCopyingBitsQuickly` -- the rule-34/41 shortcut for 32bpp sources.
    /// Answers whether the copy was done.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn tryCopyingBitsQuickly(&mut self) -> bool {
        // We need a source.
        if self.noSource {
            return false;
        }
        if !(self.combinationRule == 34 || self.combinationRule == 41) {
            return false;
        }
        if self.sourceDepth != 32 {
            return false;
        }
        if self.sourceForm == self.destForm {
            return false;
        }
        if self.combinationRule == 41 {
            match self.destDepth {
                32 => unsafe { self.rgbComponentAlpha32() },
                16 => unsafe { self.rgbComponentAlpha16() },
                8 => unsafe { self.rgbComponentAlpha8() },
                _ => return false,
            }
            self.setAffectedToBlitRect();
            return true;
        }
        if self.destDepth < 8 {
            return false;
        }
        if self.destDepth == 8 && (self.cmFlags & COLOR_MAP_PRESENT) == 0 {
            return false;
        }
        if self.destDepth == 32 {
            unsafe { self.alphaSourceBlendBits32() };
        }
        if self.destDepth == 16 {
            unsafe { self.alphaSourceBlendBits16() };
        }
        if self.destDepth == 8 {
            unsafe { self.alphaSourceBlendBits8() };
        }
        self.setAffectedToBlitRect();
        true
    }

    /// The tail of `copyBitsLockedAndClipped` after the argument fetching:
    /// the quick-path test, the copy loop, and the affected-rectangle
    /// bookkeeping. The caller has already run `copyBitsRule41Test` and, for
    /// rules 30/31, fetched `sourceAlpha` (both need the interpreter).
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn copyBitsDispatch(&mut self) {
        if unsafe { self.tryCopyingBitsQuickly() } {
            return;
        }
        // Choose and perform the actual copy loop.
        self.bitCount = 0;
        unsafe { self.performCopyLoop() };
        if self.combinationRule >= 30 && self.combinationRule <= 31 {
            // zero width and height; just return the count
            self.affectedL = 0;
            self.affectedR = 0;
            self.affectedT = 0;
            self.affectedB = 0;
        } else {
            if self.hDir > 0 {
                self.affectedL = self.dx as sqInt;
                self.affectedR = self.dx as sqInt + self.bbW as sqInt;
            } else {
                self.affectedL = (self.dx as sqInt - self.bbW as sqInt) + 1;
                self.affectedR = self.dx as sqInt + 1;
            }
            if self.vDir > 0 {
                self.affectedT = self.dy as sqInt;
                self.affectedB = self.dy as sqInt + self.bbH as sqInt;
            } else {
                self.affectedT = (self.dy as sqInt - self.bbH as sqInt) + 1;
                self.affectedB = self.dy as sqInt + 1;
            }
        }
    }

    // --- Rule 34: alphaSourceBlendBits* ---------------------------------

    /// `alphaSourceBlendBits32` -- rule 34, 32bpp -> 32bpp,
    /// `sourceForm ~= destForm`.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn alphaSourceBlendBits32(&mut self) {
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + self.dx as sqInt * 4,
            );
            let mut deltaX = self.bbW + 1;
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord = unsafe { long32At(srcIndex) };
                let srcAlpha = sourceWord >> 24;
                if srcAlpha == 0xFF {
                    unsafe { long32Atput(dstIndex, sourceWord) };
                    srcIndex = srcIndex.wrapping_add(4);
                    dstIndex = dstIndex.wrapping_add(4);
                    // Now copy as many words as possible with alpha = 255
                    loop {
                        deltaX -= 1;
                        if deltaX == 0 {
                            break;
                        }
                        debug_assert!(srcIndex < self.endOfSource);
                        sourceWord = unsafe { long32At(srcIndex) };
                        if sourceWord >> 24 != 0xFF {
                            break;
                        }
                        unsafe { long32Atput(dstIndex, sourceWord) };
                        srcIndex = srcIndex.wrapping_add(4);
                        dstIndex = dstIndex.wrapping_add(4);
                    }
                    deltaX += 1;
                } else if srcAlpha == 0 {
                    srcIndex = srcIndex.wrapping_add(4);
                    dstIndex = dstIndex.wrapping_add(4);
                    // Now skip as many words as possible
                    loop {
                        deltaX -= 1;
                        if deltaX == 0 {
                            break;
                        }
                        debug_assert!(srcIndex < self.endOfSource);
                        sourceWord = unsafe { long32At(srcIndex) };
                        if sourceWord >> 24 != 0 {
                            break;
                        }
                        srcIndex = srcIndex.wrapping_add(4);
                        dstIndex = dstIndex.wrapping_add(4);
                    }
                    deltaX += 1;
                } else {
                    // 0 < srcAlpha < 255: mix colors, copy a single word
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(dstIndex) };
                    destWord = alphaBlendScaledwith(sourceWord, destWord);
                    unsafe { long32Atput(dstIndex, destWord) };
                    srcIndex = srcIndex.wrapping_add(4);
                    dstIndex = dstIndex.wrapping_add(4);
                }
            }
            srcY += 1;
            dstY += 1;
        }
    }

    /// `alphaSourceBlendBits16` -- rule 34, 32bpp -> 16bpp with dithering.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn alphaSourceBlendBits16(&mut self) {
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        let mut srcShift: i32 = (self.dx & 1) * 16;
        if self.destMSB != 0 {
            srcShift = 16 - srcShift;
        }
        // This is the outer loop
        self.mask1 = shl32(0xFFFF, (16 - srcShift) as sqInt);
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + (self.dx as sqInt / 2) * 4,
            );
            let ditherBase = (dstY & 3) * 4;
            // For pre-increment
            let mut ditherIndex = (self.sx & 3) - 1;
            // So we can pre-decrement
            let mut deltaX = self.bbW + 1;
            let mut dstMask = self.mask1;
            let mut srcShift: i32 = if dstMask == 0xFFFF { 16 } else { 0 };
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                ditherIndex = (ditherIndex + 1) & 3;
                let ditherThreshold = DITHER_MATRIX_4X4[(ditherBase + ditherIndex) as usize];
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord = unsafe { long32At(srcIndex) };
                let srcAlpha = sourceWord >> 24;
                if srcAlpha == 0xFF {
                    // Dither from 32 to 16 bit
                    sourceWord = dither32To16threshold(sourceWord, ditherThreshold);
                    sourceWord = if sourceWord == 0 {
                        shl32(1, srcShift as sqInt)
                    } else {
                        shl32(sourceWord, srcShift as sqInt)
                    };
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut dstValue = unsafe { long32At(dstIndex) };
                    dstValue &= dstMask;
                    dstValue |= sourceWord;
                    unsafe { long32Atput(dstIndex, dstValue) };
                } else if srcAlpha != 0 {
                    // 0 < srcAlpha < 255: mix colors, copy a single word
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(dstIndex) };
                    destWord &= !dstMask;
                    // Expand from 16 to 32 bit by adding zero bits
                    destWord = shr32(destWord, srcShift as sqInt);
                    // Mix colors
                    destWord = ((destWord & 0x7C00) << 9)
                        | ((destWord & 0x3E0) << 6)
                        | ((destWord & 0x1F) << 3)
                        | 0xFF000000;
                    // And dither
                    sourceWord = alphaBlendScaledwith(sourceWord, destWord);
                    sourceWord = dither32To16threshold(sourceWord, ditherThreshold);
                    sourceWord = if sourceWord == 0 {
                        shl32(1, srcShift as sqInt)
                    } else {
                        shl32(sourceWord, srcShift as sqInt)
                    };
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut dstValue = unsafe { long32At(dstIndex) };
                    dstValue &= dstMask;
                    dstValue |= sourceWord;
                    unsafe { long32Atput(dstIndex, dstValue) };
                }
                srcIndex = srcIndex.wrapping_add(4);
                if self.destMSB != 0 {
                    if srcShift == 0 {
                        dstIndex = dstIndex.wrapping_add(4);
                    }
                } else if srcShift != 0 {
                    dstIndex = dstIndex.wrapping_add(4);
                }
                // Toggle between 0 and 16
                srcShift ^= 16;
                dstMask = !dstMask;
            }
            srcY += 1;
            dstY += 1;
        }
    }

    /// `alphaSourceBlendBits8` -- rule 34, 32bpp -> 8bpp. Not real blending
    /// since we don't have the source colors available.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn alphaSourceBlendBits8(&mut self) {
        let mapperFlags = self.cmFlags & !COLOR_MAP_NEW_STYLE;
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        self.mask1 = ((self.dx & 3) * 8) as u32;
        if self.destMSB != 0 {
            self.mask1 = 24 - self.mask1;
        }
        self.mask2 = ALL_ONES ^ shl32(0xFF, self.mask1 as sqInt);
        let mut adjust: u32 = if (self.dx & 1) == 0 { 0 } else { 522133279 };
        if (self.dy & 1) == 0 {
            adjust ^= 522133279;
        }
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            adjust ^= 522133279;
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + (self.dx as sqInt / 4) * 4,
            );
            // So we can pre-decrement
            let mut deltaX = self.bbW + 1;
            let mut srcShift = self.mask1 as sqInt;
            // This is the inner loop
            let mut dstMask = self.mask2;
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord =
                    (unsafe { long32At(srcIndex) } & !adjust).wrapping_add(adjust);
                let srcAlpha = sourceWord >> 24;
                if srcAlpha > 0x1F {
                    // Everything below 31 is transparent
                    if srcAlpha < 224 {
                        // Everything above 224 is opaque
                        debug_assert!(dstIndex < self.endOfDestination);
                        let mut destWord = unsafe { long32At(dstIndex) };
                        destWord &= !dstMask;
                        destWord = shr32(destWord, srcShift);
                        let destWord = DEFAULT_8_TO_32_TABLE[destWord as usize];
                        sourceWord = alphaBlendScaledwith(sourceWord, destWord);
                    }
                    sourceWord =
                        unsafe { self.mapPixelflags(sourceWord as sqInt, mapperFlags) } as u32;
                    // Store back
                    sourceWord = shl32(sourceWord, srcShift);
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut dstValue = unsafe { long32At(dstIndex) };
                    dstValue &= dstMask;
                    dstValue |= sourceWord;
                    unsafe { long32Atput(dstIndex, dstValue) };
                }
                srcIndex = srcIndex.wrapping_add(4);
                if self.destMSB != 0 {
                    if srcShift == 0 {
                        dstIndex = dstIndex.wrapping_add(4);
                        srcShift = 24;
                        dstMask = 0xFFFFFF;
                    } else {
                        srcShift -= 8;
                        dstMask = (dstMask >> 8) | 0xFF000000;
                    }
                } else if srcShift == 24 {
                    dstIndex = dstIndex.wrapping_add(4);
                    srcShift = 0;
                    dstMask = 0xFFFFFF00;
                } else {
                    srcShift += 8;
                    dstMask = (dstMask << 8) | 0xFF;
                }
                adjust ^= 522133279;
            }
            srcY += 1;
            dstY += 1;
        }
    }

    // --- Rule 41: rgbComponentAlpha* -------------------------------------

    /// `rgbComponentAlpha32` -- rule 41, 32bpp -> 32bpp.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn rgbComponentAlpha32(&mut self) {
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + self.dx as sqInt * 4,
            );
            let mut deltaX = self.bbW + 1;
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord = unsafe { long32At(srcIndex) };
                let srcAlpha = sourceWord & 0xFFFFFF;
                if srcAlpha == 0 {
                    srcIndex = srcIndex.wrapping_add(4);
                    dstIndex = dstIndex.wrapping_add(4);
                    // Now skip as many words as possible
                    loop {
                        deltaX -= 1;
                        if deltaX == 0 {
                            break;
                        }
                        debug_assert!(srcIndex < self.endOfSource);
                        sourceWord = unsafe { long32At(srcIndex) };
                        if sourceWord & 0xFFFFFF != 0 {
                            break;
                        }
                        srcIndex = srcIndex.wrapping_add(4);
                        dstIndex = dstIndex.wrapping_add(4);
                    }
                    deltaX += 1;
                } else {
                    // 0 < srcAlpha: mix colors, copy a single word
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(dstIndex) };
                    destWord = unsafe { self.rgbComponentAlpha32with(sourceWord, destWord) };
                    unsafe { long32Atput(dstIndex, destWord) };
                    srcIndex = srcIndex.wrapping_add(4);
                    dstIndex = dstIndex.wrapping_add(4);
                }
            }
            srcY += 1;
            dstY += 1;
        }
    }

    /// `rgbComponentAlpha16` -- rule 41, 32bpp -> 16bpp with dithering.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn rgbComponentAlpha16(&mut self) {
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        let mut srcShift: i32 = (self.dx & 1) * 16;
        if self.destMSB != 0 {
            srcShift = 16 - srcShift;
        }
        // This is the outer loop
        self.mask1 = shl32(0xFFFF, (16 - srcShift) as sqInt);
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + (self.dx as sqInt / 2) * 4,
            );
            let ditherBase = (dstY & 3) * 4;
            // For pre-increment
            let mut ditherIndex = (self.sx & 3) - 1;
            // So we can pre-decrement
            let mut deltaX = self.bbW + 1;
            let mut dstMask = self.mask1;
            let mut srcShift: i32 = if dstMask == 0xFFFF { 16 } else { 0 };
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                ditherIndex = (ditherIndex + 1) & 3;
                let ditherThreshold = DITHER_MATRIX_4X4[(ditherBase + ditherIndex) as usize];
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord = unsafe { long32At(srcIndex) };
                let srcAlpha = sourceWord & 0xFFFFFF;
                if srcAlpha != 0 {
                    // 0 < srcAlpha: mix colors, copy a single word
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(dstIndex) };
                    destWord &= !dstMask;
                    // Expand from 16 to 32 bit by adding zero bits
                    destWord = shr32(destWord, srcShift as sqInt);
                    // Mix colors
                    destWord = ((destWord & 0x7C00) << 9)
                        | ((destWord & 0x3E0) << 6)
                        | ((destWord & 0x1F) << 3)
                        | 0xFF000000;
                    // And dither
                    sourceWord = unsafe { self.rgbComponentAlpha32with(sourceWord, destWord) };
                    sourceWord = dither32To16threshold(sourceWord, ditherThreshold);
                    sourceWord = if sourceWord == 0 {
                        shl32(1, srcShift as sqInt)
                    } else {
                        shl32(sourceWord, srcShift as sqInt)
                    };
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut dstValue = unsafe { long32At(dstIndex) };
                    dstValue &= dstMask;
                    dstValue |= sourceWord;
                    unsafe { long32Atput(dstIndex, dstValue) };
                }
                srcIndex = srcIndex.wrapping_add(4);
                if self.destMSB != 0 {
                    if srcShift == 0 {
                        dstIndex = dstIndex.wrapping_add(4);
                    }
                } else if srcShift != 0 {
                    dstIndex = dstIndex.wrapping_add(4);
                }
                // Toggle between 0 and 16
                srcShift ^= 16;
                dstMask = !dstMask;
            }
            srcY += 1;
            dstY += 1;
        }
    }

    /// `rgbComponentAlpha8` -- rule 41, 32bpp -> 8bpp.
    ///
    /// Note the C's LSB advance tests `srcShift == 32` where
    /// `alphaSourceBlendBits8` tests 24; the quirk ships, so it is mirrored.
    ///
    /// # Safety
    ///
    /// See the module contract.
    pub unsafe fn rgbComponentAlpha8(&mut self) {
        let mapperFlags = self.cmFlags & !COLOR_MAP_NEW_STYLE;
        // So we can pre-decrement
        let mut deltaY = self.bbH + 1;
        let mut srcY = self.sy;
        let mut dstY = self.dy;
        self.mask1 = ((self.dx & 3) * 8) as u32;
        if self.destMSB != 0 {
            self.mask1 = 24 - self.mask1;
        }
        self.mask2 = ALL_ONES ^ shl32(0xFF, self.mask1 as sqInt);
        let mut adjust: u32 = if (self.dx & 1) == 0 { 0 } else { 522133279 };
        if (self.dy & 1) == 0 {
            adjust ^= 522133279;
        }
        loop {
            deltaY -= 1;
            if deltaY == 0 {
                break;
            }
            adjust ^= 522133279;
            let mut srcIndex = self.sourceBits.wrapping_add_signed(
                srcY as sqInt * self.sourcePitch as sqInt + self.sx as sqInt * 4,
            );
            let mut dstIndex = self.destBits.wrapping_add_signed(
                dstY as sqInt * self.destPitch as sqInt + (self.dx as sqInt / 4) * 4,
            );
            // So we can pre-decrement
            let mut deltaX = self.bbW + 1;
            let mut srcShift = self.mask1 as sqInt;
            // This is the inner loop
            let mut dstMask = self.mask2;
            loop {
                deltaX -= 1;
                if deltaX == 0 {
                    break;
                }
                debug_assert!(srcIndex < self.endOfSource);
                let mut sourceWord =
                    (unsafe { long32At(srcIndex) } & !adjust).wrapping_add(adjust);
                // set srcAlpha to the average of the 3 separate aR,aG,aB values
                let mut srcAlpha = sourceWord & 0xFFFFFF;
                srcAlpha = ((srcAlpha >> 16) + ((srcAlpha >> 8) & 0xFF) + (srcAlpha & 0xFF)) / 3;
                if srcAlpha > 0x1F {
                    // Everything below 31 is transparent
                    if srcAlpha > 224 {
                        // treat everything above 224 as opaque
                        sourceWord = 0xFFFFFFFF;
                    }
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut destWord = unsafe { long32At(dstIndex) };
                    destWord &= !dstMask;
                    destWord = shr32(destWord, srcShift);
                    let destWord = DEFAULT_8_TO_32_TABLE[destWord as usize];
                    sourceWord = unsafe { self.rgbComponentAlpha32with(sourceWord, destWord) };
                    sourceWord =
                        unsafe { self.mapPixelflags(sourceWord as sqInt, mapperFlags) } as u32;
                    // Store back
                    sourceWord = shl32(sourceWord, srcShift);
                    debug_assert!(dstIndex < self.endOfDestination);
                    let mut dstValue = unsafe { long32At(dstIndex) };
                    dstValue &= dstMask;
                    dstValue |= sourceWord;
                    unsafe { long32Atput(dstIndex, dstValue) };
                }
                srcIndex = srcIndex.wrapping_add(4);
                if self.destMSB != 0 {
                    if srcShift == 0 {
                        dstIndex = dstIndex.wrapping_add(4);
                        srcShift = 24;
                        dstMask = 0xFFFFFF;
                    } else {
                        srcShift -= 8;
                        dstMask = (dstMask >> 8) | 0xFF000000;
                    }
                } else if srcShift == 32 {
                    // sic: alphaSourceBlendBits8 tests 24 here
                    dstIndex = dstIndex.wrapping_add(4);
                    srcShift = 0;
                    dstMask = 0xFFFFFF00;
                } else {
                    srcShift += 8;
                    dstMask = (dstMask << 8) | 0xFF;
                }
                adjust ^= 522133279;
            }
            srcY += 1;
            dstY += 1;
        }
    }
}
