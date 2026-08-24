//! Replaces `src/common/sqHeapMap.c` on 64-bit builds.
//!
//! A one-bit-per-word map of the whole address space, used to check heap
//! integrity: the collector walks the heap setting a bit for every object
//! header, walks it again checking that every pointer lands on a set bit, and
//! can walk a third time clearing bits to find objects that should have been
//! collected. `CogObjectRepresentationForSpur` calls in through
//! `heapMapAtWord:`.
//!
//! # Scope
//!
//! 64-bit only. The C has a separate `SQ_IMAGE32` branch with a
//! single-level 256-entry table, which is selected by `SQ_VI_BYTES_PER_WORD`
//! and so cannot be reached from a 64-bit build at all. `cmake/rust.cmake`
//! keeps compiling the C when `SIZEOF_VOID_P` is 4.
//!
//! # Structure
//!
//! Two levels, sized as the C sized them. An address splits into
//!
//! ```text
//!   bits 63..45  directory index into mapPages   (NUMROOTPAGES entries)
//!   bits 44..26  page index within the directory (DIRECTORY_ENTRIES)
//!   bits 25..6   byte within the page
//!   bits  5..3   bit within the byte
//!   bits  2..0   must be zero -- a word address is 8-aligned
//! ```
//!
//! Both levels are allocated lazily and never freed, so a map that has seen a
//! given region keeps its 8 Mb page for the life of the process. That is the
//! C's design, not an oversight: the map is only built when heap checking is
//! switched on, and the same regions are revisited every cycle.
//!
//! # No locking
//!
//! Exactly as in the C. The map is written during a collection, with the VM
//! thread stopped, so there is no concurrent writer to protect against. If
//! that ever stops being true, the lazy allocation of a directory is the race
//! to worry about.
//!
//! # The leaf pages are eight times larger than they need to be
//!
//! `PAGESHIFT` is 26, so a page covers 2^26 bytes of address space; at one bit
//! per 8-byte word that is 2^26 / 8 / 8 = 1 Mb of bitmap. `HEAP_MAP_PAGE_SIZE`
//! is 8 Mb. The C's comment does the arithmetic as "8mb = 2^23, + 2^3 for
//! 64-bit units = 2^26" and drops the 8 bits per byte, so seven eighths of
//! every leaf page is allocated, zeroed by `clearHeapMap`, and never
//! addressed. Kept as it is: shrinking the page or widening the shift would
//! change how much memory a heap-checking build takes and how the map is
//! carved up, which is a behaviour change and not a port. It is only wasted
//! address space, never a correctness problem.
//!
//! # One divergence
//!
//! `DIRECTORYINDEX(a)` is `a >> 45`, which yields up to 19 bits, but
//! `mapPages` has only `NUMROOTPAGES` = 2^16 entries. Any address at or above
//! 2^61 therefore indexed past the end of the array -- a silent out-of-bounds
//! read, and an out-of-bounds *write* in `heapMapAtWordPut`. No such address
//! is reachable (the VM maps object memory far below it), so this has never
//! fired. Rather than reproduce it, the range is checked and a bad address
//! goes to `error`, which logs and aborts. Every address the C handled
//! correctly behaves identically.

use core::ffi::{c_int, c_void, CStr};
use core::sync::atomic::{AtomicPtr, Ordering};

/// Root table size, the C's `NUMROOTPAGES`.
const NUM_ROOT_PAGES: usize = 65536;

/// Bytes in one leaf page, the C's `HEAP_MAP_PAGE_SIZE`: 8 Mb, covering 2^26
/// bytes of address space at a bit per 8-byte word.
const HEAP_MAP_PAGE_SIZE: usize = 8 * 1024 * 1024;

/// Bytes in one directory, the C's `DIRECTORYSIZE`.
const DIRECTORY_SIZE: usize = (1 << 19) * core::mem::size_of::<*mut c_void>();

/// Pointer slots in one directory.
const DIRECTORY_ENTRIES: usize = DIRECTORY_SIZE / core::mem::size_of::<*mut c_void>();

const PAGE_SHIFT: u32 = 26;
const PAGE_MASK: usize = 0x3FF_FFFF;
const DIRECTORY_SHIFT: u32 = PAGE_SHIFT + 19;
const DIRECTORY_MASK: usize = 0x7_FFFF;
/// `log2` of the word size in bytes: word addresses are 8-aligned.
const LOG_WORD_SIZE: u32 = 3;
/// `log2` of the number of bits in a byte.
const LOG_BITS_PER_BYTE: u32 = 3;

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/common/sqHeapMap.c";

/// The root table. Entries are directories, themselves arrays of leaf pages.
///
/// The symbol is not exported -- the generated interpreter reaches the map
/// only through the three functions below -- so this need not be `static mut`.
/// `AtomicPtr` slots with relaxed ordering give the C's plain loads and stores
/// (there is no concurrent writer; see the module docs) with safe,
/// bounds-checked indexing at the root level.
static MAP_PAGES: [AtomicPtr<*mut u8>; NUM_ROOT_PAGES] = {
    // A `const` item, so the array-repeat initialiser is allowed to copy it.
    #[allow(clippy::declare_interior_mutable_const)]
    const NULL_DIRECTORY: AtomicPtr<*mut u8> = AtomicPtr::new(core::ptr::null_mut());
    [NULL_DIRECTORY; NUM_ROOT_PAGES]
};

/// Which directory in [`MAP_PAGES`] covers `address`.
#[inline]
const fn directory_index(address: usize) -> usize {
    address >> DIRECTORY_SHIFT
}

/// Which leaf page within its directory covers `address`.
#[inline]
const fn page_index(address: usize) -> usize {
    (address >> PAGE_SHIFT) & DIRECTORY_MASK
}

/// Which byte within its leaf page holds `address`'s bit.
#[inline]
const fn byte_index(address: usize) -> usize {
    (address & PAGE_MASK) >> (LOG_WORD_SIZE + LOG_BITS_PER_BYTE)
}

/// Which bit within its byte belongs to `address`.
#[inline]
const fn bit_mask(address: usize) -> u8 {
    1 << ((address >> LOG_WORD_SIZE) & ((1 << LOG_BITS_PER_BYTE) - 1))
}

/// Logs `message` and aborts. Never returns.
///
/// Under `cfg(test)` this panics instead, which keeps `src/debug.c` out of the
/// unit-test link. No test reaches it: the conditions that lead here are
/// checked through [`validate`], which is a pure function precisely so they
/// can be tested without ending the process.
fn fail(message: &'static CStr) -> ! {
    #[cfg(test)]
    panic!("{}", message.to_string_lossy());
    #[cfg(not(test))]
    // SAFETY: `error` takes a NUL-terminated string, logs it and calls abort().
    unsafe {
        pharo_vm_sys::error(message.as_ptr() as *mut _);
        // `error` is declared `void` in C but ends in abort(); if it ever
        // returns, continuing would be worse than stopping here.
        core::hint::unreachable_unchecked()
    }
}

/// Checks an address and answers its directory index, or the message to fail
/// with.
///
/// Split out from [`checked_directory_index`] so the two rejection cases can
/// be tested without aborting the process.
///
/// The alignment test is the C's. The range test is the divergence described
/// in the module docs: the C had none, and indexed past the end of the table.
#[inline]
fn validate(address: usize) -> Result<usize, &'static CStr> {
    if address & ((1 << LOG_WORD_SIZE) - 1) != 0 {
        return Err(c"misaligned word");
    }
    let index = directory_index(address);
    if index >= NUM_ROOT_PAGES {
        return Err(c"heapMap address out of range");
    }
    Ok(index)
}

/// [`validate`], failing loudly rather than returning an error.
#[inline]
fn checked_directory_index(address: usize) -> usize {
    match validate(address) {
        Ok(index) => index,
        Err(message) => fail(message),
    }
}

/// Allocates and zeroes `size` bytes, or logs and exits as the C did.
///
/// `alloc_zeroed` routes to `calloc`, so a fresh leaf page -- of which seven
/// eighths is never touched, see the module docs -- comes from lazily-zeroed
/// pages instead of being dirtied by an explicit memset. The blocks are never
/// freed (the C's design), so no layout bookkeeping for deallocation is kept.
fn alloc_zeroed_or_exit(size: usize, line: c_int) -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(size, core::mem::align_of::<*mut c_void>())
        .expect("both map levels have small, fixed sizes");
    // SAFETY: the layout has a non-zero size; the result is checked before use.
    let p = unsafe { std::alloc::alloc_zeroed(layout) };
    if p.is_null() {
        crate::logging::error_from_errno(
            c"heapMap malloc",
            crate::logging::site!(C_FILE, c"heapMapAtWordPut", line),
        );
        // SAFETY: exit runs atexit handlers and does not return.
        unsafe { libc::exit(1) };
    }
    p
}

/// Non-zero if the map has a bit set for `word_pointer`.
///
/// Answers 0 for any address whose directory or page has never been
/// allocated, which is how "not in the map" is represented.
///
/// # Safety
///
/// `word_pointer` is only ever used as an integer, never dereferenced, so any
/// value is accepted -- but it must be 8-aligned, or this logs and aborts.
#[no_mangle]
pub unsafe extern "C" fn heapMapAtWord(word_pointer: *mut c_void) -> c_int {
    let address = word_pointer as usize;
    let index = checked_directory_index(address);

    let directory = MAP_PAGES[index].load(Ordering::Relaxed);
    if directory.is_null() {
        return 0;
    }
    // SAFETY: every non-null directory points at DIRECTORY_ENTRIES slots,
    // every non-null page at HEAP_MAP_PAGE_SIZE bytes -- both guaranteed by
    // heapMapAtWordPut, the only writer.
    unsafe {
        let page = *directory.add(page_index(address));
        if page.is_null() {
            return 0;
        }
        c_int::from(*page.add(byte_index(address)) & bit_mask(address))
    }
}

/// Sets or clears the map's bit for `word_pointer`, allocating the directory
/// and page for it if this is the first time that region is touched.
///
/// # Safety
///
/// As [`heapMapAtWord`]. Not reentrant and not thread-safe: it may allocate.
#[no_mangle]
pub unsafe extern "C" fn heapMapAtWordPut(word_pointer: *mut c_void, bit: c_int) {
    let address = word_pointer as usize;
    let index = checked_directory_index(address);

    let mut directory = MAP_PAGES[index].load(Ordering::Relaxed);
    if directory.is_null() {
        directory = alloc_zeroed_or_exit(DIRECTORY_SIZE, 165).cast::<*mut u8>();
        MAP_PAGES[index].store(directory, Ordering::Relaxed);
    }

    // SAFETY: each allocation above and below is sized for the level it
    // serves, so the indexing stays inside it.
    unsafe {
        let page_slot = directory.add(page_index(address));
        let mut page = *page_slot;
        if page.is_null() {
            page = alloc_zeroed_or_exit(HEAP_MAP_PAGE_SIZE, 173);
            *page_slot = page;
        }

        let byte = page.add(byte_index(address));
        if bit != 0 {
            *byte |= bit_mask(address);
        } else {
            *byte &= !bit_mask(address);
        }
    }
}

/// Clears every bit in the map, keeping the pages allocated.
///
/// # Safety
///
/// Not thread-safe; see the module docs.
#[no_mangle]
pub unsafe extern "C" fn clearHeapMap() {
    for slot in &MAP_PAGES {
        let directory = slot.load(Ordering::Relaxed);
        if directory.is_null() {
            continue;
        }
        // SAFETY: walks only allocated directories and pages, each of the
        // size it was allocated with; nothing else touches the map while the
        // VM is stopped for a collection (see the module docs).
        let entries = unsafe { core::slice::from_raw_parts(directory, DIRECTORY_ENTRIES) };
        for &page in entries {
            if !page.is_null() {
                // SAFETY: as above.
                unsafe { core::slice::from_raw_parts_mut(page, HEAP_MAP_PAGE_SIZE) }.fill(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The map is one process-wide table, so tests that touch it run one at a
    /// time. The C had no lock either; this is for the harness.
    static MAP: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        MAP.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A word address in a region no other test uses.
    ///
    /// Kept small so that only one directory and one leaf page are ever
    /// allocated: a page is 8 Mb, and spraying addresses across the space
    /// would make the test suite allocate gigabytes.
    fn addr(word: usize) -> *mut c_void {
        (word * 8) as *mut c_void
    }

    fn get(p: *mut c_void) -> c_int {
        // SAFETY: the pointer is never dereferenced, only used as an integer.
        unsafe { heapMapAtWord(p) }
    }

    fn put(p: *mut c_void, bit: c_int) {
        // SAFETY: as above.
        unsafe { heapMapAtWordPut(p, bit) }
    }

    #[test]
    fn a_bit_survives_a_round_trip() {
        let _guard = lock();
        let p = addr(1000);

        put(p, 1);
        assert_ne!(get(p), 0);

        put(p, 0);
        assert_eq!(get(p), 0);
    }

    #[test]
    fn adjacent_words_use_different_bits_of_the_same_byte() {
        let _guard = lock();
        // Eight consecutive words share one byte of the map; setting one must
        // not disturb its neighbours. This is what BITINDEX/BYTEINDEX are for,
        // and getting the shift wrong would still round-trip a single bit.
        let base = 2000;
        for i in 0..8 {
            put(addr(base + i), 0);
        }
        put(addr(base + 3), 1);

        for i in 0..8 {
            let expected = i == 3;
            assert_eq!(
                get(addr(base + i)) != 0,
                expected,
                "word {i} of the byte should be {expected}"
            );
        }
    }

    #[test]
    fn an_address_in_an_unallocated_region_reads_as_zero() {
        let _guard = lock();
        // Never written, so neither its directory nor its page exists. The C
        // answered 0 for this rather than allocating on a read, and so must
        // this: a read that allocated would turn a leak check into a leak.
        let far = (1usize << 40) + 8 * 4096;
        assert_eq!(get(far as *mut c_void), 0);
    }

    #[test]
    fn clearing_resets_every_bit_but_keeps_the_map_usable() {
        let _guard = lock();
        let a = addr(3000);
        let b = addr(3001);
        put(a, 1);
        put(b, 1);
        assert_ne!(get(a), 0);
        assert_ne!(get(b), 0);

        // SAFETY: no other thread is using the map; the lock is held.
        unsafe { clearHeapMap() };

        assert_eq!(get(a), 0);
        assert_eq!(get(b), 0);

        put(a, 1);
        assert_ne!(get(a), 0);
    }

    #[test]
    fn the_index_split_matches_the_c_macros() {
        // Recomputed from the C's PAGESHIFT/PAGEMASK/DIRECTORY* macros rather
        // than from the Rust, so a typo in the constants shows up here.
        for address in [
            0usize,
            8,
            64,
            4096,
            1 << 26,
            (1 << 26) + 8,
            1 << 45,
            0x3600_0000,
        ] {
            assert_eq!(directory_index(address), address >> 45);
            assert_eq!(page_index(address), (address >> 26) & 0x7_FFFF);
            assert_eq!(byte_index(address), (address & 0x3FF_FFFF) >> 6);
            assert_eq!(bit_mask(address), 1u8 << ((address >> 3) & 7));
        }
    }

    #[test]
    fn only_an_eighth_of_each_page_is_ever_addressed() {
        // A page covers 2^PAGE_SHIFT bytes of address space at one bit per
        // 8-byte word, so it needs 2^26 / 8 / 8 = 1 Mb of bitmap. It is
        // allocated 8 Mb. See the over-allocation note in the module docs --
        // this test exists to keep that claim honest, and will fail if anyone
        // adjusts one of the two constants without the other.
        let bytes_addressable = (1usize << PAGE_SHIFT) >> (LOG_WORD_SIZE + LOG_BITS_PER_BYTE);
        assert_eq!(bytes_addressable, 1024 * 1024);
        assert_eq!(HEAP_MAP_PAGE_SIZE, 8 * bytes_addressable);

        // The largest byte index PAGE_MASK can produce must fit in a page.
        assert_eq!(byte_index(PAGE_MASK), bytes_addressable - 1);

        assert_eq!(DIRECTORY_ENTRIES, 1 << 19);
        assert_eq!(DIRECTORY_MASK, DIRECTORY_ENTRIES - 1);
    }

    #[test]
    fn misaligned_and_out_of_range_addresses_are_rejected() {
        // Both would have gone on to index the table in the C: the first with
        // a silently truncated address, the second past the end of `mapPages`.
        for offset in 1..8usize {
            assert_eq!(
                validate(0x1000 + offset).unwrap_err(),
                c"misaligned word",
                "offset {offset} is not 8-aligned"
            );
        }
        assert!(validate(0x1000).is_ok());

        // 2^61 is the first address whose `>> 45` exceeds the table's 2^16
        // entries. See the divergence note in the module docs.
        assert_eq!(
            validate(1usize << 61).unwrap_err(),
            c"heapMap address out of range"
        );
        assert!(validate((1usize << 61) - 8).is_ok());
    }
}
