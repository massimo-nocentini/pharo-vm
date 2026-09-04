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
| `isize` / `i32` | a SmallInteger, or a LargeInteger when it does not fit |
| `Handle<R>` | the SmallInteger the image holds the resource by |
| `bool` | `true` / `false` |
| `f64` | a Float |
| `&str` / `String` | a String |
| `Vec<u8>` | a ByteArray of those bytes (empty vector → empty ByteArray, not nil) |
| `Option<T>` | what `T` answers, or `nil` for `None` |

The rows that allocate — the integers that do not fit a SmallInteger, `&str`,
`String`, `Vec<u8>`, and `Some` of any of those — can fail with
`PrimErr::NoMemory`, and that happens *after* your body has returned. The VM's
answer to `PrimErrNoMemory` from an external primitive is to collect and **run
the primitive again** from the top, so a body that changed Rust-side state and
then failed to allocate its answer is re-entered against the state it already
changed. Allocate first, mutate last; the `IntoReturn` docs spell out the
alternatives when the order cannot be arranged.

## Reading and writing what you were handed

`bytes_of` and `words_of` borrow an argument's contents straight out of the
object — no copy — for as long as you do not allocate. The write path has a
matching pair:

```rust
vm.with_words_mut(bits_oop, |bits| {             // the object's own words
    pack_row(&pixels, &cfg, bits);               // filled in place
    Ok(())
})??;
```

`with_bytes_mut` / `with_words_mut` scope a `&mut` slice to a closure, which
is what `write_bytes` deliberately does not hand out: two live views of one
object would alias, and the image can pass the same object as two arguments
whenever it likes. So while the view is live, every other route into those
bytes — `bytes_of`, `words_of`, `write_bytes`, `write_words`,
`indexable_bytes_ptr`, a second view — fails with `PrimErr::Inappropriate`.
That check is by address range, so views of different objects nest freely.

The order matters: **take the mutable view before reading the other
arguments.** A source read inside the view fails cleanly when it is the
destination; a source read taken *before* the view is a borrow the SDK cannot
see, and the rule against holding one across an allocation applies to holding
one across a mutable view too.

`write_bytes` and `write_words` remain the right answer when you have a Rust
buffer to deliver (`memcpy` either way), or when a primitive must not touch
the object until it knows it has succeeded — a staged buffer is how you get
"all or nothing" out of a computation that can fail halfway.

Two smaller readers worth knowing: `read_f64_array::<N>` answers a stack array
where `read_f64s` allocates a `Vec` (a point, a matrix, a set of extents is
nearly always a fixed `N`), and `c_string_value` builds the `CString` a
foreign call wants straight from the object's bytes, without the `String` that
`CString::new(vm.string_value(oop)?)` puts in between.

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

## Wrapping a foreign library

Two optional pieces for a plugin that fronts something other than the image.

**`handles::Registry<R>`** keeps the resource on the Rust side and gives the
image an integer. That integer is a `Handle<R>`, and it carries four fields:

| field | what it catches | failure |
|---|---|---|
| slot index | nothing on its own — it is the address | |
| generation | a handle on a **destroyed** resource | `NotFound` |
| type tag | a handle from **another registry in this same library** | `BadArgument` |
| session byte | a handle the image **saved and replayed in a later run** | `NotFound` |

```rust
static SURFACES: Registry<Surface> = Registry::new();
static CONTEXTS: Registry<Context> = Registry::new();

// One invocation per plugin, next to the statics. The macro proves the tags
// are pairwise distinct and non-zero at compile time.
resource_tags! { Surface = 1, Context = 2 }

let handle = CONTEXTS.insert(Context::new()?)?;   // hand this to the image
CONTEXTS.with(handle, |ctx| ctx.paint())?;        // and take it back later
drop(CONTEXTS.remove(handle)?);                   // destroyed exactly once
```

Without the tag every registry in a library shares one encoding, so the first
insert into each answers the *same* integer and a context handle passed to a
surface primitive **resolves**. One library is the scope the check covers,
because `Handle::decode` compares against the `R::TAG` of a type belonging to
the library running it — not because two `dlopen`ed libraries cannot exchange a
handle. They can: PangoPlugin forwards a CairoPlugin context handle across the
bridge without decoding it, and CairoPlugin decodes it against its own tags.
Two plugins that pick the same literal for different kinds *can* therefore
confuse one another at such a bridge, and CairoPlugin and PangoPlugin both use
2; see the `handles` module docs for the scope of that.

`Handle::decode` is the only route from a bare `sqInt` to a typed handle — keep
it to the handful of accessor functions that front your registries, and let
primitives take a `Handle<Context>` argument or answer a
`PrimResult<Handle<Surface>>` from there on; `StackArg` and `IntoReturn` do the
rest, and the tag is checked while the stack slot is read, before any registry
is locked. `is_live` keeps its `sqInt` parameter, because that is the primitive
the image calls to ask *about* an integer — and it now answers `false` for a
live resource of the wrong kind.

Handles always fit in a SmallInteger, and 0 is never one. `remove_where` takes
out every live resource matching a predicate and answers them, for a library
whose objects own each other and whose destroy call invalidates entries the
image still holds handles on — it sweeps by slot index rather than by handle so
that a slot with no encodable handle is swept too.

Two limits, stated plainly. **A 32-bit image gets no session byte and a 4-bit
tag** (14/12/4/0 against 64-bit's 24/20/8/8), so a handle saved across a
snapshot is not detected there, and a registry can mint 2^14 * (2^12 - 1) =
67,092,480 handles for the whole life of the process — after which `insert`
answers `LimitExceeded` for that registry **forever**, because a slot that has
reached the top generation is retired rather than refilled and destroying
resources wins none of the budget back. It is a lifetime, not a rate. And the
session byte is the low byte of a time-derived id, so detection is 255/256, not
certainty.

**`dylib`** (behind the `dylib` feature, which is off by default so the crate
otherwise has no dependencies) opens a library the VM bundle ships beside the
executable:

```rust
let (lib, path) = unsafe { dylib::open_first(&dylib::library_names("cairo", "2")) }?;
let lib = dylib::leak(lib);
let create: unsafe extern "C" fn(...) = unsafe { dylib::symbol(lib, "cairo_create") }?;
```

It looks in the executable's directory first and falls back to the system
loader, so a bundled copy wins. Do this in the plugin's `init` hook and answer
`false` when the library is not there: the VM then rejects the module, and the
image can tell "not available on this VM" from "primitive not implemented".

`rust/plugins/cairo-plugin` and `rust/plugins/sdl3-plugin` are both built this
way.

## Rules the SDK enforces, and the ones it cannot

**Enforced for you:**

- *Panics never reach C.* Every primitive body runs in `catch_unwind`; a panic
  becomes a clean primitive failure. Letting a panic unwind into the
  interpreter would be undefined behaviour.

  This holds only if the cdylib is built to unwind, and that is a property of
  the **workspace root**, not of the crate: cargo ignores a `[profile]` in a
  member. In-tree plugins get it from `rust/plugins/Cargo.toml`; out of tree,
  simply do not set `panic = "abort"` — the default is what the promise needs.
  Under abort there is nothing to catch and the process dies instead, which is
  what this SDK's own plugins did until the workspace was split.
- *A panic that tore shared state disables the module instead of continuing.*
  `Registry` holds its mutex across your closure, so a panic in there leaves a
  slot half-written. Rather than swallow the mutex poison, the registry refuses
  every later call with `PrimErr::Unsupported`, and a module-wide flag — set
  from a panic hook that can see a critical section was open — makes every
  other primitive in that cdylib fail the same way, for the life of the
  process. There is no reset: nothing in the image can re-establish an
  invariant it cannot see. `shutdownModule` still runs, but a poisoned
  `Registry::drain` answers nothing, so a shutdown hook leaks rather than
  freeing out of a table nobody can trust.

  The flag only sees state a `Registry` owns. For shared mutable state of your
  own, open a `pharo_vm_plugin::Section` around the mutation and it is covered
  too. See the `poison` module.
- *The accessor-depth byte exists.*
- *`write_bytes` refuses immutable objects*, so you cannot quietly write
  through Pharo's immutability, and bounds-checks the write. So do
  `write_words`, `with_bytes_mut` and `with_words_mut`.
- *Two live views of one object are impossible.* An in-place view makes every
  other reader and writer of those bytes fail while it lasts, so one object
  passed as two arguments cannot alias a `&mut`.
- *Integers are range-checked on the way out.* The proxy's
  `methodReturnInteger` tags without checking (`(v << 3) | 1`), so a value past
  `SmallInteger maxVal` silently wraps; return an `isize` and anything too big
  is boxed as a LargeInteger instead.

**Still on you:**

- **Do not hold a borrow across an allocation.** `bytes_of` borrows straight
  into the object. Allocating — `vm.instantiate`, `vm.string` — can move it.
  Read, then allocate; never interleave. The same goes for a borrow held
  across a `with_*_mut` view of the same object: the lending table only knows
  about the views it hands out, so take the view first.
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
