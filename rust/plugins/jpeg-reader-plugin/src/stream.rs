//! The `JPEGReadStream` state and its bit-level reads.
//!
//! The C plugin kept this state in globals (`jsCollection`, `jsPosition`,
//! `jsReadLimit`, `jsBitBuffer`, `jsBitCount`), loaded by
//! `loadJPEGStreamFrom:` from the stream object's first five instance
//! variables and stored back by `storeJPEGStreamOn:`. Here it is a struct so
//! the decode core can be tested without a VM.

use pharo_vm_plugin::sqInt;

use crate::UsqInt;

/// A `JPEGReadStream`, as `loadJPEGStreamFrom:` sees it.
///
/// Invariant, established by [`JpegStream::new`] and preserved by every
/// method: `0 <= position` and `read_limit <= collection.len()`, and the
/// collection is only ever indexed after a `position < read_limit` check —
/// the same guard the C relied on, made load-bearing.
pub struct JpegStream<'a> {
    collection: &'a [u8],
    pub position: sqInt,
    pub read_limit: sqInt,
    pub bit_buffer: sqInt,
    pub bit_count: sqInt,
}

impl<'a> JpegStream<'a> {
    /// Builds a stream, applying exactly the checks `loadJPEGStreamFrom:`
    /// performed: the collection must hold at least `read_limit` bytes, and
    /// the position must lie in `0..read_limit`. `None` where the C answered
    /// 0, failing the primitive.
    pub fn new(
        collection: &'a [u8],
        position: sqInt,
        read_limit: sqInt,
        bit_buffer: sqInt,
        bit_count: sqInt,
    ) -> Option<Self> {
        if (collection.len() as sqInt) < read_limit {
            return None;
        }
        if position < 0 || position >= read_limit {
            return None;
        }
        Some(Self {
            collection,
            position,
            read_limit,
            bit_buffer,
            bit_count,
        })
    }

    /// `JPEGReaderPlugin>>#fillBuffer`: appends whole bytes to the bit buffer
    /// until more than 16 bits are available or input runs out.
    ///
    /// A `0xFF 0x00` pair is the entropy coder's stuffed `0xFF` and is
    /// unstuffed; a `0xFF` followed by anything else is a marker, which is
    /// pushed back and ends the fill.
    fn fill_buffer(&mut self) {
        while self.bit_count <= 16 {
            if self.position >= self.read_limit {
                return;
            }
            let byte = self.collection[self.position as usize];
            self.position += 1;
            if byte == 0xFF {
                // Peek for the stuffing 0x00.
                if !(self.position < self.read_limit
                    && self.collection[self.position as usize] == 0)
                {
                    self.position -= 1;
                    return;
                }
                self.position += 1;
            }
            self.bit_buffer = (((self.bit_buffer as UsqInt) << 8) | UsqInt::from(byte)) as sqInt;
            self.bit_count += 8;
        }
    }

    /// `JPEGReaderPlugin>>#getBits:`: the next `bits_needed` bits, MSB first,
    /// or -1 when the input cannot supply them.
    ///
    /// The C masks the remaining buffer with `(1U << jsBitCount) - 1` — a
    /// *32-bit* one, whatever `sqInt` is — and shifts the buffer by counts
    /// that a corrupt image-side `bitCount` can push past the operand width,
    /// which is undefined behaviour in C. The wrapping shifts here reproduce
    /// what the C binary actually does on the shift-count-masking hardware
    /// this VM ships on.
    pub fn get_bits(&mut self, bits_needed: sqInt) -> sqInt {
        if bits_needed > self.bit_count {
            self.fill_buffer();
            if bits_needed > self.bit_count {
                return -1;
            }
        }
        self.bit_count -= bits_needed;
        let value = (self.bit_buffer as UsqInt).wrapping_shr(self.bit_count as u32) as sqInt;
        let mask = 1u32
            .wrapping_shl(self.bit_count as u32)
            .wrapping_sub(1);
        self.bit_buffer &= mask as sqInt;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(bytes: &[u8]) -> JpegStream<'_> {
        JpegStream::new(bytes, 0, bytes.len() as sqInt, 0, 0).expect("valid stream")
    }

    #[test]
    fn reads_bits_msb_first_across_bytes() {
        let mut s = stream(&[0b1011_0100, 0xC3]);
        assert_eq!(s.get_bits(3), 0b101);
        assert_eq!(s.get_bits(5), 0b10100);
        assert_eq!(s.get_bits(8), 0xC3);
    }

    #[test]
    fn ff00_is_unstuffed_to_ff() {
        let mut s = stream(&[0xFF, 0x00, 0xAB]);
        assert_eq!(s.get_bits(8), 0xFF);
        assert_eq!(s.get_bits(8), 0xAB);
        assert_eq!(s.position, 3);
    }

    #[test]
    fn ff_marker_is_pushed_back_and_starves_the_read() {
        let mut s = stream(&[0xFF, 0xD9]);
        assert_eq!(s.get_bits(8), -1);
        // The marker byte was unread: position is back at the 0xFF.
        assert_eq!(s.position, 0);
        assert_eq!(s.bit_count, 0);
    }

    #[test]
    fn trailing_ff_at_end_of_input_is_pushed_back() {
        let mut s = stream(&[0xAB, 0xFF]);
        assert_eq!(s.get_bits(8), 0xAB);
        // Nothing to peek past the 0xFF: it was unread, not consumed.
        assert_eq!(s.position, 1);
        assert_eq!(s.get_bits(1), -1);
    }

    #[test]
    fn respects_read_limit_not_collection_size() {
        let mut s = JpegStream::new(&[0x01, 0x02, 0x03], 0, 2, 0, 0).unwrap();
        assert_eq!(s.get_bits(16), 0x0102);
        assert_eq!(s.get_bits(8), -1);
    }

    #[test]
    fn resumes_from_stored_bit_state() {
        // Two bits already in the buffer from a previous primitive call.
        let mut s = JpegStream::new(&[0xAA], 0, 1, 0b11, 2).unwrap();
        assert_eq!(s.get_bits(2), 0b11);
        assert_eq!(s.get_bits(8), 0xAA);
    }

    #[test]
    fn insufficient_bits_answer_minus_one_without_consuming_state() {
        let mut s = stream(&[0xAB]);
        assert_eq!(s.get_bits(16), -1);
        // The fill did happen (that is what the C does), but no bits were
        // taken from the buffer.
        assert_eq!(s.bit_count, 8);
        assert_eq!(s.get_bits(8), 0xAB);
    }

    #[test]
    fn new_applies_the_load_checks() {
        // read_limit beyond the collection.
        assert!(JpegStream::new(&[1, 2], 0, 3, 0, 0).is_none());
        // Position at or past the limit, or negative.
        assert!(JpegStream::new(&[1, 2], 2, 2, 0, 0).is_none());
        assert!(JpegStream::new(&[1, 2], -1, 2, 0, 0).is_none());
        // An exhausted (empty-limit) stream never validates.
        assert!(JpegStream::new(&[], 0, 0, 0, 0).is_none());
        assert!(JpegStream::new(&[1, 2], 1, 2, 0, 0).is_some());
    }
}
