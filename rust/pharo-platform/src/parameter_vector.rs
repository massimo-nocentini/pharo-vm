//! Replaces `src/parameters/parameterVector.c`.
//!
//! A growable array of borrowed C strings, used to hold the VM's and the
//! image's command-line arguments.
//!
//! # Ownership, unchanged
//!
//! The vector owns its *array* but not the strings in it: the elements point
//! into `argv` and into string literals, and the C never freed them. That is
//! preserved exactly. `vm_parameter_vector_destroy` frees the array and
//! nothing else.
//!
//! The array itself is allocated with `calloc` and freed with `free`, because
//! `src/parameters/parameters.c` still builds and releases these vectors from
//! C and the two allocators must not be mixed.

use core::ffi::{c_char, CStr};

use crate::error_code::VMErrorCode;

/// `VMParameterVector` from
/// `include/pharovm/parameters/parameterVector.h`.
///
/// Taken from `pharo-vm-sys` rather than restated here: `parameters.m` on
/// Apple still shares this struct field for field, so the layout is a live
/// ABI, and bindgen's generated layout tests are what keep the two in step.
/// It has two fields: `count`, and `parameters`, which holds the entries
/// followed by a NULL terminator so the array can be handed straight to
/// `execv` and friends, or null when empty.
pub use pharo_vm_sys::VMParameterVector;

/// Releases the vector's array, leaving it empty.
///
/// The strings are not freed; see the module docs.
///
/// # Safety
///
/// `vector` must be null or point to a valid `VMParameterVector` whose
/// `parameters` came from this module or from the C it replaces.
#[no_mangle]
pub unsafe extern "C" fn vm_parameter_vector_destroy(
    vector: *mut VMParameterVector,
) -> VMErrorCode {
    let Some(vector) = (unsafe { vector.as_mut() }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if !vector.parameters.is_null() {
        // SAFETY: allocated by calloc in insert_from, or by the C it replaces.
        unsafe { libc_free(vector.parameters.cast()) };
    }
    vector.parameters = core::ptr::null_mut();
    vector.count = 0;
    VMErrorCode::VM_SUCCESS
}

/// Appends `count` elements from `elements`, reallocating the array.
///
/// # Safety
///
/// `vector` must be null or valid. If `count` is nonzero, `elements` must point
/// to at least `count` readable pointers, each null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn vm_parameter_vector_insert_from(
    vector: *mut VMParameterVector,
    count: u32,
    elements: *const *const c_char,
) -> VMErrorCode {
    let Some(vector) = (unsafe { vector.as_mut() }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if count == 0 {
        // Nothing to add. The C still reallocated in this case; skipping the
        // work is not observable, since the array's identity is private.
        return VMErrorCode::VM_SUCCESS;
    }
    if elements.is_null() {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }

    // The C added these in `uint32_t` and would have wrapped on overflow,
    // producing an array far too small for the loop that follows. Refuse
    // instead; nothing legitimate reaches four billion arguments.
    let Some(new_count) = vector.count.checked_add(count) else {
        return VMErrorCode::VM_ERROR_OUT_OF_MEMORY;
    };
    // One extra for the NULL terminator, as the C did.
    let Some(slots) = (new_count as usize).checked_add(1) else {
        return VMErrorCode::VM_ERROR_OUT_OF_MEMORY;
    };

    // SAFETY: calloc zeroes, which supplies the NULL terminator.
    let new_data = unsafe { libc_calloc(slots, core::mem::size_of::<*const c_char>()) };
    if new_data.is_null() {
        return VMErrorCode::VM_ERROR_OUT_OF_MEMORY;
    }
    let new_data = new_data.cast::<*const c_char>();

    // SAFETY: new_data has `slots` entries and new_count < slots; the old array
    // holds `vector.count` entries and `elements` holds `count`, both distinct
    // allocations from new_data.
    unsafe {
        if !vector.parameters.is_null() {
            core::ptr::copy_nonoverlapping(
                vector.parameters.cast_const(),
                new_data,
                vector.count as usize,
            );
        }
        core::ptr::copy_nonoverlapping(
            elements,
            new_data.add(vector.count as usize),
            count as usize,
        );
        // Free of null is a no-op, as the C noted.
        libc_free(vector.parameters.cast());
    }

    vector.count = new_count;
    vector.parameters = new_data;
    VMErrorCode::VM_SUCCESS
}

/// Is `parameter` among the vector's elements?
///
/// # Safety
///
/// `vector` must be null or valid, and `parameter` null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn vm_parameter_vector_has_element(
    vector: *const VMParameterVector,
    parameter: *const c_char,
) -> bool {
    let Some(vector) = (unsafe { vector.as_ref() }) else {
        return false;
    };
    if vector.parameters.is_null() || parameter.is_null() {
        return false;
    }
    // SAFETY: the caller guarantees NUL termination.
    let needle = unsafe { CStr::from_ptr(parameter) };
    for i in 0..vector.count as usize {
        // SAFETY: i < count, and the array holds at least count entries.
        let element = unsafe { *vector.parameters.add(i) };
        if element.is_null() {
            // The C passed a null element straight to strcmp and crashed.
            continue;
        }
        // SAFETY: elements are valid C strings by the caller's contract.
        if unsafe { CStr::from_ptr(element) } == needle {
            return true;
        }
    }
    false
}

extern "C" {
    #[link_name = "calloc"]
    fn libc_calloc(count: usize, size: usize) -> *mut core::ffi::c_void;
    #[link_name = "free"]
    fn libc_free(p: *mut core::ffi::c_void);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// An empty vector, as the C's zero-initialised struct would be.
    fn empty() -> VMParameterVector {
        VMParameterVector {
            count: 0,
            parameters: core::ptr::null_mut(),
        }
    }

    /// Inserts owned strings, returning the CStrings so they outlive the call.
    fn insert(v: &mut VMParameterVector, items: &[&str]) -> Vec<CString> {
        let owned: Vec<CString> = items.iter().map(|s| CString::new(*s).unwrap()).collect();
        let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        let rc = unsafe { vm_parameter_vector_insert_from(v, ptrs.len() as u32, ptrs.as_ptr()) };
        assert_eq!(rc, VMErrorCode::VM_SUCCESS);
        owned
    }

    fn contents(v: &VMParameterVector) -> Vec<String> {
        (0..v.count as usize)
            .map(|i| {
                unsafe { CStr::from_ptr(*v.parameters.add(i)) }
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn inserts_and_reads_back() {
        let mut v = empty();
        let _own = insert(&mut v, &["--headless", "img.image"]);
        assert_eq!(v.count, 2);
        assert_eq!(contents(&v), ["--headless", "img.image"]);
        unsafe { vm_parameter_vector_destroy(&mut v) };
    }

    #[test]
    fn appends_to_an_existing_vector() {
        let mut v = empty();
        let _a = insert(&mut v, &["one"]);
        let _b = insert(&mut v, &["two", "three"]);
        assert_eq!(v.count, 3);
        assert_eq!(contents(&v), ["one", "two", "three"]);
        unsafe { vm_parameter_vector_destroy(&mut v) };
    }

    /// The array is NULL-terminated so it can be passed to execv.
    #[test]
    fn the_array_is_null_terminated() {
        let mut v = empty();
        let _own = insert(&mut v, &["a", "b"]);
        let terminator = unsafe { *v.parameters.add(v.count as usize) };
        assert!(terminator.is_null());
        unsafe { vm_parameter_vector_destroy(&mut v) };
    }

    #[test]
    fn destroy_empties_the_vector_and_is_idempotent() {
        let mut v = empty();
        let _own = insert(&mut v, &["a"]);
        assert_eq!(
            unsafe { vm_parameter_vector_destroy(&mut v) },
            VMErrorCode::VM_SUCCESS
        );
        assert_eq!(v.count, 0);
        assert!(v.parameters.is_null());
        assert_eq!(
            unsafe { vm_parameter_vector_destroy(&mut v) },
            VMErrorCode::VM_SUCCESS
        );
    }

    #[test]
    fn finds_and_rejects_elements() {
        let mut v = empty();
        let _own = insert(&mut v, &["--headless", "img.image"]);
        let yes = CString::new("--headless").unwrap();
        let no = CString::new("--interactive").unwrap();
        assert!(unsafe { vm_parameter_vector_has_element(&v, yes.as_ptr()) });
        assert!(!unsafe { vm_parameter_vector_has_element(&v, no.as_ptr()) });
        unsafe { vm_parameter_vector_destroy(&mut v) };
    }

    #[test]
    fn searching_an_empty_vector_finds_nothing() {
        let v = empty();
        let needle = CString::new("x").unwrap();
        assert!(!unsafe { vm_parameter_vector_has_element(&v, needle.as_ptr()) });
    }

    #[test]
    fn null_vector_is_reported_not_dereferenced() {
        let needle = CString::new("x").unwrap();
        assert_eq!(
            unsafe { vm_parameter_vector_destroy(core::ptr::null_mut()) },
            VMErrorCode::VM_ERROR_NULL_POINTER
        );
        assert!(!unsafe { vm_parameter_vector_has_element(core::ptr::null(), needle.as_ptr()) });
        assert_eq!(
            unsafe { vm_parameter_vector_insert_from(core::ptr::null_mut(), 0, core::ptr::null()) },
            VMErrorCode::VM_ERROR_NULL_POINTER
        );
    }

    #[test]
    fn inserting_nothing_leaves_the_vector_alone() {
        let mut v = empty();
        let _own = insert(&mut v, &["a"]);
        let rc = unsafe { vm_parameter_vector_insert_from(&mut v, 0, core::ptr::null()) };
        assert_eq!(rc, VMErrorCode::VM_SUCCESS);
        assert_eq!(contents(&v), ["a"]);
        unsafe { vm_parameter_vector_destroy(&mut v) };
    }

    /// The C's `vector->count + count` would wrap here and then loop past the
    /// end of a far-too-small array.
    #[test]
    fn a_count_overflow_is_refused_rather_than_wrapped() {
        let mut v = empty();
        v.count = u32::MAX - 1;
        let dummy: [*const c_char; 1] = [core::ptr::null()];
        let rc = unsafe { vm_parameter_vector_insert_from(&mut v, 5, dummy.as_ptr()) };
        assert_eq!(rc, VMErrorCode::VM_ERROR_OUT_OF_MEMORY);
        // Nothing to clean up: the refusal left `parameters` null, so the
        // fictional count never gets walked.
        assert!(v.parameters.is_null());
    }
}

/// Proves the hand-declared struct still matches the header.
///
/// `VMParameterVector` is declared by hand above so that this module does not
/// have to route every field access through generated bindings, but the layout
/// is a live ABI: `src/parameters/parameters.c` and `parameters.m` are still C
/// and share this struct. A silent mismatch would scramble the VM's argument
/// list, so it is checked rather than assumed.
#[cfg(test)]
mod layout {
    use core::mem::{align_of, offset_of, size_of};

    use super::VMParameterVector as Ours;
    use pharo_vm_sys::VMParameterVector as Theirs;

    #[test]
    fn matches_the_header() {
        assert_eq!(size_of::<Ours>(), size_of::<Theirs>(), "struct size");
        assert_eq!(align_of::<Ours>(), align_of::<Theirs>(), "struct alignment");
        assert_eq!(
            offset_of!(Ours, count),
            offset_of!(Theirs, count),
            "offset of `count`"
        );
        assert_eq!(
            offset_of!(Ours, parameters),
            offset_of!(Theirs, parameters),
            "offset of `parameters`"
        );
    }
}
