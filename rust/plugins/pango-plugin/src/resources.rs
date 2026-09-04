//! The six things the image holds handles on, how each is released, and the
//! shapes everything crosses the boundary in.
//!
//! Nothing here hands a pointer to the image. A `PangoLayout *` lives in a
//! [`Registry`] and the image gets an integer, so a primitive called with a
//! handle to a destroyed layout fails with `NotFound` and runs its Smalltalk
//! fallback -- where the image-side FFI binding would have dereferenced freed
//! memory inside the VM.
//!
//! **Four release verbs are in play and they are not interchangeable.**
//! `g_object_unref` for the three GObjects, `pango_font_description_free`,
//! `pango_attr_list_unref` and `pango_tab_array_free`. Note the pair that
//! exists to be confused: `PangoAttrList` is refcounted and `PangoTabArray` is
//! not, side by side in one plugin. One registry type per verb, with the verb
//! in that type's own `release`, is what stops a copy-paste from crossing
//! them.
//!
//! # Why there is no `RETAINED_PINS` analogue
//!
//! cairo-plugin has to hold pins past a surface's destruction because a
//! `cairo_surface_t` can outlive the image's handle while still writing into
//! pinned Smalltalk memory, and the plugin cannot observe that from outside.
//! Pango never writes into Smalltalk memory at all: **every `const char *`
//! argument is copied by Pango during the call** -- the `_static` family is
//! the sole exception and is exposed by nothing. So nothing the plugin hands
//! Pango outlives the primitive, and no pin is ever taken.
//!
//! Its graph is also reference-counted in both directions. Measured: a layout
//! takes its own reference on its context (context rc 1 -> 2 across
//! `pango_layout_new`) and a context on its font map. So
//! `ctx := map newContext. layout := ctx newLayout. ctx destroy.` is safe --
//! the image may release handles in any order, and a later
//! `primitiveLayoutGetContext` answers a fresh handle on the same live
//! object. The one thing not protected by a refcount is the borrowed default
//! font map, which is why [`FontMap`] carries `owned` instead.

use core::ffi::{c_char, c_int};
use std::ffi::CString;

use pharo_vm_plugin::handles::Registry;
use pharo_vm_plugin::{sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    self, gl, glib, pango, pg, PangoAttrList, PangoContext, PangoFontDescription, PangoFontMap,
    PangoLayout, PangoLayoutLine, PangoRectangle, PangoTabArray,
};

// ---- the six registry types ---------------------------------------------

/// A source of fonts, and whether this plugin owns a reference on it.
pub struct FontMap {
    ptr: *mut PangoFontMap,
    /// False for the process-wide default from
    /// `pango_cairo_font_map_get_default`, which is transfer-none: the gir
    /// says it "is owned by Pango and must not be freed", and it sits at
    /// refcount 1 for the whole process [measured].
    ///
    /// This flag is the sharpest edge in the API made explicit. Its
    /// transfer-**full** neighbour `pango_cairo_font_map_new` is one line away
    /// in the same header, and a destroy primitive that does not distinguish
    /// the two destroys text rendering for the entire image, at a point
    /// arbitrarily far from the bug.
    owned: bool,
}

/// The font map, language, direction and matrix a layout is laid out against.
pub struct Context {
    ptr: *mut PangoContext,
}

/// A paragraph of text and everything laid out about it.
pub struct Layout {
    ptr: *mut PangoLayout,
}

/// A request for a font. Not refcounted: `pango_font_description_free`.
pub struct FontDesc {
    ptr: *mut PangoFontDescription,
}

/// Attributes over ranges of text. Refcounted: `pango_attr_list_unref`.
pub struct AttrList {
    ptr: *mut PangoAttrList,
}

/// A set of tab stops. **Not** refcounted, unlike its neighbour above:
/// `pango_tab_array_free`, and there is no ref function to reach for.
pub struct TabArray {
    ptr: *mut PangoTabArray,
}

// SAFETY for the six below: these are raw pointers into Pango's heap, and
// Pango's objects are not thread-safe -- but the VM runs every primitive on
// its single interpreter thread, so no two threads ever touch one. `Send` is
// asserted only so the pointers can live in a `static Registry`, which needs
// its contents to be `Send` to be `Sync`. Nothing in this crate spawns a
// thread or moves a handle to one.
unsafe impl Send for FontMap {}
unsafe impl Send for Context {}
unsafe impl Send for Layout {}
unsafe impl Send for FontDesc {}
unsafe impl Send for AttrList {}
unsafe impl Send for TabArray {}

/// Font maps the image holds handles on.
pub static FONT_MAPS: Registry<FontMap> = Registry::new();
/// Contexts the image holds handles on.
pub static CONTEXTS: Registry<Context> = Registry::new();
/// Layouts the image holds handles on.
pub static LAYOUTS: Registry<Layout> = Registry::new();
/// Font descriptions the image holds handles on.
pub static FONT_DESCS: Registry<FontDesc> = Registry::new();
/// Attribute lists the image holds handles on.
pub static ATTR_LISTS: Registry<AttrList> = Registry::new();
/// Tab arrays the image holds handles on.
pub static TAB_ARRAYS: Registry<TabArray> = Registry::new();

// ---- one release verb each ----------------------------------------------

impl FontMap {
    /// Takes ownership of a font map a `transfer=full` producer just made --
    /// `pango_cairo_font_map_new`, or a borrowed one this plugin has taken its
    /// own `g_object_ref` on.
    pub(crate) fn adopt(ptr: *mut PangoFontMap) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr, owned: true })
    }

    /// Records a font map Pango owns, such as
    /// `pango_cairo_font_map_get_default`'s answer. Releasing this handle
    /// drops nothing.
    pub(crate) fn borrow(ptr: *mut PangoFontMap) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr, owned: false })
    }

    /// The raw font map, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoFontMap {
        self.ptr
    }

    /// Does this plugin own a reference on it?
    #[must_use]
    pub fn is_owned(&self) -> bool {
        self.owned
    }

    fn release(self) -> PrimResult<()> {
        if !self.owned {
            return Ok(());
        }
        let g = glib()?;
        gl!(g, g_object_unref(self.ptr.cast()));
        Ok(())
    }
}

impl Context {
    /// Takes ownership of a context a `transfer=full` producer just made.
    pub(crate) fn adopt(ptr: *mut PangoContext) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr })
    }

    /// The raw context, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoContext {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let g = glib()?;
        gl!(g, g_object_unref(self.ptr.cast()));
        Ok(())
    }
}

impl Layout {
    /// Takes ownership of a layout a `transfer=full` producer just made.
    pub(crate) fn adopt(ptr: *mut PangoLayout) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr })
    }

    /// The raw layout, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoLayout {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let g = glib()?;
        gl!(g, g_object_unref(self.ptr.cast()));
        Ok(())
    }
}

impl FontDesc {
    /// Takes ownership of a description a `transfer=full` producer just made.
    ///
    /// `pango_context_get_font_description` and
    /// `pango_layout_get_font_description` are **not** such producers: they
    /// answer a borrow, and it must be copied before it reaches this.
    pub(crate) fn adopt(ptr: *mut PangoFontDescription) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr })
    }

    /// The raw description, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoFontDescription {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let p = pango()?;
        // `free`, never `unref`: a PangoFontDescription is not refcounted.
        pg!(p, pango_font_description_free(self.ptr));
        Ok(())
    }
}

impl AttrList {
    /// Takes ownership of a list a `transfer=full` producer just made, or one
    /// this plugin has taken its own `pango_attr_list_ref` on.
    pub(crate) fn adopt(ptr: *mut PangoAttrList) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr })
    }

    /// The raw list, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoAttrList {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let p = pango()?;
        // `unref`: this one *is* refcounted, unlike the tab array below.
        pg!(p, pango_attr_list_unref(self.ptr));
        Ok(())
    }
}

impl TabArray {
    /// Takes ownership of a tab array a `transfer=full` producer just made.
    pub(crate) fn adopt(ptr: *mut PangoTabArray) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::NoCMemory);
        }
        Ok(Self { ptr })
    }

    /// The raw tab array, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut PangoTabArray {
        self.ptr
    }

    fn release(self) -> PrimResult<()> {
        let p = pango()?;
        // `free`: not refcounted, and there is no `pango_tab_array_unref` to
        // reach for by mistake.
        pg!(p, pango_tab_array_free(self.ptr));
        Ok(())
    }
}

// ---- registering ---------------------------------------------------------
//
// **None of these releases anything on a failed insert, and that is deliberate.**
// `Registry::insert` fails only on `LimitExceeded`, and it stores the value in
// its slot *before* the step that can fail -- so on that path the registry
// still owns the resource and `release_all` releases it exactly once at module
// shutdown. Releasing here as well would be a double free with a dangling
// pointer left in a live slot, which is a worse bug than the leak it would be
// trying to prevent, and arbitrarily far from the primitive that caused it.
// `cairo-plugin` registers the same way for the same reason.

/// Registers a font map this plugin owns a reference on.
pub fn register_font_map_owned(ptr: *mut PangoFontMap) -> PrimResult<sqInt> {
    let map = FontMap::adopt(ptr)?;
    FONT_MAPS.insert(map)
}

/// Registers a font map Pango owns, which this plugin must never unref.
pub fn register_font_map_borrowed(ptr: *mut PangoFontMap) -> PrimResult<sqInt> {
    let map = FontMap::borrow(ptr)?;
    FONT_MAPS.insert(map)
}

/// Registers a context this plugin owns a reference on.
pub fn register_context(ptr: *mut PangoContext) -> PrimResult<sqInt> {
    let context = Context::adopt(ptr)?;
    CONTEXTS.insert(context)
}

/// Registers a layout this plugin owns a reference on.
pub fn register_layout(ptr: *mut PangoLayout) -> PrimResult<sqInt> {
    let layout = Layout::adopt(ptr)?;
    LAYOUTS.insert(layout)
}

/// Registers a font description this plugin owns.
pub fn register_font_desc(ptr: *mut PangoFontDescription) -> PrimResult<sqInt> {
    let desc = FontDesc::adopt(ptr)?;
    FONT_DESCS.insert(desc)
}

/// Registers an attribute list this plugin owns a reference on.
pub fn register_attr_list(ptr: *mut PangoAttrList) -> PrimResult<sqInt> {
    let list = AttrList::adopt(ptr)?;
    ATTR_LISTS.insert(list)
}

/// Registers a tab array this plugin owns.
pub fn register_tab_array(ptr: *mut PangoTabArray) -> PrimResult<sqInt> {
    let tabs = TabArray::adopt(ptr)?;
    TAB_ARRAYS.insert(tabs)
}

// ---- destroying ---------------------------------------------------------

/// Destroys the font map `handle` names, unref'ing it only if this plugin
/// owned a reference. Destroying twice fails the second time.
pub fn destroy_font_map(handle: sqInt) -> PrimResult<()> {
    FONT_MAPS.remove(handle)?.release()
}

/// Destroys the context `handle` names.
///
/// This drops *one* reference. Any layout made from the context holds another,
/// so the `PangoContext` itself stays alive for as long as it is reachable --
/// the image loses its handle, not the object.
pub fn destroy_context(handle: sqInt) -> PrimResult<()> {
    CONTEXTS.remove(handle)?.release()
}

/// Destroys the layout `handle` names.
pub fn destroy_layout(handle: sqInt) -> PrimResult<()> {
    LAYOUTS.remove(handle)?.release()
}

/// Destroys the font description `handle` names.
pub fn destroy_font_desc(handle: sqInt) -> PrimResult<()> {
    FONT_DESCS.remove(handle)?.release()
}

/// Destroys the attribute list `handle` names.
pub fn destroy_attr_list(handle: sqInt) -> PrimResult<()> {
    ATTR_LISTS.remove(handle)?.release()
}

/// Destroys the tab array `handle` names.
pub fn destroy_tab_array(handle: sqInt) -> PrimResult<()> {
    TAB_ARRAYS.remove(handle)?.release()
}

/// Releases everything still registered, for the module's shutdown hook.
///
/// The order is tidiness rather than correctness, unlike Cairo's: a layout
/// holds a reference on its context and a context on its font map, so
/// dropping in this order takes each refcount to zero exactly once whatever
/// order it is written in. Borrowed font maps are skipped by `release`
/// itself.
pub fn release_all() {
    for l in LAYOUTS.drain() {
        let _ = l.release();
    }
    for c in CONTEXTS.drain() {
        let _ = c.release();
    }
    for d in FONT_DESCS.drain() {
        let _ = d.release();
    }
    for a in ATTR_LISTS.drain() {
        let _ = a.release();
    }
    for t in TAB_ARRAYS.drain() {
        let _ = t.release();
    }
    for m in FONT_MAPS.drain() {
        let _ = m.release();
    }
}

/// How many resources are live in each registry, in the order the image's
/// `primitiveLiveResourceCounts` reports them.
#[must_use]
pub fn live_counts() -> [usize; 6] {
    [
        FONT_MAPS.len(),
        CONTEXTS.len(),
        LAYOUTS.len(),
        FONT_DESCS.len(),
        ATTR_LISTS.len(),
        TAB_ARRAYS.len(),
    ]
}

// ---- reaching a resource -------------------------------------------------

/// Runs `f` on the font map `handle` names.
pub fn with_font_map<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoFontMap) -> PrimResult<R>,
) -> PrimResult<R> {
    FONT_MAPS.with(handle, FontMap::as_ptr).and_then(f)
}

/// Does this handle name a font map Pango owns rather than one this plugin
/// does?
pub fn font_map_is_borrowed(handle: sqInt) -> PrimResult<bool> {
    FONT_MAPS.with(handle, |m| !m.is_owned())
}

/// Runs `f` on the context `handle` names.
pub fn with_context<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoContext) -> PrimResult<R>,
) -> PrimResult<R> {
    CONTEXTS.with(handle, Context::as_ptr).and_then(f)
}

/// Runs `f` on the layout `handle` names.
pub fn with_layout<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoLayout) -> PrimResult<R>,
) -> PrimResult<R> {
    LAYOUTS.with(handle, Layout::as_ptr).and_then(f)
}

/// Runs `f` on the font description `handle` names.
pub fn with_font_desc<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoFontDescription) -> PrimResult<R>,
) -> PrimResult<R> {
    FONT_DESCS.with(handle, FontDesc::as_ptr).and_then(f)
}

/// Runs `f` on the attribute list `handle` names.
pub fn with_attr_list<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoAttrList) -> PrimResult<R>,
) -> PrimResult<R> {
    ATTR_LISTS.with(handle, AttrList::as_ptr).and_then(f)
}

/// Runs `f` on the tab array `handle` names.
pub fn with_tab_array<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoTabArray) -> PrimResult<R>,
) -> PrimResult<R> {
    TAB_ARRAYS.with(handle, TabArray::as_ptr).and_then(f)
}

// The four setters that accept NULL -- `set_attributes`, `set_font_description`,
// `set_tabs`, and `get_metrics`'s description -- take handle 0 to mean it.
// `Registry` generations start at 1, so 0 is never a live handle and the two
// meanings cannot collide.

/// Runs `f` on the attribute list `handle` names, or on NULL when it is 0.
pub fn with_optional_attr_list<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoAttrList) -> PrimResult<R>,
) -> PrimResult<R> {
    if handle == 0 {
        return f(core::ptr::null_mut());
    }
    with_attr_list(handle, f)
}

/// Runs `f` on the font description `handle` names, or on NULL when it is 0.
pub fn with_optional_font_desc<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoFontDescription) -> PrimResult<R>,
) -> PrimResult<R> {
    if handle == 0 {
        return f(core::ptr::null_mut());
    }
    with_font_desc(handle, f)
}

/// Runs `f` on the tab array `handle` names, or on NULL when it is 0.
pub fn with_optional_tab_array<R>(
    handle: sqInt,
    f: impl FnOnce(*mut PangoTabArray) -> PrimResult<R>,
) -> PrimResult<R> {
    if handle == 0 {
        return f(core::ptr::null_mut());
    }
    with_tab_array(handle, f)
}

/// Runs `f` on one line of a layout, and drops the pointer before returning.
///
/// **The only way to touch a `PangoLayoutLine` in this crate, and no pointer
/// of that type may leave the closure.** A line is invalidated by any change
/// to its layout, and a registry handle cannot express that: the handle would
/// stay live, its generation would still match, and the plugin would
/// dereference memory Pango had freed -- inside the VM, with no Smalltalk
/// fallback. `Registry` guards stale *handles*, not stale *pointees*.
///
/// The index is range-checked against `pango_layout_get_line_count` first,
/// because `pango_layout_get_line_readonly` `g_return_val_if_fail`s on a
/// negative index -- which under `G_DEBUG=fatal-criticals` aborts the whole VM
/// -- and answers NULL past the end.
///
/// `get_line_readonly` rather than `get_line`: the transfer semantics are
/// identical, but `get_line` marks the line "leaked" inside Pango and disables
/// its glyph cache, and nothing here mutates a line.
pub fn with_line<R>(
    layout: sqInt,
    index: sqInt,
    f: impl FnOnce(*mut PangoLayoutLine) -> PrimResult<R>,
) -> PrimResult<R> {
    let index = as_c_int(index)?;
    with_layout(layout, |l| {
        let p = pango()?;
        let count = pg!(p, pango_layout_get_line_count(l));
        if index < 0 || index >= count {
            return Err(PrimErr::BadIndex);
        }
        let line = pg!(p, pango_layout_get_line_readonly(l, index));
        if line.is_null() {
            return Err(PrimErr::BadIndex);
        }
        f(line)
    })
}

// ---- narrowing and range-checking ---------------------------------------

/// Narrows an image integer to the `int` every Pango integer in this API is.
///
/// `sqInt` is 64-bit and every Pango dimension is 32-bit, so this is not
/// pedantry: an unchecked truncation of a large positive value can land
/// exactly on -1, which `set_width`, `set_height`, `set_text`'s length and
/// `move_cursor_visually`'s `new_index` all read as a meaningful sentinel. The
/// image would get "no wrapping" where it asked for a very wide layout.
pub fn as_c_int(value: sqInt) -> PrimResult<c_int> {
    c_int::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Narrows an image integer that must also be non-negative.
///
/// For counts and sizes only. Do **not** use it on a dimension: `set_width`,
/// `set_height` and `set_indent` all take meaningful negative values, and
/// `index_to_pos` answers one.
pub fn as_c_int_positive(value: sqInt) -> PrimResult<c_int> {
    let v = as_c_int(value)?;
    if v < 0 {
        return Err(PrimErr::BadArgument);
    }
    Ok(v)
}

/// A glib `gboolean` argument: `1` or `0`, four bytes, never a Rust bool.
#[must_use]
pub fn as_gboolean(flag: bool) -> c_int {
    c_int::from(flag)
}

/// A glib `gboolean` answer as a Rust bool: any non-zero value is true.
#[must_use]
pub fn from_gboolean(value: c_int) -> bool {
    value != 0
}

/// Range-checks a C enum before it crosses the boundary.
///
/// Pango's switch statements have no default arm and its setters do not check:
/// `pango_layout_set_wrap(l, 3)` on a pre-1.56 Pango stores a value that falls
/// through every case in the line breaker, and the result is a layout that is
/// subtly wrong rather than a failure anyone can see. Rejecting the value here
/// fails the primitive that got it wrong instead.
pub fn enum_in(value: sqInt, lo: c_int, hi: c_int) -> PrimResult<c_int> {
    let v = as_c_int(value)?;
    if v < lo || v > hi {
        return Err(PrimErr::BadArgument);
    }
    Ok(v)
}

/// The same, for a range that only exists from Pango `hi_since` onwards.
///
/// Answers `Unsupported` rather than `BadArgument` when the runtime Pango is
/// too old, because that is what it is: the value is legal, this installation
/// cannot express it, and the image should be able to tell that from a typo.
pub fn enum_in_since(
    value: sqInt,
    lo: c_int,
    hi: c_int,
    hi_since: c_int,
    version: c_int,
) -> PrimResult<c_int> {
    let v = enum_in(value, lo, hi)?;
    if version < hi_since {
        return Err(PrimErr::Unsupported);
    }
    Ok(v)
}

/// Range-checks an enum whose upper end arrived in a later Pango.
///
/// `lo..=hi` is always legal; `hi+1..=extended_hi` needs a Pango at least
/// `since`. This is the shape all three real cases have -- `PangoWrapMode`
/// gained NONE in 1.56, `PangoVariant` gained four values in 1.50, and
/// `PangoTabAlign` gained three -- so writing the composition once keeps three
/// modules from each inventing their own.
pub fn enum_in_or_since(
    value: sqInt,
    lo: c_int,
    hi: c_int,
    extended_hi: c_int,
    since: c_int,
    version: c_int,
) -> PrimResult<c_int> {
    match enum_in(value, lo, hi) {
        Ok(v) => Ok(v),
        Err(_) => enum_in_since(value, hi + 1, extended_hi, since, version),
    }
}

/// The loaded Pango's version as `pango_version()` reports it, or 0.
///
/// 0 when Pango is not loaded or does not export `pango_version`, which makes
/// every version-gated check refuse -- the safe direction, since the gate
/// exists to keep a value out of a Pango that would mishandle it.
#[must_use]
pub fn runtime_version() -> c_int {
    let Ok(p) = pango() else { return 0 };
    let Some(f) = p.pango_version else { return 0 };
    // SAFETY: the pointer came out of the libpangocairo we loaded, and
    // `pango_version` takes no arguments and answers an int.
    unsafe { f() }
}

// ---- the wire format -----------------------------------------------------
//
// Every one of these builds a fresh Pharo object. `Interp::instantiate` never
// runs a collection -- in Spur the image decides when to collect -- so an
// object built here cannot move while the next one is being built, and nested
// Arrays can be assembled in whatever order reads best.

/// Answers a slice of `int`s as an Array of SmallIntegers.
///
/// The transport for every integral quantity in this plugin. A ByteArray of
/// native words was the alternative and is worse: the SDK's only word writer
/// takes `&[u32]`, and Pango's ints are *signed* -- `index_to_pos` answers a
/// negative width for a right-to-left grapheme -- so the image would have to
/// re-sign them, which is exactly the decoding step that gets got wrong once.
pub fn int_array(vm: &Interp, values: &[c_int]) -> PrimResult<Oop> {
    let size = sqInt::try_from(values.len()).map_err(|_| PrimErr::LimitExceeded)?;
    let array = vm.instantiate(vm.class_array()?, size)?;
    for (i, value) in values.iter().enumerate() {
        // A `c_int` always fits an `isize` on every target this VM builds for.
        let oop = vm.integer_checked(*value as sqInt)?;
        vm.store_pointer(sqInt::try_from(i).map_err(|_| PrimErr::LimitExceeded)?, array, oop)?;
    }
    Ok(array)
}

/// Answers a `PangoRectangle` as an Array of four SmallIntegers,
/// `{x. y. width. height}`.
///
/// In Pango units, except from the `*_get_pixel_extents` family. The primitive
/// that answers one names its unit; nothing here converts.
pub fn rect_array(vm: &Interp, r: &PangoRectangle) -> PrimResult<Oop> {
    int_array(vm, &[r.x, r.y, r.width, r.height])
}

/// Answers two rectangles as one 8-element Array, first then second.
///
/// Pango's extents functions come in ink/logical pairs, and the image wants
/// both or neither; two Arrays inside an Array would cost an allocation and
/// an indirection for no information.
pub fn rect_pair_array(vm: &Interp, a: &PangoRectangle, b: &PangoRectangle) -> PrimResult<Oop> {
    int_array(
        vm,
        &[a.x, a.y, a.width, a.height, b.x, b.y, b.width, b.height],
    )
}

/// Answers a slice of Rust strings as an Array of Strings.
pub fn string_array(vm: &Interp, values: &[String]) -> PrimResult<Oop> {
    let size = sqInt::try_from(values.len()).map_err(|_| PrimErr::LimitExceeded)?;
    let array = vm.instantiate(vm.class_array()?, size)?;
    for (i, value) in values.iter().enumerate() {
        let oop = vm.string(value)?;
        vm.store_pointer(sqInt::try_from(i).map_err(|_| PrimErr::LimitExceeded)?, array, oop)?;
    }
    Ok(array)
}

/// Answers a slice of already-built objects as an Array.
///
/// For the enumeration primitives, whose elements are themselves Arrays.
pub fn oop_array(vm: &Interp, values: &[Oop]) -> PrimResult<Oop> {
    let size = sqInt::try_from(values.len()).map_err(|_| PrimErr::LimitExceeded)?;
    let array = vm.instantiate(vm.class_array()?, size)?;
    for (i, value) in values.iter().enumerate() {
        vm.store_pointer(
            sqInt::try_from(i).map_err(|_| PrimErr::LimitExceeded)?,
            array,
            *value,
        )?;
    }
    Ok(array)
}

/// Reads a Smalltalk string as valid UTF-8.
///
/// `Interp::string_value`, never `c_string_value`: a Pharo ByteString holds
/// Latin-1 unless it holds UTF-8, and `string_value` decodes both, while
/// `c_string_value` passes the raw bytes through and every accented character
/// would reach Pango as an invalid sequence. **Pango on invalid UTF-8 is
/// undefined behaviour, not an error return**, so this is the difference
/// between a Smalltalk fallback and a VM crash.
pub fn utf8_text(vm: &Interp, oop: Oop) -> PrimResult<String> {
    vm.string_value(oop)
}

/// The same, NUL-terminated, for the entries that take no length.
///
/// Fails with `BadArgument` on an interior NUL, which no C string can carry.
/// Where Pango takes an explicit length -- `pango_layout_set_text`,
/// `set_markup`, `pango_parse_markup` -- prefer [`utf8_text`] and pass the
/// string's real byte count rather than -1: a Smalltalk String is not
/// NUL-terminated and may legitimately contain a NUL.
pub fn utf8_cstring(vm: &Interp, oop: Oop) -> PrimResult<CString> {
    CString::new(utf8_text(vm, oop)?).map_err(|_| PrimErr::BadArgument)
}

/// Reads a `GError` out-parameter as a message, frees it, and answers the
/// failure the primitive should report.
///
/// `message` belongs to the `GError` and dies with it, so it is copied out
/// first. Answers `None` when `error` is null, which means the callee failed
/// without saying why -- glib allows that, and it must not be reported as a
/// success.
///
/// # Safety
///
/// `error` must be null, or a `GError *` glib allocated and handed over
/// `transfer=full`, as every `GError **` out-parameter in Pango is.
pub unsafe fn take_gerror(error: *mut ffi::GError) -> Option<(c_int, String)> {
    if error.is_null() {
        return None;
    }
    // SAFETY: non-null by the check above, and a live GError by this
    // function's contract.
    let (code, message) = unsafe {
        let code = (*error).code;
        let message: *const c_char = (*error).message;
        // SAFETY: the message is NUL-terminated and owned by the GError, so
        // it must be copied before the free below.
        (code, ffi::borrowed_str(message).unwrap_or_default())
    };
    if let Ok(g) = glib() {
        if let Some(free) = g.g_error_free {
            // SAFETY: `error` is a live GError this call owns, freed once.
            unsafe { free(error) };
        }
    }
    Some((code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{
        PANGO_TAB_ALIGN_MAX, PANGO_VERSION_1_50, PANGO_VERSION_1_56, PANGO_WRAP_MAX,
        PANGO_WRAP_NONE,
    };

    #[test]
    fn narrowing_rejects_a_value_that_would_land_on_a_sentinel() {
        // The failure this exists to prevent: 2^32 - 1 truncates to exactly
        // -1, which `set_width` reads as "no wrapping at all".
        assert_eq!(as_c_int(0xFFFF_FFFF), Err(PrimErr::BadArgument));
        assert_eq!(as_c_int(-1), Ok(-1));
        assert_eq!(as_c_int(307_200), Ok(307_200));
    }

    #[test]
    fn a_count_may_not_be_negative_but_a_dimension_may() {
        assert_eq!(as_c_int_positive(-1), Err(PrimErr::BadArgument));
        assert_eq!(as_c_int_positive(0), Ok(0));
        assert_eq!(as_c_int(-1), Ok(-1), "set_width(-1) is a legal sentinel");
    }

    #[test]
    fn a_gboolean_crosses_as_one_or_zero() {
        assert_eq!(as_gboolean(true), 1);
        assert_eq!(as_gboolean(false), 0);
        // And any non-zero value read back is true, which is glib's own rule.
        assert!(from_gboolean(1));
        assert!(from_gboolean(-1));
        assert!(!from_gboolean(0));
    }

    #[test]
    fn an_out_of_range_enum_is_refused_rather_than_forwarded() {
        assert_eq!(enum_in(0, 0, 2), Ok(0));
        assert_eq!(enum_in(2, 0, 2), Ok(2));
        assert_eq!(enum_in(3, 0, 2), Err(PrimErr::BadArgument));
        assert_eq!(enum_in(-1, 0, 2), Err(PrimErr::BadArgument));
    }

    #[test]
    fn a_weight_is_a_range_and_not_an_ordinal() {
        use crate::ffi::{PANGO_WEIGHT_MAX, PANGO_WEIGHT_MIN};
        // 450 is a legal weight off a variable font's axis, and a membership
        // test against the named constants would have rejected it.
        assert_eq!(enum_in(450, PANGO_WEIGHT_MIN, PANGO_WEIGHT_MAX), Ok(450));
        assert_eq!(
            enum_in(0, PANGO_WEIGHT_MIN, PANGO_WEIGHT_MAX),
            Err(PrimErr::BadArgument),
            "an ordinal check would have accepted this"
        );
        assert_eq!(
            enum_in(1001, PANGO_WEIGHT_MIN, PANGO_WEIGHT_MAX),
            Err(PrimErr::BadArgument)
        );
    }

    #[test]
    fn a_value_a_newer_pango_added_is_unsupported_rather_than_bad() {
        // PANGO_WRAP_NONE arrived in 1.56. On an older Pango the value is not
        // wrong, it is unavailable, and the image can tell the two apart.
        assert_eq!(
            enum_in_or_since(
                PANGO_WRAP_NONE as sqInt,
                0,
                PANGO_WRAP_MAX,
                PANGO_WRAP_NONE,
                PANGO_VERSION_1_56,
                15000
            ),
            Err(PrimErr::Unsupported)
        );
        assert_eq!(
            enum_in_or_since(
                PANGO_WRAP_NONE as sqInt,
                0,
                PANGO_WRAP_MAX,
                PANGO_WRAP_NONE,
                PANGO_VERSION_1_56,
                15802
            ),
            Ok(PANGO_WRAP_NONE)
        );
    }

    #[test]
    fn the_always_legal_part_of_a_gated_range_does_not_need_the_version() {
        for wrap in 0..=PANGO_WRAP_MAX {
            assert_eq!(
                enum_in_or_since(
                    wrap as sqInt,
                    0,
                    PANGO_WRAP_MAX,
                    PANGO_WRAP_NONE,
                    PANGO_VERSION_1_56,
                    10000
                ),
                Ok(wrap)
            );
        }
        // And something outside both ranges is still simply bad input.
        assert_eq!(
            enum_in_or_since(9, 0, PANGO_WRAP_MAX, PANGO_WRAP_NONE, PANGO_VERSION_1_56, 15802),
            Err(PrimErr::BadArgument)
        );
    }

    #[test]
    fn tab_alignments_past_left_are_gated_on_1_50() {
        use crate::ffi::PANGO_TAB_ALIGN_MAX_PRE_1_50;
        assert_eq!(
            enum_in_or_since(
                3,
                0,
                PANGO_TAB_ALIGN_MAX_PRE_1_50,
                PANGO_TAB_ALIGN_MAX,
                PANGO_VERSION_1_50,
                14600
            ),
            Err(PrimErr::Unsupported)
        );
        assert_eq!(
            enum_in_or_since(
                3,
                0,
                PANGO_TAB_ALIGN_MAX_PRE_1_50,
                PANGO_TAB_ALIGN_MAX,
                PANGO_VERSION_1_50,
                15802
            ),
            Ok(3)
        );
    }

    #[test]
    fn a_handle_of_zero_is_never_live_so_it_can_mean_null() {
        // The wire convention the four NULL-accepting setters depend on:
        // `Registry` generations start at 1, so no live handle is ever 0.
        assert!(!LAYOUTS.is_live(0));
        assert!(!ATTR_LISTS.is_live(0));
        assert!(!FONT_DESCS.is_live(0));
        assert!(!TAB_ARRAYS.is_live(0));
        let seen = with_optional_attr_list(0, |p| Ok(p.is_null())).unwrap();
        assert!(seen, "handle 0 must reach the callee as NULL");
    }

    #[test]
    fn a_destroyed_handle_fails_rather_than_naming_its_successor() {
        // Without Pango loaded nothing can be registered, so this asserts the
        // half that holds everywhere: an unknown handle is NotFound, not a
        // silently wrong resource.
        assert_eq!(with_layout(1, |_| Ok(())), Err(PrimErr::NotFound));
        assert_eq!(destroy_layout(1), Err(PrimErr::NotFound));
    }

    #[test]
    fn the_live_counts_are_reported_in_a_fixed_order() {
        // The image reads these positionally, so the order is API.
        let counts = live_counts();
        assert_eq!(counts.len(), 6);
        assert_eq!(counts[0], FONT_MAPS.len());
        assert_eq!(counts[5], TAB_ARRAYS.len());
    }

    #[test]
    fn the_runtime_version_is_zero_when_pango_is_absent() {
        // And a gated check then refuses, which is the safe direction.
        let version = runtime_version();
        if pango().is_err() {
            assert_eq!(version, 0);
        } else {
            assert!(version >= 10000, "an implausible pango_version(): {version}");
        }
    }
}
