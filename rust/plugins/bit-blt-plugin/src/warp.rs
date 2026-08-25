//! WarpBlt: the affine-warp loop and its pixel pickers.
//!
//! `warpLoop` in the C interleaves interpreter access (fetching the quad
//! points from the BitBlt object, the smoothing arguments from the stack)
//! with the pixel loop. Here the interpreter half lives in `load.rs`
//! (`warpLoop`), which fetches everything and then calls
//! [`BitBlt::warpLoopBody`] -- the pure remainder, testable without a VM.

use pharo_vm_plugin::sqInt;

use crate::state::{
    long32At, long32Atput, shl32, shr32, BitBlt, ALL_ONES, BINARY_POINT, COLOR_MAP_INDEXED_PART,
    COLOR_MAP_NEW_STYLE, COLOR_MAP_PRESENT, FIXED_PT1, MASK_TABLE,
};

/// `deltaFrom:to:nSteps:` -- utility routine for computing Warp increments.
pub fn deltaFromtonSteps(x1: sqInt, x2: sqInt, n: sqInt) -> sqInt {
    if x2 > x1 {
        ((x2 - x1) + FIXED_PT1) / (n + 1) + 1
    } else if x2 == x1 {
        0
    } else {
        -(((x1 - x2) + FIXED_PT1) / (n + 1) + 1)
    }
}

impl BitBlt {
    /// `warpLoopSetup` -- setup values for faster pixel fetching.
    pub fn warpLoopSetup(&mut self) {
        // warpSrcShift = log2(sourceDepth)
        self.warpSrcShift = 0;
        let mut words = self.sourceDepth as sqInt; // recycle temp
        while words != 1 {
            self.warpSrcShift += 1;
            words = ((words as usize) >> 1) as sqInt;
        }
        self.warpSrcMask = MASK_TABLE[self.sourceDepth as usize] as i32 as sqInt;
        // warpAlignShift: Shift for aligning x position to word boundary
        self.warpAlignShift = 5 - self.warpSrcShift;
        // warpAlignMask: Mask for extracting the pixel position from an x
        // position
        self.warpAlignMask = (1 << self.warpAlignShift) - 1;
        // warpBitShiftTable: given a sub-word x value what's the bit shift?
        for i in 0..=self.warpAlignMask {
            self.warpBitShiftTable[i as usize] = if self.sourceMSB != 0 {
                (32 - ((i + 1) << self.warpSrcShift)) as i32
            } else {
                (i << self.warpSrcShift) as i32
            };
        }
    }

    /// `pickWarpPixelAtX:y:` -- pick a single (fixed-point addressed) pixel.
    ///
    /// # Safety
    ///
    /// See the module contract on [`crate::engine`].
    pub unsafe fn pickWarpPixelAtXy(&mut self, xx: sqInt, yy: sqInt) -> u32 {
        // note: it would be much faster if we could just avoid these stupid
        // tests for being inside sourceForm.
        if xx < 0 || yy < 0 {
            return 0;
        }
        let x = ((xx as usize) >> BINARY_POINT) as sqInt;
        let y = ((yy as usize) >> BINARY_POINT) as sqInt;
        if x >= self.sourceWidth as sqInt || y >= self.sourceHeight as sqInt {
            return 0;
        }
        let srcIndex = self.sourceBits.wrapping_add_signed(
            y * self.sourcePitch as sqInt + ((x as usize) >> self.warpAlignShift) as sqInt * 4,
        );
        // Extract pixel from word
        debug_assert!(srcIndex < self.endOfSource);
        let sourceWord = unsafe { long32At(srcIndex) };
        self.srcBitShift = self.warpBitShiftTable[(x & self.warpAlignMask) as usize] as sqInt;
        (shr32(sourceWord, self.srcBitShift) as sqInt & self.warpSrcMask) as u32
    }

    /// `warpPickSourcePixels:...` -- pick `nPixels` pixels along the current
    /// scan vector (no smoothing), map them, and pack the destination word.
    ///
    /// The C signature carries the vertical deltas too; they are unused
    /// there as well.
    ///
    /// # Safety
    ///
    /// See the module contract on [`crate::engine`].
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn warpPickSourcePixels(
        &mut self,
        nPixels: sqInt,
        xDeltah: sqInt,
        yDeltah: sqInt,
        _xDeltav: sqInt,
        _yDeltav: sqInt,
        dstShiftInc: sqInt,
        mapperFlags: sqInt,
    ) -> u32 {
        let dstMask = MASK_TABLE[self.destDepth as usize];
        let mut destWord: u32 = 0;
        let mut nPix = nPixels;
        debug_assert!(nPix > 0);
        if mapperFlags == (COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART) {
            // a little optimization for (pretty crucial) blits using indexed
            // lookups only
            loop {
                // grab, colormap and mix in pixel
                let sourcePix = unsafe { self.pickWarpPixelAtXy(self.sx as sqInt, self.sy as sqInt) };
                let destPix = unsafe { self.cmLookupAt(sourcePix as sqInt) };
                destWord |= shl32(destPix & dstMask, self.dstBitShift);
                self.dstBitShift += dstShiftInc;
                self.sx = (self.sx as sqInt).wrapping_add(xDeltah) as i32;
                self.sy = (self.sy as sqInt).wrapping_add(yDeltah) as i32;
                nPix -= 1;
                if nPix == 0 {
                    break;
                }
            }
        } else {
            loop {
                // grab, colormap and mix in pixel
                let sourcePix = unsafe { self.pickWarpPixelAtXy(self.sx as sqInt, self.sy as sqInt) };
                let destPix = unsafe { self.mapPixelflags(sourcePix as sqInt, mapperFlags) };
                destWord |= shl32(destPix as u32 & dstMask, self.dstBitShift);
                self.dstBitShift += dstShiftInc;
                self.sx = (self.sx as sqInt).wrapping_add(xDeltah) as i32;
                self.sy = (self.sy as sqInt).wrapping_add(yDeltah) as i32;
                nPix -= 1;
                if nPix == 0 {
                    break;
                }
            }
        }
        destWord
    }

    /// `warpPickSmoothPixels:...` -- pick and average `n * n` sub-pixels per
    /// destination pixel. Only called with `smoothingCount > 1`.
    ///
    /// # Safety
    ///
    /// See the module contract on [`crate::engine`]; additionally
    /// `sourceMap` must point at `2^sourceDepth` words when
    /// `sourceDepth < 16` (the loader checks this).
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn warpPickSmoothPixels(
        &mut self,
        nPixels: sqInt,
        xDeltah: sqInt,
        yDeltah: sqInt,
        xDeltav: sqInt,
        yDeltav: sqInt,
        sourceMap: usize,
        n: sqInt,
        dstShiftInc: sqInt,
    ) -> u32 {
        let dstMask = MASK_TABLE[self.destDepth as usize];
        let mut destWord: u32 = 0;
        // Try avoiding divides for most common n (the C's n == 2 shortcut is
        // the same arithmetic)
        let xdh = xDeltah / n;
        let ydh = yDeltah / n;
        let xdv = xDeltav / n;
        let ydv = yDeltav / n;
        let mut i = nPixels;
        debug_assert!(i > 0);
        loop {
            let mut x = self.sx as sqInt;
            let mut y = self.sy as sqInt;
            // Pick and average n*n subpixels
            let mut a: sqInt = 0;
            let mut r: sqInt = 0;
            let mut g: sqInt = 0;
            let mut b: sqInt = 0;
            // actual number of pixels (not clipped and not transparent)
            let mut nPix: sqInt = 0;
            let mut j = n;
            loop {
                let mut xx = x;
                let mut yy = y;
                let mut k = n;
                loop {
                    let mut rgb = unsafe { self.pickWarpPixelAtXy(xx, yy) };
                    if !(self.combinationRule == 25 && rgb == 0) {
                        // If not clipped and not transparent, then tally rgb
                        // values
                        nPix += 1;
                        if self.sourceDepth < 16 {
                            // Get RGBA values from sourcemap table
                            rgb = unsafe { long32At(sourceMap + ((rgb as usize) << 2)) };
                        } else if self.sourceDepth == 16 {
                            // Already in RGB format
                            rgb = crate::rules::rgbMap16To32(rgb);
                        }
                        b += (rgb & 0xFF) as sqInt;
                        g += ((rgb >> 8) & 0xFF) as sqInt;
                        r += ((rgb >> 16) & 0xFF) as sqInt;
                        a += (rgb >> 24) as sqInt;
                    }
                    xx += xdh;
                    yy += ydh;
                    k -= 1;
                    if k == 0 {
                        break;
                    }
                }
                x += xdv;
                y += ydv;
                j -= 1;
                if j == 0 {
                    break;
                }
            }
            let mut rgb: u32;
            if nPix == 0 || (self.combinationRule == 25 && nPix < (n * n) / 2) {
                // All pixels were 0, or most were transparent
                rgb = 0;
            } else {
                // normalize rgba sums
                r /= nPix;
                g /= nPix;
                b /= nPix;
                a /= nPix;
                // map the pixel
                rgb = (((a << 24) + (r << 16) + (g << 8)) + b) as u32;
                if rgb == 0 {
                    // only generate zero if pixel is really transparent
                    if r + g + b + a > 0 {
                        rgb = 1;
                    }
                }
                rgb = unsafe { self.mapPixelflags(rgb as sqInt, self.cmFlags) } as u32;
            }
            destWord |= shl32(rgb & dstMask, self.dstBitShift);
            self.dstBitShift += dstShiftInc;
            self.sx = (self.sx as sqInt).wrapping_add(xDeltah) as i32;
            self.sy = (self.sy as sqInt).wrapping_add(yDeltah) as i32;
            i -= 1;
            if i == 0 {
                break;
            }
        }
        destWord
    }

    /// The pure tail of `warpLoop`: everything after the interpreter has
    /// fetched the quad points, computed the vertical deltas, and resolved
    /// the smoothing arguments.
    ///
    /// `sourceMap` is the address of the source map's words (only read when
    /// `sourceDepth < 16`, which the caller has validated implies the map is
    /// present and large enough).
    ///
    /// # Safety
    ///
    /// See the module contract on [`crate::engine`].
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn warpLoopBody(
        &mut self,
        mut pAx: sqInt,
        mut pAy: sqInt,
        mut pBx: sqInt,
        mut pBy: sqInt,
        deltaP12x: sqInt,
        deltaP12y: sqInt,
        deltaP43x: sqInt,
        deltaP43y: sqInt,
        smoothingCount: sqInt,
        sourceMap: usize,
    ) {
        let mut halftoneWord: u32 = 0;
        let mut nSteps = self.width - 1;
        if nSteps <= 0 {
            nSteps = 1;
        }
        let mut startBits = self.destPPW - (self.dx as sqInt & (self.destPPW - 1));
        let endBits = (((self.dx as sqInt + self.bbW as sqInt) - 1) & (self.destPPW - 1)) + 1;
        if (self.bbW as sqInt) < startBits {
            startBits = self.bbW as sqInt;
        }
        if self.destY < self.clipY {
            // Advance increments if there was clipping in y
            pAx += (self.clipY - self.destY) * deltaP12x;
            pAy += (self.clipY - self.destY) * deltaP12y;
            pBx += (self.clipY - self.destY) * deltaP43x;
            pBy += (self.clipY - self.destY) * deltaP43y;
        }
        self.warpLoopSetup();
        if smoothingCount > 1 && (self.cmFlags & COLOR_MAP_NEW_STYLE) == 0 {
            if self.cmLookupTable == 0 {
                if self.destDepth == 16 {
                    self.setupColorMasksFromto(8, 5);
                }
            } else {
                self.setupColorMasksFromto(8, self.cmBitsPerColor);
            }
        }
        let mapperFlags = self.cmFlags & !COLOR_MAP_NEW_STYLE;
        let (dstShiftInc, dstShiftLeft): (sqInt, sqInt) = if self.destMSB != 0 {
            (-(self.destDepth as sqInt), 32 - self.destDepth as sqInt)
        } else {
            (self.destDepth as sqInt, 0)
        };
        if self.noHalftone {
            halftoneWord = ALL_ONES;
        }
        for i in 1..=self.bbH as sqInt {
            // here is the vertical loop...
            let xDelta = deltaFromtonSteps(pAx, pBx, nSteps);
            if xDelta >= 0 {
                self.sx = pAx as i32;
            } else {
                self.sx = (pBx - nSteps * xDelta) as i32;
            }
            let yDelta = deltaFromtonSteps(pAy, pBy, nSteps);
            if yDelta >= 0 {
                self.sy = pAy as i32;
            } else {
                self.sy = (pBy - nSteps * yDelta) as i32;
            }
            if self.destMSB != 0 {
                self.dstBitShift =
                    32 - ((self.dx as sqInt & (self.destPPW - 1)) + 1) * self.destDepth as sqInt;
            } else {
                self.dstBitShift = (self.dx as sqInt & (self.destPPW - 1)) * self.destDepth as sqInt;
            }
            if self.destX < self.clipX {
                // Advance increments if there was clipping in x
                self.sx = (self.sx as sqInt + (self.clipX - self.destX) * xDelta) as i32;
                self.sy = (self.sy as sqInt + (self.clipX - self.destX) * yDelta) as i32;
            }
            if !self.noHalftone {
                halftoneWord = unsafe {
                    long32At(self.halftoneBase.wrapping_add_signed(
                        (((self.dy as sqInt + i) - 1) % self.halftoneHeight) * 4,
                    ))
                };
            }
            self.destMask = self.mask1;
            // Here is the inner loop...
            let mut nPix = startBits;
            let mut words = self.nWords;
            loop {
                let skewWord: u32 = if smoothingCount == 1 {
                    // Faster if not smoothing
                    unsafe {
                        self.warpPickSourcePixels(
                            nPix,
                            xDelta,
                            yDelta,
                            deltaP12x,
                            deltaP12y,
                            dstShiftInc,
                            mapperFlags,
                        )
                    }
                } else {
                    // more difficult with smoothing
                    unsafe {
                        self.warpPickSmoothPixels(
                            nPix,
                            xDelta,
                            yDelta,
                            deltaP12x,
                            deltaP12y,
                            sourceMap,
                            smoothingCount,
                            dstShiftInc,
                        )
                    }
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
                    self.destMask = self.mask2;
                    nPix = endBits;
                } else {
                    self.destMask = ALL_ONES;
                    nPix = self.destPPW;
                }
                words -= 1;
                if words == 0 {
                    break;
                }
            }
            pAx += deltaP12x;
            pAy += deltaP12y;
            pBx += deltaP43x;
            pBy += deltaP43y;
            self.destIndex = self.destIndex.wrapping_add_signed(self.destDelta);
        }
    }
}
