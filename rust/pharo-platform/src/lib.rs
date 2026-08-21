//! Rust implementation of the Pharo VM's platform layer.
//!
//! The VM splits in two. The interpreter, garbage collector and JIT are
//! written in Slang (Smalltalk) under `smalltalksrc/` and translated to
//! `cointerp.c` / `cogit.c` at build time; none of that is touched here.
//! Around it sits a hand-written C platform layer in `src/` -- image loading,
//! address-space mapping, the heartbeat thread, async I/O, command-line
//! parsing, module loading, FFI -- and *that* is what this crate is replacing,
//! one file at a time.
//!
//! # The rule every module here follows
//!
//! Each `#[no_mangle] extern "C"` item below replaces a symbol that used to
//! come from a `.c` file in `src/`. The linker output must be
//! indistinguishable: same symbol, same signature, same behaviour. When a
//! module lands, its C counterpart is removed from `SUPPORT_SOURCES` in
//! `CMakeLists.txt`, and `rust/tools/abi-check.sh` proves the exported symbol
//! set did not move.
//!
//! Faithfulness beats taste during a port. Where the C did something odd, this
//! code does the same odd thing and says so in a comment; behaviour changes
//! are separate commits, so that differential testing stays meaningful.
//!
//! # Safety invariants
//!
//! * **Panics must not unwind into C.** The workspace sets `panic = "abort"`.
//!   Entry points that can fail meaningfully report failure through their
//!   normal C return convention rather than by panicking.
//! * **No allocation, locks or formatting in signal-handler paths.** The
//!   heartbeat runs on a real-time-priority thread and signal handlers are
//!   installed by `src/unix/debugUnix.c`; anything reachable from those must
//!   stay async-signal-safe.
//! * **`longjmp` must never cross a Rust frame.** The FFI trampolines in
//!   `src/ffi/` use `sigsetjmp`; those shims stay in C.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod error_code;
#[cfg(unix)]
pub mod external_primitives;
#[cfg(all(unix, target_pointer_width = "64"))]
pub mod heap_map;
#[cfg(unix)]
pub mod image_access;
mod logging;
pub mod parameter_vector;
#[cfg(unix)]
pub mod path_utilities;
#[cfg(unix)]
pub mod pharo_semaphore;
#[cfg(all(unix, not(target_vendor = "apple")))]
pub mod platform_semaphore;
pub mod string_utilities;
#[cfg(all(unix, not(target_vendor = "apple")))]
pub mod thread_safe_queue;
