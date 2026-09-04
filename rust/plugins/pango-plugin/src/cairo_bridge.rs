//! Reaching CairoPlugin's contexts.
//!
//! Rendering a `PangoLayout` needs a `cairo_t *`, and the image's contexts live
//! in CairoPlugin's registry inside a different shared library. This module is
//! the consumer half of the bridge CairoPlugin exports (see its
//! `src/bridge.rs`): the symbol is resolved once through the VM's
//! `ioLoadFunctionFrom`, the pointer is borrowed for exactly one call, and the
//! two libraries are checked to be talking to the same Cairo before anything is
//! drawn.
//!
//! That last check is not paranoia. `libpangocairo` is linked against a Cairo of
//! its own -- on macOS by absolute install name,
//! `/opt/homebrew/opt/cairo/lib/libcairo.2.dylib` -- while CairoPlugin dlopens
//! whichever Cairo the VM bundle carries, whose install name is
//! `@executable_path/Plugins/libcairo.2.dylib` and whose bytes are measurably a
//! different binary. Two copies of Cairo in one process have separate private
//! statics and (across versions) different internal struct layouts, so a
//! `cairo_t *` created by one and drawn on by the other is undefined behaviour
//! that presents as intermittent corruption inside Cairo. Comparing the address
//! of `cairo_create` as each side resolved it costs one integer compare and
//! turns that into a clean `Unsupported`.
//!
//! It is expected that the compare *fails* on a homebrew macOS machine running
//! a bundled Cairo, and that is the intended outcome rather than a bug: Pango
//! text does not draw there until a Pango built against the bundled Cairo is
//! shipped. Refusing beats corrupting, and
//! [`primitiveCairoBridgeStatus`] names both libraries so the deployment
//! problem is legible from the image instead of being guessed at.
//!
//! Nothing here is a link-time `extern "C" { }` import, and it must never
//! become one. Plugins are dlopened `RTLD_NOW|RTLD_GLOBAL`, so an unresolved
//! symbol would make PangoPlugin fail to load *entirely* on a VM without
//! CairoPlugin -- turning an optional integration into a hard dependency.

use core::ffi::{c_char, c_int, c_void};
use core::marker::PhantomData;
use std::sync::Mutex;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, PrimErr, PrimResult};

use crate::ffi::pango;

/// The module name the VM knows CairoPlugin by. Also its library file's stem.
const CAIRO_MODULE: &str = "CairoPlugin";

/// The bridge ABI this plugin speaks.
const BRIDGE_ABI: u32 = 1;

// The exported names. Versioned, so an incompatible CairoPlugin fails to
// resolve rather than being called with a struct it does not recognise.
const SYM_BORROW: &str = "cairoPluginBorrowContext_v1";
const SYM_STATUS: &str = "cairoPluginContextStatus_v1";
const SYM_IDENTITY: &str = "cairoPluginCairoIdentity_v1";
const SYM_PATH: &str = "cairoPluginCairoPath_v1";

/// CairoPlugin's `CairoBridgeContextV1`, declared a second time here.
///
/// Nothing links this declaration to CairoPlugin's; they agree because both are
/// transcribed from the documented ABI and both assert their own size. Changing
/// one without the other is the failure mode, which is why the callee writes
/// `size` and `abi` and this side checks them.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct BridgeContextV1 {
    size: u32,
    abi: u32,
    cr: *mut c_void,
    cairo_identity: *mut c_void,
    status: c_int,
    reserved: c_int,
}

// The wire format, asserted rather than assumed, on this side too. If padding
// or a field width ever moved on either side the two declarations would
// disagree silently, and this borrower would read `cr` out of the middle of
// `cairo_identity` -- a plausible-looking pointer into a live Cairo struct.
const _: () = assert!(core::mem::size_of::<BridgeContextV1>() == 32);
const _: () = assert!(core::mem::align_of::<BridgeContextV1>() == 8);

type BorrowFn = unsafe extern "C" fn(sqInt, *mut BridgeContextV1, u32) -> c_int;
type StatusFn = unsafe extern "C" fn(sqInt) -> c_int;
type IdentityFn = unsafe extern "C" fn() -> *mut c_void;
type PathFn = unsafe extern "C" fn() -> *const c_char;

/// The resolved bridge. Function pointers only, so `Copy` and `Send`.
#[derive(Clone, Copy)]
struct Bridge {
    borrow: BorrowFn,
    status: Option<StatusFn>,
    /// The Cairo both sides agreed on, kept so every later borrow can be
    /// checked against it for one integer compare.
    identity: *mut c_void,
}

// SAFETY: the fields are code addresses in a library that is never unloaded
// while this value is live -- `moduleUnloaded` drops it first -- and the
// identity is a token that is compared, never dereferenced. The VM runs
// primitives on one thread; `Send` is asserted only so this can live in a
// `static Mutex`.
unsafe impl Send for Bridge {}

/// Why the bridge is unavailable, in words the image can print.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Absent {
    /// `ioLoadFunctionFrom` answered 0 for the module itself.
    NoModule,
    /// CairoPlugin is there but exports no v1 bridge -- too old, or too new.
    NoBridge,
    /// CairoPlugin has no Cairo, so there is nothing to borrow.
    NoCairo,
    /// Two copies of Cairo are mapped. Refusing on purpose; see the module doc.
    DifferentCairo,
    /// This plugin has no pangocairo, so there is nothing to check against.
    NoPango,
}

impl Absent {
    /// The reason on its own, with nothing that has to be looked up.
    const fn message(self) -> &'static str {
        match self {
            Self::NoModule => "CairoPlugin is not installed",
            Self::NoBridge => "CairoPlugin exports no v1 context bridge",
            Self::NoCairo => "CairoPlugin loaded no Cairo",
            Self::DifferentCairo => {
                "CairoPlugin and libpangocairo are bound to different copies of Cairo"
            }
            Self::NoPango => "libpangocairo did not load",
        }
    }

    /// The reason, with the two library paths appended when the reason is that
    /// they disagree.
    ///
    /// A bare "different copies of Cairo" tells the person reading it nothing
    /// they can act on; the two file names tell them exactly which build has to
    /// change, and this is the only place in the process that knows both.
    fn describe(self, vm: &Interp) -> String {
        if self != Self::DifferentCairo {
            return self.message().to_owned();
        }
        let ours = pango().map_or_else(|_| String::new(), |p| p.path.clone());
        let theirs = cairo_plugin_library_path(vm);
        let mut out = self.message().to_owned();
        out.push_str(": libpangocairo ");
        out.push_str(if ours.is_empty() { "(unknown)" } else { &ours });
        if let Some(v) = our_cairo_version() {
            out.push_str(&format!(
                " uses cairo {}.{}.{}",
                v / 10000,
                (v / 100) % 100,
                v % 100
            ));
        }
        out.push_str(", CairoPlugin ");
        out.push_str(if theirs.is_empty() {
            "(unknown)"
        } else {
            &theirs
        });
        out
    }
}

/// `cairo_version` as libpangocairo resolves it, for the diagnostic only.
///
/// Reads the table entry directly rather than through [`crate::ffi::pg`],
/// because this is the one place in the crate that *wants* a missing entry to
/// be silent: a diagnostic string that fails to be produced because the
/// version accessor is absent would replace the useful half of the message
/// with nothing.
fn our_cairo_version() -> Option<c_int> {
    let f = pango().ok()?.cairo_version?;
    // SAFETY: the pointer came out of the libpangocairo this plugin loaded and
    // `cairo_version` takes no arguments and answers an int -- the signature
    // `ffi.rs` declares from cairo-version.h.
    Some(unsafe { f() })
}

enum State {
    /// Not looked for yet.
    Unresolved,
    /// Looked for and not usable. Cached: `ioLoadFunctionFrom` searches every
    /// plugin path on every miss, which is far too expensive per primitive.
    /// Cleared by `moduleUnloaded` and by `primitiveCairoBridgeRefresh`.
    Absent(Absent),
    Ready(Bridge),
}

static BRIDGE: Mutex<State> = Mutex::new(State::Unresolved);

fn state() -> std::sync::MutexGuard<'static, State> {
    BRIDGE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Forgets whatever was resolved, so the next call looks again.
///
/// Called from `moduleUnloaded` when CairoPlugin goes away -- the VM
/// `dlclose`s it (`src/common/sqNamedPrims.c:517`), so every pointer taken out
/// of it dangles -- and from the image, to retry after installing it.
pub fn forget() {
    *state() = State::Unresolved;
}

/// Why the bridge is unavailable, or `None` when it is available.
///
/// Resolves it if that has not been tried yet, so this is also how the image
/// asks "will Pango-on-Cairo work?" before committing to it.
#[must_use]
pub fn unavailable_because(vm: &Interp) -> Option<String> {
    match bridge(vm) {
        Ok(_) => None,
        Err(reason) => Some(reason.describe(vm)),
    }
}

/// The file CairoPlugin loaded Cairo from, for a diagnostic. Empty if unknown.
#[must_use]
pub fn cairo_plugin_library_path(vm: &Interp) -> String {
    let Ok(address) = vm.load_function_from(SYM_PATH, CAIRO_MODULE) else {
        return String::new();
    };
    // SAFETY: the symbol is CairoPlugin's `cairoPluginCairoPath_v1`, whose
    // signature is `const char *(void)` and which answers a pointer that stays
    // valid for the life of the process -- CairoPlugin keeps the `CString` in a
    // `OnceLock` precisely so no cross-library `free` is ever needed.
    let path = unsafe {
        let f: PathFn = core::mem::transmute::<*mut c_void, PathFn>(address);
        f()
    };
    if path.is_null() {
        return String::new();
    }
    // SAFETY: non-null and NUL-terminated by that function's contract.
    unsafe { core::ffi::CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned()
}

/// Resolves the bridge, once, and answers a copy of it.
///
/// Copied out rather than borrowed so the registry lock is not held across the
/// Pango call.
fn bridge(vm: &Interp) -> Result<Bridge, Absent> {
    let mut guard = state();
    match &*guard {
        State::Ready(b) => return Ok(*b),
        State::Absent(why) => return Err(*why),
        State::Unresolved => {}
    }
    let resolved = resolve(vm);
    *guard = match resolved {
        Ok(b) => State::Ready(b),
        Err(why) => State::Absent(why),
    };
    resolved
}

fn resolve(vm: &Interp) -> Result<Bridge, Absent> {
    // The Cairo `libpangocairo` will actually call. Resolved out of the
    // pangocairo handle, so this is that library's own dependency, not
    // whatever a fresh search would find.
    let ours = pango().map_err(|_| Absent::NoPango)?;
    let ours = ours
        .cairo_create
        .map(|f| f as *const () as *mut c_void)
        .ok_or(Absent::NoPango)?;

    // Distinguishes "no CairoPlugin" from "CairoPlugin without the bridge",
    // which `load_function_from` alone cannot: both answer NotFound.
    if !vm.module_is_loadable(CAIRO_MODULE).unwrap_or(false) {
        return Err(Absent::NoModule);
    }

    let borrow = vm
        .load_function_from(SYM_BORROW, CAIRO_MODULE)
        .map_err(|_| Absent::NoBridge)?;
    let identity = vm
        .load_function_from(SYM_IDENTITY, CAIRO_MODULE)
        .map_err(|_| Absent::NoBridge)?;

    // SAFETY: these are CairoPlugin's exported bridge entry points and the
    // signatures are the ones its `src/bridge.rs` declares. The `_v1` in each
    // name is what makes that a checkable claim rather than a hope: a
    // CairoPlugin with a different struct exports `_v2`, so this resolve fails
    // instead of calling with a shape the callee does not recognise.
    let borrow: BorrowFn = unsafe { core::mem::transmute::<*mut c_void, BorrowFn>(borrow) };
    // SAFETY: as above.
    let identity: IdentityFn = unsafe { core::mem::transmute::<*mut c_void, IdentityFn>(identity) };
    // SAFETY: as above -- `cairoPluginCairoIdentity_v1` takes no arguments,
    // answers a token address, and cannot fail other than by answering null.
    let theirs = unsafe { identity() };
    if theirs.is_null() {
        return Err(Absent::NoCairo);
    }
    if theirs != ours {
        // Two copies of Cairo. See the module documentation: refusing is the
        // only safe answer, and it is a deployment problem with a real fix.
        return Err(Absent::DifferentCairo);
    }

    // Optional: a CairoPlugin that has the borrow but not the status accessor
    // still works, it just cannot report a latched error afterwards.
    let status = vm
        .load_function_from(SYM_STATUS, CAIRO_MODULE)
        .ok()
        // SAFETY: as above.
        .map(|a| unsafe { core::mem::transmute::<*mut c_void, StatusFn>(a) });

    Ok(Bridge {
        borrow,
        status,
        identity: theirs,
    })
}

/// A `cairo_t *` borrowed for the body of one closure.
///
/// The lifetime is the only thing keeping the pointer from being stored, so it
/// is not decoration: `as_ptr` hands out a raw pointer, and letting that escape
/// the closure would need a `static` and would be a bug. Nothing in this crate
/// does it, and the borrow is re-taken on every primitive.
pub struct CairoRef<'a> {
    ptr: *mut c_void,
    _borrow: PhantomData<&'a ()>,
}

impl CairoRef<'_> {
    /// The raw `cairo_t *`, for the duration of this closure and no longer.
    #[must_use]
    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

/// Runs `f` on the `cairo_t *` CairoPlugin's `handle` names.
///
/// The one route to a Cairo context in this plugin. Fails with:
///
/// * `Unsupported` -- no CairoPlugin, no bridge, or two copies of Cairo. Ask
///   [`primitiveCairoBridgeStatus`] which.
/// * `NotFound` -- the handle is not a live CairoPlugin context. Includes a
///   handle destroyed since the image last used it: the borrow re-resolves it
///   every time.
/// * `Inappropriate` -- the context has already latched a Cairo error, so
///   drawing on it would silently do nothing.
/// * `OperationFailed` -- the drawing latched an error. Reported only when
///   CairoPlugin exports the status accessor.
pub fn with_cairo_context<R>(
    vm: &Interp,
    handle: sqInt,
    f: impl FnOnce(CairoRef<'_>) -> PrimResult<R>,
) -> PrimResult<R> {
    let bridge = bridge(vm).map_err(|_| PrimErr::Unsupported)?;

    let mut out = core::mem::MaybeUninit::<BridgeContextV1>::uninit();
    let size = core::mem::size_of::<BridgeContextV1>() as u32;
    // SAFETY: `out` is writable and aligned for the struct, and `size` is its
    // real size -- the two things `cairoPluginBorrowContext_v1` asks of a
    // caller. It writes the struct whole or not at all, so nothing here reads
    // `out` before the answer says it was filled.
    let filled = unsafe { (bridge.borrow)(handle, out.as_mut_ptr(), size) };
    if filled != 1 {
        return Err(PrimErr::NotFound);
    }
    // SAFETY: the callee answered 1, so it wrote the whole struct.
    let out = unsafe { out.assume_init() };

    // Belt and braces against a mismatched declaration on either side: the
    // callee stamps what it thinks it wrote.
    if out.abi != BRIDGE_ABI || out.size != size {
        return Err(PrimErr::Unsupported);
    }
    if out.cr.is_null() {
        return Err(PrimErr::NotFound);
    }
    // One compare, every call: if CairoPlugin were somehow reloaded against a
    // different Cairo, this catches it before Pango dereferences the context.
    if out.cairo_identity != bridge.identity {
        forget();
        return Err(PrimErr::Unsupported);
    }
    // Cairo latches errors and then ignores every drawing call. Drawing onto a
    // broken context would answer success and paint nothing, and the image
    // would go looking for the bug in its own layout code.
    if out.status != 0 {
        return Err(PrimErr::Inappropriate);
    }

    let result = f(CairoRef {
        ptr: out.cr,
        _borrow: PhantomData,
    });

    // Did the drawing itself break the context? Asked only on the success path;
    // an error the closure already reported is the more specific one.
    if result.is_ok() {
        if let Some(status) = bridge.status {
            // SAFETY: CairoPlugin's `cairoPluginContextStatus_v1`, signature as
            // declared. Answers -1 for a handle it cannot resolve, which is not
            // 0 and so is reported, correctly, as a failure.
            let after = unsafe { status(handle) };
            if after != 0 {
                return Err(PrimErr::OperationFailed);
            }
        }
    }
    result
}

// ---- diagnostics ---------------------------------------------------------

/// Why Pango cannot draw onto CairoPlugin's contexts, or an empty string when
/// it can. Resolves the bridge if that has not been tried.
///
/// Never fails, so the image can ask before choosing a backend. When the answer
/// is that the two libraries are bound to different copies of Cairo it names
/// both files, because that is a deployment problem with a real fix and the
/// only way to see it is from in here.
#[pharo_primitive]
fn primitiveCairoBridgeStatus(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(unavailable_because(vm).unwrap_or_default())
}

/// The file CairoPlugin loaded Cairo from, for diagnosing a mismatch.
///
/// Empty when CairoPlugin is absent, has no bridge, or never loaded a Cairo.
/// Compare it against `primitiveLibraryPath`'s libpangocairo, whose own Cairo
/// is a hard dependency recorded in that file.
#[pharo_primitive]
fn primitiveCairoPluginLibraryPath(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(cairo_plugin_library_path(vm))
}

/// Forgets a failed bridge lookup so the next call tries again.
///
/// The negative result is cached because a miss costs a search of every plugin
/// path times every name pattern, plus a load attempt of the module; this is
/// the way out when the image has just installed CairoPlugin.
#[pharo_primitive]
fn primitiveCairoBridgeRefresh(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    forget();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Absent, BridgeContextV1, BRIDGE_ABI};

    #[test]
    fn the_borrowed_struct_has_the_layout_cairo_plugin_writes() {
        // Two crates, two hand-written declarations, nothing linking them. The
        // size and alignment are const-asserted above; these check the field
        // offsets, which is what a re-ordering would move without changing
        // either.
        assert_eq!(core::mem::size_of::<BridgeContextV1>(), 32);
        assert_eq!(core::mem::align_of::<BridgeContextV1>(), 8);
        let zero = BridgeContextV1 {
            size: 0,
            abi: 0,
            cr: core::ptr::null_mut(),
            cairo_identity: core::ptr::null_mut(),
            status: 0,
            reserved: 0,
        };
        let base = core::ptr::from_ref(&zero) as usize;
        assert_eq!(core::ptr::from_ref(&zero.size) as usize - base, 0);
        assert_eq!(core::ptr::from_ref(&zero.abi) as usize - base, 4);
        assert_eq!(core::ptr::from_ref(&zero.cr) as usize - base, 8);
        assert_eq!(core::ptr::from_ref(&zero.cairo_identity) as usize - base, 16);
        assert_eq!(core::ptr::from_ref(&zero.status) as usize - base, 24);
        assert_eq!(core::ptr::from_ref(&zero.reserved) as usize - base, 28);
    }

    #[test]
    fn the_abi_this_side_speaks_is_the_one_in_the_symbol_names() {
        // The version lives in the symbol name so that an incompatible
        // CairoPlugin fails to resolve; the constant exists so the check on the
        // struct the callee stamped can say the same thing twice.
        assert_eq!(BRIDGE_ABI, 1);
        assert!(super::SYM_BORROW.ends_with("_v1"));
        assert!(super::SYM_STATUS.ends_with("_v1"));
        assert!(super::SYM_IDENTITY.ends_with("_v1"));
        assert!(super::SYM_PATH.ends_with("_v1"));
    }

    #[test]
    fn every_reason_the_bridge_is_absent_says_something_the_image_can_print() {
        for reason in [
            Absent::NoModule,
            Absent::NoBridge,
            Absent::NoCairo,
            Absent::DifferentCairo,
            Absent::NoPango,
        ] {
            let message = reason.message();
            assert!(!message.is_empty());
            // An empty status string is the image's signal that the bridge
            // works, so no reason may ever produce one.
            assert!(message.len() > 8, "too terse to diagnose: {message}");
        }
    }
}
