//! Replaces `src/externalPrimitives.c` on Unix.
//!
//! Loading a plugin means finding a shared object by name. The VM does not
//! know the file name, only the module name (`FilePlugin`), so it tries every
//! combination of a search path and a naming convention until `dlopen`
//! succeeds. That search, plus the accessor-depth convention Slang relies on,
//! is all this file is.
//!
//! # Scope
//!
//! Unix only. The `_WIN32` half uses `LoadLibraryW` and `GetProcAddress`, has
//! a fallback that reaches into `PharoVMCore.dll` by name, and formats error
//! messages through `formatMessageFromErrorCode`; none of that is testable
//! here, so `cmake/rust.cmake` keeps compiling the C on Windows. See the note
//! there.
//!
//! # Faithful oddities
//!
//! * `freeModuleHandle` returns 0 for success on Unix and 1 for success on
//!   Windows -- the two branches of the same function disagree. Only the Unix
//!   convention is reproduced here, because only the Unix branch is ported;
//!   the Windows C is untouched and keeps its own.
//! * `getModuleSymbol(NULL, sym)` passes `dlopen(NULL, 0)` as the handle. Mode
//!   0 names neither `RTLD_LAZY` nor `RTLD_NOW`, so glibc rejects it and
//!   returns null -- which then works anyway, because glibc's `RTLD_DEFAULT`
//!   *is* the null pointer, so `dlsym(NULL, sym)` searches the global scope.
//!   On macOS `RTLD_DEFAULT` is `(void *)-2` and the same code searches
//!   nothing. Reproduced exactly, including the failed `dlopen`, because the
//!   Linux behaviour the VM depends on is an accident of that null.
//! * `moduleNameBuffer` is a single process-wide `char[FILENAME_MAX]` that
//!   every load attempt overwrites, and it is an exported symbol rather than a
//!   local. Kept, because it is exported: something outside this file could be
//!   reading it.
//! * The C's third `#else` branch, for platforms that are neither Windows nor
//!   Unix, defines `getModuleSymbol` twice and would not compile. Not ported,
//!   since nothing can have been building it.
//!
//! # One divergence, and it is a fix
//!
//! `ioFindExternalFunctionInAccessorDepthInto` did `strcpy(buf, lookupName)`
//! into a `char buf[256]` with no length check, then appended
//! `"AccessorDepth"`. A selector of 256 bytes or more smashed the stack. The
//! Rust truncates to the buffer instead. For every input the C handled
//! correctly the result is identical; for the inputs that overflowed there is
//! no behaviour worth preserving.

use core::ffi::{c_char, c_int, c_void, CStr};

use pharo_vm_sys::{sqInt, FILENAME_MAX};

use crate::logging::{self, site, LOG_DEBUG, LOG_TRACE};

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/externalPrimitives.c";

/// The size of [`moduleNameBuffer`], taken from the same `stdio.h` the C saw.
const NAME_BUFFER_LEN: usize = FILENAME_MAX as usize;

/// Size of the accessor-depth scratch buffer, the C's `char buf[256]`.
const ACCESSOR_DEPTH_BUF_LEN: usize = 256;

/// Set from the command line to trace module loading. Nothing in this file
/// reads it; it is exported because the C exported it.
#[no_mangle]
pub static mut sqVMOptionTraceModuleLoading: c_int = 0;

/// The naming conventions tried for each search path, in order, terminated by
/// a null as the C's loop expects.
///
/// These are `snprintf` format strings taking the path and then the module
/// name, so `"%slib%s.so"` turns `("/usr/lib/", "FilePlugin")` into
/// `/usr/lib/libFilePlugin.so`. The bare `"%s%s"` first entry is what lets a
/// caller pass a full file name through unchanged.
///
/// On Apple that bare entry appears twice: the C emitted one before the
/// `#if` and the `__APPLE__` arm opened with another. The second is a wasted
/// `dlopen` attempt on a name that already failed, and it is kept, because
/// removing it changes how many times the loader is asked and what a trace log
/// shows.
#[cfg(target_vendor = "apple")]
#[no_mangle]
pub static mut moduleNamePatterns: [*const c_char; 5] = [
    c"%s%s".as_ptr(),
    c"%s%s".as_ptr(),
    c"%s%s.dylib".as_ptr(),
    c"%slib%s.dylib".as_ptr(),
    core::ptr::null(),
];

/// See the Apple definition above.
#[cfg(not(target_vendor = "apple"))]
#[no_mangle]
pub static mut moduleNamePatterns: [*const c_char; 4] = [
    c"%s%s".as_ptr(),
    c"%s%s.so".as_ptr(),
    c"%slib%s.so".as_ptr(),
    core::ptr::null(),
];

/// How many slots [`moduleNamePatterns`] has, including its null terminator.
///
/// A constant rather than `moduleNamePatterns.len()`, because taking `len()`
/// of a `static mut` forms a reference to it.
#[cfg(target_vendor = "apple")]
const PATTERN_SLOTS: usize = 5;
/// See the Apple definition above.
#[cfg(not(target_vendor = "apple"))]
const PATTERN_SLOTS: usize = 4;

/// Scratch space for the file name currently being tried.
///
/// Process-wide and overwritten by every attempt, exactly as in the C. See the
/// module docs.
#[no_mangle]
pub static mut moduleNameBuffer: [c_char; NAME_BUFFER_LEN] = [0; NAME_BUFFER_LEN];

/// The `dlopen` flags the C passed.
///
/// `RTLD_DEEPBIND` makes the object prefer its own symbols over ones already
/// in the global scope, which is what stops a plugin's private copy of, say, a
/// jpeg routine from being captured by the VM's. The C guarded it with
/// `#ifdef` because it is a glibc extension.
#[cfg(target_os = "linux")]
const DLOPEN_FLAGS: c_int = libc::RTLD_NOW | libc::RTLD_GLOBAL | libc::RTLD_DEEPBIND;
#[cfg(not(target_os = "linux"))]
const DLOPEN_FLAGS: c_int = libc::RTLD_NOW | libc::RTLD_GLOBAL;

/// Builds the `<lookupName>AccessorDepth` symbol name a plugin publishes its
/// accessor depth under, NUL-terminated.
///
/// This is where the C's unbounded `strcpy` into `char buf[256]` lived. Both
/// the name and the suffix are clamped to what the buffer holds, so a long
/// selector produces a symbol that simply will not be found instead of
/// smashing the stack. Every name short enough for the C to handle produces
/// exactly the same bytes.
fn accessor_depth_symbol_name(name: &[u8]) -> [u8; ACCESSOR_DEPTH_BUF_LEN] {
    const SUFFIX: &[u8] = b"AccessorDepth";

    let mut buf = [0u8; ACCESSOR_DEPTH_BUF_LEN];
    // One byte always reserved for the terminator.
    let name_len = name.len().min(ACCESSOR_DEPTH_BUF_LEN - 1);
    buf[..name_len].copy_from_slice(&name[..name_len]);

    let suffix_len = SUFFIX.len().min(ACCESSOR_DEPTH_BUF_LEN - 1 - name_len);
    buf[name_len..name_len + suffix_len].copy_from_slice(&SUFFIX[..suffix_len]);

    buf
}

/// The two search-path accessors, which still live in `src/utils.c`.
///
/// Both return a NULL-terminated `char **` owned by the callee. Under
/// `cfg(test)` they answer from a list the test installs, so that the search
/// order can be exercised without linking the rest of the platform layer.
mod paths {
    use core::ffi::c_char;

    /// The plugin paths given on the command line.
    pub fn plugin() -> *mut *mut c_char {
        #[cfg(test)]
        return super::tests::plugin_paths();
        #[cfg(not(test))]
        // SAFETY: getPluginPaths takes no arguments and returns a
        // NULL-terminated array valid for the process lifetime.
        unsafe {
            pharo_vm_sys::getPluginPaths()
        }
    }

    /// The platform's system search paths.
    pub fn system() -> *mut *mut c_char {
        #[cfg(test)]
        return super::tests::system_paths();
        #[cfg(not(test))]
        // SAFETY: as above.
        unsafe {
            pharo_vm_sys::getSystemSearchPaths()
        }
    }
}

/// `dlopen`s `file_name`, or returns null.
///
/// # Safety
///
/// `file_name` must be a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn loadModuleHandle(file_name: *const c_char) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        logging::message_one_string(
            LOG_TRACE,
            c"Try loading  %s\n",
            site!(C_FILE, c"loadModuleHandle", 220),
            file_name,
        );
        libc::dlopen(file_name, DLOPEN_FLAGS)
    }
}

/// `dlclose`s `module`. Returns 0 on success and 1 on failure -- see the
/// module docs on the Windows branch disagreeing.
///
/// # Safety
///
/// `module` must be a handle from [`loadModuleHandle`] that has not been
/// closed.
#[no_mangle]
pub unsafe extern "C" fn freeModuleHandle(module: *mut c_void) -> sqInt {
    // SAFETY: delegated to the caller.
    if unsafe { libc::dlclose(module) } == 0 {
        0
    } else {
        1
    }
}

/// Looks `symbol` up in `module`, or in the global scope when `module` is
/// null.
///
/// # Safety
///
/// `module` must be null or a live handle, and `symbol` must be a
/// NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn getModuleSymbol(
    module: *mut c_void,
    symbol: *const c_char,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        let handle = if module.is_null() {
            // Verbatim `dlopen(NULL, 0)`: mode 0 is invalid, so this is null on
            // glibc, which is exactly RTLD_DEFAULT there. See the module docs
            // before "simplifying" it.
            libc::dlopen(core::ptr::null(), 0)
        } else {
            module
        };
        libc::dlsym(handle, symbol)
    }
}

/// Tries every naming convention for `module_name` under `path`, returning the
/// first handle that opens.
///
/// # Safety
///
/// Both arguments must be NUL-terminated strings valid for the call. Not
/// reentrant: it writes [`moduleNameBuffer`].
#[no_mangle]
pub unsafe extern "C" fn tryToLoadModuleInPath(
    path: *mut c_char,
    module_name: *const c_char,
) -> *mut c_void {
    // Raw pointers throughout rather than slices: both globals are `static
    // mut` shared with C, and forming a Rust reference to one would be
    // undefined behaviour the moment anything else touched it.
    let buffer = core::ptr::addr_of_mut!(moduleNameBuffer).cast::<c_char>();
    let patterns = core::ptr::addr_of!(moduleNamePatterns).cast::<*const c_char>();

    // SAFETY: delegated to the caller. The loop stops at the null terminator
    // the array is declared with, so it never reads past the end.
    unsafe {
        for i in 0..PATTERN_SLOTS {
            let pattern = *patterns.add(i);
            if pattern.is_null() {
                break;
            }

            // The pattern comes from a mutable global rather than being a
            // literal, so this is a runtime format string -- as it was in C.
            // The patterns are two `%s` conversions each and nothing writes the
            // array, but note that `moduleNamePatterns` being exported means
            // that is a convention, not a guarantee.
            libc::snprintf(buffer, NAME_BUFFER_LEN, pattern, path, module_name);
            // snprintf always terminates; the C re-terminated anyway.
            *buffer.add(NAME_BUFFER_LEN - 1) = 0;

            let handle = loadModuleHandle(buffer);
            if !handle.is_null() {
                return handle;
            }
        }
    }

    core::ptr::null_mut()
}

/// Finds and opens the plugin named `plugin_name`.
///
/// Searches, in order: the plugin paths from the command line, then the empty
/// path (so a bare name resolves through the loader's own rules), then the
/// system search paths. Returns null and logs if none of them work.
///
/// # Safety
///
/// `plugin_name` must be a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn ioLoadModule(plugin_name: *mut c_char) -> *mut c_void {
    // SAFETY: both accessors return NULL-terminated arrays whose entries stay
    // valid for the process lifetime.
    unsafe {
        let mut paths = paths::plugin();
        while !(*paths).is_null() {
            let handle = tryToLoadModuleInPath(*paths, plugin_name);
            if !handle.is_null() {
                return handle;
            }
            paths = paths.add(1);
        }

        // An empty prefix, so the patterns produce bare names and the dynamic
        // loader applies its own search rules.
        let mut empty = *b"\0";
        let handle = tryToLoadModuleInPath(empty.as_mut_ptr().cast::<c_char>(), plugin_name);
        if !handle.is_null() {
            return handle;
        }

        let mut paths = paths::system();
        while !(*paths).is_null() {
            let handle = tryToLoadModuleInPath(*paths, plugin_name);
            if !handle.is_null() {
                return handle;
            }
            paths = paths.add(1);
        }

        logging::message_one_string(
            LOG_DEBUG,
            c"Failed to load module: %s\n",
            site!(C_FILE, c"ioLoadModule", 102),
            plugin_name,
        );
    }

    core::ptr::null_mut()
}

/// Closes a module handle. See [`freeModuleHandle`] for the return convention.
///
/// # Safety
///
/// `module_handle` must be a live handle from [`ioLoadModule`].
#[no_mangle]
pub unsafe extern "C" fn ioFreeModule(module_handle: *mut c_void) -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe { freeModuleHandle(module_handle) }
}

/// Looks up primitive `lookup_name` in `module_handle` and, if
/// `accessor_depth_ptr` is non-null, reports the primitive's accessor depth
/// through it.
///
/// The accessor depth is published by the plugin as a `signed char` named
/// `<lookupName>AccessorDepth`. Slang omits the variable when the depth is
/// -1, which is also the value this reports when the symbol is missing -- so
/// "absent" and "-1" are indistinguishable by design.
///
/// # Safety
///
/// `lookup_name` must be null or a NUL-terminated string valid for the call,
/// `module_handle` must be null or live, and `accessor_depth_ptr` must be null
/// or point to a writable `sqInt`.
#[no_mangle]
pub unsafe extern "C" fn ioFindExternalFunctionInAccessorDepthInto(
    lookup_name: *mut c_char,
    module_handle: *mut c_void,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        // The C tested `!*lookupName` to keep empty names out of dlsym, which
        // the comment attributes to `eitherPlugin:` code. A null name reached
        // that test through findInternalFunctionIn, which canonicalises an
        // empty name to NULL and then passes it straight here -- so the C
        // dereferenced null for a primitive whose name was the empty string.
        // Treated as empty instead, which reaches the same answer.
        if lookup_name.is_null() || *lookup_name == 0 {
            return core::ptr::null_mut();
        }

        let function = getModuleSymbol(module_handle, lookup_name);

        if function.is_null() || accessor_depth_ptr.is_null() {
            return function;
        }

        // Truncating rather than overflowing; see the module docs.
        let buf = accessor_depth_symbol_name(CStr::from_ptr(lookup_name).to_bytes());

        let depth_var = getModuleSymbol(module_handle, buf.as_ptr().cast::<c_char>());

        *accessor_depth_ptr = if depth_var.is_null() {
            logging::message_one_string(
                LOG_DEBUG,
                c"Missing Accessor Depth: %s",
                site!(C_FILE, c"ioFindExternalFunctionInAccessorDepthInto", 149),
                lookup_name,
            );
            // Slang saves space by not emitting -1 depths, so absent means -1.
            -1
        } else {
            sqInt::from(*depth_var.cast::<i8>())
        };

        function
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::Mutex;

    /// Serialises everything that touches `moduleNameBuffer` or the installed
    /// search paths, both of which are process-wide -- and shared with
    /// `named_prims`, hence the crate-wide lock.
    use crate::test_support::lock_globals as lock;

    /// The NULL-terminated arrays [`super::paths`] answers from during tests.
    ///
    /// Raw pointers are neither `Send` nor `Sync`, so this cannot go in a
    /// `static` as it stands. They are safe to move and share here because
    /// each one points into `_owned`, which is stored alongside them and
    /// outlives them, and because every reader holds `GLOBALS`.
    struct Installed {
        plugin: Vec<*mut c_char>,
        system: Vec<*mut c_char>,
        /// Keeps the strings the pointers above refer to alive.
        _owned: Vec<CString>,
    }

    // SAFETY: see the type's documentation.
    unsafe impl Send for Installed {}
    // SAFETY: see the type's documentation.
    unsafe impl Sync for Installed {}

    static INSTALLED: Mutex<Option<Installed>> = Mutex::new(None);

    fn install(plugin: &[&str], system: &[&str]) {
        let mut owned: Vec<CString> = Vec::new();
        let mut to_ptrs = |v: &[&str]| -> Vec<*mut c_char> {
            let mut out: Vec<*mut c_char> = v
                .iter()
                .map(|s| {
                    owned.push(CString::new(*s).expect("no interior NUL"));
                    owned.last().unwrap().as_ptr() as *mut c_char
                })
                .collect();
            out.push(core::ptr::null_mut());
            out
        };
        let plugin = to_ptrs(plugin);
        let system = to_ptrs(system);
        *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Installed {
            plugin,
            system,
            _owned: owned,
        });
    }

    /// Answers [`super::paths::plugin`] during tests.
    pub(super) fn plugin_paths() -> *mut *mut c_char {
        with_installed(|i| i.plugin.as_ptr() as *mut *mut c_char)
    }

    /// Answers [`super::paths::system`] during tests.
    pub(super) fn system_paths() -> *mut *mut c_char {
        with_installed(|i| i.system.as_ptr() as *mut *mut c_char)
    }

    /// An empty NULL-terminated array, for tests that never call `install`.
    ///
    /// Typed `usize` rather than `*mut c_char` so that the `static` can be
    /// `Sync`. The two have the same size and alignment, and the only value
    /// stored is the null terminator itself.
    static EMPTY: [usize; 1] = [0];

    fn with_installed(f: impl FnOnce(&Installed) -> *mut *mut c_char) -> *mut *mut c_char {
        let guard = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(i) => f(i),
            None => EMPTY.as_ptr() as *mut *mut c_char,
        }
    }

    /// Reads `moduleNameBuffer` back as a string.
    fn module_name_buffer() -> String {
        // SAFETY: the buffer is always NUL-terminated -- both snprintf and the
        // explicit store after it guarantee that.
        unsafe {
            CStr::from_ptr(core::ptr::addr_of!(moduleNameBuffer).cast::<c_char>())
                .to_string_lossy()
                .into_owned()
        }
    }

    #[test]
    fn the_pattern_list_is_null_terminated_and_platform_shaped() {
        // SAFETY: read-only access to the static, serialised by the lock.
        let _guard = lock();
        let patterns = core::ptr::addr_of!(moduleNamePatterns).cast::<*const c_char>();
        let mut seen = Vec::new();
        for i in 0..PATTERN_SLOTS {
            // SAFETY: i stays below the declared length.
            let p = unsafe { *patterns.add(i) };
            if p.is_null() {
                break;
            }
            // SAFETY: every non-null entry is a 'static literal.
            seen.push(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned());
        }

        // The terminator must be the last slot, or the loops here and in
        // tryToLoadModuleInPath would read past the end.
        assert_eq!(seen.len(), PATTERN_SLOTS - 1);

        if cfg!(target_vendor = "apple") {
            // Including the duplicate the C had; see the docs on the static.
            assert_eq!(seen, ["%s%s", "%s%s", "%s%s.dylib", "%slib%s.dylib"]);
        } else {
            assert_eq!(seen, ["%s%s", "%s%s.so", "%slib%s.so"]);
        }
    }

    #[test]
    fn a_missing_module_tries_every_pattern_and_fails() {
        let _guard = lock();
        let mut path = *b"/nonexistent-directory-for-tests/\0";
        let module = CString::new("NoSuchPlugin").unwrap();

        // SAFETY: both strings outlive the call.
        let handle =
            unsafe { tryToLoadModuleInPath(path.as_mut_ptr().cast::<c_char>(), module.as_ptr()) };
        assert!(handle.is_null());

        // The buffer holds whatever the *last* pattern produced, which is how
        // the C left it too: it is scratch space, not a result.
        let expected = if cfg!(target_vendor = "apple") {
            "/nonexistent-directory-for-tests/libNoSuchPlugin.dylib"
        } else {
            "/nonexistent-directory-for-tests/libNoSuchPlugin.so"
        };
        assert_eq!(module_name_buffer(), expected);
    }

    #[test]
    fn load_module_searches_plugin_paths_then_bare_then_system_paths() {
        let _guard = lock();
        install(
            &["/nonexistent-plugin-path-a/", "/nonexistent-plugin-path-b/"],
            &["/nonexistent-system-path/"],
        );

        let module = CString::new("StillNoSuchPlugin").unwrap();
        // SAFETY: the name outlives the call.
        let handle = unsafe { ioLoadModule(module.as_ptr() as *mut c_char) };
        assert!(handle.is_null());

        // The system paths are searched last, so the scratch buffer ends up
        // holding an attempt from the final one. That pins the search order:
        // if the system paths were tried before the plugin paths, the last
        // attempt would name a plugin path instead.
        assert!(
            module_name_buffer().starts_with("/nonexistent-system-path/"),
            "expected the last attempt to come from the system paths, got {}",
            module_name_buffer()
        );
    }

    #[test]
    fn an_empty_lookup_name_never_reaches_dlsym() {
        // The C guarded this explicitly, attributing it to `eitherPlugin:`
        // code; dlsym with an empty name sets an error that would then be
        // reported against the wrong lookup.
        let mut empty = *b"\0";
        let mut depth: sqInt = 1234;
        // SAFETY: the name is NUL-terminated and depth is writable.
        let f = unsafe {
            ioFindExternalFunctionInAccessorDepthInto(
                empty.as_mut_ptr().cast::<c_char>(),
                core::ptr::null_mut(),
                &mut depth,
            )
        };
        assert!(f.is_null());
        // Bailing out early means the depth is left alone rather than set
        // to -1, which is what the C did too.
        assert_eq!(depth, 1234);
    }

    #[test]
    fn accessor_depth_names_append_the_suffix() {
        let built = accessor_depth_symbol_name(b"primitiveFoo");
        let s = CStr::from_bytes_until_nul(&built).expect("terminated");
        assert_eq!(s.to_bytes(), b"primitiveFooAccessorDepth");
    }

    #[test]
    fn accessor_depth_names_truncate_instead_of_overflowing() {
        // This is the C's `strcpy(buf, lookupName)` into `char buf[256]`. At
        // 300 bytes it wrote 45 bytes past the end of a stack buffer.
        let long = vec![b'x'; 300];
        let built = accessor_depth_symbol_name(&long);
        let s = CStr::from_bytes_until_nul(&built).expect("terminated");
        assert_eq!(s.to_bytes().len(), ACCESSOR_DEPTH_BUF_LEN - 1);
        assert!(s.to_bytes().iter().all(|b| *b == b'x'));

        // And the boundary: a name that leaves exactly enough room, and one
        // that leaves one byte too few.
        let fits = vec![b'y'; ACCESSOR_DEPTH_BUF_LEN - 1 - b"AccessorDepth".len()];
        let built = accessor_depth_symbol_name(&fits);
        let s = CStr::from_bytes_until_nul(&built).expect("terminated");
        assert!(s.to_bytes().ends_with(b"AccessorDepth"));
        assert_eq!(s.to_bytes().len(), ACCESSOR_DEPTH_BUF_LEN - 1);

        let one_short = vec![b'z'; ACCESSOR_DEPTH_BUF_LEN - b"AccessorDepth".len()];
        let built = accessor_depth_symbol_name(&one_short);
        let s = CStr::from_bytes_until_nul(&built).expect("terminated");
        assert!(s.to_bytes().ends_with(b"AccessorDept"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_null_module_resolves_against_the_global_scope() {
        // Documents the accident described in the module docs: the C passes
        // `dlopen(NULL, 0)`, glibc rejects mode 0 and hands back null, and null
        // is glibc's RTLD_DEFAULT -- so the lookup searches everything already
        // loaded and finds libc's symbols. If this ever starts failing, the
        // `getModuleSymbol` null path needs rewriting rather than patching.
        let symbol = CString::new("malloc").unwrap();
        // SAFETY: a null module is explicitly allowed, and the name is
        // NUL-terminated.
        let f = unsafe { getModuleSymbol(core::ptr::null_mut(), symbol.as_ptr()) };
        assert!(
            !f.is_null(),
            "dlsym could not see libc through RTLD_DEFAULT"
        );
    }
}
