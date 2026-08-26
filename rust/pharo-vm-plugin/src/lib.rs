//! Write Pharo VM plugins in Rust.
//!
//! A Pharo *plugin* is a shared library the VM loads on demand to service
//! named primitives -- the things an image declares as
//! `<primitive: 'primitiveFoo' module: 'MyPlugin'>`. Historically that meant
//! writing C against a 153-entry function-pointer table, remembering to export
//! an `AccessorDepth` byte next to every primitive, and being careful never to
//! let an error escape as anything but a clean failure.
//!
//! This crate does that part. You write:
//!
//! ```ignore
//! use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, PrimResult};
//!
//! pharo_plugin!("MyPlugin");
//!
//! #[pharo_primitive]
//! fn primitiveDoubled(vm: &Interp) -> PrimResult<isize> {
//!     vm.expect_argument_count(0)?;   // unary method: receiver at offset 0
//!     Ok(vm.stack_integer(0)? * 2)
//! }
//! ```
//!
//! and call it from the image with
//! `<primitive: 'primitiveDoubled' module: 'MyPlugin'>`.
//!
//! # No Pharo checkout required
//!
//! This crate is header-free: no bindgen, no libclang, no CMake, no VM sources.
//! It pins the published proxy ABI (major 1, minor 15) directly, which is what
//! a separately-compiled plugin has to agree with anyway. See [`proxy`].
//!
//! # What the macros guarantee
//!
//! * **Panics never reach C.** Every primitive body runs inside
//!   [`std::panic::catch_unwind`]; a panic becomes a clean primitive failure
//!   instead of unwinding into the interpreter, which would be undefined
//!   behaviour.
//! * **Failures are ordinary `Result`s.** `Err(PrimErr::BadArgument)` calls
//!   `primitiveFailFor` with the right code, so the image's Smalltalk fallback
//!   code sees what it expects.
//! * **The accessor-depth byte is emitted for you.** Forgetting it in C gets
//!   you a silent `-1` and a partial read barrier that does not walk far
//!   enough; see [`macro@pharo_primitive`].
//!
//! # Building
//!
//! A plugin is a `cdylib` named after the module:
//!
//! ```toml
//! [lib]
//! name = "MyPlugin"
//! crate-type = ["cdylib"]
//! ```
//!
//! Drop the resulting `libMyPlugin.so` (`.dylib`, `.dll`) next to the `pharo`
//! executable, or anywhere else on the VM's plugin search path.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod error;
#[cfg(feature = "dylib")]
pub mod dylib;
pub mod handles;
pub mod interp;
pub mod proxy;
pub mod ret;

pub use error::{PrimErr, PrimResult};
pub use handles::Registry;
pub use interp::{Interp, Oop, StackArg, StackArgs};
pub use proxy::{sqInt, VirtualMachine};
pub use ret::IntoReturn;

pub use pharo_vm_plugin_macros::pharo_primitive;

/// Machinery the macros expand into. Not a stable API.
#[doc(hidden)]
pub mod __private {
    use core::ptr;
    use core::sync::atomic::{AtomicPtr, Ordering};

    use crate::error::PrimErr;
    use crate::interp::Interp;
    use crate::proxy::{sqInt, VirtualMachine};
    use crate::ret::IntoReturn;

    /// The proxy table, stored by `setInterpreter` before any primitive runs.
    ///
    /// An atomic rather than a `static mut`: the write happens once during
    /// module initialisation and every read is on the interpreter thread, but
    /// an atomic states that plainly and avoids the unsound-`static mut`
    /// footgun.
    pub static INTERP: AtomicPtr<VirtualMachine> = AtomicPtr::new(ptr::null_mut());

    /// Records the proxy table. Called from the generated `setInterpreter`.
    pub fn set_interpreter(vt: *mut VirtualMachine) -> sqInt {
        if vt.is_null() {
            return 0;
        }
        INTERP.store(vt, Ordering::Release);
        1
    }

    /// Runs a primitive body with the stored proxy, containing panics.
    ///
    /// This is the single place where Rust meets the interpreter, so it is the
    /// single place that has to get the failure story right:
    ///
    /// * no proxy yet (primitive somehow called before `setInterpreter`)
    ///   -> nothing we can even fail through, so answer 0 and leave the stack
    ///   alone;
    /// * `Err(code)` -> `primitiveFailFor(code)`;
    /// * panic -> `primitiveFailFor(GenericFailure)`, never an unwind into C.
    pub fn run_primitive<T, F>(body: F) -> sqInt
    where
        T: IntoReturn,
        F: FnOnce(&Interp) -> Result<T, PrimErr> + core::panic::UnwindSafe,
    {
        let vt = INTERP.load(Ordering::Acquire);
        if vt.is_null() {
            return 0;
        }
        // SAFETY: non-null, and set_interpreter only ever stores the VM's own
        // process-lifetime proxy table.
        let vm = unsafe { Interp::from_raw(vt) };

        let outcome = std::panic::catch_unwind(move || match body(&vm) {
            Ok(value) => value.into_return(&vm),
            Err(code) => Err(code),
        });

        match outcome {
            Ok(Ok(())) => 1,
            Ok(Err(code)) => {
                vm.fail_for(code);
                0
            }
            Err(_) => {
                // A panic already printed its message via the default hook.
                // Turn it into an ordinary primitive failure: the image will
                // run its fallback code, which is far kinder than aborting the
                // VM under the user's image.
                vm.fail_for(PrimErr::GenericFailure);
                0
            }
        }
    }
}

/// Declares the module-level exports every plugin must provide.
///
/// Emits `getModuleName` and `setInterpreter`. The name **must** match the
/// module name used in the image's `<primitive:module:>` pragma and the
/// library's file name, because the VM verifies it: `callInitializersIn` in
/// `src/common/sqNamedPrims.c` compares what `getModuleName` answers against
/// the module it was asked to load and rejects the library on a mismatch.
///
/// ```ignore
/// pharo_plugin!("MyPlugin");
/// ```
///
/// To run code when the module loads or unloads, name the hooks. Each is a
/// `fn() -> bool`; answering `false` from the init hook makes the VM reject
/// the module, which is how a plugin declines to load on an unsupported
/// platform.
///
/// ```ignore
/// pharo_plugin!("MyPlugin", init = my_init, shutdown = my_shutdown);
/// ```
#[macro_export]
macro_rules! pharo_plugin {
    ($name:literal $(, init = $init:path)? $(, shutdown = $shutdown:path)?) => {
        $crate::pharo_plugin!(@common $name);
        $($crate::pharo_plugin!(@init $init);)?
        $($crate::pharo_plugin!(@shutdown $shutdown);)?
    };

    (@common $name:literal) => {
        /// Answers this module's compiled-in name, which the VM checks against
        /// the module it asked for.
        #[no_mangle]
        pub extern "C" fn getModuleName() -> *const ::core::ffi::c_char {
            concat!($name, "\0").as_ptr().cast::<::core::ffi::c_char>()
        }

        /// Receives the interpreter proxy table. Answering 0 rejects the load.
        #[no_mangle]
        pub extern "C" fn setInterpreter(
            vt: *mut $crate::VirtualMachine,
        ) -> $crate::sqInt {
            $crate::__private::set_interpreter(vt)
        }
    };

    (@init $init:path) => {
        /// Optional load hook. Answering 0 makes the VM reject this module.
        #[no_mangle]
        pub extern "C" fn initialiseModule() -> $crate::sqInt {
            let hook: fn() -> bool = $init;
            match ::std::panic::catch_unwind(hook) {
                Ok(true) => 1,
                _ => 0,
            }
        }
    };

    (@shutdown $shutdown:path) => {
        /// Optional unload hook.
        #[no_mangle]
        pub extern "C" fn shutdownModule() -> $crate::sqInt {
            let hook: fn() -> bool = $shutdown;
            match ::std::panic::catch_unwind(hook) {
                Ok(true) => 1,
                _ => 0,
            }
        }
    };
}
