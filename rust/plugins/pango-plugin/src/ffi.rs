//! The Pango and glib bindings: types, constants, both function tables, and
//! the four `dlopen`s that fill them.
//!
//! Bound at runtime rather than at link time, and for a stronger reason than
//! cairo-plugin has. Cairo arrives in the bundle: `cmake/importCairo.cmake`
//! downloads a ready-made binary, so the file is always there and only its
//! version varies. **Nothing in `cmake/` downloads Pango.** It is whatever the
//! machine happens to have installed, so its complete absence is the ordinary
//! case, not the exceptional one, and the module has to decline cleanly enough
//! that the image can tell "no Pango here" from "primitive not implemented".
//!
//! Four libraries are opened, not one:
//!
//! * **`libpangocairo`** -- and every `pango_*` and `pango_cairo_*` entry is
//!   resolved from that one handle. Measured: a single `dlopen` of
//!   libpangocairo resolves `pango_*`, `pango_cairo_*`, `g_*` and even
//!   `cairo_*` through its dependency graph, on both dyld and glibc, while a
//!   libpango-only handle resolves no `pango_cairo_*` at all.
//! * **`libgobject`** and **`libglib`**, separately, with the `g_*` entries
//!   resolved from their own handles. macOS obliges a `g_free` lookup on the
//!   pangocairo handle, but `GetProcAddress` on a pango DLL will not, and the
//!   point of going through `dylib` at all is that the three platforms behave
//!   the same.
//! * **`libpango`** last, opened but never resolved from, purely so a
//!   diagnostic can name the file.
//!
//! Every entry is an `Option`, and a missing one costs its primitives rather
//! than the module: Pango is versioned independently of the VM, and this table
//! declares entries introduced as recently as 1.58 alongside entries that have
//! been there since 1.4. [`Pango::missing_entry_points`] is the only thing in
//! the tree that catches a misspelt name -- a typo leaves the `Option`
//! permanently `None`, which looks exactly like a Pango too old to have it --
//! and [`entries_introduced_after`] is what lets the live test tell those two
//! apart.
//!
//! Signatures are transcribed by hand from the 1.58.2 headers. Nothing in this
//! build catches a wrong *type*: it compiles, links, resolves, and then
//! produces garbage or corrupts memory. The three that bite are called out
//! where they are declared.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_double, c_float, c_int, c_uint, c_void};
use std::sync::OnceLock;

use pharo_vm_plugin::dylib;
use pharo_vm_plugin::{PrimErr, PrimResult};

// ---- glib scalar typedefs ----------------------------------------------

/// `gboolean`, `gtypes.h:55-56`: `typedef gint gboolean`, and `gint` is `int`.
///
/// **Four bytes, and never a Rust `bool`.** A Rust `bool` is one byte *and*
/// carries a validity invariant of exactly 0 or 1, so declaring a `gboolean`
/// return as `bool` is undefined behaviour the moment Pango answers anything
/// else -- and glib's own convention is that any non-zero value is true.
/// Convert at the edge with `!= 0`, never in the declaration.
pub type gboolean = c_int;

/// `gunichar`, `gunicode.h`: a UCS-4 code point in a `guint32`.
pub type gunichar = c_uint;

/// `GQuark`, `gquark.h`: `typedef guint32 GQuark`.
pub type GQuark = u32;

/// `PangoLogAttr`, `pango-break.h:83-102`: fifteen one-bit fields and a
/// seventeen-bit reserved tail, so exactly one 32-bit word.
///
/// Deliberately a plain `u32` rather than a Rust struct of bitfields. Rust has
/// no bitfields, the packing order of C's is implementation-defined, and the
/// image is going to mask the bits itself anyway; modelling it here would be a
/// second place to get the bit numbering wrong.
pub type PangoLogAttr = u32;

// ---- opaque types -------------------------------------------------------

/// A `PangoContext`: the font map, language, direction and matrix a layout is
/// laid out against.
#[repr(C)]
pub struct PangoContext {
    _private: [u8; 0],
}

/// A `PangoLayout`: a paragraph of text and everything laid out about it.
#[repr(C)]
pub struct PangoLayout {
    _private: [u8; 0],
}

/// One line of a laid-out `PangoLayout`.
///
/// Refcounted, but the reference protects the allocation and not its meaning:
/// a line is invalidated by any change to its layout. No pointer of this type
/// ever reaches the image; see [`crate::resources::with_line`].
#[repr(C)]
pub struct PangoLayoutLine {
    _private: [u8; 0],
}

/// A cursor over a layout's lines, runs and clusters. Not exposed in v1, for
/// the same reason as [`PangoLayoutLine`].
#[repr(C)]
pub struct PangoLayoutIter {
    _private: [u8; 0],
}

/// A source of fonts.
#[repr(C)]
pub struct PangoFontMap {
    _private: [u8; 0],
}

/// The same object as a [`PangoFontMap`], at the same address, seen through
/// PangoCairo's interface.
///
/// A distinct Rust type on purpose. `pango_cairo_font_map_get_default` answers
/// `PangoFontMap *` (pangocairo.h:110) while
/// `pango_cairo_font_map_set_resolution` takes `PangoCairoFontMap *`
/// (pangocairo.h:117). In C the caller writes the checked
/// `PANGO_CAIRO_FONT_MAP()` cast; GObject interfaces are not separate
/// pointers, so a Rust `.cast()` is correct -- but making that cast *visible*
/// at each call site is the point, because Rust checks nothing and handing a
/// non-PangoCairo font map to those entries is undefined behaviour where C
/// would at least print a critical.
#[repr(C)]
pub struct PangoCairoFontMap {
    _private: [u8; 0],
}

/// A loaded font.
#[repr(C)]
pub struct PangoFont {
    _private: [u8; 0],
}

/// A loaded font, seen through PangoCairo's interface. See
/// [`PangoCairoFontMap`] for why this is a separate type.
#[repr(C)]
pub struct PangoCairoFont {
    _private: [u8; 0],
}

/// A family of faces, borrowed from a font map and never owned by the plugin.
#[repr(C)]
pub struct PangoFontFamily {
    _private: [u8; 0],
}

/// One face of a family, borrowed from it and never owned by the plugin.
#[repr(C)]
pub struct PangoFontFace {
    _private: [u8; 0],
}

/// An ordered set of fonts to try for one language.
#[repr(C)]
pub struct PangoFontset {
    _private: [u8; 0],
}

/// A font's overall measurements. Refcounted; read in one primitive and
/// unref'd there, so it never becomes a handle.
#[repr(C)]
pub struct PangoFontMetrics {
    _private: [u8; 0],
}

/// A request for a font: family, style, weight, size and the rest.
#[repr(C)]
pub struct PangoFontDescription {
    _private: [u8; 0],
}

/// A list of attributes over ranges of text. Refcounted -- `unref`, not
/// `free`, unlike its neighbour [`PangoTabArray`].
#[repr(C)]
pub struct PangoAttrList {
    _private: [u8; 0],
}

/// One attribute over one range. Not refcounted; `pango_attribute_destroy`,
/// except that three of the list functions swallow it.
#[repr(C)]
pub struct PangoAttribute {
    _private: [u8; 0],
}

/// A cursor over a `PangoAttrList`. Released with
/// `pango_attr_iterator_destroy` -- **not** `_free`, which is what releases a
/// [`PangoLayoutIter`]. Two iterators, two verbs; neither is exposed in v1.
#[repr(C)]
pub struct PangoAttrIterator {
    _private: [u8; 0],
}

/// A set of tab stops. Not refcounted: `pango_tab_array_free`, and there is no
/// ref function to reach for by mistake.
#[repr(C)]
pub struct PangoTabArray {
    _private: [u8; 0],
}

/// An RFC-3066 language tag, canonicalised and interned.
///
/// Static and immortal: no free function exists, every producer is
/// transfer-none, and the docs say the pointer *is* the value. It gets no
/// registry and no destroy path -- the image holds Strings and the plugin
/// calls `pango_language_from_string` at each use, which is a hash lookup into
/// a permanent intern table rather than an allocation.
#[repr(C)]
pub struct PangoLanguage {
    _private: [u8; 0],
}

/// A Cairo drawing context, declared here rather than shared with
/// cairo-plugin: the two crates do not depend on each other, and one is
/// opaque either way.
#[repr(C)]
pub struct cairo_t {
    _private: [u8; 0],
}

/// Cairo's font rendering options. Borrowed from a context; never destroyed
/// here.
#[repr(C)]
pub struct cairo_font_options_t {
    _private: [u8; 0],
}

/// A Cairo font at a particular size and transform.
#[repr(C)]
pub struct cairo_scaled_font_t {
    _private: [u8; 0],
}

/// A glib byte buffer, from `pango_layout_serialize`. Declared so the
/// serialization names are checked; no primitive answers one in v1.
#[repr(C)]
pub struct GBytes {
    _private: [u8; 0],
}

// ---- transparent types --------------------------------------------------

/// `PangoRectangle`, pango-types.h:169. Four ints, 16 bytes, align 4
/// [measured].
///
/// Values are in Pango units -- `device_units * PANGO_SCALE` -- except from
/// `*_get_pixel_extents`, which answers pixels. `x`, `y` and `width` are all
/// routinely negative: `PANGO_ASCENT(r)` is `-r.y`, and `index_to_pos`
/// answers a negative `width` for a right-to-left grapheme.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PangoRectangle {
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
}

/// `PangoMatrix`, pango-matrix.h. 48 bytes, align 8 [measured].
///
/// Field order is `xx, xy, yx, yy, x0, y0` -- the middle two are **transposed**
/// relative to `cairo_matrix_t`, which is `xx, yx, xy, yy, x0, y0`. Same
/// mathematical convention, opposite storage. Both are six doubles, so a
/// memcpy between them compiles, links, and silently transposes every rotation
/// and skew while leaving identity, scale and translation looking perfectly
/// right. That is why [`PangoMatrix::from_cairo_slice`] exists and why the
/// unit tests below assert the two orders differ.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PangoMatrix {
    pub xx: c_double,
    pub xy: c_double,
    pub yx: c_double,
    pub yy: c_double,
    pub x0: c_double,
    pub y0: c_double,
}

impl PangoMatrix {
    /// The six doubles in **Pango's** order, which is the order the image
    /// sends and receives them in.
    #[must_use]
    pub fn to_pango_array(self) -> [f64; 6] {
        [self.xx, self.xy, self.yx, self.yy, self.x0, self.y0]
    }

    /// The six doubles in **Cairo's** order, for handing one to Cairo.
    #[must_use]
    pub fn to_cairo_array(self) -> [f64; 6] {
        [self.xx, self.yx, self.xy, self.yy, self.x0, self.y0]
    }

    /// Rebuilds a matrix from six doubles in Pango's order.
    #[must_use]
    pub fn from_pango_slice(v: &[f64]) -> Option<Self> {
        match v {
            [xx, xy, yx, yy, x0, y0] => Some(Self {
                xx: *xx,
                xy: *xy,
                yx: *yx,
                yy: *yy,
                x0: *x0,
                y0: *y0,
            }),
            _ => None,
        }
    }

    /// Rebuilds a matrix from six doubles in **Cairo's** order.
    ///
    /// Elements 1 and 2 swap. This is the whole conversion, and writing it as
    /// a swap rather than a memcpy is the entire defence against a silently
    /// transposed transform.
    #[must_use]
    pub fn from_cairo_slice(v: &[f64]) -> Option<Self> {
        match v {
            [xx, yx, xy, yy, x0, y0] => Some(Self {
                xx: *xx,
                xy: *xy,
                yx: *yx,
                yy: *yy,
                x0: *x0,
                y0: *y0,
            }),
            _ => None,
        }
    }
}

/// `PangoColor`, pango-color.h. Three `guint16`, size 6, align 2 [measured].
///
/// **No alpha field -- do not add one.** Alpha comes back through a separate
/// `guint16 *` out parameter on `pango_color_parse_with_alpha`. The channels
/// are 16-bit, and the conversion from the image's 8 bits is `v * 257`
/// (== `v << 8 | v`), never `v << 8`: `0xFF << 8` is 65280, so white would
/// not round-trip.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PangoColor {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

/// `GError`, gerror.h:45. Size 16, align 8 [measured].
///
/// `message` belongs to the `GError` and dies with it, so it must be copied
/// into a Rust `String` **before** `g_error_free` runs.
#[repr(C)]
pub struct GError {
    pub domain: GQuark,
    pub code: c_int,
    pub message: *mut c_char,
}

/// `GSList`, gslist.h. Two pointers, size 16, align 8 [measured].
///
/// Walked directly from this struct rather than through `g_slist_length` and
/// `g_slist_nth_data`, which are O(n) each and would make a walk O(n squared).
#[repr(C)]
pub struct GSList {
    pub data: *mut c_void,
    pub next: *mut GSList,
}

// ---- enumeration bounds -------------------------------------------------
//
// Pango range-checks none of these. Its setters store whatever they are given
// and its switch statements have no default arm, so an out-of-range value
// falls through silently and surfaces as a layout that is subtly wrong rather
// than as a failure. Every enum crossing this boundary is checked against the
// bounds below first.

/// `PANGO_STYLE_ITALIC`, the highest `PangoStyle`. pango-font.h:74-78.
pub const PANGO_STYLE_MAX: c_int = 2;
/// `PANGO_VARIANT_TITLE_CAPS`, the highest `PangoVariant`. pango-font.h:100-108.
/// Values 2..=6 arrived in 1.50, so they need the version gate as well.
pub const PANGO_VARIANT_MAX: c_int = 6;
/// The highest `PangoVariant` before 1.50: `PANGO_VARIANT_SMALL_CAPS`.
pub const PANGO_VARIANT_MAX_PRE_1_50: c_int = 1;
/// `PANGO_STRETCH_ULTRA_EXPANDED`, the highest `PangoStretch`.
/// pango-font.h:160-170.
pub const PANGO_STRETCH_MAX: c_int = 8;

/// The lowest legal `PangoWeight`. pango-font.h:130-143.
///
/// Weight is **a range, not an ordinal**: the header calls it "a numeric value
/// ranging from 100 to 1000" and the gir adds that intermediate values are
/// possible. Testing membership of the named constants would reject a legal
/// 450 from a variable font's weight axis; a `0..=n` ordinal check would
/// reject every legal value and accept near-zero garbage.
pub const PANGO_WEIGHT_MIN: c_int = 100;
/// The highest legal `PangoWeight`.
pub const PANGO_WEIGHT_MAX: c_int = 1000;

/// The lowest legal `PangoWidth`. pango-font.h:193-203, since 1.58. A range,
/// exactly as [`PANGO_WEIGHT_MIN`] is.
pub const PANGO_WIDTH_MIN: c_int = 500;
/// The highest legal `PangoWidth`.
pub const PANGO_WIDTH_MAX: c_int = 2000;

/// `PANGO_GRAVITY_AUTO`, the highest `PangoGravity`. pango-gravity.h:54-58.
///
/// On `pango_font_description_set_gravity` this value *unsets* the gravity
/// mask rather than setting gravity to AUTO; on
/// `pango_context_set_base_gravity` it means what it says. Same constant, two
/// meanings.
pub const PANGO_GRAVITY_MAX: c_int = 4;
/// `PANGO_GRAVITY_HINT_LINE`, the highest `PangoGravityHint`.
/// pango-gravity.h:82-84.
pub const PANGO_GRAVITY_HINT_MAX: c_int = 2;
/// `PANGO_DIRECTION_NEUTRAL`, the highest `PangoDirection`.
/// pango-direction.h:60-71.
pub const PANGO_DIRECTION_MAX: c_int = 6;
/// `PANGO_ALIGN_RIGHT`, the highest `PangoAlignment`. pango-layout.h:61-65.
pub const PANGO_ALIGN_MAX: c_int = 2;
/// `PANGO_WRAP_WORD_CHAR`, the highest `PangoWrapMode` before 1.56.
/// pango-layout.h:90-95.
pub const PANGO_WRAP_MAX: c_int = 2;
/// `PANGO_WRAP_NONE`, added in 1.56. Setting it on an older Pango stores a
/// value that falls through every case in the line breaker.
pub const PANGO_WRAP_NONE: c_int = 3;
/// `PANGO_ELLIPSIZE_END`, the highest `PangoEllipsizeMode`.
/// pango-layout.h:111-116.
pub const PANGO_ELLIPSIZE_MAX: c_int = 3;
/// `PANGO_TAB_DECIMAL`, the highest `PangoTabAlign`. pango-tabs.h:46-52;
/// 1..=3 arrived in 1.50.
pub const PANGO_TAB_ALIGN_MAX: c_int = 3;
/// `PANGO_TAB_LEFT`, the only `PangoTabAlign` before 1.50.
pub const PANGO_TAB_ALIGN_MAX_PRE_1_50: c_int = 0;

/// Every `PangoFontMask` bit defined as of 1.57. pango-font.h:254-266.
///
/// **`PANGO_FONT_MASK_WIDTH` and `PANGO_FONT_MASK_STRETCH` are the same bit**
/// -- both `1 << 4`; the header itself calls WIDTH "an alias for STRETCH". The
/// image must not treat them as two predicates that can disagree.
pub const PANGO_FONT_MASK_ALL: c_int = 0x3FF;
/// The mask bits defined before 1.56, for a version-gated check.
pub const PANGO_FONT_MASK_ALL_PRE_1_56: c_int = 0xFF;

/// `PANGO_ATTR_INDEX_TO_TEXT_END`, pango-attributes.h: `G_MAXUINT`
/// [measured: 4294967295].
///
/// Not representable as a positive `sqInt` on a 32-bit image, so it crosses
/// the boundary as `-1` and is mapped here.
pub const PANGO_ATTR_INDEX_TO_TEXT_END: c_uint = c_uint::MAX;

/// `G_MAXINT`, which `pango_layout_move_cursor_visually` uses as its "off the
/// end of the text" sentinel for `new_index`. Its counterpart is `-1`.
pub const G_MAXINT: c_int = c_int::MAX;

/// `PANGO_VERSION_ENCODE(major, minor, micro)`, pango-utils.h:121. What
/// `pango_version()` answers.
#[must_use]
pub const fn pango_version_encode(major: c_int, minor: c_int, micro: c_int) -> c_int {
    major * 10000 + minor * 100 + micro
}

/// Pango 1.46, which introduced `pango_layout_get_direction` and the
/// `get_family`/`get_face` lookups.
pub const PANGO_VERSION_1_46: c_int = 14600;
/// Pango 1.50, which introduced serialization, `get_caret_pos`, the
/// `PangoLayoutLine` accessors and the `PangoTabAlign` values past LEFT.
pub const PANGO_VERSION_1_50: c_int = 15000;
/// Pango 1.56, which introduced `PANGO_WRAP_NONE` and `add_font_file`.
pub const PANGO_VERSION_1_56: c_int = 15600;
/// Pango 1.58, which introduced `PangoWidth`.
pub const PANGO_VERSION_1_58: c_int = 15800;

// ---- the two function tables --------------------------------------------

/// Declares Pango's function table, one `Option` per symbol.
macro_rules! pango_api {
    ($( fn $name:ident ( $($arg:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        /// Pango's entry points, resolved once when the module loads.
        pub struct Pango {
            /// Where libpangocairo was found, for `primitiveLibraryPath`.
            pub path: String,
            $(
                #[allow(missing_docs)]
                pub $name: Option<unsafe extern "C" fn($($arg),*) $(-> $ret)?>,
            )*
        }

        impl Pango {
            /// Resolves every entry out of an already-open library.
            ///
            /// # Safety
            ///
            /// `lib` must be libpangocairo, and must outlive every pointer
            /// taken from it -- which is why the caller leaks it.
            unsafe fn resolve(lib: &'static dylib::Library, path: String) -> Self {
                Self {
                    path,
                    // SAFETY: each name is a Pango entry point whose signature
                    // is transcribed from the 1.58.2 headers just below.
                    $( $name: unsafe { dylib::symbol(lib, stringify!($name)) }, )*
                }
            }

            /// How many of the entry points this library actually exports.
            #[must_use]
            pub fn resolved_count(&self) -> usize {
                let mut n = 0;
                $( if self.$name.is_some() { n += 1; } )*
                n
            }

            /// How many entry points this plugin knows about.
            #[must_use]
            pub fn declared_count(&self) -> usize {
                let mut n = 0;
                $( { let _ = &self.$name; n += 1; } )*
                n
            }

            /// Names this library did not export.
            ///
            /// Pango is a *system* library, so this is legitimately non-empty
            /// on an older install and an unconditional emptiness assertion
            /// would fail everywhere but the machine it was written on. Cross
            /// it with [`entries_introduced_after`] to separate "too old" from
            /// "misspelt": a typo leaves the `Option` permanently `None` and
            /// its primitives answering `Unsupported` for the life of the
            /// plugin, which is exactly what an old Pango looks like.
            #[must_use]
            pub fn missing_entry_points(&self) -> Vec<&'static str> {
                let mut missing = Vec::new();
                $( if self.$name.is_none() { missing.push(stringify!($name)); } )*
                missing
            }
        }
    };
}

/// Declares glib's function table, one `Option` per symbol.
///
/// A second macro rather than one parameterised over both, because the two
/// tables are reported separately by the diagnostic primitives and "pango
/// loaded, glib did not" is a real and confusing state that the image has to
/// be able to name.
macro_rules! glib_api {
    ($( fn $name:ident ( $($arg:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        /// glib's and gobject's entry points, resolved once when the module
        /// loads.
        pub struct Glib {
            /// Where libglib was found, for `primitiveGlibLibraryPath`.
            pub path: String,
            /// Where libgobject was found.
            pub gobject_path: String,
            $(
                #[allow(missing_docs)]
                pub $name: Option<unsafe extern "C" fn($($arg),*) $(-> $ret)?>,
            )*
        }

        impl Glib {
            /// Resolves every entry out of the two already-open libraries.
            ///
            /// gobject is tried first and glib second, because the pair splits
            /// unevenly -- `g_object_ref`/`g_object_unref` live in gobject and
            /// everything else in glib -- and gobject links glib anyway, so
            /// the order is a preference rather than a partition.
            ///
            /// # Safety
            ///
            /// `gobject` and `glib` must be those libraries, and must outlive
            /// every pointer taken from them, which is why the caller leaks
            /// them.
            unsafe fn resolve(
                gobject: &'static dylib::Library,
                glib: &'static dylib::Library,
                gobject_path: String,
                path: String,
            ) -> Self {
                Self {
                    path,
                    gobject_path,
                    // SAFETY: each name is a glib entry point whose signature
                    // is transcribed from the 2.88.3 headers just below.
                    $( $name: unsafe { symbol_in_either(gobject, glib, stringify!($name)) }, )*
                }
            }

            /// How many of the entry points these libraries actually export.
            #[must_use]
            pub fn resolved_count(&self) -> usize {
                let mut n = 0;
                $( if self.$name.is_some() { n += 1; } )*
                n
            }

            /// How many entry points this plugin knows about.
            #[must_use]
            pub fn declared_count(&self) -> usize {
                let mut n = 0;
                $( { let _ = &self.$name; n += 1; } )*
                n
            }

            /// Names these libraries did not export.
            #[must_use]
            pub fn missing_entry_points(&self) -> Vec<&'static str> {
                let mut missing = Vec::new();
                $( if self.$name.is_none() { missing.push(stringify!($name)); } )*
                missing
            }
        }
    };
}

pango_api! {
    // ---- the library itself, units, and version ----
    fn pango_version() -> c_int;
    fn pango_version_string() -> *const c_char;
    fn pango_version_check(c_int, c_int, c_int) -> *const c_char;
    // The two official converters, rather than a hard-coded 1024. PANGO_SCALE
    // is a macro with no symbol and the header says it "may be changed in the
    // future", so nothing in this crate multiplies by it.
    fn pango_units_from_double(c_double) -> c_int;
    fn pango_units_to_double(c_int) -> c_double;
    fn pango_extents_to_pixels(*mut PangoRectangle, *mut PangoRectangle);

    // ---- PangoLayout: construction and identity ----
    fn pango_layout_new(*mut PangoContext) -> *mut PangoLayout;
    fn pango_layout_copy(*mut PangoLayout) -> *mut PangoLayout;
    fn pango_layout_get_context(*mut PangoLayout) -> *mut PangoContext;
    fn pango_layout_context_changed(*mut PangoLayout);
    fn pango_layout_get_serial(*mut PangoLayout) -> c_uint;

    // ---- PangoLayout: text and markup ----
    fn pango_layout_set_text(*mut PangoLayout, *const c_char, c_int);
    fn pango_layout_get_text(*mut PangoLayout) -> *const c_char;
    fn pango_layout_get_character_count(*mut PangoLayout) -> c_int;
    fn pango_layout_set_markup(*mut PangoLayout, *const c_char, c_int);
    fn pango_layout_set_markup_with_accel(
        *mut PangoLayout, *const c_char, c_int, gunichar, *mut gunichar
    );
    // All three out params may legitimately be NULL, which is what makes this
    // usable as a validator for `set_markup` -- that one returns void and
    // reports a parse error only as a g_warning.
    fn pango_parse_markup(
        *const c_char, c_int, gunichar,
        *mut *mut PangoAttrList, *mut *mut c_char, *mut gunichar, *mut *mut GError
    ) -> gboolean;

    // ---- PangoLayout: attributes, font description, tabs ----
    // Three getters, three ownership rules, one header block: `get_tabs` is
    // transfer FULL, `get_attributes` and `get_font_description` are transfer
    // NONE. Freeing either of the latter is a double free.
    fn pango_layout_set_attributes(*mut PangoLayout, *mut PangoAttrList);
    fn pango_layout_get_attributes(*mut PangoLayout) -> *mut PangoAttrList;
    fn pango_layout_set_font_description(*mut PangoLayout, *const PangoFontDescription);
    fn pango_layout_get_font_description(*mut PangoLayout) -> *const PangoFontDescription;
    fn pango_layout_set_tabs(*mut PangoLayout, *mut PangoTabArray);
    fn pango_layout_get_tabs(*mut PangoLayout) -> *mut PangoTabArray;

    // ---- PangoLayout: geometry, wrapping, alignment ----
    fn pango_layout_set_width(*mut PangoLayout, c_int);
    fn pango_layout_get_width(*mut PangoLayout) -> c_int;
    fn pango_layout_set_height(*mut PangoLayout, c_int);
    fn pango_layout_get_height(*mut PangoLayout) -> c_int;
    fn pango_layout_set_wrap(*mut PangoLayout, c_int);
    fn pango_layout_get_wrap(*mut PangoLayout) -> c_int;
    fn pango_layout_is_wrapped(*mut PangoLayout) -> gboolean;
    fn pango_layout_set_indent(*mut PangoLayout, c_int);
    fn pango_layout_get_indent(*mut PangoLayout) -> c_int;
    fn pango_layout_set_spacing(*mut PangoLayout, c_int);
    fn pango_layout_get_spacing(*mut PangoLayout) -> c_int;
    // c_float, NOT c_double: pango-layout.h:229-232 says `float`, and these
    // are the only floating-point values in the whole PangoLayout API. A
    // c_double declaration compiles, links and resolves, and then produces
    // garbage -- on AArch64 the caller writes d0 and the callee reads s0.
    // Only a live round-trip test ever catches it.
    fn pango_layout_set_line_spacing(*mut PangoLayout, c_float);
    fn pango_layout_get_line_spacing(*mut PangoLayout) -> c_float;
    fn pango_layout_set_justify(*mut PangoLayout, gboolean);
    fn pango_layout_get_justify(*mut PangoLayout) -> gboolean;
    fn pango_layout_set_justify_last_line(*mut PangoLayout, gboolean);
    fn pango_layout_get_justify_last_line(*mut PangoLayout) -> gboolean;
    fn pango_layout_set_auto_dir(*mut PangoLayout, gboolean);
    fn pango_layout_get_auto_dir(*mut PangoLayout) -> gboolean;
    fn pango_layout_set_alignment(*mut PangoLayout, c_int);
    fn pango_layout_get_alignment(*mut PangoLayout) -> c_int;
    fn pango_layout_set_single_paragraph_mode(*mut PangoLayout, gboolean);
    fn pango_layout_get_single_paragraph_mode(*mut PangoLayout) -> gboolean;
    fn pango_layout_set_ellipsize(*mut PangoLayout, c_int);
    fn pango_layout_get_ellipsize(*mut PangoLayout) -> c_int;
    fn pango_layout_is_ellipsized(*mut PangoLayout) -> gboolean;
    // The `int` is a BYTE offset into the layout's text, like every other
    // index in this block -- "the byte index of the char", says the gir, and
    // measured: on "a<U+05E9>b" it answers RTL for indices 1 and 2, which is
    // the Hebrew letter's two bytes, not its one character. 1.46+.
    fn pango_layout_get_direction(*mut PangoLayout, c_int) -> c_int;

    // ---- PangoLayout: measurement ----
    // `get_pixel_extents` rounds outwards and is therefore NOT
    // PANGO_PIXELS(get_extents); neither may be implemented from the other.
    fn pango_layout_get_extents(*mut PangoLayout, *mut PangoRectangle, *mut PangoRectangle);
    fn pango_layout_get_pixel_extents(*mut PangoLayout, *mut PangoRectangle, *mut PangoRectangle);
    fn pango_layout_get_size(*mut PangoLayout, *mut c_int, *mut c_int);
    fn pango_layout_get_pixel_size(*mut PangoLayout, *mut c_int, *mut c_int);
    fn pango_layout_get_baseline(*mut PangoLayout) -> c_int;
    fn pango_layout_get_line_count(*mut PangoLayout) -> c_int;
    fn pango_layout_get_unknown_glyphs_count(*mut PangoLayout) -> c_int;

    // ---- PangoLayout: hit-testing and the caret ----
    // `xy_to_index` answers a gboolean, not the index: the index comes back
    // through the first `*mut c_int`. FALSE means "the point was outside, so
    // the answer was clamped", not "failed", and both out params are valid
    // either way.
    fn pango_layout_xy_to_index(
        *mut PangoLayout, c_int, c_int, *mut c_int, *mut c_int
    ) -> gboolean;
    fn pango_layout_index_to_pos(*mut PangoLayout, c_int, *mut PangoRectangle);
    fn pango_layout_index_to_line_x(
        *mut PangoLayout, c_int, gboolean, *mut c_int, *mut c_int
    );
    fn pango_layout_get_cursor_pos(
        *mut PangoLayout, c_int, *mut PangoRectangle, *mut PangoRectangle
    );
    fn pango_layout_get_caret_pos(
        *mut PangoLayout, c_int, *mut PangoRectangle, *mut PangoRectangle
    );
    fn pango_layout_move_cursor_visually(
        *mut PangoLayout, gboolean, c_int, c_int, c_int, *mut c_int, *mut c_int
    );

    // ---- PangoLayout: log attributes ----
    // The allocating form's `attrs` must be g_free'd; the readonly form
    // allocates nothing and is preferred. Both answer one more attr than the
    // character count -- there is a position before the first character and
    // one after the last.
    fn pango_layout_get_log_attrs(*mut PangoLayout, *mut *mut PangoLogAttr, *mut c_int);
    fn pango_layout_get_log_attrs_readonly(*mut PangoLayout, *mut c_int) -> *const PangoLogAttr;

    // ---- PangoLayout: lines ----
    // `get_line` marks the line "leaked" inside Pango and disables its glyph
    // cache; `get_line_readonly` has identical transfer semantics without
    // that cost, and no primitive here mutates a line.
    fn pango_layout_get_line(*mut PangoLayout, c_int) -> *mut PangoLayoutLine;
    fn pango_layout_get_line_readonly(*mut PangoLayout, c_int) -> *mut PangoLayoutLine;
    // transfer=none GSLists, declared for the name census. Freeing either is
    // a double free of the layout's own lines.
    fn pango_layout_get_lines(*mut PangoLayout) -> *mut GSList;
    fn pango_layout_get_lines_readonly(*mut PangoLayout) -> *mut GSList;

    // ---- PangoLayout: serialization. Declared, never exposed in v1. ----
    fn pango_layout_serialize(*mut PangoLayout, c_uint) -> *mut GBytes;
    fn pango_layout_deserialize(
        *mut PangoContext, *mut GBytes, c_uint, *mut *mut GError
    ) -> *mut PangoLayout;

    // ---- PangoLayoutLine ----
    fn pango_layout_line_ref(*mut PangoLayoutLine) -> *mut PangoLayoutLine;
    fn pango_layout_line_unref(*mut PangoLayoutLine);
    fn pango_layout_line_get_start_index(*mut PangoLayoutLine) -> c_int;
    fn pango_layout_line_get_length(*mut PangoLayoutLine) -> c_int;
    fn pango_layout_line_is_paragraph_start(*mut PangoLayoutLine) -> gboolean;
    fn pango_layout_line_get_resolved_direction(*mut PangoLayoutLine) -> c_int;
    fn pango_layout_line_x_to_index(
        *mut PangoLayoutLine, c_int, *mut c_int, *mut c_int
    ) -> gboolean;
    fn pango_layout_line_index_to_x(*mut PangoLayoutLine, c_int, gboolean, *mut c_int);
    // `ranges` is a g_malloc'd array of 2 * n_ranges ints: copy it out and
    // g_free it inside the same primitive.
    fn pango_layout_line_get_x_ranges(
        *mut PangoLayoutLine, c_int, c_int, *mut *mut c_int, *mut c_int
    );
    fn pango_layout_line_get_extents(
        *mut PangoLayoutLine, *mut PangoRectangle, *mut PangoRectangle
    );
    fn pango_layout_line_get_pixel_extents(
        *mut PangoLayoutLine, *mut PangoRectangle, *mut PangoRectangle
    );
    fn pango_layout_line_get_height(*mut PangoLayoutLine, *mut c_int);

    // ---- PangoFontDescription ----
    fn pango_font_description_new() -> *mut PangoFontDescription;
    fn pango_font_description_copy(
        *const PangoFontDescription
    ) -> *mut PangoFontDescription;
    fn pango_font_description_free(*mut PangoFontDescription);
    fn pango_font_description_hash(*const PangoFontDescription) -> c_uint;
    fn pango_font_description_equal(
        *const PangoFontDescription, *const PangoFontDescription
    ) -> gboolean;
    fn pango_font_description_merge(
        *mut PangoFontDescription, *const PangoFontDescription, gboolean
    );
    fn pango_font_description_from_string(*const c_char) -> *mut PangoFontDescription;
    fn pango_font_description_to_string(*const PangoFontDescription) -> *mut c_char;
    fn pango_font_description_to_filename(*const PangoFontDescription) -> *mut c_char;
    fn pango_font_description_set_family(*mut PangoFontDescription, *const c_char);
    fn pango_font_description_get_family(*const PangoFontDescription) -> *const c_char;
    fn pango_font_description_set_style(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_style(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_variant(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_variant(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_weight(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_weight(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_stretch(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_stretch(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_width(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_width(*const PangoFontDescription) -> c_int;
    // `set_size` is points * PANGO_SCALE and takes an int; `set_absolute_size`
    // is device units * PANGO_SCALE and takes a double. The double buys
    // sub-unit precision, not a different scale: passing 10.0 gives a
    // ~0.0098-unit font, not a 10-pixel one.
    fn pango_font_description_set_size(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_size(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_absolute_size(*mut PangoFontDescription, c_double);
    fn pango_font_description_get_size_is_absolute(
        *const PangoFontDescription
    ) -> gboolean;
    fn pango_font_description_set_gravity(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_gravity(*const PangoFontDescription) -> c_int;
    fn pango_font_description_set_variations(*mut PangoFontDescription, *const c_char);
    fn pango_font_description_get_variations(
        *const PangoFontDescription
    ) -> *const c_char;
    fn pango_font_description_set_features(*mut PangoFontDescription, *const c_char);
    fn pango_font_description_get_features(*const PangoFontDescription) -> *const c_char;
    fn pango_font_description_set_color(*mut PangoFontDescription, c_int);
    fn pango_font_description_get_color(*const PangoFontDescription) -> c_int;
    fn pango_font_description_get_set_fields(*const PangoFontDescription) -> c_int;
    fn pango_font_description_unset_fields(*mut PangoFontDescription, c_int);

    // ---- PangoFontDescription: the `_static` family ----
    //
    // Declared for an honest symbol census and reachable by no primitive.
    // Every one of these stores the caller's `char *` BY POINTER, on the
    // promise that the string outlives the description. A primitive's string
    // argument is a CString dropped at the end of the primitive, so each of
    // these is a use-after-free waiting to happen. The copying variants above
    // are the ones with primitives.
    fn pango_font_description_copy_static(
        *const PangoFontDescription
    ) -> *mut PangoFontDescription;
    fn pango_font_description_merge_static(
        *mut PangoFontDescription, *const PangoFontDescription, gboolean
    );
    fn pango_font_description_set_family_static(*mut PangoFontDescription, *const c_char);
    fn pango_font_description_set_variations_static(
        *mut PangoFontDescription, *const c_char
    );
    fn pango_font_description_set_features_static(
        *mut PangoFontDescription, *const c_char
    );

    // ---- PangoFontMap ----
    // `list_families` is transfer=CONTAINER: g_free the array, touch not one
    // element. A listed family sits at refcount 1, held by the font map
    // alone, so unref'ing one frees an object the font map still points at.
    fn pango_font_map_list_families(
        *mut PangoFontMap, *mut *mut *mut PangoFontFamily, *mut c_int
    );
    fn pango_font_map_get_family(*mut PangoFontMap, *const c_char) -> *mut PangoFontFamily;
    fn pango_font_map_create_context(*mut PangoFontMap) -> *mut PangoContext;
    fn pango_font_map_get_serial(*mut PangoFontMap) -> c_uint;
    fn pango_font_map_add_font_file(
        *mut PangoFontMap, *const c_char, *mut *mut GError
    ) -> gboolean;
    // Declared, not exposed: a loaded font is transfer=full out of a cache
    // that holds dozens of other references (measured: rc 42), so the plugin
    // would own exactly one and have to release exactly one.
    fn pango_font_map_load_font(
        *mut PangoFontMap, *mut PangoContext, *const PangoFontDescription
    ) -> *mut PangoFont;
    fn pango_font_map_load_fontset(
        *mut PangoFontMap, *mut PangoContext, *const PangoFontDescription, *mut PangoLanguage
    ) -> *mut PangoFontset;
    fn pango_font_map_reload_font(
        *mut PangoFontMap, *mut PangoFont, c_double, *mut PangoContext, *const c_char
    ) -> *mut PangoFont;

    // ---- PangoFontFamily and PangoFontFace: always borrowed ----
    fn pango_font_family_get_name(*mut PangoFontFamily) -> *const c_char;
    fn pango_font_family_is_monospace(*mut PangoFontFamily) -> gboolean;
    fn pango_font_family_is_variable(*mut PangoFontFamily) -> gboolean;
    fn pango_font_family_list_faces(
        *mut PangoFontFamily, *mut *mut *mut PangoFontFace, *mut c_int
    );
    fn pango_font_family_get_face(
        *mut PangoFontFamily, *const c_char
    ) -> *mut PangoFontFace;
    // NOT `pango_font_face_get_name`, which does not exist -- `nm -gU` on
    // libpango-1.0.0.dylib shows it absent. A wrong name here is
    // indistinguishable from an old Pango.
    fn pango_font_face_get_face_name(*mut PangoFontFace) -> *const c_char;
    fn pango_font_face_describe(*mut PangoFontFace) -> *mut PangoFontDescription;
    // `sizes` may legitimately come back (NULL, 0) for a scalable face, and
    // `slice::from_raw_parts(NULL, 0)` is undefined behaviour.
    fn pango_font_face_list_sizes(*mut PangoFontFace, *mut *mut c_int, *mut c_int);
    fn pango_font_face_is_synthesized(*mut PangoFontFace) -> gboolean;
    fn pango_font_face_get_family(*mut PangoFontFace) -> *mut PangoFontFamily;

    // ---- PangoFont. Declared, not exposed: no font handles in v1. ----
    fn pango_font_describe(*mut PangoFont) -> *mut PangoFontDescription;
    fn pango_font_describe_with_absolute_size(*mut PangoFont) -> *mut PangoFontDescription;
    fn pango_font_get_metrics(*mut PangoFont, *mut PangoLanguage) -> *mut PangoFontMetrics;
    fn pango_font_get_font_map(*mut PangoFont) -> *mut PangoFontMap;
    fn pango_font_get_face(*mut PangoFont) -> *mut PangoFontFace;

    // ---- PangoContext ----
    fn pango_context_new() -> *mut PangoContext;
    fn pango_context_changed(*mut PangoContext);
    fn pango_context_set_font_map(*mut PangoContext, *mut PangoFontMap);
    fn pango_context_get_font_map(*mut PangoContext) -> *mut PangoFontMap;
    fn pango_context_get_serial(*mut PangoContext) -> c_uint;
    fn pango_context_list_families(
        *mut PangoContext, *mut *mut *mut PangoFontFamily, *mut c_int
    );
    fn pango_context_load_font(
        *mut PangoContext, *const PangoFontDescription
    ) -> *mut PangoFont;
    fn pango_context_load_fontset(
        *mut PangoContext, *const PangoFontDescription, *mut PangoLanguage
    ) -> *mut PangoFontset;
    // transfer=full: read the nine accessors and unref in the same primitive.
    // Both `desc` and `language` are nullable.
    fn pango_context_get_metrics(
        *mut PangoContext, *const PangoFontDescription, *mut PangoLanguage
    ) -> *mut PangoFontMetrics;
    fn pango_context_set_font_description(*mut PangoContext, *const PangoFontDescription);
    // transfer=NONE, nullable: copy it before registering, or a later destroy
    // primitive frees the context's own description.
    fn pango_context_get_font_description(*mut PangoContext) -> *mut PangoFontDescription;
    fn pango_context_get_language(*mut PangoContext) -> *mut PangoLanguage;
    fn pango_context_set_language(*mut PangoContext, *mut PangoLanguage);
    fn pango_context_set_base_dir(*mut PangoContext, c_int);
    fn pango_context_get_base_dir(*mut PangoContext) -> c_int;
    fn pango_context_set_base_gravity(*mut PangoContext, c_int);
    fn pango_context_get_base_gravity(*mut PangoContext) -> c_int;
    fn pango_context_get_gravity(*mut PangoContext) -> c_int;
    fn pango_context_set_gravity_hint(*mut PangoContext, c_int);
    fn pango_context_get_gravity_hint(*mut PangoContext) -> c_int;
    // Pango copies the matrix; NULL unsets it. The getter's answer is
    // borrowed and nullable, and "no matrix" is distinguishable from "the
    // identity matrix" only by that NULL.
    fn pango_context_set_matrix(*mut PangoContext, *const PangoMatrix);
    fn pango_context_get_matrix(*mut PangoContext) -> *const PangoMatrix;
    fn pango_context_set_round_glyph_positions(*mut PangoContext, gboolean);
    fn pango_context_get_round_glyph_positions(*mut PangoContext) -> gboolean;

    // ---- PangoFontMetrics: nine ints, all in Pango units ----
    fn pango_font_metrics_ref(*mut PangoFontMetrics) -> *mut PangoFontMetrics;
    fn pango_font_metrics_unref(*mut PangoFontMetrics);
    fn pango_font_metrics_get_ascent(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_descent(*mut PangoFontMetrics) -> c_int;
    // Documented to answer 0 when the line height is unavailable. Do not
    // divide by it.
    fn pango_font_metrics_get_height(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_approximate_char_width(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_approximate_digit_width(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_underline_position(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_underline_thickness(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_strikethrough_position(*mut PangoFontMetrics) -> c_int;
    fn pango_font_metrics_get_strikethrough_thickness(*mut PangoFontMetrics) -> c_int;

    // ---- PangoLanguage: static, interned, never freed ----
    fn pango_language_get_default() -> *mut PangoLanguage;
    // transfer=none PangoLanguage**, NULL-terminated. NOT a `char **`: calling
    // g_strfreev on it would free live interned strings.
    fn pango_language_get_preferred() -> *mut *mut PangoLanguage;
    fn pango_language_from_string(*const c_char) -> *mut PangoLanguage;
    // pango-language.h:53 `#define`s this away to `((const char *)language)`,
    // so a C caller never reaches the function -- but the symbol exists and
    // is exported (`nm`: `T _pango_language_to_string`). Bound rather than
    // reimplemented as a Rust cast, which would hard-code an internal
    // representation glib is not obliged to keep.
    fn pango_language_to_string(*mut PangoLanguage) -> *const c_char;
    fn pango_language_get_sample_string(*mut PangoLanguage) -> *const c_char;
    fn pango_language_matches(*mut PangoLanguage, *const c_char) -> gboolean;

    // ---- PangoAttrList: refcounted ----
    fn pango_attr_list_new() -> *mut PangoAttrList;
    fn pango_attr_list_ref(*mut PangoAttrList) -> *mut PangoAttrList;
    fn pango_attr_list_unref(*mut PangoAttrList);
    fn pango_attr_list_copy(*mut PangoAttrList) -> *mut PangoAttrList;
    // These three take the attribute transfer=FULL: after the call the plugin
    // must not destroy it, and must retire any handle on it in the same
    // primitive. That is the whole reason v1 exposes no attribute handles.
    fn pango_attr_list_insert(*mut PangoAttrList, *mut PangoAttribute);
    fn pango_attr_list_insert_before(*mut PangoAttrList, *mut PangoAttribute);
    fn pango_attr_list_change(*mut PangoAttrList, *mut PangoAttribute);
    // `len == 0` does not mean "splice nothing": the header says the other
    // list's attributes are then not limited at all and are simply overlayed.
    fn pango_attr_list_splice(*mut PangoAttrList, *mut PangoAttrList, c_int, c_int);
    fn pango_attr_list_update(*mut PangoAttrList, c_int, c_int, c_int);
    // transfer=full GSList: destroy each attribute, then free the list.
    fn pango_attr_list_get_attributes(*mut PangoAttrList) -> *mut GSList;
    fn pango_attr_list_equal(*mut PangoAttrList, *mut PangoAttrList) -> gboolean;
    fn pango_attr_list_to_string(*mut PangoAttrList) -> *mut c_char;
    fn pango_attr_list_from_string(*const c_char) -> *mut PangoAttrList;
    // Declared, not exposed: the gir says the list must not be modified until
    // the iterator is freed, which an image-visible handle cannot promise.
    fn pango_attr_list_get_iterator(*mut PangoAttrList) -> *mut PangoAttrIterator;
    fn pango_attr_iterator_destroy(*mut PangoAttrIterator);

    // ---- PangoAttribute and its constructors ----
    //
    // Declared from day one and exposed by nothing in v1, so that v2 is
    // primitives-only work with no new FFI risk. Note the argument types:
    // colour channels are guint16, `scale` is a double, and the two flags are
    // gboolean and so four bytes.
    fn pango_attribute_copy(*const PangoAttribute) -> *mut PangoAttribute;
    fn pango_attribute_destroy(*mut PangoAttribute);
    fn pango_attr_type_get_name(c_int) -> *const c_char;
    fn pango_attr_family_new(*const c_char) -> *mut PangoAttribute;
    fn pango_attr_foreground_new(u16, u16, u16) -> *mut PangoAttribute;
    fn pango_attr_background_new(u16, u16, u16) -> *mut PangoAttribute;
    fn pango_attr_size_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_size_new_absolute(c_int) -> *mut PangoAttribute;
    fn pango_attr_style_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_weight_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_underline_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_underline_color_new(u16, u16, u16) -> *mut PangoAttribute;
    fn pango_attr_strikethrough_new(gboolean) -> *mut PangoAttribute;
    fn pango_attr_strikethrough_color_new(u16, u16, u16) -> *mut PangoAttribute;
    fn pango_attr_rise_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_scale_new(c_double) -> *mut PangoAttribute;
    fn pango_attr_fallback_new(gboolean) -> *mut PangoAttribute;
    fn pango_attr_letter_spacing_new(c_int) -> *mut PangoAttribute;
    fn pango_attr_font_features_new(*const c_char) -> *mut PangoAttribute;
    fn pango_attr_foreground_alpha_new(u16) -> *mut PangoAttribute;
    fn pango_attr_background_alpha_new(u16) -> *mut PangoAttribute;

    // ---- PangoTabArray: NOT refcounted -- `free`, and there is no ref ----
    fn pango_tab_array_new(c_int, gboolean) -> *mut PangoTabArray;
    fn pango_tab_array_copy(*mut PangoTabArray) -> *mut PangoTabArray;
    fn pango_tab_array_free(*mut PangoTabArray);
    fn pango_tab_array_get_size(*mut PangoTabArray) -> c_int;
    fn pango_tab_array_resize(*mut PangoTabArray, c_int);
    // `set_tab` does not bounds-check: it silently grows the array, so an
    // image typo becomes a huge allocation. `get_tab` does the opposite --
    // it g_return_if_fail's and leaves both out params untouched, which under
    // G_DEBUG=fatal-criticals aborts the VM. Both are checked before the call.
    fn pango_tab_array_set_tab(*mut PangoTabArray, c_int, c_int, c_int);
    fn pango_tab_array_get_tab(*mut PangoTabArray, c_int, *mut c_int, *mut c_int);
    // Both out arrays are transfer=full: g_free each, independently, when
    // non-NULL. The `PangoTabAlign *` is declared as `*mut c_int` on purpose
    // -- a Rust `#[repr(C)] enum` would carry a validity invariant over
    // memory Pango wrote and we did not.
    fn pango_tab_array_get_tabs(*mut PangoTabArray, *mut *mut c_int, *mut *mut c_int);
    fn pango_tab_array_get_positions_in_pixels(*mut PangoTabArray) -> gboolean;
    fn pango_tab_array_set_positions_in_pixels(*mut PangoTabArray, gboolean);
    fn pango_tab_array_to_string(*mut PangoTabArray) -> *mut c_char;
    fn pango_tab_array_from_string(*const c_char) -> *mut PangoTabArray;

    // ---- PangoColor: always stack-allocated ----
    fn pango_color_parse(*mut PangoColor, *const c_char) -> gboolean;
    fn pango_color_parse_with_alpha(
        *mut PangoColor, *mut u16, *const c_char
    ) -> gboolean;
    fn pango_color_to_string(*const PangoColor) -> *mut c_char;

    // ---- pangocairo ----
    fn pango_cairo_create_context(*mut cairo_t) -> *mut PangoContext;
    fn pango_cairo_create_layout(*mut cairo_t) -> *mut PangoLayout;
    fn pango_cairo_update_layout(*mut cairo_t, *mut PangoLayout);
    fn pango_cairo_update_context(*mut cairo_t, *mut PangoContext);
    fn pango_cairo_show_layout(*mut cairo_t, *mut PangoLayout);
    fn pango_cairo_show_layout_line(*mut cairo_t, *mut PangoLayoutLine);
    fn pango_cairo_layout_path(*mut cairo_t, *mut PangoLayout);
    fn pango_cairo_layout_line_path(*mut cairo_t, *mut PangoLayoutLine);
    fn pango_cairo_show_error_underline(
        *mut cairo_t, c_double, c_double, c_double, c_double
    );
    fn pango_cairo_error_underline_path(
        *mut cairo_t, c_double, c_double, c_double, c_double
    );
    // transfer=NONE, and its neighbour `_new` right below is transfer=full.
    // Two adjacent functions, opposite rules; the default map sits at
    // refcount 1 for the whole process, so one stray unref destroys text
    // rendering for the entire image, arbitrarily far from the bug.
    fn pango_cairo_font_map_get_default() -> *mut PangoFontMap;
    fn pango_cairo_font_map_new() -> *mut PangoFontMap;
    fn pango_cairo_font_map_new_for_font_type(c_int) -> *mut PangoFontMap;
    // These take PangoCairoFontMap, which is the same address seen through a
    // different interface; the call sites write the `.cast()` out.
    fn pango_cairo_font_map_get_font_type(*mut PangoCairoFontMap) -> c_int;
    fn pango_cairo_font_map_get_resolution(*mut PangoCairoFontMap) -> c_double;
    fn pango_cairo_font_map_set_resolution(*mut PangoCairoFontMap, c_double);
    // Deprecated since 1.22 and behind `#ifndef PANGO_DISABLE_DEPRECATED`, so
    // it is absent from some builds and not others. Declared for the census;
    // `pango_font_map_create_context` is what the primitive calls.
    fn pango_cairo_font_map_create_context(*mut PangoCairoFontMap) -> *mut PangoContext;
    fn pango_cairo_context_set_resolution(*mut PangoContext, c_double);
    // Answers a negative number when the resolution has not been set.
    fn pango_cairo_context_get_resolution(*mut PangoContext) -> c_double;
    // The context copies the options; the caller keeps ownership of what it
    // passed, and NULL unsets. The getter's answer is borrowed and nullable.
    fn pango_cairo_context_set_font_options(*mut PangoContext, *const cairo_font_options_t);
    fn pango_cairo_context_get_font_options(
        *mut PangoContext
    ) -> *const cairo_font_options_t;
    fn pango_cairo_font_get_scaled_font(*mut PangoCairoFont) -> *mut cairo_scaled_font_t;

    // ---- Cairo, as libpangocairo resolves it ----
    //
    // Not for calling: CairoPlugin owns the drawing. `cairo_create`'s ADDRESS
    // is the identity token the bridge handshake compares. `dlsym` on the
    // pangocairo handle searches that image and its dependencies, so this is
    // the Cairo pangocairo will really call -- and if it differs from the one
    // CairoPlugin resolved, there are two copies of Cairo mapped in the
    // process and no `cairo_t *` may cross between them. `cairo_version` is
    // here so the diagnostic can say *how* the two differ.
    fn cairo_create(*mut c_void) -> *mut c_void;
    fn cairo_destroy(*mut c_void);
    fn cairo_status(*mut c_void) -> c_int;
    fn cairo_version() -> c_int;
}

glib_api! {
    // gobject.h:513, 515. Both are macros in the 2.88 headers -- a
    // type-preserving cast around the real symbol -- and `dlsym` returns the
    // function regardless, which is the whole reason this crate never
    // compiles against a header.
    fn g_object_ref(*mut c_void) -> *mut c_void;
    fn g_object_unref(*mut c_void);
    // gmem.h:74, also a macro (it expands to `g_free_sized` when the compiler
    // knows the size). Every buffer Pango or glib hands back is released with
    // this, never `libc::free` and never Rust's allocator: a cross-heap free
    // is a hard crash on Windows.
    fn g_free(*mut c_void);
    // gerror.h:208. The GError's `message` dies with it, so copy first.
    fn g_error_free(*mut GError);
    // gslist.h:52, 57. `pango_attr_list_get_attributes` is transfer=full and
    // needs one of these; `pango_layout_get_lines` is transfer=none and needs
    // neither.
    fn g_slist_free(*mut GSList);
    fn g_slist_free_full(*mut GSList, Option<unsafe extern "C" fn(*mut c_void)>);
    // gbytes.h:60, 70. For `pango_layout_serialize` if it is ever bound;
    // declared, not exposed.
    fn g_bytes_get_data(*mut GBytes, *mut usize) -> *const c_void;
    fn g_bytes_unref(*mut GBytes);
}

/// The Pango version each gated entry point first appeared in, encoded as
/// `pango_version()` reports it.
///
/// Pango is a system library, so [`Pango::missing_entry_points`] is
/// legitimately non-empty on an older install: `add_font_file` is 1.56,
/// `set_width` is 1.58, `serialize` and `tab_array_to_string` are 1.50. This
/// table is what lets the live test say "absent because old" rather than
/// "absent because misspelt", which is the only distinction that matters and
/// the only one the build cannot make for itself.
///
/// Entries introduced at or before 1.16 are omitted: no Pango that old will
/// meet this plugin, and listing them would only make the table harder to
/// audit against the headers.
const ENTRY_MIN_VERSION: &[(&str, c_int)] = &[
    ("pango_cairo_font_get_scaled_font", 11800),
    ("pango_cairo_font_map_get_font_type", 11800),
    ("pango_cairo_font_map_new_for_font_type", 11800),
    ("pango_font_face_is_synthesized", 11800),
    ("pango_layout_get_height", 12000),
    ("pango_layout_set_height", 12000),
    ("pango_attr_type_get_name", 12200),
    ("pango_cairo_create_context", 12200),
    ("pango_cairo_font_map_create_context", 12200),
    ("pango_font_map_create_context", 12200),
    ("pango_layout_get_baseline", 12200),
    ("pango_layout_get_character_count", 13000),
    ("pango_layout_get_log_attrs_readonly", 13000),
    ("pango_context_changed", 13200),
    ("pango_context_get_serial", 13200),
    ("pango_font_map_get_serial", 13200),
    ("pango_layout_get_serial", 13200),
    ("pango_attr_background_alpha_new", 13800),
    ("pango_attr_font_features_new", 13800),
    ("pango_attr_foreground_alpha_new", 13800),
    ("pango_font_description_get_features", 14200),
    ("pango_font_description_get_variations", 14200),
    ("pango_font_description_set_variations", 14200),
    ("pango_font_description_set_variations_static", 14200),
    ("pango_attr_list_get_attributes", 14400),
    ("pango_attr_list_update", 14400),
    ("pango_context_get_round_glyph_positions", 14400),
    ("pango_context_set_round_glyph_positions", 14400),
    ("pango_font_family_is_variable", 14400),
    ("pango_font_metrics_get_height", 14400),
    ("pango_layout_get_line_spacing", 14400),
    ("pango_layout_line_get_height", 14400),
    ("pango_layout_set_line_spacing", 14400),
    ("pango_attr_list_equal", 14600),
    ("pango_color_parse_with_alpha", 14600),
    ("pango_font_face_get_family", 14600),
    ("pango_font_family_get_face", 14600),
    ("pango_font_get_face", 14600),
    ("pango_font_map_get_family", 14600),
    ("pango_layout_get_direction", 14600),
    ("pango_language_get_preferred", 14800),
    ("pango_attr_list_from_string", 15000),
    ("pango_attr_list_to_string", 15000),
    ("pango_layout_deserialize", 15000),
    ("pango_layout_get_caret_pos", 15000),
    ("pango_layout_get_justify_last_line", 15000),
    ("pango_layout_line_get_length", 15000),
    ("pango_layout_line_get_resolved_direction", 15000),
    ("pango_layout_line_get_start_index", 15000),
    ("pango_layout_line_is_paragraph_start", 15000),
    ("pango_layout_serialize", 15000),
    ("pango_layout_set_justify_last_line", 15000),
    ("pango_tab_array_from_string", 15000),
    ("pango_tab_array_set_positions_in_pixels", 15000),
    ("pango_tab_array_to_string", 15000),
    ("pango_font_map_reload_font", 15200),
    ("pango_font_description_set_features", 15600),
    ("pango_font_description_set_features_static", 15600),
    ("pango_font_map_add_font_file", 15600),
    ("pango_font_description_get_color", 15700),
    ("pango_font_description_set_color", 15700),
    ("pango_font_description_get_width", 15800),
    ("pango_font_description_set_width", 15800),
];

/// The declared entry points a Pango of `version` is too old to export.
///
/// The expected half of the live name check: anything in
/// [`Pango::missing_entry_points`] that is *not* in this list is a misspelt
/// name, which is the one failure the rest of the build cannot see.
#[must_use]
pub fn entries_introduced_after(version: c_int) -> Vec<&'static str> {
    ENTRY_MIN_VERSION
        .iter()
        .filter(|(_, since)| *since > version)
        .map(|(name, _)| *name)
        .collect()
}

// ---- loading ------------------------------------------------------------

static PANGO: OnceLock<Option<Pango>> = OnceLock::new();
static GLIB: OnceLock<Option<Glib>> = OnceLock::new();
static PANGO_ONLY_PATH: OnceLock<Option<String>> = OnceLock::new();

/// Absolute directories to try after the loader's own search.
///
/// The macOS fix, which costs Linux nothing. Measured on this machine with no
/// `DYLD_LIBRARY_PATH` set: `dlopen("libpangocairo-1.0.0.dylib")` **fails**,
/// and so does `dlopen("libcairo.2.dylib")` -- dyld tries only the cwd, the
/// Cryptexes prefix and `/usr/lib`, and Homebrew's `/opt/homebrew/lib` is not
/// on that fallback path. `dylib::open_first` tries the executable's
/// directory, its `Plugins` and `lib` subdirectories, `../lib`, and then the
/// bare name -- and nothing in `cmake/` downloads Pango, so no bundle
/// candidate can ever hit. Without these prefixes the plugin declines on
/// every stock Mac including the developer's, and the natural conclusion --
/// "Pango must not be installed" -- is wrong and wastes a day.
///
/// On Linux the bare name resolves through ldconfig and these are never
/// reached; they are appended rather than prepended so a bundled copy still
/// wins, which is the precedence `dylib::open_first` exists to express.
const UNIX_LIBRARY_PREFIXES: &[&str] = &[
    "/opt/homebrew/lib", // Apple Silicon Homebrew -- the hit on this machine
    "/usr/local/lib",    // Intel Homebrew, and /usr/local installs generally
    "/opt/local/lib",    // MacPorts
    "/usr/lib64",        // Fedora, SUSE and other multilib layouts
    "/usr/lib",          // Debian-family, and the macOS system prefix
];

/// The Homebrew formula a stem belongs to, for the keg-only fallback.
///
/// Homebrew symlinks all four libraries into `<prefix>/lib`, so the per-formula
/// `<prefix>/opt/<formula>/lib` is only reached when that symlink farm is
/// missing or the formula is keg-only. Cheap to try, and it is where the files
/// actually live.
fn homebrew_formula(stem: &str) -> Option<&'static str> {
    match stem {
        "pangocairo-1.0" | "pango-1.0" => Some("pango"),
        "glib-2.0" | "gobject-2.0" => Some("glib"),
        _ => None,
    }
}

/// The environment variable that names an explicit library file for `stem`.
///
/// A packager who installs Pango somewhere none of the prefixes below covers
/// can point the plugin at it without a rebuild. Checked first, so it also
/// overrides a bundled copy -- which is what an override is for.
fn override_variable(stem: &str) -> &'static str {
    match stem {
        "pangocairo-1.0" => "PHARO_PANGOCAIRO_LIBRARY",
        "gobject-2.0" => "PHARO_GOBJECT_LIBRARY",
        "glib-2.0" => "PHARO_GLIB_LIBRARY",
        // "pango-1.0", and any stem a later caller invents: this plugin opens
        // only the four above, and naming the pango variable is the honest
        // answer for a name it has not been taught.
        _ => "PHARO_PANGO_LIBRARY",
    }
}

/// File names, and absolute paths, the system might have `stem` under.
///
/// `dylib::library_names` produces the right leaf shapes on all three
/// platforms with no SDK change -- the `-1.0`/`-2.0` is part of the stem and
/// the ABI version is the trailing `0` -- and those come first, so the
/// bundled-then-system order every other plugin depends on is preserved. The
/// absolute candidates are appended after them, and only on Unix, where they
/// mean something.
///
/// `Path::join` with an absolute argument discards the prefix (verified), so
/// `dylib::open_first` tries each absolute entry five identical times.
/// Harmless, and much cheaper than a second opening function that two other
/// plugins would then have to be re-audited against.
fn library_names(stem: &str) -> Vec<String> {
    let leaves = dylib::library_names(stem, "0");
    let mut names = Vec::with_capacity(leaves.len() * (UNIX_LIBRARY_PREFIXES.len() + 2) + 1);

    if let Ok(path) = std::env::var(override_variable(stem)) {
        if !path.is_empty() {
            names.push(path);
        }
    }
    names.extend(leaves.iter().cloned());

    if !cfg!(target_os = "windows") {
        for prefix in UNIX_LIBRARY_PREFIXES {
            for leaf in &leaves {
                names.push(format!("{prefix}/{leaf}"));
            }
        }
        if let Some(formula) = homebrew_formula(stem) {
            for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
                for leaf in &leaves {
                    names.push(format!("{prefix}/{formula}/lib/{leaf}"));
                }
            }
        }
    }
    names
}

/// Opens the first library `stem` names, leaking it so its symbols stay valid.
///
/// # Safety
///
/// Loading a shared library runs its initialisers. The caller is asserting
/// that `stem` denotes the library it means and that the signatures declared
/// above are that library's.
unsafe fn open(stem: &str) -> Option<(&'static dylib::Library, String)> {
    let names = library_names(stem);
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    // SAFETY: delegated to this function's own contract.
    let (lib, path) = unsafe { dylib::open_first(&refs) }?;
    Some((dylib::leak(lib), path))
}

/// Resolves `name` from `first`, falling back to `second`.
///
/// # Safety
///
/// `T` must be the symbol's actual type; nothing checks this.
unsafe fn symbol_in_either<T: Copy>(
    first: &dylib::Library,
    second: &dylib::Library,
    name: &str,
) -> Option<T> {
    // SAFETY: delegated to this function's own contract.
    unsafe { dylib::symbol(first, name) }.or_else(||
        // SAFETY: same.
        unsafe { dylib::symbol(second, name) })
}

/// Loads Pango and glib, once. Answers whether both are now available.
///
/// Called from the module's init hook, and answering `false` makes the VM
/// reject the module. That is not an error path here: Pango is a *system*
/// library, so a machine without it is the ordinary case, and a clean decline
/// is what lets the image tell "no Pango" from "primitive not implemented".
pub fn load() -> bool {
    let pango = PANGO.get_or_init(|| {
        // SAFETY: this name denotes libpangocairo, and every signature the
        // table declares is transcribed from the Pango 1.58.2 headers.
        let (lib, path) = unsafe { open("pangocairo-1.0") }?;
        // SAFETY: `lib` is now 'static, so the function pointers taken out of
        // it stay valid for the life of the process.
        let pango = unsafe { Pango::resolve(lib, path) };
        // A library that exports neither of these is not Pango, whatever its
        // file name says. Better to decline than to load and fail every
        // primitive.
        if pango.pango_layout_new.is_none() || pango.pango_font_description_new.is_none() {
            return None;
        }
        Some(pango)
    });

    let glib = GLIB.get_or_init(|| {
        // Opened separately rather than resolved from the pangocairo handle,
        // even though macOS obliges. `libloading` opens RTLD_LAZY|RTLD_LOCAL,
        // so nothing is published globally, and on Windows `GetProcAddress`
        // on the pango DLL will not find `g_free` however well dyld obliges
        // here.
        // SAFETY: these names denote libgobject and libglib, and the eight
        // signatures are transcribed from the glib 2.88.3 headers.
        let (gobject, gobject_path) = unsafe { open("gobject-2.0") }?;
        // SAFETY: as above.
        let (glib, path) = unsafe { open("glib-2.0") }?;
        // SAFETY: both are now 'static.
        let g = unsafe { Glib::resolve(gobject, glib, gobject_path, path) };
        // Without `g_object_unref` every PangoLayout the image makes leaks
        // for the life of the process; without `g_free`, every string and
        // every container array does.
        if g.g_object_unref.is_none() || g.g_free.is_none() {
            return None;
        }
        Some(g)
    });

    // Opened last and resolved from by nothing: it exists so a diagnostic can
    // name the file, and because its absence beside a present libpangocairo
    // is worth being able to see.
    PANGO_ONLY_PATH.get_or_init(|| {
        // SAFETY: no symbol is ever taken from this handle.
        unsafe { open("pango-1.0") }.map(|(_, path)| path)
    });

    pango.is_some() && glib.is_some()
}

/// The loaded Pango, or `Unsupported` if it never loaded.
pub fn pango() -> PrimResult<&'static Pango> {
    PANGO.get().and_then(Option::as_ref).ok_or(PrimErr::Unsupported)
}

/// The loaded glib, or `Unsupported` if it never loaded.
pub fn glib() -> PrimResult<&'static Glib> {
    GLIB.get().and_then(Option::as_ref).ok_or(PrimErr::Unsupported)
}

/// Where libpango itself was found, when it was. Diagnostic only -- no entry
/// point is resolved from that handle.
#[must_use]
pub fn pango_only_path() -> Option<&'static str> {
    PANGO_ONLY_PATH.get()?.as_deref()
}

/// Calls a Pango entry point, failing cleanly when the library lacks it.
///
/// The same shape as the SDK's proxy `call!` and cairo-plugin's `cc!`, and for
/// the same reason: an absent symbol is a deployment fact, not bad input from
/// the image, so it is `Unsupported` rather than a failure the fallback code
/// would misread.
macro_rules! pg {
    ($pango:expr, $f:ident ( $($arg:expr),* $(,)? )) => {{
        let f = $pango.$f.ok_or(::pharo_vm_plugin::PrimErr::Unsupported)?;
        // SAFETY: the pointer came out of the libpangocairo we loaded, and the
        // signature is the one declared in `pango_api!` from the 1.58.2
        // headers.
        unsafe { f($($arg),*) }
    }};
}
pub(crate) use pg;

/// Calls a glib entry point, failing cleanly when the library lacks it.
macro_rules! gl {
    ($glib:expr, $f:ident ( $($arg:expr),* $(,)? )) => {{
        let f = $glib.$f.ok_or(::pharo_vm_plugin::PrimErr::Unsupported)?;
        // SAFETY: the pointer came out of the libgobject or libglib we loaded,
        // and the signature is the one declared in `glib_api!` from the 2.88.3
        // headers.
        unsafe { f($($arg),*) }
    }};
}
pub(crate) use gl;

// ---- unit conversion ----------------------------------------------------

/// `PANGO_PIXELS` from pango-types.h:97, including its asymmetry around zero.
///
/// Measured: `+512` rounds to 1 but `-512` rounds to 0, because `>>` on a
/// negative value floors rather than truncating towards zero. Pango documents
/// this as a known wart (pango-types.h:100-107) and every other Pango client
/// has it, so reproducing it exactly is what makes extents from this plugin
/// agree with extents from anything else. `wrapping_add` matches C's release
/// behaviour near `i32::MAX` without a Rust debug panic.
#[must_use]
pub const fn pango_pixels(d: i32) -> i32 {
    d.wrapping_add(512) >> 10
}

/// `PANGO_PIXELS_FLOOR` from pango-types.h:98.
#[must_use]
pub const fn pango_pixels_floor(d: i32) -> i32 {
    d >> 10
}

/// `PANGO_PIXELS_CEIL` from pango-types.h:99.
#[must_use]
pub const fn pango_pixels_ceil(d: i32) -> i32 {
    d.wrapping_add(1023) >> 10
}

// ---- strings ------------------------------------------------------------

/// A `char *` glib allocated and the caller must `g_free`.
///
/// Nine functions in Pango answer one -- `pango_font_description_to_string`,
/// `_to_filename`, `pango_attr_list_to_string`, `pango_tab_array_to_string`,
/// `pango_color_to_string`, and the `char **text` out-params of
/// `pango_parse_markup` and `pango_markup_parser_finish` -- and eight answer a
/// `const char *` that must **not** be freed. The rule is mechanical, with no
/// exception anywhere in Pango: `const char *` in the header means
/// transfer-none.
///
/// So the distinction is encoded in the type. `*mut c_char` gets this wrapper
/// and its `Drop`; `*const c_char` gets [`borrowed_str`] and no `Drop`.
/// Freeing a borrowed string then becomes a compile error rather than heap
/// corruption, and nine leak sites become one -- an early `?` between the call
/// and the conversion cannot lose the buffer.
pub struct GStr(*mut c_char);

impl GStr {
    /// Takes ownership of a string glib allocated.
    ///
    /// # Safety
    ///
    /// `ptr` must be null, or a NUL-terminated string glib allocated and
    /// handed over `transfer=full`. Nothing else may free it.
    #[must_use]
    pub unsafe fn from_owned(ptr: *mut c_char) -> Self {
        Self(ptr)
    }

    /// Did the function answer NULL? Several of the nine are nullable, and
    /// nullable means "no value", not "failed".
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// The string as Rust text, replacing any invalid UTF-8. Empty for NULL.
    #[must_use]
    pub fn to_string_lossy_owned(&self) -> String {
        if self.0.is_null() {
            return String::new();
        }
        // SAFETY: non-null by the check above, and NUL-terminated by this
        // type's contract; the borrow ends before `self` is dropped.
        unsafe { core::ffi::CStr::from_ptr(self.0) }
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for GStr {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // Never `libc::free` and never Rust's allocator: glib allocated this,
        // and a cross-heap free is a hard crash on Windows. If glib is not
        // loaded there is nothing that could have produced this pointer
        // either, so there is nothing to leak.
        if let Ok(g) = glib() {
            if let Some(free) = g.g_free {
                // SAFETY: `self.0` is a live glib allocation this value owns,
                // and dropping happens exactly once.
                unsafe { free(self.0.cast()) };
            }
        }
    }
}

/// Reads a `const char *` Pango owns, without taking ownership of it.
///
/// Answers `None` for NULL, which several of the borrowed getters use to mean
/// "unset" -- `get_family` and `get_variations` among them -- so the primitive
/// can answer nil rather than an empty String that the image cannot tell from
/// a genuinely empty value.
///
/// # Safety
///
/// `ptr` must be null or NUL-terminated, and must stay valid for the call.
/// Every producer in this crate is `transfer=none` and outlives the primitive.
#[must_use]
pub unsafe fn borrowed_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: delegated to this function's contract.
    Some(
        unsafe { core::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_four_ints_in_sixteen_bytes() {
        // Measured against the real library: size 16, align 4, offsets
        // x=0 y=4 width=8 height=12. This is what lets a stack rectangle be
        // handed to an out-parameter and read back as four SmallIntegers.
        assert_eq!(core::mem::size_of::<PangoRectangle>(), 16);
        assert_eq!(core::mem::align_of::<PangoRectangle>(), 4);
        assert_eq!(
            core::mem::size_of::<PangoRectangle>(),
            4 * core::mem::size_of::<c_int>()
        );
        assert_eq!(core::mem::offset_of!(PangoRectangle, x), 0);
        assert_eq!(core::mem::offset_of!(PangoRectangle, y), 4);
        assert_eq!(core::mem::offset_of!(PangoRectangle, width), 8);
        assert_eq!(core::mem::offset_of!(PangoRectangle, height), 12);
    }

    #[test]
    fn a_colour_is_three_channels_of_sixteen_bits_and_no_alpha() {
        assert_eq!(core::mem::size_of::<PangoColor>(), 6);
        assert_eq!(core::mem::align_of::<PangoColor>(), 2);
    }

    #[test]
    fn the_matrix_struct_is_six_packed_doubles() {
        assert_eq!(core::mem::size_of::<PangoMatrix>(), 48);
        assert_eq!(core::mem::align_of::<PangoMatrix>(), 8);
    }

    #[test]
    fn an_error_struct_matches_the_measured_glib_layout() {
        assert_eq!(core::mem::size_of::<GError>(), 16);
        assert_eq!(core::mem::offset_of!(GError, domain), 0);
        assert_eq!(core::mem::offset_of!(GError, code), 4);
        assert_eq!(core::mem::offset_of!(GError, message), 8);
    }

    #[test]
    fn a_gboolean_is_four_bytes_and_not_a_rust_bool() {
        // The whole reason `gboolean` is an alias rather than `bool`: a Rust
        // bool is one byte and has a validity invariant of exactly 0 or 1,
        // while glib's convention is that any non-zero value is true.
        assert_eq!(core::mem::size_of::<gboolean>(), 4);
        assert_ne!(
            core::mem::size_of::<gboolean>(),
            core::mem::size_of::<bool>()
        );
    }

    #[test]
    fn pangos_matrix_field_order_is_not_cairos() {
        // Cairo stores xx, yx, xy, yy, x0, y0; Pango stores xx, xy, yx, yy,
        // x0, y0. Both are 48 bytes of six doubles, so a memcpy between them
        // compiles and links and silently transposes every rotation and skew.
        let m = PangoMatrix {
            xx: 1.0,
            xy: 2.0,
            yx: 3.0,
            yy: 4.0,
            x0: 5.0,
            y0: 6.0,
        };
        assert_eq!(m.to_pango_array(), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(m.to_cairo_array(), [1.0, 3.0, 2.0, 4.0, 5.0, 6.0]);
        assert_ne!(m.to_pango_array(), m.to_cairo_array());
    }

    #[test]
    fn a_matrix_round_trips_through_each_field_order() {
        let m = PangoMatrix {
            xx: 1.0,
            xy: 2.0,
            yx: 3.0,
            yy: 4.0,
            x0: 5.0,
            y0: 6.0,
        };
        assert_eq!(PangoMatrix::from_pango_slice(&m.to_pango_array()), Some(m));
        assert_eq!(PangoMatrix::from_cairo_slice(&m.to_cairo_array()), Some(m));
        // And crossing the two orders is exactly the silent bug: the same six
        // doubles read the other way round transpose the shear terms.
        let crossed = PangoMatrix::from_pango_slice(&m.to_cairo_array()).unwrap();
        assert_eq!(crossed.xy, m.yx);
        assert_eq!(crossed.yx, m.xy);
        assert_ne!(crossed, m);
    }

    #[test]
    fn a_matrix_needs_exactly_six_doubles() {
        assert_eq!(PangoMatrix::from_pango_slice(&[1.0; 5]), None);
        assert_eq!(PangoMatrix::from_pango_slice(&[1.0; 7]), None);
        assert_eq!(PangoMatrix::from_cairo_slice(&[1.0; 5]), None);
    }

    #[test]
    fn a_scale_only_matrix_looks_identical_in_both_orders() {
        // Why the wrong order is so easy to ship: identity, pure scale and
        // pure translation all survive the transposition unchanged, so only a
        // rotated or sheared layout ever shows the bug.
        let scale = PangoMatrix {
            xx: 2.0,
            xy: 0.0,
            yx: 0.0,
            yy: 3.0,
            x0: 7.0,
            y0: 9.0,
        };
        assert_eq!(scale.to_pango_array(), scale.to_cairo_array());
    }

    #[test]
    fn pango_pixels_rounds_to_nearest() {
        assert_eq!(pango_pixels(0), 0);
        assert_eq!(pango_pixels(1024), 1);
        assert_eq!(pango_pixels(1536), 2);
        assert_eq!(pango_pixels(61440), 60);
        assert_eq!(pango_pixels(65536), 64);
    }

    #[test]
    fn pango_pixels_is_asymmetric_around_zero_exactly_as_pango_is() {
        // Measured against the real macro: +512 answers 1 but -512 answers 0,
        // because `>>` floors. Pango calls this a known wart and every other
        // Pango client has it, so being bug-compatible is what makes this
        // plugin's extents agree with everyone else's.
        assert_eq!(pango_pixels(512), 1);
        assert_eq!(pango_pixels(-512), 0);
        assert_ne!(pango_pixels(-512), -pango_pixels(512));
    }

    #[test]
    fn pango_pixels_does_not_panic_near_the_top_of_the_range() {
        // C's `(int)(d) + 512` overflows here and is undefined; Rust's `+`
        // would panic in a debug build, which inside a primitive means a
        // caught panic and a mystery failure. `wrapping_add` matches what a
        // release C build actually does.
        let _ = pango_pixels(i32::MAX);
        let _ = pango_pixels_ceil(i32::MAX);
    }

    #[test]
    fn floor_and_ceil_bracket_the_rounded_value() {
        for d in [-3000, -1025, -1, 0, 1, 1023, 1024, 5000] {
            assert!(pango_pixels_floor(d) <= pango_pixels(d));
            assert!(pango_pixels(d) <= pango_pixels_ceil(d));
        }
        assert_eq!(pango_pixels_floor(1023), 0);
        assert_eq!(pango_pixels_ceil(1), 1);
    }

    #[test]
    fn version_encoding_matches_pangos_own() {
        assert_eq!(pango_version_encode(1, 46, 0), PANGO_VERSION_1_46);
        assert_eq!(pango_version_encode(1, 50, 0), PANGO_VERSION_1_50);
        assert_eq!(pango_version_encode(1, 56, 0), PANGO_VERSION_1_56);
        assert_eq!(pango_version_encode(1, 58, 0), PANGO_VERSION_1_58);
        assert_eq!(pango_version_encode(1, 58, 2), 15802);
    }

    #[test]
    fn library_names_are_platform_shaped() {
        let names = library_names("pangocairo-1.0");
        assert!(!names.is_empty());
        if cfg!(target_os = "linux") {
            assert_eq!(names[0], "libpangocairo-1.0.so.0");
            assert_eq!(names[1], "libpangocairo-1.0.so");
        }
        if cfg!(target_os = "macos") {
            assert_eq!(names[0], "libpangocairo-1.0.0.dylib");
            assert_eq!(names[1], "libpangocairo-1.0.dylib");
        }
    }

    #[test]
    fn the_macos_name_list_reaches_homebrews_prefix() {
        // The measured failure this exists to prevent: on Apple Silicon the
        // bare name does not resolve, nothing bundles Pango, and without an
        // absolute candidate the plugin declines on a machine that plainly
        // has Pango 1.58.2 installed.
        if !cfg!(target_os = "windows") {
            let names = library_names("pangocairo-1.0");
            for prefix in UNIX_LIBRARY_PREFIXES {
                assert!(
                    names.iter().any(|n| n.starts_with(prefix)),
                    "no candidate under {prefix}"
                );
            }
            assert!(names
                .iter()
                .any(|n| n == "/opt/homebrew/lib/libpangocairo-1.0.0.dylib"
                    || n == "/opt/homebrew/lib/libpangocairo-1.0.so.0"));
            assert!(names
                .iter()
                .any(|n| n.starts_with("/opt/homebrew/opt/pango/lib/")));
            let glib = library_names("glib-2.0");
            assert!(glib
                .iter()
                .any(|n| n.starts_with("/opt/homebrew/opt/glib/lib/")));
        }
    }

    #[test]
    fn the_bundled_names_still_come_before_the_absolute_ones() {
        // `dylib::open_first` tries the names in order, so a copy beside the
        // executable has to be reachable before /opt/homebrew is. Two other
        // plugins depend on that precedence and it is not this crate's to
        // change.
        let names = library_names("pangocairo-1.0");
        let first_absolute = names.iter().position(|n| n.starts_with('/'));
        assert_eq!(first_absolute, Some(dylib::library_names("pangocairo-1.0", "0").len()));
    }

    #[test]
    fn an_environment_override_is_tried_before_anything_else() {
        // Not set here, so the list must start with the ordinary leaf name --
        // asserting the negative keeps the override from silently becoming
        // unconditional.
        assert_eq!(override_variable("pangocairo-1.0"), "PHARO_PANGOCAIRO_LIBRARY");
        assert_eq!(override_variable("glib-2.0"), "PHARO_GLIB_LIBRARY");
        assert_eq!(override_variable("gobject-2.0"), "PHARO_GOBJECT_LIBRARY");
        assert!(std::env::var(override_variable("pangocairo-1.0")).is_err());
        assert!(!library_names("pangocairo-1.0")[0].starts_with('/'));
    }

    #[test]
    fn the_version_gate_names_only_entries_the_table_declares() {
        // A stale name here would quietly excuse a real typo in `pango_api!`,
        // which is the one thing `missing_entry_points` exists to catch. The
        // check runs only with a Pango present, because the declared names
        // are only enumerable through a resolved table.
        let Ok(p) = pango() else { return };
        let declared: Vec<&str> = {
            let mut all = p.missing_entry_points();
            all.extend(ENTRY_MIN_VERSION.iter().map(|(n, _)| *n));
            all
        };
        for (name, _) in ENTRY_MIN_VERSION {
            assert!(declared.contains(name), "{name} is not in the table");
        }
    }

    #[test]
    fn the_version_gate_is_sorted_and_free_of_duplicates() {
        let mut seen = std::collections::HashSet::new();
        let mut previous = 0;
        for (name, since) in ENTRY_MIN_VERSION {
            assert!(seen.insert(*name), "{name} listed twice");
            assert!(*since >= previous, "{name} is out of order");
            previous = *since;
        }
    }

    #[test]
    fn nothing_is_excused_on_a_pango_new_enough_to_have_it_all() {
        assert!(entries_introduced_after(PANGO_VERSION_1_58).is_empty());
        assert!(entries_introduced_after(15802).is_empty());
        // And an old one excuses exactly the entries it should.
        let old = entries_introduced_after(PANGO_VERSION_1_50);
        assert!(old.contains(&"pango_font_description_set_width"));
        assert!(old.contains(&"pango_font_map_add_font_file"));
        assert!(!old.contains(&"pango_attr_list_to_string"));
    }

    #[test]
    fn pango_is_unsupported_until_it_loads() {
        // In a test binary there is no VM bundle; whether this machine has
        // Pango or not, both accessors must answer cleanly rather than panic.
        match pango() {
            Ok(p) => assert!(p.resolved_count() <= p.declared_count()),
            Err(e) => assert_eq!(e, PrimErr::Unsupported),
        }
        match glib() {
            Ok(g) => assert_eq!(g.declared_count(), 8),
            Err(e) => assert_eq!(e, PrimErr::Unsupported),
        }
    }

    #[test]
    fn a_null_owned_string_frees_nothing_and_reads_as_empty() {
        // SAFETY: null is explicitly allowed by `from_owned`'s contract, and
        // the several nullable producers really do answer it.
        let s = unsafe { GStr::from_owned(core::ptr::null_mut()) };
        assert!(s.is_null());
        assert_eq!(s.to_string_lossy_owned(), "");
    }

    #[test]
    fn a_borrowed_null_string_is_none_rather_than_empty() {
        // The distinction the image needs: `get_family` on a description with
        // no family answers NULL, which is not the same as a family named "".
        // SAFETY: null is explicitly allowed by `borrowed_str`'s contract.
        assert_eq!(unsafe { borrowed_str(core::ptr::null()) }, None);
        let text = c"Helvetica";
        // SAFETY: a static NUL-terminated string that outlives the call.
        assert_eq!(
            unsafe { borrowed_str(text.as_ptr()) },
            Some("Helvetica".to_owned())
        );
    }
}
