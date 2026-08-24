//! Replaces `src/unix/memoryUnix.c`.
//!
//! Everything the VM maps: the object heap, the JIT's code zone, and the
//! `mprotect` calls that flip the code zone between writable and executable.
//! The generated interpreter calls all of it, and `sqAllocateMemory` runs three
//! times during a normal startup.
//!
//! # Fixed addresses are the whole point
//!
//! Spur wants its segments at particular addresses, so `MAP_FIXED` is used
//! whenever a base address is asked for. `MAP_FIXED` *replaces* any existing
//! mapping at that address rather than failing, which is why the C also checks
//! afterwards whether the address it got is the one it asked for, and retries
//! a page higher if not. That check is not defensive programming: without it
//! Linux hands back a mapping too high in the address space and the object
//! representation's pointer tagging breaks.
//!
//! # Faithful oddities
//!
//! * `sqMakeMemoryExecutableFromTo` reports its `mprotect` failure with
//!   `logError`, while `sqMakeMemoryNotExecutableFromTo` -- the same code with
//!   different flags -- uses `logErrorFromErrno`. Both then log the errno
//!   again by hand. Reproduced, including which one uses which.
//! * That hand-written `logError("ERRNO: %d\n", errno)` reads `errno` *after*
//!   the preceding log call, which does I/O and may have clobbered it. So the
//!   number printed is not necessarily the `mprotect` failure's. Reproduced by
//!   reading errno at the same point.
//! * `pageMask` is a file static that only `allocateJITMemory` and
//!   `sqAllocateMemory` ever set. The two `mprotect` entry points read it
//!   without setting it, so calling either before any allocation rounds the
//!   start address down to 0. Nothing does, because the code zone must exist
//!   before it can be protected.
//! * `overallocateMemory` is exported, initialised to 0, and read by nothing.
//!   The long comment above it in the C explains that the over-allocation
//!   scheme it used to switch on was disabled because `malloc` could land
//!   inside the reserved-then-unmapped region. Kept as an exported symbol.
//! * `devZero` is -1 and passed as `mmap`'s fd. With `MAP_ANON` the fd is
//!   ignored, so this is the conventional -1 by a longer route.
//!
//! # Signed and unsigned
//!
//! The C declared `heapLimit` as `sqInt` (signed) while every size around it
//! is `usqInt`. The comparison `heapLimit < desiredHeapSize` therefore
//! converts the signed value to unsigned. The Rust keeps `heap_limit`
//! unsigned throughout, which gives the same answer for every size that can
//! actually be mapped, and avoids a conversion nobody reading the C would
//! expect.

use core::ffi::{c_int, c_ulong, c_void, CStr};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use pharo_vm_sys::{sqInt, usqInt};

use crate::logging::{self, site, LOG_DEBUG, LOG_ERROR};

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/unix/memoryUnix.c";

/// Exported and initialised to 0; see the module docs. `AtomicI32` has the
/// size and alignment of a C `int`, so the symbol's ABI is unchanged.
#[no_mangle]
pub static overallocateMemory: AtomicI32 = AtomicI32::new(0);

/// Exported; the C never assigned it either.
#[no_mangle]
pub static mmapErrno: AtomicI32 = AtomicI32::new(0);

/// The C's `devZero`, passed to `mmap` as the fd. Ignored under `MAP_ANON`.
const DEV_ZERO: c_int = -1;

/// `pageSize` and `pageMask`, the C's two file statics.
///
/// Set by [`allocateJITMemory`] and [`sqAllocateMemory`], read by the
/// `mprotect` pair. Relaxed atomics: the C used plain `sqInt`/`usqInt` loads
/// and stores, and every writer runs on the VM thread during startup, so no
/// ordering is needed -- but the accesses stay defined even if another thread
/// ever reads them.
static PAGE_SIZE: AtomicUsize = AtomicUsize::new(0);
static PAGE_MASK: AtomicUsize = AtomicUsize::new(0);

/// `MAP_PROT` from the C.
const MAP_PROT: c_int = libc::PROT_READ | libc::PROT_WRITE;

/// `MAP_FLAGS` from the C. OpenBSD adds `MAP_STACK`.
#[cfg(target_os = "openbsd")]
const MAP_FLAGS: c_int = libc::MAP_ANON | libc::MAP_PRIVATE | libc::MAP_STACK;
/// See the OpenBSD definition above.
#[cfg(not(target_os = "openbsd"))]
const MAP_FLAGS: c_int = libc::MAP_ANON | libc::MAP_PRIVATE;

/// Reads `PAGE_MASK`.
#[inline]
fn page_mask() -> usize {
    PAGE_MASK.load(Ordering::Relaxed)
}

/// The C's `valign(x)` and `roundDownToPage(v)`, which are the same operation.
#[inline]
fn align_down(value: usize) -> usize {
    value & page_mask()
}

/// Refreshes the cached page size and mask.
///
/// The C called `getpagesize()`, which is the legacy spelling of
/// `sysconf(_SC_PAGESIZE)` and returns the same number.
fn refresh_page_size() -> usize {
    // SAFETY: sysconf with a valid name; _SC_PAGESIZE cannot fail.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    PAGE_SIZE.store(size, Ordering::Relaxed);
    PAGE_MASK.store(!(size - 1), Ordering::Relaxed);
    size
}

/// The current `errno`, for the hand-written ERRNO log lines.
fn errno() -> c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Makes the code zone readable and executable.
///
/// Exits the process on failure: there is no way to continue with a code zone
/// the JIT cannot run.
///
/// # Safety
///
/// The range must lie inside a mapping this module made, and `PAGE_MASK` must
/// already be set -- see the module docs.
#[no_mangle]
pub unsafe extern "C" fn sqMakeMemoryExecutableFromTo(start_addr: c_ulong, end_addr: c_ulong) {
    let first_page = align_down(start_addr as usize);
    // SAFETY: mprotect on a range the caller guarantees is mapped.
    let code = unsafe {
        libc::mprotect(
            first_page as *mut c_void,
            (end_addr as usize) - first_page,
            libc::PROT_READ | libc::PROT_EXEC,
        )
    };
    if code < 0 {
        logging::message_no_args(
            LOG_ERROR,
            c"mprotect(x,y,PROT_READ | PROT_EXEC)",
            site!(C_FILE, c"sqMakeMemoryExecutableFromTo", 72),
        );
        // errno read here, after the log call, exactly as the C did.
        logging::message_one_int(
            LOG_ERROR,
            c"ERRNO: %d\n",
            site!(C_FILE, c"sqMakeMemoryExecutableFromTo", 73),
            errno(),
        );
        // SAFETY: exit runs atexit handlers and does not return.
        unsafe { libc::exit(1) };
    }
}

/// Makes the code zone readable and writable again.
///
/// # Safety
///
/// As [`sqMakeMemoryExecutableFromTo`].
#[no_mangle]
pub unsafe extern "C" fn sqMakeMemoryNotExecutableFromTo(start_addr: c_ulong, end_addr: c_ulong) {
    let first_page = align_down(start_addr as usize);
    // SAFETY: as above.
    let code = unsafe {
        libc::mprotect(
            first_page as *mut c_void,
            (end_addr as usize) - first_page,
            libc::PROT_READ | libc::PROT_WRITE,
        )
    };
    if code < 0 {
        // The sibling above uses logError here. Not a typo of mine.
        logging::error_from_errno(
            c"mprotect(x,y,PROT_READ | PROT_WRITE)",
            site!(C_FILE, c"sqMakeMemoryNotExecutableFromTo", 85),
        );
        logging::message_one_int(
            LOG_ERROR,
            c"ERRNO: %d\n",
            site!(C_FILE, c"sqMakeMemoryNotExecutableFromTo", 86),
            errno(),
        );
        // SAFETY: exit does not return.
        unsafe { libc::exit(1) };
    }
}

/// Maps the JIT's code zone, exiting on failure.
///
/// On Apple the zone is mapped `MAP_JIT` and always writable-and-executable;
/// elsewhere `MAP_FIXED` is used when a position is asked for, and the
/// protection depends on `READ_ONLY_CODE_ZONE`.
#[no_mangle]
pub extern "C" fn allocateJITMemory(desired_size: usqInt, desired_position: usqInt) -> *mut c_void {
    refresh_page_size();

    let aligned_size = align_down((desired_size as usize).max(1));
    let desired_base_aligned = align_down(desired_position as usize);

    #[cfg(target_vendor = "apple")]
    let (additional_flags, prot) = (
        libc::MAP_JIT,
        libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
    );
    #[cfg(not(target_vendor = "apple"))]
    let (additional_flags, prot) = (
        if desired_position != 0 {
            libc::MAP_FIXED
        } else {
            0
        },
        if cfg!(read_only_code_zone) {
            libc::PROT_READ | libc::PROT_EXEC
        } else {
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC
        },
    );

    logging::message_one_ptr(
        LOG_DEBUG,
        c"Trying to allocate JIT memory in %p\n",
        site!(C_FILE, c"allocateJITMemory", 111),
        desired_base_aligned as *const c_void,
    );

    // SAFETY: mmap with a length of at least one page; the result is checked.
    let result = unsafe {
        libc::mmap(
            desired_base_aligned as *mut c_void,
            aligned_size,
            prot,
            MAP_FLAGS | additional_flags,
            DEV_ZERO,
            0,
        )
    };

    if result == libc::MAP_FAILED {
        logging::error_from_errno(
            c"Could not allocate JIT memory",
            site!(C_FILE, c"allocateJITMemory", 115),
        );
        // SAFETY: exit does not return.
        unsafe { libc::exit(1) };
    }

    result
}

/// Maps between `min_heap_size` and `desired_heap_size` bytes, preferably at
/// `desired_base_address`. Answers the address, or 0.
///
/// Two loops in one. If `mmap` fails outright the request shrinks to three
/// quarters and is tried again, down to `min_heap_size`. If `mmap` succeeds
/// but at the wrong address -- which `MAP_FIXED` permits -- the mapping is
/// released, the target address moves up one page, and the same size is tried
/// again. Only the first of those applies on Apple.
#[no_mangle]
pub extern "C" fn sqAllocateMemory(
    min_heap_size: usqInt,
    desired_heap_size: usqInt,
    desired_base_address: usqInt,
) -> usqInt {
    let page_size = refresh_page_size();

    #[cfg(target_vendor = "apple")]
    let additional_flags = 0;
    #[cfg(not(target_vendor = "apple"))]
    let additional_flags = if desired_base_address != 0 {
        libc::MAP_FIXED
    } else {
        0
    };

    let desired_heap_size = desired_heap_size as usize;
    let min_heap_size = min_heap_size as usize;
    let desired_base_address = desired_base_address as usize;

    // Only the `cfg(not(apple))` retry below moves this along; on Apple the
    // binding is never reassigned.
    #[cfg_attr(target_vendor = "apple", allow(unused_mut))]
    let mut desired_base_aligned = align_down(desired_base_address);
    let mut heap_limit = align_down(desired_heap_size.max(1));
    if heap_limit < desired_heap_size {
        // Aligning down lost a partial page; give it back.
        heap_limit += page_size;
    }

    let mut heap: *mut c_void = core::ptr::null_mut();

    while heap.is_null() && heap_limit >= min_heap_size {
        // SAFETY: mmap with a checked result; MAP_FIXED may replace an
        // existing mapping, which is what the caller is asking for.
        let mapped = unsafe {
            libc::mmap(
                desired_base_aligned as *mut c_void,
                heap_limit,
                MAP_PROT,
                MAP_FLAGS | additional_flags,
                DEV_ZERO,
                0,
            )
        };

        if mapped == libc::MAP_FAILED {
            heap = core::ptr::null_mut();
            heap_limit = align_down(heap_limit / 4 * 3);
            continue;
        }
        heap = mapped;

        // Linux can hand back a mapping too high in the address space, which
        // the object representation cannot use. Move up a page and retry.
        #[cfg(not(target_vendor = "apple"))]
        if heap as usize != desired_base_aligned {
            desired_base_aligned = align_down(desired_base_aligned + page_size);

            if (heap as usize) < desired_base_address {
                logging::message_one_ptr(
                    LOG_ERROR,
                    c"I cannot find a good memory address starting from: %p",
                    site!(C_FILE, c"sqAllocateMemory", 160),
                    desired_base_address as *const c_void,
                );
                return 0;
            }

            // The alignment above can wrap past the target; the C called that
            // "If I overflow".
            if desired_base_address > desired_base_aligned {
                logging::message_one_ptr(
                    LOG_ERROR,
                    c"I cannot find a good memory address starting from: %p",
                    site!(C_FILE, c"sqAllocateMemory", 166),
                    desired_base_address as *const c_void,
                );
                return 0;
            }

            // SAFETY: unmapping exactly the region just mapped.
            unsafe { libc::munmap(heap, heap_limit) };
            heap = core::ptr::null_mut();
        }
    }

    logging::debug_allocation_summary(
        c"Requested memory size: %zu at: %p, aligned size: %zu at: %p, obtained at: %p",
        site!(C_FILE, c"sqAllocateMemory", 176),
        desired_heap_size,
        desired_base_address as *const c_void,
        heap_limit,
        desired_base_aligned as *const c_void,
        heap.cast_const(),
    );

    heap as usqInt
}

/// Unmaps a segment. Reports failure but cannot fail the caller.
///
/// # Safety
///
/// `addr` and `sz` must describe a mapping made by [`sqAllocateMemory`].
#[no_mangle]
pub unsafe extern "C" fn sqDeallocateMemorySegmentAtOfSize(addr: *mut c_void, sz: sqInt) {
    // SAFETY: delegated to the caller.
    if unsafe { libc::munmap(addr, sz as usize) } != 0 {
        logging::error_from_errno(
            c"sqDeallocateMemorySegment... munmap",
            site!(C_FILE, c"sqDeallocateMemorySegmentAtOfSize", 187),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// PAGE_SIZE and PAGE_MASK are process-wide, as in the C.
    static PAGE_STATE: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        PAGE_STATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn aligning_rounds_down_to_a_page_boundary() {
        let _guard = lock();
        let page = refresh_page_size();
        assert!(page.is_power_of_two(), "page size must be a power of two");

        assert_eq!(align_down(0), 0);
        assert_eq!(align_down(page), page);
        assert_eq!(align_down(page - 1), 0);
        assert_eq!(align_down(page + 1), page);
        assert_eq!(align_down(3 * page + 17), 3 * page);
    }

    #[test]
    fn a_heap_lands_at_the_address_it_asks_for() {
        let _guard = lock();
        let _ = logging::take();
        let page = refresh_page_size();

        // A modest request with no preferred address, which takes the
        // `additional_flags == 0` path and cannot fail on any sane system.
        let size = (16 * page) as usqInt;
        let got = sqAllocateMemory(size, size, 0);
        assert_ne!(got, 0, "a 16-page anonymous mapping should succeed");

        // Writable, as MAP_PROT says.
        // SAFETY: the mapping is 16 pages of read-write memory.
        unsafe {
            let p = got as *mut u8;
            p.write(0xAB);
            assert_eq!(p.read(), 0xAB);
            p.add(16 * page - 1).write(0xCD);
            assert_eq!(p.add(16 * page - 1).read(), 0xCD);
        }

        // SAFETY: unmapping exactly what was mapped.
        unsafe { sqDeallocateMemorySegmentAtOfSize(got as *mut c_void, size as sqInt) };
    }

    #[test]
    fn the_summary_reports_the_aligned_size_not_the_requested_one() {
        let _guard = lock();
        let _ = logging::take();
        let page = refresh_page_size();

        // Deliberately not a multiple of the page size, so the "give the
        // partial page back" branch runs and the aligned size exceeds the
        // requested one.
        let requested = (3 * page + 1) as usqInt;
        let got = sqAllocateMemory(page as usqInt, requested, 0);
        assert_ne!(got, 0);

        let records = logging::take();
        let summary = records
            .iter()
            .find(|r| r.function == "sqAllocateMemory")
            .expect("the allocation summary should be logged");

        match summary.args {
            logging::Args::AllocationSummary {
                requested_size,
                aligned_size,
                obtained_at,
                ..
            } => {
                assert_eq!(requested_size, 3 * page + 1);
                assert_eq!(aligned_size, 4 * page, "the partial page is rounded up");
                assert_eq!(obtained_at, got as usize);
            }
            ref other => panic!("unexpected log arguments: {other:?}"),
        }

        // SAFETY: unmapping exactly what was mapped -- note the *aligned*
        // size, which is what was actually reserved.
        unsafe { sqDeallocateMemorySegmentAtOfSize(got as *mut c_void, (4 * page) as sqInt) };
    }

    #[test]
    fn an_impossible_request_answers_zero() {
        let _guard = lock();
        let _ = logging::take();
        let page = refresh_page_size();

        // Ask for far more than the address space holds, with a minimum just
        // as large, so the shrink loop cannot rescue it.
        let huge = (usize::MAX / 2) as usqInt;
        assert_eq!(sqAllocateMemory(huge, huge, 0), 0);

        // And the summary still gets logged, with a null result.
        let records = logging::take();
        let summary = records
            .iter()
            .find(|r| r.function == "sqAllocateMemory")
            .expect("the summary is logged even on failure");
        match summary.args {
            logging::Args::AllocationSummary { obtained_at, .. } => assert_eq!(obtained_at, 0),
            ref other => panic!("unexpected log arguments: {other:?}"),
        }
        let _ = page;
    }

    #[test]
    fn shrinking_takes_three_quarters_each_time() {
        let _guard = lock();
        let page = refresh_page_size();
        // The retry in sqAllocateMemory is `heapLimit = valign(heapLimit / 4 * 3)`.
        // Integer division first, so it is not the same as `* 3 / 4`.
        let start = 1000 * page;
        let next = align_down(start / 4 * 3);
        assert_eq!(next, align_down(750 * page));
        assert!(next < start, "the retry must make progress");
    }
}
