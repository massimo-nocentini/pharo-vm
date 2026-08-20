//! A Pharo VM plugin, in Rust: drop-in replacement for the C `UUIDPlugin`.
//!
//! This exists to prove the SDK end to end against a real VM and a real image.
//! Pharo's own `UUID` class already calls this module --
//! `<primitive: 'primitiveMakeUUID' module: 'UUIDPlugin'>` -- so building this
//! crate and dropping `libUUIDPlugin.so` next to the `pharo` executable is
//! enough to see it run; no image-side changes at all.
//!
//! Compare `plugins/UUIDPlugin/common/UUIDPlugin.c`. The C version is about
//! 170 lines across four `#ifdef` branches, and on Linux it installs a SIGSEGV
//! handler with `sigsetjmp`/`siglongjmp` to probe whether `libuuid` actually
//! works. This version has no C, no conditional compilation, and no external
//! dependencies.
//!
//! # The primitive's contract
//!
//! `primitiveMakeUUID` takes no arguments. Its receiver is a 16-byte
//! byte-indexable object which it fills in place and answers. Anything else --
//! wrong arity, a receiver that is not bytes, a receiver of the wrong size --
//! is a clean primitive failure, and the image runs its Smalltalk fallback.

// The crate is named for the shared library the VM loads (libUUIDPlugin.so),
// which fixes its spelling.
#![allow(non_snake_case)]

use std::fs::File;
use std::io::Read;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, Oop, PrimErr, PrimResult};

/// A UUID is 16 bytes.
const UUID_LEN: usize = 16;

pharo_plugin!("UUIDPlugin", init = initialise);

/// Runs when the VM loads the module. Answering `false` makes the VM reject it.
///
/// The C plugin probes `libuuid` here by catching a SIGSEGV. We have no such
/// dependency, so the only thing worth checking is that we can actually get
/// randomness -- better to decline the module now than to fail every call.
fn initialise() -> bool {
    random_bytes().is_ok()
}

/// Fills the receiver with a version 4 (random) UUID and answers it.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveMakeUUID(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;

    // With no arguments, stack offset 0 is the receiver.
    let receiver = vm.stack_value(0)?;
    if vm.byte_size_of(receiver)? != UUID_LEN as isize {
        return Err(PrimErr::BadReceiver);
    }

    // Generate before borrowing the object: this must not fail halfway
    // through, leaving the receiver holding a half-written UUID.
    let uuid = make_v4()?;

    // write_bytes rejects a non-bytes, wrong-sized or immutable receiver.
    vm.write_bytes(receiver, 0, &uuid)?;

    Ok(receiver)
}

/// Builds a version 4 UUID as laid down by RFC 4122 §4.4: random everywhere,
/// except the version nibble and the two variant bits.
fn make_v4() -> PrimResult<[u8; UUID_LEN]> {
    let mut bytes = random_bytes()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    Ok(bytes)
}

/// Reads 16 bytes from the OS entropy source.
///
/// `/dev/urandom` is the portable-enough choice across the Unixes this VM
/// targets, and keeps the example dependency-free. A production plugin would
/// reach for `getrandom` and support Windows.
fn random_bytes() -> PrimResult<[u8; UUID_LEN]> {
    let mut bytes = [0u8; UUID_LEN];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| PrimErr::OSError)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_sets_version_and_variant_bits() {
        for _ in 0..64 {
            let u = make_v4().expect("entropy available in the test environment");
            assert_eq!(u[6] & 0xf0, 0x40, "version nibble");
            assert_eq!(u[8] & 0xc0, 0x80, "variant bits");
        }
    }

    #[test]
    fn successive_uuids_differ() {
        let a = make_v4().unwrap();
        let b = make_v4().unwrap();
        assert_ne!(a, b);
    }
}
