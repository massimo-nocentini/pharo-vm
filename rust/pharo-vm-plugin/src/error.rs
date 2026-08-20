//! Primitive failure codes.
//!
//! A Pharo primitive does not raise; it *fails*, and the image then runs the
//! method's Smalltalk fallback code. Failing with a specific code lets the
//! fallback tell "bad argument" from "out of memory".
//!
//! Values mirror the `PrimErr*` constants in the generated `interp.h`.

use crate::proxy::sqInt;

/// Why a primitive failed.
///
/// Returning `Err(_)` from a `#[pharo_primitive]` function calls the VM's
/// `primitiveFailFor` with this code and leaves the stack untouched, which is
/// exactly the contract the image's fallback code expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[repr(isize)]
pub enum PrimErr {
    /// No more specific reason. Equivalent to a bare `primitiveFail()`.
    GenericFailure = 1,
    /// The receiver was not of a type this primitive accepts.
    BadReceiver = 2,
    /// An argument was not of a type this primitive accepts.
    BadArgument = 3,
    /// An index argument was out of bounds.
    BadIndex = 4,
    /// The primitive was called with the wrong number of arguments.
    BadNumArgs = 5,
    /// The operation does not apply to this receiver.
    Inappropriate = 6,
    /// Not supported by this VM or platform.
    Unsupported = 7,
    /// The target is read-only.
    NoModification = 8,
    /// Object memory allocation failed.
    NoMemory = 9,
    /// Malloc (C heap) allocation failed.
    NoCMemory = 10,
    /// A required entity was not found.
    NotFound = 11,
    /// The method is malformed.
    BadMethod = 12,
    /// Reserved for the VM's internal named-primitive machinery.
    NamedInternal = 13,
    /// The object may move; pin it first.
    ObjectMayMove = 14,
    /// An implementation limit was exceeded.
    LimitExceeded = 15,
    /// The object is pinned and cannot be moved.
    ObjectIsPinned = 16,
    /// A write would have run past the end of the object.
    WritePastObject = 17,
    /// The object moved during the operation.
    ObjectMoved = 18,
    /// The object was not pinned.
    ObjectNotPinned = 19,
    /// A callback failed.
    CallbackError = 20,
    /// The underlying OS call failed.
    OSError = 21,
    /// An FFI call raised.
    FFIException = 22,
    /// Object memory needs compaction first.
    NeedCompaction = 23,
    /// The operation failed for a reason the caller is expected to interpret.
    OperationFailed = 24,
}

impl PrimErr {
    /// The numeric code the VM expects.
    #[must_use]
    pub const fn code(self) -> sqInt {
        self as sqInt
    }
}

/// What a primitive body returns.
///
/// `Ok` carries whatever the primitive answers (see [`crate::IntoReturn`]);
/// `Err` carries the failure code.
pub type PrimResult<T> = Result<T, PrimErr>;
