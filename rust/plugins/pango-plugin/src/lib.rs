//! PangoPlugin: Pango text layout as Pharo named primitives.
//!
//! See `README.md` for the image-side contract this implements: which
//! primitive corresponds to which `pango_*` entry point, how handles work, and
//! the two conventions the image has to know -- that every integer is in Pango
//! units unless the primitive's name says pixels, and that a `PangoRectangle`
//! crosses as an Array of four SmallIntegers.
//!
//! Unlike cairo-plugin, this one binds a **system** library. Nothing in
//! `cmake/` downloads Pango, so a machine without it is the ordinary case and
//! not an error: [`init`] declines, the VM rejects the module, and the image
//! can tell "no Pango here" from "primitive not implemented". Everything else
//! being absent -- an entry point a newer Pango added -- costs its primitives
//! rather than the module.

#![allow(non_snake_case)] // primitive names follow the image's pragmas
#![deny(unsafe_op_in_unsafe_fn)]

pub mod cairo_bridge;
pub mod ffi;
pub mod font;
pub mod fontmap;
pub mod layout;
pub mod lines;
pub mod markup;
pub mod render;
pub mod resources;
pub mod tabs;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimResult};

pharo_plugin!("PangoPlugin", init = init, shutdown = shutdown);

/// The VM tells every other loaded plugin when one is unloaded, just before it
/// `dlclose`s it (`ioUnloadModule`, `src/common/sqNamedPrims.c:487-517`). Any
/// function pointer this plugin resolved out of that library dangles from this
/// moment, so the cached bridge into CairoPlugin has to go.
///
/// The signature is the VM's own: it calls the symbol as
/// `((sqInt (*) (char *))fn)(entry->name)` (`sqNamedPrims.c:510`), and every C
/// plugin that wants the notification exports exactly that, e.g.
/// `plugins/B2DPlugin/src/common/B2DPlugin.c:536`. `pharo_plugin!` emits no
/// such hook, so it is written out here -- and **getting the arity or the
/// return type wrong is silent**: the VM ignores the answer, and a wrong-arity
/// `extern "C"` simply corrupts the stack on a call that only happens when the
/// image unloads a plugin, which is rare enough that nobody would connect the
/// two.
///
/// The whole body is a `catch_unwind` because this is called from C, and an
/// unwind out of an `extern "C"` function is undefined behaviour. The answer is
/// 1 whatever happens; there is nothing the VM would do differently.
///
/// # Safety
///
/// `module` must be null or a NUL-terminated C string, which is what the VM
/// passes. `unsafe` because of that dereference; the exported C symbol is the
/// same either way, and `clippy::not_unsafe_ptr_arg_deref` is deny-by-default.
#[no_mangle]
pub unsafe extern "C" fn moduleUnloaded(module: *mut core::ffi::c_char) -> sqInt {
    let _ = std::panic::catch_unwind(|| {
        if module.is_null() {
            return;
        }
        // SAFETY: the VM passes its own `ModuleEntry::name`, which it built
        // from the module name it was asked to load and keeps NUL-terminated.
        let name = unsafe { core::ffi::CStr::from_ptr(module) };
        // Compared as bytes rather than as a `str`: a name that is not UTF-8
        // is not CairoPlugin either, and this must not decide otherwise
        // because of how it decoded.
        if name.to_bytes() == b"CairoPlugin" {
            cairo_bridge::forget();
        }
    });
    1
}

/// Loads Pango and glib, and declines the module if either is missing.
///
/// Declining is the point, and it is a stronger point here than in
/// cairo-plugin. Cairo arrives in the bundle; Pango is whatever the machine
/// has. A plugin that loaded anyway would answer `Unsupported` from every
/// primitive, which the image cannot tell from a primitive that is merely
/// unimplemented -- so the image would have no way to decide whether to fall
/// back to its own FFI binding.
///
/// The four reasons to decline are all inside [`ffi::load`]: libpangocairo did
/// not open (it is the handle everything is resolved from); libgobject or
/// libglib did not open, or `g_object_unref`/`g_free` are missing, without
/// which every layout the image makes leaks for the life of the process; or
/// the library that did open exports neither `pango_layout_new` nor
/// `pango_font_description_new`, in which case it is not Pango.
fn init() -> bool {
    ffi::load()
}

/// Releases every layout, context, description, attribute list, tab array and
/// owned font map still registered.
fn shutdown() -> bool {
    resources::release_all();
    true
}

/// Answers whether Pango *and* glib are loaded. Never fails, so the image can
/// ask before committing to this backend.
#[pharo_primitive]
fn primitiveIsAvailable(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(ffi::pango().is_ok() && ffi::glib().is_ok())
}

/// The file the plugin loaded libpangocairo from.
///
/// The diagnostic that turns "it declined" into "it declined and here is where
/// it looked" -- which matters more here than for any other plugin in this
/// tree, because the macOS candidate list is the one thing standing between a
/// machine with Pango installed and a plugin that refuses to load on it.
#[pharo_primitive]
fn primitiveLibraryPath(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(ffi::pango()?.path.clone())
}

/// The file the plugin loaded libglib from.
///
/// Reported separately from [`primitiveLibraryPath`] because "pango loaded,
/// glib did not" is a real state and a confusing one to diagnose from the
/// image with one path between them.
#[pharo_primitive]
fn primitiveGlibLibraryPath(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(ffi::glib()?.path.clone())
}

/// Pango's version as an integer, `major * 10000 + minor * 100 + micro`.
///
/// What the image gates on before asking for anything a newer Pango added.
#[pharo_primitive]
fn primitiveVersion(vm: &Interp) -> PrimResult<i32> {
    vm.expect_argument_count(0)?;
    let p = ffi::pango()?;
    Ok(ffi::pg!(p, pango_version()))
}

/// Pango's version as a string.
#[pharo_primitive]
fn primitiveVersionString(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let p = ffi::pango()?;
    let s = ffi::pg!(p, pango_version_string());
    // SAFETY: `pango_version_string` answers a static NUL-terminated string
    // (pango-utils.h:177, `const char *`, so transfer-none): borrowed, never
    // freed.
    Ok(unsafe { ffi::borrowed_str(s) }.unwrap_or_default())
}

/// `PANGO_SCALE`: how many Pango units make one device unit.
///
/// Answered by asking `pango_units_from_double(1.0)` rather than by returning
/// 1024. `PANGO_SCALE` is a macro with no symbol, the header says it "may be
/// changed in the future", and a hard-coded 1024 anywhere in this crate is a
/// bug waiting for that day. Publishing it lets the image compute rather than
/// hard-code it too.
#[pharo_primitive]
fn primitiveScale(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let p = ffi::pango()?;
    Ok(ffi::pg!(p, pango_units_from_double(1.0)) as sqInt)
}

/// Converts device units to Pango units, rounding to the nearest.
#[pharo_primitive]
fn primitiveUnitsFromDouble(_vm: &Interp, value: f64) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    Ok(ffi::pg!(p, pango_units_from_double(value)) as sqInt)
}

/// Converts Pango units to device units.
#[pharo_primitive]
fn primitiveUnitsToDouble(_vm: &Interp, units: sqInt) -> PrimResult<f64> {
    let p = ffi::pango()?;
    Ok(ffi::pg!(
        p,
        pango_units_to_double(resources::as_c_int(units)?)
    ))
}

/// Entry points this build asked for and the installed Pango does not export,
/// as an Array of Strings, both tables together.
///
/// Non-empty is not by itself a fault: Pango is a system library and this
/// table declares entries as recent as 1.58. What is a fault is a name in here
/// that [`ffi::entries_introduced_after`] does not excuse for the running
/// version -- that is a typo in the table, and it looks exactly like an old
/// Pango from every other angle.
#[pharo_primitive]
fn primitiveMissingEntryPoints(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let mut missing: Vec<String> = Vec::new();
    if let Ok(p) = ffi::pango() {
        missing.extend(p.missing_entry_points().iter().map(|s| (*s).to_owned()));
    }
    if let Ok(g) = ffi::glib() {
        missing.extend(g.missing_entry_points().iter().map(|s| (*s).to_owned()));
    }
    resources::string_array(vm, &missing)
}

/// How many font maps, contexts, layouts, font descriptions, attribute lists
/// and tab arrays the image is holding, as an Array of six integers, in that
/// order.
///
/// For leak-hunting from the image side, the way
/// `primitiveRetainedPinCount` serves cairo-plugin. A count that only grows
/// across a workload means the image is dropping handles without destroying
/// them.
#[pharo_primitive]
fn primitiveLiveResourceCounts(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let counts = resources::live_counts();
    let array = vm.instantiate(vm.class_array()?, 6)?;
    for (i, n) in counts.iter().enumerate() {
        let oop = vm.integer_checked(sqInt::try_from(*n).unwrap_or(sqInt::MAX))?;
        vm.store_pointer(sqInt::try_from(i).unwrap_or(0), array, oop)?;
    }
    Ok(array)
}
