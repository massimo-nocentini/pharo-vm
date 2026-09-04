//! A safe handle on the interpreter proxy.

use core::ffi::{c_char, c_void};
use core::slice;
use std::ffi::CString;

use crate::error::{PrimErr, PrimResult};
use crate::handles::{Handle, Resource};
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

/// The byte ranges of image memory currently lent out as `&mut`.
///
/// [`Interp::with_bytes_mut`] and [`Interp::with_words_mut`] hand a caller a
/// mutable slice into an object it was passed; every other route into those
/// same bytes -- a second in-place view, `bytes_of`, `words_of`,
/// `write_bytes`, `write_words` -- has to refuse while that slice is alive,
/// or two live paths to one byte alias each other and Rust's rules are broken
/// even though the C API permits it. The image can pass one object as two
/// arguments, so this is a case that arrives from outside, not a mistake a
/// plugin author can be told not to make.
///
/// The VM runs primitives on one thread, and only a handful of views can be
/// live at once, so a thread-local table of ranges is the whole bookkeeping.
mod lent {
    use core::cell::Cell;

    /// How many in-place views may be live at once. A primitive nests a
    /// source and a destination; the table has room to spare.
    const CAPACITY: usize = 8;

    thread_local! {
        /// `(start, end)` of each lent range, `(0, 0)` for a free slot.
        static REGIONS: [Cell<(usize, usize)>; CAPACITY] =
            const { [const { Cell::new((0usize, 0usize)) }; CAPACITY] };
    }

    /// A lent range, released when dropped -- on the panic path too, since a
    /// primitive body unwinds into the macro's `catch_unwind`.
    pub struct Lease(usize);

    impl Drop for Lease {
        fn drop(&mut self) {
            let slot = self.0;
            REGIONS.with(|regions| regions[slot].set((0, 0)));
        }
    }

    /// Records `[start, start + len)` as lent, or answers `None` when the
    /// table is full.
    ///
    /// An empty range is recorded as a free slot would be, which is right:
    /// nothing can overlap it.
    pub fn claim(start: usize, len: usize) -> Option<Lease> {
        let end = start.checked_add(len)?;
        REGIONS.with(|regions| {
            let free = regions.iter().position(|r| r.get() == (0, 0))?;
            regions[free].set((start, end));
            Some(Lease(free))
        })
    }

    /// Does `[start, start + len)` touch anything currently lent out?
    pub fn overlaps(start: usize, len: usize) -> bool {
        let end = start.saturating_add(len);
        REGIONS.with(|regions| {
            regions
                .iter()
                .map(Cell::get)
                .any(|(lo, hi)| start < hi && lo < end)
        })
    }
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

    /// The interpreter this process's plugin was handed, if it has one yet.
    ///
    /// Primitives are given an `Interp` and should use that. This is for the
    /// module hooks -- `initialiseModule`, `shutdownModule` -- which the VM
    /// calls outside any primitive and hands nothing: a plugin that has to
    /// release image resources on the way out has no other way to reach the
    /// proxy. Answers `None` before `setInterpreter` has run.
    #[must_use]
    pub fn current() -> Option<Self> {
        let vt = crate::__private::INTERP.load(core::sync::atomic::Ordering::Acquire);
        if vt.is_null() {
            return None;
        }
        // SAFETY: set_interpreter only ever stores the VM's own proxy table,
        // which lives as long as the process.
        Some(unsafe { Self::from_raw(vt) })
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

    /// Runs `f` over an object's bytes where they lie.
    ///
    /// The write path's counterpart to [`Interp::bytes_of`], for a primitive
    /// whose destination is an object it was handed: a staging buffer plus
    /// [`Interp::write_bytes`] costs an allocation and a second pass over
    /// every byte, and neither buys anything when the caller is going to fill
    /// the object anyway.
    ///
    /// This is the `&mut [u8]` `write_bytes` refuses to hand out, made
    /// answerable rather than avoided: the slice is scoped to the closure,
    /// and while it is live every other route into the same bytes --
    /// `bytes_of`, `words_of`, `write_bytes`, `write_words`, another in-place
    /// view -- fails with `Inappropriate`. The image can pass one object as
    /// two arguments, so that is a case a primitive meets from outside, and
    /// it now fails cleanly instead of aliasing.
    ///
    /// Fails with `BadArgument` if `oop` is not byte-indexable,
    /// `NoModification` if it is immutable, and `LimitExceeded` if more
    /// in-place views are live than the SDK tracks (eight).
    ///
    /// The same borrow caveat as `bytes_of` applies inside the closure: the
    /// slice is valid only until the next allocation, so do not allocate
    /// through the VM while holding it.
    pub fn with_bytes_mut<R>(&self, oop: Oop, f: impl FnOnce(&mut [u8]) -> R) -> PrimResult<R> {
        if call!(self, isOopImmutable(oop.0)) != 0 {
            return Err(PrimErr::NoModification);
        }
        // Answers Inappropriate if this object is already lent out.
        let (ptr, len) = self.byte_region(oop)?;
        let _lease = lent::claim(ptr as usize, len).ok_or(PrimErr::LimitExceeded)?;
        // SAFETY: `len` writable bytes belong to the object, it is mutable,
        // nothing moves it while the closure runs, and the lease makes every
        // other view of those bytes fail until it is dropped -- so this is
        // the only live reference to them.
        let slice = unsafe { slice::from_raw_parts_mut(ptr.cast_mut(), len) };
        Ok(f(slice))
    }

    /// Runs `f` over an object's words where they lie.
    ///
    /// [`Interp::with_bytes_mut`] for the shape image bitmaps come in; the
    /// same scoping, the same failures, and `BadArgument` when the object is
    /// not word-indexable or its size is not a whole number of words.
    pub fn with_words_mut<R>(&self, oop: Oop, f: impl FnOnce(&mut [u32]) -> R) -> PrimResult<R> {
        if call!(self, isOopImmutable(oop.0)) != 0 {
            return Err(PrimErr::NoModification);
        }
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
        if lent::overlaps(base as usize, byte_len) {
            return Err(PrimErr::Inappropriate);
        }
        let _lease = lent::claim(base as usize, byte_len).ok_or(PrimErr::LimitExceeded)?;
        // SAFETY: as in `with_bytes_mut`, with `words_of`'s alignment
        // argument: the image gives word objects word alignment.
        let slice = unsafe { slice::from_raw_parts_mut(base.cast::<u32>(), byte_len / 4) };
        Ok(f(slice))
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
        if lent::overlaps(ptr as usize, len) {
            return Err(PrimErr::Inappropriate);
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
        if lent::overlaps(base as usize, byte_len) {
            return Err(PrimErr::Inappropriate);
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
        if lent::overlaps(base as usize + byte_offset, byte_span) {
            return Err(PrimErr::Inappropriate);
        }
        // SAFETY: the destination lies wholly inside the object (checked just
        // above) and the object is mutable. Copied as bytes, which asks
        // nothing of the destination's alignment -- the image only guarantees
        // word alignment for word objects -- and `src` is a Rust slice that
        // cannot overlap it. A native-endian `[u32]` in memory already is the
        // byte sequence the object wants, so this is a `memcpy`, not a
        // conversion.
        let dst = base.cast::<u8>().wrapping_add(byte_offset);
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr().cast::<u8>(), dst, byte_span);
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
    /// proxy takes a C string, and with `NoMemory` if the image cannot hold
    /// the String: `stringForCString:` allocates, and answers nil -- 0 in C --
    /// when the allocation fails
    /// (`SpurMemoryManager>>#stringForCString:`,
    /// `smalltalksrc/VMMaker/SpurMemoryManager.class.st:12944-12961`), the
    /// same convention [`Interp::instantiate`] follows. The proxy's own
    /// string-answering entry tests for it and raises `PrimErrNoMemory`
    /// (`InterpreterProxy>>#methodReturnString:`,
    /// `smalltalksrc/VMMaker/InterpreterProxy.class.st:687-695`), as does the
    /// JIT (`smalltalksrc/VMMaker/Cogit.class.st:4339`). It is only the raw
    /// proxy call that hands a plugin a bare 0.
    pub fn string(&self, s: &str) -> PrimResult<Oop> {
        let s = CString::new(s).map_err(|_| PrimErr::BadArgument)?;
        let oop = call!(self, stringForCString(s.as_ptr()));
        if oop == 0 {
            return Err(PrimErr::NoMemory);
        }
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

    // ---- pinning -----------------------------------------------------------

    /// Pins an object so the garbage collector will not move it.
    ///
    /// Needed before handing the address of an image object to a foreign
    /// library that will keep it: a `cairo_surface_t` created over a Bitmap's
    /// words, say, outlives the primitive that made it, and Spur's collector
    /// is free to move an unpinned object at any allocation.
    ///
    /// Answers the oop, which may differ from the argument: pinning an object
    /// that is not already in old space moves it there first.
    pub fn pin_object(&self, oop: Oop) -> PrimResult<Oop> {
        let pinned = call!(self, pinObject(oop.0));
        if pinned == 0 {
            return Err(PrimErr::ObjectMayMove);
        }
        Ok(Oop(pinned))
    }

    /// Releases a pin taken by [`Interp::pin_object`].
    pub fn unpin_object(&self, oop: Oop) -> PrimResult<()> {
        call!(self, unpinObject(oop.0));
        Ok(())
    }

    /// Is this object pinned?
    pub fn is_pinned(&self, oop: Oop) -> PrimResult<bool> {
        Ok(call!(self, isPinned(oop.0)) != 0)
    }

    /// The address of an indexable object's first element.
    ///
    /// The escape hatch for foreign libraries that write into image memory
    /// directly. Everything the safe accessors guarantee is now the caller's
    /// job -- above all that the object stays put, which means
    /// [`Interp::pin_object`] first if the pointer outlives the primitive.
    ///
    /// Answers the pointer and the object's size in bytes.
    pub fn indexable_bytes_ptr(&self, oop: Oop) -> PrimResult<(*mut u8, usize)> {
        if !self.is_words_or_bytes(oop)? {
            return Err(PrimErr::BadArgument);
        }
        let len = usize::try_from(self.byte_size_of(oop)?)?;
        let ptr = call!(self, firstIndexableField(oop.0));
        if ptr.is_null() {
            return Err(PrimErr::BadArgument);
        }
        if lent::overlaps(ptr as usize, len) {
            return Err(PrimErr::Inappropriate);
        }
        Ok((ptr.cast::<u8>(), len))
    }

    // ---- doubles in and out of byte objects --------------------------------

    /// Reads `n` native-endian `f64`s from a byte object.
    ///
    /// How a foreign struct of doubles -- a `cairo_matrix_t`, a set of extents
    /// -- reaches a primitive: the image passes a ByteArray of the right size
    /// rather than a pointer, so nothing crosses the boundary unchecked.
    ///
    /// Fails with `BadArgument` unless the object holds exactly `n` doubles.
    pub fn read_f64s(&self, oop: Oop, n: usize) -> PrimResult<Vec<f64>> {
        let bytes = self.bytes_of(oop)?;
        let want = n.checked_mul(8).ok_or(PrimErr::BadArgument)?;
        if bytes.len() != want {
            return Err(PrimErr::BadArgument);
        }
        Ok(bytes
            .chunks_exact(8)
            .map(|c| f64::from_ne_bytes(c.try_into().expect("chunks_exact(8) yields 8 bytes")))
            .collect())
    }

    /// Reads exactly `N` native-endian `f64`s from a byte object.
    ///
    /// [`Interp::read_f64s`] for the fixed-size shapes -- a point, a
    /// `cairo_matrix_t`, a set of extents -- which is nearly all of them.
    /// The count is known at compile time, so the answer is an array on the
    /// stack and the call costs no allocation.
    ///
    /// Fails with `BadArgument` unless the object holds exactly `N` doubles.
    pub fn read_f64_array<const N: usize>(&self, oop: Oop) -> PrimResult<[f64; N]> {
        let bytes = self.bytes_of(oop)?;
        let want = N.checked_mul(8).ok_or(PrimErr::BadArgument)?;
        if bytes.len() != want {
            return Err(PrimErr::BadArgument);
        }
        let mut values = [0.0f64; N];
        for (dst, chunk) in values.iter_mut().zip(bytes.chunks_exact(8)) {
            *dst = f64::from_ne_bytes(chunk.try_into().expect("chunks_exact(8) yields 8 bytes"));
        }
        Ok(values)
    }

    /// Writes native-endian `f64`s into a byte object, filling it exactly.
    ///
    /// The counterpart of [`Interp::read_f64s`], for the out-parameter shape:
    /// the image hands in a ByteArray and reads its doubles back afterwards.
    ///
    /// Fails with `BadArgument` if the object is not exactly `values.len()`
    /// doubles long, so a caller who sized the buffer wrongly finds out.
    pub fn write_f64s(&self, oop: Oop, values: &[f64]) -> PrimResult<()> {
        let want = values.len().checked_mul(8).ok_or(PrimErr::BadArgument)?;
        if usize::try_from(self.byte_size_of(oop)?)? != want {
            return Err(PrimErr::BadArgument);
        }
        self.with_bytes_mut(oop, |dst| {
            for (chunk, v) in dst.chunks_exact_mut(8).zip(values) {
                chunk.copy_from_slice(&v.to_ne_bytes());
            }
        })
    }

    // ---- strings -----------------------------------------------------------

    /// Reads a Smalltalk String or Symbol as Rust text.
    ///
    /// ByteString is a byte-indexable object whose contents the image treats
    /// as Latin-1 unless it knows better, and modern Pharo puts UTF-8 in one
    /// routinely. Decoded as UTF-8 when it is valid UTF-8, and as Latin-1
    /// otherwise -- which is lossless both ways round and never fails, so a
    /// primitive cannot be made to reject a filename it merely cannot name.
    pub fn string_value(&self, oop: Oop) -> PrimResult<String> {
        let bytes = self.bytes_of(oop)?;
        Ok(match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        })
    }

    /// Reads a Smalltalk String or Symbol as a NUL-terminated C string.
    ///
    /// What a primitive that hands a name to a C library wants, and one copy
    /// rather than the two `CString::new(vm.string_value(oop)?)` makes: the
    /// bytes go straight from the object into the `CString`, with no `String`
    /// in between. Nothing is decoded on the way, which is what the library
    /// on the other side expects anyway.
    ///
    /// Fails with `BadArgument` if the text contains an interior NUL, which
    /// no C string can carry.
    pub fn c_string_value(&self, oop: Oop) -> PrimResult<CString> {
        CString::new(self.bytes_of(oop)?).map_err(|_| PrimErr::BadArgument)
    }

    /// Reads the argument at `offset` as Rust text.
    pub fn stack_string(&self, offset: sqInt) -> PrimResult<String> {
        let oop = self.stack_value(offset)?;
        self.string_value(oop)
    }

    // ---- more well-known objects and constructors --------------------------

    /// The class `Bitmap`.
    pub fn class_bitmap(&self) -> PrimResult<Oop> {
        Ok(Oop(call!(self, classBitmap())))
    }

    /// Is this `nil`?
    pub fn is_nil(&self, oop: Oop) -> PrimResult<bool> {
        Ok(oop == self.nil()?)
    }

    /// Makes a `Point`.
    pub fn point(&self, x: sqInt, y: sqInt) -> PrimResult<Oop> {
        Ok(Oop(call!(self, makePointwithxValueyValue(x, y))))
    }

    /// Stores into instance variable `index` of a pointer object.
    pub fn store_pointer(&self, index: sqInt, oop: Oop, value: Oop) -> PrimResult<()> {
        call!(self, storePointerofObjectwithValue(index, oop.0, value.0));
        self.check_failed()
    }

    // ---- semaphores --------------------------------------------------------

    /// Signals the external semaphore registered at `index`.
    ///
    /// How a plugin wakes an image-side process: the image registers a
    /// Semaphore in its external-objects array and passes the index in, and
    /// the plugin signals it when there is something to collect.
    pub fn signal_semaphore(&self, index: sqInt) -> PrimResult<()> {
        call!(self, signalSemaphoreWithIndex(index));
        Ok(())
    }

    // ---- this run ----------------------------------------------------------

    /// The VM's session id: a value that identifies *this run of this process*.
    ///
    /// `globalSessionID` is a VM global rather than image state, set once from
    /// `time(NULL) + ioMSecs()` while the image is being read
    /// (`StackInterpreter >> initializeInterpreterFromHeader:withBytes:`), and
    /// never zero. So it changes when a snapshot is resumed in a new process
    /// and does *not* change when an image snapshots and keeps running -- which
    /// is exactly the distinction a handle the image saved in an inst var needs
    /// drawn. [`crate::handles`] spends a byte of every handle on it.
    ///
    /// Fails with `Unsupported` on a VM whose proxy leaves the entry unset.
    pub fn session_id(&self) -> PrimResult<sqInt> {
        Ok(call!(self, getThisSessionID()))
    }

    // ---- other plugins -----------------------------------------------------

    /// Looks a function up in another plugin, loading that plugin if needed.
    ///
    /// The VM's own sanctioned way for one plugin to reach another. Two
    /// separately-loaded shared libraries cannot share a `static`, so a plugin
    /// that needs a resource another plugin owns -- a `cairo_t *` living in
    /// CairoPlugin's registry, say -- has to ask for it through an exported C
    /// entry point, and this is how that entry point is found. Linking the
    /// other plugin as an rlib instead would compile its code a second time
    /// and give this library a second, empty copy of its registries.
    ///
    /// `ioLoadFunctionFrom` (`src/common/sqNamedPrims.c:319`) **loads the
    /// module** when it is not loaded yet: a filesystem search over every
    /// plugin path, followed by `getModuleName`/`setInterpreter`/
    /// `initialiseModule`, with the library `dlclose`d again if any of those
    /// declines. So this is not a cheap call, and a plugin should resolve once
    /// and cache -- including caching the *failure*, or a miss walks the
    /// filesystem again on every attempt.
    ///
    /// The name is looked up literally with `dlsym`, with no module prefix and
    /// no `AccessorDepth` byte, so the other plugin must export exactly this
    /// symbol.
    ///
    /// Fails with `BadArgument` for an empty name or one containing an interior
    /// NUL, and `NotFound` when either the module or the symbol is missing --
    /// which the caller cannot tell apart, by design of the C entry point. Use
    /// [`Interp::module_is_loadable`] first if the difference matters.
    ///
    /// The answer stays valid only until the module is unloaded. The image can
    /// unload one (`ioUnloadModule`, `src/common/sqNamedPrims.c:487`), which
    /// `dlclose`s the library and dangles every pointer taken out of it. A
    /// plugin that caches one of these **must** export `moduleUnloaded` and
    /// drop its cache when named the module it borrowed from.
    ///
    /// # Safety of the result
    ///
    /// The answer is a code address the caller will transmute to a function
    /// pointer. Nothing checks the signature; getting it wrong is exactly as
    /// dangerous as getting a `dlsym` signature wrong, because it is one.
    pub fn load_function_from(&self, function: &str, module: &str) -> PrimResult<*mut c_void> {
        // An empty name is not merely useless: `ioLoadFunctionFrom` tests the
        // *pointer* for NULL (`sqNamedPrims.c:329`), so an empty CString takes
        // the string path and `ioFindExternalFunctionInAccessorDepthInto`
        // answers 0 for it -- while a genuine NULL would answer the constant
        // 1, which is not an address at all. Refusing here keeps `(void *) 1`
        // out of the return type.
        if function.is_empty() || module.is_empty() {
            return Err(PrimErr::BadArgument);
        }
        // The proxy takes `char *`, not `const char *`, so the bytes must live
        // somewhere writable for the call. Bound to locals: a temporary would
        // be dropped before `call!` ran, leaving the proxy reading freed
        // memory.
        let mut function = CString::new(function)
            .map_err(|_| PrimErr::BadArgument)?
            .into_bytes_with_nul();
        let mut module = CString::new(module)
            .map_err(|_| PrimErr::BadArgument)?
            .into_bytes_with_nul();
        let address = call!(
            self,
            ioLoadFunctionFrom(
                function.as_mut_ptr().cast::<c_char>(),
                module.as_mut_ptr().cast::<c_char>(),
            )
        );
        if address.is_null() {
            return Err(PrimErr::NotFound);
        }
        Ok(address)
    }

    /// Is this plugin loadable? Loads it, if it is not loaded already.
    ///
    /// `ioLoadFunctionFrom` with a null function name answers the constant 1
    /// when the module is there and 0 when it is not
    /// (`src/common/sqNamedPrims.c:329-332`). Note that "there" includes having
    /// initialised successfully: a plugin whose `initialiseModule` declined has
    /// been unloaded again by the time this answers.
    ///
    /// Distinguishes "no such plugin" from "no such symbol", which
    /// [`Interp::load_function_from`] cannot -- worth asking when the two want
    /// different diagnostics, and not worth asking otherwise, because it is the
    /// same filesystem search.
    pub fn module_is_loadable(&self, module: &str) -> PrimResult<bool> {
        if module.is_empty() {
            return Err(PrimErr::BadArgument);
        }
        let mut module = CString::new(module)
            .map_err(|_| PrimErr::BadArgument)?
            .into_bytes_with_nul();
        // A literal null, not an empty string: the C tests the pointer, and
        // this is the only way to reach the "module is there" answer
        // deliberately.
        let answer = call!(
            self,
            ioLoadFunctionFrom(core::ptr::null_mut(), module.as_mut_ptr().cast::<c_char>())
        );
        Ok(!answer.is_null())
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

/// A typed resource handle, checked while the stack slot is read.
///
/// This is what lets a primitive be written
/// `fn primitiveSetSource(vm: &Interp, cr: Handle<Context>, pat: Handle<Pattern>)`
/// and get `BadArgument` for a handle of the wrong kind and `NotFound` for a
/// dead one -- both decided here, before any registry is even locked.
/// CairoPlugin's `primitiveSetSource`
/// (`rust/plugins/cairo-plugin/src/context.rs`) is written exactly that way and
/// is the call site this impl ships behind; every other primitive in the tree
/// takes a bare `sqInt` and decodes inside its crate's `with_*` accessor, which
/// keeps `Handle::decode` to one place per crate. Both shapes are supported on
/// purpose -- this one moves the kind check into the signature, that one keeps
/// the seam greppable.
impl<R: Resource> StackArg for Handle<R> {
    fn from_stack(vm: &Interp, offset: sqInt) -> PrimResult<Self> {
        Handle::decode(vm.stack_integer(offset)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::VirtualMachine;

    /// A proxy table with every entry unset, which is what an old or
    /// differently-configured VM hands over and what a test binary has instead
    /// of a VM at all. Only the checks that run *before* the proxy call are
    /// exercisable against it -- but those are the ones carrying the trap.
    fn interp_without_a_vm() -> (Box<VirtualMachine>, Interp) {
        // SAFETY: every field of `VirtualMachine` is an `Option<fn>`, whose
        // all-zero bit pattern is the guaranteed niche for `None`. Nothing
        // here is ever called through.
        let mut vt: Box<VirtualMachine> = Box::new(unsafe { core::mem::zeroed() });
        // SAFETY: the box outlives the `Interp`, both are returned together,
        // and the table is never mutated after this point.
        let interp = unsafe { Interp::from_raw(core::ptr::from_mut(&mut *vt)) };
        (vt, interp)
    }

    #[test]
    fn an_empty_function_name_is_refused_before_it_can_answer_the_constant_one() {
        let (_vt, vm) = interp_without_a_vm();
        assert_eq!(
            vm.load_function_from("", "CairoPlugin"),
            Err(PrimErr::BadArgument)
        );
    }

    #[test]
    fn an_empty_module_name_is_refused_by_both_entry_points() {
        let (_vt, vm) = interp_without_a_vm();
        assert_eq!(
            vm.load_function_from("cairoPluginBridgeAbiVersion", ""),
            Err(PrimErr::BadArgument)
        );
        assert_eq!(vm.module_is_loadable(""), Err(PrimErr::BadArgument));
    }

    #[test]
    fn an_interior_nul_cannot_be_smuggled_through_as_a_shorter_name() {
        let (_vt, vm) = interp_without_a_vm();
        assert_eq!(
            vm.load_function_from("cairo\0Plugin", "CairoPlugin"),
            Err(PrimErr::BadArgument)
        );
        assert_eq!(
            vm.module_is_loadable("Cairo\0Plugin"),
            Err(PrimErr::BadArgument)
        );
    }

    #[test]
    fn a_vm_without_the_entry_point_declines_rather_than_calling_through_null() {
        let (_vt, vm) = interp_without_a_vm();
        assert_eq!(
            vm.load_function_from("cairoPluginBridgeAbiVersion", "CairoPlugin"),
            Err(PrimErr::Unsupported)
        );
        assert_eq!(vm.module_is_loadable("CairoPlugin"), Err(PrimErr::Unsupported));
    }
}
