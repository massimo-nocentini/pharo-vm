//! Exercises `#[pharo_primitive]` with typed extra parameters.
//!
//! No VM is running here, so the primitives cannot be driven end to end; what
//! this pins down is that the macro accepts the typed form, generates the
//! `StackArgs` extraction, and that the exports keep their contract: a call
//! before `setInterpreter` answers 0, and the accessor-depth byte is emitted.

#![allow(non_snake_case)]

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimResult};

#[pharo_primitive]
fn primitiveTypedArgs(vm: &Interp, form: Oop, quality: sqInt, dither: bool) -> PrimResult<()> {
    let _ = (form, quality, dither);
    vm.return_receiver()
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveBareForm(vm: &Interp) -> PrimResult<()> {
    vm.return_receiver()
}

#[test]
fn typed_primitive_answers_zero_without_an_interpreter() {
    assert_eq!(primitiveTypedArgs(), 0);
    assert_eq!(primitiveBareForm(), 0);
}

#[test]
fn accessor_depth_bytes_are_emitted() {
    assert_eq!(primitiveTypedArgsAccessorDepth, 1);
    assert_eq!(primitiveBareFormAccessorDepth, 0);
}
