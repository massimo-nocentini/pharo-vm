//! The in-place accessors, driven through a fake proxy table.
//!
//! No VM is running here, so the five entries `with_bytes_mut` /
//! `with_words_mut` need -- `isBytes`, `isWordsOrBytes`, `byteSizeOf`,
//! `firstIndexableField`, `isOopImmutable` -- are answered from a Rust-side
//! heap whose objects are leaked, so their addresses behave like image
//! objects that never move. What is under test is the lending: that a
//! mutable view really writes the object, that nothing else can reach those
//! bytes while it is live, and that the lease comes back on every exit.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pharo_vm_plugin::{sqInt, Interp, Oop, PrimErr, VirtualMachine};

// ---------------------------------------------------------------------------
// A heap of five-field objects
// ---------------------------------------------------------------------------

struct Obj {
    ptr: *mut u8,
    len: usize,
    bytes: bool,
    immutable: bool,
}

thread_local! {
    static HEAP: RefCell<Vec<Obj>> = const { RefCell::new(Vec::new()) };
}

/// Interns a leaked buffer and answers its oop: a 1-based index, so oop 0 is
/// never a valid object, as in the image.
fn intern(data: Vec<u8>, bytes: bool, immutable: bool) -> Oop {
    let leaked = Box::leak(data.into_boxed_slice());
    HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.push(Obj {
            ptr: leaked.as_mut_ptr(),
            len: leaked.len(),
            bytes,
            immutable,
        });
        Oop(heap.len() as sqInt)
    })
}

fn byte_object(len: usize) -> Oop {
    intern(vec![0u8; len], true, false)
}

fn word_object(words: usize) -> Oop {
    intern(vec![0u8; words * 4], false, false)
}

fn with_obj<R>(oop: sqInt, f: impl FnOnce(&Obj) -> R) -> R {
    HEAP.with(|heap| f(&heap.borrow()[oop as usize - 1]))
}

/// The object's current contents, read without going through the proxy.
fn contents(oop: Oop) -> Vec<u8> {
    with_obj(oop.0, |o| {
        // SAFETY: the buffer was leaked at `intern` and is `o.len` long.
        unsafe { std::slice::from_raw_parts(o.ptr, o.len) }.to_vec()
    })
}

// ---------------------------------------------------------------------------
// The proxy entries the accessors call
// ---------------------------------------------------------------------------

unsafe extern "C" fn is_bytes(oop: sqInt) -> sqInt {
    with_obj(oop, |o| sqInt::from(o.bytes))
}

unsafe extern "C" fn is_words_or_bytes(_oop: sqInt) -> sqInt {
    1
}

unsafe extern "C" fn byte_size_of(oop: sqInt) -> sqInt {
    with_obj(oop, |o| o.len as sqInt)
}

unsafe extern "C" fn first_indexable_field(oop: sqInt) -> *mut std::ffi::c_void {
    with_obj(oop, |o| o.ptr.cast())
}

unsafe extern "C" fn is_oop_immutable(oop: sqInt) -> sqInt {
    with_obj(oop, |o| sqInt::from(o.immutable))
}

/// An `Interp` over a proxy table holding only those five entries. Every
/// other field is `None`, which is what a plugin sees from a VM that does
/// not export it -- and what the accessors under test never reach for.
fn interp() -> Interp {
    // SAFETY: `VirtualMachine` is function pointers all the way down, and a
    // zeroed `Option<fn>` is `None`.
    let mut vt: VirtualMachine = unsafe { std::mem::zeroed() };
    vt.isBytes = Some(is_bytes);
    vt.isWordsOrBytes = Some(is_words_or_bytes);
    vt.byteSizeOf = Some(byte_size_of);
    vt.firstIndexableField = Some(first_indexable_field);
    vt.isOopImmutable = Some(is_oop_immutable);
    // SAFETY: the table is leaked, so it lives as long as the process, which
    // is the contract `from_raw` asks for.
    unsafe { Interp::from_raw(Box::leak(Box::new(vt))) }
}

// ---------------------------------------------------------------------------
// Writing where the object lies
// ---------------------------------------------------------------------------

#[test]
fn a_mutable_view_writes_the_object_itself() {
    let vm = interp();
    let oop = byte_object(4);
    vm.with_bytes_mut(oop, |dst| dst.copy_from_slice(&[1, 2, 3, 4]))
        .unwrap();
    assert_eq!(contents(oop), vec![1, 2, 3, 4]);
}

#[test]
fn a_mutable_word_view_writes_the_object_itself() {
    let vm = interp();
    let oop = word_object(2);
    vm.with_words_mut(oop, |dst| dst.copy_from_slice(&[0xDEAD_BEEF, 7]))
        .unwrap();
    assert_eq!(vm.words_of(oop).unwrap(), &[0xDEAD_BEEF, 7]);
}

#[test]
fn the_closure_answers_through() {
    let vm = interp();
    let oop = byte_object(3);
    let sum = vm.with_bytes_mut(oop, |dst| {
        dst.fill(5);
        dst.iter().map(|&b| u32::from(b)).sum::<u32>()
    });
    assert_eq!(sum.unwrap(), 15);
}

#[test]
fn an_immutable_object_refuses_a_mutable_view() {
    let vm = interp();
    let oop = intern(vec![0u8; 4], true, true);
    assert_eq!(
        vm.with_bytes_mut(oop, |_| ()).unwrap_err(),
        PrimErr::NoModification
    );
    assert_eq!(
        vm.with_words_mut(oop, |_| ()).unwrap_err(),
        PrimErr::NoModification
    );
}

#[test]
fn a_pointers_object_refuses_a_byte_view() {
    let vm = interp();
    let oop = intern(vec![0u8; 4], false, false);
    assert_eq!(
        vm.with_bytes_mut(oop, |_| ()).unwrap_err(),
        PrimErr::BadArgument
    );
}

#[test]
fn a_word_view_wants_a_whole_number_of_words() {
    let vm = interp();
    let oop = byte_object(6);
    assert_eq!(
        vm.with_words_mut(oop, |_| ()).unwrap_err(),
        PrimErr::BadArgument
    );
}

// ---------------------------------------------------------------------------
// What the lease forbids while it is live
// ---------------------------------------------------------------------------

#[test]
fn nothing_else_reaches_the_bytes_being_written() {
    let vm = interp();
    let oop = byte_object(8);
    vm.with_bytes_mut(oop, |_| {
        assert_eq!(vm.bytes_of(oop).unwrap_err(), PrimErr::Inappropriate);
        assert_eq!(vm.words_of(oop).unwrap_err(), PrimErr::Inappropriate);
        assert_eq!(
            vm.write_bytes(oop, 0, &[1]).unwrap_err(),
            PrimErr::Inappropriate
        );
        assert_eq!(
            vm.write_words(oop, 0, &[1]).unwrap_err(),
            PrimErr::Inappropriate
        );
        assert_eq!(
            vm.with_bytes_mut(oop, |_| ()).unwrap_err(),
            PrimErr::Inappropriate
        );
        assert_eq!(
            vm.indexable_bytes_ptr(oop).unwrap_err(),
            PrimErr::Inappropriate
        );
    })
    .unwrap();
}

#[test]
fn a_different_object_is_still_reachable() {
    let vm = interp();
    let (dst, src) = (byte_object(4), byte_object(4));
    vm.write_bytes(src, 0, &[9, 9, 9, 9]).unwrap();
    vm.with_bytes_mut(dst, |out| {
        out.copy_from_slice(vm.bytes_of(src).unwrap());
    })
    .unwrap();
    assert_eq!(contents(dst), vec![9, 9, 9, 9]);
}

#[test]
fn one_object_passed_as_two_arguments_fails_rather_than_aliases() {
    let vm = interp();
    let oop = byte_object(4);
    let both = vm.with_bytes_mut(oop, |_| vm.bytes_of(oop).map(<[u8]>::to_vec));
    assert_eq!(both.unwrap().unwrap_err(), PrimErr::Inappropriate);
}

// ---------------------------------------------------------------------------
// Giving the lease back
// ---------------------------------------------------------------------------

#[test]
fn the_lease_is_released_when_the_closure_returns() {
    let vm = interp();
    let oop = byte_object(4);
    vm.with_bytes_mut(oop, |_| ()).unwrap();
    assert!(vm.bytes_of(oop).is_ok());
}

#[test]
fn the_lease_is_released_when_the_closure_panics() {
    let vm = interp();
    let oop = byte_object(4);
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        vm.with_bytes_mut(oop, |_| panic!("primitive bodies unwind into catch_unwind"))
    }));
    assert!(panicked.is_err());
    assert!(vm.bytes_of(oop).is_ok());
}

#[test]
fn views_nest_until_the_table_is_full() {
    let vm = interp();
    // Nine distinct objects: the ninth finds no free slot.
    fn nest(vm: &Interp, oops: &[Oop]) -> Result<(), PrimErr> {
        match oops.split_first() {
            None => Ok(()),
            Some((&head, rest)) => vm.with_bytes_mut(head, |_| nest(vm, rest))?,
        }
    }
    let oops: Vec<Oop> = (0..8).map(|_| byte_object(4)).collect();
    assert!(nest(&vm, &oops).is_ok());
    let oops: Vec<Oop> = (0..9).map(|_| byte_object(4)).collect();
    assert_eq!(nest(&vm, &oops).unwrap_err(), PrimErr::LimitExceeded);
}

// ---------------------------------------------------------------------------
// The fixed-size doubles, and words written at an offset
// ---------------------------------------------------------------------------

#[test]
fn doubles_round_trip_through_a_stack_array() {
    let vm = interp();
    let oop = byte_object(6 * 8);
    let matrix = [1.0, 0.0, 0.0, 1.0, 12.5, -3.75];
    vm.write_f64s(oop, &matrix).unwrap();
    assert_eq!(vm.read_f64_array::<6>(oop).unwrap(), matrix);
    assert_eq!(vm.read_f64s(oop, 6).unwrap(), matrix.to_vec());
}

#[test]
fn a_double_array_of_the_wrong_length_is_refused() {
    let vm = interp();
    let oop = byte_object(3 * 8);
    assert_eq!(
        vm.read_f64_array::<6>(oop).unwrap_err(),
        PrimErr::BadArgument
    );
    assert_eq!(
        vm.write_f64s(oop, &[1.0, 2.0]).unwrap_err(),
        PrimErr::BadArgument
    );
}

#[test]
fn words_land_at_the_offset_they_were_given() {
    let vm = interp();
    let oop = word_object(4);
    vm.write_words(oop, 2, &[0x1122_3344, 0x5566_7788]).unwrap();
    assert_eq!(vm.words_of(oop).unwrap(), &[0, 0, 0x1122_3344, 0x5566_7788]);
    assert_eq!(
        vm.write_words(oop, 3, &[1, 2]).unwrap_err(),
        PrimErr::BadIndex
    );
}

#[test]
fn a_c_string_comes_out_of_the_object_in_one_copy() {
    let vm = interp();
    let oop = intern(b"/tmp/pharo.image".to_vec(), true, false);
    assert_eq!(
        vm.c_string_value(oop).unwrap().as_bytes(),
        b"/tmp/pharo.image"
    );
    let with_nul = intern(b"a\0b".to_vec(), true, false);
    assert_eq!(
        vm.c_string_value(with_nul).unwrap_err(),
        PrimErr::BadArgument
    );
}
