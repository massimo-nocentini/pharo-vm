//! Integer handles for resources a plugin owns.
//!
//! A plugin that wraps a foreign library holds things the image has no
//! representation for: a `cairo_t *`, an `SDL_Window *`, a socket. The
//! temptation is to hand the pointer over as an integer and take it back on
//! the next call. That is what the image-side FFI does, and it is why a stale
//! Athens context crashes the VM instead of failing a primitive: nothing on
//! the way in can tell a live pointer from a freed one.
//!
//! A [`Registry`] keeps the resource on the Rust side and gives the image an
//! opaque integer instead. That integer is a [`Handle<R>`](Handle), and it
//! carries four fields rather than one pointer:
//!
//! | field | what it catches |
//! |---|---|
//! | slot index | nothing on its own -- it is the address |
//! | generation | a handle on a *destroyed* resource, which would otherwise name whatever took the slot |
//! | type tag | a handle from *another registry in this same library* |
//! | session byte | a handle the image saved in an inst var and replayed in a *later run* |
//!
//! ```ignore
//! static CONTEXTS: Registry<Context> = Registry::new();
//! resource_tags! { Context = 1 }
//!
//! let handle = CONTEXTS.insert(Context::new()?)?;   // hand this to the image
//! CONTEXTS.with(handle, |ctx| ctx.paint())?;        // and take it back later
//! drop(CONTEXTS.remove(handle)?);                   // destroyed exactly once
//! ```
//!
//! # Why the type tag exists
//!
//! Before it did, every `Registry` in a library used the identical encoding,
//! so `CONTEXTS.insert(..)` and `SURFACES.insert(..)` both answered the same
//! integer for their first insert and a Context handle passed to a Surface
//! primitive *resolved*. That is a `cairo_t *` reaching
//! `cairo_pattern_destroy`: silent memory corruption, reproducible on the
//! first insert into each registry of every session. The tag turns it into
//! [`PrimErr::BadArgument`] at the seam, before the registry is even locked.
//!
//! Tags are hand-picked through [`resource_tags!`](crate::resource_tags), which
//! proves them pairwise distinct and non-zero *at compile time*, over the one
//! scope that a decode can be confused within: a handle is only ever read by
//! [`Handle::decode`], which compares against `R::TAG` for an `R` belonging to
//! the library doing the decoding. The tags a decode can mistake one another
//! for are therefore exactly the tags of the *decoding* library, and those are
//! what the macro proves distinct.
//!
//! Separate `static`s are **not** the argument, whatever the shape of it might
//! suggest. An image-supplied handle does cross a library boundary today:
//! `primitiveCairoCreateLayout` in PangoPlugin
//! (`rust/plugins/pango-plugin/src/render.rs:74`) takes a *CairoPlugin*
//! context handle and forwards it, still an opaque `sqInt`, through
//! [`Interp::load_function_from`](crate::Interp::load_function_from) to
//! `cairoPluginBorrowContext_v1`
//! (`rust/plugins/cairo-plugin/src/bridge.rs:167`). That crossing is sound not
//! because the two libraries have separate registries but because the bridge is
//! an entry point *into the minting library*: PangoPlugin never decodes the
//! integer, and CairoPlugin decodes it against CairoPlugin's own tags. What
//! per-library tags buy is the guarantee inside one library; what they cannot
//! buy is stated in the Divergences below.
//!
//! # Faithful oddities / Divergences
//!
//! * **A 32-bit image gets no session byte, and a shorter tag.** A SmallInteger
//!   there has 30 magnitude bits against 60, and the fields are paid for out of
//!   the index and the generation. The split is 14/12/4/0 rather than
//!   24/20/8/8, so on 32-bit a handle saved across a snapshot is **not**
//!   detected as stale, and a registry mints at most 2^14 * (2^12 - 1) =
//!   67,092,480 handles -- down from 2^14 * (2^15 - 1) = 536,854,528. Keeping
//!   the session byte there instead would have left 2^18 mints, which a
//!   text-rendering image exhausts in minutes: that trades a rare false accept
//!   for a certain outage, which is the wrong way round.
//! * **That 32-bit mint budget is terminal, not a rate.** It is the easy thing
//!   to misread, so plainly: the count is per registry for the life of the
//!   *process*, and once it is spent [`Registry::insert`] answers
//!   [`PrimErr::LimitExceeded`] for that registry **forever**. Destroying
//!   resources does not win any back. A slot that reaches `MAX_GENERATION` is
//!   retired rather than refilled, so a registry whose 2^14 slots have all
//!   retired can only grow a fresh slot, whose index is past the 14-bit index
//!   field and has no handle -- and each further attempt pushes another such
//!   slot and keeps the resource in it, per `insert`'s own contract. A 64-bit
//!   image has the same terminal wall at 2^24 * (2^20 - 1), which is ~1.8e13
//!   and out of reach; a text-heavy 32-bit image minting a layout per frame is
//!   not obviously so, and its failure mode is that text stops rendering and
//!   stays stopped until the image is restarted.
//! * **The session byte is a probabilistic defence, not a proof.**
//!   `getThisSessionID` is `(time(NULL) + ioMSecs()) & 0x7FFFFFFF`, so its low
//!   byte repeats every 256 seconds of launch separation. Detection is 255/256,
//!   and 0 is reserved for "this process never learned its session id" (an
//!   older VM with a null proxy entry), which stays self-consistent within the
//!   process and simply forfeits the check.
//! * **A tag cannot tell another library's handle from one of its own, and
//!   two libraries here collide.** `resource_tags!` proves distinctness within
//!   a library and nothing checks across libraries -- nor could it: each plugin
//!   picks its own literals, and a 32-bit build has four tag bits in total, so
//!   there is no room to spend any of them on a library id. CairoPlugin
//!   declares `Context = 2` (`rust/plugins/cairo-plugin/src/resources.rs:63`)
//!   and PangoPlugin declares `Context = 2`
//!   (`rust/plugins/pango-plugin/src/resources.rs:118`). Both registries fill
//!   slot 0 at generation 1 first and both run in one process, so they agree on
//!   the session byte too: **the first PangoContext handle and the first Cairo
//!   context handle of a session are the same integer.** A foreign handle
//!   reaches a decode only through the bridge, whose entry points expect a
//!   handle minted by the library they are compiled into --
//!   `cairoPluginBorrowContext_v1`/`cairoPluginContextStatus_v1`
//!   (`rust/plugins/cairo-plugin/src/bridge.rs:167` and `:234`), called from
//!   `with_cairo_context` (`rust/plugins/pango-plugin/src/cairo_bridge.rs:366`)
//!   with an integer PangoPlugin forwards without decoding. Hand
//!   `primitiveCairoCreateLayout` a PangoContext handle instead of a Cairo one
//!   and CairoPlugin resolves it as one of its own contexts rather than
//!   failing. The hole is bounded: it can only name a resource that is *live*
//!   in the target registry with a coinciding tag, slot and generation, so what
//!   comes back is a well-formed `cairo_t *` belonging to the wrong drawing --
//!   the wrong object, never freed memory, and every lifetime check still
//!   holds. Keeping the two apart is the image's type discipline, exactly as it
//!   was for Surface and Pattern before the tag existed. Closing it belongs in
//!   the bridge rather than in the tag, and the bridge is versioned for that:
//!   a `_v2` carrying the minting library's identity beside the handle would
//!   let the callee refuse an integer it did not mint.
//! * **`is_live` answers `false` for a live resource of the wrong kind.** It is
//!   an image-visible change: `primitiveSurfaceIsLive` on a pattern handle used
//!   to be able to answer true. A Pattern is not a live Surface, so this is the
//!   fix rather than a regression -- but an image author reading a `false`
//!   where they used to read `true` deserves to find the reason written down.
//! * **A poisoned registry refuses everything, for good.** The mutex is held
//!   across the caller's closure in [`Registry::with`] and
//!   [`Registry::with_mut`], so a panic in there leaves a slot half-written
//!   and poisons the mutex. Rather than swallow that poison -- which would let
//!   the next primitive compute on the torn table -- every operation answers
//!   [`PrimErr::Unsupported`] from then on, and the module-wide flag in
//!   [`crate::poison`] fails the *other* primitives fast too. The queries
//!   answer their fail-closed value instead of an error: `is_live` false,
//!   `len` 0 (and therefore `is_empty` true), `remove_where` empty.
//! * **[`Registry::drain`] answers nothing once poisoned, and so leaks.** It
//!   exists for a shutdown hook that releases each resource; releasing a
//!   `cairo_t *` read out of a half-written slot would be a double free or a
//!   free of a dangling pointer. Leaking at process teardown costs nothing,
//!   and is the only safe reading of a table nobody can trust.
//! * **The lock is not reentrant, deliberately.** `with` and `with_mut` hold
//!   the mutex across `f`, so a closure that re-enters the *same* registry
//!   deadlocks. That is a std `Mutex` and it is the price of handing the
//!   closure a plain `&T` into the slot; the alternative -- cloning the
//!   resource out -- is not available for a raw handle to a foreign object.
//! * **This does not reach SurfacePlugin's own registry.** See [`Handle`].

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::error::{PrimErr, PrimResult};
use crate::interp::Interp;
use crate::poison::{self, Guarded};
use crate::proxy::{sqInt, MAX_SMALL_INTEGER};

// ---- the bit budget --------------------------------------------------------

/// How the magnitude bits of a handle are divided, lowest field first.
///
/// A plain struct behind a `const fn` rather than `cfg(target_pointer_width)`
/// branches, so that *both* widths can be asserted from a host of either width
/// -- there is no 32-bit target installed here, and a bad 32-bit split would
/// otherwise ship unchecked.
struct Layout {
    index: u32,
    generation: u32,
    tag: u32,
    session: u32,
}

impl Layout {
    const fn total(&self) -> u32 {
        self.index + self.generation + self.tag + self.session
    }
}

/// The field widths for a `sqInt` of `ptr_bytes` bytes.
///
/// 64-bit spends 8 bits each on the tag and the session out of an index that
/// was 32 bits wide and a generation that was 28. Both are free in practice:
/// [`Registry::insert`] scans linearly for a free slot, so a registry holding
/// 2^24 live entries is already unusable for reasons that have nothing to do
/// with the encoding, and 2^20 destroy/create cycles on one slot is far past
/// any real workload.
///
/// 32-bit drops the session field entirely and narrows the tag to 4 bits; see
/// the module docs for why that is the right trade and what it costs.
const fn layout(ptr_bytes: usize) -> Layout {
    if ptr_bytes == 8 {
        Layout {
            index: 24,
            generation: 20,
            tag: 8,
            session: 8,
        }
    } else {
        Layout {
            index: 14,
            generation: 12,
            tag: 4,
            session: 0,
        }
    }
}

/// Magnitude bits in a SmallInteger of this width -- the whole budget.
///
/// Spur spends three tag bits on immediates in a 64-bit image and one in a
/// 32-bit one, and one more bit is the sign. Mirrors
/// [`crate::proxy::MAX_SMALL_INTEGER`], parameterised so both widths are
/// evaluable here.
const fn magnitude_bits(ptr_bytes: usize) -> u32 {
    let immediate_tag_bits = if ptr_bytes == 8 { 3 } else { 1 };
    (ptr_bytes as u32) * 8 - immediate_tag_bits - 1
}

/// The largest handle the layout can produce, as a `u64` so that a 64-bit
/// layout can be checked from a 32-bit host without overflowing `sqInt`.
const fn max_handle(ptr_bytes: usize) -> u64 {
    (1u64 << layout(ptr_bytes).total()) - 1
}

const LAYOUT: Layout = layout(core::mem::size_of::<sqInt>());

const INDEX_BITS: u32 = LAYOUT.index;
const GENERATION_BITS: u32 = LAYOUT.generation;
const TAG_BITS: u32 = LAYOUT.tag;
const SESSION_BITS: u32 = LAYOUT.session;

const GENERATION_SHIFT: u32 = INDEX_BITS;
const TAG_SHIFT: u32 = GENERATION_SHIFT + GENERATION_BITS;
const SESSION_SHIFT: u32 = TAG_SHIFT + TAG_BITS;

const INDEX_MASK: sqInt = (1 << INDEX_BITS) - 1;
const GENERATION_MASK: sqInt = (1 << GENERATION_BITS) - 1;

/// The widest tag this build can carry. Enforced per type by
/// [`Resource::TAG_FITS`].
pub const TAG_MASK: u32 = (1 << TAG_BITS) - 1;

/// Zero session bits means a mask of zero, so the field simply vanishes on
/// 32-bit rather than needing a branch at every use.
const SESSION_MASK: sqInt = (1 << SESSION_BITS) - 1;

/// Highest generation a slot can reach before it is retired.
///
/// A retired slot is never reused, so the counter cannot wrap and make an
/// ancient handle valid again. The cost of retiring is one leaked `Vec` entry.
const MAX_GENERATION: u32 = (1 << GENERATION_BITS) - 1;

/// The largest handle value this build mints.
const MAX_HANDLE: sqInt = max_handle(core::mem::size_of::<sqInt>()) as sqInt;

// The layout is arithmetic, so it is checked as arithmetic -- at compile time,
// for both widths, on whichever width is doing the compiling.
const _: () = assert!(layout(8).total() == 60);
const _: () = assert!(layout(4).total() == 30);
const _: () = assert!(layout(8).total() == magnitude_bits(8));
const _: () = assert!(layout(4).total() == magnitude_bits(4));
const _: () = assert!(max_handle(8) == 1_152_921_504_606_846_975);
const _: () = assert!(max_handle(4) == 1_073_741_823);
// Floors on the narrow layout: enough tag for sixteen kinds in one library,
// and an index and generation that keep a 32-bit image usable.
const _: () = assert!(layout(4).tag >= 4);
const _: () = assert!(layout(4).index >= 14);
const _: () = assert!(layout(4).generation >= 12);
// And the whole thing must stay immediate, or the image boxes every handle.
const _: () = assert!(MAX_HANDLE <= MAX_SMALL_INTEGER);

// ---- what a handle names ---------------------------------------------------

/// A kind of resource the image names by handle.
///
/// Implement it through [`resource_tags!`](crate::resource_tags) rather than by
/// hand: the macro is what proves the tags in one library are pairwise distinct
/// and non-zero, and nothing in `Resource` alone can see two types at once.
pub trait Resource: Send + Sized + 'static {
    /// Distinguishes this kind from every other kind in *this* shared library.
    ///
    /// One library is the scope a decode can be confused within, because
    /// [`Handle::decode`] compares against the `R::TAG` of a type belonging to
    /// the library running it. It is *not* a claim that tags are unique across
    /// plugins -- CairoPlugin and PangoPlugin both use 2, and the module docs
    /// spell out where that shows. Tag 0 is reserved, so a zeroed word never
    /// looks valid.
    const TAG: u8;

    /// Forced by [`Handle::decode`], so a tag too wide for the layout -- or
    /// zero -- is a compile error on that target rather than a truncated tag
    /// at runtime. Do not override.
    ///
    /// The width matters on 32-bit, where the tag field is four bits wide; the
    /// zero check matters everywhere, and is why a hand-written impl is caught
    /// as surely as one the macro emits:
    ///
    /// ```compile_fail
    /// pub struct Ghost;
    /// impl pharo_vm_plugin::handles::Resource for Ghost {
    ///     const TAG: u8 = 0; // reserved, so a zeroed word is never a handle
    /// }
    /// // `TAG_FITS` is evaluated where the tag is used.
    /// let _ = pharo_vm_plugin::handles::Handle::<Ghost>::decode(1);
    /// ```
    const TAG_FITS: () = assert!(
        Self::TAG != 0 && (Self::TAG as u32) <= TAG_MASK,
        "a resource tag must be non-zero and fit this target's tag field"
    );
}

/// Declares the resource tags of one shared library, and checks them.
///
/// One invocation per plugin, next to the `Registry` statics, listing every
/// kind the image gets a handle on. The macro emits the [`Resource`] impls plus
/// a compile-time proof that the literals are pairwise distinct and non-zero --
/// a check over exactly the scope a decode can be confused within, since
/// [`Handle::decode`] only ever compares against a tag of the library it runs
/// in.
///
/// ```
/// pub struct Surface(*mut ());
/// pub struct Context(*mut ());
/// # unsafe impl Send for Surface {}
/// # unsafe impl Send for Context {}
/// pharo_vm_plugin::resource_tags! {
///     Surface = 1,
///     Context = 2,
/// }
/// ```
///
/// A repeated tag does not compile, which is the whole point:
///
/// ```compile_fail
/// pub struct Surface(*mut ());
/// pub struct Context(*mut ());
/// # unsafe impl Send for Surface {}
/// # unsafe impl Send for Context {}
/// pharo_vm_plugin::resource_tags! {
///     Surface = 1,
///     Context = 1,
/// }
/// ```
#[macro_export]
macro_rules! resource_tags {
    ($($ty:ty = $tag:literal),+ $(,)?) => {
        $(
            impl $crate::handles::Resource for $ty {
                const TAG: u8 = $tag;
            }
        )+

        // Pairwise distinct and non-zero, proved where the tags are written.
        // A `const` item, so it is evaluated whether or not anything uses it.
        const _: () = {
            const TAGS: &[u8] = &[$($tag),+];
            let mut i = 0;
            while i < TAGS.len() {
                assert!(
                    TAGS[i] != 0,
                    "tag 0 is reserved so that a zeroed word never looks like a handle"
                );
                let mut j = i + 1;
                while j < TAGS.len() {
                    assert!(
                        TAGS[i] != TAGS[j],
                        "two resources in this library share a handle tag, so a \
                         handle on one would resolve in the other's registry"
                    );
                    j += 1;
                }
                i += 1;
            }
        };
    };
}

/// The integer the image holds, naming one resource of kind `R`.
///
/// Obtained from [`Registry::insert`], or from an integer the image supplied
/// with [`Handle::decode`] -- which is the *only* route from a bare `sqInt` to
/// a `Handle`, and is deliberately greppable for that reason.
///
/// # What this does not close
///
/// **`surface-plugin`'s own registry is not this one and is deliberately not
/// migrated.** Its surface IDs are an array index, published through
/// `SurfacePlugin.h` / `ioRegisterSurface` and read back out of a Form's `bits`
/// by BitBltPlugin, so a freed slot is reused lowest-index-first and a stale ID
/// resolves to the new occupant -- exactly as in the C, and part of a contract
/// that crosses to plugins this port does not own. Retagging those IDs would
/// break every C plugin and every image that computes on them. So the accurate
/// claim for `Handle` is: cross-kind confusion inside one library is
/// impossible, and cross-session reuse is detectable on 64-bit, **for the
/// registries built on [`Registry`]** -- Cairo's, Pango's and SDL3's.
/// SurfacePlugin's stale-ID hole stays exactly where it was.
#[repr(transparent)]
pub struct Handle<R> {
    raw: sqInt,
    /// `fn() -> R` rather than `R`: it keeps `Handle` `Copy`, `Send` and `Sync`
    /// even though every real `R` wraps a raw pointer and is none of those.
    kind: PhantomData<fn() -> R>,
}

// Hand-written rather than derived, and unconditional in `R`: `derive` would
// add an `R: Copy` bound that no resource type can satisfy.
impl<R> Clone for Handle<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> Copy for Handle<R> {}
impl<R> PartialEq for Handle<R> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<R> Eq for Handle<R> {}
impl<R> Hash for Handle<R> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
impl<R> fmt::Debug for Handle<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Handle").field(&self.raw).finish()
    }
}

impl<R: Resource> Handle<R> {
    /// Checks the structural fields of an integer the image supplied.
    ///
    /// Two distinguishable failures, because the image's fallback code wants to
    /// tell them apart:
    ///
    /// * the tag names another kind -> [`PrimErr::BadArgument`], a *type*
    ///   error: the caller passed a Pattern where a Surface was wanted, and no
    ///   amount of retrying will help;
    /// * out of range, non-positive, generation 0, the reserved tag 0, or a
    ///   session byte from an earlier run -> [`PrimErr::NotFound`], which
    ///   covers both "never a handle" and "a handle whose resource is gone".
    ///   A handle restored from a snapshot is effectively the latter, and the
    ///   image's fallback code wants it treated as such.
    ///
    /// The range check is not redundant.
    /// [`Interp::stack_integer`](crate::Interp::stack_integer) cannot deliver a
    /// value above `MAX_SMALL_INTEGER`, but a `#[no_mangle]` bridge entry point
    /// called from another shared library takes a raw `sqInt` and can.
    pub fn decode(raw: sqInt) -> PrimResult<Self> {
        // Forces `TAG_FITS` for this `R`, turning a tag too wide for the target
        // into a compile error at the point of use.
        #[allow(clippy::let_unit_value)]
        let () = R::TAG_FITS;

        if raw <= 0 || raw > MAX_HANDLE {
            return Err(PrimErr::NotFound);
        }
        // Structure before kind. A generation of 0 or the reserved tag 0 means
        // this integer was never a handle at all -- an arbitrary small integer
        // the image made up, or a zeroed word -- and calling that a *type*
        // error would be a lie. Only an integer shaped like a handle from some
        // registry, carrying a tag some other registry in this library mints,
        // earns `BadArgument`.
        if (raw >> GENERATION_SHIFT) & GENERATION_MASK == 0 {
            return Err(PrimErr::NotFound);
        }
        let tag = ((raw >> TAG_SHIFT) & (TAG_MASK as sqInt)) as u8;
        if tag == 0 {
            return Err(PrimErr::NotFound);
        }
        if tag != R::TAG {
            return Err(PrimErr::BadArgument);
        }
        if SESSION_BITS > 0 {
            let session = ((raw >> SESSION_SHIFT) & SESSION_MASK) as u8;
            if session != session_byte() {
                return Err(PrimErr::NotFound);
            }
        }
        Ok(Self {
            raw,
            kind: PhantomData,
        })
    }

    /// The integer to hand the image.
    #[must_use]
    pub fn raw(self) -> sqInt {
        self.raw
    }

    /// Slot index and generation, for the registry that minted it.
    fn parts(self) -> (usize, u32) {
        let index = (self.raw & INDEX_MASK) as usize;
        let generation = ((self.raw >> GENERATION_SHIFT) & GENERATION_MASK) as u32;
        (index, generation)
    }
}

// ---- the session byte ------------------------------------------------------

/// Marks the cached session byte as resolved, so "not read yet" and "read, and
/// this VM would not tell us" stay distinguishable and the proxy is asked once.
const SESSION_RESOLVED: u32 = 0x100;

static SESSION: AtomicU32 = AtomicU32::new(0);

/// This process's session byte, read once from the VM and cached.
///
/// `globalSessionID` is a VM global rather than image state:
/// `smalltalksrc/VMMaker/StackInterpreter.class.st:344` lists it among the
/// interpreter's own instance variables, and that is precisely what makes it a
/// file-scope `_iss sqInt` in the generated C rather than a slot in any object
/// the image can reach. `initializeGlobalSessionID` (same file, line 7557)
/// sets it exactly once, from `(self time: #NULL) + self ioMSecs`, and
/// `initializeInterpreter:` (line 7582) calls it while the image is being
/// read. So it changes when a snapshot is resumed in a new process -- the case
/// that matters -- and correctly does *not* change when an image snapshots and
/// keeps running, where the resources are still alive and the handles must
/// stay valid. Caching it is sound for the same reason.
///
/// The Slang is cited rather than the C it generates because the C is not a
/// source a reader can check: `build/generated/**/cointerp.c` is gitignored,
/// is produced by a build, and numbers its lines differently in the 32- and
/// 64-bit configurations. `StackInterpreter.class.st` is committed, so the
/// line numbers above stay checkable at any revision this file is read at.
///
/// Answers the reserved 0 when the VM has no `getThisSessionID` entry, or
/// before `setInterpreter` has run at all (a test binary). That is
/// self-consistent within the process -- everything minted and everything
/// decoded agrees on 0 -- and only forfeits the cross-session check.
fn session_byte() -> u8 {
    let cached = SESSION.load(Ordering::Relaxed);
    if cached != 0 {
        return (cached & 0xFF) as u8;
    }
    let byte = match Interp::current().and_then(|vm| vm.session_id().ok()) {
        // A session id whose low byte is 0 borrows 1 instead, so that 0 keeps
        // meaning exactly one thing. It costs one extra false accept in 2^16
        // launch pairs.
        Some(id) => match (id & 0xFF) as u8 {
            0 => 1,
            b => b,
        },
        None => 0,
    };
    SESSION.store(SESSION_RESOLVED | u32::from(byte), Ordering::Relaxed);
    byte
}

// ---- the registry ----------------------------------------------------------

/// A table of resources the image refers to by integer.
///
/// Intended to live in a `static`. `R` must be [`Resource`] -- which is what
/// makes the migration to typed handles atomic: a registry over a type with no
/// tag does not compile, so no build can hand the image two readable
/// encodings. `Resource: Send` because the registry is shared: primitives run
/// on the interpreter thread, but Rust has to be told that, and a raw pointer
/// type will need a documented `unsafe impl Send`.
pub struct Registry<R: Resource> {
    slots: Mutex<Vec<Slot<R>>>,
}

struct Slot<R> {
    /// Bumped every time the slot is refilled. Starts at 1, so a valid handle
    /// is never 0 and `nil`-shaped zero never resolves.
    generation: u32,
    value: Option<R>,
}

impl<R: Resource> Default for Registry<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Resource> Registry<R> {
    /// An empty registry, constructible in a `static`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Mutex::new(Vec::new()),
        }
    }

    /// Takes the lock, and refuses once a panic has torn the table.
    ///
    /// The poison is honoured rather than recovered from. A `Mutex` is
    /// poisoned precisely when a guard is dropped during an unwind, and this
    /// guard is held across the caller's closure -- so poison here means a
    /// slot was left half-written, which is not something a later primitive
    /// can be allowed to compute on. See [`crate::poison`].
    fn lock(&self) -> PrimResult<Guarded<'_, Vec<Slot<R>>>> {
        poison::lock(&self.slots)
    }

    /// Stores `value` and answers the handle the image should hold.
    ///
    /// Fails with [`PrimErr::LimitExceeded`] when the slot index does not fit
    /// the handle's index field: more than 2^24 live resources in a 64-bit
    /// image, more than 2^14 in a 32-bit one.
    ///
    /// **On that failure the registry keeps `value` anyway.** The slot is
    /// filled before the handle is encoded, and encoding is the only fallible
    /// step, so a failed insert leaves the resource owned by this registry and
    /// released exactly once by [`Registry::drain`] at module shutdown -- with
    /// no handle in the image naming it in the meantime (the one carve-out is
    /// a registry poisoned by a panic, which releases nothing at all -- see
    /// the module docs). A caller must
    /// therefore **not** release the resource on the error path: that would be
    /// a double free, and it would leave a dangling pointer in a live slot for
    /// `drain` to release a second time. Answering the error and dropping the
    /// raw pointer on the floor is the correct thing to do.
    pub fn insert(&self, value: R) -> PrimResult<Handle<R>> {
        let mut slots = self.lock()?;

        if let Some((index, slot)) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.value.is_none() && s.generation < MAX_GENERATION)
        {
            slot.generation += 1;
            slot.value = Some(value);
            return encode(index, slot.generation);
        }

        let index = slots.len();
        slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        encode(index, 1)
    }

    /// Runs `f` on the resource `handle` names.
    ///
    /// Fails with [`PrimErr::NotFound`] if the handle is already removed or
    /// from an earlier occupant of the slot. Wrong-kind handles never reach
    /// here: [`Handle::decode`] refused them with [`PrimErr::BadArgument`].
    pub fn with<X>(&self, handle: Handle<R>, f: impl FnOnce(&R) -> X) -> PrimResult<X> {
        let slots = self.lock()?;
        Ok(f(resolve(&slots, handle)?))
    }

    /// Runs `f` on the resource `handle` names, mutably.
    pub fn with_mut<X>(&self, handle: Handle<R>, f: impl FnOnce(&mut R) -> X) -> PrimResult<X> {
        let (index, generation) = handle.parts();
        let mut slots = self.lock()?;
        let slot = slots.get_mut(index).ok_or(PrimErr::NotFound)?;
        if slot.generation != generation {
            return Err(PrimErr::NotFound);
        }
        let value = slot.value.as_mut().ok_or(PrimErr::NotFound)?;
        Ok(f(value))
    }

    /// Takes the resource out of the registry, invalidating `handle`.
    ///
    /// The caller owns the value and is responsible for releasing it. Removing
    /// twice fails the second time rather than double-freeing.
    pub fn remove(&self, handle: Handle<R>) -> PrimResult<R> {
        let (index, generation) = handle.parts();
        let mut slots = self.lock()?;
        let slot = slots.get_mut(index).ok_or(PrimErr::NotFound)?;
        if slot.generation != generation {
            return Err(PrimErr::NotFound);
        }
        slot.value.take().ok_or(PrimErr::NotFound)
    }

    /// Is this integer a handle on a live resource *of this kind*?
    ///
    /// Keeps its `sqInt` parameter: this is the primitive the image calls to
    /// ask about an integer it holds, so it has to be able to answer "no"
    /// rather than refuse to be called. A live resource of another kind answers
    /// `false` -- see the module docs, it is an image-visible change.
    ///
    /// Answers `false` for a poisoned registry: nothing in it is trustworthy
    /// any more, and this is a query with no failure channel.
    #[must_use]
    pub fn is_live(&self, handle: sqInt) -> bool {
        let Ok(handle) = Handle::<R>::decode(handle) else {
            return false;
        };
        let Ok(slots) = self.lock() else {
            return false;
        };
        resolve(&slots, handle).is_ok()
    }

    /// How many resources are live. Answers 0 for a poisoned registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock()
            .map(|slots| slots.iter().filter(|s| s.value.is_some()).count())
            .unwrap_or(0)
    }

    /// Are there no live resources?
    ///
    /// A poisoned registry answers `true`, following `len`. That is the
    /// fail-closed reading: a shutdown hook that walks the table finds nothing
    /// to release, which is exactly what it must do (see the module docs).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes every live resource `predicate` accepts out of the registry.
    ///
    /// For a library whose objects own each other -- SDL destroys a window's
    /// renderer with it, and a renderer's textures with that -- where one
    /// destroy call invalidates entries the image is still holding handles on.
    /// The caller owns what comes back, and the lock is released before it sees
    /// any of it; the predicate itself runs under the lock, so it must not
    /// re-enter this registry.
    ///
    /// This answers *resources* and not handles, deliberately. The obvious
    /// shape -- answer the matching handles and let the caller feed them back
    /// to [`Registry::remove`] -- has to encode one per match, and encoding is
    /// the only step here that can fail. A live slot whose index does not fit
    /// the handle's index field has no handle to answer, and that is a
    /// *reachable* state rather than a hypothetical one, because
    /// [`Registry::insert`] keeps the value when `encode` answers
    /// [`PrimErr::LimitExceeded`]. Skipping such a slot would leave behind
    /// exactly the entries whose foreign object the caller is about to destroy,
    /// for [`Registry::drain`] to release a second time at shutdown. Removing
    /// by slot index cannot be blind that way.
    ///
    /// A poisoned registry answers nothing, and so leaks, for the reason
    /// [`Registry::drain`] does.
    #[must_use]
    pub fn remove_where(&self, predicate: impl Fn(&R) -> bool) -> Vec<R> {
        let Ok(mut slots) = self.lock() else {
            return Vec::new();
        };
        slots
            .iter_mut()
            .filter_map(|slot| {
                // `matches!` rather than a `match` arm with a guard: the
                // immutable borrow of `slot.value` has to end before `take`.
                let wanted = matches!(slot.value.as_ref(), Some(value) if predicate(value));
                if wanted {
                    slot.value.take()
                } else {
                    None
                }
            })
            .collect()
    }

    /// Empties the registry, answering everything that was in it.
    ///
    /// For a module shutdown hook: the caller releases each resource. Handles
    /// the image still holds are all invalidated, and stay invalidated,
    /// because every emptied slot's generation moves on before it is reused.
    /// **A poisoned registry answers nothing, and so leaks.** The caller would
    /// release each resource, and a pointer read out of a half-written slot is
    /// as likely to be a double free or a dangling pointer as a live object.
    /// Leaking at process teardown costs nothing; freeing garbage does not.
    #[must_use]
    pub fn drain(&self) -> Vec<R> {
        let Ok(mut slots) = self.lock() else {
            return Vec::new();
        };
        slots.iter_mut().filter_map(|s| s.value.take()).collect()
    }
}

fn encode<R: Resource>(index: usize, generation: u32) -> PrimResult<Handle<R>> {
    encode_with(index, generation, session_byte())
}

/// Mints a handle with an explicit session byte.
///
/// Separated from [`encode`] only so the tests can mint a handle that claims to
/// come from another run; nothing else should pass a session in.
fn encode_with<R: Resource>(index: usize, generation: u32, session: u8) -> PrimResult<Handle<R>> {
    #[allow(clippy::let_unit_value)]
    let () = R::TAG_FITS;

    let index = sqInt::try_from(index).map_err(|_| PrimErr::LimitExceeded)?;
    if index > INDEX_MASK {
        return Err(PrimErr::LimitExceeded);
    }
    if generation == 0 || generation > MAX_GENERATION {
        return Err(PrimErr::LimitExceeded);
    }
    let raw = ((sqInt::from(session) & SESSION_MASK) << SESSION_SHIFT)
        | ((R::TAG as sqInt) << TAG_SHIFT)
        | ((generation as sqInt) << GENERATION_SHIFT)
        | index;
    Ok(Handle {
        raw,
        kind: PhantomData,
    })
}

fn resolve<R: Resource>(slots: &[Slot<R>], handle: Handle<R>) -> PrimResult<&R> {
    let (index, generation) = handle.parts();
    let slot = slots.get(index).ok_or(PrimErr::NotFound)?;
    if slot.generation != generation {
        return Err(PrimErr::NotFound);
    }
    slot.value.as_ref().ok_or(PrimErr::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two kinds in one imaginary library, declared the way a plugin declares
    /// them -- so the macro's own uniqueness proof is exercised by every run.
    #[derive(Debug, PartialEq, Eq)]
    struct A(u32);
    #[derive(Debug, PartialEq, Eq)]
    struct B(u32);

    crate::resource_tags! {
        A = 1,
        B = 2,
    }

    /// The widest tag this target can carry, for the saturation test.
    struct Widest;
    impl Resource for Widest {
        const TAG: u8 = TAG_MASK as u8;
    }

    /// A resource with no payload, so that the slot table the index-boundary
    /// test has to build costs `size_of::<Slot<Slim>>()` = 8 bytes an entry
    /// rather than 12. On a 64-bit build that is the difference between 128 MiB
    /// and 192 MiB for one test; on a 32-bit one it is 128 KiB either way.
    struct Slim;
    impl Resource for Slim {
        const TAG: u8 = 3;
    }

    fn h<R: Resource>(raw: sqInt) -> PrimResult<Handle<R>> {
        Handle::<R>::decode(raw)
    }

    #[test]
    fn a_handle_round_trips() {
        let reg: Registry<A> = Registry::new();
        let handle = reg.insert(A(42)).unwrap();
        assert_eq!(reg.with(handle, |v| v.0).unwrap(), 42);
    }

    #[test]
    fn cross_registry_confusion_is_a_type_error() {
        // The bug this whole encoding exists to close. Before the tag, the
        // first insert into every registry in a library answered the *same*
        // integer, so a Context handle passed to a Surface primitive resolved
        // -- and a `cairo_t *` reached `cairo_pattern_destroy`.
        let reg_a: Registry<A> = Registry::new();
        let reg_b: Registry<B> = Registry::new();
        let first_a = reg_a.insert(A(1)).unwrap();
        let first_b = reg_b.insert(B(2)).unwrap();

        assert_ne!(
            first_a.raw(),
            first_b.raw(),
            "the first insert into two registries must not answer one integer"
        );
        assert_eq!(
            h::<B>(first_a.raw()).unwrap_err(),
            PrimErr::BadArgument,
            "a handle of the wrong kind is a type error, not a lifetime error"
        );
        assert_eq!(h::<A>(first_b.raw()).unwrap_err(), PrimErr::BadArgument);
        assert!(!reg_b.is_live(first_a.raw()));
        assert!(!reg_a.is_live(first_b.raw()));
    }

    /// The residual hole the module docs scope, pinned so it cannot drift.
    ///
    /// It asserts a *limit*, not a guarantee: if the encoding ever grows a
    /// library discriminant this test is what fails, and the Divergences entry
    /// about CairoPlugin and PangoPlugin both using `Context = 2` is what has
    /// to be rewritten alongside it.
    #[test]
    fn a_tag_cannot_tell_another_librarys_handle_from_its_own() {
        // `Twin` stands in for the other plugin's type: a different kind that
        // picked the same tag literal, with a registry of its own -- which is
        // what a second shared library has -- that also fills slot 0 at
        // generation 1 first. Nothing catches the clash, because
        // `resource_tags!` proves distinctness within one library and this is
        // deliberately outside it.
        struct Twin;
        impl Resource for Twin {
            const TAG: u8 = A::TAG;
        }

        let mine: Registry<A> = Registry::new();
        let theirs: Registry<Twin> = Registry::new();
        let a = mine.insert(A(1)).expect("a slot");
        let t = theirs.insert(Twin).expect("a slot");

        assert_eq!(
            a.raw(),
            t.raw(),
            "same tag, same slot, same generation, same session: same integer"
        );
        // So an integer minted over there decodes over here and resolves --
        // which is what happens when a PangoContext handle reaches
        // `cairoPluginBorrowContext_v1`.
        let crossed = Handle::<A>::decode(t.raw()).expect("indistinguishable by construction");
        assert_eq!(mine.with(crossed, |v| v.0).unwrap(), 1);
    }

    #[test]
    fn zero_is_never_a_valid_handle() {
        let reg: Registry<A> = Registry::new();
        reg.insert(A(1)).unwrap();
        assert_eq!(h::<A>(0).unwrap_err(), PrimErr::NotFound);
        assert!(!reg.is_live(0));
    }

    #[test]
    fn negative_handles_are_rejected_rather_than_wrapping() {
        assert_eq!(h::<A>(-1).unwrap_err(), PrimErr::NotFound);
    }

    #[test]
    fn garbage_above_the_layout_is_rejected() {
        // A bridge entry point takes a raw `sqInt` from another shared library,
        // so out-of-range values do arrive. None of them may alias a slot.
        assert!(h::<A>(MAX_SMALL_INTEGER).is_err());
        assert_eq!(
            h::<A>(MAX_SMALL_INTEGER.wrapping_add(1)).unwrap_err(),
            PrimErr::NotFound
        );
        assert_eq!(h::<A>(sqInt::MAX).unwrap_err(), PrimErr::NotFound);
        // Every field set is `MAX_HANDLE`, which is in range but is not this
        // kind unless the tag happens to match.
        assert_eq!(h::<A>(MAX_HANDLE).unwrap_err(), PrimErr::BadArgument);
        // ...but an integer the image simply made up is not a type error: it
        // was never a handle, so it is NotFound like a destroyed one.
        assert_eq!(h::<A>(1).unwrap_err(), PrimErr::NotFound);
        assert_eq!(h::<A>(sqInt::from(7u8)).unwrap_err(), PrimErr::NotFound);
    }

    #[test]
    fn removing_twice_fails_instead_of_double_freeing() {
        let reg: Registry<A> = Registry::new();
        let handle = reg.insert(A(7)).unwrap();
        assert_eq!(reg.remove(handle).unwrap(), A(7));
        assert_eq!(reg.remove(handle).unwrap_err(), PrimErr::NotFound);
    }

    #[test]
    fn a_stale_handle_still_fails_as_not_found() {
        // The lifetime hole: SurfacePlugin's registry reuses the lowest free
        // index and a stale ID silently names the newcomer. Distinctly *not*
        // BadArgument -- the kind is right, the resource is gone -- because the
        // image's fallback code treats the two differently.
        let reg: Registry<A> = Registry::new();
        let first = reg.insert(A(1)).unwrap();
        reg.remove(first).unwrap();
        let second = reg.insert(A(2)).unwrap();

        assert_ne!(first, second);
        assert_eq!(first.raw() & INDEX_MASK, second.raw() & INDEX_MASK); // same slot
        assert_eq!(reg.with(first, |v| v.0).unwrap_err(), PrimErr::NotFound);
        assert_ne!(reg.with(first, |v| v.0).unwrap_err(), PrimErr::BadArgument);
        assert_eq!(reg.with(second, |v| v.0).unwrap(), 2);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_handle_from_another_session_does_not_resolve() {
        // A 32-bit image has no session field, so there is nothing to test
        // there -- see the Divergences in the module docs.
        let mine = session_byte();
        let theirs = mine.wrapping_add(1).max(1);
        let stale = encode_with::<A>(0, 1, theirs).unwrap();
        assert_eq!(
            Handle::<A>::decode(stale.raw()).unwrap_err(),
            PrimErr::NotFound,
            "a handle an image saved in a previous run is stale, not wrong-kind"
        );
        // And the same fields with this session's byte do decode, so the test
        // is about the session and nothing else.
        let fresh = encode_with::<A>(0, 1, mine).unwrap();
        assert!(Handle::<A>::decode(fresh.raw()).is_ok());
    }

    #[test]
    fn handles_stay_small_integers() {
        let reg: Registry<A> = Registry::new();
        for i in 0..64 {
            let handle = reg.insert(A(i)).unwrap();
            let raw = handle.raw();
            assert!(
                raw > 0 && raw <= MAX_SMALL_INTEGER,
                "handle {raw} not immediate"
            );
        }
        // The loop only exercises tiny values. Saturating every field is what
        // would actually catch a layout that has overflowed its budget.
        let widest = encode_with::<Widest>(INDEX_MASK as usize, MAX_GENERATION, 0xFF).unwrap();
        assert_eq!(widest.raw(), MAX_HANDLE);
        assert!(widest.raw() <= MAX_SMALL_INTEGER);
    }

    #[test]
    fn mutation_is_visible_to_later_lookups() {
        let reg: Registry<A> = Registry::new();
        let handle = reg.insert(A(1)).unwrap();
        reg.with_mut(handle, |v| v.0 = 9).unwrap();
        assert_eq!(reg.with(handle, |v| v.0).unwrap(), 9);
    }

    #[test]
    fn draining_empties_the_registry_and_invalidates_handles() {
        let reg: Registry<A> = Registry::new();
        let a = reg.insert(A(1)).unwrap();
        let b = reg.insert(A(2)).unwrap();
        let mut drained: Vec<u32> = reg.drain().into_iter().map(|v| v.0).collect();
        drained.sort_unstable();
        assert_eq!(drained, vec![1, 2]);
        assert!(reg.is_empty());
        assert!(!reg.is_live(a.raw()));
        assert!(!reg.is_live(b.raw()));
    }

    #[test]
    fn remove_where_takes_live_matches_only() {
        let reg: Registry<A> = Registry::new();
        let a = reg.insert(A(1)).unwrap();
        let b = reg.insert(A(2)).unwrap();
        let c = reg.insert(A(1)).unwrap();
        reg.remove(c).unwrap();

        assert_eq!(reg.remove_where(|v| v.0 == 1), vec![A(1)]);
        assert_eq!(
            reg.with(a, |v| v.0).unwrap_err(),
            PrimErr::NotFound,
            "what came back is gone from the registry"
        );
        assert!(
            reg.remove_where(|v| v.0 == 1).is_empty(),
            "the already-removed slot must not come back a second time"
        );
        assert_eq!(reg.remove_where(|v| v.0 == 99), vec![]);
        assert_eq!(reg.remove_where(|v| v.0 == 2), vec![A(2)]);
        assert!(reg.is_empty());
        assert!(!reg.is_live(b.raw()));
    }

    /// The boundary `insert` documents itself as leaving behind, and the reason
    /// this sweep works on slot indices rather than on handles.
    ///
    /// A slot one past the index field is live and has no handle: `encode`
    /// answers `LimitExceeded` for it, so a handle-returning sweep would skip
    /// it silently and a caller destroying the foreign objects it just swept
    /// would leave this one for `drain` to release a second time.
    ///
    /// Built by hand rather than through `insert`, because reaching the index
    /// honestly would mean 2^24 successful inserts first on a 64-bit build.
    #[test]
    fn a_live_slot_past_the_index_field_is_still_swept() {
        let past = INDEX_MASK as usize + 1;
        assert_eq!(
            encode_with::<Slim>(past, 1, session_byte()).unwrap_err(),
            PrimErr::LimitExceeded,
            "the premise: one past the widest index has no handle at all"
        );

        let reg: Registry<Slim> = Registry::new();
        {
            let mut slots = reg.lock().expect("a fresh registry is not poisoned");
            slots.resize_with(past, || Slot {
                generation: 1,
                value: None,
            });
            slots.push(Slot {
                generation: 1,
                value: Some(Slim),
            });
        }
        assert_eq!(reg.len(), 1, "it is live, whatever the encoding thinks");

        assert_eq!(
            reg.remove_where(|_| true).len(),
            1,
            "a live slot must not be skipped for want of a handle"
        );
        assert!(reg.is_empty());
    }

    #[test]
    fn a_registry_torn_by_a_panic_refuses_every_later_call() {
        // std's own mutex poison, on its own: no panic hook is installed in
        // this binary, so this is the per-registry half of the defence. It is
        // exact -- a `Mutex` is poisoned precisely when a guard is dropped
        // during an unwind, which is what `with_mut` holding the lock across
        // the closure arranges.
        let reg: Registry<B> = Registry::new();
        let handle = reg.insert(B(1)).unwrap();

        let torn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reg.with_mut(handle, |v| {
                v.0 = 2;
                panic!("halfway through");
            })
        }));
        assert!(torn.is_err());

        assert_eq!(reg.with(handle, |v| v.0).unwrap_err(), PrimErr::Unsupported);
        assert!(matches!(reg.insert(B(3)), Err(PrimErr::Unsupported)));
        assert!(matches!(reg.remove(handle), Err(PrimErr::Unsupported)));
        assert!(!reg.is_live(handle.raw()));
        assert_eq!(reg.len(), 0);
        assert!(reg.is_empty());
        assert!(reg.remove_where(|_| true).is_empty());
        assert!(
            reg.drain().is_empty(),
            "a shutdown hook must not free out of a half-written table"
        );
    }

    #[test]
    fn len_counts_only_live_resources() {
        let reg: Registry<A> = Registry::new();
        let a = reg.insert(A(1)).unwrap();
        reg.insert(A(2)).unwrap();
        assert_eq!(reg.len(), 2);
        reg.remove(a).unwrap();
        assert_eq!(reg.len(), 1);
    }
}
