//! The algorithmic core of `MiscPrimitivePlugin`, free of any VM dependency.
//!
//! Everything here is a literal transcription of the Slang-generated C in
//! `plugins/MiscPrimitivePlugin/src/common/MiscPrimitivePlugin.c`, minus the
//! out-of-bounds reads (see [`decompress`]). Keeping the algorithms pure lets
//! them be unit-tested without a running VM; the primitive shells in
//! `lib.rs` only fetch arguments, run the C's validation sequence, and call
//! in here.

/// Answer of a collated string comparison: `1` before, `2` equal, `3` after.
///
/// `ByteString class>>compare:with:collated:`. Both strings are compared
/// byte-wise through `order` (a 256-entry collation table); the shorter
/// string sorts first on a tie. `order` must have at least 256 entries --
/// the caller has checked that, as the C did.
pub fn compare_collated(s1: &[u8], s2: &[u8], order: &[u8]) -> isize {
    let min = s1.len().min(s2.len());
    for i in 0..min {
        let c1 = order[usize::from(s1[i])];
        let c2 = order[usize::from(s2[i])];
        if c1 != c2 {
            return if c1 < c2 { 1 } else { 3 };
        }
    }
    if s1.len() == s2.len() {
        2
    } else if s1.len() < s2.len() {
        1
    } else {
        3
    }
}

/// 1-based position of the first byte of `s`, at or after 0-based `start0`,
/// whose entry in `inclusion_map` is non-zero; `0` when there is none.
///
/// `ByteString class>>findFirstInString:inSet:startingAt:`. The caller has
/// checked `start0 >= 0` and `inclusion_map.len() == 256`, as the C did.
pub fn find_first_in_string(s: &[u8], inclusion_map: &[u8], start0: usize) -> usize {
    let mut i = start0;
    while i < s.len() && inclusion_map[usize::from(s[i])] == 0 {
        i += 1;
    }
    if i >= s.len() {
        0
    } else {
        i + 1
    }
}

/// 1-based position of `key` in `body` at or after 1-based `start`, matching
/// bytes through `match_table`; `0` when absent or `key` is empty.
///
/// `ByteString>>findSubstring:in:startingAt:matchTable:`. A `start` below 1
/// is clamped to 1, as the C did. `match_table` must have at least 256
/// entries -- caller-checked.
pub fn find_substring(key: &[u8], body: &[u8], start: isize, match_table: &[u8]) -> usize {
    if key.is_empty() {
        return 0;
    }
    // The C works with keySize adjusted down by one ("zero relative"); kept
    // here so the loop shapes line up with the generated code.
    let key_last = key.len() - 1;
    let start0 = if start - 1 < 0 { 0 } else { (start - 1) as usize };
    let limit = body.len() as isize - 1 - key_last as isize;
    let mut start_index = start0 as isize;
    while start_index <= limit {
        let base = start_index as usize;
        let mut index = 0;
        while match_table[usize::from(body[base + index])]
            == match_table[usize::from(key[index])]
        {
            if index == key_last {
                return base + 1;
            }
            index += 1;
        }
        start_index += 1;
    }
    0
}

/// 1-based position of the first byte of `s` equal to `ascii`, at or after
/// 1-based `start`; `0` when absent.
///
/// `ByteString>>indexOfAscii:inString:startingAt:`. The comparison is
/// byte-against-`sqInt`, exactly as in the C: a value outside 0..=255 simply
/// never matches. The caller has checked `start >= 1`.
pub fn index_of_ascii(ascii: isize, s: &[u8], start: isize) -> isize {
    let mut pos = (start - 1) as usize;
    while pos < s.len() {
        if isize::from(s[pos]) == ascii {
            return pos as isize + 1;
        }
        pos += 1;
    }
    0
}

/// The image's byte-array hash: fold each byte into a 32-bit accumulator with
/// the multiplier 1664525, then keep the low 28 bits.
///
/// `ByteArray class>>hashBytes:startingWith:`. All arithmetic is modulo
/// 2^32, matching the C's `unsigned int`.
pub fn hash_bytes(bytes: &[u8], initial: u32) -> u32 {
    let mut hash = initial;
    for &b in bytes {
        hash = hash.wrapping_add(u32::from(b)).wrapping_mul(1_664_525);
    }
    hash & 0x0FFF_FFFF
}

/// Replaces every byte of `region` with its entry in `table`.
///
/// `ByteString class>>translate:from:to:table:`, restricted to the
/// non-aliased case; when the image passes the string as its own table the
/// shell runs this over a single shared buffer instead (see `lib.rs`).
/// `table` must have at least 256 entries -- caller-checked.
pub fn translate(region: &mut [u8], table: &[u8]) {
    for b in region {
        *b = table[usize::from(*b)];
    }
}

/// One sample of `SampledSound class>>convert8bitSignedFrom:to16Bit:`: a
/// signed 8-bit sample scaled to signed 16 bits.
///
/// The C spells this as two branches -- `(s - 256) << 8` for `s > 0x7F`,
/// `s << 8` otherwise -- but truncation to `unsigned short` makes both equal
/// to a plain shift, which is what this is.
pub fn sample_16(sample: u8) -> u16 {
    u16::from(sample) << 8
}

/// Bytes `Bitmap class>>compress:toByteArray:` requires the destination to
/// hold before it will run: the worst-case output size plus slack, exactly
/// the C's `(size * 4 + 7) + (size // 0x7C0) * 3`.
pub fn compress_bound(size: usize) -> usize {
    size.saturating_mul(4)
        .saturating_add(7)
        .saturating_add((size / 0x7C0).saturating_mul(3))
}

/// Where [`compress_into`] writes: the destination's own bytes, and how far
/// into them the stream has got.
///
/// The C encoded straight into the ByteArray it was given, and so does this;
/// the caller has already checked that it holds [`compress_bound`] bytes, so
/// every write here is in bounds.
struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Out<'_> {
    fn push(&mut self, byte: u8) {
        self.buf[self.len] = byte;
        self.len += 1;
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
}

/// Appends the C's variable-length integer encoding (`encodeInt:in:at:`):
/// one byte up to 223, two bytes up to 7935, else `0xFF` and four big-endian
/// bytes (of the value truncated to 32 bits, as the C's cast does).
fn encode_int(v: usize, out: &mut Out) {
    if v <= 223 {
        out.push(v as u8);
    } else if v <= 7935 {
        out.push((v / 256 + 224) as u8);
        out.push((v % 256) as u8);
    } else {
        out.push(0xFF);
        out.extend_from_slice(&(v as u32).to_be_bytes());
    }
}

/// Run-length encodes a Bitmap's words, `Bitmap class>>compress:toByteArray:`.
///
/// The stream is the encoded word count followed by tokens `len*4 + code`:
/// code 1 a run of words all four of whose bytes equal the following byte,
/// code 2 a run of one word given big-endian, code 3 that many verbatim
/// big-endian words. The output never exceeds [`compress_bound`] of the
/// input length -- asserted in the tests, relied on by the C, which sized
/// its destination check with it.
/// The reference spelling the tests compare against: [`compress_into`] a
/// buffer of the bound, trimmed to length. The primitive encodes into the
/// image object instead and never builds one of these.
#[cfg(test)]
pub fn compress(bm: &[u32]) -> Vec<u8> {
    let mut buf = vec![0u8; compress_bound(bm.len())];
    let len = compress_into(bm, &mut buf);
    buf.truncate(len);
    buf
}

/// Encodes into `dst`, answering how many bytes it took.
///
/// `dst` must hold at least [`compress_bound`] of `bm.len()` bytes, which is
/// the check the C made -- and makes here -- before calling.
pub fn compress_into(bm: &[u32], dst: &mut [u8]) -> usize {
    let size = bm.len();
    debug_assert!(dst.len() >= compress_bound(size));
    let mut out = Out { buf: dst, len: 0 };
    encode_int(size, &mut out);
    let mut k = 0;
    while k < size {
        let word = bm[k];
        let low_byte = word & 0xFF;
        let eq_bytes = (word >> 8) & 0xFF == low_byte
            && (word >> 16) & 0xFF == low_byte
            && (word >> 24) & 0xFF == low_byte;
        let mut j = k;
        while j + 1 < size && word == bm[j + 1] {
            j += 1;
        }
        if j > k {
            // A run of two or more equal words.
            if eq_bytes {
                encode_int((j - k + 1) * 4 + 1, &mut out);
                out.push(low_byte as u8);
            } else {
                encode_int((j - k + 1) * 4 + 2, &mut out);
                out.extend_from_slice(&word.to_be_bytes());
            }
            k = j + 1;
        } else if eq_bytes {
            // One word on its own, but its four bytes agree.
            encode_int(4 + 1, &mut out);
            out.push(low_byte as u8);
            k += 1;
        } else {
            // Verbatim words up to (and excluding) the next equal pair. The
            // C's closing `if (j + 1 == size) j += 1` folds the final word
            // into the run when the scan hit the end.
            while j + 1 < size && bm[j] != bm[j + 1] {
                j += 1;
            }
            if j + 1 == size {
                j += 1;
            }
            encode_int((j - k) * 4 + 3, &mut out);
            for &w in &bm[k..j] {
                out.extend_from_slice(&w.to_be_bytes());
            }
            k = j;
        }
    }
    out.len
}

/// Why [`decompress`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecompressError {
    /// A length or data byte lay outside the byte array. The C performs the
    /// read anyway -- out of bounds -- so this variant is this port's
    /// defined stand-in for that undefined behaviour.
    TruncatedInput,
    /// A run would have gone past the destination's element count, or past
    /// the words the destination actually holds; the C fails with
    /// `PrimErrBadIndex` for the first and wrote out of bounds for the
    /// second.
    WouldOverrun,
}

/// Decodes a [`compress`] stream, `Bitmap>>decompress:fromByteArray:at:`.
///
/// `start` is the 0-based position in `ba` to decode from (the primitive's
/// 1-based `index` minus one -- past the size header, which this function
/// does not interpret). `past_end` is the destination's element count as the
/// image reports it, which the C checked each run against; `dst` is the
/// destination itself, filled run by run in stream order, so a mid-stream
/// failure leaves the earlier runs in place exactly as the C's in-place
/// writes did.
pub fn decompress(
    ba: &[u8],
    start: isize,
    past_end: usize,
    dst: &mut [u32],
) -> Result<(), DecompressError> {
    /// One byte at `*i`, or `None` outside the array (a negative `start`
    /// makes the very first read negative, which the C also does not check).
    fn read_byte(ba: &[u8], i: &mut isize) -> Option<u32> {
        let b = usize::try_from(*i).ok().and_then(|idx| ba.get(idx).copied())?;
        *i += 1;
        Some(u32::from(b))
    }

    /// The `n` destination words at `at`, or `WouldOverrun` if the object
    /// is shorter than its element count promised.
    fn run(dst: &mut [u32], at: usize, n: usize) -> Result<&mut [u32], DecompressError> {
        dst.get_mut(at..at + n).ok_or(DecompressError::WouldOverrun)
    }

    let end = ba.len() as isize;
    let mut i = start;
    let mut k: usize = 0;
    while i < end {
        let mut an_int =
            read_byte(ba, &mut i).ok_or(DecompressError::TruncatedInput)?;
        if an_int > 223 {
            if an_int <= 0xFE {
                let b = read_byte(ba, &mut i).ok_or(DecompressError::TruncatedInput)?;
                an_int = (an_int - 224) * 256 + b;
            } else {
                an_int = 0;
                for _ in 0..4 {
                    let b =
                        read_byte(ba, &mut i).ok_or(DecompressError::TruncatedInput)?;
                    an_int = (an_int << 8) + b;
                }
            }
        }
        let n = (an_int >> 2) as usize;
        // The C bounds-checks the destination before dispatching on the
        // code, including for code 0, which writes nothing.
        match k.checked_add(n) {
            Some(kn) if kn <= past_end => {}
            _ => return Err(DecompressError::WouldOverrun),
        }
        match an_int & 3 {
            1 => {
                let b = read_byte(ba, &mut i).ok_or(DecompressError::TruncatedInput)?;
                let data = b | (b << 8);
                let data = data | (data << 16);
                run(dst, k, n)?.fill(data);
                k += n;
            }
            2 => {
                let mut data = 0u32;
                for _ in 0..4 {
                    let b =
                        read_byte(ba, &mut i).ok_or(DecompressError::TruncatedInput)?;
                    data = (data << 8) | b;
                }
                run(dst, k, n)?.fill(data);
                k += n;
            }
            3 => {
                // Word by word into the destination, as the C read and
                // stored them; a stream that runs out mid-run leaves the
                // words already decoded in place, where the C read on past
                // the byte array instead.
                let out = run(dst, k, n)?;
                for slot in out.iter_mut() {
                    let mut data = 0u32;
                    for _ in 0..4 {
                        let b = read_byte(ba, &mut i)
                            .ok_or(DecompressError::TruncatedInput)?;
                        data = (data << 8) | b;
                    }
                    *slot = data;
                }
                k += n;
            }
            // Code 0 is "nil" in the Smalltalk original: nothing is written
            // and, notably, k does not advance.
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- collated comparison ----------------------------------------------

    /// The identity collation, what `String>>compare:` passes for
    /// case-sensitive comparison.
    fn identity_order() -> Vec<u8> {
        (0..=255).collect()
    }

    #[test]
    fn compare_equal_less_greater() {
        let order = identity_order();
        assert_eq!(compare_collated(b"abc", b"abc", &order), 2);
        assert_eq!(compare_collated(b"abc", b"abd", &order), 1);
        assert_eq!(compare_collated(b"abd", b"abc", &order), 3);
    }

    #[test]
    fn compare_prefix_sorts_first() {
        let order = identity_order();
        assert_eq!(compare_collated(b"ab", b"abc", &order), 1);
        assert_eq!(compare_collated(b"abc", b"ab", &order), 3);
        assert_eq!(compare_collated(b"", b"", &order), 2);
        assert_eq!(compare_collated(b"", b"a", &order), 1);
        assert_eq!(compare_collated(b"a", b"", &order), 3);
    }

    #[test]
    fn compare_respects_the_order_table() {
        // A case-folding table makes 'A' and 'a' compare equal.
        let mut order = identity_order();
        for c in b'A'..=b'Z' {
            order[usize::from(c)] = c + 32;
        }
        assert_eq!(compare_collated(b"ABC", b"abc", &order), 2);
        // An order that inverts bytes flips the verdict.
        let inverted: Vec<u8> = (0..=255u8).map(|b| 255 - b).collect();
        assert_eq!(compare_collated(b"a", b"b", &inverted), 3);
    }

    // ---- findFirstInString -------------------------------------------------

    #[test]
    fn find_first_hits_and_misses() {
        let mut map = [0u8; 256];
        map[usize::from(b',')] = 1;
        map[usize::from(b' ')] = 1;
        assert_eq!(find_first_in_string(b"hello, world", &map, 0), 6);
        assert_eq!(find_first_in_string(b"hello, world", &map, 5), 6);
        assert_eq!(find_first_in_string(b"hello, world", &map, 6), 7);
        assert_eq!(find_first_in_string(b"hello, world", &map, 7), 0);
        assert_eq!(find_first_in_string(b"hello", &map, 0), 0);
        assert_eq!(find_first_in_string(b"", &map, 0), 0);
        // A start past the end answers 0, not an error.
        assert_eq!(find_first_in_string(b"hi", &map, 10), 0);
    }

    // ---- findSubstring -----------------------------------------------------

    #[test]
    fn find_substring_basics() {
        let t = identity_order();
        assert_eq!(find_substring(b"lo", b"hello hello", 1, &t), 4);
        assert_eq!(find_substring(b"lo", b"hello hello", 5, &t), 10);
        assert_eq!(find_substring(b"lo", b"hello hello", 11, &t), 0);
        assert_eq!(find_substring(b"hello", b"hello", 1, &t), 1);
        assert_eq!(find_substring(b"x", b"hello", 1, &t), 0);
    }

    #[test]
    fn find_substring_edges() {
        let t = identity_order();
        // Empty key answers 0, per the C's early exit.
        assert_eq!(find_substring(b"", b"anything", 1, &t), 0);
        // Key longer than body cannot match.
        assert_eq!(find_substring(b"abc", b"ab", 1, &t), 0);
        assert_eq!(find_substring(b"a", b"", 1, &t), 0);
        // Starts below 1 clamp to the beginning.
        assert_eq!(find_substring(b"a", b"abc", 0, &t), 1);
        assert_eq!(find_substring(b"a", b"abc", -5, &t), 1);
        // Match flush at the end of the body.
        assert_eq!(find_substring(b"bc", b"abc", 1, &t), 2);
    }

    #[test]
    fn find_substring_uses_the_match_table() {
        let mut t = identity_order();
        for c in b'A'..=b'Z' {
            t[usize::from(c)] = c + 32;
        }
        assert_eq!(find_substring(b"WoRlD", b"hello world", 1, &t), 7);
    }

    // ---- indexOfAscii ------------------------------------------------------

    #[test]
    fn index_of_ascii_basics() {
        assert_eq!(index_of_ascii(isize::from(b'l'), b"hello", 1), 3);
        assert_eq!(index_of_ascii(isize::from(b'l'), b"hello", 4), 4);
        assert_eq!(index_of_ascii(isize::from(b'z'), b"hello", 1), 0);
        assert_eq!(index_of_ascii(isize::from(b'h'), b"hello", 6), 0);
        assert_eq!(index_of_ascii(isize::from(b'h'), b"", 1), 0);
    }

    #[test]
    fn index_of_ascii_out_of_byte_range_never_matches() {
        // The C compares the byte against a full sqInt; 256 + 'l' is not 'l'.
        assert_eq!(index_of_ascii(256 + isize::from(b'l'), b"hello", 1), 0);
        assert_eq!(index_of_ascii(-1, b"hello", 1), 0);
    }

    // ---- hashBytes ---------------------------------------------------------

    /// The C loop restated with explicit 64-bit arithmetic and an explicit
    /// modulus, as an independent cross-check.
    fn reference_hash(bytes: &[u8], initial: u32) -> u32 {
        let mut hash = u64::from(initial);
        for &b in bytes {
            hash = (hash + u64::from(b)) * 1_664_525 % (1 << 32);
        }
        (hash & 0x0FFF_FFFF) as u32
    }

    #[test]
    fn hash_known_answers() {
        // No bytes: the species hash comes back masked to 28 bits.
        assert_eq!(hash_bytes(b"", 17), 17);
        assert_eq!(hash_bytes(b"", u32::MAX), 0x0FFF_FFFF);
        // One byte, hand-computed: (0 + 97) * 1664525 = 161458925 < 2^28.
        assert_eq!(hash_bytes(b"a", 0), 161_458_925);
    }

    #[test]
    fn hash_matches_reference() {
        let cases: &[(&[u8], u32)] = &[
            (b"", 0),
            (b"a", 1),
            (b"abc", 487_896_546),
            (b"the quick brown fox", 0xFFFF_FFFF),
            (&[0u8, 255, 128, 7], 12345),
        ];
        for &(bytes, initial) in cases {
            assert_eq!(
                hash_bytes(bytes, initial),
                reference_hash(bytes, initial),
                "bytes {bytes:?} initial {initial}"
            );
        }
    }

    // ---- translate ---------------------------------------------------------

    #[test]
    fn translate_maps_through_the_table() {
        let mut table = identity_order();
        for c in b'a'..=b'z' {
            table[usize::from(c)] = c - 32;
        }
        let mut s = *b"Hello, World";
        translate(&mut s, &table);
        assert_eq!(&s, b"HELLO, WORLD");
        let mut empty: [u8; 0] = [];
        translate(&mut empty, &table);
    }

    // ---- convert8BitSigned -------------------------------------------------

    #[test]
    fn sample_16_matches_the_c_branches() {
        // The C: s > 0x7F ? (usqInt)(s - 256) << 8 : (usqInt)s << 8, then
        // truncated into an unsigned short.
        fn c_reference(s: u8) -> u16 {
            if s > 0x7F {
                (((i64::from(s) - 256) as u64) << 8) as u16
            } else {
                (u64::from(s) << 8) as u16
            }
        }
        for s in 0..=255u8 {
            assert_eq!(sample_16(s), c_reference(s), "sample {s}");
        }
        // Spot checks: silence, extremes.
        assert_eq!(sample_16(0), 0);
        assert_eq!(sample_16(0x7F), 0x7F00);
        assert_eq!(sample_16(0x80), 0x8000); // most negative
        assert_eq!(sample_16(0xFF), 0xFF00); // -1 -> -256
    }

    // ---- compress / decompress ---------------------------------------------

    /// Decompresses into a fresh buffer of `past_end` words, panicking on any
    /// error; roundtrip helper.
    fn decompress_to_vec(ba: &[u8], start: isize, past_end: usize) -> Vec<u32> {
        let mut out = vec![0u32; past_end];
        decompress(ba, start, past_end, &mut out).expect("valid stream");
        out
    }

    /// Bytes the size header occupies, so tests can start decoding after it
    /// the way `Bitmap>>decompress:fromByteArray:at:` does.
    fn header_len(size: usize) -> isize {
        if size <= 223 {
            1
        } else if size <= 7935 {
            2
        } else {
            5
        }
    }

    fn roundtrip(bm: &[u32]) {
        let ba = compress(bm);
        assert!(
            ba.len() <= compress_bound(bm.len()),
            "output {} exceeds the C's destination bound {} for {} words",
            ba.len(),
            compress_bound(bm.len()),
            bm.len()
        );
        let out = decompress_to_vec(&ba, header_len(bm.len()), bm.len());
        assert_eq!(out, bm, "roundtrip of {} words", bm.len());
    }

    #[test]
    fn compress_known_answers() {
        // Empty bitmap: just the size header.
        assert_eq!(compress(&[]), [0]);
        // One word, four distinct bytes: a verbatim (code 3) run of one.
        assert_eq!(compress(&[0x1234_5678]), [1, 7, 0x12, 0x34, 0x56, 0x78]);
        // One word, all bytes equal: code 1, one data byte.
        assert_eq!(compress(&[0xAAAA_AAAA]), [1, 5, 0xAA]);
        // A run of three equal-byte words: 3*4+1 = 13.
        assert_eq!(compress(&[0x0B0B_0B0B; 3]), [3, 13, 0x0B]);
        // A run of two mixed-byte words: 2*4+2 = 10, then the word.
        assert_eq!(
            compress(&[0x1234_5678; 2]),
            [2, 10, 0x12, 0x34, 0x56, 0x78]
        );
    }

    #[test]
    fn compress_run_then_verbatim() {
        // The C folds the trailing word into the verbatim run at the end of
        // the scan (`if (j + 1 == size) j += 1`).
        let bm = [0x0101_0101, 0x0101_0101, 0xDEAD_BEEF];
        assert_eq!(
            compress(&bm),
            [3, 9, 0x01, 7, 0xDE, 0xAD, 0xBE, 0xEF]
        );
        roundtrip(&bm);
    }

    #[test]
    fn encode_int_boundaries() {
        let enc = |v: usize| {
            let mut buf = [0u8; 5];
            let mut out = Out {
                buf: &mut buf,
                len: 0,
            };
            encode_int(v, &mut out);
            let len = out.len;
            buf[..len].to_vec()
        };
        assert_eq!(enc(0), [0]);
        assert_eq!(enc(223), [223]);
        assert_eq!(enc(224), [224, 224]); // 224/256 + 224, 224%256
        assert_eq!(enc(7935), [254, 255]);
        assert_eq!(enc(7936), [255, 0x00, 0x00, 0x1F, 0x00]);
        assert_eq!(enc(0xFFFF_FFFF), [255, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn roundtrip_edge_shapes() {
        roundtrip(&[]);
        roundtrip(&[0]);
        roundtrip(&[0xFFFF_FFFF]);
        roundtrip(&[1, 2, 3, 4, 5]);
        roundtrip(&[7; 100]);
        // Runs meeting the two-byte/five-byte token boundary: a token value
        // of r*4+1 crosses 7935 at r = 1984.
        roundtrip(&vec![0x3C3C_3C3C; 1982]);
        roundtrip(&vec![0x3C3C_3C3C; 1983]);
        roundtrip(&vec![0x3C3C_3C3C; 1984]);
        roundtrip(&vec![0x3C3C_3C3C; 1985]);
        // Mixed-byte word runs cross at the same length via r*4+2.
        roundtrip(&vec![0x0102_0304; 1984]);
        // Verbatim runs cross via r*4+3; distinct words, no adjacent equals.
        let distinct: Vec<u32> = (0..1990u32).map(|i| i.wrapping_mul(2654435761)).collect();
        roundtrip(&distinct);
        // Size headers at their own encoding boundaries.
        roundtrip(&vec![5u32; 223]);
        roundtrip(&vec![5u32; 224]);
    }

    #[test]
    fn roundtrip_runs_at_buffer_boundaries() {
        // A run that ends exactly at the destination boundary, runs of one
        // at both ends, and alternating shapes that stress the scan logic.
        roundtrip(&[9, 9, 9, 9, 0x0102_0304]);
        roundtrip(&[0x0102_0304, 9, 9, 9, 9]);
        roundtrip(&[1, 1, 2, 3, 3, 4, 5, 5, 5, 6]);
        roundtrip(&[0xAB, 0xAB, 0xCD, 0xCD, 0xAB, 0xAB]);
    }

    #[test]
    fn roundtrip_pseudorandom() {
        // Deterministic LCG corpus; small word range forces frequent runs.
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state
        };
        for len in [1usize, 2, 3, 17, 64, 257, 1000] {
            let noisy: Vec<u32> = (0..len).map(|_| next()).collect();
            roundtrip(&noisy);
            let runny: Vec<u32> = (0..len).map(|_| next() % 4 * 0x0101_0101).collect();
            roundtrip(&runny);
        }
    }

    #[test]
    fn decompress_known_stream() {
        // 13 = run of 3 filled from one byte; 7 = one verbatim word.
        let ba = [13u8, 0xAA, 7, 0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(
            decompress_to_vec(&ba, 0, 4),
            [0xAAAA_AAAA, 0xAAAA_AAAA, 0xAAAA_AAAA, 0xDEAD_BEEF]
        );
        // 10 = run of 2 from a big-endian word.
        let ba = [10u8, 0x01, 0x02, 0x03, 0x04];
        assert_eq!(decompress_to_vec(&ba, 0, 2), [0x0102_0304, 0x0102_0304]);
    }

    #[test]
    fn decompress_two_byte_and_five_byte_lengths() {
        // Two-byte token: a 1983-word fill uses token value 7933, encoded
        // as (7933 / 256) + 224 = 0xFE then 7933 % 256 = 0xFD.
        let ba = [0xFEu8, 0xFD, 0x55];
        let out = decompress_to_vec(&ba, 0, 1983);
        assert!(out.iter().all(|&w| w == 0x5555_5555));
        // Five-byte token: 1984-word fill, 1984*4+1 = 7937 = 0x1F01.
        let ba = [0xFFu8, 0x00, 0x00, 0x1F, 0x01, 0x55];
        let out = decompress_to_vec(&ba, 0, 1984);
        assert!(out.iter().all(|&w| w == 0x5555_5555));
    }

    #[test]
    fn decompress_empty_and_out_of_range_starts() {
        let mut out = vec![0u32; 4];
        // Nothing between start and the end: the loop never runs.
        assert_eq!(decompress(&[], 0, 4, &mut out), Ok(()));
        assert_eq!(decompress(&[13, 0xAA], 2, 4, &mut out), Ok(()));
        assert_eq!(decompress(&[13, 0xAA], 99, 4, &mut out), Ok(()));
        assert!(out.iter().all(|&w| w == 0), "nothing was written");
        // A negative start makes the first read out of bounds; the C would
        // read before the array.
        assert_eq!(
            decompress(&[13, 0xAA], -1, 4, &mut out),
            Err(DecompressError::TruncatedInput)
        );
    }

    #[test]
    fn decompress_truncated_streams_fail() {
        let mut out = vec![0u32; 4000];
        // Token 13 promises a fill byte that is not there.
        assert_eq!(
            decompress(&[13], 0, 4, &mut out),
            Err(DecompressError::TruncatedInput)
        );
        // Token 7 promises four word bytes; only two arrive.
        assert_eq!(
            decompress(&[7, 0xDE, 0xAD], 0, 4, &mut out),
            Err(DecompressError::TruncatedInput)
        );
        // A two-byte length cut after its first byte.
        assert_eq!(
            decompress(&[0xE1], 0, 4000, &mut out),
            Err(DecompressError::TruncatedInput)
        );
        // A five-byte length cut midway.
        assert_eq!(
            decompress(&[0xFF, 0x00, 0x00], 0, 4, &mut out),
            Err(DecompressError::TruncatedInput)
        );
        // Code-2 token cut inside its word.
        assert_eq!(
            decompress(&[10, 0x01, 0x02], 0, 4, &mut out),
            Err(DecompressError::TruncatedInput)
        );
    }

    #[test]
    fn decompress_overrun_fails_even_for_code_0() {
        let mut out = vec![0u32; 2];
        // 9 = fill of 2: does not fit in 1.
        assert_eq!(
            decompress(&[9, 0xAA], 0, 1, &mut out),
            Err(DecompressError::WouldOverrun)
        );
        // 8 = code 0 with n = 2: writes nothing, but the C still checks.
        assert_eq!(
            decompress(&[8], 0, 1, &mut out),
            Err(DecompressError::WouldOverrun)
        );
        assert_eq!(decompress(&[8], 0, 2, &mut out), Ok(()));
    }

    #[test]
    fn decompress_stops_at_the_destination_the_object_actually_holds() {
        // The element count the image reports is what the C checked against;
        // a destination shorter than that -- a byte object counted in bytes
        // -- is where the C wrote out of bounds.
        let mut out = vec![0u32; 1];
        assert_eq!(
            decompress(&[9, 0xAA], 0, 2, &mut out),
            Err(DecompressError::WouldOverrun)
        );
    }

    #[test]
    fn decompress_code_0_does_not_advance() {
        // After a code-0 token, the next run still lands at offset 0,
        // because the C never advances k for code 0.
        let ba = [8u8, 9, 0xAA];
        assert_eq!(
            decompress_to_vec(&ba, 0, 2),
            [0xAAAA_AAAA, 0xAAAA_AAAA]
        );
    }

    #[test]
    fn decompress_failure_keeps_earlier_writes() {
        // The C writes each run as it decodes; a later failure leaves the
        // earlier runs in the bitmap. Port keeps that.
        let mut out = vec![0u32; 3];
        let ba = [13u8, 0xAA, 9, 0xBB]; // fill 3, then fill 2 : overruns
        assert_eq!(
            decompress(&ba, 0, 3, &mut out),
            Err(DecompressError::WouldOverrun)
        );
        assert_eq!(out, [0xAAAA_AAAA, 0xAAAA_AAAA, 0xAAAA_AAAA]);
    }

    #[test]
    fn compress_bound_is_the_c_formula() {
        assert_eq!(compress_bound(0), 7);
        assert_eq!(compress_bound(1), 11);
        assert_eq!(compress_bound(0x7C0), 0x7C0 * 4 + 7 + 3);
        assert_eq!(compress_bound(0x7BF), 0x7BF * 4 + 7);
    }
}
