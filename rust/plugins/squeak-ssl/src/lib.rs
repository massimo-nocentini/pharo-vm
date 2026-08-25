//! `SqueakSSL`, in Rust.
//!
//! Replaces the C plugin pair `plugins/SqueakSSL/src/common/SqueakSSL.c`
//! (the Slang-generated primitive shims) and
//! `plugins/SqueakSSL/src/unix/sqUnixSSL.c` (the OpenSSL implementation)
//! behind the same ten primitives. The image cannot tell the difference:
//! same module name, same primitive names, same argument order, same return
//! codes.
//!
//! The plugin does TLS over memory BIOs: the image owns the socket and feeds
//! raw TLS bytes through these primitives, so nothing here ever touches the
//! network. A session is a small-integer handle into a global table, exactly
//! as in the C; the state machine itself lives in [`ssl`], VM-free, which is
//! what lets the tests drive a whole client-against-server handshake without
//! a VM.

#![allow(non_snake_case)] // primitive names are fixed by the image

pub mod ssl;

use std::ffi::c_int;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

// The C's getModuleName answers "SqueakSSL VMMaker.oscog-eem.2480 (e)"; the
// VM compares only the module-name prefix, so the plain name is what matters
// (same choice as jpeg-plugin).
pharo_plugin!("SqueakSSL");

/// Reads a stack slot through the proxy's `signed32BitValueOf`, which --
/// unlike `stackIntegerValue` -- also accepts LargeIntegers that fit in 32
/// bits. That is how the C's `primitiveSetIntProperty` reads its value.
fn signed32_stack_value(vm: &Interp, offset: sqInt) -> PrimResult<i32> {
    let oop = vm.stack_value(offset)?;
    // SAFETY: the proxy table the VM handed to setInterpreter lives for the
    // whole process, and signed32BitValueOf only reads the oop. A
    // non-integer is reported through the failed flag, checked just after.
    let f = unsafe { (*vm.as_raw()).signed32BitValueOf }.ok_or(PrimErr::Unsupported)?;
    let value = unsafe { f(oop.0) };
    vm.check_failed()?;
    Ok(value)
}

/// The shared body of the four buffer primitives (connect, accept, encrypt,
/// decrypt). All take `handle srcOop start srcLen dstOop`, feed
/// `srcOop[start-1 .. start-1+srcLen]` (a 1-based range) to the core, write
/// the core's output into `dstOop`, and answer the core's return code.
fn buffer_primitive(
    vm: &Interp,
    handle: sqInt,
    src_oop: Oop,
    start: sqInt,
    src_len: sqInt,
    dst_oop: Oop,
    op: fn(isize, &[u8], &mut [u8]) -> isize,
) -> PrimResult<isize> {
    // The C's guard, with the same short-circuit order and the same generic
    // failure. `start + srcLen` cannot overflow: both came from SmallIntegers.
    if !(start > 0
        && src_len >= 0
        && vm.is_bytes(src_oop)?
        && vm.is_bytes(dst_oop)?
        && vm.byte_size_of(src_oop)? >= start + src_len - 1)
    {
        return Err(PrimErr::GenericFailure);
    }

    // Copy the input out of image memory before doing anything else; the
    // guard above proved the range is in bounds.
    let from = (start - 1) as usize;
    let src = vm.bytes_of(src_oop)?[from..from + src_len as usize].to_vec();

    // The C hands the whole destination object to OpenSSL to write into
    // directly; we stage in a Rust buffer and copy back what was produced.
    let dst_len = usize::try_from(vm.byte_size_of(dst_oop)?)?;
    let mut dst = vec![0u8; dst_len];

    let result = op(handle, &src, &mut dst);
    if result > 0 {
        vm.write_bytes(dst_oop, 0, &dst[..result as usize])?;
    }
    Ok(result)
}

/// Creates a new SSL session and answers its handle.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveCreate(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    let handle = ssl::create_ssl();
    // Unreachable -- handles start at 1 -- but the C checks, so we check.
    if handle == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(handle)
}

/// Destroys an SSL session. Fails on an invalid handle.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDestroy(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?;
    let handle = vm.stack_integer(0)?;
    if ssl::destroy_ssl(handle) == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// Starts or continues a client handshake. Answers the number of bytes
/// written to the destination for the server, 0 once connected, -1 when more
/// input is required, or an error code below -1.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveConnect(
    vm: &Interp,
    handle: sqInt,
    src: Oop,
    start: sqInt,
    srcLen: sqInt,
    dst: Oop,
) -> PrimResult<isize> {
    buffer_primitive(vm, handle, src, start, srcLen, dst, ssl::connect_ssl)
}

/// Starts or continues a server handshake; the session needs its certificate
/// name set. Same return convention as [`primitiveConnect`].
#[pharo_primitive(accessor_depth = 1)]
fn primitiveAccept(
    vm: &Interp,
    handle: sqInt,
    src: Oop,
    start: sqInt,
    srcLen: sqInt,
    dst: Oop,
) -> PrimResult<isize> {
    buffer_primitive(vm, handle, src, start, srcLen, dst, ssl::accept_ssl)
}

/// Encrypts plaintext into TLS records for the image to send. Requires an
/// established session; answers the bytes produced or an error code.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveEncrypt(
    vm: &Interp,
    handle: sqInt,
    src: Oop,
    start: sqInt,
    srcLen: sqInt,
    dst: Oop,
) -> PrimResult<isize> {
    buffer_primitive(vm, handle, src, start, srcLen, dst, ssl::encrypt_ssl)
}

/// Decrypts received TLS records into plaintext. Requires an established
/// session; answers the bytes produced (0 when a record is incomplete) or an
/// error code.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveDecrypt(
    vm: &Interp,
    handle: sqInt,
    src: Oop,
    start: sqInt,
    srcLen: sqInt,
    dst: Oop,
) -> PrimResult<isize> {
    buffer_primitive(vm, handle, src, start, srcLen, dst, ssl::decrypt_ssl)
}

/// Answers an integer property of the session. An invalid handle or unknown
/// property answers 0 -- it does not fail, matching the C.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveGetIntProperty(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(2)?;
    let prop_id = vm.stack_integer(0)?;
    let handle = vm.stack_integer(1)?;
    Ok(ssl::get_int_property_ssl(handle, prop_id))
}

/// Sets an integer property of the session (only the log level is
/// settable). The value is read with `signed32BitValueOf`, so 32-bit
/// LargeIntegers are accepted, as in the C.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveSetIntProperty(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(3)?;
    let value = signed32_stack_value(vm, 0)?;
    let prop_id = vm.stack_integer(1)?;
    let handle = vm.stack_integer(2)?;
    if ssl::set_int_property_ssl(handle, prop_id, value as isize) == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// Answers a string property of the session, or `nil` where the C answers
/// NULL (unknown ID, invalid handle, unset cert/server name).
#[pharo_primitive(accessor_depth = 0)]
fn primitiveGetStringProperty(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(2)?;
    let prop_id = vm.stack_integer(0)?;
    let handle = vm.stack_integer(1)?;
    match ssl::get_string_property_ssl(handle, prop_id as c_int) {
        None => vm.nil(),
        Some(bytes) => {
            // As the C does: instantiate a String of the exact length and
            // copy the bytes in. `bytes` is our own copy, so the allocation
            // does not invalidate anything we still hold.
            let string = vm.instantiate(vm.class_string()?, bytes.len() as sqInt)?;
            vm.write_bytes(string, 0, &bytes)?;
            Ok(string)
        }
    }
}

/// Sets a string property of the session from a byte object (only the cert
/// and server names are settable; an empty value clears back to `nil`).
#[pharo_primitive(accessor_depth = 1)]
fn primitiveSetStringProperty(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(3)?;
    let src_oop = vm.stack_value(0)?;
    let prop_id = vm.stack_integer(1)?;
    let handle = vm.stack_integer(2)?;
    if !vm.is_bytes(src_oop)? {
        return Err(PrimErr::GenericFailure);
    }
    let value = vm.bytes_of(src_oop)?;
    if ssl::set_string_property_ssl(handle, prop_id as c_int, value) == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}
