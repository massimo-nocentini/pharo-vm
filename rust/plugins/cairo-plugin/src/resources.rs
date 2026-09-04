//! The three things the image holds handles on, and how they are released.
//!
//! Nothing here hands a pointer to the image. A `cairo_t *` lives in a
//! [`Registry`] and the image gets an integer, so a primitive called with a
//! handle to a destroyed context fails with `NotFound` and runs its Smalltalk
//! fallback -- where the image-side FFI binding would have dereferenced freed
//! memory inside the VM.
//!
//! Each registry's handles carry a type tag, declared once in
//! [`resource_tags!`] below, so a pattern handle passed to a surface primitive
//! fails with `BadArgument` rather than resolving. See
//! [`pharo_vm_plugin::handles`] for the encoding and what it does not cover.

use core::ffi::c_int;
use std::sync::Mutex;

use pharo_vm_plugin::handles::{Handle, Registry};
use pharo_vm_plugin::poison::{self, Guarded};
use pharo_vm_plugin::{resource_tags, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{cairo, cairo_pattern_t, cairo_surface_t, cairo_t, cc};

/// A drawing target, and the image memory it draws into if any.
pub struct Surface {
    ptr: *mut cairo_surface_t,
    /// The Bitmap or ByteArray this surface writes straight into, pinned for
    /// as long as Cairo can reach it. `None` for a surface Cairo allocated.
    backing: Option<Oop>,
}

/// A drawing context.
pub struct Context {
    ptr: *mut cairo_t,
}

/// A source of paint.
pub struct Pattern {
    ptr: *mut cairo_pattern_t,
}

// SAFETY for the three below: these are raw pointers into Cairo's heap, and
// Cairo's objects are not thread-safe -- but the VM runs every primitive on
// its single interpreter thread, so no two threads ever touch one. `Send` is
// asserted only so the pointers can live in a `static Registry`, which needs
// its contents to be `Send` to be `Sync`. Nothing in this crate spawns a
// thread or moves a handle to one.
unsafe impl Send for Surface {}
unsafe impl Send for Context {}
unsafe impl Send for Pattern {}

// The one place this library's handle tags are declared, next to the statics
// they distinguish. Without them all three registries used the same encoding,
// so `CONTEXTS.insert` and `SURFACES.insert` answered the same integer for
// their first insert and a Context handle passed to a Surface primitive
// resolved -- a `cairo_t *` reaching `cairo_pattern_destroy`. The macro proves
// the three are distinct and non-zero at compile time, which is the scope
// `Handle::decode` can be confused within. This library also decodes handles
// arriving from *another* one -- the bridge in `crate::bridge` takes an integer
// PangoPlugin forwards without reading -- and PangoPlugin's `Context` tag is 2
// as well; `pharo_vm_plugin::handles` scopes what that costs.
resource_tags! {
    Surface = 1,
    Context = 2,
    Pattern = 3,
}

/// Surfaces the image holds handles on.
pub static SURFACES: Registry<Surface> = Registry::new();
/// Contexts the image holds handles on.
pub static CONTEXTS: Registry<Context> = Registry::new();
/// Patterns the image holds handles on.
pub static PATTERNS: Registry<Pattern> = Registry::new();

/// Pins that could not be released when their surface was destroyed.
///
/// `cairo_surface_destroy` drops *our* reference. A context created over the
/// surface, or a pattern made from it, holds one of its own -- so the surface
/// can outlive the image's handle on it, and it is still writing into the
/// object we pinned. Unpinning then would let the collector move memory Cairo
/// is about to write to, which is the one failure mode this whole design
/// exists to prevent.
///
/// So the pin is kept instead. It costs the collector one immovable object
/// until the image exits; `primitiveRetainedPinCount` reports how many, and a
/// growing number means the image is destroying surfaces before the contexts
/// drawn on them.
static RETAINED_PINS: Mutex<Vec<Oop>> = Mutex::new(Vec::new());

/// The retained pins, refused once a panic has torn the list.
///
/// Through [`poison::lock`] like every other global in this tree. Recovering
/// this one locally would in fact be defensible -- the guard is held for a
/// single `push` or `len` and the list is append-only -- but the
/// [`Section`](pharo_vm_plugin::Section) it opens is not about this list: it
/// is what makes a panic under this lock disable the module, and the module is
/// holding a `cairo_surface_t *` registry whose invariants are not
/// reconstructible.
fn retained() -> PrimResult<Guarded<'static, Vec<Oop>>> {
    poison::lock(&RETAINED_PINS)
}

/// How many pins are being held past their surface's destruction.
pub fn retained_pin_count() -> PrimResult<usize> {
    Ok(retained()?.len())
}

impl Surface {
    /// Takes ownership of a surface Cairo just created.
    ///
    /// Fails, releasing the surface again, if Cairo answered one in an error
    /// state: `cairo_image_surface_create` never answers null, it answers a
    /// "nil surface" that silently swallows every later call. Handing the
    /// image a handle on one of those would turn a mistake here into a blank
    /// drawing much later.
    pub(crate) fn adopt(ptr: *mut cairo_surface_t, backing: Option<Oop>) -> PrimResult<Self> {
        let c = cairo()?;
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        let status = cc!(c, cairo_surface_status(ptr));
        if let Err(e) = crate::ffi::check_status(status) {
            cc!(c, cairo_surface_destroy(ptr));
            return Err(e);
        }
        Ok(Self { ptr, backing })
    }

    /// The raw surface, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut cairo_surface_t {
        self.ptr
    }

    /// Releases the surface, and the pin on its backing store when it is safe.
    fn release(self, vm: Option<&Interp>) -> PrimResult<()> {
        let c = cairo()?;
        // Asked before destroying: afterwards the pointer may be freed. A
        // count of 1 means ours is the only reference, so nothing will touch
        // the backing store again.
        let sole_owner = match c.cairo_surface_get_reference_count {
            // SAFETY: `self.ptr` is a live surface this registry owned.
            Some(f) => (unsafe { f(self.ptr) }) <= 1,
            // Without the accessor we cannot prove it is safe, so we do not.
            None => false,
        };
        cc!(c, cairo_surface_destroy(self.ptr));

        if let Some(backing) = self.backing {
            match (sole_owner, vm) {
                (true, Some(vm)) => vm.unpin_object(backing)?,
                _ => retained()?.push(backing),
            }
        }
        Ok(())
    }
}

impl Context {
    /// Takes ownership of a context Cairo just created, rejecting a broken one.
    pub(crate) fn adopt(ptr: *mut cairo_t) -> PrimResult<Self> {
        let c = cairo()?;
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        let status = cc!(c, cairo_status(ptr));
        if let Err(e) = crate::ffi::check_status(status) {
            cc!(c, cairo_destroy(ptr));
            return Err(e);
        }
        Ok(Self { ptr })
    }

    /// The raw context, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut cairo_t {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let c = cairo()?;
        cc!(c, cairo_destroy(self.ptr));
        Ok(())
    }
}

impl Pattern {
    /// Takes ownership of a pattern Cairo just created, rejecting a broken one.
    pub(crate) fn adopt(ptr: *mut cairo_pattern_t) -> PrimResult<Self> {
        let c = cairo()?;
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        let status = cc!(c, cairo_pattern_status(ptr));
        if let Err(e) = crate::ffi::check_status(status) {
            cc!(c, cairo_pattern_destroy(ptr));
            return Err(e);
        }
        Ok(Self { ptr })
    }

    /// The raw pattern, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut cairo_pattern_t {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let c = cairo()?;
        cc!(c, cairo_pattern_destroy(self.ptr));
        Ok(())
    }
}

// These accessors are the whole untyped seam of this crate: every one of the
// forty-odd primitives reaches its resource through them, so `Handle::decode`
// appears here and nowhere else, and a wrong-kind handle is rejected before
// any registry is locked. The `sqInt` parameter stays because that is what the
// image hands over; a primitive that wants the check in its own signature takes
// a `Handle<Surface>`-shaped argument instead, as `primitiveSetSource` in
// `context.rs` does.

/// Runs `f` on the surface `handle` names.
pub fn with_surface<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_surface_t) -> PrimResult<R>,
) -> PrimResult<R> {
    SURFACES
        .with(Handle::decode(handle)?, |s| s.as_ptr())
        .and_then(f)
}

/// Runs `f` on the context `handle` names.
pub fn with_context<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_t) -> PrimResult<R>,
) -> PrimResult<R> {
    CONTEXTS
        .with(Handle::decode(handle)?, |c| c.as_ptr())
        .and_then(f)
}

/// Runs `f` on the pattern `handle` names.
pub fn with_pattern<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_pattern_t) -> PrimResult<R>,
) -> PrimResult<R> {
    PATTERNS
        .with(Handle::decode(handle)?, |p| p.as_ptr())
        .and_then(f)
}

/// Destroys the surface `handle` names. Destroying twice fails the second time.
pub fn destroy_surface(vm: &Interp, handle: sqInt) -> PrimResult<()> {
    SURFACES.remove(Handle::decode(handle)?)?.release(Some(vm))
}

/// Destroys the context `handle` names.
pub fn destroy_context(handle: sqInt) -> PrimResult<()> {
    CONTEXTS.remove(Handle::decode(handle)?)?.release()
}

/// Destroys the pattern `handle` names.
pub fn destroy_pattern(handle: sqInt) -> PrimResult<()> {
    PATTERNS.remove(Handle::decode(handle)?)?.release()
}

/// Releases everything, for the module's shutdown hook.
///
/// Contexts and patterns go first: each holds a reference on a surface, and
/// releasing them first is what lets the surfaces answer a reference count of
/// 1 and release their pins properly rather than retaining them.
pub fn release_all() {
    for c in CONTEXTS.drain() {
        let _ = c.release();
    }
    for p in PATTERNS.drain() {
        let _ = p.release();
    }
    let vm = Interp::current();
    for s in SURFACES.drain() {
        let _ = s.release(vm.as_ref());
    }
}

/// Narrows an image integer to the `int` Cairo's enumerations use.
///
/// Cairo takes enumerations as `int` and does not range-check them: an
/// out-of-range operator or line cap puts the context into an error state that
/// silences every later call. Rejecting the value here fails the primitive
/// that got it wrong instead.
pub fn as_c_int(value: sqInt) -> PrimResult<c_int> {
    c_int::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Narrows an image integer that must also be non-negative.
pub fn as_c_int_positive(value: sqInt) -> PrimResult<c_int> {
    let v = as_c_int(value)?;
    if v < 0 {
        return Err(PrimErr::BadArgument);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live bug, against the real `SURFACES`/`CONTEXTS`/`PATTERNS`.
    ///
    /// Nothing here calls into Cairo, and it does not need to: all three
    /// registries used to share one encoding, so their first inserts answered
    /// the *same* integer and `with_surface(a_context_handle, ..)` resolved --
    /// a `cairo_t *` on its way to `cairo_pattern_destroy`. A null pointer
    /// stands in for the foreign object perfectly, because the tag is checked
    /// while the integer is decoded, before any registry is locked.
    #[test]
    fn a_handle_from_one_registry_is_refused_by_the_others() {
        let surface = SURFACES
            .insert(Surface {
                ptr: core::ptr::null_mut(),
                backing: None,
            })
            .expect("a slot");
        let context = CONTEXTS
            .insert(Context {
                ptr: core::ptr::null_mut(),
            })
            .expect("a slot");
        let pattern = PATTERNS
            .insert(Pattern {
                ptr: core::ptr::null_mut(),
            })
            .expect("a slot");

        assert_ne!(surface.raw(), context.raw());
        assert_ne!(surface.raw(), pattern.raw());
        assert_ne!(context.raw(), pattern.raw());

        // Wrong kind: a type error, not a lifetime error, and the resource is
        // never reached.
        for (name, wrong) in [("context", context.raw()), ("pattern", pattern.raw())] {
            assert_eq!(
                with_surface(wrong, |_| Ok(())),
                Err(PrimErr::BadArgument),
                "a {name} handle must not resolve as a surface"
            );
            assert_eq!(destroy_surface_kind_check(wrong), Err(PrimErr::BadArgument));
        }
        for (name, wrong) in [("surface", surface.raw()), ("pattern", pattern.raw())] {
            assert_eq!(
                with_context(wrong, |_| Ok(())),
                Err(PrimErr::BadArgument),
                "a {name} handle must not resolve as a context"
            );
        }
        for (name, wrong) in [("surface", surface.raw()), ("context", context.raw())] {
            assert_eq!(
                with_pattern(wrong, |_| Ok(())),
                Err(PrimErr::BadArgument),
                "a {name} handle must not resolve as a pattern"
            );
        }

        // ...and `primitiveSurfaceIsLive` on a pattern answers false, which is
        // the image-visible half of the same fix.
        assert!(SURFACES.is_live(surface.raw()));
        assert!(!SURFACES.is_live(context.raw()));
        assert!(!SURFACES.is_live(pattern.raw()));

        // Each handle still works in its own registry.
        assert!(with_surface(surface.raw(), |p| Ok(p.is_null())).unwrap());
        assert!(with_context(context.raw(), |p| Ok(p.is_null())).unwrap());
        assert!(with_pattern(pattern.raw(), |p| Ok(p.is_null())).unwrap());

        // Leave the shared statics as they were found. `release` is never
        // called, so no null pointer reaches Cairo.
        SURFACES.remove(surface).expect("still there");
        CONTEXTS.remove(context).expect("still there");
        PATTERNS.remove(pattern).expect("still there");
    }

    // -----------------------------------------------------------------------
    // Fail-fast after a panic mid-release
    // -----------------------------------------------------------------------

    /// A panic while [`retained`] is held refuses every later lock.
    ///
    /// The SDK proves the mechanism in
    /// `pharo-vm-plugin/tests/plugin_mutex_poison.rs`; this proves the
    /// *wiring* at this site, which is the half a `RETAINED_PINS.clear_poison()`
    /// slipped in front of the `poison::lock` would silently undo.
    ///
    /// The panic is placed where `Surface::release` actually holds the lock:
    /// `cairo_surface_destroy` has already dropped our reference to the
    /// surface, and the pin on the Bitmap that surface was writing into is on
    /// its way into this list precisely because it *cannot* be released. A
    /// panic in that window leaves the list one entry short of the pins the
    /// module is obliged to keep, so what a recovered lock would hand back is
    /// not a shorter list -- it is a list that says an object is movable while
    /// a `cairo_surface_t *` the image can no longer see is still writing into
    /// it. Refusing is the only answer, and refusing is what the poisoned
    /// module then extends to every other primitive: the surface, context and
    /// pattern registries alongside this list hold `cairo_*_t *` whose
    /// invariants nothing in the image knows how to rebuild.
    ///
    /// It shares this binary with the two tests above, which is safe in one
    /// direction only and deliberately: nothing else in the crate's unit tests
    /// touches `RETAINED_PINS` (the only two callers are `Surface::release`
    /// and `primitiveRetainedPinCount`, both of which need a live Cairo or a
    /// live proxy), and a poisoned `Mutex` never unpoisons -- so this test must
    /// stay the only one here that takes that lock. The module-wide flag is not
    /// touched at all: that needs `setInterpreter` to have installed the panic
    /// hook, which no unit test does.
    ///
    /// The panic is raised by this test rather than injected through Cairo on
    /// purpose: every call out of this plugin crosses `extern "C"`, whose
    /// abort-on-unwind shim would turn an injected panic into `SIGABRT`
    /// instead of the unwind the hazard is made of.
    #[test]
    fn a_panic_while_the_retained_pins_are_held_refuses_every_later_lock() {
        assert_eq!(
            retained_pin_count(),
            Ok(0),
            "a fresh module hands out the pin list"
        );

        let torn = std::panic::catch_unwind(|| {
            let mut pins = retained().expect("still healthy");
            // Half of what `release` does: our reference to the surface is
            // gone, and this is the record that the pin outlived it.
            pins.push(Oop(0x1234));
            panic!("the proxy raised an error mid-release");
        });
        assert!(torn.is_err());

        assert_eq!(
            retained().err(),
            Some(PrimErr::Unsupported),
            "a list that no longer accounts for every unreleasable pin must \
             never be handed to a caller"
        );
        assert_eq!(
            retained_pin_count(),
            Err(PrimErr::Unsupported),
            "and the one caller the image can reach propagates that refusal \
             rather than reporting a count it cannot stand behind"
        );
    }

    /// `destroy_surface` needs a proxy to unpin with, so the kind check is
    /// exercised through the decode step it shares with it.
    fn destroy_surface_kind_check(handle: sqInt) -> PrimResult<()> {
        Handle::<Surface>::decode(handle).map(|_| ())
    }

    #[test]
    fn a_destroyed_handle_is_not_found_rather_than_wrong_kind() {
        // The two failures the image tells apart: `BadArgument` says "you
        // passed the wrong kind of thing", `NotFound` says "that one is gone".
        let context = CONTEXTS
            .insert(Context {
                ptr: core::ptr::null_mut(),
            })
            .expect("a slot");
        let raw = context.raw();
        CONTEXTS.remove(context).expect("still there");

        assert_eq!(with_context(raw, |_| Ok(())), Err(PrimErr::NotFound));
        assert_ne!(with_context(raw, |_| Ok(())), Err(PrimErr::BadArgument));
        assert!(!CONTEXTS.is_live(raw));
    }
}
