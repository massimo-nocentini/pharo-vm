//! Pins the SQFile record layout to what the C compiler produced.
//!
//! The record lives inside an image-side ByteArray sized by
//! `primitiveFileOpen`, and other plugins (UnixOSProcessPlugin,
//! FileAttributesPlugin) reach into those bytes through the same struct
//! definition, so every offset is ABI.

use core::mem::{align_of, offset_of, size_of};

use libc::c_void;
use FilePlugin::sqfile::SQFile;

/// The C layout, spelled out:
///
/// ```c
/// typedef struct {
///   int   sessionID;   /* offset 0 */
///   void *file;        /* offset = pointer alignment rounds 4 up */
///   char  writable;
///   char  lastOp;
///   char  lastChar;
///   char  isStdioStream;
/// } SQFile;
/// ```
#[test]
fn record_matches_the_c_struct() {
    let ptr = size_of::<*mut c_void>();
    let ptr_align = align_of::<*mut c_void>();
    let file_offset = 4_usize.div_ceil(ptr_align) * ptr_align;
    let chars_offset = file_offset + ptr;

    assert_eq!(offset_of!(SQFile, sessionID), 0, "ikp: must be first");
    assert_eq!(offset_of!(SQFile, file), file_offset);
    assert_eq!(offset_of!(SQFile, writable), chars_offset);
    assert_eq!(offset_of!(SQFile, lastOp), chars_offset + 1);
    assert_eq!(offset_of!(SQFile, lastChar), chars_offset + 2);
    assert_eq!(offset_of!(SQFile, isStdioStream), chars_offset + 3);

    // Trailing padding rounds the size up to the struct's alignment.
    let align = align_of::<SQFile>();
    let expect_size = (chars_offset + 4).div_ceil(align) * align;
    assert_eq!(size_of::<SQFile>(), expect_size);
    assert_eq!(FilePlugin::fileRecordSize(), size_of::<SQFile>());
}

/// The concrete numbers on the platforms the VM ships on, as a second
/// witness independent of the formula above.
#[test]
#[cfg(target_pointer_width = "64")]
fn record_is_24_bytes_on_64_bit() {
    assert_eq!(size_of::<SQFile>(), 24);
    assert_eq!(offset_of!(SQFile, file), 8);
    assert_eq!(offset_of!(SQFile, writable), 16);
}

#[test]
#[cfg(target_pointer_width = "32")]
fn record_is_12_bytes_on_32_bit() {
    assert_eq!(size_of::<SQFile>(), 12);
    assert_eq!(offset_of!(SQFile, file), 4);
    assert_eq!(offset_of!(SQFile, writable), 8);
}
