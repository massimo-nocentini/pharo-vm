//! `PangoTabArray`: tab stops, and the two ways Pango lets an out-of-range
//! index through.
//!
//! Every index that reaches Pango from here has been checked against
//! `pango_tab_array_get_size` first, and the two functions that take one fail
//! in *opposite* directions, which is why neither can be trusted to do it:
//!
//! * `pango_tab_array_get_tab` `g_return_if_fail`s on the index and leaves
//!   both out parameters **untouched** -- measured, the caller then reads
//!   uninitialised stack and hands the image two arbitrary integers. Worse, an
//!   environment carrying `G_DEBUG=fatal-criticals` turns that assertion into
//!   `abort()`, which takes the whole VM with it.
//! * `pango_tab_array_set_tab` does not check its index at all: it silently
//!   grows the array -- measured, `set_tab(ta, 9, ..)` on a size-2 array left
//!   `get_size` answering 10. An image typo becomes an allocation nobody asked
//!   for and a layout with eight invisible tab stops in it.
//!
//! So both are `BadIndex` here, and they behave the same way from the image,
//! which is the only defensible contract when the two halves of one C API
//! disagree about what an index means.
//!
//! `set_tab` does assert on its *other* number: `g_return_if_fail (location >=
//! 0)`, measured -- the stop is then left exactly as it was, with only a line
//! on stderr to say so, and under `G_DEBUG=fatal-criticals` the same assertion
//! ends the process. So a negative location is `BadArgument` here and Pango is
//! never reached with one.
//!
//! **A tab array is freed, not unref'd.** It is the one handle type in this
//! plugin with no reference count (`PangoAttrList`, its neighbour in every
//! layout call, has one). The verb lives in `resources::TabArray::release`
//! and nothing in this file releases anything by hand.
//!
//! # Units
//!
//! A tab location is in Pango units (1/`PANGO_SCALE` of a device unit) unless
//! the array was made with `inPixels` true, in which case it is in device
//! pixels. The flag is a *reinterpretation*, not a conversion: changing it
//! with [`primitiveTabArraySetPositionsInPixels`] leaves the stored numbers
//! exactly as they were, so a 1024 meant as one pixel becomes 1024 pixels.
//! Nothing in this file scales anything.

use core::ffi::c_int;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    self, pango, pg, PANGO_TAB_ALIGN_MAX, PANGO_TAB_ALIGN_MAX_PRE_1_50, PANGO_VERSION_1_50,
};
use pharo_vm_plugin::handles::Handle;

use crate::resources::{
    as_c_int, as_c_int_positive, as_gboolean, destroy_tab_array, enum_in_or_since, from_gboolean,
    int_array, register_tab_array, runtime_version, utf8_cstring, with_tab_array, TabArray,
};

/// The index bounds check both `get_tab` and `set_tab` need, and neither does.
///
/// Answers the narrowed index, having established that `0 <= index < size`.
/// Kept in one place because the two call sites must not drift apart: the
/// moment they do, one of Pango's two failure modes is reachable again.
fn checked_index(tabs: *mut ffi::PangoTabArray, index: sqInt) -> PrimResult<c_int> {
    let index = as_c_int(index)?;
    let p = pango()?;
    let size = pg!(p, pango_tab_array_get_size(tabs));
    if index < 0 || index >= size {
        return Err(PrimErr::BadIndex);
    }
    Ok(index)
}

/// `pango_tab_array_new`. Answers a handle on an array of `size` tab stops,
/// every one of them left-aligned at location 0 until it is set.
///
/// `inPixels` fixes what a location *means* for the life of the array, unless
/// [`primitiveTabArraySetPositionsInPixels`] changes it: true is device
/// pixels, false is Pango units.
///
/// `size` is rejected when negative rather than truncated, because Pango
/// allocates `size` tab records up front and a negative `gint` reaching it
/// through a cast is an allocation the size of the address space.
#[pharo_primitive]
fn primitiveTabArrayNew(
    _vm: &Interp,
    size: sqInt,
    in_pixels: bool,
) -> PrimResult<Handle<TabArray>> {
    let p = pango()?;
    let size = as_c_int_positive(size)?;
    let tabs = pg!(p, pango_tab_array_new(size, as_gboolean(in_pixels)));
    if tabs.is_null() {
        return Err(PrimErr::NoCMemory);
    }
    register_tab_array(tabs)
}

/// `pango_tab_array_copy`. Answers a handle on an independent array with the
/// same stops, the same size and the same pixel flag.
///
/// The copy is transfer-full: the image owns it and must destroy it, exactly
/// as it owns the original. Nothing is shared between them, so Pango's own
/// copy-on-write does not enter into it.
#[pharo_primitive]
fn primitiveTabArrayCopy(_vm: &Interp, tabs: sqInt) -> PrimResult<Handle<TabArray>> {
    with_tab_array(tabs, |t| {
        let p = pango()?;
        let copy = pg!(p, pango_tab_array_copy(t));
        if copy.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        register_tab_array(copy)
    })
}

/// `pango_tab_array_free`. Releases the array and retires the handle.
///
/// Free, not unref: the first call destroys it and a second would be a double
/// free. The registry's generation counter is what makes a repeat call answer
/// `NotFound` instead, so a Smalltalk finaliser running twice is harmless.
#[pharo_primitive]
fn primitiveTabArrayDestroy(_vm: &Interp, tabs: sqInt) -> PrimResult<()> {
    destroy_tab_array(tabs)
}

/// `pango_tab_array_get_size`. How many tab stops the array holds.
///
/// The number every index in this file is checked against, so the image can
/// do the same check before it calls and get a Smalltalk error instead of a
/// primitive failure.
#[pharo_primitive]
fn primitiveTabArraySize(_vm: &Interp, tabs: sqInt) -> PrimResult<sqInt> {
    with_tab_array(tabs, |t| {
        let p = pango()?;
        Ok(pg!(p, pango_tab_array_get_size(t)) as sqInt)
    })
}

/// `pango_tab_array_resize`. Grows the array with left-aligned stops at
/// location 0, or truncates it, discarding the stops past the new end.
///
/// The only supported way to make an array bigger. Growing it by writing past
/// the end with `set_tab` works in Pango and is refused here, because that
/// path cannot tell a deliberate growth from a typo'd index.
#[pharo_primitive]
fn primitiveTabArrayResize(_vm: &Interp, tabs: sqInt, size: sqInt) -> PrimResult<()> {
    let size = as_c_int_positive(size)?;
    with_tab_array(tabs, |t| {
        let p = pango()?;
        pg!(p, pango_tab_array_resize(t, size));
        Ok(())
    })
}

/// `pango_tab_array_set_tab`. Sets one stop's alignment and location.
///
/// `location` is in Pango units, or in device pixels when the array was made
/// with `inPixels` true.
///
/// `align` is a `PangoTabAlign`: 0 left, 1 right, 2 centre, 3 decimal. Only 0
/// exists before Pango 1.50, so 1..=3 answer `Unsupported` on an older
/// install rather than being stored and silently ignored by a line breaker
/// whose switch has no arm for them.
///
/// `index` must already be in range. Pango would grow the array instead --
/// see this module's header for why that is refused. `location` must be
/// non-negative, which is Pango's own rule, enforced here because Pango
/// enforces it with an assertion.
#[pharo_primitive]
fn primitiveTabArraySetTab(
    _vm: &Interp,
    tabs: sqInt,
    index: sqInt,
    align: sqInt,
    location: sqInt,
) -> PrimResult<()> {
    let align = enum_in_or_since(
        align,
        0,
        PANGO_TAB_ALIGN_MAX_PRE_1_50,
        PANGO_TAB_ALIGN_MAX,
        PANGO_VERSION_1_50,
        runtime_version(),
    )?;
    // `g_return_if_fail (location >= 0)`, measured: a negative location does
    // not reach the array at all, it raises a glib critical and leaves the
    // stop untouched -- and a VM carrying `G_DEBUG=fatal-criticals` turns that
    // into `abort()`. Refusing it here answers `BadArgument` instead, which is
    // the same "nothing happened" with a way for the image to notice.
    let location = as_c_int_positive(location)?;
    with_tab_array(tabs, |t| {
        let index = checked_index(t, index)?;
        let p = pango()?;
        pg!(p, pango_tab_array_set_tab(t, index, align, location));
        Ok(())
    })
}

/// `pango_tab_array_get_tab`. Answers one stop as an Array of two
/// SmallIntegers, `{alignment. location}`.
///
/// `location` is in Pango units, or in device pixels when the array's pixel
/// flag is set; `alignment` is a `PangoTabAlign`, 0..=3.
///
/// `index` is bounds-checked before the call. Out of range, Pango writes
/// **neither** out parameter and this primitive would answer whatever was on
/// the stack -- so an out-of-range index is `BadIndex` and Pango is never
/// reached.
#[pharo_primitive]
fn primitiveTabArrayGetTab(vm: &Interp, tabs: sqInt, index: sqInt) -> PrimResult<Oop> {
    let pair = with_tab_array(tabs, |t| {
        let index = checked_index(t, index)?;
        let p = pango()?;
        let mut align: c_int = 0;
        let mut location: c_int = 0;
        // The `PangoTabAlign *` out parameter is declared `*mut c_int`, never
        // a pointer to a `#[repr(C)]` Rust enum: such an enum carries a
        // validity invariant, and Pango -- not this crate -- wrote that
        // memory, so an unexpected value there would be instant undefined
        // behaviour rather than a number the image can be told about.
        pg!(
            p,
            pango_tab_array_get_tab(t, index, &mut align, &mut location)
        );
        Ok([align, location])
    })?;
    int_array(vm, &pair)
}

/// `pango_tab_array_get_tabs`. Answers all the stops as one flat Array of
/// `2 * size` SmallIntegers: alignment, location, alignment, location, ...
///
/// Interleaved rather than two blocks so that the element pair at `2*i` is
/// exactly what [`primitiveTabArrayGetTab`] answers for index `i`, and an
/// image that changes its mind about which primitive to use does not have to
/// change its indexing with it.
///
/// Locations are in Pango units, or in device pixels when the array's pixel
/// flag is set.
///
/// Both out parameters are transfer-full and are `g_free`d here,
/// independently -- **two** frees, not one, and neither array knows about the
/// other. A zero-size array is not an error: Pango writes NULL into both
/// pointers and this answers an empty Array.
#[pharo_primitive]
fn primitiveTabArrayGetTabs(vm: &Interp, tabs: sqInt) -> PrimResult<Oop> {
    let values = with_tab_array(tabs, |t| {
        let p = pango()?;
        // Resolved before the call rather than after it, so that a glib
        // without `g_free` -- which `init` already refuses to load on -- fails
        // this primitive while there is still nothing allocated to leak.
        let g = ffi::glib()?;
        let size = pg!(p, pango_tab_array_get_size(t));
        let count = usize::try_from(size).map_err(|_| PrimErr::GenericFailure)?;
        let mut aligns: *mut c_int = core::ptr::null_mut();
        let mut locations: *mut c_int = core::ptr::null_mut();
        pg!(
            p,
            pango_tab_array_get_tabs(t, &mut aligns, &mut locations)
        );
        // Copy first, free second, and let nothing fallible run between the
        // two frees: a `?` in there would lose one of the arrays for the life
        // of the process.
        let mut values = Vec::with_capacity(count * 2);
        if count > 0 && !aligns.is_null() && !locations.is_null() {
            // SAFETY: both pointers came from `pango_tab_array_get_tabs`,
            // which documents them as allocated arrays of exactly
            // `pango_tab_array_get_size` elements, and `size` was read from
            // that function on this same array with no call in between that
            // could have resized it.
            let (a, l) = unsafe {
                (
                    core::slice::from_raw_parts(aligns, count),
                    core::slice::from_raw_parts(locations, count),
                )
            };
            for i in 0..count {
                values.push(a[i]);
                values.push(l[i]);
            }
        }
        // `g_free(NULL)` is a documented no-op, so the zero-size case needs no
        // branch of its own.
        ffi::gl!(g, g_free(aligns.cast()));
        ffi::gl!(g, g_free(locations.cast()));
        if count > 0 && values.is_empty() {
            // A non-zero size with a null array: Pango does not do this, and
            // if it ever did, answering a short Array would be worse than
            // failing.
            return Err(PrimErr::GenericFailure);
        }
        Ok(values)
    })?;
    int_array(vm, &values)
}

/// `pango_tab_array_get_positions_in_pixels`. True when locations are device
/// pixels, false when they are Pango units.
///
/// A `gboolean`, which is a four-byte C int and not a Rust bool anywhere on
/// this side of the boundary; the conversion happens once, here.
#[pharo_primitive]
fn primitiveTabArrayGetPositionsInPixels(_vm: &Interp, tabs: sqInt) -> PrimResult<bool> {
    with_tab_array(tabs, |t| {
        let p = pango()?;
        Ok(from_gboolean(pg!(
            p,
            pango_tab_array_get_positions_in_pixels(t)
        )))
    })
}

/// `pango_tab_array_set_positions_in_pixels`, since Pango 1.50; `Unsupported`
/// on an older install.
///
/// **This reinterprets, it does not convert.** Measured: the stored locations
/// are left untouched, so an array built in Pango units and switched to
/// pixels has every stop 1024 times further out than the image meant. Rescale
/// on the image side, or set the flag at construction and never move it.
#[pharo_primitive]
fn primitiveTabArraySetPositionsInPixels(_vm: &Interp, tabs: sqInt, on: bool) -> PrimResult<()> {
    with_tab_array(tabs, |t| {
        let p = pango()?;
        pg!(
            p,
            pango_tab_array_set_positions_in_pixels(t, as_gboolean(on))
        );
        Ok(())
    })
}

/// `pango_tab_array_to_string`, since Pango 1.50; `Unsupported` on an older
/// install.
///
/// The string round-trips through [`primitiveTabArrayFromString`], which makes
/// it the supported way to store a tab array in an image's settings. It is
/// documented as a debugging aid too, but the round trip is the reason it is
/// exposed.
///
/// `char *`, so transfer-full: freed by [`ffi::GStr`] on the way out, which is
/// what keeps an early return between the call and the conversion from losing
/// the buffer.
#[pharo_primitive]
fn primitiveTabArrayToString(_vm: &Interp, tabs: sqInt) -> PrimResult<String> {
    with_tab_array(tabs, |t| {
        let p = pango()?;
        let s = pg!(p, pango_tab_array_to_string(t));
        // SAFETY: `pango_tab_array_to_string` answers a `char *` glib
        // allocated and transfer-full, which is precisely this type's
        // contract; nothing else frees it.
        let s = unsafe { ffi::GStr::from_owned(s) };
        Ok(s.to_string_lossy_owned())
    })
}

/// `pango_tab_array_from_string`, since Pango 1.50; `Unsupported` on an older
/// install.
///
/// Answers a handle on a new array the image owns, or **nil** when the text
/// does not parse. Nil rather than a failure because unparsable text is an
/// ordinary answer for a parser -- the image asked a question and got "no" --
/// and a primitive failure would send it down a fallback path meant for a
/// missing Pango.
#[pharo_primitive]
fn primitiveTabArrayFromString(vm: &Interp, text: Oop) -> PrimResult<Oop> {
    let p = pango()?;
    // NUL-terminated, and an interior NUL is `BadArgument`: this entry takes
    // no length, so a NUL in the middle would silently parse a prefix.
    let text = utf8_cstring(vm, text)?;
    let tabs = pg!(p, pango_tab_array_from_string(text.as_ptr()));
    if tabs.is_null() {
        return vm.nil();
    }
    let handle = register_tab_array(tabs)?;
    vm.integer_checked(handle.raw())
}
