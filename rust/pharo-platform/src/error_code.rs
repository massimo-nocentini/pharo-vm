//! Replaces `src/errorCode.c`.
//!
//! Wave 0 of the port: the smallest real file in the platform layer, ported to
//! prove the build integration end to end (CMake -> cargo -> bindgen ->
//! staticlib -> link -> ABI check) rather than for its own sake.

use core::ffi::c_char;
pub use pharo_vm_sys::VMErrorCode;

/// The strings the C implementation returned, as NUL-terminated literals so
/// they can be handed out as `const char*` with no allocation and a `'static`
/// lifetime.
///
/// `SUCCESS` reads "sucess" because `src/errorCode.c` did. It is a typo, and
/// nothing depends on it (it only ever reaches a log line), but fixing it here
/// would make this commit a behaviour change and spoil the before/after
/// comparison this wave exists to establish. Fix it separately.
mod strings {
    pub const SUCCESS: &[u8] = b"sucess\0";
    pub const OUT_OF_MEMORY: &[u8] = b"out of memory.\0";
    pub const NULL_POINTER: &[u8] = b"null pointer.\0";
    pub const EXIT_WITH_SUCCESS: &[u8] = b"exit with success.\0";
    pub const INVALID_PARAMETER: &[u8] = b"invalid parameter.\0";
    /// Yes, "null" where the constant says "invalid": also copied verbatim.
    pub const INVALID_PARAMETER_VALUE: &[u8] = b"null parameter value.\0";
    pub const GENERIC: &[u8] = b"generic error\0";
}

/// Returns a human-readable description of `error_code`.
///
/// Unrecognised values -- including `VM_ERROR` itself, which shared the
/// `default` arm in C -- map to "generic error".
///
/// # Safety
///
/// The returned pointer is to a `'static` NUL-terminated string in read-only
/// memory. Callers must not free or modify it. It stays valid for the lifetime
/// of the process.
#[no_mangle]
pub extern "C" fn vm_error_code_to_string(error_code: VMErrorCode) -> *const c_char {
    let s: &[u8] = match error_code {
        VMErrorCode::VM_SUCCESS => strings::SUCCESS,
        VMErrorCode::VM_ERROR_OUT_OF_MEMORY => strings::OUT_OF_MEMORY,
        VMErrorCode::VM_ERROR_NULL_POINTER => strings::NULL_POINTER,
        VMErrorCode::VM_ERROR_EXIT_WITH_SUCCESS => strings::EXIT_WITH_SUCCESS,
        VMErrorCode::VM_ERROR_INVALID_PARAMETER => strings::INVALID_PARAMETER,
        VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE => strings::INVALID_PARAMETER_VALUE,
        // C listed `case VM_ERROR:` alongside `default:`; kept explicit so the
        // correspondence to the original switch stays readable.
        VMErrorCode::VM_ERROR => strings::GENERIC,
        _ => strings::GENERIC,
    };
    s.as_ptr().cast::<c_char>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::CStr;

    /// Reads back what the C caller would see.
    fn describe(code: VMErrorCode) -> &'static str {
        // SAFETY: vm_error_code_to_string only ever returns pointers to the
        // 'static NUL-terminated literals in `strings`.
        unsafe { CStr::from_ptr(vm_error_code_to_string(code)) }
            .to_str()
            .expect("all descriptions are ASCII")
    }

    #[test]
    fn maps_each_documented_code() {
        // Verbatim against src/errorCode.c, typos included.
        assert_eq!(describe(VMErrorCode::VM_SUCCESS), "sucess");
        assert_eq!(
            describe(VMErrorCode::VM_ERROR_OUT_OF_MEMORY),
            "out of memory."
        );
        assert_eq!(
            describe(VMErrorCode::VM_ERROR_NULL_POINTER),
            "null pointer."
        );
        assert_eq!(
            describe(VMErrorCode::VM_ERROR_EXIT_WITH_SUCCESS),
            "exit with success."
        );
        assert_eq!(
            describe(VMErrorCode::VM_ERROR_INVALID_PARAMETER),
            "invalid parameter."
        );
        assert_eq!(
            describe(VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE),
            "null parameter value."
        );
        assert_eq!(describe(VMErrorCode::VM_ERROR), "generic error");
    }

    #[test]
    fn unknown_codes_fall_through_to_generic() {
        // The C switch had no arm for these; they hit `default`.
        assert_eq!(describe(VMErrorCode(42)), "generic error");
        assert_eq!(describe(VMErrorCode(-99)), "generic error");
    }
}
