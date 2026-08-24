//! A safe handle on the interpreter proxy.

use core::slice;
use std::ffi::CString;

use crate::error::{PrimErr, PrimResult};
use crate::proxy::{sqInt, VirtualMachine, MAX_SMALL_INTEGER, MIN_SMALL_INTEGER};

/// An ordinary object pointer: a reference to a heap object, or an immediate
/// value (SmallInteger, Character, SmallFloat) encoded in the pointer's tag
/// bits.
///
/// Wrapped rather than passed as a bare integer so that an oop cannot be
/// confused with a count, an index or a C integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Oop(pub sqInt);

/// Calls a proxy function pointer, failing cleanly if the VM did not supply it.
///
/// Every field of the proxy is nullable. A missing entry means the VM is older
/// or differently configured than this crate expects -- a deployment problem,
/// not bad input -- so it surfaces as `Unsupported` rather than a panic, which
/// must never unwind into C.
macro_rules! call {
    ($self:ident, $field:ident ( $($arg:expr),* $(,)? )) => {{
        let f = $self.vt().$field.ok_or(PrimErr::Unsupported)?;
        // SAFETY: the pointer came from the VM's own proxy table, and the
        // signature is the one bindgen read from virtualMachine.h.
        unsafe { f($($arg),*) }
    }};
}

/// Same, for calls whose failure cannot be reported (no `Result` in scope).
macro_rules! call_opt {
    ($self:ident, $field:ident ( $($arg:expr),* $(,)? )) => {{
        match $self.vt().$field {
            // SAFETY: as above.
            Some(f) => Some(unsafe { f($($arg),*) }),
            None => None,
        }
    }};
}

/// The interpreter, as seen from inside a primitive.
///
/// Obtained by the `#[pharo_primitive]` machinery; plugin code receives one by
/// reference and never constructs it.
#[derive(Debug, Clone, Copy)]
pub struct Interp {
    vt: *mut VirtualMachine,
}

// The VM calls primitives on its own single interpreter thread, so the handle
// is only ever used from that thread. It is not Sync/Send by accident: it is
// simply never sent anywhere.
impl Interp {
    /// Wraps the proxy pointer the VM passed to `setInterpreter`.
    ///
    /// # Safety
    ///
    /// `vt` must be the non-null proxy table supplied by the VM, valid for the
    /// lifetime of the process.
    #[must_use]
    pub const unsafe fn from_raw(vt: *mut VirtualMachine) -> Self {
        Self { vt }
    }

    /// The raw proxy table, for operations this wrapper does not yet cover.
    #[must_use]
    pub const fn as_raw(&self) -> *mut VirtualMachine {
        self.vt
    }

    #[inline]
    fn vt(&self) -> &VirtualMachine {
        // SAFETY: from_raw's contract guarantees a valid, process-lifetime
        // pointer, and the VM never mutates the table after handing it over.
        unsafe { &*self.vt }
    }

    // ---- versions ----------------------------------------------------------

    /// Proxy major version this VM implements.
    pub fn major_version(&self) -> PrimResult<sqInt> {
        Ok(call!(self, majorVersion()))
    }

    /// Proxy minor version this VM implements.
    pub fn minor_version(&self) -> PrimResult<sqInt> {
        Ok(call!(self, minorVersion()))
    }

    // ---- the stack ---------------------------------------------------------

    /// How many arguments the current primitive was called with.
    pub fn argument_count(&self) -> PrimResult<sqInt> {
        Ok(call!(self, methodArgumentCount()))
    }

    /// Fails unless the primitive was called with exactly `n` arguments.
    ///
    /// Worth calling first in almost every primitive: a named primitive can be
    /// installed in a method of any arity, so the argument count is input, not
    /// a given.
    pub fn expect_argument_count(&self, n: sqInt) -> PrimResult<()> {
        if self.argument_count()? == n {
            Ok(())
        } else {
            Err(PrimErr::BadNumArgs)
        }
    }

    /// The oop `offset` slots down the stack. Offset 0 is the last argument,
    /// or the receiver when the primitive takes none.
    pub fn stack_value(&self, offset: sqInt) -> PrimResult<Oop> {
        Ok(Oop(call!(self, stackValue(offset))))
    }

    /// The receiver of the current primitive.
    pub fn receiver(&self) -> PrimResult<Oop> {
        let argc = self.argument_count()?;
        self.stack_value(argc)
    }

    /// The oop at `offset`, as an integer.
    ///
    /// Fails with `BadArgument` unless it really is a SmallInteger, rather
    /// than silently coercing.
    pub fn stack_integer(&self, offset: sqInt) -> PrimResult<sqInt> {
        let oop = self.stack_value(offset)?;
        if !self.is_integer_object(oop)? {
            return Err(PrimErr::BadArgument);
        }
        let v = call!(self, stackIntegerValue(offset));
        self.check_failed()?;
        Ok(v)
    }

    /// The oop at `offset`, as a float. Accepts SmallIntegers too, matching
    /// the VM's own `stackFloatValue`.
    pub fn stack_float(&self, offset: sqInt) -> PrimResult<f64> {
        let v = call!(self, stackFloatValue(offset));
        self.check_failed()?;
        Ok(v)
    }

    /// Reads all of the current primitive's arguments at once, typed.
    ///
    /// The tuple's arity is checked against the actual argument count, and its
    /// elements are the arguments in declaration order -- the first element is
    /// the method's first argument, however deep it sits on the stack:
    ///
    /// ```ignore
    /// let (form, quality): (Oop, sqInt) = vm.args()?;
    /// ```
    pub fn args<T: StackArgs>(&self) -> PrimResult<T> {
        T::from_stack_args(self)
    }

    /// Pushes an oop onto the stack.
    pub fn push(&self, oop: Oop) -> PrimResult<()> {
        call!(self, push(oop.0));
        Ok(())
    }

    /// Pops `n` items.
    pub fn pop(&self, n: sqInt) -> PrimResult<()> {
        call!(self, pop(n));
        Ok(())
    }

    // ---- testing -----------------------------------------------------------

    /// Is this an immediate value rather than a heap object?
    pub fn is_immediate(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isImmediate(oop.0)) != 0)
    }

    /// Is this a SmallInteger?
    pub fn is_integer_object(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isIntegerObject(oop.0)) != 0)
    }

    /// Is this a byte-indexable object (ByteArray, String, ...)?
    pub fn is_bytes(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isBytes(oop.0)) != 0)
    }

    /// Is this a pointer-indexable object?
    pub fn is_pointers(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isPointers(oop.0)) != 0)
    }

    /// Is this a boxed Float?
    pub fn is_float_object(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isFloatObject(oop.0)) != 0)
    }

    /// Did a previous proxy call set the failure flag?
    ///
    /// The older proxy accessors report errors this way instead of returning
    /// a status, so the safe wrappers consult it after calling them.
    pub fn check_failed(&self) -> PrimResult<()> {
        if call!(self, failed()) != 0 {
            Err(PrimErr::GenericFailure)
        } else {
            Ok(())
        }
    }

    // ---- sizes and contents ------------------------------------------------

    /// Size in bytes of a byte-indexable object.
    pub fn byte_size_of(&self, oop: Oop) -> PrimResult<sqInt> {
        Ok(call!(self, byteSizeOf(oop.0)))
    }

    /// Number of slots in a pointer object.
    pub fn slot_size_of(&self, oop: Oop) -> PrimResult<sqInt> {
        Ok(call!(self, slotSizeOf(oop.0)))
    }

    /// The bytes of a byte-indexable object.
    ///
    /// Fails with `BadArgument` if `oop` is not byte-indexable.
    ///
    /// The borrow is valid only until the next allocation or garbage
    /// collection. Inside a primitive that is the whole body, because
    /// primitives do not allocate unless they ask to -- but do not stash the
    /// slice anywhere.
    pub fn bytes_of(&self, oop: Oop) -> PrimResult<&[u8]> {
        let (ptr, len) = self.byte_region(oop)?;
        // SAFETY: firstIndexableField points at `len` readable bytes owned by
        // the object, and nothing moves it for the duration of the borrow.
        Ok(unsafe { slice::from_raw_parts(ptr, len) })
    }

    /// Writes `src` into a byte-indexable object at `offset`.
    ///
    /// Fails with `BadArgument` if `oop` is not byte-indexable, `BadIndex` if
    /// the write would run past the end, and `NoModification` if the object is
    /// immutable.
    ///
    /// This is deliberately a write rather than a `&mut [u8]`. Handing out a
    /// mutable slice derived from `&self` would let two of them alias the same
    /// object, which is undefined behaviour in Rust even though the underlying
    /// C API permits it. Copying in avoids the question entirely.
    pub fn write_bytes(&self, oop: Oop, offset: usize, src: &[u8]) -> PrimResult<()> {
        if call!(self, isOopImmutable(oop.0)) != 0 {
            return Err(PrimErr::NoModification);
        }
        let (ptr, len) = self.byte_region(oop)?;
        let end = offset.checked_add(src.len()).ok_or(PrimErr::BadIndex)?;
        if end > len {
            return Err(PrimErr::BadIndex);
        }
        // SAFETY: the destination lies wholly inside the object (checked just
        // above), the object is mutable, and `src` cannot overlap it because
        // it is a Rust slice the caller owns.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), ptr.cast_mut().add(offset), src.len());
        }
        Ok(())
    }

    fn byte_region(&self, oop: Oop) -> PrimResult<(*const u8, usize)> {
        if !self.is_bytes(oop)? {
            return Err(PrimErr::BadArgument);
        }
        let len = usize::try_from(self.byte_size_of(oop)?)?;
        let ptr = call!(self, firstIndexableField(oop.0));
        if ptr.is_null() {
            return Err(PrimErr::BadArgument);
        }
        Ok((ptr.cast::<u8>(), len))
    }

    /// Reads instance variable `index` of a pointer object.
    ///
    /// Used to reach into image-side objects a primitive is handed, such as a
    /// `Form`'s bits, width, height and depth.
    pub fn fetch_pointer(&self, index: sqInt, oop: Oop) -> PrimResult<Oop> {
        Ok(Oop(call!(self, fetchPointerofObject(index, oop.0))))
    }

    /// Reads instance variable `index` of a pointer object as an integer.
    pub fn fetch_integer(&self, index: sqInt, oop: Oop) -> PrimResult<sqInt> {
        let v = call!(self, fetchIntegerofObject(index, oop.0));
        self.check_failed()?;
        Ok(v)
    }

    /// Is `oop` an instance of the named class, or of a subclass of it?
    ///
    /// Matches by class name, as the VM's own `isKindOf` does, so it does not
    /// need the class to be a well-known object.
    pub fn is_kind_of_named(&self, oop: Oop, class_name: &str) -> PrimResult<bool> {
        let class_name = CString::new(class_name).map_err(|_| PrimErr::BadArgument)?;
        Ok(call!(self, isKindOf(oop.0, class_name.as_ptr().cast_mut())) != 0)
    }

    /// Is this a word- or byte-indexable object (Bitmap, ByteArray, ...)?
    pub fn is_words_or_bytes(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isWordsOrBytes(oop.0)) != 0)
    }

    /// Unwraps a Smalltalk boolean.
    pub fn boolean_value(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, booleanValueOf(oop.0)) != 0)
    }

    /// The contents of a word-indexable object as native-endian 32-bit words.
    ///
    /// Fails with `BadArgument` if `oop` is not word- or byte-indexable, or if
    /// its size is not a whole number of words.
    ///
    /// Same borrow caveat as [`Interp::bytes_of`]: valid only until the next
    /// allocation.
    pub fn words_of(&self, oop: Oop) -> PrimResult<&[u32]> {
        if !self.is_words_or_bytes(oop)? {
            return Err(PrimErr::BadArgument);
        }
        let byte_len = usize::try_from(self.byte_size_of(oop)?)?;
        if byte_len % 4 != 0 {
            return Err(PrimErr::BadArgument);
        }
        let base = call!(self, firstIndexableField(oop.0));
        if base.is_null() {
            return Err(PrimErr::BadArgument);
        }
        // SAFETY: `byte_len` bytes belong to the object, the length is a whole
        // number of words, and the image guarantees word alignment for word
        // objects. Nothing moves it for the duration of the borrow.
        Ok(unsafe { slice::from_raw_parts(base.cast::<u32>(), byte_len / 4) })
    }

    /// Writes native-endian 32-bit words into a word-indexable object,
    /// starting `word_offset` words in.
    ///
    /// This is the shape image bitmaps come in: a `Bitmap` is words, not bytes,
    /// so [`Interp::write_bytes`] rejects it.
    ///
    /// Fails with `BadArgument` if `oop` is not word- or byte-indexable,
    /// `BadIndex` if the write would run past the end, and `NoModification` if
    /// it is immutable.
    pub fn write_words(&self, oop: Oop, word_offset: usize, src: &[u32]) -> PrimResult<()> {
        if call!(self, isOopImmutable(oop.0)) != 0 {
            return Err(PrimErr::NoModification);
        }
        if !self.is_words_or_bytes(oop)? {
            return Err(PrimErr::BadArgument);
        }
        let byte_len = usize::try_from(self.byte_size_of(oop)?)?;
        let byte_offset = word_offset.checked_mul(4).ok_or(PrimErr::BadIndex)?;
        let byte_span = src.len().checked_mul(4).ok_or(PrimErr::BadIndex)?;
        let end = byte_offset
            .checked_add(byte_span)
            .ok_or(PrimErr::BadIndex)?;
        if end > byte_len {
            return Err(PrimErr::BadIndex);
        }
        let base = call!(self, firstIndexableField(oop.0));
        if base.is_null() {
            return Err(PrimErr::BadArgument);
        }
        // SAFETY: the destination lies wholly inside the object (checked just
        // above) and the object is mutable. Written a word at a time through
        // `write_unaligned` because the image only guarantees word alignment
        // for word objects, and `src` is a Rust slice that cannot overlap.
        let dst = base.cast::<u8>().wrapping_add(byte_offset).cast::<u32>();
        for (i, w) in src.iter().enumerate() {
            unsafe { dst.add(i).write_unaligned(*w) };
        }
        Ok(())
    }

    // ---- converting --------------------------------------------------------

    /// Boxes an integer as a SmallInteger.
    ///
    /// Tags without checking range, matching the proxy. Prefer
    /// [`Interp::integer_checked`] unless you have already established that
    /// `value` fits.
    pub fn integer(&self, value: sqInt) -> PrimResult<Oop> {
        Ok(Oop(call!(self, integerObjectOf(value))))
    }

    /// Unboxes a SmallInteger.
    pub fn integer_value(&self, oop: Oop) -> PrimResult<sqInt> {
        if !self.is_integer_object(oop)? {
            return Err(PrimErr::BadArgument);
        }
        Ok(call!(self, integerValueOf(oop.0)))
    }

    /// Boxes a float as an oop.
    pub fn float(&self, value: f64) -> PrimResult<Oop> {
        Ok(Oop(call!(self, floatObjectOf(value))))
    }

    /// Makes a Smalltalk String from a Rust string.
    ///
    /// Fails with `BadArgument` if `s` contains an interior NUL, since the
    /// proxy takes a C string.
    pub fn string(&self, s: &str) -> PrimResult<Oop> {
        let s = CString::new(s).map_err(|_| PrimErr::BadArgument)?;
        let oop = call!(self, stringForCString(s.as_ptr()));
        Ok(Oop(oop))
    }

    // ---- well-known objects ------------------------------------------------

    /// `nil`.
    pub fn nil(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, nilObject())))
    }

    /// `true`.
    pub fn true_object(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, trueObject())))
    }

    /// `false`.
    pub fn false_object(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, falseObject())))
    }

    /// The class `ByteArray`.
    pub fn class_byte_array(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, classByteArray())))
    }

    /// The class `Array`.
    pub fn class_array(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, classArray())))
    }

    /// The class `String`.
    pub fn class_string(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, classString())))
    }

    // ---- allocation --------------------------------------------------------

    /// Instantiates `class` with `size` indexable slots.
    ///
    /// Fails with `NoMemory` rather than triggering a collection: in Spur no
    /// allocation runs the GC, so the image decides when to collect.
    pub fn instantiate(&self, class: Oop, size: sqInt) -> PrimResult<Oop> {
        let oop = call!(self, instantiateClassindexableSize(class.0, size));
        if oop == 0 {
            return Err(PrimErr::NoMemory);
        }
        Ok(Oop(oop))
    }

    // ---- answering ---------------------------------------------------------

    /// Answers `oop` from the current primitive.
    pub fn return_value(&self, oop: Oop) -> PrimResult<()> {
        call!(self, methodReturnValue(oop.0));
        Ok(())
    }

    /// Answers an integer, as a SmallInteger or a LargeInteger as needed.
    ///
    /// The proxy's own `methodReturnInteger` tags without checking range --
    /// it is literally `(value << 3) | 1` -- so handing it anything outside
    /// [`MIN_SMALL_INTEGER`]..=[`MAX_SMALL_INTEGER`] silently wraps and the
    /// image sees a nonsense number. Values outside the range are boxed as
    /// LargeIntegers here instead.
    pub fn return_integer(&self, value: sqInt) -> PrimResult<()> {
        if (MIN_SMALL_INTEGER..=MAX_SMALL_INTEGER).contains(&value) {
            call!(self, methodReturnInteger(value));
        } else {
            let oop = call!(self, signed64BitIntegerFor(value as i64));
            if oop == 0 {
                return Err(PrimErr::NoMemory);
            }
            call!(self, methodReturnValue(oop));
        }
        Ok(())
    }

    /// Boxes an integer as an oop, widening to a LargeInteger when it does not
    /// fit in a SmallInteger.
    ///
    /// The raw `integerObjectOf` has the same unchecked-tagging behaviour as
    /// `methodReturnInteger`; see [`Interp::return_integer`].
    pub fn integer_checked(&self, value: sqInt) -> PrimResult<Oop> {
        if (MIN_SMALL_INTEGER..=MAX_SMALL_INTEGER).contains(&value) {
            return self.integer(value);
        }
        let oop = call!(self, signed64BitIntegerFor(value as i64));
        if oop == 0 {
            return Err(PrimErr::NoMemory);
        }
        Ok(Oop(oop))
    }

    /// Answers a boolean.
    pub fn return_bool(&self, value: bool) -> PrimResult<()> {
        call!(self, methodReturnBool(sqInt::from(value)));
        Ok(())
    }

    /// Answers a float.
    pub fn return_float(&self, value: f64) -> PrimResult<()> {
        call!(self, methodReturnFloat(value));
        Ok(())
    }

    /// Answers the receiver, which is what a primitive answers by default.
    pub fn return_receiver(&self) -> PrimResult<()> {
        call!(self, methodReturnReceiver());
        Ok(())
    }

    // ---- failing -----------------------------------------------------------

    /// Fails the current primitive with `code`.
    ///
    /// Returning `Err` from a primitive body does this for you; call it
    /// directly only when you need to fail without unwinding out of a helper.
    pub fn fail_for(&self, code: PrimErr) {
        if let Some(f) = call_opt!(self, primitiveFailFor(code.code())) {
            let _ = f;
        }
    }
}

/// A single primitive argument, read from a stack slot.
///
/// Implemented for the handful of shapes a stack slot can be asked to take;
/// [`StackArgs`] assembles these into whole argument lists.
pub trait StackArg: Sized {
    /// Reads the value `offset` slots down the stack.
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self>;
}

impl StackArg for Oop {
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self> {
        vm.stack_value(offset)
    }
}

impl StackArg for sqInt {
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self> {
        vm.stack_integer(offset)
    }
}

impl StackArg for bool {
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self> {
        let oop = vm.stack_value(offset)?;
        vm.boolean_value(oop)
    }
}

impl StackArg for f64 {
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self> {
        vm.stack_float(offset)
    }
}

/// A primitive's entire argument list, read and checked in one go.
///
/// Implemented for tuples of [`StackArg`] up to arity 8. Reading first checks
/// the argument count against the arity, then fetches each element from its
/// slot: argument `i` of `n` sits `n - 1 - i` slots down, because the last
/// argument is on top. Obtain one through [`Interp::args`].
pub trait StackArgs: Sized {
    /// Checks the argument count and reads every argument.
    fn from_stack_args(vm: &Interp) -> PrimResult<Self>;
}

macro_rules! impl_stack_args {
    ($n:literal: $($ty:ident @ $offset:literal),+) => {
        impl<$($ty: StackArg),+> StackArgs for ($($ty,)+) {
            fn from_stack_args(vm: &Interp) -> PrimResult<Self> {
                vm.expect_argument_count($n)?;
                Ok(($($ty::from_stack(vm, $offset)?,)+))
            }
        }
    };
}

impl_stack_args!(1: A @ 0);
impl_stack_args!(2: A @ 1, B @ 0);
impl_stack_args!(3: A @ 2, B @ 1, C @ 0);
impl_stack_args!(4: A @ 3, B @ 2, C @ 1, D @ 0);
impl_stack_args!(5: A @ 4, B @ 3, C @ 2, D @ 1, E @ 0);
impl_stack_args!(6: A @ 5, B @ 4, C @ 3, D @ 2, E @ 1, F @ 0);
impl_stack_args!(7: A @ 6, B @ 5, C @ 4, D @ 3, E @ 2, F @ 1, G @ 0);
impl_stack_args!(8: A @ 7, B @ 6, C @ 5, D @ 4, E @ 3, F @ 2, G @ 1, H @ 0);
