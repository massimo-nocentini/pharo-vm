//! One job: let the four `aio*` symbols stay undefined in the cdylib on Apple
//! platforms.
//!
//! `src/aio.rs` declares `aioEnable`, `aioHandle`, `aioDisable` and `aioFini`
//! as plain externs. They are defined in the VM core (`src/unix/aio.c`), not
//! here, and are meant to bind to the host process when it `dlopen`s the
//! plugin -- exactly the arrangement the C plugin had.
//!
//! On ELF that needs no help: `ld` leaves undefined symbols in a shared object
//! alone and the dynamic linker resolves them at load time, which is why this
//! crate has never needed a build script for Linux. Mach-O is the opposite:
//! `ld64` resolves every symbol at static-link time and fails the link
//! otherwise, so `cargo build` on macOS ends in
//!
//! ```text
//! Undefined symbols for architecture arm64: "_aioEnable", ...
//! ```
//!
//! The C plugin never hit this because CMake links it against the VM core
//! library (`addLibraryWithRPATH` in `cmake/macros.cmake` does
//! `target_link_libraries(${NAME} PRIVATE ${VM_LIBRARY_NAME})`), so on macOS
//! the C's `aio*` references are satisfied by `libPharoVMCore.dylib` at link
//! time. Corrosion builds this crate with a bare `cargo build`, with no such
//! library on the command line, so the link has to be told instead.
//!
//! `-Wl,-U,_symbol` is that instruction, one symbol at a time: ld64 permits
//! exactly the named symbol to remain undefined and marks it for lookup in the
//! loading process. Deliberately not `-undefined dynamic_lookup`, which says
//! the same thing about *every* symbol and would turn a genuine typo in an
//! extern declaration into a runtime crash instead of a build error.
//!
//! The leading underscore is the Mach-O C symbol prefix; `_aioEnable` here is
//! the `aioEnable` of `aio.h`.

/// The VM-core symbols `src/aio.rs` leaves undefined. Keep in step with the
/// `extern "C"` block there -- and only with it: an entry that no longer
/// matches an extern is harmless, one that is missing is a broken macOS link.
const VM_CORE_SYMBOLS: &[&str] = &["aioEnable", "aioHandle", "aioDisable", "aioFini"];

/// The Mach-O targets this crate is built for, spelled as `CARGO_CFG_TARGET_OS`
/// values.
///
/// This list must stay in step with the `#[cfg]` predicates in `src/`, every
/// one of which is `any(target_os = "macos", target_os = "ios")`
/// (`resolver.rs`, `sock.rs`, `address.rs`).
/// It was written here as `CARGO_CFG_TARGET_VENDOR == "apple"`
/// first, which is a *wider* set (tvos, watchos, visionos too): harmless while
/// nothing builds for those, but two different answers to "is this the Apple
/// build?" in one crate is exactly the kind of thing that drifts in silence.
/// One predicate, in both places.
const APPLE_TARGET_OS: &[&str] = &["macos", "ios"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if APPLE_TARGET_OS.contains(&target_os.as_str()) {
        for symbol in VM_CORE_SYMBOLS {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-U,_{symbol}");
        }
    }
}
