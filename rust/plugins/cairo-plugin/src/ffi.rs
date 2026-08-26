//! The Cairo binding: types, enumerations and the function table.
//!
//! Bound at runtime rather than at link time. `cmake/importCairo.cmake`
//! downloads Cairo as a ready-made binary -- "Cairo does not support building
//! on CMake", as the file says -- so what the bundle contains is a `.so`,
//! `.dylib` or `.dll` with no headers and no `pkg-config` metadata. There is
//! nothing for a linker to consume, and requiring a system `libcairo-dev`
//! purely to compile a VM that already ships Cairo would be a poor trade. So
//! this module `dlopen`s the same file the image's FFI would have used.
//!
//! Every entry is `Option`, and a missing one costs its primitives rather than
//! the module: Cairo is versioned independently of the VM, and this plugin has
//! to load beside whichever of 1.16, 1.17 and 1.18 the platform's bundle
//! carries.
//!
//! Signatures are transcribed from `cairo.h`. They are stable published API --
//! Cairo has not broken one since 1.0 -- but they are transcribed, not
//! generated, so an error here is an error nothing in this tree would catch.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_double, c_int, c_uchar, c_uint};
use std::sync::OnceLock;

use pharo_vm_plugin::dylib;
use pharo_vm_plugin::{PrimErr, PrimResult};

/// A drawing context. Opaque: only Cairo ever dereferences one.
#[repr(C)]
pub struct cairo_t {
    _private: [u8; 0],
}

/// A drawing target.
#[repr(C)]
pub struct cairo_surface_t {
    _private: [u8; 0],
}

/// A source of paint: a colour, a gradient, or another surface.
#[repr(C)]
pub struct cairo_pattern_t {
    _private: [u8; 0],
}

/// An affine transformation, in Cairo's field order.
///
/// `x' = xx*x + xy*y + x0`, `y' = yx*x + yy*y + y0`. Note that the middle two
/// fields are *not* in the order the names suggest reading them: `yx` comes
/// before `xy`, as in the header. Passed to and from the image as six
/// native-endian doubles in a ByteArray, in exactly this order.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct cairo_matrix_t {
    pub xx: c_double,
    pub yx: c_double,
    pub xy: c_double,
    pub yy: c_double,
    pub x0: c_double,
    pub y0: c_double,
}

impl cairo_matrix_t {
    /// The six doubles, in the order the image sends and receives them.
    #[must_use]
    pub fn to_array(self) -> [f64; 6] {
        [self.xx, self.yx, self.xy, self.yy, self.x0, self.y0]
    }

    /// Rebuilds a matrix from those six doubles.
    #[must_use]
    pub fn from_slice(v: &[f64]) -> Option<Self> {
        match v {
            [xx, yx, xy, yy, x0, y0] => Some(Self {
                xx: *xx,
                yx: *yx,
                xy: *xy,
                yy: *yy,
                x0: *x0,
                y0: *y0,
            }),
            _ => None,
        }
    }
}

/// What one run of text occupies.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct cairo_text_extents_t {
    pub x_bearing: c_double,
    pub y_bearing: c_double,
    pub width: c_double,
    pub height: c_double,
    pub x_advance: c_double,
    pub y_advance: c_double,
}

/// What the current font occupies, independently of any particular text.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct cairo_font_extents_t {
    pub ascent: c_double,
    pub descent: c_double,
    pub height: c_double,
    pub max_x_advance: c_double,
    pub max_y_advance: c_double,
}

/// `CAIRO_STATUS_SUCCESS`. Every other value is an error.
pub const CAIRO_STATUS_SUCCESS: c_int = 0;
/// `CAIRO_STATUS_NO_MEMORY`.
pub const CAIRO_STATUS_NO_MEMORY: c_int = 1;
/// `CAIRO_STATUS_NULL_POINTER`, which is what a nil surface answers.
pub const CAIRO_STATUS_NULL_POINTER: c_int = 9;

/// Highest `cairo_format_t` this plugin will pass through, `CAIRO_FORMAT_RGB16_565`.
///
/// Formats are validated rather than forwarded: `cairo_image_surface_create`
/// with an out-of-range format answers a surface in an error state, and the
/// image would then be holding a handle on something unusable. Failing the
/// primitive says so at the point of the mistake.
pub const CAIRO_FORMAT_MAX: c_int = 4;
/// Lowest `cairo_format_t` accepted, `CAIRO_FORMAT_ARGB32`.
///
/// `CAIRO_FORMAT_INVALID` is -1 and is deliberately excluded.
pub const CAIRO_FORMAT_MIN: c_int = 0;

/// Declares the function table, one `Option` per symbol.
macro_rules! cairo_api {
    ($( fn $name:ident ( $($arg:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        /// Cairo's entry points, resolved once when the module loads.
        pub struct Cairo {
            /// Where the library was found, for `primitiveLibraryPath`.
            pub path: String,
            $(
                #[allow(missing_docs)]
                pub $name: Option<unsafe extern "C" fn($($arg),*) $(-> $ret)?>,
            )*
        }

        impl Cairo {
            /// Resolves every entry out of an already-open library.
            ///
            /// # Safety
            ///
            /// `lib` must be Cairo, and must outlive every pointer taken from
            /// it -- which is why the caller leaks it.
            unsafe fn resolve(lib: &'static dylib::Library, path: String) -> Self {
                Self {
                    path,
                    // SAFETY: each name is a Cairo entry point whose signature
                    // is transcribed from cairo.h just above.
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
            /// A name misspelled here would otherwise look exactly like a
            /// Cairo too old to have the entry point, and the primitive would
            /// answer `Unsupported` for the life of the plugin. The live tests
            /// assert this is empty against a real Cairo, which is the only
            /// check that catches a typo in a symbol name.
            #[must_use]
            pub fn missing_entry_points(&self) -> Vec<&'static str> {
                let mut missing = Vec::new();
                $( if self.$name.is_none() { missing.push(stringify!($name)); } )*
                missing
            }
        }
    };
}

cairo_api! {
    // ---- surfaces ----
    fn cairo_image_surface_create(c_int, c_int, c_int) -> *mut cairo_surface_t;
    fn cairo_image_surface_create_for_data(
        *mut c_uchar, c_int, c_int, c_int, c_int
    ) -> *mut cairo_surface_t;
    fn cairo_image_surface_create_from_png(*const c_char) -> *mut cairo_surface_t;
    fn cairo_image_surface_get_data(*mut cairo_surface_t) -> *mut c_uchar;
    fn cairo_image_surface_get_format(*mut cairo_surface_t) -> c_int;
    fn cairo_image_surface_get_width(*mut cairo_surface_t) -> c_int;
    fn cairo_image_surface_get_height(*mut cairo_surface_t) -> c_int;
    fn cairo_image_surface_get_stride(*mut cairo_surface_t) -> c_int;
    fn cairo_format_stride_for_width(c_int, c_int) -> c_int;
    fn cairo_surface_destroy(*mut cairo_surface_t);
    fn cairo_surface_status(*mut cairo_surface_t) -> c_int;
    fn cairo_surface_flush(*mut cairo_surface_t);
    fn cairo_surface_mark_dirty(*mut cairo_surface_t);
    fn cairo_surface_mark_dirty_rectangle(*mut cairo_surface_t, c_int, c_int, c_int, c_int);
    fn cairo_surface_set_device_offset(*mut cairo_surface_t, c_double, c_double);
    fn cairo_surface_write_to_png(*mut cairo_surface_t, *const c_char) -> c_int;
    // Consulted before destroying a surface that draws into pinned image
    // memory: it is the only way to know whether a context or pattern still
    // holds a reference, and so whether the pin can be released.
    fn cairo_surface_get_reference_count(*mut cairo_surface_t) -> c_uint;

    // ---- contexts ----
    fn cairo_create(*mut cairo_surface_t) -> *mut cairo_t;
    fn cairo_destroy(*mut cairo_t);
    fn cairo_status(*mut cairo_t) -> c_int;
    fn cairo_status_to_string(c_int) -> *const c_char;
    fn cairo_save(*mut cairo_t);
    fn cairo_restore(*mut cairo_t);
    fn cairo_push_group(*mut cairo_t);
    fn cairo_pop_group_to_source(*mut cairo_t);

    // ---- painting ----
    fn cairo_paint(*mut cairo_t);
    fn cairo_paint_with_alpha(*mut cairo_t, c_double);
    fn cairo_fill(*mut cairo_t);
    fn cairo_fill_preserve(*mut cairo_t);
    fn cairo_stroke(*mut cairo_t);
    fn cairo_stroke_preserve(*mut cairo_t);
    fn cairo_clip(*mut cairo_t);
    fn cairo_clip_preserve(*mut cairo_t);
    fn cairo_reset_clip(*mut cairo_t);
    fn cairo_mask(*mut cairo_t, *mut cairo_pattern_t);
    fn cairo_mask_surface(*mut cairo_t, *mut cairo_surface_t, c_double, c_double);

    // ---- paths ----
    fn cairo_new_path(*mut cairo_t);
    fn cairo_new_sub_path(*mut cairo_t);
    fn cairo_close_path(*mut cairo_t);
    fn cairo_move_to(*mut cairo_t, c_double, c_double);
    fn cairo_line_to(*mut cairo_t, c_double, c_double);
    fn cairo_rel_move_to(*mut cairo_t, c_double, c_double);
    fn cairo_rel_line_to(*mut cairo_t, c_double, c_double);
    fn cairo_curve_to(*mut cairo_t, c_double, c_double, c_double, c_double, c_double, c_double);
    fn cairo_rel_curve_to(
        *mut cairo_t, c_double, c_double, c_double, c_double, c_double, c_double
    );
    fn cairo_rectangle(*mut cairo_t, c_double, c_double, c_double, c_double);
    fn cairo_arc(*mut cairo_t, c_double, c_double, c_double, c_double, c_double);
    fn cairo_arc_negative(*mut cairo_t, c_double, c_double, c_double, c_double, c_double);

    // ---- sources ----
    fn cairo_set_source_rgb(*mut cairo_t, c_double, c_double, c_double);
    fn cairo_set_source_rgba(*mut cairo_t, c_double, c_double, c_double, c_double);
    fn cairo_set_source(*mut cairo_t, *mut cairo_pattern_t);
    fn cairo_set_source_surface(*mut cairo_t, *mut cairo_surface_t, c_double, c_double);

    // ---- graphics state ----
    fn cairo_set_line_width(*mut cairo_t, c_double);
    fn cairo_get_line_width(*mut cairo_t) -> c_double;
    fn cairo_set_line_cap(*mut cairo_t, c_int);
    fn cairo_set_line_join(*mut cairo_t, c_int);
    fn cairo_set_miter_limit(*mut cairo_t, c_double);
    fn cairo_set_dash(*mut cairo_t, *const c_double, c_int, c_double);
    fn cairo_set_fill_rule(*mut cairo_t, c_int);
    fn cairo_set_operator(*mut cairo_t, c_int);
    fn cairo_set_antialias(*mut cairo_t, c_int);
    fn cairo_set_tolerance(*mut cairo_t, c_double);

    // ---- transformations ----
    fn cairo_translate(*mut cairo_t, c_double, c_double);
    fn cairo_scale(*mut cairo_t, c_double, c_double);
    fn cairo_rotate(*mut cairo_t, c_double);
    fn cairo_transform(*mut cairo_t, *const cairo_matrix_t);
    fn cairo_set_matrix(*mut cairo_t, *const cairo_matrix_t);
    fn cairo_get_matrix(*mut cairo_t, *mut cairo_matrix_t);
    fn cairo_identity_matrix(*mut cairo_t);
    fn cairo_user_to_device(*mut cairo_t, *mut c_double, *mut c_double);
    fn cairo_device_to_user(*mut cairo_t, *mut c_double, *mut c_double);
    fn cairo_user_to_device_distance(*mut cairo_t, *mut c_double, *mut c_double);
    fn cairo_device_to_user_distance(*mut cairo_t, *mut c_double, *mut c_double);

    // ---- measuring ----
    fn cairo_path_extents(*mut cairo_t, *mut c_double, *mut c_double, *mut c_double, *mut c_double);
    fn cairo_fill_extents(*mut cairo_t, *mut c_double, *mut c_double, *mut c_double, *mut c_double);
    fn cairo_stroke_extents(
        *mut cairo_t, *mut c_double, *mut c_double, *mut c_double, *mut c_double
    );
    fn cairo_clip_extents(*mut cairo_t, *mut c_double, *mut c_double, *mut c_double, *mut c_double);
    fn cairo_in_fill(*mut cairo_t, c_double, c_double) -> c_int;
    fn cairo_in_stroke(*mut cairo_t, c_double, c_double) -> c_int;
    fn cairo_in_clip(*mut cairo_t, c_double, c_double) -> c_int;

    // ---- text ----
    fn cairo_select_font_face(*mut cairo_t, *const c_char, c_int, c_int);
    fn cairo_set_font_size(*mut cairo_t, c_double);
    fn cairo_set_font_matrix(*mut cairo_t, *const cairo_matrix_t);
    fn cairo_show_text(*mut cairo_t, *const c_char);
    fn cairo_text_path(*mut cairo_t, *const c_char);
    fn cairo_text_extents(*mut cairo_t, *const c_char, *mut cairo_text_extents_t);
    fn cairo_font_extents(*mut cairo_t, *mut cairo_font_extents_t);

    // ---- patterns ----
    fn cairo_pattern_create_rgb(c_double, c_double, c_double) -> *mut cairo_pattern_t;
    fn cairo_pattern_create_rgba(c_double, c_double, c_double, c_double) -> *mut cairo_pattern_t;
    fn cairo_pattern_create_linear(
        c_double, c_double, c_double, c_double
    ) -> *mut cairo_pattern_t;
    fn cairo_pattern_create_radial(
        c_double, c_double, c_double, c_double, c_double, c_double
    ) -> *mut cairo_pattern_t;
    fn cairo_pattern_create_for_surface(*mut cairo_surface_t) -> *mut cairo_pattern_t;
    fn cairo_pattern_destroy(*mut cairo_pattern_t);
    fn cairo_pattern_status(*mut cairo_pattern_t) -> c_int;
    fn cairo_pattern_add_color_stop_rgba(
        *mut cairo_pattern_t, c_double, c_double, c_double, c_double, c_double
    );
    fn cairo_pattern_set_extend(*mut cairo_pattern_t, c_int);
    fn cairo_pattern_set_filter(*mut cairo_pattern_t, c_int);
    fn cairo_pattern_set_matrix(*mut cairo_pattern_t, *const cairo_matrix_t);
    fn cairo_pattern_get_matrix(*mut cairo_pattern_t, *mut cairo_matrix_t);

    // ---- the library itself ----
    fn cairo_version() -> c_int;
    fn cairo_version_string() -> *const c_char;
    fn cairo_debug_reset_static_data();
}

static CAIRO: OnceLock<Option<Cairo>> = OnceLock::new();

/// File names the bundle or the system might have Cairo under.
///
/// `cmake/importCairo.cmake` unpacks whatever `cairo-1.16.0.zip` and friends
/// contain into the directory beside the executable, so the versioned soname
/// is the likely hit; the unversioned name covers a system install.
fn library_names() -> Vec<String> {
    dylib::library_names("cairo", "2")
}

/// Loads Cairo, once. Answers whether it is now available.
///
/// Called from the module's init hook, so a VM whose bundle has no Cairo
/// rejects the module cleanly and the image can fall back to its FFI binding,
/// instead of loading a plugin every primitive of which would fail.
pub fn load() -> bool {
    CAIRO
        .get_or_init(|| {
            let names: Vec<String> = library_names();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            // SAFETY: these names denote Cairo, and every signature this
            // module goes on to declare is transcribed from cairo.h.
            let (lib, path) = unsafe { dylib::open_first(&refs) }?;
            let lib = dylib::leak(lib);
            // SAFETY: `lib` is now 'static, so the function pointers taken out
            // of it stay valid for the life of the process.
            let cairo = unsafe { Cairo::resolve(lib, path) };
            // A library that exports none of what we asked for is not Cairo.
            // Better to decline than to load and fail every primitive.
            if cairo.cairo_create.is_none() || cairo.cairo_image_surface_create.is_none() {
                return None;
            }
            Some(cairo)
        })
        .is_some()
}

/// The loaded library, or `Unsupported` if it never loaded.
pub fn cairo() -> PrimResult<&'static Cairo> {
    CAIRO
        .get()
        .and_then(Option::as_ref)
        .ok_or(PrimErr::Unsupported)
}

/// Calls a Cairo entry point, failing cleanly when the library lacks it.
///
/// The same shape as the SDK's proxy `call!`, and for the same reason: an
/// absent symbol is a deployment fact, not bad input from the image.
macro_rules! cc {
    ($cairo:expr, $f:ident ( $($arg:expr),* $(,)? )) => {{
        let f = $cairo.$f.ok_or(::pharo_vm_plugin::PrimErr::Unsupported)?;
        // SAFETY: the pointer came out of the Cairo we loaded, and the
        // signature is the one declared in `cairo_api!` from cairo.h.
        unsafe { f($($arg),*) }
    }};
}
pub(crate) use cc;

/// Turns a Cairo status into a primitive outcome.
///
/// Cairo does not report errors per call: a context or surface that hits one
/// latches it and ignores everything afterwards. Primitives that can leave the
/// image holding a broken object check the status and fail, so the mistake
/// surfaces where it was made rather than as a silently blank drawing.
pub fn check_status(status: c_int) -> PrimResult<()> {
    match status {
        CAIRO_STATUS_SUCCESS => Ok(()),
        CAIRO_STATUS_NO_MEMORY => Err(PrimErr::NoCMemory),
        CAIRO_STATUS_NULL_POINTER => Err(PrimErr::BadArgument),
        _ => Err(PrimErr::OperationFailed),
    }
}

/// Is this a `cairo_format_t` the plugin will pass through?
#[must_use]
pub fn is_valid_format(format: c_int) -> bool {
    (CAIRO_FORMAT_MIN..=CAIRO_FORMAT_MAX).contains(&format)
}

/// Reads a NUL-terminated string Cairo owns.
///
/// # Safety
///
/// `ptr` must be NUL-terminated and stay valid for the call. Cairo's
/// `cairo_status_to_string` and `cairo_version_string` both answer static
/// strings, which is the only place this is used.
pub unsafe fn owned_str(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: delegated to this function's contract.
    unsafe { core::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Bytes per pixel in a `cairo_format_t`, where that is meaningful.
///
/// Needed to size the copies between an image Bitmap and a surface Cairo owns.
/// `CAIRO_FORMAT_A1` has no whole-byte pixel size, so it answers `None` and
/// its primitives decline rather than guess.
#[must_use]
pub fn format_bytes_per_pixel(format: c_int) -> Option<usize> {
    match format {
        0 | 1 => Some(4),  // ARGB32, RGB24
        2 => Some(1),      // A8
        3 => None,         // A1: one bit per pixel
        4 => Some(2),      // RGB16_565
        _ => None,
    }
}

/// `c_uint` is unused outside the declarations above; naming it keeps the
/// import list honest about what the table needs.
const _: Option<c_uint> = None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matrix_round_trips_through_its_six_doubles() {
        let m = cairo_matrix_t {
            xx: 1.0,
            yx: 2.0,
            xy: 3.0,
            yy: 4.0,
            x0: 5.0,
            y0: 6.0,
        };
        assert_eq!(m.to_array(), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(cairo_matrix_t::from_slice(&m.to_array()), Some(m));
    }

    #[test]
    fn a_matrix_needs_exactly_six_doubles() {
        assert_eq!(cairo_matrix_t::from_slice(&[1.0; 5]), None);
        assert_eq!(cairo_matrix_t::from_slice(&[1.0; 7]), None);
    }

    #[test]
    fn the_matrix_struct_is_six_packed_doubles() {
        // What lets the image hand one over as a 48-byte ByteArray.
        assert_eq!(core::mem::size_of::<cairo_matrix_t>(), 48);
        assert_eq!(core::mem::align_of::<cairo_matrix_t>(), 8);
    }

    #[test]
    fn extents_structs_match_the_headers_field_counts() {
        assert_eq!(core::mem::size_of::<cairo_text_extents_t>(), 48);
        assert_eq!(core::mem::size_of::<cairo_font_extents_t>(), 40);
    }

    #[test]
    fn status_maps_success_apart_from_every_error() {
        assert!(check_status(CAIRO_STATUS_SUCCESS).is_ok());
        assert_eq!(check_status(CAIRO_STATUS_NO_MEMORY), Err(PrimErr::NoCMemory));
        assert_eq!(
            check_status(CAIRO_STATUS_NULL_POINTER),
            Err(PrimErr::BadArgument)
        );
        assert_eq!(check_status(31), Err(PrimErr::OperationFailed));
    }

    #[test]
    fn only_real_formats_are_accepted() {
        assert!(!is_valid_format(-1)); // CAIRO_FORMAT_INVALID
        for f in 0..=4 {
            assert!(is_valid_format(f), "format {f} should be accepted");
        }
        assert!(!is_valid_format(5));
    }

    #[test]
    fn a1_has_no_whole_byte_pixel_size() {
        assert_eq!(format_bytes_per_pixel(0), Some(4));
        assert_eq!(format_bytes_per_pixel(1), Some(4));
        assert_eq!(format_bytes_per_pixel(2), Some(1));
        assert_eq!(format_bytes_per_pixel(3), None);
        assert_eq!(format_bytes_per_pixel(4), Some(2));
        assert_eq!(format_bytes_per_pixel(9), None);
    }

    #[test]
    fn library_names_are_platform_shaped() {
        let names = library_names();
        assert!(!names.is_empty());
        if cfg!(target_os = "linux") {
            assert_eq!(names[0], "libcairo.so.2");
        }
    }

    #[test]
    fn cairo_is_unsupported_until_it_loads() {
        // In a test binary there is no VM bundle, so `load()` finds nothing
        // unless the machine happens to have Cairo installed. Either way,
        // `cairo()` must answer a clean failure rather than panic.
        match cairo() {
            Ok(c) => assert!(c.resolved_count() <= c.declared_count()),
            Err(e) => assert_eq!(e, PrimErr::Unsupported),
        }
    }
}
