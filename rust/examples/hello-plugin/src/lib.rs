//! The smallest useful Pharo VM plugin: one primitive that answers a String.
//!
//! Where `examples/uuid-plugin` shows a real-world port, this one shows the
//! floor: how little it takes to hand an object from Rust to the image. The
//! image side declares
//!
//! ```smalltalk
//! <primitive: 'primitiveHelloString' module: 'HelloPlugin'>
//! ```
//!
//! and `hello.st` next to this crate is a ready-to-run driver script.

// The crate is named for the shared library the VM loads (libHelloPlugin.so),
// which fixes its spelling.
#![allow(non_snake_case)]

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, PrimResult};

pharo_plugin!("HelloPlugin");

/// What the primitive answers. A constant so the test below can pin it.
const GREETING: &str = "Hello from Rust!";

/// Answers a freshly allocated Smalltalk String.
///
/// Returning `&str` is enough: the SDK's `IntoReturn` impl asks the VM to
/// allocate a String with these bytes and answers that object. The primitive
/// never reads its receiver or arguments, so the accessor depth is -1.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveHelloString(vm: &Interp) -> PrimResult<&'static str> {
    vm.expect_argument_count(0)?;
    Ok(GREETING)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Interp::string` takes a C string, so an interior NUL would turn the
    /// answer into a primitive failure at runtime. Catch that at test time.
    #[test]
    fn greeting_has_no_interior_nul() {
        assert!(!GREETING.as_bytes().contains(&0));
    }
}
