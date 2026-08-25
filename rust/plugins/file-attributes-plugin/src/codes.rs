//! The plugin's status protocol, verbatim from `faConstants.h`.
//!
//! The image maps these to exceptions (its `FileAttributesPluginPrims`
//! decodes the OS-error value a failed primitive recorded), so the numbers
//! are ABI: they must not be renumbered or "improved". The full table is kept
//! even where this port expresses an outcome through types instead (e.g.
//! [`FA_NO_MORE_DATA`] became an enum variant), so a reader can diff it
//! against the C header line by line.

// The unreferenced codes stay for parity with faConstants.h; see module docs.
#![allow(dead_code)]

/// Operation completed.
pub const FA_SUCCESS: i64 = 0;

/// A path or file name did not fit the plugin's fixed-size buffers.
pub const FA_STRING_TOO_LONG: i64 = -1;
/// Unused by the Unix code paths (the C reports [`FA_CANT_STAT_PATH`]).
pub const FA_STAT_FAILED: i64 = -2;
/// `stat()`/`lstat()` failed on the supplied path.
pub const FA_CANT_STAT_PATH: i64 = -3;
/// Windows support layer only.
pub const FA_GET_ATTRIBUTES_FAILED: i64 = -4;
/// Windows support layer only.
pub const FA_TIME_CONVERSION_FAILED: i64 = -5;
/// Neither stats nor access attributes were requested.
pub const FA_INVALID_ARGUMENTS: i64 = -6;
/// A directory session no longer holds an open stream.
pub const FA_CORRUPT_VALUE: i64 = -7;
/// `readlink()` failed. Only reachable through the generated
/// `readLinkintomaxLength`, which nothing calls.
pub const FA_CANT_READ_LINK: i64 = -8;
/// `opendir()` failed.
pub const FA_CANT_OPEN_DIR: i64 = -9;
/// `calloc` of the directory session failed (C only; Rust allocation aborts).
pub const FA_CANT_ALLOCATE_MEMORY: i64 = -10;
/// Reserved in `faConstants.h`; never raised.
pub const FA_INVALID_REQUEST: i64 = -11;
/// `closedir()` failed.
pub const FA_UNABLE_TO_CLOSE_DIR: i64 = -12;
/// The build has no chmod/chown (the Windows C build); unreachable on Unix.
pub const FA_UNSUPPORTED_OPERATION: i64 = -13;
/// "It shouldn't be possible to get here."
pub const FA_UNEXPECTED_ERROR: i64 = -14;
/// The real error was flagged in the interpreter proxy.
pub const FA_INTERPRETER_ERROR: i64 = -15;
/// `readdir()` failed.
pub const FA_CANT_READ_DIR: i64 = -16;
/// The session stamp in a directory handle does not match this VM run.
pub const FA_BAD_SESSION_ID: i64 = -17;

/// End of a directory stream. The one positive status.
pub const FA_NO_MORE_DATA: i64 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned against `faConstants.h`: the image decodes these numbers.
    #[test]
    fn codes_match_fa_constants_h() {
        assert_eq!(FA_SUCCESS, 0);
        assert_eq!(FA_STRING_TOO_LONG, -1);
        assert_eq!(FA_STAT_FAILED, -2);
        assert_eq!(FA_CANT_STAT_PATH, -3);
        assert_eq!(FA_GET_ATTRIBUTES_FAILED, -4);
        assert_eq!(FA_TIME_CONVERSION_FAILED, -5);
        assert_eq!(FA_INVALID_ARGUMENTS, -6);
        assert_eq!(FA_CORRUPT_VALUE, -7);
        assert_eq!(FA_CANT_READ_LINK, -8);
        assert_eq!(FA_CANT_OPEN_DIR, -9);
        assert_eq!(FA_CANT_ALLOCATE_MEMORY, -10);
        assert_eq!(FA_INVALID_REQUEST, -11);
        assert_eq!(FA_UNABLE_TO_CLOSE_DIR, -12);
        assert_eq!(FA_UNSUPPORTED_OPERATION, -13);
        assert_eq!(FA_UNEXPECTED_ERROR, -14);
        assert_eq!(FA_INTERPRETER_ERROR, -15);
        assert_eq!(FA_CANT_READ_DIR, -16);
        assert_eq!(FA_BAD_SESSION_ID, -17);
        assert_eq!(FA_NO_MORE_DATA, 1);
    }
}
