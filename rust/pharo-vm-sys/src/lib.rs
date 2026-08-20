//! Raw FFI bindings to the Pharo VM's C headers.
//!
//! This crate is the unsafe floor of the Rust side of the VM. It adds no
//! abstraction, no safety and no opinions: it exists so that `pharo-platform`
//! (the port target) and `pharo-vm-plugin` (the plugin SDK) have one agreed,
//! build-accurate view of the C types.
//!
//! The bindings are generated at build time from `wrapper.h`; see `build.rs`
//! for why they are not checked in.

#![no_std]
// Generated bindings do not follow Rust naming conventions, and cannot.
#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// `sqInt` is the VM's object-pointer-sized signed integer. It must be exactly
/// pointer-sized: the whole object representation (immediates, tagged
/// pointers, the Spur header) depends on it. If this assert ever fires, the
/// build has picked a `config.h` that disagrees with the target Rust is
/// compiling for, and every binding below is suspect.
const _: () = assert!(
    core::mem::size_of::<usize>() == core::mem::size_of::<*const core::ffi::c_void>(),
    "pointer-sized integer assumption violated"
);
