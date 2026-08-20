# Writing Pharo VM plugins in Rust

A **plugin** is a shared library the Pharo VM loads on demand to service *named
primitives* — the things an image declares as:

```smalltalk
myMethod
    <primitive: 'primitiveFoo' module: 'MyPlugin'>
    ^ self primitiveFailed
```

This crate lets you write that in Rust, with no C, no Pharo checkout, no CMake
and no bindgen. Add the dependency and go.

## A whole plugin

```rust
use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, PrimResult};

pharo_plugin!("MyPlugin");

#[pharo_primitive]
fn primitiveDoubled(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;   // `doubled` is unary
    Ok(vm.stack_integer(0)? * 2)    // offset 0 is the receiver
}
```

```toml
[lib]
name = "MyPlugin"          # decides the file name AND the module name
crate-type = ["cdylib"]

[dependencies]
pharo-vm-plugin = "0.1"
```

`cargo build --release`, drop `target/release/libMyPlugin.so` next to the
`pharo` executable, and call it:

```smalltalk
Integer compile: 'doubled
    <primitive: ''primitiveDoubled'' module: ''MyPlugin''>
    ^ self primitiveFailed'.

21 doubled.  "=> 42"
```

There is a ready-made skeleton in [`template/`](template/), and a complete
worked example — a drop-in replacement for the C `UUIDPlugin` — in
[`../examples/uuid-plugin`](../examples/uuid-plugin).

## The three names that must agree

The VM checks this, and rejects the library if they disagree
(`callInitializersIn` in `src/common/sqNamedPrims.c`):

| | |
|---|---|
| the `module:` in the image's pragma | `'MyPlugin'` |
| the argument to `pharo_plugin!` | `"MyPlugin"` |
| the `[lib] name` (hence the file name) | `MyPlugin` → `libMyPlugin.so` |

## Arguments and the receiver

Primitive arguments live on the interpreter's stack, addressed by depth:

| method | `argument_count()` | offset 0 | offset 1 |
|---|---|---|---|
| `foo` | 0 | receiver | --- |
| `foo: a` | 1 | `a` | receiver |
| `foo: a bar: b` | 2 | `b` | `a` |

So offset 0 is the *last* argument, or the receiver when there are none, and
`vm.receiver()` fetches the receiver whatever the arity.

Call `vm.expect_argument_count(n)?` first, always. A named primitive can be
installed in a method of any arity --- the image decides, not you --- so the
count is untrusted input. Skip the check and a primitive written for `foo: a`
will happily read the receiver as its argument when installed on `foo`.

## Failing

A Smalltalk primitive does not raise; it *fails*, and the image runs the
method's Smalltalk fallback. Return `Err` and that happens for you, with the
code the image expects:

```rust
// For a one-argument method: Integer>>#thirdOf: aNumber
#[pharo_primitive]
fn primitiveThird(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(1)?;   // wrong arity     -> BadNumArgs
    let n = vm.stack_integer(0)?;   // not an integer  -> BadArgument
    if n % 3 != 0 {
        return Err(PrimErr::Inappropriate);
    }
    Ok(n / 3)
}
```

Validate before you mutate. A primitive that fails halfway through leaves the
receiver in a state the fallback code did not expect, and Smalltalk has no way
to tell that happened.

## What you get back

Return any type implementing `IntoReturn`:

| return type | the image sees |
|---|---|
| `()` | the receiver (Smalltalk's default) |
| `Oop` | that object |
| `isize` / `i32` | a SmallInteger |
| `bool` | `true` / `false` |
| `f64` | a Float |
| `&str` | a String |

## Accessor depth

Spur collects with *lazy forwarding*: `become:` leaves a forwarder behind
instead of scanning the heap, and primitives are expected to fail when they
meet one so the VM can resolve it and retry. To do that, the VM needs to know
how deep into its arguments' object graph the primitive reads. That number is
the **accessor depth**, and it is exported as a byte next to the primitive.

In C you must remember to write `EXPORT(signed char) primitiveFooAccessorDepth
= 1;` by hand; forget it and you silently get `-1`, and a read barrier that
does not walk far enough. `#[pharo_primitive]` always emits it, defaulting to
`1` — enough for a primitive that reads or writes its arguments' contents,
which is nearly all of them. Override when you know better:

```rust
#[pharo_primitive(accessor_depth = 0)]    // touches only the oops themselves
#[pharo_primitive(accessor_depth = -1)]   // does not traverse at all
```

Too high only costs a little work on the failure path; too low risks a spurious
failure against a forwarded argument. The default errs high deliberately.

## Rules the SDK enforces, and the ones it cannot

**Enforced for you:**

- *Panics never reach C.* Every primitive body runs in `catch_unwind`; a panic
  becomes a clean primitive failure. Letting a panic unwind into the
  interpreter would be undefined behaviour.
- *The accessor-depth byte exists.*
- *`write_bytes` refuses immutable objects*, so you cannot quietly write
  through Pharo's immutability, and bounds-checks the write.
- *Integers are range-checked on the way out.* The proxy's
  `methodReturnInteger` tags without checking (`(v << 3) | 1`), so a value past
  `SmallInteger maxVal` silently wraps; return an `isize` and anything too big
  is boxed as a LargeInteger instead.

**Still on you:**

- **Do not hold a borrow across an allocation.** `bytes_of` borrows straight
  into the object. Allocating — `vm.instantiate`, `vm.string` — can move it.
  Read, then allocate; never interleave.
- **Do not call back into the image.** A primitive runs with the interpreter
  mid-flight. Talk to the OS, compute, answer.
- **Keep it prompt.** The VM is blocked while your primitive runs, including
  its garbage collector and its finalization.
- **`longjmp` must not cross a Rust frame.** Relevant only if you link C that
  uses `setjmp`; the VM's own FFI does.

## Naming

The exported symbol defaults to the Rust function's name, so `fn primitiveFoo`
just works and reads the same as the pragma. To keep an idiomatic Rust name,
give the export explicitly:

```rust
#[pharo_primitive(name = "primitiveMakeUUID")]
fn make_uuid(vm: &Interp) -> PrimResult<()> { /* ... */ }
```

## Reaching past the safe API

`Interp` covers the common surface, not all 153 proxy entries. For the rest,
`vm.as_raw()` hands you the `*mut VirtualMachine` and you are in `unsafe`
territory with the same rules C plugins have. If you find yourself doing that
for something ordinary, it probably belongs in `Interp` — patches welcome.

## Checking a plugin loads

The VM logs module resolution at debug level:

```sh
pharo --logLevel=4 --headless my.image st test.st 2>&1 | grep -i module
```

`Failed to load module: MyPlugin` means the VM never found or accepted the
library — wrong directory, wrong file name, or `getModuleName` disagreeing with
the module name.
