//! `LargeIntegers`, in Rust: drop-in replacement for the Slang-generated C
//! plugin (`plugins/LargeIntegers/src/common/LargeIntegers.c`, VMMaker
//! `oscog-eem.2495`).
//!
//! The plugin services the image's arbitrary-precision arithmetic: digit-level
//! add, subtract, multiply, divide, bit logic, shifts, comparison, Montgomery
//! multiplication and normalization over LargePositiveInteger /
//! LargeNegativeInteger byte objects and SmallIntegers.
//!
//! Same module name (version tag included — the image checks the prefix),
//! same primitive names, same argument shapes, same accessor depths, same
//! failure codes, and — deliberately — the same oddities:
//!
//! * `primDigitAdd`'s carry case, `primDigitBitShiftMagnitude`'s left shift
//!   and `primDigitDivNegative`'s quotient and remainder are answered
//!   **unnormalized**; the image normalizes them itself.
//! * `primDigitSubtract` on unnormalized operands answers the same
//!   mod-2³²ⁿ nonsense the C computes.
//! * `primNormalize*` answer the receiver itself when nothing needs
//!   trimming.
//! * Failure codes are per-check: a wrong-classed *argument* fails with
//!   `PrimErrBadArgument`, a wrong-classed *receiver* (the C's
//!   `success(...)` pattern) and all semantic errors (negative operands to
//!   bit logic, division by zero, unnormalized division operands) fail with
//!   the generic code.
//!
//! # What changes underneath
//!
//! The C converts SmallIntegers by allocating scratch LargeInteger objects in
//! image memory and runs every digit loop over raw object pointers, writing
//! whole words through partial trailing words into allocation slack. Here the
//! digit arithmetic runs in [`digits`] over owned Rust vectors — read the
//! operands, compute, then allocate exactly the answer — so no borrow ever
//! spans an allocation and nothing is written out of bounds. See the README
//! for the handful of undefined behaviours this removes.
//!
//! Those buffers are [`digits::DigitBuf`], which stays on the stack for
//! magnitudes up to 256 bits — the size image code actually meets.
//!
//! A buffer is what *computing* needs, not what that safety argument needs,
//! so the primitives that compute nothing never build one: `primDigitCompare`
//! answers from the objects' bytes, and so do `primAnyBitFromTo`,
//! `primNormalize*` and `primDigitDivNegative`'s guards. [`IntKind`] is the
//! classification on its own, with [`operand_of`] the step that costs a
//! magnitude.

// The crate is named for the shared library the VM loads (libLargeIntegers.so)
// and the primitive names are fixed by the image.
#![allow(non_snake_case)]

mod digits;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use digits::{BitOp, Normalized};

/// What the C's `getModuleName` answers: the `(e)` variant, since this build
/// is an external plugin. `sqNamedPrims.c` compares the name only up to the
/// space, so the version tag rides along exactly as in the C.
const MODULE_NAME: &str = "LargeIntegers v2.1 VMMaker.oscog-eem.2495 (e)";

pharo_plugin!("LargeIntegers v2.1 VMMaker.oscog-eem.2495 (e)");

// ---------------------------------------------------------------------------
// Proxy entries the safe API lacks
// ---------------------------------------------------------------------------

/// Raw proxy calls this plugin needs beyond `Interp`: class lookups for the
/// two LargeInteger classes, `stObject:at:put:` for the division's result
/// Array, and the 32-bit unsigned unboxing for the Montgomery inverse.
mod raw {
    use pharo_vm_plugin::{sqInt, Interp, Oop, PrimErr, PrimResult};

    /// Fetches one nullable proxy entry, failing as `Unsupported` when the VM
    /// did not supply it — the same policy as the safe wrappers.
    macro_rules! entry {
        ($vm:expr, $field:ident) => {{
            // SAFETY: as_raw() is the proxy table the VM passed to
            // setInterpreter, valid for the process lifetime and never
            // mutated afterwards.
            let vt = unsafe { &*$vm.as_raw() };
            vt.$field.ok_or(PrimErr::Unsupported)?
        }};
    }

    /// `fetchClassOf` — the class of any oop, immediates included.
    pub fn fetch_class_of(vm: &Interp, oop: Oop) -> PrimResult<Oop> {
        let f = entry!(vm, fetchClassOf);
        // SAFETY: proxy function with the signature virtualMachine.h declares.
        Ok(Oop(unsafe { f(oop.0) }))
    }

    /// The class `LargePositiveInteger`.
    pub fn class_large_positive(vm: &Interp) -> PrimResult<Oop> {
        let f = entry!(vm, classLargePositiveInteger);
        // SAFETY: as above.
        Ok(Oop(unsafe { f() }))
    }

    /// The class `LargeNegativeInteger`.
    pub fn class_large_negative(vm: &Interp) -> PrimResult<Oop> {
        let f = entry!(vm, classLargeNegativeInteger);
        // SAFETY: as above.
        Ok(Oop(unsafe { f() }))
    }

    /// `isBooleanObject` — is this `true` or `false`?
    pub fn is_boolean_object(vm: &Interp, oop: Oop) -> PrimResult<bool> {
        let f = entry!(vm, isBooleanObject);
        // SAFETY: as above.
        Ok(unsafe { f(oop.0) } != 0)
    }

    /// `positive32BitValueOf` — unboxes a non-negative integer below 2³².
    ///
    /// The proxy reports a bad value through the failure flag, which the C
    /// checks with `failed()` before computing; `check_failed` turns that
    /// into the same generic failure here.
    pub fn positive_32bit_value_of(vm: &Interp, oop: Oop) -> PrimResult<u32> {
        let f = entry!(vm, positive32BitValueOf);
        // SAFETY: as above.
        let value = unsafe { f(oop.0) };
        vm.check_failed()?;
        Ok(value)
    }

    /// `stObject:at:put:` — 1-based indexed store, with the store barrier.
    pub fn st_object_at_put(vm: &Interp, array: Oop, index: sqInt, value: Oop) -> PrimResult<()> {
        let f = entry!(vm, stObjectatput);
        // SAFETY: as above; the callers pass a freshly instantiated 2-slot
        // Array and in-range indices.
        unsafe { f(array.0, index, value.0) };
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Operands
// ---------------------------------------------------------------------------

/// What an integer-kind oop *is*, before any magnitude is read: the C's
/// `isKindOfInteger` decision on its own.
///
/// Several primitives need no more than this — a comparison, a bit test, a
/// normalization check — and for them reading the operand's digits into a
/// buffer is pure waste, so the classification is separate from [`Operand`].
#[derive(Clone, Copy)]
enum IntKind {
    /// A SmallInteger, unboxed.
    Small(isize),
    /// A LargeInteger: the object itself, and whether its class is the
    /// negative one.
    Large { oop: Oop, negative: bool },
}

impl IntKind {
    fn negative(self) -> bool {
        match self {
            IntKind::Small(value) => value < 0,
            IntKind::Large { negative, .. } => negative,
        }
    }
}

/// Classifies an oop as SmallInteger, LargePositiveInteger or
/// LargeNegativeInteger. Answers `None` for anything else, mirroring the C's
/// `isKindOfInteger`.
fn integer_kind(vm: &Interp, oop: Oop) -> PrimResult<Option<IntKind>> {
    if vm.is_integer_object(oop)? {
        return Ok(Some(IntKind::Small(vm.integer_value(oop)?)));
    }
    let class = raw::fetch_class_of(vm, oop)?;
    let negative = if class == raw::class_large_positive(vm)? {
        false
    } else if class == raw::class_large_negative(vm)? {
        true
    } else {
        return Ok(None);
    };
    Ok(Some(IntKind::Large { oop, negative }))
}

/// An integer operand as the C sees it after `createLargeFromSmallInteger:`:
/// a magnitude plus where it came from.
struct Operand {
    /// Little-endian 32-bit digits, exactly `digit_len(byte_len)` of them.
    digits: digits::DigitBuf,
    /// The magnitude's length in bytes (`slotSizeOf` in the C).
    byte_len: usize,
    kind: IntKind,
}

impl Operand {
    fn digit_count(&self) -> usize {
        digits::digit_len(self.byte_len)
    }

    fn negative(&self) -> bool {
        self.kind.negative()
    }

    /// The original object, when the operand already was a LargeInteger.
    fn oop(&self) -> Option<Oop> {
        match self.kind {
            IntKind::Large { oop, .. } => Some(oop),
            IntKind::Small(_) => None,
        }
    }
}

/// Reads a classified oop's magnitude — the one step that costs a buffer, so
/// the primitives that can answer without it do not call this.
///
/// A SmallInteger becomes its unnormalized magnitude digits without the
/// scratch LargeInteger object the C allocates for the same purpose.
fn operand_of(vm: &Interp, kind: IntKind) -> PrimResult<Operand> {
    Ok(match kind {
        IntKind::Small(value) => Operand {
            digits: digits::small_digits(value),
            byte_len: digits::small_byte_size(value),
            kind,
        },
        IntKind::Large { oop, .. } => {
            let bytes = vm.bytes_of(oop)?;
            Operand {
                digits: digits::bytes_to_digits(bytes),
                byte_len: bytes.len(),
                kind,
            }
        }
    })
}

/// Reads an integer-kind oop as an [`Operand`], or `None` if it is not one.
fn integer_operand(vm: &Interp, oop: Oop) -> PrimResult<Option<Operand>> {
    match integer_kind(vm, oop)? {
        Some(kind) => Ok(Some(operand_of(vm, kind)?)),
        None => Ok(None),
    }
}

/// The C's checked-argument pattern: not an integer kind fails with
/// `PrimErrBadArgument`.
fn argument_kind(vm: &Interp, offset: sqInt) -> PrimResult<IntKind> {
    integer_kind(vm, vm.stack_value(offset)?)?.ok_or(PrimErr::BadArgument)
}

/// The C's receiver pattern, `success(isKindOfInteger(...))`: not an integer
/// kind fails with the generic code.
fn receiver_kind(vm: &Interp, offset: sqInt) -> PrimResult<IntKind> {
    integer_kind(vm, vm.stack_value(offset)?)?.ok_or(PrimErr::GenericFailure)
}

/// [`argument_kind`], magnitude included.
fn argument_operand(vm: &Interp, offset: sqInt) -> PrimResult<Operand> {
    let kind = argument_kind(vm, offset)?;
    operand_of(vm, kind)
}

/// [`receiver_kind`], magnitude included.
fn receiver_operand(vm: &Interp, offset: sqInt) -> PrimResult<Operand> {
    let kind = receiver_kind(vm, offset)?;
    operand_of(vm, kind)
}

/// The LargeInteger class for a sign.
fn large_class(vm: &Interp, negative: bool) -> PrimResult<Oop> {
    if negative {
        raw::class_large_negative(vm)
    } else {
        raw::class_large_positive(vm)
    }
}

/// The class a shift or division result inherits: the operand's own class
/// (`fetchClassOf` in the C) for a LargeInteger, the sign's class for a
/// converted SmallInteger.
fn operand_class(vm: &Interp, operand: &Operand) -> PrimResult<Oop> {
    match operand.oop() {
        Some(oop) => raw::fetch_class_of(vm, oop),
        None => large_class(vm, operand.negative()),
    }
}

/// Instantiates a `byte_len`-byte instance of `class` holding `digits`.
fn make_large(vm: &Interp, class: Oop, digits: &[u32], byte_len: usize) -> PrimResult<Oop> {
    let oop = vm.instantiate(class, byte_len as sqInt)?;
    // On a little-endian host the digits are already their own byte image, so
    // this memcpys straight out of the computed buffer -- no second one.
    digits::with_bytes(digits, byte_len, |bytes| vm.write_bytes(oop, 0, bytes))?;
    Ok(oop)
}

/// `normalizePositive:` / `normalizeNegative:` over a freshly computed
/// magnitude: a SmallInteger when the value fits, otherwise a LargeInteger of
/// the trimmed byte length.
///
/// The C allocates the full-size object first and then a shrunk copy; the
/// answer is identical with one allocation.
fn return_normalized(
    vm: &Interp,
    digits: &[u32],
    byte_len: usize,
    negative: bool,
) -> PrimResult<Oop> {
    match digits::normalize_scan(digits, byte_len, negative) {
        Normalized::Small(value) => vm.integer(value),
        Normalized::Large(len) => make_large(vm, large_class(vm, negative)?, digits, len),
    }
}

// ---------------------------------------------------------------------------
// Arithmetic primitives
// ---------------------------------------------------------------------------

/// Magnitude addition; the result carries the first operand's sign.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitAdd(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let second = argument_operand(vm, 0)?;
    let first = receiver_operand(vm, 1)?;
    let neg = first.negative();
    let (short, long) = if first.digits.len() <= second.digits.len() {
        (&first.digits, &second.digits)
    } else {
        (&second.digits, &first.digits)
    };
    let long_len = long.len();
    let (mut sum, over) = digits::add(short, long);
    if over > 0 {
        // The C grows the sum by one byte for the carry and answers it
        // unnormalized — the top byte is the carry, so it cannot have leading
        // zeros anyway.
        sum.push(over);
        make_large(vm, large_class(vm, neg)?, &sum, long_len * 4 + 1)
    } else {
        return_normalized(vm, &sum, long_len * 4, neg)
    }
}

/// Magnitude subtraction; the sign flips when the second magnitude is larger.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitSubtract(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let second = argument_operand(vm, 0)?;
    let first = receiver_operand(vm, 1)?;
    let (res, neg) = digits::subtract(&first.digits, &second.digits, first.negative());
    let byte_len = res.len() * 4;
    return_normalized(vm, &res, byte_len, neg)
}

/// Magnitude product; the sign comes as the boolean argument.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitMultiplyNegative(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(2)?;
    let second = integer_operand(vm, vm.stack_value(1)?)?.ok_or(PrimErr::BadArgument)?;
    let neg_oop = vm.stack_value(0)?;
    if !raw::is_boolean_object(vm, neg_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let neg = vm.boolean_value(neg_oop)?;
    let first = receiver_operand(vm, 2)?;
    // The C picks short/long by byte length, first on a tie.
    let (short, long) = if first.byte_len <= second.byte_len {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let prod = digits::multiply(&short.digits, short.byte_len, &long.digits, long.byte_len);
    return_normalized(vm, &prod, short.byte_len + long.byte_len, neg)
}

/// Magnitude division: answers an Array of {quotient. remainder}, both
/// **unnormalized** — the image normalizes them.
///
/// Fails (generic) on a zero divisor and on unnormalized LargeInteger
/// operands, which would derail the quotient estimation.
#[pharo_primitive(accessor_depth = 2)]
fn primDigitDivNegative(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(2)?;
    let second_kind = integer_kind(vm, vm.stack_value(1)?)?.ok_or(PrimErr::BadArgument)?;
    let neg_oop = vm.stack_value(0)?;
    if !raw::is_boolean_object(vm, neg_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let neg = vm.boolean_value(neg_oop)?;
    let first_kind = receiver_kind(vm, 2)?;
    // Every guard reads a top byte or an unboxed value, so all three run
    // before either magnitude is read: a rejected division costs no copying.
    if let IntKind::Large { oop, .. } = first_kind {
        if !digits::is_normalized_bytes(vm.bytes_of(oop)?) {
            return Err(PrimErr::GenericFailure);
        }
    }
    if let IntKind::Small(0) = second_kind {
        return Err(PrimErr::GenericFailure);
    }
    if let IntKind::Large { oop, .. } = second_kind {
        if !digits::is_normalized_bytes(vm.bytes_of(oop)?) {
            return Err(PrimErr::GenericFailure);
        }
    }
    let second = operand_of(vm, second_kind)?;
    let first = operand_of(vm, first_kind)?;

    let first_class = operand_class(vm, &first)?;
    if first.digit_count() < second.digit_count() {
        // The quotient would have no digits: answer {0. dividend} directly.
        // For a SmallInteger dividend the C stores its scratch LargeInteger
        // conversion, so materialize the same object here.
        let rem_oop = match first.oop() {
            Some(oop) => oop,
            None => make_large(vm, first_class, &first.digits, first.byte_len)?,
        };
        let quo_oop = vm.integer(0)?;
        return div_result_array(vm, quo_oop, rem_oop);
    }

    let (quo, quo_bytes, rem) = digits::divide(
        &first.digits,
        first.byte_len,
        &second.digits,
        second.byte_len,
    );
    let quo_oop = make_large(vm, large_class(vm, neg)?, &quo, quo_bytes)?;
    let rem_oop = match rem {
        Some((rem_digits, rem_bytes)) => make_large(vm, first_class, &rem_digits, rem_bytes)?,
        // A zero remainder is a 0-length LargeInteger of the dividend's class.
        None => vm.instantiate(first_class, 0)?,
    };
    div_result_array(vm, quo_oop, rem_oop)
}

/// The division's 2-slot result Array.
fn div_result_array(vm: &Interp, quo: Oop, rem: Oop) -> PrimResult<Oop> {
    let array = vm.instantiate(vm.class_array()?, 2)?;
    raw::st_object_at_put(vm, array, 1, quo)?;
    raw::st_object_at_put(vm, array, 2, rem)?;
    Ok(array)
}

/// Magnitude comparison: 1, 0, -1 for receiver >, =, < argument.
///
/// Two SmallIntegers compare by value magnitude; a SmallInteger against a
/// LargeInteger is always smaller (the C never checks whether the large is
/// normalized), and two LargeIntegers compare digit length first.
#[pharo_primitive(accessor_depth = 0)]
fn primDigitCompare(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(1)?;
    let second = argument_kind(vm, 0)?;
    let first = receiver_kind(vm, 1)?;
    Ok(match (first, second) {
        (IntKind::Small(f), IntKind::Small(s)) => {
            // SmallIntegers are tagged, so their magnitudes cannot overflow.
            let (f, s) = (f.unsigned_abs(), s.unsigned_abs());
            match f.cmp(&s) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Greater => 1,
            }
        }
        (IntKind::Small(_), IntKind::Large { .. }) => -1,
        (IntKind::Large { .. }, IntKind::Small(_)) => 1,
        (IntKind::Large { oop: f, .. }, IntKind::Large { oop: s, .. }) => {
            // The answer is three values wide, so building two magnitudes to
            // reach it was the whole cost of this primitive. Both objects are
            // compared where they lie, most significant byte first.
            let (first_bytes, second_bytes) = (vm.bytes_of(f)?, vm.bytes_of(s)?);
            let fdl = digits::digit_len(first_bytes.len());
            let sdl = digits::digit_len(second_bytes.len());
            if sdl != fdl {
                if sdl > fdl {
                    -1
                } else {
                    1
                }
            } else {
                digits::compare_bytes(first_bytes, second_bytes, fdl) as isize
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Bit logic and shifts
// ---------------------------------------------------------------------------

/// `digitBitLogic:with:opIndex:` — defined for non-negative operands only;
/// a negative one fails with the plain (generic) code, as in the C.
fn digit_bit_logic(vm: &Interp, op: BitOp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let second = argument_operand(vm, 0)?;
    let first = receiver_operand(vm, 1)?;
    if first.negative() || second.negative() {
        return Err(PrimErr::GenericFailure);
    }
    // The C picks short/long by byte length, second on a tie.
    let (short, long) = if first.byte_len < second.byte_len {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let res = digits::bit_op(op, &short.digits, &long.digits);
    return_normalized(vm, &res, long.byte_len, false)
}

/// Bitwise and of two non-negative integers.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitBitAnd(vm: &Interp) -> PrimResult<Oop> {
    digit_bit_logic(vm, BitOp::And)
}

/// Bitwise or of two non-negative integers.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitBitOr(vm: &Interp) -> PrimResult<Oop> {
    digit_bit_logic(vm, BitOp::Or)
}

/// Bitwise xor of two non-negative integers.
#[pharo_primitive(accessor_depth = 1)]
fn primDigitBitXor(vm: &Interp) -> PrimResult<Oop> {
    digit_bit_logic(vm, BitOp::Xor)
}

/// Shifts the receiver's magnitude; positive counts shift left, negative
/// right. A left shift is answered **unnormalized** (the image normalizes);
/// a right shift is normalized here, as in the C.
#[pharo_primitive(accessor_depth = 3)]
fn primDigitBitShiftMagnitude(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let shift_oop = vm.stack_value(0)?;
    if !vm.is_integer_object(shift_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let shift_count = vm.integer_value(shift_oop)?;
    let first = receiver_operand(vm, 1)?;
    let class = operand_class(vm, &first)?;
    if shift_count >= 0 {
        let shift = shift_count as usize;
        let hb = digits::high_bit(&first.digits, first.digits.len());
        if hb == 0 {
            // The C shifts a zero magnitude to a fresh 1-byte zero
            // LargeInteger and answers it unnormalized.
            return vm.instantiate(class, 1);
        }
        // Allocate before computing, as the C does: an absurd shift count
        // fails with PrimErrNoMemory instead of first building the digits.
        let new_byte_len = (hb + shift).div_ceil(8);
        let oop = vm.instantiate(class, new_byte_len as sqInt)?;
        let (words, byte_len) =
            digits::lshift(&first.digits, shift).expect("magnitude is non-zero");
        debug_assert_eq!(byte_len, new_byte_len);
        digits::with_bytes(&words, new_byte_len, |bytes| vm.write_bytes(oop, 0, bytes))?;
        Ok(oop)
    } else {
        let shift = shift_count.unsigned_abs();
        match digits::rshift(&first.digits, shift, first.digits.len()) {
            // All bits lost: the C builds a 0-length LargeInteger and
            // normalizes it, which always answers SmallInteger 0.
            None => vm.integer(0),
            Some((words, byte_len)) => return_normalized(vm, &words, byte_len, first.negative()),
        }
    }
}

/// Any magnitude bit set between the 1-based positions `from` and `to`?
#[pharo_primitive(accessor_depth = 1)]
fn primAnyBitFromTo(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(2)?;
    let from_oop = vm.stack_value(1)?;
    let to_oop = vm.stack_value(0)?;
    if !vm.is_integer_object(from_oop)? || !vm.is_integer_object(to_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let from = vm.integer_value(from_oop)?;
    let to = vm.integer_value(to_oop)?;
    let receiver = receiver_kind(vm, 2)?;
    if from < 1 || to < 1 {
        return Err(PrimErr::GenericFailure);
    }
    let (from, to) = (from as usize, to as usize);
    Ok(match receiver {
        // A LargeInteger's magnitude is scanned where it lies.
        IntKind::Large { oop, .. } => digits::any_bit_bytes(vm.bytes_of(oop)?, from, to),
        IntKind::Small(value) => digits::any_bit(&digits::small_digits(value), from, to),
    })
}

// ---------------------------------------------------------------------------
// Montgomery multiplication
// ---------------------------------------------------------------------------

/// The Montgomery digit size this plugin works in: 32 bits.
#[pharo_primitive(accessor_depth = -1)]
fn primMontgomeryDigitLength(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(32)
}

/// `receiver * arg * (2³²)⁻ⁿ mod modulo`, for n the modulo's digit length —
/// the inner step of Montgomery exponentiation. `mInvModB` must be
/// `-modulo⁻¹ mod 2³²` as a non-negative integer below 2³².
#[pharo_primitive(accessor_depth = 1)]
fn primMontgomeryTimesModulo(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(3)?;
    let second = integer_operand(vm, vm.stack_value(2)?)?.ok_or(PrimErr::BadArgument)?;
    let third = integer_operand(vm, vm.stack_value(1)?)?.ok_or(PrimErr::BadArgument)?;
    let m_inv_oop = vm.stack_value(0)?;
    if integer_operand(vm, m_inv_oop)?.is_none() {
        return Err(PrimErr::BadArgument);
    }
    let first = receiver_operand(vm, 3)?;
    let m_inv = raw::positive_32bit_value_of(vm, m_inv_oop)?;
    let third_len = third.digit_count();
    if !(first.digit_count() <= third_len && second.digit_count() <= third_len) {
        return Err(PrimErr::GenericFailure);
    }
    let prod = digits::montgomery(&first.digits, &second.digits, &third.digits, m_inv)
        .ok_or(PrimErr::GenericFailure)?;
    return_normalized(vm, &prod, third_len * 4, false)
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// `normalizePositive:` / `normalizeNegative:` on an image object: answers
/// the receiver itself when it is already normal, a SmallInteger when the
/// value fits, or a trimmed copy of the receiver's class.
fn normalize_receiver(vm: &Interp, negative: bool) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let receiver = vm.stack_value(0)?;
    let class = raw::fetch_class_of(vm, receiver)?;
    let expected = large_class(vm, negative)?;
    if class != expected {
        return Err(PrimErr::GenericFailure);
    }
    // The scan reads the receiver in place; only an actual trim needs a
    // buffer, and only because the prefix has to outlive the allocation.
    match digits::normalize_scan_bytes(vm.bytes_of(receiver)?, negative) {
        Normalized::Small(value) => vm.integer(value),
        Normalized::Large(len) => {
            let prefix = {
                let bytes = vm.bytes_of(receiver)?;
                if len >= bytes.len() {
                    return Ok(receiver);
                }
                bytes[..len].to_vec()
            };
            let oop = vm.instantiate(class, len as sqInt)?;
            vm.write_bytes(oop, 0, &prefix)?;
            Ok(oop)
        }
    }
}

/// Strips leading zero bytes from a LargePositiveInteger, reducing to a
/// SmallInteger when the value fits.
#[pharo_primitive(accessor_depth = 1)]
fn primNormalizePositive(vm: &Interp) -> PrimResult<Oop> {
    normalize_receiver(vm, false)
}

/// Strips leading zero bytes from a LargeNegativeInteger, reducing to a
/// SmallInteger when the value fits.
#[pharo_primitive(accessor_depth = 1)]
fn primNormalizeNegative(vm: &Interp) -> PrimResult<Oop> {
    normalize_receiver(vm, true)
}

// ---------------------------------------------------------------------------
// Module identity
// ---------------------------------------------------------------------------

/// Answers the module name as a String, so the image can check which
/// LargeIntegers revision it is talking to. Failing this primitive at all is
/// how the image detects the module is missing.
#[pharo_primitive(accessor_depth = -1)]
fn primGetModuleName(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    // The C strncpy's into the object without checking the allocation; here
    // a failed allocation is a clean PrimErrNoMemory failure.
    let oop = vm.instantiate(vm.class_string()?, MODULE_NAME.len() as sqInt)?;
    vm.write_bytes(oop, 0, MODULE_NAME.as_bytes())?;
    Ok(oop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    /// The three names that must agree, plus the version-tag contract: the VM
    /// validates the compiled-in name against the module it asked for by
    /// prefix, so it must start with exactly "LargeIntegers ".
    #[test]
    fn module_name_keeps_the_version_tag() {
        let compiled = unsafe { CStr::from_ptr(getModuleName()) };
        assert_eq!(compiled.to_str().unwrap(), MODULE_NAME);
        assert!(MODULE_NAME.starts_with("LargeIntegers "));
        assert!(MODULE_NAME.contains("v2.1"));
        assert!(MODULE_NAME.ends_with("(e)"));
    }

    /// The core's SmallInteger bounds must agree with the SDK's, which are
    /// pinned against the generated interp.h.
    #[test]
    fn small_integer_bounds_match_the_sdk() {
        use pharo_vm_plugin::proxy::{MAX_SMALL_INTEGER, MIN_SMALL_INTEGER};
        assert_eq!(digits::MAX_SMALL, MAX_SMALL_INTEGER as u64);
        assert_eq!(
            digits::MIN_SMALL_MAG,
            MIN_SMALL_INTEGER.unsigned_abs() as u64
        );
    }
}
