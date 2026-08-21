//! Turns the C build's preprocessor definitions into `cfg` flags.
//!
//! A few ported files branch on a definition CMake sets rather than on the
//! target triple. `READ_ONLY_CODE_ZONE` is the first: it is a CMake option, so
//! it cannot be derived from the platform, and a Rust module that guessed
//! would map the JIT's code zone with the wrong protection.
//!
//! `cmake/rust.cmake` already exports the full list as `PHAROVM_COMPILE_DEFS`
//! for bindgen. This reads the same variable, so the Rust and the C always
//! agree by construction.
//!
//! Only definitions listed in `TRACKED` become cfgs, and each one is declared
//! to `check-cfg` so a typo in a `#[cfg(...)]` is a warning rather than a
//! silently dead branch.

use std::env;

/// The separator `cmake/rust.cmake` joins the definitions with.
const LIST_SEP: char = '|';

/// C definitions that a `pharo-platform` module branches on.
///
/// The cfg name is the definition lowercased. A definition counts as "on" when
/// it is present and not set to `0`, matching `#if NAME` in the C.
const TRACKED: &[&str] = &[
    // Cogit's code zone is never writable and executable at once. Set by
    // cmake/OpenBSD.cmake; read by memory_unix.rs.
    "READ_ONLY_CODE_ZONE",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PHAROVM_COMPILE_DEFS");

    for name in TRACKED {
        println!("cargo:rustc-check-cfg=cfg({})", name.to_lowercase());
    }

    let raw = env::var("PHAROVM_COMPILE_DEFS").unwrap_or_default();
    for def in raw.split(LIST_SEP).map(str::trim).filter(|s| !s.is_empty()) {
        // Definitions arrive as `NAME` or `NAME=VALUE`, without a leading -D.
        let (name, value) = match def.split_once('=') {
            Some((name, value)) => (name.trim(), value.trim()),
            None => (def, "1"),
        };

        if TRACKED.contains(&name) && value != "0" {
            println!("cargo:rustc-cfg={}", name.to_lowercase());
        }
    }
}
