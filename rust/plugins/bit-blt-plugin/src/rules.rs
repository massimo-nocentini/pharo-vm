//! The combination rules: one function per rule, plus the partitioned-word
//! helpers they build on.
//!
//! Function-for-function port of the merge functions the C installs in
//! `opTable` (`initBBOpTable`). The C dispatches through a function-pointer
//! table indexed `combinationRule + 1`; here [`BitBlt::mergeFnwith`] is a
//! `match` on the same rule numbers. Functions that read blitter state
//! (`destDepth`, `destMask`, the color map, ...) are methods; the pure word
//! operations are free functions.
//!
//! All 32-bit pixel arithmetic uses `wrapping_*`: the C computes in
//! `unsigned int` (or in 64-bit `sqInt` truncated back to `unsigned int` on
//! assignment), both of which are arithmetic mod 2^32.

use pharo_vm_plugin::sqInt;

use crate::state::{
    shift_sq, shl32, shr32, BitBlt, CmTable, ALPHA_INDEX, BLUE_INDEX, COLOR_MAP_INDEXED_PART,
    COLOR_MAP_FIXED_PART, COLOR_MAP_PRESENT, DITHER8_LOOKUP, GREEN_INDEX, MASK_TABLE, RED_INDEX,
};

// --- Stateless word rules ------------------------------------------------

/// Rule 18: `addWord:with:`.
#[inline]
pub fn addWordwith(sourceWord: u32, destinationWord: u32) -> u32 {
    sourceWord.wrapping_add(destinationWord)
}

/// Rule 19: `subWord:with:`.
#[inline]
pub fn subWordwith(sourceWord: u32, destinationWord: u32) -> u32 {
    sourceWord.wrapping_sub(destinationWord)
}

/// Rule 34/35/36: `alphaBlendScaled:with:` -- premultiplied source over dest.
pub fn alphaBlendScaledwith(sourceWord: u32, destinationWord: u32) -> u32 {
    // High 8 bits of source pixel is source opacity (ARGB format).
    let unAlpha = 0xFF - (sourceWord >> 24);
    // blend red and blue components
    let mut rb = (((destinationWord & 16711935).wrapping_mul(unAlpha) >> 8) & 16711935)
        .wrapping_add(sourceWord & 16711935);
    // blend alpha and green components
    let mut ag = ((((destinationWord >> 8) & 16711935).wrapping_mul(unAlpha) >> 8) & 16711935)
        .wrapping_add((sourceWord >> 8) & 16711935);
    // saturate red and blue components if there is a carry
    rb = (rb & 16711935) | ((rb & 16777472).wrapping_mul(0xFF) >> 8);
    // saturate alpha and green components if there is a carry
    ag = ((ag & 16711935) << 8) | (ag & 16777472).wrapping_mul(0xFF);
    ag | rb
}

/// Rule 24: `alphaBlend:with:` -- blend by the source's alpha byte.
pub fn alphaBlendwith(sourceWord: u32, destinationWord: u32) -> u32 {
    let alpha = sourceWord >> 24;
    if alpha == 0 {
        return destinationWord;
    }
    if alpha == 0xFF {
        return sourceWord;
    }
    let unAlpha = 0xFF - alpha;
    // blend red and blue
    let mut blendRB = (sourceWord & 16711935)
        .wrapping_mul(alpha)
        .wrapping_add((destinationWord & 16711935).wrapping_mul(unAlpha))
        .wrapping_add(16711935);
    // blend alpha and green
    let mut blendAG = (((sourceWord >> 8) | 0xFF0000) & 16711935)
        .wrapping_mul(alpha)
        .wrapping_add(((destinationWord >> 8) & 16711935).wrapping_mul(unAlpha))
        .wrapping_add(16711935);
    // divide by 255
    blendRB = blendRB
        .wrapping_add((blendRB.wrapping_sub(65537) >> 8) & 16711935)
        .wrapping_shr(8)
        & 16711935;
    blendAG = blendAG
        .wrapping_add((blendAG.wrapping_sub(65537) >> 8) & 16711935)
        .wrapping_shr(8)
        & 16711935;
    blendRB | (blendAG << 8)
}

// --- Partitioned helpers -------------------------------------------------

/// `partitionedAdd:to:nBits:componentMask:carryOverflowMask:` -- saturating
/// add of packed components.
pub fn partitionedAddtonBitscomponentMaskcarryOverflowMask(
    word1: u32,
    word2: u32,
    nBits: sqInt,
    componentMask: u32,
    carryOverflowMask: u32,
) -> u32 {
    // mask to remove high bit of each component
    let w1 = word1 & carryOverflowMask;
    let w2 = word2 & carryOverflowMask;
    // sum without high bit to avoid overflowing over next component
    let sum = (word1 ^ w1).wrapping_add(word2 ^ w2);
    // detect overflow condition for saturating
    let carryOverflow = (w1 & w2) | ((w1 | w2) & sum);
    ((sum ^ w1) ^ w2) | shr32(carryOverflow, nBits - 1).wrapping_mul(componentMask)
}

/// `partitionedAND:to:nBits:nPartitions:` -- a field of `word1` passes the
/// matching field of `word2` only when it is all ones.
///
/// The mask variable is `sqInt` in the C, so on an LP64 build `maskTable[32]`
/// (the `int` -1) sign-extends to 64-bit all-ones which a 32-bit field can
/// never equal: with `nBits = 32` the C answers 0 no matter what. Mirrored
/// exactly (including the difference on a 32-bit host, where `sqInt` is
/// 32 bits and the comparison works).
pub fn partitionedANDtonBitsnPartitions(word1: u32, word2: u32, nBits: sqInt, nParts: sqInt) -> u32 {
    // partition mask starts at the right
    let mut mask: sqInt = MASK_TABLE[nBits as usize] as i32 as sqInt;
    let mut result: u32 = 0;
    for _ in 1..=nParts {
        if (word1 as sqInt & mask) == mask {
            result |= (word2 as sqInt & mask) as u32;
        }
        // slide left to next partition
        mask = shift_sq(mask, nBits);
    }
    result
}

/// `partitionedMax:with:nBits:nPartitions:`.
pub fn partitionedMaxwithnBitsnPartitions(word1: u32, word2: u32, nBits: sqInt, nParts: sqInt) -> u32 {
    let mut mask = MASK_TABLE[nBits as usize];
    let mut result: u32 = 0;
    for _ in 1..=nParts {
        result |= (word1 & mask).max(word2 & mask);
        mask = shl32(mask, nBits);
    }
    result
}

/// `partitionedMin:with:nBits:nPartitions:`.
pub fn partitionedMinwithnBitsnPartitions(word1: u32, word2: u32, nBits: sqInt, nParts: sqInt) -> u32 {
    let mut mask = MASK_TABLE[nBits as usize];
    let mut result: u32 = 0;
    for _ in 1..=nParts {
        result |= (word1 & mask).min(word2 & mask);
        mask = shl32(mask, nBits);
    }
    result
}

/// `partitionedMul:with:nBits:nPartitions:` -- per-field
/// `((a+1)*(b+1)-1) >> nBits`, unrolled as in the C.
pub fn partitionedMulwithnBitsnPartitions(word1: u32, word2: u32, nBits: sqInt, nParts: sqInt) -> u32 {
    let sMask = MASK_TABLE[nBits as usize];
    let dMask = shl32(sMask, nBits);
    // optimized first step
    let mut result = ((word1 & sMask) + 1)
        .wrapping_mul((word2 & sMask) + 1)
        .wrapping_sub(1)
        & dMask;
    result = shr32(result, nBits);
    if nParts == 1 {
        return result;
    }
    let mut product = ((shr32(word1, nBits) & sMask) + 1)
        .wrapping_mul((shr32(word2, nBits) & sMask) + 1)
        .wrapping_sub(1)
        & dMask;
    result |= product;
    if nParts == 2 {
        return result;
    }
    product = ((shr32(word1, 2 * nBits) & sMask) + 1)
        .wrapping_mul((shr32(word2, 2 * nBits) & sMask) + 1)
        .wrapping_sub(1)
        & dMask;
    result |= shl32(product, nBits);
    if nParts == 3 {
        return result;
    }
    product = ((shr32(word1, 3 * nBits) & sMask) + 1)
        .wrapping_mul((shr32(word2, 3 * nBits) & sMask) + 1)
        .wrapping_sub(1)
        & dMask;
    result | shl32(product, 2 * nBits)
}

/// `partitionedSub:from:nBits:nPartitions:` -- per-field absolute difference.
pub fn partitionedSubfromnBitsnPartitions(word1: u32, word2: u32, nBits: sqInt, nParts: sqInt) -> u32 {
    let mut mask = MASK_TABLE[nBits as usize];
    let mut result: u32 = 0;
    for _ in 1..=nParts {
        let p1 = word1 & mask;
        let p2 = word2 & mask;
        if p1 < p2 {
            // result is really abs value of the difference
            result |= p2 - p1;
        } else {
            result |= p1 - p2;
        }
        mask = shl32(mask, nBits);
    }
    result
}

/// `rgbMap:from:to:` -- repack a pixel's three color components from
/// `nBitsIn` bits each to `nBitsOut` bits each.
pub fn rgbMapfromto(sourcePixel: sqInt, nBitsIn: sqInt, nBitsOut: sqInt) -> sqInt {
    let mut d = nBitsOut - nBitsIn;
    if d > 0 {
        // Expand to more bits by zero-fill
        let mut mask: sqInt = (shl32(1, nBitsIn).wrapping_sub(1)) as sqInt; // transfer mask
        let mut srcPix = shift_sq(sourcePixel, d);
        mask = shift_sq(mask, d);
        let destPix = srcPix & mask;
        mask = shift_sq(mask, nBitsOut);
        srcPix = shift_sq(srcPix, d);
        (destPix + (srcPix & mask)) + (shift_sq(srcPix, d) & shift_sq(mask, nBitsOut))
    } else {
        // Compress to fewer bits by truncation
        if d == 0 {
            if nBitsIn == 5 {
                // Sometimes called with 16 bits, though pixel is 15,
                // but we must never return more than 15.
                return sourcePixel & 0x7FFF;
            }
            if nBitsIn == 8 {
                // Sometimes called with 32 bits, though pixel is 24,
                // but we must never return more than 24.
                return sourcePixel & 0xFFFFFF;
            }
            return sourcePixel;
        }
        if sourcePixel == 0 {
            return sourcePixel;
        }
        d = nBitsIn - nBitsOut;
        let mut mask: sqInt = (shl32(1, nBitsOut).wrapping_sub(1)) as sqInt; // transfer mask
        let mut srcPix = shift_sq(sourcePixel, -d);
        let mut destPix = srcPix & mask;
        mask = shift_sq(mask, nBitsOut);
        srcPix = shift_sq(srcPix, -d);
        destPix = (destPix + (srcPix & mask)) + (shift_sq(srcPix, -d) & shift_sq(mask, nBitsOut));
        if destPix == 0 {
            return 1;
        }
        destPix
    }
}

/// `rgbMap16To32:` -- expand a 15-bit pixel to 24-bit RGB (no rounding fill).
#[inline]
pub fn rgbMap16To32(sourcePixel: u32) -> u32 {
    ((sourcePixel & 0x1F) << 3) | ((sourcePixel & 0x3E0) << 6) | ((sourcePixel & 0x7C00) << 9)
}

/// `dither32To16:threshold:` via the precomputed lookup.
pub fn dither32To16threshold(srcWord: u32, ditherValue: i32) -> u32 {
    let addThreshold = (ditherValue as usize) << 8;
    ((DITHER8_LOOKUP[addThreshold + ((srcWord >> 16) & 0xFF) as usize] as u32) << 10)
        .wrapping_add((DITHER8_LOOKUP[addThreshold + ((srcWord >> 8) & 0xFF) as usize] as u32) << 5)
        .wrapping_add(DITHER8_LOOKUP[addThreshold + (srcWord & 0xFF) as usize] as u32)
}

// --- Stateful rules ------------------------------------------------------

impl BitBlt {
    /// Rule 30: `alphaBlendConst:with:`.
    pub fn alphaBlendConstwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        self.alphaBlendConstwithpaintMode(sourceWord, destinationWord, false)
    }

    /// Rule 31: `alphaPaintConst:with:`.
    pub fn alphaPaintConstwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if sourceWord == 0 {
            return destinationWord;
        }
        self.alphaBlendConstwithpaintMode(sourceWord, destinationWord, true)
    }

    /// `alphaBlendConst:with:paintMode:` -- blend with the constant
    /// `sourceAlpha`; 16 and 32 bpp destinations only.
    pub fn alphaBlendConstwithpaintMode(
        &mut self,
        sourceWord: u32,
        destinationWord: u32,
        paintMode: bool,
    ) -> u32 {
        if self.destDepth < 16 {
            return destinationWord;
        }
        let sourceAlpha = self.sourceAlpha as u32;
        let unAlpha = 0xFF - sourceAlpha;
        let mut result = destinationWord;
        if self.destPPW == 1 {
            // 32bpp blends include alpha
            if !(paintMode && sourceWord == 0) {
                // blendRB red and blue
                let mut blendRB = (sourceWord & 16711935)
                    .wrapping_mul(sourceAlpha)
                    .wrapping_add((destinationWord & 16711935).wrapping_mul(unAlpha))
                    .wrapping_add(16711935);
                // blendAG alpha and green
                let mut blendAG = ((sourceWord >> 8) & 16711935)
                    .wrapping_mul(sourceAlpha)
                    .wrapping_add(((destinationWord >> 8) & 16711935).wrapping_mul(unAlpha))
                    .wrapping_add(16711935);
                // divide by 255
                blendRB = (blendRB.wrapping_add((blendRB.wrapping_sub(65537) >> 8) & 16711935) >> 8)
                    & 16711935;
                blendAG = (blendAG.wrapping_add((blendAG.wrapping_sub(65537) >> 8) & 16711935) >> 8)
                    & 16711935;
                result = blendRB | (blendAG << 8);
            }
        } else {
            let pixMask = MASK_TABLE[self.destDepth as usize];
            let bitsPerColor: sqInt = 5;
            let rgbMask: u32 = 0x1F;
            let mut maskShifted = self.destMask;
            let mut destShifted = destinationWord;
            let mut sourceShifted = sourceWord;
            for j in 1..=self.destPPW {
                let sourcePixVal = sourceShifted & pixMask;
                if !((maskShifted & pixMask) == 0 || (paintMode && sourcePixVal == 0)) {
                    let destPixVal = destShifted & pixMask;
                    let mut pixBlend: u32 = 0;
                    for i in 1..=3 {
                        let shift = (i - 1) * bitsPerColor;
                        let blend = ((shr32(sourcePixVal, shift) & rgbMask)
                            .wrapping_mul(sourceAlpha)
                            .wrapping_add(
                                (shr32(destPixVal, shift) & rgbMask).wrapping_mul(unAlpha),
                            )
                            .wrapping_add(0xFE)
                            / 0xFF)
                            & rgbMask;
                        pixBlend |= shl32(blend, shift);
                    }
                    result = (result & !shl32(pixMask, (j - 1) * 16))
                        | shl32(pixBlend, (j - 1) * 16);
                }
                maskShifted = shr32(maskShifted, self.destDepth as sqInt);
                sourceShifted = shr32(sourceShifted, self.destDepth as sqInt);
                destShifted = shr32(destShifted, self.destDepth as sqInt);
            }
        }
        result
    }

    /// Rule 20: `rgbAdd:with:`.
    pub fn rgbAddwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            // Add each pixel separately
            let componentMask = shl32(1, self.destDepth as sqInt).wrapping_sub(1);
            let carryOverflowMask =
                shl32(0xFFFF_FFFFu32 / componentMask, self.destDepth as sqInt - 1);
            partitionedAddtonBitscomponentMaskcarryOverflowMask(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                componentMask,
                carryOverflowMask,
            )
        } else if self.destDepth == 16 {
            // Add RGB components of each pixel separately
            partitionedAddtonBitscomponentMaskcarryOverflowMask(
                sourceWord & 2147450879,
                destinationWord & 2147450879,
                5,
                0x1F,
                1108361744,
            )
        } else {
            // Add RGBA components of the pixel separately
            partitionedAddtonBitscomponentMaskcarryOverflowMask(
                sourceWord,
                destinationWord,
                8,
                0xFF,
                2155905152,
            )
        }
    }

    /// Rule 21: `rgbSub:with:`.
    pub fn rgbSubwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            partitionedSubfromnBitsnPartitions(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
        } else if self.destDepth == 16 {
            partitionedSubfromnBitsnPartitions(sourceWord, destinationWord, 5, 3).wrapping_add(
                partitionedSubfromnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3)
                    << 16,
            )
        } else {
            partitionedSubfromnBitsnPartitions(sourceWord, destinationWord, 8, 4)
        }
    }

    /// Rule 27: `rgbMax:with:`.
    pub fn rgbMaxwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            partitionedMaxwithnBitsnPartitions(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
        } else if self.destDepth == 16 {
            partitionedMaxwithnBitsnPartitions(sourceWord, destinationWord, 5, 3).wrapping_add(
                partitionedMaxwithnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3)
                    << 16,
            )
        } else {
            partitionedMaxwithnBitsnPartitions(sourceWord, destinationWord, 8, 4)
        }
    }

    /// Rule 28: `rgbMin:with:`.
    pub fn rgbMinwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            partitionedMinwithnBitsnPartitions(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
        } else if self.destDepth == 16 {
            partitionedMinwithnBitsnPartitions(sourceWord, destinationWord, 5, 3).wrapping_add(
                partitionedMinwithnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3)
                    << 16,
            )
        } else {
            partitionedMinwithnBitsnPartitions(sourceWord, destinationWord, 8, 4)
        }
    }

    /// Rule 29: `rgbMinInvert:with:`.
    pub fn rgbMinInvertwith(&mut self, wordToInvert: u32, destinationWord: u32) -> u32 {
        let sourceWord = !wordToInvert;
        if self.destDepth < 16 {
            partitionedMinwithnBitsnPartitions(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
        } else if self.destDepth == 16 {
            partitionedMinwithnBitsnPartitions(sourceWord, destinationWord, 5, 3).wrapping_add(
                partitionedMinwithnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3)
                    << 16,
            )
        } else {
            partitionedMinwithnBitsnPartitions(sourceWord, destinationWord, 8, 4)
        }
    }

    /// Rule 37: `rgbMul:with:`.
    pub fn rgbMulwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            partitionedMulwithnBitsnPartitions(
                sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
        } else if self.destDepth == 16 {
            partitionedMulwithnBitsnPartitions(sourceWord, destinationWord, 5, 3).wrapping_add(
                partitionedMulwithnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3)
                    << 16,
            )
        } else {
            partitionedMulwithnBitsnPartitions(sourceWord, destinationWord, 8, 4)
        }
    }

    /// Rule 22: `OLDrgbDiff:with:` -- tallies differences into `bitCount`,
    /// leaves the destination untouched.
    pub fn OLDrgbDiffwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth < 16 {
            // Just xor and count differing bits if not RGB
            let mut diff = sourceWord ^ destinationWord;
            let pixMask = MASK_TABLE[self.destDepth as usize];
            while diff != 0 {
                if diff & pixMask != 0 {
                    self.bitCount += 1;
                }
                diff = shr32(diff, self.destDepth as sqInt);
            }
            return destinationWord;
        }
        if self.destDepth == 16 {
            let mut diff = partitionedSubfromnBitsnPartitions(sourceWord, destinationWord, 5, 3);
            self.bitCount = self.bitCount
                + (diff & 0x1F) as sqInt
                + ((diff >> 5) & 0x1F) as sqInt
                + ((diff >> 10) & 0x1F) as sqInt;
            diff =
                partitionedSubfromnBitsnPartitions(sourceWord >> 16, destinationWord >> 16, 5, 3);
            self.bitCount = self.bitCount
                + (diff & 0x1F) as sqInt
                + ((diff >> 5) & 0x1F) as sqInt
                + ((diff >> 10) & 0x1F) as sqInt;
        } else {
            let diff = partitionedSubfromnBitsnPartitions(sourceWord, destinationWord, 8, 3);
            self.bitCount = self.bitCount
                + (diff & 0xFF) as sqInt
                + ((diff >> 8) & 0xFF) as sqInt
                + ((diff >> 16) & 0xFF) as sqInt;
        }
        destinationWord
    }

    /// Rule 23: `OLDtallyIntoMap:with:` -- tallies destination pixels into the
    /// color map, word-clipped (see the C's comment).
    ///
    /// # Safety
    ///
    /// Dereferences `cmLookupTable`; see [`BitBlt::cmLookupAt`].
    pub unsafe fn OLDtallyIntoMapwith(&mut self, _sourceWord: u32, destinationWord: u32) -> u32 {
        if self.cmFlags & (COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART)
            != (COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART)
        {
            return destinationWord;
        }
        if self.destDepth < 16 {
            // loop through all packed pixels.
            let pixMask = MASK_TABLE[self.destDepth as usize] as sqInt & self.cmMask;
            let mut shiftWord = destinationWord;
            for _ in 1..=self.destPPW {
                let mapIndex = shiftWord as sqInt & pixMask;
                let value = unsafe { self.cmLookupAt(mapIndex) }.wrapping_add(1);
                unsafe { self.cmLookupAtPut(mapIndex, value) };
                shiftWord = shr32(shiftWord, self.destDepth as sqInt);
            }
            return destinationWord;
        }
        if self.destDepth == 16 {
            // Two pixels. Tally the right half...
            let mapIndex = rgbMapfromto((destinationWord & 0xFFFF) as sqInt, 5, self.cmBitsPerColor);
            let value = unsafe { self.cmLookupAt(mapIndex) }.wrapping_add(1);
            unsafe { self.cmLookupAtPut(mapIndex, value) };
            // ... then the left half.
            let mapIndex = rgbMapfromto((destinationWord >> 16) as sqInt, 5, self.cmBitsPerColor);
            let value = unsafe { self.cmLookupAt(mapIndex) }.wrapping_add(1);
            unsafe { self.cmLookupAtPut(mapIndex, value) };
        } else {
            // Just one pixel.
            let mapIndex = rgbMapfromto(destinationWord as sqInt, 8, self.cmBitsPerColor);
            let value = unsafe { self.cmLookupAt(mapIndex) }.wrapping_add(1);
            unsafe { self.cmLookupAtPut(mapIndex, value) };
        }
        destinationWord
    }

    /// Rule 32: `rgbDiff:with:` -- like rule 22, but respects the destination
    /// mask so only pixels inside the rectangle are tallied.
    pub fn rgbDiffwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        let pixMask = MASK_TABLE[self.destDepth as usize];
        let (bitsPerColor, rgbMask): (sqInt, u32) = if self.destDepth == 16 {
            (5, 0x1F)
        } else {
            (8, 0xFF)
        };
        let mut maskShifted = self.destMask;
        let mut destShifted = destinationWord;
        let mut sourceShifted = sourceWord;
        for _ in 1..=self.destPPW {
            if (maskShifted & pixMask) > 0 {
                // Only tally pixels within the destination rectangle
                let destPixVal = destShifted & pixMask;
                let sourcePixVal = sourceShifted & pixMask;
                let diff: sqInt = if self.destDepth < 16 {
                    if sourcePixVal == destPixVal {
                        0
                    } else {
                        1
                    }
                } else {
                    let d = partitionedSubfromnBitsnPartitions(
                        sourcePixVal,
                        destPixVal,
                        bitsPerColor,
                        3,
                    );
                    (d & rgbMask) as sqInt
                        + (shr32(d, bitsPerColor) & rgbMask) as sqInt
                        + (shr32(shr32(d, bitsPerColor), bitsPerColor) & rgbMask) as sqInt
                };
                self.bitCount += diff;
            }
            maskShifted = shr32(maskShifted, self.destDepth as sqInt);
            sourceShifted = shr32(sourceShifted, self.destDepth as sqInt);
            destShifted = shr32(destShifted, self.destDepth as sqInt);
        }
        destinationWord
    }

    /// Rule 33: `tallyIntoMap:with:` -- destination-masked tally.
    ///
    /// # Safety
    ///
    /// Dereferences `cmLookupTable`; see [`BitBlt::cmLookupAt`].
    pub unsafe fn tallyIntoMapwith(&mut self, _sourceWord: u32, destinationWord: u32) -> u32 {
        if self.cmFlags & (COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART)
            != (COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART)
        {
            return destinationWord;
        }
        let pixMask = MASK_TABLE[self.destDepth as usize];
        let mut destShifted = destinationWord;
        let mut maskShifted = self.destMask;
        for _ in 1..=self.destPPW {
            if maskShifted & pixMask != 0 {
                // Only tally pixels within the destination rectangle
                let pixVal = destShifted & pixMask;
                let mapIndex: sqInt = if self.destDepth < 16 {
                    pixVal as sqInt
                } else if self.destDepth == 16 {
                    rgbMapfromto(pixVal as sqInt, 5, self.cmBitsPerColor)
                } else {
                    rgbMapfromto(pixVal as sqInt, 8, self.cmBitsPerColor)
                };
                let value = unsafe { self.cmLookupAt(mapIndex) }.wrapping_add(1);
                unsafe { self.cmLookupAtPut(mapIndex, value) };
            }
            maskShifted = shr32(maskShifted, self.destDepth as sqInt);
            destShifted = shr32(destShifted, self.destDepth as sqInt);
        }
        destinationWord
    }

    /// Rule 39: `pixClear:with:` -- zero destination pixels equal to the
    /// matching source pixel.
    pub fn pixClearwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth == 32 {
            return if sourceWord == destinationWord {
                0
            } else {
                destinationWord
            };
        }
        let nBits = self.destDepth as sqInt;
        let mut mask = MASK_TABLE[nBits as usize];
        let mut result: u32 = 0;
        for _ in 1..=self.destPPW {
            let mut pv = destinationWord & mask;
            if sourceWord & mask == pv {
                pv = 0;
            }
            result |= pv;
            mask = shl32(mask, nBits);
        }
        result
    }

    /// Rule 26: `pixMask:with:`.
    pub fn pixMaskwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        partitionedANDtonBitsnPartitions(
            !sourceWord,
            destinationWord,
            self.destDepth as sqInt,
            self.destPPW,
        )
    }

    /// Rule 25: `pixPaint:with:`.
    pub fn pixPaintwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if sourceWord == 0 {
            return destinationWord;
        }
        sourceWord
            | partitionedANDtonBitsnPartitions(
                !sourceWord,
                destinationWord,
                self.destDepth as sqInt,
                self.destPPW,
            )
    }

    /// Rule 38: `pixSwap:with:` -- reverse the pixel order within the word.
    pub fn pixSwapwith(&mut self, _sourceWord: u32, destWord: u32) -> u32 {
        if self.destPPW == 1 {
            return destWord;
        }
        let destDepth = self.destDepth as sqInt;
        let mut result: u32 = 0;
        // mask low pixel / mask high pixel
        let mut lowMask = shl32(1, destDepth).wrapping_sub(1);
        let mut highMask = shl32(lowMask, (self.destPPW - 1) * destDepth);
        let mut shift = 32 - destDepth;
        result |= shl32(destWord & lowMask, shift) | shr32(destWord & highMask, shift);
        if self.destPPW <= 2 {
            return result;
        }
        for _ in 2..=(self.destPPW / 2) {
            lowMask = shl32(lowMask, destDepth);
            highMask = shr32(highMask, destDepth);
            shift -= destDepth * 2;
            result |= shl32(destWord & lowMask, shift) | shr32(destWord & highMask, shift);
        }
        result
    }

    /// Rule 40: `fixAlpha:with:` -- fill zero alpha channels from the source.
    pub fn fixAlphawith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if self.destDepth != 32 {
            return destinationWord;
        }
        if destinationWord == 0 {
            return 0;
        }
        if destinationWord & 0xFF000000 != 0 {
            return destinationWord;
        }
        destinationWord | (sourceWord & 0xFF000000)
    }

    /// `rgbComponentAlpha32:with:` -- the rule-41 kernel on two 32-bit pixels.
    ///
    /// # Safety
    ///
    /// Dereferences the gamma tables when they are set; the loader only sets
    /// them from byte objects of the image (at least 256 bytes are assumed,
    /// as the C assumes).
    pub unsafe fn rgbComponentAlpha32with(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        let mut alpha = sourceWord as sqInt;
        if alpha == 0 {
            return destinationWord;
        }
        let mut srcColor = self.componentAlphaModeColor;
        let srcAlpha = self.componentAlphaModeAlpha & 0xFF;
        let aB0 = alpha & 0xFF;
        alpha = shift_sq(alpha, -8);
        let aG0 = alpha & 0xFF;
        alpha = shift_sq(alpha, -8);
        let aR0 = alpha & 0xFF;
        alpha = shift_sq(alpha, -8);
        let aA0 = alpha & 0xFF;
        let (aA, aR, aG, aB) = if srcAlpha != 0xFF {
            (
                (aA0 * srcAlpha) >> 8,
                (aR0 * srcAlpha) >> 8,
                (aG0 * srcAlpha) >> 8,
                (aB0 * srcAlpha) >> 8,
            )
        } else {
            (aA0, aR0, aG0, aB0)
        };
        let ungamma = |v: sqInt, table: usize| -> sqInt {
            if table == 0 {
                v
            } else {
                // SAFETY: the caller's contract; index is 0..256.
                unsafe { byte_at(table + v as usize) as sqInt }
            }
        };
        let mut dstMask = destinationWord as sqInt;

        let mut d = ungamma(dstMask & 0xFF, self.ungammaLookupTable);
        let mut s = ungamma(srcColor & 0xFF, self.ungammaLookupTable);
        let mut b = ((d * (0xFF - aB)) >> 8) + ((s * aB) >> 8);
        if b > 0xFF {
            b = 0xFF;
        }
        b = ungamma(b, self.gammaLookupTable);
        dstMask = shift_sq(dstMask, -8);
        srcColor = shift_sq(srcColor, -8);

        d = ungamma(dstMask & 0xFF, self.ungammaLookupTable);
        s = ungamma(srcColor & 0xFF, self.ungammaLookupTable);
        let mut g = ((d * (0xFF - aG)) >> 8) + ((s * aG) >> 8);
        if g > 0xFF {
            g = 0xFF;
        }
        g = ungamma(g, self.gammaLookupTable);
        dstMask = shift_sq(dstMask, -8);
        srcColor = shift_sq(srcColor, -8);

        d = ungamma(dstMask & 0xFF, self.ungammaLookupTable);
        s = ungamma(srcColor & 0xFF, self.ungammaLookupTable);
        let mut r = ((d * (0xFF - aR)) >> 8) + ((s * aR) >> 8);
        if r > 0xFF {
            r = 0xFF;
        }
        r = ungamma(r, self.gammaLookupTable);
        dstMask = shift_sq(dstMask, -8);

        // no need to gamma correct alpha value ?
        let mut a = (((dstMask & 0xFF) * (0xFF - aA)) >> 8) + aA;
        if a > 0xFF {
            a = 0xFF;
        }
        (shift_sq(shift_sq(shift_sq(a, 8) + r, 8) + g, 8) + b) as u32
    }

    /// Rule 41: `rgbComponentAlpha:with:` -- the general (any destination
    /// depth) form, partitioned as in the C's inlined
    /// `partitionedRgbComponentAlpha:dest:nBits:nPartitions:`.
    ///
    /// # Safety
    ///
    /// As [`BitBlt::rgbComponentAlpha32with`].
    pub unsafe fn rgbComponentAlphawith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        if sourceWord == 0 {
            return destinationWord;
        }
        let destDepth = self.destDepth as sqInt;
        let mut mask = MASK_TABLE[self.destDepth as usize];
        let mut result: u32 = 0;
        for i in 1..=self.destPPW {
            let mut p1 = shr32(sourceWord & mask, (i - 1) * destDepth);
            let mut p2 = shr32(destinationWord & mask, (i - 1) * destDepth);
            if destDepth != 32 {
                if destDepth == 16 {
                    p1 = ((p1 & 0x1F) << 3) | ((p1 & 0x3E0) << 6) | ((p1 & 0x7C00) << 9)
                        | 0xFF000000;
                    p2 = ((p2 & 0x1F) << 3) | ((p2 & 0x3E0) << 6) | ((p2 & 0x7C00) << 9)
                        | 0xFF000000;
                } else {
                    p1 = rgbMapfromto(p1 as sqInt, destDepth, 32) as u32 | 0xFF000000;
                    p2 = rgbMapfromto(p2 as sqInt, destDepth, 32) as u32 | 0xFF000000;
                }
            }
            let mut v = unsafe { self.rgbComponentAlpha32with(p1, p2) } as sqInt;
            if destDepth != 32 {
                v = rgbMapfromto(v, 32, destDepth);
            }
            result |= shl32(v as u32, (i - 1) * destDepth);
            mask = shl32(mask, destDepth);
        }
        result
    }

    /// `rgbMapPixel:flags:` -- apply the fixed (shift/mask) part of the map.
    pub fn rgbMapPixelflags(&self, sourcePixel: sqInt) -> sqInt {
        let term = |i: usize| -> sqInt {
            let shift = self.cmShiftAt(i) as sqInt;
            let masked = sourcePixel & self.cmMaskAt(i) as sqInt;
            shift_sq(masked, shift)
        };
        term(0) | term(1) | term(2) | term(3)
    }

    /// `mapPixel:flags:` -- the full color-map transform of one pixel.
    ///
    /// # Safety
    ///
    /// Dereferences `cmLookupTable` when the indexed part is present; see
    /// [`BitBlt::cmLookupAt`].
    pub unsafe fn mapPixelflags(&self, sourcePixel: sqInt, mapperFlags: sqInt) -> sqInt {
        let mut pv = sourcePixel;
        if mapperFlags & COLOR_MAP_PRESENT != 0 {
            if mapperFlags & COLOR_MAP_FIXED_PART != 0 {
                pv = self.rgbMapPixelflags(sourcePixel);
                if pv == 0 && sourcePixel != 0 {
                    pv = 1;
                }
            }
            if mapperFlags & COLOR_MAP_INDEXED_PART != 0 {
                pv = unsafe { self.cmLookupAt(pv) } as sqInt;
            }
        }
        pv
    }

    /// `isIdentityMap:with:` on the current shift/mask tables.
    pub fn isIdentityMapwith(&self) -> bool {
        if self.cmShiftTable == CmTable::Null || self.cmMaskTable == CmTable::Null {
            return true;
        }
        self.cmShiftAt(RED_INDEX) == 0
            && self.cmShiftAt(GREEN_INDEX) == 0
            && self.cmShiftAt(BLUE_INDEX) == 0
            && self.cmShiftAt(ALPHA_INDEX) == 0
            && self.cmMaskAt(RED_INDEX) == 0xFF0000
            && self.cmMaskAt(GREEN_INDEX) == 0xFF00
            && self.cmMaskAt(BLUE_INDEX) == 0xFF
            && self.cmMaskAt(ALPHA_INDEX) == 0xFF000000
    }

    /// `setupColorMasks` -- derive the implicit RGB conversion for old-style
    /// color maps. (The C warns: for WarpBlt with smoothing the source depth
    /// is wrong here; mirrored.)
    pub fn setupColorMasks(&mut self) {
        let mut bits: sqInt = 0;
        if self.sourceDepth <= 8 {
            return;
        }
        if self.sourceDepth == 16 {
            bits = 5;
        }
        if self.sourceDepth == 32 {
            bits = 8;
        }
        let targetBits: sqInt = if self.cmBitsPerColor == 0 {
            // Convert to destDepth
            if self.destDepth <= 8 {
                return;
            }
            if self.destDepth == 16 {
                5
            } else if self.destDepth == 32 {
                8
            } else {
                0
            }
        } else {
            self.cmBitsPerColor
        };
        self.setupColorMasksFromto(bits, targetBits);
    }

    /// `setupColorMasksFrom:to:` -- fill the local shift/mask tables.
    pub fn setupColorMasksFromto(&mut self, srcBits: sqInt, targetBits: sqInt) {
        let deltaBits = targetBits - srcBits;
        if deltaBits == 0 {
            return;
        }
        if deltaBits <= 0 {
            // Mask for extracting a color part of the source
            let mask = shl32(1, targetBits).wrapping_sub(1);
            self.cmLocalMasks[RED_INDEX] = shl32(mask, (srcBits * 2) - deltaBits);
            self.cmLocalMasks[GREEN_INDEX] = shl32(mask, srcBits - deltaBits);
            self.cmLocalMasks[BLUE_INDEX] = shl32(mask, -deltaBits);
            self.cmLocalMasks[ALPHA_INDEX] = 0;
        } else {
            let mask = shl32(1, srcBits).wrapping_sub(1);
            self.cmLocalMasks[RED_INDEX] = shl32(mask, srcBits * 2);
            self.cmLocalMasks[GREEN_INDEX] = shl32(mask, srcBits);
            self.cmLocalMasks[BLUE_INDEX] = mask;
            // The C leaves masks[AlphaIndex] at whatever the static held;
            // since these statics start zeroed and nothing else writes the
            // alpha slot, that value is 0.
            self.cmLocalMasks[ALPHA_INDEX] = 0;
        }
        self.cmLocalShifts[RED_INDEX] = (deltaBits * 3) as i32;
        self.cmLocalShifts[GREEN_INDEX] = (deltaBits * 2) as i32;
        self.cmLocalShifts[BLUE_INDEX] = deltaBits as i32;
        self.cmLocalShifts[ALPHA_INDEX] = 0;
        self.cmShiftTable = CmTable::Local;
        self.cmMaskTable = CmTable::Local;
        self.cmFlags |= COLOR_MAP_PRESENT | COLOR_MAP_FIXED_PART;
    }

    /// The `opTable` dispatch: `opTable[combinationRule + 1](source, dest)`.
    ///
    /// Rules 16, 17 and anything out of 0..=41 answer the destination, as the
    /// C's table does for 15/16/17 (out-of-range rules are rejected at load).
    ///
    /// # Safety
    ///
    /// Rules 23, 33 and 41 dereference the color map / gamma tables; the
    /// loader's validation (or the test harness) must have set those up.
    pub unsafe fn mergeFnwith(&mut self, sourceWord: u32, destinationWord: u32) -> u32 {
        match self.combinationRule {
            0 => 0,                                                    // clearWord
            1 => sourceWord & destinationWord,                         // bitAnd
            2 => sourceWord & !destinationWord,                        // bitAndInvert
            3 => sourceWord,                                           // sourceWord
            4 => !sourceWord & destinationWord,                        // bitInvertAnd
            5 => destinationWord,                                      // destinationWord
            6 => sourceWord ^ destinationWord,                         // bitXor
            7 => sourceWord | destinationWord,                         // bitOr
            8 => !sourceWord & !destinationWord,                       // bitInvertAndInvert
            9 => !sourceWord ^ destinationWord,                        // bitInvertXor
            10 => !destinationWord,                                    // bitInvertDestination
            11 => sourceWord | !destinationWord,                       // bitOrInvert
            12 => !sourceWord,                                         // bitInvertSource
            13 => !sourceWord | destinationWord,                       // bitInvertOr
            14 => !sourceWord | !destinationWord,                      // bitInvertOrInvert
            15..=17 => destinationWord,                                // destinationWord
            18 => addWordwith(sourceWord, destinationWord),
            19 => subWordwith(sourceWord, destinationWord),
            20 => self.rgbAddwith(sourceWord, destinationWord),
            21 => self.rgbSubwith(sourceWord, destinationWord),
            22 => self.OLDrgbDiffwith(sourceWord, destinationWord),
            23 => unsafe { self.OLDtallyIntoMapwith(sourceWord, destinationWord) },
            24 => alphaBlendwith(sourceWord, destinationWord),
            25 => self.pixPaintwith(sourceWord, destinationWord),
            26 => self.pixMaskwith(sourceWord, destinationWord),
            27 => self.rgbMaxwith(sourceWord, destinationWord),
            28 => self.rgbMinwith(sourceWord, destinationWord),
            29 => self.rgbMinInvertwith(sourceWord, destinationWord),
            30 => self.alphaBlendConstwith(sourceWord, destinationWord),
            31 => self.alphaPaintConstwith(sourceWord, destinationWord),
            32 => self.rgbDiffwith(sourceWord, destinationWord),
            33 => unsafe { self.tallyIntoMapwith(sourceWord, destinationWord) },
            34..=36 => alphaBlendScaledwith(sourceWord, destinationWord),
            37 => self.rgbMulwith(sourceWord, destinationWord),
            38 => self.pixSwapwith(sourceWord, destinationWord),
            39 => self.pixClearwith(sourceWord, destinationWord),
            40 => self.fixAlphawith(sourceWord, destinationWord),
            41 => unsafe { self.rgbComponentAlphawith(sourceWord, destinationWord) },
            _ => destinationWord,
        }
    }
}

/// A byte read used by the gamma tables.
///
/// # Safety
///
/// `addr` must be within a live buffer.
#[inline(always)]
unsafe fn byte_at(addr: usize) -> u8 {
    unsafe { crate::state::byteAtPointer(addr) }
}
