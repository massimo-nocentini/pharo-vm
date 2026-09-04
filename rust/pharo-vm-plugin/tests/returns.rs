//! What [`IntoReturn`] hands back, driven through a fake proxy table.
//!
//! No VM is running here, so the entries the answering impls need --
//! `classByteArray`, `instantiateClassindexableSize`, `stringForCString`,
//! `nilObject`, the `methodReturn*` family, and the four accessors
//! `write_bytes` consults --
//! are answered from a Rust-side heap whose objects are leaked, so their
//! addresses behave like image objects that never move. What is under test is
//! the answer: which proxy call the impl reaches for, which class it
//! instantiated, what the object it built actually contains -- and what
//! happens when the allocation fails, which is the case a running VM will not
//! reproduce on demand.
//!
//! Everything the fake proxy observes is *recorded*, never asserted in place:
//! these entries are `extern "C"`, and since Rust 1.71 a panic unwinding out
//! of a non-`-unwind` `extern "C"` function aborts the process. An assertion
//! inside one is not a test failure, it is a dead harness, so the assertions
//! all live in the test bodies.

use std::cell::{Cell, RefCell};

use pharo_vm_plugin::{sqInt, Interp, IntoReturn, Oop, PrimErr, VirtualMachine};

// ---------------------------------------------------------------------------
// A heap of byte objects, and a record of what was answered
// ---------------------------------------------------------------------------

/// The oop of `ByteArray` itself. Distinct from any object's oop, which is a
/// 1-based heap index, so a mix-up cannot pass unnoticed.
const CLASS_BYTE_ARRAY: sqInt = -1;
/// The oop of `nil`, likewise unmistakable for an object.
const NIL: sqInt = -2;

/// What the primitive answered, in the shape the proxy call carried it.
#[derive(Debug, Clone, PartialEq)]
enum Answer {
    Value(sqInt),
    Integer(sqInt),
    Bool(bool),
    Float(f64),
    Receiver,
}

thread_local! {
    /// Every leaked buffer, indexed by oop - 1.
    static HEAP: RefCell<Vec<&'static mut [u8]>> = const { RefCell::new(Vec::new()) };
    /// Everything answered since the last [`answers`] call.
    static ANSWERS: RefCell<Vec<Answer>> = const { RefCell::new(Vec::new()) };
    /// The class oop of every `instantiateClassindexableSize` since the last
    /// [`instantiations`] call, failed attempts included.
    static INSTANTIATED: RefCell<Vec<sqInt>> = const { RefCell::new(Vec::new()) };
    /// When set, `instantiateClassindexableSize` answers 0 -- the image out of
    /// memory, which is what makes a primitive fail with `PrimErrNoMemory`.
    static ALLOCATION_FAILS: Cell<bool> = const { Cell::new(false) };
}

fn with_obj<R>(oop: sqInt, f: impl FnOnce(&mut [u8]) -> R) -> R {
    HEAP.with(|heap| f(heap.borrow_mut()[oop as usize - 1]))
}

/// The object's current contents, read without going through the proxy.
fn contents(oop: Oop) -> Vec<u8> {
    with_obj(oop.0, |bytes| bytes.to_vec())
}

/// Drains the answers recorded so far.
fn answers() -> Vec<Answer> {
    ANSWERS.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

/// The single answer recorded so far, or a panic if there was not exactly one.
fn answer() -> Answer {
    let mut recorded = answers();
    assert_eq!(recorded.len(), 1, "expected exactly one answer");
    recorded.pop().expect("length checked just above")
}

fn record(a: Answer) {
    ANSWERS.with(|answers| answers.borrow_mut().push(a));
}

/// Drains the classes instantiated so far.
///
/// This is how the tests pin that a `Vec<u8>` is built as a ByteArray and not
/// as a String or an Array: the fake proxy only records the class, because an
/// assertion inside it could not be reported as a failure.
fn instantiations() -> Vec<sqInt> {
    INSTANTIATED.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

/// Runs `f` with the image out of memory, so every allocation answers 0.
fn with_allocation_failing<R>(f: impl FnOnce() -> R) -> R {
    ALLOCATION_FAILS.with(|failing| failing.set(true));
    let result = f();
    ALLOCATION_FAILS.with(|failing| failing.set(false));
    result
}

// ---------------------------------------------------------------------------
// The proxy entries the answering impls call
// ---------------------------------------------------------------------------

unsafe extern "C" fn class_byte_array() -> sqInt {
    CLASS_BYTE_ARRAY
}

unsafe extern "C" fn nil_object() -> sqInt {
    NIL
}

unsafe extern "C" fn instantiate(class: sqInt, size: sqInt) -> sqInt {
    INSTANTIATED.with(|seen| seen.borrow_mut().push(class));
    if ALLOCATION_FAILS.with(Cell::get) {
        // What `instantiateClassindexableSize` answers when the image cannot
        // find the room, and what `Interp::instantiate` turns into `NoMemory`.
        return 0;
    }
    let leaked: &'static mut [u8] = vec![0u8; size as usize].leak();
    HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.push(leaked);
        heap.len() as sqInt
    })
}

/// Allocates the String itself, the way the real one does -- so a `&str`
/// answer reaches the image without any `instantiateClassindexableSize` on the
/// way, and an out-of-memory image makes it answer 0.
unsafe extern "C" fn string_for_c_string(s: *const std::ffi::c_char) -> sqInt {
    if ALLOCATION_FAILS.with(Cell::get) {
        return 0;
    }
    // SAFETY: `Interp::string` passes a NUL-terminated `CString` it owns, and
    // it outlives this call.
    let bytes = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes().to_vec();
    let leaked: &'static mut [u8] = bytes.leak();
    HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.push(leaked);
        heap.len() as sqInt
    })
}

unsafe extern "C" fn is_bytes(_oop: sqInt) -> sqInt {
    1
}

unsafe extern "C" fn is_oop_immutable(_oop: sqInt) -> sqInt {
    0
}

unsafe extern "C" fn byte_size_of(oop: sqInt) -> sqInt {
    with_obj(oop, |bytes| bytes.len() as sqInt)
}

unsafe extern "C" fn first_indexable_field(oop: sqInt) -> *mut std::ffi::c_void {
    with_obj(oop, |bytes| bytes.as_mut_ptr().cast())
}

unsafe extern "C" fn method_return_value(oop: sqInt) -> sqInt {
    record(Answer::Value(oop));
    0
}

unsafe extern "C" fn method_return_integer(value: sqInt) -> sqInt {
    record(Answer::Integer(value));
    0
}

unsafe extern "C" fn method_return_bool(value: sqInt) -> sqInt {
    record(Answer::Bool(value != 0));
    0
}

unsafe extern "C" fn method_return_float(value: f64) -> sqInt {
    record(Answer::Float(value));
    0
}

unsafe extern "C" fn method_return_receiver() -> sqInt {
    record(Answer::Receiver);
    0
}

/// An `Interp` over a proxy table holding only those entries. Every other
/// field is `None`, which is what a plugin sees from a VM that does not
/// export it -- and what the impls under test never reach for.
fn interp() -> Interp {
    // SAFETY: `VirtualMachine` is function pointers all the way down, and a
    // zeroed `Option<fn>` is `None`.
    let mut vt: VirtualMachine = unsafe { std::mem::zeroed() };
    vt.classByteArray = Some(class_byte_array);
    vt.nilObject = Some(nil_object);
    vt.instantiateClassindexableSize = Some(instantiate);
    vt.stringForCString = Some(string_for_c_string);
    vt.isBytes = Some(is_bytes);
    vt.isOopImmutable = Some(is_oop_immutable);
    vt.byteSizeOf = Some(byte_size_of);
    vt.firstIndexableField = Some(first_indexable_field);
    vt.methodReturnValue = Some(method_return_value);
    vt.methodReturnInteger = Some(method_return_integer);
    vt.methodReturnBool = Some(method_return_bool);
    vt.methodReturnFloat = Some(method_return_float);
    vt.methodReturnReceiver = Some(method_return_receiver);
    // SAFETY: the table is leaked, so it lives as long as the process, which
    // is the contract `from_raw` asks for.
    unsafe { Interp::from_raw(Box::leak(Box::new(vt))) }
}

/// The oop of the object the last answer carried, or a panic if the answer
/// was not an object at all.
fn answered_object() -> Oop {
    match answer() {
        Answer::Value(oop) => Oop(oop),
        other => panic!("expected an object, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Bytes
// ---------------------------------------------------------------------------

#[test]
fn a_byte_vector_answers_a_byte_array_of_the_same_bytes() {
    let vm = interp();
    let bytes = vec![0, 1, 2, 250, 251, 255];
    bytes.clone().into_return(&vm).unwrap();
    let oop = answered_object();
    // ByteArray, not String and not Array: the class is what decides whether
    // the image sees `#[0 1 2 250 251 255]` or six characters.
    assert_eq!(instantiations(), vec![CLASS_BYTE_ARRAY]);
    assert_eq!(vm.byte_size_of(oop).unwrap(), bytes.len() as sqInt);
    assert_eq!(contents(oop), bytes);
}

#[test]
fn an_empty_byte_vector_answers_an_empty_byte_array_rather_than_nil() {
    let vm = interp();
    Vec::<u8>::new().into_return(&vm).unwrap();
    let oop = answered_object();
    assert_eq!(instantiations(), vec![CLASS_BYTE_ARRAY]);
    assert_ne!(oop.0, NIL);
    assert_eq!(vm.byte_size_of(oop).unwrap(), 0);
    assert!(contents(oop).is_empty());
}

// ---------------------------------------------------------------------------
// Bytes the image has no room for
// ---------------------------------------------------------------------------

/// The contract the `Vec<u8>` impl is documented against: the allocation is
/// the impl's own, it happens after the primitive body has returned, and a
/// failure there is a `NoMemory` failure of the *primitive* -- which the
/// interpreter answers by collecting and running the primitive again
/// (`StackInterpreter>>#retryPrimitiveOnFailure`,
/// `smalltalksrc/VMMaker/StackInterpreter.class.st:13146-13182`).
#[test]
fn a_byte_vector_the_image_cannot_hold_fails_with_no_memory() {
    let vm = interp();
    let outcome = with_allocation_failing(|| vec![1u8, 2, 3].into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    // It did try, and it tried for a ByteArray.
    assert_eq!(instantiations(), vec![CLASS_BYTE_ARRAY]);
    // And nothing was answered: the primitive fails, so the retry sees a stack
    // the impl never touched. Answering *and* failing would leave the image
    // with the answer of a call that officially did not happen.
    assert_eq!(answers(), vec![]);
}

#[test]
fn an_empty_byte_vector_the_image_cannot_hold_fails_too() {
    // Zero-sized is still an allocation -- an empty ByteArray is an object.
    let vm = interp();
    let outcome = with_allocation_failing(|| Vec::<u8>::new().into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    assert_eq!(answers(), vec![]);
}

#[test]
fn some_bytes_the_image_cannot_hold_fail_exactly_as_the_bare_vector_does() {
    // `Option` adds no allocation of its own, so it adds no failure of its
    // own either: `Some` is whatever `T` does, including this.
    let vm = interp();
    let outcome = with_allocation_failing(|| Some(vec![4u8, 5]).into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    assert_eq!(answers(), vec![]);
    assert_eq!(instantiations(), vec![CLASS_BYTE_ARRAY]);

    // `None` answers nil, which is a well-known object, so it still succeeds
    // with the image full -- and allocates nothing on the way.
    with_allocation_failing(|| None::<Vec<u8>>.into_return(&vm)).unwrap();
    assert_eq!(answer(), Answer::Value(NIL));
    assert_eq!(instantiations(), vec![]);
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

#[test]
fn a_str_answers_a_string_the_proxy_allocated() {
    let vm = interp();
    "hello".into_return(&vm).unwrap();
    let oop = answered_object();
    assert_eq!(contents(oop), b"hello".to_vec());
    // `stringForCString` allocates the String itself, so nothing was
    // instantiated by class on the way.
    assert_eq!(instantiations(), vec![]);
}

#[test]
fn an_owned_string_answers_what_the_borrowed_one_would() {
    let vm = interp();
    String::from("hello").into_return(&vm).unwrap();
    assert_eq!(contents(answered_object()), b"hello".to_vec());
}

/// `stringForCString:` answers nil -- 0 in C -- when the image cannot hold the
/// String (`smalltalksrc/VMMaker/SpurMemoryManager.class.st:12944-12961`).
/// Reading that 0 as an oop would hand the image a String that is not there,
/// so [`Interp::string`] checks it the way `Interp::instantiate` does.
#[test]
fn a_str_the_image_cannot_hold_fails_with_no_memory_rather_than_answering_zero() {
    let vm = interp();
    let outcome = with_allocation_failing(|| "hello".into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    assert_eq!(answers(), vec![]);

    let outcome = with_allocation_failing(|| String::from("hello").into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    assert_eq!(answers(), vec![]);

    let outcome = with_allocation_failing(|| Some("hello").into_return(&vm));
    assert_eq!(outcome.unwrap_err(), PrimErr::NoMemory);
    assert_eq!(answers(), vec![]);
}

#[test]
fn a_str_with_an_interior_nul_fails_before_it_allocates() {
    // The proxy takes a C string, so this one cannot be represented at all --
    // a bad argument, not an out-of-memory.
    let vm = interp();
    assert_eq!("a\0b".into_return(&vm).unwrap_err(), PrimErr::BadArgument);
    assert_eq!(answers(), vec![]);
}

// ---------------------------------------------------------------------------
// Option
// ---------------------------------------------------------------------------

#[test]
fn some_answers_exactly_what_its_value_answers() {
    let vm = interp();

    Some(7i32).into_return(&vm).unwrap();
    let wrapped = answer();
    7i32.into_return(&vm).unwrap();
    assert_eq!(wrapped, answer());

    Some(true).into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Bool(true));

    Some(1.5f64).into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Float(1.5));

    Some(()).into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Receiver);
}

#[test]
fn some_bytes_answer_the_byte_array_the_bare_vector_would() {
    let vm = interp();
    Some(vec![9u8, 8, 7]).into_return(&vm).unwrap();
    assert_eq!(contents(answered_object()), vec![9, 8, 7]);
    assert_eq!(instantiations(), vec![CLASS_BYTE_ARRAY]);
}

#[test]
fn none_answers_nil() {
    let vm = interp();
    None::<Vec<u8>>.into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Value(NIL));
    // Nothing was allocated on the way: nil is a well-known object.
    None::<sqInt>.into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Value(NIL));
    assert_eq!(instantiations(), vec![]);
}

#[test]
fn a_nested_option_still_bottoms_out_at_nil() {
    let vm = interp();
    Some(None::<i32>).into_return(&vm).unwrap();
    assert_eq!(answer(), Answer::Value(NIL));
}
