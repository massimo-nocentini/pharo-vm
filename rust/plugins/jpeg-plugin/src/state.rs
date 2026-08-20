//! The blob the image passes back and forth in place of libjpeg's struct.
//!
//! # Why this is not libjpeg's struct
//!
//! The C plugin hands the image a `ByteArray` sized to
//! `sizeof(struct jpeg_decompress_struct)` and casts it back on every call.
//! That struct is full of pointers, including one stored *into a different
//! ByteArray* (`pcinfo->err = jpeg_std_error(&pjerr->pub)`), and it stays live
//! across two separate primitive calls. Nothing pins those arrays, so a
//! garbage collection between `primJPEGReadHeader...` and
//! `primJPEGReadImage...` would leave the struct pointing at where the error
//! record used to be.
//!
//! Rather than reproduce that, this port keeps the blob **plain old data**:
//! just the header fields, no pointers, nothing to free. `readImage` re-parses
//! the JPEG header from the source bytes, which costs microseconds and cannot
//! dangle. The image cannot tell the difference -- it only ever asks the
//! plugin how big the blob should be and hands the same blob back
//! (`isValidDecompressionStruct:` checks nothing but the size).

use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut};

/// Identifies our blob, so a stale or foreign one is rejected rather than
/// misread. "RJPG" in ASCII.
const MAGIC: u32 = 0x524A_5047;

/// Bumped if the layout below ever changes meaning.
const VERSION: u32 = 1;

/// Decompression state: what `readHeader` learned, for `readImage` to use.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decompress {
    magic: u32,
    version: u32,
    /// Image width in pixels; 0 when no header has been read successfully.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Components in the *source* image, which is what
    /// `primImageNumComponents` answers.
    pub num_components: u32,
    /// Components the decoder will emit per pixel: 3 for colour, 1 for
    /// grayscale. This, not `num_components`, drives the pixel packing.
    pub out_components: u32,
    /// Reserved so the blob size can stay stable while fields are added.
    _reserved: [u32; 10],
}

impl Decompress {
    /// An empty blob: the state after a failed or absent header read.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            width: 0,
            height: 0,
            num_components: 0,
            out_components: 0,
            _reserved: [0; 10],
        }
    }

    /// Builds the state for a successfully parsed header.
    #[must_use]
    pub const fn new(width: u32, height: u32, num_components: u32, out_components: u32) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            width,
            height,
            num_components,
            out_components,
            _reserved: [0; 10],
        }
    }

    /// Reads a blob the image handed back.
    ///
    /// Returns `None` for anything that is not one of ours -- a blob from a
    /// different plugin build, or one the image never filled in.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < size_of::<Self>() {
            return None;
        }
        let mut state = Self::empty();
        // SAFETY: `state` is POD and we have just checked `bytes` is at least
        // as long as it. Copied bytewise because the image gives no alignment
        // guarantee beyond the object header.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                addr_of_mut!(state).cast::<u8>(),
                size_of::<Self>(),
            );
        }
        (state.magic == MAGIC && state.version == VERSION).then_some(state)
    }

    /// The blob's bytes, for writing back into the image's ByteArray.
    #[must_use]
    pub fn to_bytes(self) -> [u8; size_of::<Self>()] {
        let mut out = [0u8; size_of::<Self>()];
        // SAFETY: `Self` is POD with no padding that matters; copying its
        // representation out is well defined.
        unsafe {
            core::ptr::copy_nonoverlapping(
                addr_of!(self).cast::<u8>(),
                out.as_mut_ptr(),
                size_of::<Self>(),
            );
        }
        out
    }

    /// Size the image should allocate for a decompression blob.
    #[must_use]
    pub const fn blob_size() -> usize {
        size_of::<Self>()
    }
}

/// The error record.
///
/// In C this held libjpeg's `jpeg_error_mgr` plus a `jmp_buf*`, because errors
/// came back through `longjmp`. Rust returns `Result`, so there is nothing to
/// keep -- but the image still allocates one and passes it in, and
/// `isValidErrorMessageStruct:` still checks its size, so the shape stays.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ErrorMgr {
    magic: u32,
    version: u32,
    /// Nonzero if the last operation using this record failed.
    pub failed: u32,
    _reserved: [u32; 5],
}

impl ErrorMgr {
    /// Size the image should allocate for an error record.
    #[must_use]
    pub const fn blob_size() -> usize {
        size_of::<Self>()
    }

    /// A record marking success or failure.
    #[must_use]
    pub const fn new(failed: bool) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            failed: failed as u32,
            _reserved: [0; 5],
        }
    }

    /// The record's bytes.
    #[must_use]
    pub fn to_bytes(self) -> [u8; size_of::<Self>()] {
        let mut out = [0u8; size_of::<Self>()];
        // SAFETY: POD, as above.
        unsafe {
            core::ptr::copy_nonoverlapping(
                addr_of!(self).cast::<u8>(),
                out.as_mut_ptr(),
                size_of::<Self>(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_bytes() {
        let s = Decompress::new(640, 480, 3, 3);
        let bytes = s.to_bytes();
        assert_eq!(Decompress::from_bytes(&bytes), Some(s));
    }

    #[test]
    fn rejects_a_blob_that_is_not_ours() {
        assert_eq!(Decompress::from_bytes(&[0u8; 64]), None);
    }

    #[test]
    fn rejects_a_short_blob() {
        let bytes = Decompress::new(1, 1, 1, 1).to_bytes();
        assert_eq!(Decompress::from_bytes(&bytes[..8]), None);
    }

    #[test]
    fn an_empty_blob_reports_zero_width() {
        let bytes = Decompress::empty().to_bytes();
        let s = Decompress::from_bytes(&bytes).expect("still ours");
        assert_eq!(s.width, 0);
        assert_eq!(s.height, 0);
    }

    /// The image allocates exactly what we ask for, so the two must agree.
    #[test]
    fn blob_sizes_match_the_structs() {
        assert_eq!(Decompress::blob_size(), size_of::<Decompress>());
        assert_eq!(ErrorMgr::blob_size(), size_of::<ErrorMgr>());
    }
}
