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

impl core::fmt::Display for PrimErr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::GenericFailure => "primitive failed",
            Self::BadReceiver => "bad receiver",
            Self::BadArgument => "bad argument",
            Self::BadIndex => "index out of bounds",
            Self::BadNumArgs => "wrong number of arguments",
            Self::Inappropriate => "inappropriate operation",
            Self::Unsupported => "unsupported operation",
            Self::NoModification => "target is read-only",
            Self::NoMemory => "out of object memory",
            Self::NoCMemory => "out of C memory",
            Self::NotFound => "not found",
            Self::BadMethod => "malformed method",
            Self::NamedInternal => "named-primitive machinery failure",
            Self::ObjectMayMove => "object may move",
            Self::LimitExceeded => "limit exceeded",
            Self::ObjectIsPinned => "object is pinned",
            Self::WritePastObject => "write past end of object",
            Self::ObjectMoved => "object moved",
            Self::ObjectNotPinned => "object not pinned",
            Self::CallbackError => "callback failed",
            Self::OSError => "OS call failed",
            Self::FFIException => "FFI call raised",
            Self::NeedCompaction => "object memory needs compaction",
            Self::OperationFailed => "operation failed",
        })
    }
}

impl std::error::Error for PrimErr {}

/// A value that does not fit the integer type a primitive needs is a malformed
/// argument, so `usize::try_from(len)?` fails the right way on its own.
impl From<core::num::TryFromIntError> for PrimErr {
    fn from(_: core::num::TryFromIntError) -> Self {
        Self::BadArgument
    }
}

/// An I/O error maps to the code the VM reserves for failed OS calls; the
/// underlying `io::Error` detail has nowhere to go, since the image only sees
/// the code.
impl From<std::io::Error> for PrimErr {
    fn from(_: std::io::Error) -> Self {
        Self::OSError
    }
}

/// What a primitive body returns.
///
/// `Ok` carries whatever the primitive answers (see [`crate::IntoReturn`]);
/// `Err` carries the failure code.
pub type PrimResult<T> = Result<T, PrimErr>;
