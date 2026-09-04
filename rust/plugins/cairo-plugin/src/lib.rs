//! CairoPlugin: Cairo as Pharo named primitives.
//!
//! See `README.md` for the image-side contract this implements: which
//! primitive corresponds to which `cairo_*` entry point, how handles work,
//! and what the image must do differently from the FFI binding it replaces.

#![allow(non_snake_case)] // primitive names follow the image's pragmas
#![deny(unsafe_op_in_unsafe_fn)]

pub mod bridge;
pub mod context;
pub mod ffi;
pub mod pattern;
pub mod resources;
pub mod surface;
pub mod text;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, PrimResult};

pharo_plugin!("CairoPlugin", init = init, shutdown = shutdown);

/// Loads Cairo, and declines the module if it is not there.
///
/// Declining is the point: a VM whose bundle has no Cairo leaves the image
/// free to fall back to its FFI binding. A plugin that loaded anyway would
/// answer `Unsupported` from every primitive, which the image cannot tell from
/// a primitive that is merely unimplemented.
fn init() -> bool {
    ffi::load()
}

/// Releases every context, pattern and surface still registered.
fn shutdown() -> bool {
    resources::release_all();
    true
}

/// Answers whether Cairo is loaded. Never fails, so the image can ask before
/// committing to this backend.
#[pharo_primitive]
fn primitiveIsAvailable(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(ffi::cairo().is_ok())
}

/// The file the plugin loaded Cairo from.
#[pharo_primitive]
fn primitiveLibraryPath(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(ffi::cairo()?.path.clone())
}

/// Cairo's version as an integer, `major * 10000 + minor * 100 + micro`.
#[pharo_primitive]
fn primitiveVersion(vm: &Interp) -> PrimResult<i32> {
    vm.expect_argument_count(0)?;
    let c = ffi::cairo()?;
    Ok(ffi::cc!(c, cairo_version()))
}

/// Cairo's version as a string.
#[pharo_primitive]
fn primitiveVersionString(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let c = ffi::cairo()?;
    let s = ffi::cc!(c, cairo_version_string());
    // SAFETY: cairo_version_string answers a static NUL-terminated string.
    Ok(unsafe { ffi::owned_str(s) })
}

/// A `cairo_status_t` as the message Cairo prints for it.
#[pharo_primitive]
fn primitiveStatusToString(_vm: &Interp, status: isize) -> PrimResult<String> {
    let c = ffi::cairo()?;
    let s = ffi::cc!(c, cairo_status_to_string(resources::as_c_int(status)?));
    // SAFETY: cairo_status_to_string answers a static NUL-terminated string,
    // for an out-of-range status too.
    Ok(unsafe { ffi::owned_str(s) })
}

/// How many surfaces, contexts and patterns the image is holding, as an Array
/// of three integers. For leak-hunting from the image side.
#[pharo_primitive]
fn primitiveLiveResourceCounts(vm: &Interp) -> PrimResult<pharo_vm_plugin::Oop> {
    vm.expect_argument_count(0)?;
    let counts = [
        resources::SURFACES.len(),
        resources::CONTEXTS.len(),
        resources::PATTERNS.len(),
    ];
    let array = vm.instantiate(vm.class_array()?, 3)?;
    for (i, n) in counts.iter().enumerate() {
        let oop = vm.integer_checked(isize::try_from(*n).unwrap_or(isize::MAX))?;
        vm.store_pointer(isize::try_from(i).unwrap_or(0), array, oop)?;
    }
    Ok(array)
}

/// How many pins are being held past their surface's destruction.
///
/// Anything but zero means the image destroyed a surface while a context or
/// pattern still referenced it; see `resources::RETAINED_PINS`.
#[pharo_primitive]
fn primitiveRetainedPinCount(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(isize::try_from(resources::retained_pin_count()?).unwrap_or(isize::MAX))
}
