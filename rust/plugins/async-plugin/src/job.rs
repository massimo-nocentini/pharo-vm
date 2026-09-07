//! What a worker actually does, and the whole of what a worker is allowed to do.
//!
//! A `Job` owns everything it needs before it leaves the interpreter thread --
//! an owned `PathBuf`, an owned `Vec<u8>` -- because a foreign thread may not
//! read an oop. The copy out of image memory happens in the primitive, on the
//! VM thread, where it is safe; the job carries the copy.
//!
//! That is a real cost and it is the right one: a 200 MB write is copied once
//! and then the image is free for the length of the write, instead of frozen
//! for it. The alternative -- pinning the ByteArray and letting the worker read
//! image memory directly -- is not available, because the GC moves objects and
//! a worker cannot participate in the interpreter's pinning protocol.

use std::path::PathBuf;
use std::time::Duration;

use crate::task::{Outcome, Task, ERR_TOO_LARGE, ERR_UNKNOWN};

/// The largest file [`Job::ReadFile`] will read into memory.
///
/// A guard rather than a policy: the image asks for a path and gets a
/// ByteArray, so an unbounded read is an image-reachable allocation of
/// arbitrary size, and running the VM out of memory from a worker thread is not
/// a failure it can report. 256 MiB is past any configuration file and short of
/// anything that should be streamed instead.
pub const MAX_READ_BYTES: u64 = 256 * 1024 * 1024;

/// One unit of blocking work.
#[derive(Debug)]
pub enum Job {
    /// Read a whole file. Answers its bytes.
    ReadFile { path: PathBuf },
    /// Write a whole file, replacing it. Answers the byte count.
    WriteFile { path: PathBuf, contents: Vec<u8> },
    /// Occupy a worker for a while. Answers nothing.
    ///
    /// Diagnostic rather than useful: `AioPlugin`'s timers are better at
    /// waiting, because they wait in the VM's own poll loop and occupy no
    /// thread at all. This exists so that a test can hold a worker busy for a
    /// known length of time, which is what proves the pool is a pool.
    Sleep { millis: u64 },
}

impl Job {
    /// Runs the job and records its answer on `task`.
    ///
    /// No lock of this plugin's is held while this runs -- that is the point of
    /// the pool -- and nothing here touches the task registry or the
    /// interpreter.
    pub fn run(self, task: &Task) {
        if task.is_cancelled() {
            task.abandon();
            return;
        }
        match self {
            Job::ReadFile { path } => match read_bounded(&path) {
                Ok(bytes) => task.finish(Outcome::Bytes(bytes)),
                Err(code) => task.fail(code),
            },
            Job::WriteFile { path, contents } => {
                let len = contents.len();
                match std::fs::write(&path, contents) {
                    Ok(()) => task.finish(Outcome::Count(len)),
                    Err(e) => task.fail(errno_of(&e)),
                }
            }
            Job::Sleep { millis } => {
                // In slices, so that a cancelled sleep stops being a job the
                // pool is waiting on sooner than its full duration -- and so
                // that `stop()` is not held up by one.
                let mut left = millis;
                while left > 0 {
                    if task.is_cancelled() {
                        task.abandon();
                        return;
                    }
                    let slice = left.min(20);
                    std::thread::sleep(Duration::from_millis(slice));
                    left -= slice;
                }
                task.finish(Outcome::Nothing);
            }
        }
    }
}

/// `std::fs::read`, refusing a file past [`MAX_READ_BYTES`] before allocating
/// for it.
///
/// The size is checked by `stat` and then the read is bounded again by
/// `take`, because the two can disagree: a file can grow between the two calls,
/// and on Linux a `/proc` entry reports a size of zero and then yields content.
fn read_bounded(path: &std::path::Path) -> Result<Vec<u8>, i32> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| errno_of(&e))?;
    if let Ok(metadata) = file.metadata() {
        if metadata.len() > MAX_READ_BYTES {
            return Err(ERR_TOO_LARGE);
        }
    }
    let mut bytes = Vec::new();
    let read = file
        .take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| errno_of(&e))?;
    if read as u64 > MAX_READ_BYTES {
        return Err(ERR_TOO_LARGE);
    }
    Ok(bytes)
}

/// The OS error number, or [`ERR_UNKNOWN`] for an error that carries none.
fn errno_of(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(ERR_UNKNOWN)
}
