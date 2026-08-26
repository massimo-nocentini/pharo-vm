//! The digit-level core of the LargeIntegers plugin, as pure functions.
//!
//! Everything here mirrors the Slang-generated C in
//! `plugins/LargeIntegers/src/common/LargeIntegers.c`, one function per
//! `cDigit...` / `digit...` helper, so each can be unit-tested without a VM.
//!
//! # Representation
//!
//! A LargeInteger's magnitude is a byte-indexable object read as little-endian
//! 32-bit digits (`SQ_SWAP_4_BYTES_IF_BIGENDIAN` in the C makes big-endian
//! hosts see the same values). Two numbers describe one magnitude:
//!
//! * `byte_len` — the object's byte size (`slotSizeOf` in the C). It need not
//!   be a multiple of four.
//! * a digit slice of exactly `digit_len(byte_len)` words, the trailing
//!   partial word padded with zero bytes — exactly what the C reads, since
//!   Spur zero-fills the slack of byte objects.
//!
//! The C reads and writes whole words even over a partial trailing word,
//! relying on allocation slack; here the same values flow through fully
//! in-bounds buffers instead, which is the memory-safety half of the port.
//!
//! Most helpers take the digits as `&[u32]`, because their caller had to
//! compute them anyway. The read-only scans — [`high_bit`], [`any_bit`],
//! [`normalize_scan`], and comparison — also come in a `_bytes` spelling that
//! reads an image object in place through [`digit_at`], for the primitives
//! whose whole answer is a scan and which would otherwise copy a magnitude
//! only to look at it. Each pair shares one `#[inline]` body, so the two
//! domains cannot answer differently.

// The index-heavy loops and the `(x + 3) / 4`-style ceilings deliberately
// mirror the Slang line by line, because being diffable against the C is what
// makes the port reviewable.
#![allow(clippy::needless_range_loop, clippy::manual_div_ceil)]

/// Largest value a SmallInteger holds; mirrors `MaxSmallInteger` in the
/// generated `interp.h` (3 tag bits on 64-bit images, 1 on 32-bit).
#[cfg(target_pointer_width = "64")]
pub const MAX_SMALL: u64 = (1 << 60) - 1;
#[cfg(target_pointer_width = "32")]
pub const MAX_SMALL: u64 = (1 << 30) - 1;

/// Magnitude of the smallest SmallInteger, `0 - MinSmallInteger` in the C.
#[cfg(target_pointer_width = "64")]
pub const MIN_SMALL_MAG: u64 = 1 << 60;
#[cfg(target_pointer_width = "32")]
pub const MIN_SMALL_MAG: u64 = 1 << 30;

/// A magnitude as this module passes it around: little-endian 32-bit digits
/// plus the byte length they stand for.
pub type Magnitude = (Vec<u32>, usize);

// ---------------------------------------------------------------------------
// Delegating the asymptotically bad operations to num-bigint
// ---------------------------------------------------------------------------
//
// The C's multiplication and division are schoolbook over 32-bit digits.
// `num-bigint` packs into 64-bit limbs and, for large enough operands,
// switches to Karatsuba/Toom-3 multiplication and Burnikel-Ziegler division.
// Its digit representation is *exactly* this module's -- `BigUint::new` takes
// little-endian `u32` digits and `to_u32_digits` gives them back -- so handing
// work over costs one normalising copy each way and no reinterpretation.
//
// Two distinct effects make it faster, and they arrive at different sizes:
//
// * from ~20 digits, the 64-bit limbs alone win, because a `n/2 x n/2` limb
//   schoolbook is a quarter of the multiply-accumulates of an `n x n` digit
//   one. This is most of the benefit at the sizes image code actually meets.
// * from a few hundred digits, the sub-quadratic algorithms take over and the
//   gap widens without bound.
//
// Below the thresholds the conversion costs more than the limb packing saves,
// so the loops stay -- and they are the code already checked digit for digit
// against the C. `delegated_multiply_agrees_with_the_loop` and
// `delegated_divide_agrees_with_the_loop` keep the two from drifting.
//
// The thresholds are measured, not derived; re-measure before changing them.
// On the machine this was ported on (x86-64, release build):
//
// ```text
//   multiply, digits per factor    20    24    64   256  1024  4096
//   speedup over the loop        1.12  1.30  2.74  5.04 13.05 17.57
//
//   divide, dividend/divisor    32/16 48/24 64/32 128/64 512/256 8192/4096
//   speedup over the loop        1.24  1.64  1.92   2.93    4.42     13.49
// ```

/// Delegate a multiplication once the *shorter* factor passes this many
/// 32-bit digits. Below it the loop wins: at 8-14 digits num-bigint runs
/// 0.72-0.84x the speed of the loop, and the common image case (a
/// LargeInteger just past SmallInteger range) is 2-3 digits.
const MUL_DELEGATE_DIGITS: usize = 20;

/// Delegate a division once *both* operands pass these digit counts.
/// Division carries more per-call setup than multiplication, so it breaks
/// even later: 24/12 digits still runs at 0.89x.
const DIV_DELEGATE_DIVIDEND_DIGITS: usize = 32;
const DIV_DELEGATE_DIVISOR_DIGITS: usize = 16;

/// Borrows a digit slice as a `BigUint`. Leading zero digits are dropped by
/// the constructor, which is what every caller wants.
fn to_big(digits: &[u32]) -> num_bigint::BigUint {
    num_bigint::BigUint::new(digits.to_vec())
}

/// A `BigUint` back as exactly `len` little-endian digits, zero-padded or --
/// where the caller's contract drops a high word, as `multiply`'s does --
/// truncated.
fn from_big(value: &num_bigint::BigUint, len: usize) -> Vec<u32> {
    let mut digits = value.to_u32_digits();
    digits.resize(len, 0);
    digits
}

/// `digitSizeOfLargeInt:` — 32-bit digits covering `byte_len` bytes.
pub fn digit_len(byte_len: usize) -> usize {
    (byte_len + 3) / 4
}

/// `cHighBit32:` — 1-based index of the highest set bit, 0 for zero.
///
/// The C computes this with a shift ladder; `leading_zeros` is the same
/// function.
pub fn high_bit_32(word: u32) -> usize {
    (32 - word.leading_zeros()) as usize
}

/// `cDigitHighBit:len:` — highest set bit among the first `len` digits,
/// 1-based, 0 if they are all zero.
pub fn high_bit(digits: &[u32], len: usize) -> usize {
    high_bit_core(|ix| digits[ix], len)
}

/// [`high_bit`] over an image object's bytes, read in place.
pub fn high_bit_bytes(bytes: &[u8]) -> usize {
    high_bit_core(|ix| digit_at(bytes, ix), digit_len(bytes.len()))
}

/// The scan both spellings share. `digit` is inlined at each call site, so
/// neither pays for the indirection; having one body is what keeps the
/// digit-domain and byte-domain answers from drifting apart.
#[inline]
fn high_bit_core(digit: impl Fn(usize) -> u32, len: usize) -> usize {
    let mut real_length = len;
    loop {
        if real_length == 0 {
            return 0;
        }
        real_length -= 1;
        let last_digit = digit(real_length);
        if last_digit != 0 {
            return high_bit_32(last_digit) + 32 * real_length;
        }
    }
}

/// `cDigitCompare:with:len:` — magnitude order over `len` digits.
///
/// Answers 1, 0, -1 for `first` >, =, < `second`. Both slices must hold at
/// least `len` digits; the caller passes equal digit lengths, as the C's
/// precondition demands.
pub fn compare(first: &[u32], second: &[u32], len: usize) -> i32 {
    let mut ix = len;
    while ix > 0 {
        ix -= 1;
        let first_digit = first[ix];
        let second_digit = second[ix];
        if second_digit != first_digit {
            return if second_digit < first_digit { 1 } else { -1 };
        }
    }
    0
}

/// [`compare`] over two image objects' bytes, read in place.
///
/// `len` is the digit length both magnitudes share, as in [`compare`]; the
/// byte lengths need not match, since two byte counts in the same word round
/// to the same digit count. Bytes past an object's end read as zero, which is
/// what its zero-padded trailing word holds.
///
/// Scanning bytes rather than words costs nothing in practice: the loop runs
/// from the most significant byte down and all but the equal case leaves it
/// almost immediately, whereas materialising the digits is unconditionally
/// linear in both operands.
pub fn compare_bytes(first: &[u8], second: &[u8], len: usize) -> i32 {
    let mut ix = len * 4;
    while ix > 0 {
        ix -= 1;
        let first_byte = first.get(ix).copied().unwrap_or(0);
        let second_byte = second.get(ix).copied().unwrap_or(0);
        if second_byte != first_byte {
            return if second_byte < first_byte { 1 } else { -1 };
        }
    }
    0
}

/// `byteSizeOfCSI:` — bytes needed for a SmallInteger's magnitude, at least 1.
///
/// The C spells this as a threshold ladder (`< 256`, `< 65536`, ...) for both
/// signs; the thresholds are symmetric, so it is exactly the magnitude's byte
/// count. A SmallInteger magnitude never exceeds the oop size, so the C's
/// `BytesPerOop` cap is unreachable.
pub fn small_byte_size(value: isize) -> usize {
    let magnitude = (value as i64).unsigned_abs();
    if magnitude == 0 {
        1
    } else {
        (64 - magnitude.leading_zeros() as usize + 7) / 8
    }
}

/// The digit part of `createLargeFromSmallInteger:` — the magnitude as
/// exactly `digit_len(small_byte_size(value))` little-endian digits.
pub fn small_digits(value: isize) -> Vec<u32> {
    let magnitude = (value as i64).unsigned_abs();
    let count = digit_len(small_byte_size(value));
    (0..count)
        .map(|ix| (magnitude >> (ix * 32)) as u32)
        .collect()
}

/// `cDigitAdd:len:with:len:into:` — magnitude addition.
///
/// `short` must not be longer than `long`. Answers the digit sum sized like
/// `long`, plus the final carry (`over` in the C, 0 or 1).
pub fn add(short: &[u32], long: &[u32]) -> (Vec<u32>, u32) {
    debug_assert!(short.len() <= long.len());
    let mut res = vec![0u32; long.len()];
    let mut accum: u64 = 0;
    for i in 0..short.len() {
        accum = (accum >> 32) + short[i] as u64 + long[i] as u64;
        res[i] = accum as u32;
    }
    for i in short.len()..long.len() {
        accum = (accum >> 32) + long[i] as u64;
        res[i] = accum as u32;
    }
    (res, (accum >> 32) as u32)
}

/// `digitSubLarge:with:` minus the object plumbing — magnitude subtraction
/// with the C's larger/smaller decision.
///
/// Answers the digit difference and whether the result is negative, given the
/// sign of the first operand. When the digit lengths are equal, common leading
/// digits are trimmed before comparing, exactly as the C does; the larger
/// magnitude is decided *by top digit only*, so unnormalized garbage in gives
/// the same mod-2³² ⁿ garbage out as the C.
pub fn subtract(first: &[u32], second: &[u32], first_negative: bool) -> (Vec<u32>, bool) {
    let mut first_len = first.len();
    let mut second_len = second.len();
    if first_len == second_len {
        while first_len > 1 && first[first_len - 1] == second[first_len - 1] {
            first_len -= 1;
        }
        second_len = first_len;
    }
    // The C reads the word below the object for a zero digit length;
    // substituting zero keeps the decision deterministic and in bounds.
    let top = |digits: &[u32], len: usize| if len == 0 { 0 } else { digits[len - 1] };
    let first_smaller = first_len < second_len
        || (first_len == second_len && top(first, first_len) < top(second, second_len));
    let (larger, larger_len, smaller, smaller_len, neg) = if first_smaller {
        (second, second_len, first, first_len, !first_negative)
    } else {
        (first, first_len, second, second_len, first_negative)
    };
    // cDigitSub:len:with:len:into: -- z is the borrow, kept as the C keeps it:
    // 0 or (as u64) -1, folded into the next step's sum.
    let mut res = vec![0u32; larger_len];
    let mut z: u64 = 0;
    for i in 0..smaller_len {
        z = z
            .wrapping_add(larger[i] as u64)
            .wrapping_sub(smaller[i] as u64);
        res[i] = z as u32;
        z = 0u64.wrapping_sub(z >> 63);
    }
    for i in smaller_len..larger_len {
        z = z.wrapping_add(larger[i] as u64);
        res[i] = z as u32;
        z = 0u64.wrapping_sub(z >> 63);
    }
    (res, neg)
}

/// `cDigitMultiply:len:with:len:into:len:` — schoolbook magnitude product.
///
/// Lengths are in *bytes*, as in the C: the product is sized
/// `digit_len(short_bytes + long_bytes)`, which can be one digit fewer than
/// the sum of the operand digit counts, hence the guarded final carry store.
pub fn multiply(short: &[u32], short_bytes: usize, long: &[u32], long_bytes: usize) -> Vec<u32> {
    debug_assert_eq!(short.len(), digit_len(short_bytes));
    debug_assert_eq!(long.len(), digit_len(long_bytes));
    let capacity = digit_len(short_bytes + long_bytes);
    if short.len().min(long.len()) > MUL_DELEGATE_DIGITS {
        multiply_delegated(short, long, capacity)
    } else {
        multiply_schoolbook(short, long, capacity)
    }
}

/// [`multiply`] through num-bigint, for factors large enough that Karatsuba
/// or Toom-3 beats the schoolbook loop.
///
/// `capacity` is one short of `short.len() + long.len()` whenever both byte
/// lengths have partial trailing words, so the resize inside [`from_big`]
/// drops the same high carry digit [`multiply_schoolbook`] drops with its
/// `if k < capacity`. It can never drop more: the true product needs at most
/// `short.len() + long.len()` digits, and `capacity` is always at least
/// `short.len() + long.len() - 1`.
fn multiply_delegated(short: &[u32], long: &[u32], capacity: usize) -> Vec<u32> {
    from_big(&(to_big(short) * to_big(long)), capacity)
}

/// [`multiply`]'s schoolbook loop, digit for digit as the C runs it.
fn multiply_schoolbook(short: &[u32], long: &[u32], capacity: usize) -> Vec<u32> {
    let mut res = vec![0u32; capacity];
    if short.len() == 1 && short[0] == 0 {
        return res;
    }
    if long.len() == 1 && long[0] == 0 {
        return res;
    }
    for i in 0..short.len() {
        let digit = short[i] as u64;
        if digit != 0 {
            let mut k = i;
            let mut carry: u64 = 0;
            for j in 0..long.len() {
                // At most (2^32-1)^2 + 2*(2^32-1) = 2^64 - 1: no overflow.
                let ab = (long[j] as u64) * digit + carry + res[k] as u64;
                carry = ab >> 32;
                res[k] = ab as u32;
                k += 1;
            }
            if k < capacity {
                res[k] = carry as u32;
            }
        }
    }
    res
}

/// The three ops of `cDigitOp:short:len:long:len:into:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOp {
    And,
    Or,
    Xor,
}

/// `cDigitOp:short:len:long:len:into:` — digitwise logic on magnitudes.
///
/// `short` must not be longer than `long`. The result is sized like `long`;
/// for And the high digits are zero, for Or/Xor they are copied from `long`.
/// The C notes these are endian-neutral and works on raw words; on the
/// value-level digits used here that is the same computation.
pub fn bit_op(op: BitOp, short: &[u32], long: &[u32]) -> Vec<u32> {
    debug_assert!(short.len() <= long.len());
    let mut res = vec![0u32; long.len()];
    for i in 0..short.len() {
        res[i] = match op {
            BitOp::And => short[i] & long[i],
            BitOp::Or => short[i] | long[i],
            BitOp::Xor => short[i] ^ long[i],
        };
    }
    if op != BitOp::And {
        res[short.len()..].copy_from_slice(&long[short.len()..]);
    }
    res
}

/// `digit:Lshift:` minus the object plumbing.
///
/// Answers the shifted digits and the result's byte length,
/// `(highBit + shift + 7) / 8`, or `None` when the magnitude is zero — the C
/// answers a fresh 1-byte zero LargeInteger in that case, which needs the VM.
pub fn lshift(digits: &[u32], shift: usize) -> Option<(Vec<u32>, usize)> {
    let old_digit_len = digits.len();
    let hb = high_bit(digits, old_digit_len);
    if hb == 0 {
        return None;
    }
    let new_byte_len = (hb + shift + 7) / 8;
    let new_digit_len = digit_len(new_byte_len);
    let digit_shift = shift / 32;
    let bit_shift = shift % 32;
    // The C writes whole words past newDigitLen into allocation slack when
    // the input has leading zero digits; those writes are always zero, so a
    // scratch tail truncated afterwards reproduces them safely.
    let mut out = vec![0u32; new_digit_len.max(old_digit_len + digit_shift + 1)];
    if bit_shift == 0 {
        // Fast version for digit-aligned shifts (cDigitCopyFrom:to:len:).
        out[digit_shift..new_digit_len].copy_from_slice(&digits[..new_digit_len - digit_shift]);
    } else {
        let rshift = 32 - bit_shift;
        let mut carry: u32 = 0;
        for i in 0..old_digit_len {
            let digit = digits[i];
            out[i + digit_shift] = carry | (digit << bit_shift);
            carry = digit >> rshift;
        }
        if carry != 0 {
            out[new_digit_len - 1] = carry;
        }
    }
    debug_assert!(out[new_digit_len..].iter().all(|&w| w == 0));
    out.truncate(new_digit_len);
    Some((out, new_byte_len))
}

/// `digit:Rshift:lookfirst:` minus the object plumbing.
///
/// Considers only the first `look_first` digits (the rest are discarded, as
/// the division's remainder path requires), shifts right and answers the
/// digits with their byte length `(newBitLen + 7) / 8`. `None` means all bits
/// were lost — the C answers a 0-length LargeInteger then.
pub fn rshift(digits: &[u32], shift: usize, look_first: usize) -> Option<(Vec<u32>, usize)> {
    let old_bit_len = high_bit(digits, look_first);
    let new_bit_len = old_bit_len as isize - shift as isize;
    if new_bit_len <= 0 {
        return None;
    }
    let old_digit_len = (old_bit_len + 31) / 32;
    let new_byte_len = (new_bit_len as usize + 7) / 8;
    let new_digit_len = digit_len(new_byte_len);
    let digit_shift = shift / 32;
    let bit_shift = shift % 32;
    let mut out = vec![0u32; new_digit_len];
    if bit_shift == 0 {
        // Fast version for digit-aligned shifts (cDigitReplace...startingAt:).
        out.copy_from_slice(&digits[digit_shift..digit_shift + new_digit_len]);
    } else {
        let left_shift = 32 - bit_shift;
        let mut carry = digits[digit_shift] >> bit_shift;
        let start = digit_shift + 1;
        for j in start..old_digit_len {
            let digit = digits[j];
            out[j - start] = carry | (digit << left_shift);
            carry = digit >> bit_shift;
        }
        if carry != 0 {
            out[new_digit_len - 1] = carry;
        }
    }
    Some((out, new_byte_len))
}

/// `anyBitOfLargeInt:from:to:` — any magnitude bit set in `start..=stop_arg`?
///
/// Bit positions are 1-based. The caller has already rejected positions below
/// 1; `stop_arg` is clamped to the magnitude's high bit.
pub fn any_bit(digits: &[u32], start: usize, stop_arg: usize) -> bool {
    any_bit_core(
        |ix| digits[ix],
        high_bit(digits, digits.len()),
        start,
        stop_arg,
    )
}

/// [`any_bit`] over an image object's bytes, read in place.
pub fn any_bit_bytes(bytes: &[u8], start: usize, stop_arg: usize) -> bool {
    any_bit_core(
        |ix| digit_at(bytes, ix),
        high_bit_bytes(bytes),
        start,
        stop_arg,
    )
}

/// The mask arithmetic both spellings share; `high` is the magnitude's high
/// bit, which the C computes first to clamp `stop_arg`.
#[inline]
fn any_bit_core(digit: impl Fn(usize) -> u32, high: usize, start: usize, stop_arg: usize) -> bool {
    let stop = stop_arg.min(high);
    if start > stop {
        return false;
    }
    let first_digit_ix = (start - 1) / 32; // C's 1-based index, made 0-based
    let last_digit_ix = (stop - 1) / 32;
    let first_mask = 0xFFFF_FFFFu32 << ((start - 1) & 0x1F);
    let last_mask = 0xFFFF_FFFFu32 >> (0x1F - ((stop - 1) & 0x1F));
    if first_digit_ix == last_digit_ix {
        return digit(first_digit_ix) & (first_mask & last_mask) != 0;
    }
    if digit(first_digit_ix) & first_mask != 0 {
        return true;
    }
    for ix in first_digit_ix + 1..last_digit_ix {
        if digit(ix) != 0 {
            return true;
        }
    }
    digit(last_digit_ix) & last_mask != 0
}

/// `cDigitDiv:len:rem:len:quo:len:` — Knuth division, digit for digit.
///
/// `div` is the divisor shifted so its top significant digit has bit 32 set,
/// grown by one zero digit; `rem` starts as the equally shifted dividend and
/// finishes as the (still shifted) remainder. Answers the quotient digits.
///
/// All arithmetic wraps exactly as the C's `unsigned long long` does —
/// including `q * dnh`, which can genuinely exceed 64 bits mid-estimate.
pub fn div_core(div: &[u32], rem: &mut [u32], quo_len: usize) -> Vec<u32> {
    let div_len = div.len();
    let rem_len = rem.len();
    let mut quo = vec![0u32; quo_len];
    let dl = div_len - 1; // last digit of actual divisor data
    let dh = div[dl - 1] as u64;
    let dnh = if dl == 1 { 0u64 } else { div[dl - 2] as u64 };
    for k in 1..=quo_len {
        // Estimate rem/div by dividing the leading two digits of rem by dh.
        let j = rem_len + 1 - k;
        let mut q: u64;
        if rem[j - 1] as u64 == dh {
            q = 0xFFFF_FFFF;
        } else {
            let r1r2 = ((rem[j - 1] as u64) << 32) + rem[j - 2] as u64;
            let t = r1r2 % dh;
            q = r1r2 / dh;
            let mul = q.wrapping_mul(dnh);
            let mut hi = mul >> 32;
            let mut lo = mul & 0xFFFF_FFFF;
            let r3 = if j < 3 { 0u64 } else { rem[j - 3] as u64 };
            // Correct the overestimate; at most 2 iterations (Knuth vol. 2).
            loop {
                let cond = if t < hi || (t == hi && r3 < lo) {
                    // i.e. (t,r3) < (hi,lo)
                    q = q.wrapping_sub(1);
                    if hi == 0 {
                        false
                    } else {
                        if lo < dnh {
                            hi -= 1;
                            lo = lo + 0x1_0000_0000 - dnh;
                        } else {
                            lo -= dnh;
                        }
                        hi >= dh
                    }
                } else {
                    false
                };
                if !cond {
                    break;
                }
                hi -= dh;
            }
        }
        // Multiply div by q and subtract from rem, maintaining
        // quo*div + rem = dividend. The C walks l alongside i; here l is
        // simply j - dl + i.
        let mut a: u64 = 0;
        for i in 0..div_len {
            let l = j - dl + i;
            let hi = (div[i] as u64).wrapping_mul(q >> 32);
            let lo = (div[i] as u64).wrapping_mul(q & 0xFFFF_FFFF);
            let b = (rem[l - 1] as u64)
                .wrapping_sub(a)
                .wrapping_sub(lo & 0xFFFF_FFFF);
            rem[l - 1] = b as u32;
            // Arithmetic >> 32 emulated on an unsigned value, as in the C.
            let b = (b >> 32) | (0u64.wrapping_sub(b >> 63) & 0xFFFF_FFFF_0000_0000);
            a = hi.wrapping_add(lo >> 32).wrapping_sub(b);
        }
        if a > 0 {
            // q was still one too large: add div back into rem.
            q = q.wrapping_sub(1);
            let mut a: u64 = 0;
            for i in 0..div_len {
                let l = j - dl + i;
                a = (a >> 32)
                    .wrapping_add(rem[l - 1] as u64)
                    .wrapping_add(div[i] as u64);
                rem[l - 1] = a as u32;
            }
        }
        quo[quo_len - k] = q as u32;
    }
    quo
}

/// `digitDivLarge:with:negative:`'s arithmetic, from normalization shift to
/// remainder unshift.
///
/// Precondition (checked by the caller): both operands are normalized and
/// non-zero-divisor, and `digit_len(first) >= digit_len(second)` so the
/// quotient has at least one digit.
///
/// Answers `(quotient digits, quotient byte length, remainder)`; the
/// remainder is `None` when it is zero — the C answers a 0-length
/// LargeInteger of the dividend's class then. Both results are unnormalized,
/// exactly as the primitive hands them to the image.
pub fn divide(
    first: &[u32],
    first_byte_len: usize,
    second: &[u32],
    second_byte_len: usize,
) -> (Vec<u32>, usize, Option<Magnitude>) {
    let first_digit_len = digit_len(first_byte_len);
    let second_digit_len = digit_len(second_byte_len);
    debug_assert!(first_digit_len >= second_digit_len);
    let quo_digit_len = first_digit_len - second_digit_len + 1;
    if first_digit_len > DIV_DELEGATE_DIVIDEND_DIGITS
        && second_digit_len > DIV_DELEGATE_DIVISOR_DIGITS
    {
        divide_delegated(
            &first[..first_digit_len],
            &second[..second_digit_len],
            quo_digit_len,
        )
    } else {
        divide_schoolbook(
            &first[..first_digit_len],
            &second[..second_digit_len],
            quo_digit_len,
        )
    }
}

/// [`divide`]'s shift-and-Knuth-D path, as the C runs it.
fn divide_schoolbook(
    first: &[u32],
    second: &[u32],
    quo_digit_len: usize,
) -> (Vec<u32>, usize, Option<Magnitude>) {
    let first_digit_len = first.len();
    let second_digit_len = second.len();
    let d = 32 - high_bit_32(second[second_digit_len - 1]);
    // div := (second << d) grown by one zero digit (largeIntgrowTo in the C).
    let (mut div, _) = lshift(&second[..second_digit_len], d).expect("divisor is non-zero");
    div.push(0);
    // rem := first << d; a zero dividend shifts to the C's 1-byte zero.
    let mut rem = match lshift(&first[..first_digit_len], d) {
        Some((digits, _)) => digits,
        None => vec![0u32],
    };
    if rem.len() == first_digit_len {
        rem.push(0);
    }
    let quo = div_core(&div, &mut rem, quo_digit_len);
    let rem_out = rshift(&rem, d, div.len() - 1);
    (quo, quo_digit_len * 4, rem_out)
}

/// [`divide`] for operands big enough that Burnikel-Ziegler beats Knuth D.
///
/// The normalisation shift the loop version needs (`d`, so the quotient-digit
/// estimate is exact) is num-bigint's own business, so it is absent here. The
/// output shapes are the ones `divide` documents, and they fall out of the
/// same rules the shifted path arrives at:
///
/// * the quotient always occupies `quo_digit_len` digits, zero-padded;
/// * the remainder carries the *minimal* byte length holding it, which is
///   what `rshift` computes as `(newBitLen + 7) / 8` -- and since it right
///   shifts by exactly the `d` bits the dividend was left shifted by,
///   `newBitLen` is the true remainder's bit length, i.e. `BigUint::bits`;
/// * a zero remainder is `None`, which `rshift` reports as `newBitLen <= 0`.
fn divide_delegated(
    first: &[u32],
    second: &[u32],
    quo_digit_len: usize,
) -> (Vec<u32>, usize, Option<Magnitude>) {
    use num_integer::Integer;

    let (quotient, remainder) = to_big(first).div_rem(&to_big(second));
    let rem_bits = remainder.bits() as usize;
    let rem_out = if rem_bits == 0 {
        None
    } else {
        let byte_len = (rem_bits + 7) / 8;
        Some((from_big(&remainder, digit_len(byte_len)), byte_len))
    };
    (
        from_big(&quotient, quo_digit_len),
        quo_digit_len * 4,
        rem_out,
    )
}

/// `cDigitMontgomery:len:times:len:modulo:len:mInvModB:into:` — Montgomery
/// multiplication: `first * second * (2^32)^-len(third) mod third`.
///
/// The caller has checked `first` and `second` are no longer than `third`.
/// Answers `None` for the one shape whose C reads out of bounds: a non-empty
/// `first` against an empty `second`.
pub fn montgomery(first: &[u32], second: &[u32], third: &[u32], m_inv: u32) -> Option<Vec<u32>> {
    const M: u64 = 0xFFFF_FFFF;
    let first_len = first.len();
    let second_len = second.len();
    let third_len = third.len();
    debug_assert!(first_len <= third_len && second_len <= third_len);
    if first_len > 0 && second_len == 0 {
        // The C dereferences pSecond[0] here, one word past an empty object.
        return None;
    }
    let mut res = vec![0u32; third_len];
    let m_inv = m_inv as u64;
    let mut last_digit: u32 = 0;
    for i in 0..first_len {
        let accum3 = (first[i] as u64)
            .wrapping_mul(second[0] as u64)
            .wrapping_add(res[0] as u64);
        let u = accum3.wrapping_mul(m_inv) & M;
        let accum2 = u.wrapping_mul(third[0] as u64);
        let mut accum = (accum2 & M) + (accum3 & M);
        accum = (accum >> 32) + (accum2 >> 32) + (accum3 >> 32);
        for k in 1..second_len {
            let accum3 = (first[i] as u64)
                .wrapping_mul(second[k] as u64)
                .wrapping_add(res[k] as u64);
            let accum2 = u.wrapping_mul(third[k] as u64);
            accum = accum.wrapping_add(accum2 & M).wrapping_add(accum3 & M);
            res[k - 1] = accum as u32;
            accum = (accum >> 32) + (accum2 >> 32) + (accum3 >> 32);
        }
        for k in second_len..third_len {
            let accum2 = u.wrapping_mul(third[k] as u64);
            accum = accum.wrapping_add(res[k] as u64).wrapping_add(accum2 & M);
            res[k - 1] = accum as u32;
            accum = (accum >> 32) + (accum2 >> 32);
        }
        accum = accum.wrapping_add(last_digit as u64);
        res[third_len - 1] = accum as u32;
        last_digit = (accum >> 32) as u32;
    }
    for _i in first_len..third_len {
        let mut accum = res[0] as u64;
        let u = accum.wrapping_mul(m_inv) & M;
        accum = accum.wrapping_add(u.wrapping_mul(third[0] as u64));
        accum >>= 32;
        for k in 1..third_len {
            let accum2 = u.wrapping_mul(third[k] as u64);
            accum = accum.wrapping_add(res[k] as u64).wrapping_add(accum2 & M);
            res[k - 1] = accum as u32;
            accum = (accum >> 32) + (accum2 >> 32);
        }
        accum = accum.wrapping_add(last_digit as u64);
        res[third_len - 1] = accum as u32;
        last_digit = (accum >> 32) as u32;
    }
    if !(last_digit == 0 && compare(third, &res, third_len) == 1) {
        // res >= third (or an overflow digit is pending): subtract third once.
        let mut accum: u64 = 0;
        for i in 0..third_len {
            accum = accum
                .wrapping_add(res[i] as u64)
                .wrapping_sub(third[i] as u64);
            res[i] = accum as u32;
            accum = 0u64.wrapping_sub(accum >> 63);
        }
    }
    Some(res)
}

/// What `normalizePositive:` / `normalizeNegative:` decide about a magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalized {
    /// The value fits a SmallInteger; here it is, signed.
    Small(isize),
    /// It stays a LargeInteger of this many bytes (possibly fewer than it
    /// occupies now, never more).
    Large(usize),
}

/// The decision logic of `normalizePositive:` / `normalizeNegative:`.
///
/// Strips leading zero digits, reduces to a SmallInteger when the value fits,
/// otherwise trims the byte length to the top digit's significant bytes.
pub fn normalize_scan(digits: &[u32], byte_len: usize, negative: bool) -> Normalized {
    debug_assert!(digit_len(byte_len) <= digits.len());
    normalize_scan_core(|ix| digits[ix], byte_len, negative)
}

/// [`normalize_scan`] over an image object's bytes, read in place — the shape
/// `primNormalizePositive` / `primNormalizeNegative` need, where the operand
/// is an object and nothing has been computed into a buffer.
pub fn normalize_scan_bytes(bytes: &[u8], negative: bool) -> Normalized {
    normalize_scan_core(|ix| digit_at(bytes, ix), bytes.len(), negative)
}

/// The decision both spellings share.
#[inline]
fn normalize_scan_core(
    digit: impl Fn(usize) -> u32,
    byte_len: usize,
    negative: bool,
) -> Normalized {
    let mut digit_count = digit_len(byte_len);
    while digit_count != 0 && digit(digit_count - 1) == 0 {
        digit_count -= 1;
    }
    if digit_count == 0 {
        return Normalized::Small(0);
    }
    let mut val = digit(digit_count - 1) as u64;
    // "SmallInteger maxVal digitLength": 2 digits on 64-bit images, 1 on 32.
    let s_len: usize = if MIN_SMALL_MAG > 0x4000_0000 { 2 } else { 1 };
    if digit_count <= s_len {
        let mut val2 = val;
        if digit_count > 1 {
            val2 = (val2 << 32) + digit(0) as u64;
        }
        if negative {
            if val2 <= MIN_SMALL_MAG {
                return Normalized::Small((val2 as i64).wrapping_neg() as isize);
            }
        } else if val2 <= MAX_SMALL {
            return Normalized::Small(val2 as isize);
        }
    }
    let mut new_byte_len = digit_count * 4;
    if val <= 0xFFFF {
        new_byte_len -= 2;
    } else {
        val >>= 16;
    }
    if val <= 0xFF {
        new_byte_len -= 1;
    }
    Normalized::Large(new_byte_len)
}

/// `isNormalized:` — a non-empty magnitude whose top byte is non-zero.
///
/// Only ever asked of an image object, whose byte length *is* the slice
/// length, so this needs no digits at all — which is why there is no
/// digit-domain spelling to keep in step.
pub fn is_normalized_bytes(bytes: &[u8]) -> bool {
    matches!(bytes.last(), Some(&top) if top != 0)
}

/// Reads a byte object's contents as little-endian digits, the trailing
/// partial word zero-padded — the values `cDigitOf:at:` sees.
pub fn bytes_to_digits(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(digit_len(bytes.len()));
    let mut words = bytes.chunks_exact(4);
    for word in &mut words {
        out.push(u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    }
    let tail = words.remainder();
    if !tail.is_empty() {
        let mut padded = [0u8; 4];
        padded[..tail.len()].copy_from_slice(tail);
        out.push(u32::from_le_bytes(padded));
    }
    out
}

/// `cDigitOf:at:` — digit `ix` of a byte object's magnitude, read in place.
///
/// Bytes past the object's end read as zero: that is the zero-padded trailing
/// word the C sees in allocation slack, produced here without ever leaving
/// the slice.
#[inline]
pub fn digit_at(bytes: &[u8], ix: usize) -> u32 {
    let start = ix * 4;
    if let Some(word) = bytes.get(start..start + 4) {
        return u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut padded = [0u8; 4];
    if let Some(tail) = bytes.get(start..) {
        padded[..tail.len()].copy_from_slice(tail);
    }
    u32::from_le_bytes(padded)
}

/// Runs `f` on the digits' little-endian byte image, `byte_len` bytes long.
///
/// On a little-endian host a `[u32]` in memory already *is* that byte
/// sequence, so the callback sees the buffer itself and nothing is copied —
/// `write_bytes` then memcpys straight from the computed digits into the new
/// object. On a big-endian host the bytes genuinely differ, so they are built
/// the long way and the callback sees that.
///
/// `byte_len` must not exceed the digits' own byte length, and any digit
/// bytes past it must be zero — the same contract as [`digits_to_bytes`],
/// checked the same way.
pub fn with_bytes<R>(digits: &[u32], byte_len: usize, f: impl FnOnce(&[u8]) -> R) -> R {
    debug_assert!(byte_len <= digits.len() * 4);
    debug_assert!(
        (byte_len..digits.len() * 4).all(|i| (digits[i / 4] >> ((i % 4) * 8)) as u8 == 0),
        "digit bytes beyond the byte length must be zero"
    );
    #[cfg(target_endian = "little")]
    {
        // SAFETY: `[u32]` is `4 * len` initialised bytes with no padding, so
        // reinterpreting it as bytes reads only initialised memory the slice
        // already owns; `u8` needs no alignment, and `byte_len` is in range
        // by the assertion above. The borrow lives only for the call.
        let all =
            unsafe { core::slice::from_raw_parts(digits.as_ptr().cast::<u8>(), digits.len() * 4) };
        f(&all[..byte_len])
    }
    #[cfg(target_endian = "big")]
    {
        f(&digits_to_bytes(digits, byte_len))
    }
}

/// Serializes digits back to `byte_len` bytes, little-endian.
///
/// Any digit bytes beyond `byte_len` must be zero — the C writes them into
/// allocation slack, which only ever receives zeros.
///
/// This is [`with_bytes`]'s big-endian path, and the tests' independent
/// spelling of the same conversion; on a little-endian host nothing in the
/// plugin proper calls it, because there the digits are already their own
/// byte image.
#[cfg_attr(target_endian = "little", allow(dead_code))]
pub fn digits_to_bytes(digits: &[u32], byte_len: usize) -> Vec<u8> {
    debug_assert!(byte_len <= digits.len() * 4);
    let mut out = vec![0u8; byte_len];
    for (i, b) in out.iter_mut().enumerate() {
        *b = (digits[i / 4] >> ((i % 4) * 8)) as u8;
    }
    debug_assert!(
        (byte_len..digits.len() * 4).all(|i| (digits[i / 4] >> ((i % 4) * 8)) as u8 == 0),
        "digit bytes beyond the byte length must be zero"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers -----------------------------------------------------------

    /// xorshift64*: deterministic, dependency-free randomness for the tests.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn to_u128(digits: &[u32]) -> u128 {
        assert!(digits.len() <= 4);
        digits
            .iter()
            .rev()
            .fold(0u128, |acc, &w| (acc << 32) | w as u128)
    }

    /// Minimal byte length of a value, at least 1 (the plugin's convention).
    fn min_byte_len(v: u128) -> usize {
        if v == 0 {
            1
        } else {
            (128 - v.leading_zeros() as usize + 7) / 8
        }
    }

    /// A value as the (digits, byte_len) pair the plugin passes around.
    fn digits_for(v: u128) -> (Vec<u32>, usize) {
        let byte_len = min_byte_len(v);
        let digits = (0..digit_len(byte_len))
            .map(|i| (v >> (i * 32)) as u32)
            .collect();
        (digits, byte_len)
    }

    /// Independent schoolbook reference: base-256 little-endian addition.
    fn ref_add(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; a.len().max(b.len()) + 1];
        let mut carry = 0u16;
        for i in 0..out.len() {
            let s = carry + *a.get(i).unwrap_or(&0) as u16 + *b.get(i).unwrap_or(&0) as u16;
            out[i] = s as u8;
            carry = s >> 8;
        }
        out
    }

    /// Independent schoolbook reference: base-256 multiplication.
    fn ref_mul(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; a.len() + b.len()];
        for (i, &x) in a.iter().enumerate() {
            let mut carry = 0u32;
            for (j, &y) in b.iter().enumerate() {
                let t = out[i + j] as u32 + x as u32 * y as u32 + carry;
                out[i + j] = t as u8;
                carry = t >> 8;
            }
            let mut k = i + b.len();
            while carry != 0 {
                let t = out[k] as u32 + carry;
                out[k] = t as u8;
                carry = t >> 8;
                k += 1;
            }
        }
        out
    }

    fn trim(bytes: &[u8]) -> &[u8] {
        let mut n = bytes.len();
        while n > 0 && bytes[n - 1] == 0 {
            n -= 1;
        }
        &bytes[..n]
    }

    // ---- high bits and comparison ------------------------------------------

    #[test]
    fn high_bit_32_matches_definition() {
        assert_eq!(high_bit_32(0), 0);
        assert_eq!(high_bit_32(1), 1);
        assert_eq!(high_bit_32(2), 2);
        assert_eq!(high_bit_32(3), 2);
        assert_eq!(high_bit_32(0x7FFF_FFFF), 31);
        assert_eq!(high_bit_32(0x8000_0000), 32);
        assert_eq!(high_bit_32(0xFFFF_FFFF), 32);
    }

    #[test]
    fn high_bit_scans_from_the_top() {
        assert_eq!(high_bit(&[], 0), 0);
        assert_eq!(high_bit(&[0], 1), 0);
        assert_eq!(high_bit(&[1], 1), 1);
        assert_eq!(high_bit(&[0, 1], 2), 33);
        assert_eq!(high_bit(&[5, 0], 2), 3);
        assert_eq!(high_bit(&[0xFFFF_FFFF, 0, 0], 3), 32);
        // The len parameter crops what is looked at.
        assert_eq!(high_bit(&[5, 7], 1), 3);
    }

    #[test]
    fn compare_orders_by_magnitude() {
        assert_eq!(compare(&[1], &[1], 1), 0);
        assert_eq!(compare(&[2], &[1], 1), 1);
        assert_eq!(compare(&[1], &[2], 1), -1);
        assert_eq!(compare(&[9, 1], &[0, 2], 2), -1);
        assert_eq!(compare(&[0, 2], &[9, 1], 2), 1);
        assert_eq!(compare(&[7, 3], &[7, 3], 2), 0);
        // Low digits only decide when the high ones tie.
        assert_eq!(compare(&[8, 3], &[7, 3], 2), 1);
    }

    // ---- SmallInteger conversion -------------------------------------------

    #[test]
    fn small_byte_size_thresholds_match_the_c_ladder() {
        assert_eq!(small_byte_size(0), 1);
        assert_eq!(small_byte_size(1), 1);
        assert_eq!(small_byte_size(255), 1);
        assert_eq!(small_byte_size(256), 2);
        assert_eq!(small_byte_size(65535), 2);
        assert_eq!(small_byte_size(65536), 3);
        assert_eq!(small_byte_size(0xFF_FFFF), 3);
        assert_eq!(small_byte_size(0x100_0000), 4);
        assert_eq!(small_byte_size(-1), 1);
        assert_eq!(small_byte_size(-255), 1);
        assert_eq!(small_byte_size(-256), 2);
        assert_eq!(small_byte_size(-65535), 2);
        assert_eq!(small_byte_size(-65536), 3);
        assert_eq!(small_byte_size(-0xFF_FFFF), 3);
        assert_eq!(small_byte_size(-0x100_0000), 4);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn small_byte_size_wide_thresholds() {
        assert_eq!(small_byte_size(0xFFFF_FFFF), 4);
        assert_eq!(small_byte_size(0x1_0000_0000), 5);
        assert_eq!(small_byte_size((1 << 40) - 1), 5);
        assert_eq!(small_byte_size(1 << 40), 6);
        assert_eq!(small_byte_size(1 << 48), 7);
        assert_eq!(small_byte_size(1 << 56), 8);
        assert_eq!(small_byte_size((1 << 60) - 1), 8);
        assert_eq!(small_byte_size(-(1 << 60)), 8);
        assert_eq!(small_byte_size(-0xFFFF_FFFF), 4);
        assert_eq!(small_byte_size(-0x1_0000_0000), 5);
    }

    #[test]
    fn small_digits_hold_the_magnitude() {
        for &v in &[0isize, 1, 255, 256, 65536, -1, -256, -65537, 0x100_0000] {
            let d = small_digits(v);
            assert_eq!(d.len(), digit_len(small_byte_size(v)));
            assert_eq!(to_u128(&d), (v as i64).unsigned_abs() as u128);
        }
        #[cfg(target_pointer_width = "64")]
        {
            let d = small_digits(-(1isize << 60));
            assert_eq!(d.len(), 2);
            assert_eq!(to_u128(&d), 1 << 60);
        }
    }

    // ---- addition ----------------------------------------------------------

    #[test]
    fn add_matches_u128() {
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for _ in 0..500 {
            let a = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let b = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let (da, _) = digits_for(a);
            let (db, _) = digits_for(b);
            let (short, long) = if da.len() <= db.len() {
                (&da, &db)
            } else {
                (&db, &da)
            };
            let (sum, over) = add(short, long);
            let total = to_u128(&sum) + ((over as u128) << (32 * long.len()));
            assert_eq!(total, a + b);
        }
    }

    #[test]
    fn add_carries_across_all_digits() {
        let (sum, over) = add(&[1], &[0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF]);
        assert_eq!(sum, vec![0, 0, 0]);
        assert_eq!(over, 1);
        let (sum, over) = add(&[0xFFFF_FFFF], &[0xFFFF_FFFF]);
        assert_eq!(sum, vec![0xFFFF_FFFE]);
        assert_eq!(over, 1);
    }

    #[test]
    fn add_of_empty_magnitudes_is_zero() {
        let (sum, over) = add(&[], &[]);
        assert!(sum.is_empty());
        assert_eq!(over, 0);
    }

    // ---- subtraction -------------------------------------------------------

    #[test]
    fn subtract_matches_i128_with_both_signs() {
        let mut rng = Rng(0xFEED_FACE_CAFE_BEEF);
        for _ in 0..500 {
            let a = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let b = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            for &first_neg in &[false, true] {
                let (da, _) = digits_for(a);
                let (db, _) = digits_for(b);
                let (res, neg) = subtract(&da, &db, first_neg);
                let mag = to_u128(&res[..res.len().min(4)]);
                assert!(res.len() <= 4 || res[4..].iter().all(|&w| w == 0));
                let signed = if neg { -(mag as i128) } else { mag as i128 };
                let expected = if first_neg {
                    b as i128 - a as i128
                } else {
                    a as i128 - b as i128
                };
                // A zero difference may come out as -0; the sign flag is
                // separate from the magnitude, exactly as in the C.
                if expected == 0 {
                    assert_eq!(mag, 0);
                } else {
                    assert_eq!(signed, expected, "a={a} b={b} neg={first_neg}");
                }
            }
        }
    }

    #[test]
    fn subtract_trims_common_leading_digits() {
        // Equal lengths, equal top digits: the C trims before comparing, so
        // the result is only as long as the differing prefix.
        let (res, neg) = subtract(&[5, 7, 9], &[3, 7, 9], false);
        assert_eq!(res, vec![2]);
        assert!(!neg);
        let (res, neg) = subtract(&[3, 7, 9], &[5, 7, 9], false);
        assert_eq!(res, vec![2]);
        assert!(neg);
    }

    #[test]
    fn subtract_equal_values_gives_zero_with_first_sign() {
        let (res, neg) = subtract(&[42, 7], &[42, 7], true);
        assert_eq!(res, vec![0]);
        assert!(neg);
    }

    #[test]
    fn subtract_unnormalized_input_wraps_like_the_c() {
        // second is longer only by a leading zero digit, so the C picks it as
        // "larger" and the difference wraps mod 2^64. Garbage in, the same
        // garbage out.
        let (res, neg) = subtract(&[9], &[1, 0], false);
        assert_eq!(res, vec![0xFFFF_FFF8, 0xFFFF_FFFF]);
        assert!(neg);
    }

    // ---- multiplication ----------------------------------------------------

    // ---- the delegated paths against the loops they replace ---------------

    /// The whole safety argument for handing multiplication to num-bigint is
    /// that the two paths are the same function. This checks that digit for
    /// digit, at sizes on both sides of `MUL_DELEGATE_DIGITS` -- including
    /// the partial-trailing-word shapes where `capacity` is one digit short
    /// of the full product and the top carry is dropped.
    #[test]
    fn delegated_multiply_agrees_with_the_loop() {
        let mut rng = Rng(0x5EED_1234_5678_9ABC);
        for case in 0..300 {
            // Three bands: tiny shapes exercise the dropped carry and the
            // zero short-circuits, the middle band sits either side of
            // `MUL_DELEGATE_DIGITS` where dispatch actually switches, and the
            // large band reaches Karatsuba and Toom-3.
            let (short_bytes, long_bytes) = match case % 3 {
                0 => (1 + rng.below(24) as usize, 1 + rng.below(24) as usize),
                1 => (50 + rng.below(80) as usize, 50 + rng.below(80) as usize),
                _ => (250 + rng.below(600) as usize, 250 + rng.below(600) as usize),
            };
            let short: Vec<u32> = (0..digit_len(short_bytes))
                .map(|_| rng.next() as u32)
                .collect();
            let long: Vec<u32> = (0..digit_len(long_bytes))
                .map(|_| rng.next() as u32)
                .collect();
            let capacity = digit_len(short_bytes + long_bytes);

            assert_eq!(
                multiply_delegated(&short, &long, capacity),
                multiply_schoolbook(&short, &long, capacity),
                "case {case}: {short_bytes} x {long_bytes} bytes",
            );
        }
    }

    /// Same argument for division: quotient digits, quotient byte length and
    /// the remainder's `(digits, byte_len)` -- including its `None` for a
    /// zero remainder -- must match the shift-and-Knuth-D path exactly.
    #[test]
    fn delegated_divide_agrees_with_the_loop() {
        let mut rng = Rng(0xD1D1_5108_ABCD_EF01);
        for case in 0..200 {
            let second_digits = 1 + rng.below(40) as usize;
            let first_digits = second_digits + rng.below(40) as usize;
            let mut second: Vec<u32> = (0..second_digits).map(|_| rng.next() as u32).collect();
            // The divisor must be normalized (non-zero top digit), which is
            // what `divide`'s caller guarantees.
            if second[second_digits - 1] == 0 {
                second[second_digits - 1] = 1;
            }
            let first: Vec<u32> = (0..first_digits).map(|_| rng.next() as u32).collect();
            let quo_digit_len = first_digits - second_digits + 1;

            assert_eq!(
                divide_delegated(&first, &second, quo_digit_len),
                divide_schoolbook(&first, &second, quo_digit_len),
                "case {case}: {first_digits} / {second_digits} digits",
            );
        }
    }

    /// An exact division, so the remainder is zero and both paths must answer
    /// `None` for it rather than a zero-valued magnitude.
    #[test]
    fn delegated_divide_agrees_on_a_zero_remainder() {
        let mut rng = Rng(0x0000_0000_DEAD_BEEF);
        for _ in 0..50 {
            let second_digits = 1 + rng.below(20) as usize;
            let mut second: Vec<u32> = (0..second_digits).map(|_| rng.next() as u32).collect();
            if second[second_digits - 1] == 0 {
                second[second_digits - 1] = 1;
            }
            let factor_digits = 1 + rng.below(20) as usize;
            let factor: Vec<u32> = (0..factor_digits).map(|_| rng.next() as u32).collect();

            // first := second * factor, so the division is exact.
            let product = to_big(&second) * to_big(&factor);
            let first = product.to_u32_digits();
            if first.len() < second_digits {
                continue;
            }
            let quo_digit_len = first.len() - second_digits + 1;

            let delegated = divide_delegated(&first, &second, quo_digit_len);
            assert_eq!(delegated.2, None, "an exact division has no remainder");
            assert_eq!(delegated, divide_schoolbook(&first, &second, quo_digit_len));
        }
    }

    #[test]
    fn multiply_matches_u128() {
        let mut rng = Rng(0x0DDB_A11B_EEF5_1DE5);
        for _ in 0..500 {
            let a = rng.next() as u128 & ((1 << (rng.below(64) + 1)) - 1);
            let b = rng.next() as u128 & ((1 << (rng.below(64) + 1)) - 1);
            let (da, la) = digits_for(a);
            let (db, lb) = digits_for(b);
            let (s, sb, l, lo) = if la <= lb {
                (&da, la, &db, lb)
            } else {
                (&db, lb, &da, la)
            };
            let prod = multiply(s, sb, l, lo);
            assert_eq!(prod.len(), digit_len(sb + lo));
            assert_eq!(to_u128(&prod), a * b, "a={a} b={b}");
        }
    }

    #[test]
    fn multiply_matches_schoolbook_reference() {
        let mut rng = Rng(0x5EED_5EED_5EED_5EED);
        for _ in 0..60 {
            let la = 1 + rng.below(40) as usize;
            let lb = 1 + rng.below(40) as usize;
            let a: Vec<u8> = (0..la).map(|_| rng.next() as u8).collect();
            let b: Vec<u8> = (0..lb).map(|_| rng.next() as u8).collect();
            let (da, db) = (bytes_to_digits(&a), bytes_to_digits(&b));
            let (s, sb, l, lo) = if la <= lb {
                (&da, la, &db, lb)
            } else {
                (&db, lb, &da, la)
            };
            let prod = multiply(s, sb, l, lo);
            let expected = ref_mul(&a, &b);
            assert_eq!(digits_to_bytes(&prod, la + lb), expected);
        }
    }

    #[test]
    fn multiply_zero_shortcuts() {
        // A single zero digit on either side leaves the product untouched.
        assert_eq!(multiply(&[0], 1, &[7, 8], 8), vec![0, 0, 0]);
        assert_eq!(multiply(&[7], 1, &[0], 1), vec![0]);
        // ... but a multi-digit zero goes through the loops with the same
        // result.
        assert_eq!(multiply(&[0, 0], 8, &[7, 8], 8), vec![0, 0, 0, 0]);
    }

    #[test]
    fn multiply_partial_top_words() {
        // 3-byte times 5-byte: the product capacity (2 digits) is smaller
        // than the operands' digit count sum, exercising the guarded carry.
        let a = 0xABCDEFu128;
        let b = 0x1122334455u128;
        let (da, la) = digits_for(a);
        let (db, lb) = digits_for(b);
        assert_eq!((la, lb), (3, 5));
        let prod = multiply(&da, la, &db, lb);
        assert_eq!(prod.len(), 2);
        assert_eq!(to_u128(&prod), a * b);
    }

    // ---- bit logic ---------------------------------------------------------

    #[test]
    fn bit_ops_match_u128() {
        let mut rng = Rng(0xB017_B017_B017_B017);
        for _ in 0..300 {
            let a = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let b = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let (da, _) = digits_for(a);
            let (db, _) = digits_for(b);
            let (s, l) = if da.len() <= db.len() {
                (&da, &db)
            } else {
                (&db, &da)
            };
            assert_eq!(to_u128(&bit_op(BitOp::And, s, l)), a & b);
            assert_eq!(to_u128(&bit_op(BitOp::Or, s, l)), a | b);
            assert_eq!(to_u128(&bit_op(BitOp::Xor, s, l)), a ^ b);
        }
    }

    #[test]
    fn bit_op_result_is_sized_like_the_longer_operand() {
        assert_eq!(
            bit_op(BitOp::And, &[0xFF], &[0xF0F0, 3, 9]),
            vec![0xF0, 0, 0]
        );
        assert_eq!(
            bit_op(BitOp::Or, &[0xFF], &[0xF0F0, 3, 9]),
            vec![0xF0FF, 3, 9]
        );
        assert_eq!(
            bit_op(BitOp::Xor, &[0xFF], &[0xF0F0, 3, 9]),
            vec![0xF00F, 3, 9]
        );
    }

    // ---- shifts ------------------------------------------------------------

    #[test]
    fn lshift_matches_u128() {
        let mut rng = Rng(0x15EA_F00D_15EA_F00D);
        for _ in 0..300 {
            let v = (rng.next() as u128 & ((1 << (rng.below(64) + 1)) - 1)) | 1;
            for &shift in &[0usize, 1, 7, 31, 32, 33, 63, 64] {
                let (dv, _) = digits_for(v);
                let (out, byte_len) = lshift(&dv, shift).expect("non-zero");
                let hb = 128 - v.leading_zeros() as usize;
                assert_eq!(byte_len, (hb + shift + 7) / 8);
                assert_eq!(out.len(), digit_len(byte_len));
                if hb + shift <= 128 {
                    assert_eq!(to_u128(&out), v << shift, "v={v} shift={shift}");
                }
            }
        }
    }

    #[test]
    fn lshift_of_zero_is_none() {
        assert_eq!(lshift(&[0], 5), None);
        assert_eq!(lshift(&[0, 0], 0), None);
        assert_eq!(lshift(&[], 3), None);
    }

    #[test]
    fn lshift_drops_leading_zero_digits() {
        // Unnormalized input: the result is sized from the high bit, not from
        // the input's digit count.
        let (out, byte_len) = lshift(&[1, 0], 0).unwrap();
        assert_eq!((out, byte_len), (vec![1], 1));
        let (out, byte_len) = lshift(&[1, 0], 1).unwrap();
        assert_eq!((out, byte_len), (vec![2], 1));
    }

    #[test]
    fn rshift_matches_u128() {
        let mut rng = Rng(0xDEAD_BEEF_DEAD_BEEF);
        for _ in 0..300 {
            let v = (rng.next() as u128) << 32 | rng.next() as u128 & 0xFFFF_FFFF;
            let v = v | 1;
            for &shift in &[0usize, 1, 7, 31, 32, 33, 63, 64, 95] {
                let (dv, _) = digits_for(v);
                let hb = 128 - v.leading_zeros() as usize;
                match rshift(&dv, shift, dv.len()) {
                    None => assert!(shift >= hb, "v={v} shift={shift}"),
                    Some((out, byte_len)) => {
                        assert_eq!(to_u128(&out), v >> shift, "v={v} shift={shift}");
                        assert_eq!(byte_len, (hb - shift + 7) / 8);
                        assert_eq!(out.len(), digit_len(byte_len));
                    }
                }
            }
        }
    }

    #[test]
    fn rshift_all_bits_lost_is_none() {
        assert_eq!(rshift(&[0xFF], 8, 1), None);
        assert_eq!(rshift(&[0], 0, 1), None);
        assert_eq!(rshift(&[], 1, 0), None);
    }

    #[test]
    fn rshift_looks_only_at_the_first_digits() {
        // look_first crops the magnitude before shifting: the top digit here
        // is invisible.
        let (out, byte_len) = rshift(&[0xFF, 0xAB], 4, 1).unwrap();
        assert_eq!((out, byte_len), (vec![0xF], 1));
        // Digit-aligned path; the byte length follows the high bit (bit 34
        // of the shifted value -> 5 bytes).
        let (out, byte_len) = rshift(&[1, 2, 3], 32, 3).unwrap();
        assert_eq!((out, byte_len), (vec![2, 3], 5));
    }

    // ---- anyBit ------------------------------------------------------------

    #[test]
    fn any_bit_matches_mask_arithmetic() {
        let mut rng = Rng(0xA11B_17A1_1B17_A11B);
        for _ in 0..300 {
            let v = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let (dv, _) = digits_for(v);
            let start = 1 + rng.below(100) as usize;
            let stop = 1 + rng.below(100) as usize;
            let expected = if start > stop {
                false
            } else {
                let width = stop - start + 1;
                let mask = if width >= 128 {
                    u128::MAX
                } else {
                    ((1u128 << width) - 1) << (start - 1)
                };
                v & mask != 0
            };
            assert_eq!(
                any_bit(&dv, start, stop),
                expected,
                "v={v} {start}..={stop}"
            );
        }
    }

    #[test]
    fn any_bit_edges() {
        let (dv, _) = digits_for(0x8000_0000_0000_0000u128);
        assert!(any_bit(&dv, 64, 64));
        assert!(!any_bit(&dv, 1, 63));
        assert!(any_bit(&dv, 1, 1000)); // stop clamps to the high bit
        assert!(!any_bit(&dv, 65, 1000)); // start above the high bit
        assert!(!any_bit(&digits_for(0).0, 1, 32)); // zero has no bits
    }

    // ---- division ----------------------------------------------------------

    #[test]
    fn divide_matches_u128() {
        let mut rng = Rng(0xD1F1_D1F1_D1F1_D1F1);
        for _ in 0..1000 {
            let a = rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1);
            let b = (rng.next() as u128 & ((1 << (rng.below(96) + 1)) - 1)) | 1;
            let (da, la) = digits_for(a);
            let (db, lb) = digits_for(b);
            if digit_len(la) < digit_len(lb) {
                continue; // the primitive answers {0. a} before dividing
            }
            let (quo, quo_bytes, rem) = divide(&da, la, &db, lb);
            assert_eq!(quo.len(), digit_len(la) - digit_len(lb) + 1);
            assert_eq!(quo_bytes, quo.len() * 4);
            assert_eq!(to_u128(&quo[..quo.len().min(4)]), a / b, "a={a} b={b}");
            assert!(quo.len() <= 4 || quo[4..].iter().all(|&w| w == 0));
            match rem {
                None => assert_eq!(a % b, 0, "a={a} b={b}"),
                Some((rd, rb)) => {
                    let r = a % b;
                    assert_ne!(r, 0);
                    assert_eq!(to_u128(&rd), r, "a={a} b={b}");
                    assert_eq!(rb, min_byte_len(r), "a={a} b={b}");
                }
            }
        }
    }

    #[test]
    fn divide_reconstructs_adversarial_operands() {
        // Build a = q*b + r from adversarial digit patterns (the ones that
        // exercise the q-estimate corrections and the add-back path), then
        // check the division recovers q and r exactly.
        let mut rng = Rng(0xC0FF_EEC0_FFEE_C0FF);
        let pattern = |rng: &mut Rng| -> u32 {
            match rng.below(7) {
                0 => 0,
                1 => 1,
                2 => 0xFFFF_FFFF,
                3 => 0xFFFF_FFFE,
                4 => 0x8000_0000,
                5 => 0x7FFF_FFFF,
                _ => rng.next() as u32,
            }
        };
        let mut checked = 0;
        for _ in 0..3000 {
            let bq: Vec<u8> = {
                let n = 1 + rng.below(4) as usize;
                let d: Vec<u32> = (0..n).map(|_| pattern(&mut rng)).collect();
                digits_to_bytes(&d, n * 4)
            };
            let bb: Vec<u8> = {
                let n = 1 + rng.below(4) as usize;
                let d: Vec<u32> = (0..n).map(|_| pattern(&mut rng)).collect();
                digits_to_bytes(&d, n * 4)
            };
            let q = trim(&bq).to_vec();
            let b = trim(&bb).to_vec();
            if b.is_empty() || q.is_empty() {
                continue;
            }
            // r: strictly below b, built by trimming random bytes until so.
            let mut r: Vec<u8> = (0..b.len()).map(|_| rng.next() as u8).collect();
            loop {
                let (dr, db2) = (bytes_to_digits(&r), bytes_to_digits(&b));
                let n = digit_len(b.len());
                let mut dr = dr;
                dr.resize(n, 0);
                if compare(&db2, &dr, n) == 1 {
                    break;
                }
                r.pop();
            }
            let a_bytes = {
                let prod = ref_mul(&q, &b);
                let sum = ref_add(&prod, &r);
                trim(&sum).to_vec()
            };
            let (da, la) = (bytes_to_digits(&a_bytes), a_bytes.len());
            let (db3, lb) = (bytes_to_digits(&b), b.len());
            if digit_len(la) < digit_len(lb) {
                continue;
            }
            let (quo, _, rem) = divide(&da, la, &db3, lb);
            let quo_bytes = digits_to_bytes(&quo, quo.len() * 4);
            assert_eq!(trim(&quo_bytes), trim(&q), "q mismatch");
            match rem {
                None => assert!(trim(&r).is_empty()),
                Some((rd, rb)) => {
                    assert_eq!(digits_to_bytes(&rd, rb), trim(&r).to_vec());
                }
            }
            checked += 1;
        }
        assert!(checked > 1000, "enough adversarial cases actually ran");
    }

    #[test]
    fn divide_small_known_answers() {
        // 5 / 3 = 1 rem 2; the quotient stays unnormalized at one full digit.
        let (quo, quo_bytes, rem) = divide(&[5], 1, &[3], 1);
        assert_eq!((quo, quo_bytes), (vec![1], 4));
        assert_eq!(rem, Some((vec![2], 1)));
        // 6 / 3 = 2 rem 0: the zero remainder is None (a 0-length object).
        let (quo, _, rem) = divide(&[6], 1, &[3], 1);
        assert_eq!(quo, vec![2]);
        assert_eq!(rem, None);
        // 0 / 3 = 0 rem 0.
        let (quo, _, rem) = divide(&[0], 1, &[3], 1);
        assert_eq!(quo, vec![0]);
        assert_eq!(rem, None);
    }

    #[test]
    fn divide_estimate_hits_the_dh_shortcut() {
        // Dividend top digit equal to the shifted divisor's top digit forces
        // the q = 0xFFFFFFFF path; the correction still recovers the truth.
        let b = 0x8000_0000u128;
        let a = 0x8000_0000_0000_0000u128 - 1;
        let (da, la) = digits_for(a);
        let (db, lb) = digits_for(b);
        let (quo, _, rem) = divide(&da, la, &db, lb);
        assert_eq!(to_u128(&quo), a / b);
        assert_eq!(rem.map(|(d, _)| to_u128(&d)), Some(a % b));
    }

    // ---- Montgomery --------------------------------------------------------

    /// Modular inverse by extended Euclid, for the test's expectations only.
    fn mod_inv(a: u128, m: u128) -> u128 {
        let (mut old_r, mut r) = (a as i128, m as i128);
        let (mut old_s, mut s) = (1i128, 0i128);
        while r != 0 {
            let q = old_r / r;
            (old_r, r) = (r, old_r - q * r);
            (old_s, s) = (s, old_s - q * s);
        }
        assert_eq!(old_r, 1, "not coprime");
        old_s.rem_euclid(m as i128) as u128
    }

    #[test]
    fn montgomery_matches_reference() {
        let mut rng = Rng(0x3070_3070_3070_3071);
        for _ in 0..300 {
            // An odd modulus of 1 or 2 digits keeps a*b*R^-1 within u128.
            let m = (rng.next() as u128 & ((1 << (33 + rng.below(31))) - 1)) | 1;
            let a = rng.next() as u128 % m;
            let b = rng.next() as u128 % m;
            let (dm, lm) = digits_for(m);
            let n = digit_len(lm);
            let (da, _) = digits_for(a);
            let (db, _) = digits_for(b);
            if da.len() > n || db.len() > n {
                continue;
            }
            // mInvModB = -m^-1 mod 2^32, as the image computes it.
            let m_inv = (0x1_0000_0000u128 - mod_inv(m & 0xFFFF_FFFF, 1 << 32)) as u32;
            let res = montgomery(&da, &db, &dm, m_inv).unwrap();
            assert_eq!(res.len(), n);
            let r = 1u128 << (32 * n);
            let expected = ((a * b) % m) * mod_inv(r % m, m) % m;
            assert_eq!(to_u128(&res), expected, "a={a} b={b} m={m}");
        }
    }

    #[test]
    fn montgomery_with_r_mod_m_is_identity() {
        // a * (R mod m) * R^-1 = a (mod m)
        let m = 0xFFFF_FFFB_u128; // prime, one digit
        let (dm, lm) = digits_for(m);
        let n = digit_len(lm);
        let r_mod_m = (1u128 << (32 * n)) % m;
        let m_inv = (0x1_0000_0000u128 - mod_inv(m, 1 << 32)) as u32;
        for a in [1u128, 2, 12345, m - 1] {
            let (da, _) = digits_for(a);
            let (dr, _) = digits_for(r_mod_m);
            let res = montgomery(&da, &dr, &dm, m_inv).unwrap();
            assert_eq!(to_u128(&res), a);
        }
    }

    #[test]
    fn montgomery_refuses_the_c_out_of_bounds_shape() {
        assert_eq!(montgomery(&[1], &[], &[3], 1), None);
        // Empty first is fine: the C never touches second then.
        assert!(montgomery(&[], &[], &[3], 1).is_some());
    }

    // ---- normalization -----------------------------------------------------

    #[test]
    fn normalize_zero_and_stripping() {
        assert_eq!(normalize_scan(&[], 0, false), Normalized::Small(0));
        assert_eq!(normalize_scan(&[0], 1, false), Normalized::Small(0));
        assert_eq!(normalize_scan(&[0, 0], 8, true), Normalized::Small(0));
        // Leading zero digits are stripped before deciding.
        assert_eq!(normalize_scan(&[1, 0], 8, false), Normalized::Small(1));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn normalize_small_integer_boundaries() {
        let max = digits_for(MAX_SMALL as u128).0;
        assert_eq!(
            normalize_scan(&max, 8, false),
            Normalized::Small(MAX_SMALL as isize)
        );
        let over = digits_for(MAX_SMALL as u128 + 1).0;
        assert_eq!(normalize_scan(&over, 8, false), Normalized::Large(8));
        // -(2^60) is the one negative value with no positive twin.
        let min = digits_for(MIN_SMALL_MAG as u128).0;
        assert_eq!(
            normalize_scan(&min, 8, true),
            Normalized::Small(-(1isize << 60))
        );
        let under = digits_for(MIN_SMALL_MAG as u128 + 1).0;
        assert_eq!(normalize_scan(&under, 8, true), Normalized::Large(8));
        // Two digits combine before the range check.
        let v = digits_for(1u128 << 32).0;
        assert_eq!(
            normalize_scan(&v, 8, false),
            Normalized::Small(1isize << 32)
        );
    }

    #[test]
    fn normalize_trims_top_zero_bytes() {
        // 2^64 stored in 12 bytes: three digits, top digit 1 -> 9 bytes.
        let v = digits_for(1u128 << 64).0;
        assert_eq!(normalize_scan(&v, 12, false), Normalized::Large(9));
        // Top digit needs all four bytes: nothing to trim.
        let v = digits_for(0xDEADBEEF_00000000_00000000_u128).0;
        assert_eq!(normalize_scan(&v, 12, false), Normalized::Large(12));
        // Top digit 0x00FFAABB: one byte trimmed.
        let v = digits_for(0x00FF_AABB_0000_0000_0000_0000_u128).0;
        assert_eq!(normalize_scan(&v, 12, false), Normalized::Large(11));
        // Top digit 0xFFAA: two bytes trimmed.
        let v = digits_for(0xFFAA_0000_0000_0000_0000_u128).0;
        assert_eq!(normalize_scan(&v, 12, false), Normalized::Large(10));
    }

    #[test]
    fn normalize_keeps_an_already_normal_length() {
        #[cfg(target_pointer_width = "64")]
        {
            let (d, l) = digits_for((1u128 << 61) + 1);
            assert_eq!(l, 8);
            assert_eq!(normalize_scan(&d, l, false), Normalized::Large(8));
        }
        let (d, l) = digits_for((0x11u128 << 64) | 0x0123_4567_89AB_CDEF);
        assert_eq!(l, 9);
        assert_eq!(normalize_scan(&d, l, false), Normalized::Large(9));
    }

    #[test]
    fn byte_domain_readers_agree_with_the_digit_domain() {
        // Every read-only scan has two spellings -- one over a computed digit
        // buffer, one over an image object's bytes. They share a body, so this
        // pins the shared body's two entry points to each other, and pins
        // `digit_at`'s zero padding to `bytes_to_digits`'.
        let mut rng = Rng(0xF00D_BEEF);
        for byte_len in 1..=40usize {
            for _ in 0..40 {
                let bytes: Vec<u8> = (0..byte_len)
                    .map(|_| {
                        // Plenty of zero bytes, so the leading-zero and
                        // all-zero paths are actually exercised.
                        if rng.below(3) == 0 {
                            0
                        } else {
                            rng.below(256) as u8
                        }
                    })
                    .collect();
                let words = bytes_to_digits(&bytes);
                assert_eq!(words.len(), digit_len(byte_len));
                for ix in 0..words.len() {
                    assert_eq!(digit_at(&bytes, ix), words[ix], "digit {ix} of {bytes:?}");
                }
                assert_eq!(high_bit_bytes(&bytes), high_bit(&words, words.len()));
                for negative in [false, true] {
                    assert_eq!(
                        normalize_scan_bytes(&bytes, negative),
                        normalize_scan(&words, byte_len, negative),
                        "normalize {bytes:?} negative={negative}"
                    );
                }
                for _ in 0..8 {
                    let from = 1 + rng.below(byte_len as u64 * 8 + 4) as usize;
                    let to = from + rng.below(40) as usize;
                    assert_eq!(
                        any_bit_bytes(&bytes, from, to),
                        any_bit(&words, from, to),
                        "any_bit {from}..={to} of {bytes:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn digit_at_zero_pads_past_the_end() {
        assert_eq!(digit_at(&[0xAA], 0), 0xAA);
        assert_eq!(digit_at(&[0x11, 0x22, 0x33], 0), 0x0033_2211);
        assert_eq!(digit_at(&[1, 0, 0, 0, 2], 1), 2);
        // Wholly past the end -- what a caller that rounds up to a digit
        // boundary asks for, and what the C reads out of allocation slack.
        assert_eq!(digit_at(&[1, 2, 3, 4], 1), 0);
        assert_eq!(digit_at(&[], 0), 0);
    }

    #[test]
    fn compare_bytes_agrees_with_the_digit_compare() {
        let mut rng = Rng(0x5EED_1234);
        for byte_len in 1..=17usize {
            let digit_count = digit_len(byte_len);
            for _ in 0..300 {
                let mut a: Vec<u8> = (0..byte_len).map(|_| rng.below(256) as u8).collect();
                let mut b: Vec<u8> = (0..byte_len).map(|_| rng.below(256) as u8).collect();
                // Force ties often enough to reach the equal-prefix path.
                let shared = rng.below(byte_len as u64 + 1) as usize;
                let tie = byte_len - shared;
                b[tie..].copy_from_slice(&a[tie..]);
                if rng.below(8) == 0 {
                    b.clone_from(&a);
                }
                // A shorter object with the same digit count must answer the
                // same, since its missing bytes read as the zeros its trailing
                // word holds. Dropping a zero top byte builds exactly that
                // pair: same value, one byte shorter, same digit count.
                if rng.below(4) == 0 && byte_len % 4 != 0 {
                    a[byte_len - 1] = 0;
                    a.truncate(byte_len - 1);
                }
                let mut da = bytes_to_digits(&a);
                da.resize(digit_count, 0);
                let db = bytes_to_digits(&b);
                assert_eq!(
                    compare_bytes(&a, &b, digit_count),
                    compare(&da, &db, digit_count),
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn with_bytes_matches_the_reference_serialization() {
        // `with_bytes` reinterprets the digit buffer's own memory on a
        // little-endian host; `digits_to_bytes` builds the bytes arithmetically
        // and is endian-independent. They must not disagree.
        let mut rng = Rng(0xC0FF_EE01);
        for byte_len in 0..=33usize {
            for _ in 0..50 {
                let bytes: Vec<u8> = (0..byte_len).map(|_| rng.below(256) as u8).collect();
                let words = bytes_to_digits(&bytes);
                assert_eq!(with_bytes(&words, byte_len, <[u8]>::to_vec), bytes);
                assert_eq!(
                    with_bytes(&words, byte_len, <[u8]>::to_vec),
                    digits_to_bytes(&words, byte_len)
                );
            }
        }
    }

    #[test]
    fn is_normalized_checks_the_top_byte() {
        assert!(!is_normalized_bytes(&[]));
        assert!(is_normalized_bytes(&[0xFF]));
        assert!(!is_normalized_bytes(&[0xFF, 0x00])); // top byte is zero
        assert!(is_normalized_bytes(&[0xFF, 0x01]));
        assert!(!is_normalized_bytes(&[0xFF, 0x00, 0x01, 0x00])); // ditto, 4 bytes
        assert!(is_normalized_bytes(&[0, 0, 0, 0, 1]));
        assert!(!is_normalized_bytes(&[0, 0, 0, 0, 1, 0]));
    }

    // ---- byte <-> digit plumbing -------------------------------------------

    #[test]
    fn bytes_digits_roundtrip() {
        let mut rng = Rng(0x0B0E_0B0E_0B0E_0B0E);
        for len in 0..24usize {
            let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let digits = bytes_to_digits(&bytes);
            assert_eq!(digits.len(), digit_len(len));
            assert_eq!(digits_to_bytes(&digits, len), bytes);
        }
    }

    #[test]
    fn bytes_to_digits_zero_pads_the_partial_word() {
        assert_eq!(bytes_to_digits(&[0xAA]), vec![0xAA]);
        assert_eq!(bytes_to_digits(&[0x11, 0x22, 0x33]), vec![0x0033_2211]);
        assert_eq!(bytes_to_digits(&[1, 0, 0, 0, 2]), vec![1, 2]);
    }

    #[test]
    fn digits_to_bytes_carries_the_add_overflow_byte() {
        // The add-with-carry path stores a whole extra digit but only one
        // extra byte, as the C writes the carry word into slack.
        assert_eq!(
            digits_to_bytes(&[0xFFFF_FFFE, 1], 5),
            vec![0xFE, 0xFF, 0xFF, 0xFF, 1]
        );
    }
}
