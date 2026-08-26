//! The three things the image holds handles on, and how they are released.
//!
//! Nothing here hands a pointer to the image. A `cairo_t *` lives in a
//! [`Registry`] and the image gets an integer, so a primitive called with a
//! handle to a destroyed context fails with `NotFound` and runs its Smalltalk
//! fallback -- where the image-side FFI binding would have dereferenced freed
//! memory inside the VM.

use core::ffi::c_int;
use std::sync::Mutex;

use pharo_vm_plugin::handles::Registry;
use pharo_vm_plugin::{sqInt, Interp, Oop, PrimErr, PrimResult};

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

fn retained() -> std::sync::MutexGuard<'static, Vec<Oop>> {
    RETAINED_PINS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How many pins are being held past their surface's destruction.
#[must_use]
pub fn retained_pin_count() -> usize {
    retained().len()
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
                _ => retained().push(backing),
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

/// Runs `f` on the surface `handle` names.
pub fn with_surface<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_surface_t) -> PrimResult<R>,
) -> PrimResult<R> {
    SURFACES.with(handle, |s| s.as_ptr()).and_then(f)
}

/// Runs `f` on the context `handle` names.
pub fn with_context<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_t) -> PrimResult<R>,
) -> PrimResult<R> {
    CONTEXTS.with(handle, |c| c.as_ptr()).and_then(f)
}

/// Runs `f` on the pattern `handle` names.
pub fn with_pattern<R>(
    handle: sqInt,
    f: impl FnOnce(*mut cairo_pattern_t) -> PrimResult<R>,
) -> PrimResult<R> {
    PATTERNS.with(handle, |p| p.as_ptr()).and_then(f)
}

/// Destroys the surface `handle` names. Destroying twice fails the second time.
pub fn destroy_surface(vm: &Interp, handle: sqInt) -> PrimResult<()> {
    SURFACES.remove(handle)?.release(Some(vm))
}

/// Destroys the context `handle` names.
pub fn destroy_context(handle: sqInt) -> PrimResult<()> {
    CONTEXTS.remove(handle)?.release()
}

/// Destroys the pattern `handle` names.
pub fn destroy_pattern(handle: sqInt) -> PrimResult<()> {
    PATTERNS.remove(handle)?.release()
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
