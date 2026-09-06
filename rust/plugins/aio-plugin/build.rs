//! One job: let the three `aio*` symbols stay undefined in the cdylib on Apple
//! platforms.
//!
//! `src/aio.rs` declares `aioEnable`, `aioHandle` and `aioDisable` as plain
//! externs. They are defined in the VM core (`src/unix/aio.c`,
//! `src/osx/aioOSX.c`), not here, and bind to the host process when it
//! `dlopen`s the plugin -- the same arrangement `SocketPlugin` has, and the
//! same one the C plugins had before it.
//!
//! ELF needs no help: `ld` leaves undefined symbols in a shared object alone
//! and the dynamic linker resolves them at load time. Mach-O is the opposite --
//! `ld64` resolves everything at static-link time and fails otherwise -- and
//! corrosion builds this crate with a bare `cargo build`, with no VM library on
//! the command line, so the link has to be told.
//!
//! `-Wl,-U,_symbol`, one symbol at a time, is that instruction. Deliberately
//! not `-undefined dynamic_lookup`, which says the same of *every* symbol and
//! would turn a typo in an extern into a runtime crash instead of a build
//! error. The leading underscore is Mach-O's C symbol prefix.
//!
//! See `rust/plugins/socket-plugin/build.rs`, which explains the same thing at
//! length and is the reason this one can be short.

/// The VM-core symbols `src/aio.rs` leaves undefined. Keep in step with the
/// `extern "C"` block there: an entry that no longer matches an extern is
/// harmless, a missing one is a broken macOS link.
const VM_CORE_SYMBOLS: &[&str] = &["aioEnable", "aioHandle", "aioDisable"];

/// The Mach-O targets this crate is built for, spelled as `CARGO_CFG_TARGET_OS`
/// values -- kept identical to socket-plugin's list rather than widened to
/// `CARGO_CFG_TARGET_VENDOR == "apple"`, so the two crates cannot drift into
/// two different answers to "is this the Apple build?".
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
