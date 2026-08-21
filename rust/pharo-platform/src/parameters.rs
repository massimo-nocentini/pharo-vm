//! Replaces `src/parameters/parameters.c` on Unix.
//!
//! Everything between the shell and the image: splitting `argv` into VM
//! options and image options, finding a default image when none is named,
//! parsing sizes like `2M`, and printing the usage text.
//!
//! # How argv is split
//!
//! The first argument that is neither an option nor an option's value is the
//! image name; everything after it belongs to the image. `--` on its own also
//! ends the VM's half, and then no image is named and the default is used.
//! Deciding whether an option consumes the next argument needs the option
//! table, which is why [`find_parameter_arity`] exists.
//!
//! # Scope
//!
//! Unix. The `_WIN32` half of `vm_parameters_parse` finds the executable with
//! `GetModuleFileNameW` rather than from `argv[0]`, and Apple additionally
//! reads defaults from a PList before parsing. `cmake/rust.cmake` keeps the C
//! on both.
//!
//! # `ALWAYS_INTERACTIVE` is dead code in every build, and that is a bug
//!
//! `config.h.in` has `#define ALWAYS_INTERACTIVE @ALWAYS_INTERACTIVE@`, and
//! CMake substitutes the option's value, which is the bare token `OFF` or
//! `ON`. `#if` replaces any identifier it does not know with 0, so **both**
//! spell false: turning the option on in CMake changes packaging
//! (`cmake/packaging.cmake` tests it with CMake's own `if`, which does
//! understand `ON`) but cannot change the VM's behaviour. So
//! `vm_parameters_ensure_interactive_image_parameter` has never inverted the
//! headless/interactive logic, and `--headless` has always been appended to
//! the VM parameters.
//!
//! This port reproduces the behaviour the C actually has, and does not carry
//! the unreachable branches. Fixing the `#define` is a separate change, and it
//! would change behaviour for anyone who has the option switched on today.
//!
//! # Faithful oddities
//!
//! * `logParameterVector` prints a `size_t` count with `%u`. On a 64-bit build
//!   that is a mismatched conversion; it prints the low 32 bits, which is
//!   right for every count that occurs. Reproduced so the log stays diffable.
//! * `parseByteSize` reports failure by returning
//!   `VM_ERROR_INVALID_PARAMETER_VALUE`, which is -6, in the same `long long`
//!   it uses for sizes. Callers test `< 0`. So `--edenSize=-6` and a malformed
//!   `--edenSize` are indistinguishable, and both are rejected.
//! * `processStackPageSizeOption` and `processMaxFramesToPrintOption` truncate
//!   their `long long` into an `int` before the `< 0` test. A size above 2 Gb
//!   can therefore wrap to a negative and be rejected, or worse wrap to a
//!   small positive. Reproduced exactly.
//! * `processLogLevelOption` rejects `0` along with everything unparseable,
//!   because `strtol` answers 0 for both. `--logLevel=0` is not a way to
//!   silence the VM.

use core::ffi::{c_char, c_int, c_longlong, c_void, CStr};

use pharo_vm_sys::{VMErrorCode, VMParameterVector, VMParameters};

use crate::logging::{self, site, LOG_DEBUG, LOG_ERROR};
use crate::parameter_vector::{
    vm_parameter_vector_destroy, vm_parameter_vector_has_element, vm_parameter_vector_insert_from,
};
use crate::path_utilities::{
    vm_path_extract_dirname_into, vm_path_get_current_working_dir_into, vm_path_join_into,
    vm_path_make_absolute_into,
};

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/parameters/parameters.c";

/// `FILENAME_MAX` from stdio.h, which the C sized its path buffers with.
const FILENAME_MAX: usize = pharo_vm_sys::FILENAME_MAX as usize;

/// The image name looked for when none is given, from `config.h`.
fn default_image_name() -> &'static CStr {
    CStr::from_bytes_with_nul(pharo_vm_sys::DEFAULT_IMAGE_NAME).expect("NUL-terminated macro")
}

/// The VM's name, from `config.h`. Used only in the usage text.
fn vm_name() -> &'static str {
    CStr::from_bytes_with_nul(pharo_vm_sys::VM_NAME)
        .expect("NUL-terminated macro")
        .to_str()
        .expect("VM_NAME is ASCII")
}

/// What a parameter does when it is seen.
type ProcessFn = fn(Option<&CStr>, &mut VMParameters) -> VMErrorCode;

/// One row of the option table.
struct ParameterSpec {
    /// The option's name, without leading dashes.
    name: &'static str,
    /// Whether it consumes a value.
    has_argument: bool,
    /// What to do with it, or `None` for options only the image cares about.
    function: Option<ProcessFn>,
}

/// The VM's options, in the C's order.
///
/// `headless`, `interactive` and `vm-display-null` have no function: they are
/// recognised here only so that the split between VM and image arguments knows
/// they are VM options, and the image reads them from the vector afterwards.
const PARAMETER_SPECS: &[ParameterSpec] = &[
    ParameterSpec {
        name: "headless",
        has_argument: false,
        function: None,
    },
    #[cfg(pharo_vm_in_worker_thread)]
    ParameterSpec {
        name: "worker",
        has_argument: false,
        function: Some(process_worker),
    },
    // For pharo-ui scripts.
    ParameterSpec {
        name: "interactive",
        has_argument: false,
        function: None,
    },
    // For Smalltalk CI.
    ParameterSpec {
        name: "vm-display-null",
        has_argument: false,
        function: None,
    },
    ParameterSpec {
        name: "help",
        has_argument: false,
        function: Some(process_help),
    },
    ParameterSpec {
        name: "h",
        has_argument: false,
        function: Some(process_help),
    },
    ParameterSpec {
        name: "version",
        has_argument: false,
        function: Some(process_print_version),
    },
    ParameterSpec {
        name: "logLevel",
        has_argument: true,
        function: Some(process_log_level),
    },
    ParameterSpec {
        name: "stackPageSize",
        has_argument: true,
        function: Some(process_stack_page_size),
    },
    ParameterSpec {
        name: "maxFramesToLog",
        has_argument: true,
        function: Some(process_max_frames_to_print),
    },
    ParameterSpec {
        name: "maxOldSpaceSize",
        has_argument: true,
        function: Some(process_max_old_space_size),
    },
    ParameterSpec {
        name: "codeSize",
        has_argument: true,
        function: Some(process_max_code_space_size),
    },
    ParameterSpec {
        name: "edenSize",
        has_argument: true,
        function: Some(process_eden_size),
    },
    ParameterSpec {
        name: "minPermSpaceSize",
        has_argument: true,
        function: Some(process_min_perm_space_size),
    },
    ParameterSpec {
        name: "maxSlotsForNewSpaceAlloc",
        has_argument: true,
        function: Some(process_max_slots_for_new_space_alloc),
    },
    ParameterSpec {
        name: "workingDirectory",
        has_argument: true,
        function: Some(process_working_directory),
    },
    ParameterSpec {
        name: "avoidSearchingSegmentsWithPinnedObjects",
        has_argument: false,
        function: Some(process_avoid_searching_segments_with_pinned_objects),
    },
    // The XCode debugger passes this one.
    #[cfg(target_vendor = "apple")]
    ParameterSpec {
        name: "NSDocumentRevisionsDebugMode",
        has_argument: false,
        function: None,
    },
];

/// Where an image is looked for, relative to the VM's directory.
const IMAGE_SEARCH_SUFFIXES: &[&str] = &[
    "",
    #[cfg(target_vendor = "apple")]
    "../Resources/",
    #[cfg(target_vendor = "apple")]
    "../../../",
];

/// Parses `[integer][kKmMgG]` into a byte count.
///
/// Answers `VM_ERROR_INVALID_PARAMETER_VALUE` (-6) when the text does not
/// start with a non-negative number; see the module docs on why that shares
/// the return type with a size.
///
/// # Safety
///
/// `text` must be a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn parseByteSize(text: *const c_char) -> c_longlong {
    // The C copied into a 255-byte alloca first, so anything longer is
    // truncated before parsing rather than rejected.
    // SAFETY: delegated to the caller.
    let bytes = unsafe { CStr::from_ptr(text) }.to_bytes();
    let mut argument = &bytes[..bytes.len().min(254)];

    let mut multiplier: c_longlong = 1;
    if let Some(&last) = argument.last() {
        multiplier = match last {
            b'k' | b'K' => 1024,
            b'm' | b'M' => 1024 * 1024,
            b'g' | b'G' => 1024 * 1024 * 1024,
            _ => 1,
        };
        if multiplier != 1 {
            argument = &argument[..argument.len() - 1];
        }
    }

    // strtoll stops at the first character it cannot use and answers 0 for a
    // string with no digits at all, which is why "abc" parses as 0 rather than
    // failing. Reproduced.
    let text = core::str::from_utf8(argument).unwrap_or("");
    let digits_end = text
        .find(|c: char| !c.is_ascii_digit() && c != '+' && c != '-')
        .unwrap_or(text.len());
    let head = text[..digits_end].trim_start();

    let Ok(value) = head.parse::<c_longlong>() else {
        // Either nothing numeric, which strtoll reports as 0 with errno unset,
        // or a value too large, which it reports as ERANGE. The C tested errno
        // and the sign, so an unparseable string reached `intValue == 0` and
        // was accepted as zero; only an overflow or a negative was rejected.
        return if head.is_empty() {
            0
        } else {
            VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE.0 as c_longlong
        };
    };

    if value < 0 {
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE.0 as c_longlong;
    }

    value.wrapping_mul(multiplier)
}

/// Finds the option named by the first `name_size` bytes of `name`.
fn find_parameter_with_name(name: &[u8]) -> Option<&'static ParameterSpec> {
    PARAMETER_SPECS
        .iter()
        .find(|spec| spec.name.as_bytes() == name)
}

/// How many following arguments `parameter` consumes: 0 or 1.
///
/// Anything that is not a recognised option consumes nothing, and so does an
/// option written as `--name=value`, because the value is already attached.
fn find_parameter_arity(parameter: &[u8]) -> usize {
    let Some(rest) = parameter.strip_prefix(b"-") else {
        return 0;
    };
    let rest = rest.strip_prefix(b"-").unwrap_or(rest);

    if rest.contains(&b'=') {
        return 0;
    }

    match find_parameter_with_name(rest) {
        Some(spec) if spec.has_argument => 1,
        _ => 0,
    }
}

/// Whether the process has a console attached.
///
/// Always false on Unix, as in the C -- which carries a `FIXME` saying the
/// client should decide this.
fn is_in_console() -> bool {
    false
}

/// Releases everything a [`VMParameters`] owns and zeroes it.
///
/// # Safety
///
/// `parameters`, if non-null, must point to an initialised [`VMParameters`]
/// that is not used again without re-initialising.
#[no_mangle]
pub unsafe extern "C" fn vm_parameters_destroy(parameters: *mut VMParameters) -> VMErrorCode {
    if parameters.is_null() {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }
    // SAFETY: delegated to the caller. imageFileName is owned, so it came from
    // strdup or calloc and goes back to libc::free.
    unsafe {
        libc::free((*parameters).imageFileName.cast::<c_void>());
        vm_parameter_vector_destroy(core::ptr::addr_of_mut!((*parameters).vmParameters));
        vm_parameter_vector_destroy(core::ptr::addr_of_mut!((*parameters).imageParameters));
        core::ptr::write_bytes(parameters, 0, 1);
    }
    VMErrorCode::VM_SUCCESS
}

/// Copies a Rust path into a freshly `malloc`ed C string.
///
/// The result is owned by `VMParameters::imageFileName`, which
/// [`vm_parameters_destroy`] frees with `libc::free`, so it cannot come from
/// Rust's allocator.
fn strdup_bytes(bytes: &[u8]) -> *mut c_char {
    // SAFETY: the allocation is one byte longer than the copy, and the extra
    // byte is written as the terminator.
    unsafe {
        let p = libc::malloc(bytes.len() + 1).cast::<u8>();
        if p.is_null() {
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        *p.add(bytes.len()) = 0;
        p.cast::<c_char>()
    }
}

/// Whether a path exists, through the image file handler.
///
/// The C used the `sqImageFileExists` macro, which dispatches through
/// `currentFileAccessHandler()`, so a test that installed its own handler is
/// obeyed here too.
fn image_file_exists(path: *const c_char) -> bool {
    // SAFETY: currentFileAccessHandler never answers null, and the slot is
    // always filled.
    unsafe {
        let handler = crate::image_access::currentFileAccessHandler();
        match (*handler).imageFileExists {
            Some(f) => f(path) != 0,
            None => false,
        }
    }
}

/// Looks for a default image next to the VM, then in the working directory.
///
/// Always answers `VM_SUCCESS` unless it runs out of memory: when no image is
/// found it still sets `imageFileName` to the bare default name and clears
/// `defaultImageFound`, leaving the caller to report it.
///
/// # Safety
///
/// `vm_executable_path` must be a NUL-terminated string and `parameters` must
/// be initialised.
#[no_mangle]
pub unsafe extern "C" fn vm_find_startup_image(
    vm_executable_path: *const c_char,
    parameters: *mut VMParameters,
) -> VMErrorCode {
    let mut image_path = [0u8; FILENAME_MAX + 1];
    let mut vm_path = [0u8; FILENAME_MAX + 1];
    let mut search_path = [0u8; FILENAME_MAX + 1];

    // SAFETY: delegated to the caller; every buffer below is passed with its
    // real length.
    unsafe {
        // The VM's own absolute directory.
        vm_path_make_absolute_into(
            search_path.as_mut_ptr().cast::<c_char>(),
            search_path.len(),
            vm_executable_path,
        );
        if image_file_exists(search_path.as_ptr().cast::<c_char>()) {
            vm_path_extract_dirname_into(
                vm_path.as_mut_ptr().cast::<c_char>(),
                vm_path.len(),
                search_path.as_ptr().cast::<c_char>(),
            );
        } else {
            // strncpy of at most FILENAME_MAX, then terminate at [FILENAME_MAX].
            let source = CStr::from_ptr(vm_executable_path).to_bytes();
            let n = source.len().min(FILENAME_MAX);
            vm_path[..n].copy_from_slice(&source[..n]);
            vm_path[FILENAME_MAX] = 0;
        }

        for suffix in IMAGE_SEARCH_SUFFIXES {
            let mut joined = [0u8; FILENAME_MAX + 1];
            let suffix_with_name = format!("{suffix}{}", default_image_name().to_string_lossy());
            let Ok(suffix_c) = std::ffi::CString::new(suffix_with_name) else {
                continue;
            };
            vm_path_join_into(
                joined.as_mut_ptr().cast::<c_char>(),
                joined.len(),
                vm_path.as_ptr().cast::<c_char>(),
                suffix_c.as_ptr(),
            );
            if image_file_exists(joined.as_ptr().cast::<c_char>()) {
                image_path = joined;
                let name = CStr::from_ptr(image_path.as_ptr().cast::<c_char>()).to_bytes();
                (*parameters).imageFileName = strdup_bytes(name);
                (*parameters).isDefaultImage = true;
                (*parameters).defaultImageFound = true;
                return if (*parameters).imageFileName.is_null() {
                    VMErrorCode::VM_ERROR_OUT_OF_MEMORY
                } else {
                    VMErrorCode::VM_SUCCESS
                };
            }
        }

        // Then the current working directory.
        vm_path_get_current_working_dir_into(
            search_path.as_mut_ptr().cast::<c_char>(),
            search_path.len(),
        );
        vm_path_join_into(
            image_path.as_mut_ptr().cast::<c_char>(),
            image_path.len(),
            search_path.as_ptr().cast::<c_char>(),
            default_image_name().as_ptr(),
        );

        (*parameters).isDefaultImage = true;
        if image_file_exists(image_path.as_ptr().cast::<c_char>()) {
            let name = CStr::from_ptr(image_path.as_ptr().cast::<c_char>()).to_bytes();
            (*parameters).imageFileName = strdup_bytes(name);
            (*parameters).defaultImageFound = true;
        } else {
            (*parameters).imageFileName = strdup_bytes(default_image_name().to_bytes());
            (*parameters).defaultImageFound = false;
        }

        if (*parameters).imageFileName.is_null() {
            VMErrorCode::VM_ERROR_OUT_OF_MEMORY
        } else {
            VMErrorCode::VM_SUCCESS
        }
    }
}

/// Borrows `argv` as a slice of C strings.
///
/// # Safety
///
/// `argv` must have at least `argc` readable entries, each null or a
/// NUL-terminated string.
unsafe fn argv_slice<'a>(argc: c_int, argv: *const *const c_char) -> &'a [*const c_char] {
    if argv.is_null() || argc <= 0 {
        return &[];
    }
    // SAFETY: delegated to the caller.
    unsafe { core::slice::from_raw_parts(argv, argc as usize) }
}

/// The bytes of `argv[i]`, or empty if it is null.
///
/// # Safety
///
/// The entry must be null or a NUL-terminated string.
unsafe fn arg_bytes<'a>(arg: *const c_char) -> &'a [u8] {
    if arg.is_null() {
        return b"";
    }
    // SAFETY: delegated to the caller.
    unsafe { CStr::from_ptr(arg) }.to_bytes()
}

/// The index of the image name in `argv`, or `argc` if there is none.
///
/// # Safety
///
/// As [`argv_slice`].
unsafe fn find_image_name_index(argc: c_int, argv: *const *const c_char) -> c_int {
    // SAFETY: delegated to the caller.
    let args = unsafe { argv_slice(argc, argv) };

    // argv[0] is the executable.
    let mut i = 1usize;
    while i < args.len() {
        // SAFETY: as above.
        let argument = unsafe { arg_bytes(args[i]) };

        // The explicit end of the VM's arguments.
        if argument == b"--" {
            return i as c_int;
        }

        if argument.first() == Some(&b'-') {
            i += 1 + find_parameter_arity(argument);
            continue;
        }

        // The first argument that is not an option is the image.
        return i as c_int;
    }

    argc
}

/// Takes the image name from `argv`, if one is named there.
///
/// # Safety
///
/// As [`argv_slice`], and `parameters` must be initialised.
unsafe fn fill_up_image_name(
    argc: c_int,
    argv: *const *const c_char,
    parameters: *mut VMParameters,
) -> VMErrorCode {
    // SAFETY: delegated to the caller.
    unsafe {
        let index = find_image_name_index(argc, argv);
        if index == argc {
            return VMErrorCode::VM_SUCCESS;
        }
        let args = argv_slice(argc, argv);
        let named = arg_bytes(args[index as usize]);
        if named == b"--" {
            return VMErrorCode::VM_SUCCESS;
        }

        (*parameters).imageFileName = strdup_bytes(named);
        (*parameters).isDefaultImage = false;
        (*parameters).isInteractiveSession = false;
    }
    VMErrorCode::VM_SUCCESS
}

/// Splits `argv` into the VM's parameters and the image's.
///
/// # Safety
///
/// As [`fill_up_image_name`].
unsafe fn split_vm_and_image_parameters(
    argc: c_int,
    argv: *const *const c_char,
    parameters: *mut VMParameters,
) -> VMErrorCode {
    // SAFETY: delegated to the caller.
    unsafe {
        let image_name_index = find_image_name_index(argc, argv);
        let number_of_vm_parameters = image_name_index;
        let number_of_image_parameters = (argc - image_name_index - 1).max(0);

        if (*parameters).imageFileName.is_null() {
            let args = argv_slice(argc, argv);
            let executable = args.first().copied().unwrap_or(core::ptr::null());
            let error = vm_find_startup_image(executable, parameters);
            if error != VMErrorCode::VM_SUCCESS {
                return error;
            }
            (*parameters).isInteractiveSession = !is_in_console() && (*parameters).isDefaultImage;
        }

        let error = vm_parameter_vector_insert_from(
            core::ptr::addr_of_mut!((*parameters).imageParameters),
            number_of_image_parameters as u32,
            argv.add(image_name_index as usize + 1),
        );
        if error != VMErrorCode::VM_SUCCESS {
            vm_parameters_destroy(parameters);
            return error;
        }

        let error = vm_parameter_vector_insert_from(
            core::ptr::addr_of_mut!((*parameters).vmParameters),
            number_of_vm_parameters as u32,
            argv,
        );
        if error != VMErrorCode::VM_SUCCESS {
            vm_parameters_destroy(parameters);
            return error;
        }

        // Always appended: see the ALWAYS_INTERACTIVE note in the module docs.
        let extra: *const c_char = c"--headless".as_ptr();
        let error = vm_parameter_vector_insert_from(
            core::ptr::addr_of_mut!((*parameters).vmParameters),
            1,
            &extra,
        );
        if error != VMErrorCode::VM_SUCCESS {
            vm_parameters_destroy(parameters);
            return error;
        }
    }
    VMErrorCode::VM_SUCCESS
}

/// Logs one parameter vector, name then each element.
///
/// # Safety
///
/// `vector` must be initialised.
unsafe fn log_parameter_vector(vector_name: &'static CStr, vector: *const VMParameterVector) {
    // SAFETY: delegated to the caller.
    unsafe {
        // `%u` against a size_t: the C's mismatch, kept. See the module docs.
        logging::message_string_and_u32(
            LOG_DEBUG,
            c"%s [count = %u]:",
            site!(C_FILE, c"logParameterVector", 349),
            vector_name.as_ptr(),
            (*vector).count,
        );
        for i in 0..(*vector).count as usize {
            logging::message_one_string(
                LOG_DEBUG,
                c" %s",
                site!(C_FILE, c"logParameterVector", 352),
                *(*vector).parameters.add(i),
            );
        }
    }
}

/// Logs the whole parsed configuration at debug level.
///
/// # Safety
///
/// `parameters` must be initialised.
unsafe fn log_parameters(parameters: *const VMParameters) {
    /// The C's `flag ? "yes" : "no"`.
    fn yes_no(flag: bool) -> *const c_char {
        if flag {
            c"yes".as_ptr()
        } else {
            c"no".as_ptr()
        }
    }

    // SAFETY: delegated to the caller.
    unsafe {
        logging::message_one_string(
            LOG_DEBUG,
            c"Image file name: %s",
            site!(C_FILE, c"logParameters", 359),
            (*parameters).imageFileName,
        );
        logging::message_one_string(
            LOG_DEBUG,
            c"Is default Image: %s",
            site!(C_FILE, c"logParameters", 360),
            yes_no((*parameters).isDefaultImage),
        );
        logging::message_one_string(
            LOG_DEBUG,
            c"Is interactive session: %s",
            site!(C_FILE, c"logParameters", 361),
            yes_no((*parameters).isInteractiveSession),
        );
        logging::message_one_string(
            LOG_DEBUG,
            c"Is in worker mode: %s",
            site!(C_FILE, c"logParameters", 362),
            yes_no((*parameters).isWorker),
        );

        log_parameter_vector(
            c"vmParameters",
            core::ptr::addr_of!((*parameters).vmParameters),
        );
        log_parameter_vector(
            c"imageParameters",
            core::ptr::addr_of!((*parameters).imageParameters),
        );
    }
}

/// Adds `--interactive` to the image's parameters when the session is
/// interactive and it is not already there.
///
/// # Safety
///
/// `parameters` must be initialised.
#[no_mangle]
pub unsafe extern "C" fn vm_parameters_ensure_interactive_image_parameter(
    parameters: *mut VMParameters,
) -> VMErrorCode {
    // SAFETY: delegated to the caller.
    unsafe {
        if !(*parameters).isInteractiveSession {
            return VMErrorCode::VM_SUCCESS;
        }
        if vm_parameter_vector_has_element(
            core::ptr::addr_of!((*parameters).imageParameters),
            c"--interactive".as_ptr(),
        ) {
            return VMErrorCode::VM_SUCCESS;
        }
        let interactive: *const c_char = c"--interactive".as_ptr();
        vm_parameter_vector_insert_from(
            core::ptr::addr_of_mut!((*parameters).imageParameters),
            1,
            &interactive,
        )
    }
}

/// The usage text, with the VM's name from `config.h` substituted in.
///
/// Transcribed from the `fprintf` in `vm_printUsageTo`, with the
/// `ALWAYS_INTERACTIVE` branch dropped because it is never compiled -- see the
/// module docs. Built at call time rather than being a constant, because the
/// name is a macro from the generated `config.h`.
///
/// Odd spacing, the missing newline after `--maxSlotsForNewSpaceAlloc` and the
/// stray tabs are all the C's, and are kept so `--help` is byte-identical
/// between the two builds.
fn usage_text() -> String {
    let vm_name = vm_name();
    let mut text = String::new();
    text.push_str("Usage: ");
    text.push_str(vm_name);
    text.push_str(" [<option>...] [<imageName> [<argument>...]]\n       ");
    text.push_str(vm_name);
    text.push_str(" [<option>...] -- [<argument>...]\n\nCommon <option>s:\n  --help                               Print this help message, then exit\n");
    text.push_str(
        "  --headless                           Run in headless (no window) mode (default: true)\n",
    );
    #[cfg(pharo_vm_in_worker_thread)]
    text.push_str("  --worker                             Run in worker thread (default: false)\n");
    text.push_str("  --logLevel=<level>                   Sets the log level number (ERROR(1), WARN(2), INFO(3), DEBUG(4), TRACE(5))\n  --version                            Print version information, then exit\n  --maxFramesToLog=<cant>              Sets the max numbers of Smalltalk frames to log\n  --maxOldSpaceSize=<bytes>            Sets the max size of the old space. As the other\n                                       spaces are fixed (or calculated from this) with\n                                       this parameter is possible to set the total size.\n                                       It is possible to use k(kB), M(MB) and G(GB).\n  --codeSize=<size>[mk]                Sets the max size of code zone (default: 1M)\n                                       It is possible to use k(kB), M(MB) and G(GB).\n  --edenSize=<size>[mk]                Sets the size of eden (default: 15M)\n                                       It is possible to use k(kB), M(MB) and G(GB).\n  --maxSlotsForNewSpaceAlloc=<words>	The max numbers of slots to allow allocating in a single young indexable object  --minPermSpaceSize=<size>[mk]        Sets the min size of the permanent space (default: 0k)\n                                       It is possible to use k(kB), M(MB) and G(GB).\n  --stackPageSize=<size>[mk]           Sets the size of each stack page (default: 8k)\n                                       It is possible to use k(kB), M(MB) and G(GB).\n  --workingDirectory=<dir>		It sets the working directory for the running image.\n  --avoidSearchingSegmentsWithPinnedObjects\n                                       When pinning young objects, the objects are cloned into the old space. (default: false)\n                                       It tries to allocate the object in a segment with already pinned objects.\n	                                  Avoid the clonning process avoid this search and allocate the clonned object anywhere?\n\n\nNotes:\n\n  <imageName> defaults to `Pharo.image'.\n  <argument>s are ignored, but are processed by the Pharo image.\n  Precede <arguments> by `--' to use default image.\n");
    text
}

/// Writes the usage text to `out`.
///
/// # Safety
///
/// `out` must be an open `FILE *`.
#[no_mangle]
pub unsafe extern "C" fn vm_printUsageTo(out: *mut c_void) {
    let text = usage_text();
    // SAFETY: writing `text.len()` bytes from a live buffer to the caller's
    // stream. fwrite rather than fprintf because the text is data, not a
    // format string -- the C passed it as one, which was safe only because it
    // happens to contain no conversions.
    unsafe {
        libc::fwrite(
            text.as_ptr().cast::<c_void>(),
            1,
            text.len(),
            out.cast::<libc::FILE>(),
        );
    }
}

/// Reports a rejected option value and prints the usage to stderr.
fn reject(message: &'static CStr, line: c_int, function: &'static CStr, value: Option<&CStr>) {
    // SAFETY: the format string has one %s and the value is NUL-terminated.
    unsafe {
        logging::message_one_string(
            LOG_ERROR,
            message,
            site!(C_FILE, function, line),
            value.map_or(core::ptr::null(), CStr::as_ptr),
        );
        vm_printUsageTo(c_stderr().cast::<c_void>());
    }
}

/// `stderr` and `stdout`, which the `libc` crate does not expose because C
/// hides them behind macros.
mod c_streams {
    extern "C" {
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
            link_name = "__stderrp"
        )]
        static mut stderr: *mut libc::FILE;
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
            link_name = "__stdoutp"
        )]
        static mut stdout: *mut libc::FILE;
    }

    /// The `FILE *` that C's `stderr` macro evaluates to.
    pub fn err() -> *mut libc::FILE {
        // SAFETY: initialised by the C runtime before main.
        unsafe { stderr }
    }

    /// The `FILE *` that C's `stdout` macro evaluates to.
    pub fn out() -> *mut libc::FILE {
        // SAFETY: as above.
        unsafe { stdout }
    }
}

use c_streams::err as c_stderr;

/// The entry points that still live in C: `logLevel` in `src/debug.c`, the
/// rest in `src/utils.c`.
///
/// Under `cfg(test)` they answer from local state, so the unit-test binary does
/// not have to link the rest of the platform layer.
mod c_api {
    use core::ffi::{c_char, c_int};

    /// Sets the global log level.
    pub fn log_level(level: c_int) {
        #[cfg(test)]
        super::tests::record_log_level(level);
        #[cfg(not(test))]
        // SAFETY: takes an int and sets a global.
        unsafe {
            pharo_vm_sys::logLevel(level);
        }
    }

    /// The VM's version string, owned by the VM.
    pub fn vm_version() -> *const c_char {
        #[cfg(test)]
        return c"test-vm-version".as_ptr();
        #[cfg(not(test))]
        // SAFETY: answers a 'static string built into the VM.
        unsafe {
            pharo_vm_sys::getVMVersion()
        }
    }

    /// The source revision the VM was built from.
    pub fn source_version() -> *const c_char {
        #[cfg(test)]
        return c"test-source-version".as_ptr();
        #[cfg(not(test))]
        // SAFETY: as above.
        unsafe {
            pharo_vm_sys::getSourceVersion()
        }
    }

    /// Records where the VM executable is, for `Smalltalk vmPath`.
    ///
    /// # Safety
    ///
    /// `path` must be null or a NUL-terminated string. The C copied it with
    /// `strcpy` and so dereferenced null; that is the caller's problem here
    /// too, and `vm_parameters_parse` can still pass null when `realpath`
    /// fails.
    pub unsafe fn set_vm_path(path: *const c_char) {
        #[cfg(test)]
        let _ = path;
        #[cfg(not(test))]
        // SAFETY: delegated to the caller.
        unsafe {
            pharo_vm_sys::setVMPath(path);
        }
    }

    /// Resolves `relative` into `buffer`, answering `buffer` or null.
    ///
    /// # Safety
    ///
    /// `relative` must be a NUL-terminated string and `buffer` must be
    /// writable for `size` bytes.
    pub unsafe fn get_full_path(
        relative: *const c_char,
        buffer: *mut c_char,
        size: c_int,
    ) -> *mut c_char {
        #[cfg(test)]
        {
            let _ = (relative, size);
            buffer
        }
        #[cfg(not(test))]
        // SAFETY: delegated to the caller.
        unsafe {
            pharo_vm_sys::getFullPath(relative, buffer, size)
        }
    }
}

/// Parses `value` as a base-10 integer the way `strtol` does: leading digits
/// only, and 0 when there are none.
fn strtol_like(value: Option<&CStr>) -> c_longlong {
    let Some(value) = value else {
        return 0;
    };
    let Ok(text) = value.to_str() else {
        return 0;
    };
    let text = text.trim_start();
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    sign * digits.parse::<c_longlong>().unwrap_or(0)
}

/// `--logLevel=<n>`. Rejects 0 along with anything unparseable.
fn process_log_level(value: Option<&CStr>, _params: &mut VMParameters) -> VMErrorCode {
    let int_value = strtol_like(value) as c_int;
    if int_value == 0 {
        reject(
            c"Invalid option for logLevel: %s\n",
            475,
            c"processLogLevelOption",
            value,
        );
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
    }
    c_api::log_level(int_value);
    VMErrorCode::VM_SUCCESS
}

/// Parses a byte size for an option, or `None` if it is rejected.
///
/// `truncate_to_int` reproduces the C's `int intValue = parseByteSize(...)`
/// for the two options that declared an `int`.
fn byte_size_option(value: Option<&CStr>, truncate_to_int: bool) -> Option<c_longlong> {
    let raw = value?;
    // SAFETY: `raw` is a live CStr for the duration of the call.
    let parsed = unsafe { parseByteSize(raw.as_ptr()) };
    let parsed = if truncate_to_int {
        c_longlong::from(parsed as c_int)
    } else {
        parsed
    };
    if parsed < 0 {
        None
    } else {
        Some(parsed)
    }
}

/// `--stackPageSize=<size>`.
fn process_stack_page_size(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    match byte_size_option(value, true) {
        Some(size) => {
            params.stackPageSize = size as c_int;
            VMErrorCode::VM_SUCCESS
        }
        None => {
            reject(
                c"Invalid option for stackPageSize: %s\n",
                493,
                c"processStackPageSizeOption",
                value,
            );
            VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE
        }
    }
}

/// `--maxFramesToLog=<n>`. Counted, not a byte size.
fn process_max_frames_to_print(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    let int_value = strtol_like(value) as c_int;
    if int_value < 0 {
        reject(
            c"Invalid option for maxFramesToLog: %s\n",
            512,
            c"processMaxFramesToPrintOption",
            value,
        );
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
    }
    params.maxStackFramesToPrint = int_value;
    VMErrorCode::VM_SUCCESS
}

/// `--maxOldSpaceSize=<size>`.
fn process_max_old_space_size(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    match byte_size_option(value, false) {
        Some(size) => {
            params.maxOldSpaceSize = size;
            VMErrorCode::VM_SUCCESS
        }
        None => {
            reject(
                c"Invalid option for maxOldSpaceSize: %s\n",
                529,
                c"processMaxOldSpaceSizeOption",
                value,
            );
            VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE
        }
    }
}

/// `--codeSize=<size>`.
fn process_max_code_space_size(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    match byte_size_option(value, false) {
        Some(size) => {
            params.maxCodeSize = size;
            VMErrorCode::VM_SUCCESS
        }
        None => {
            reject(
                c"Invalid option for codeSize: %s\n",
                546,
                c"processMaxCodeSpaceSizeOption",
                value,
            );
            VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE
        }
    }
}

/// `--minPermSpaceSize=<size>`.
fn process_min_perm_space_size(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    match byte_size_option(value, false) {
        Some(size) => {
            params.minPermSpaceSize = size;
            VMErrorCode::VM_SUCCESS
        }
        None => {
            reject(
                c"Invalid option for min perm space size: %s\n",
                563,
                c"processMinPermSpaceSizeOption",
                value,
            );
            VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE
        }
    }
}

/// `--maxSlotsForNewSpaceAlloc=<words>`. A plain count, so no k/M/G suffix.
fn process_max_slots_for_new_space_alloc(
    value: Option<&CStr>,
    params: &mut VMParameters,
) -> VMErrorCode {
    let int_value = strtol_like(value);
    if int_value < 0 {
        reject(
            c"Invalid option for max slots for new space allocation: %s\n",
            580,
            c"processMaxSlotsForNewSpaceAlloc",
            value,
        );
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
    }
    params.maxSlotsForNewSpaceAlloc = int_value;
    VMErrorCode::VM_SUCCESS
}

/// `--workingDirectory=<dir>`. Reports a failed `chdir` but does not fail.
fn process_working_directory(value: Option<&CStr>, _params: &mut VMParameters) -> VMErrorCode {
    // SAFETY: the value is a live NUL-terminated string.
    unsafe {
        logging::message_one_string(
            LOG_DEBUG,
            c"Changing working directory to: %s",
            site!(C_FILE, c"processWorkingDirectory", 594),
            value.map_or(core::ptr::null(), CStr::as_ptr),
        );
        let path = value.map_or(core::ptr::null(), CStr::as_ptr);
        if !path.is_null() && libc::chdir(path) == -1 {
            logging::error_from_errno(
                c"Error changing directory",
                site!(C_FILE, c"processWorkingDirectory", 596),
            );
        }
    }
    VMErrorCode::VM_SUCCESS
}

/// `--edenSize=<size>`, capped at 1 Gb.
fn process_eden_size(value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    let Some(size) = byte_size_option(value, false) else {
        reject(
            c"Invalid option for eden: %s\n",
            609,
            c"processEdenSizeOption",
            value,
        );
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
    };

    // The cap comes from #nextCorpseOffset: in the scavenger.
    if size > 1024 * 1024 * 1024 {
        reject(
            c"The max value for eden is 1G: %s\n",
            617,
            c"processEdenSizeOption",
            value,
        );
        return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
    }

    params.edenSize = size;
    VMErrorCode::VM_SUCCESS
}

/// `--worker`.
#[cfg(pharo_vm_in_worker_thread)]
fn process_worker(_value: Option<&CStr>, params: &mut VMParameters) -> VMErrorCode {
    params.isWorker = true;
    VMErrorCode::VM_SUCCESS
}

/// `--help` and `-h`. Answers the "stop, successfully" code.
fn process_help(_value: Option<&CStr>, _params: &mut VMParameters) -> VMErrorCode {
    // SAFETY: stdout is a valid stream.
    unsafe { vm_printUsageTo(c_streams::out().cast::<c_void>()) };
    VMErrorCode::VM_ERROR_EXIT_WITH_SUCCESS
}

/// `--version`.
fn process_print_version(_value: Option<&CStr>, _params: &mut VMParameters) -> VMErrorCode {
    // SAFETY: both answer 'static NUL-terminated strings, and printf is given
    // one %s each time.
    unsafe {
        libc::printf(c"%s\n".as_ptr(), c_api::vm_version());
        libc::printf(c"Built from: %s\n".as_ptr(), c_api::source_version());
    }
    VMErrorCode::VM_ERROR_EXIT_WITH_SUCCESS
}

/// `--avoidSearchingSegmentsWithPinnedObjects`.
fn process_avoid_searching_segments_with_pinned_objects(
    _value: Option<&CStr>,
    params: &mut VMParameters,
) -> VMErrorCode {
    params.avoidSearchingSegmentsWithPinnedObjects = true;
    VMErrorCode::VM_SUCCESS
}

/// Walks the VM's parameter vector and runs each option's handler.
///
/// Starts at index 1: index 0 is the executable name.
///
/// # Safety
///
/// `parameters` must be initialised.
unsafe fn process_vm_options(parameters: *mut VMParameters) -> VMErrorCode {
    // SAFETY: delegated to the caller.
    unsafe {
        let vector = &(*parameters).vmParameters;
        let mut i = 1usize;
        let count = vector.count as usize;
        while i < count {
            let param = *vector.parameters.add(i);
            if param.is_null() {
                break;
            }
            let param_bytes = arg_bytes(param);

            if param_bytes.first() != Some(&b'-') {
                i += 1;
                continue;
            }

            #[cfg(target_vendor = "apple")]
            if param_bytes.starts_with(b"-psn_") {
                // The process serial number OS X passes to applications.
                i += 1;
                continue;
            }

            // Skip the leading dashes.
            let after_first = &param_bytes[1..];
            let name_and_value = after_first.strip_prefix(b"-").unwrap_or(after_first);

            // An option may carry its value as `--name=value`; otherwise the
            // value is the next entry in the vector.
            let equals = name_and_value.iter().position(|b| *b == b'=');
            let name = match equals {
                Some(eq) => &name_and_value[..eq],
                None => name_and_value,
            };
            let mut argument_value: Option<&CStr> = equals.map(|eq| {
                // The value points into `param` itself, just past the '='.
                let offset = param_bytes.len() - name_and_value.len() + eq + 1;
                CStr::from_ptr(param.add(offset))
            });

            let Some(spec) = find_parameter_with_name(name) else {
                logging::message_one_string(
                    LOG_ERROR,
                    c"Invalid or unknown VM parameter %s\n",
                    site!(C_FILE, c"processVMOptions", 704),
                    param,
                );
                vm_printUsageTo(c_stderr().cast::<c_void>());
                return VMErrorCode::VM_ERROR_INVALID_PARAMETER;
            };

            if spec.has_argument {
                if argument_value.is_none() && i + 1 < count {
                    i += 1;
                    let next = *vector.parameters.add(i);
                    if !next.is_null() {
                        argument_value = Some(CStr::from_ptr(next));
                    }
                }
                if argument_value.is_none() {
                    logging::message_one_string(
                        LOG_ERROR,
                        c"VM parameter %s requires a value\n",
                        site!(C_FILE, c"processVMOptions", 724),
                        param,
                    );
                    vm_printUsageTo(c_stderr().cast::<c_void>());
                    return VMErrorCode::VM_ERROR_INVALID_PARAMETER_VALUE;
                }
            }

            if let Some(function) = spec.function {
                let error = function(argument_value, &mut *parameters);
                if error != VMErrorCode::VM_SUCCESS {
                    return error;
                }
            }

            i += 1;
        }
    }
    VMErrorCode::VM_SUCCESS
}

/// Parses `argv` into `parameters`.
///
/// # Safety
///
/// `argv` must have `argc` entries and `parameters` must have been through
/// [`vm_parameters_init`].
#[no_mangle]
pub unsafe extern "C" fn vm_parameters_parse(
    argc: c_int,
    argv: *const *const c_char,
    parameters: *mut VMParameters,
) -> VMErrorCode {
    // SAFETY: delegated to the caller.
    unsafe {
        let error = fill_up_image_name(argc, argv, parameters);
        if error != VMErrorCode::VM_SUCCESS {
            return error;
        }

        let error = split_vm_and_image_parameters(argc, argv, parameters);
        if error != VMErrorCode::VM_SUCCESS {
            return error;
        }

        // The VM's own location, from argv[0].
        let mut full_path_buffer = [0u8; FILENAME_MAX];
        let args = argv_slice(argc, argv);
        let executable = args.first().copied().unwrap_or(core::ptr::null());
        let full_path = c_api::get_full_path(
            executable,
            full_path_buffer.as_mut_ptr().cast::<c_char>(),
            FILENAME_MAX as c_int,
        );
        // getFullPath answers null when realpath fails; the C passed that
        // straight to setVMPath, which strcpy'd from it.
        c_api::set_vm_path(full_path);

        let error = process_vm_options(parameters);
        if error != VMErrorCode::VM_SUCCESS {
            vm_parameters_destroy(parameters);
            return error;
        }

        log_parameters(parameters);
    }
    VMErrorCode::VM_SUCCESS
}

/// Zeroes a [`VMParameters`] before parsing into it.
///
/// # Safety
///
/// `parameters` must point to writable storage for a [`VMParameters`].
#[no_mangle]
pub unsafe extern "C" fn vm_parameters_init(parameters: *mut VMParameters) -> VMErrorCode {
    // SAFETY: delegated to the caller. The C assigned every field explicitly;
    // zeroing sets each to the same value, and covers the fields it forgot
    // (processArgc, processArgv, environmentVector), which the caller fills in
    // afterwards.
    unsafe { core::ptr::write_bytes(parameters, 0, 1) };
    VMErrorCode::VM_SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// The last level passed to `logLevel`.
        static LOG_LEVEL: Cell<c_int> = const { Cell::new(0) };
    }

    /// Answers [`super::c_api::log_level`] during tests.
    pub(super) fn record_log_level(level: c_int) {
        LOG_LEVEL.with(|l| l.set(level));
    }

    /// Parses a byte size the way the option handlers do.
    fn size(text: &CStr) -> c_longlong {
        // SAFETY: the literal is NUL-terminated and outlives the call.
        unsafe { parseByteSize(text.as_ptr()) }
    }

    /// The error `parseByteSize` shares with its result type.
    const INVALID: c_longlong = -6;

    #[test]
    fn byte_sizes_take_a_k_m_or_g_suffix() {
        assert_eq!(size(c"0"), 0);
        assert_eq!(size(c"2"), 2);
        assert_eq!(size(c"2k"), 2 * 1024);
        assert_eq!(size(c"2K"), 2 * 1024);
        assert_eq!(size(c"2m"), 2 * 1024 * 1024);
        assert_eq!(size(c"2M"), 2 * 1024 * 1024);
        assert_eq!(size(c"2g"), 2 * 1024 * 1024 * 1024);
        assert_eq!(size(c"2G"), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn an_unrecognised_suffix_is_ignored_rather_than_rejected() {
        // strtoll stops at the first character it cannot use, so the trailing
        // junk is dropped and the number in front of it is accepted. That is
        // the C's behaviour, not a design decision worth defending.
        assert_eq!(size(c"2x"), 2);
        assert_eq!(size(c"12abc"), 12);

        // The suffix is taken from the *last* character of the whole string,
        // and only then is the rest parsed. So a space before the suffix is
        // silently accepted and the multiplier still applies -- "2 M" is two
        // megabytes, not two bytes.
        assert_eq!(size(c"2 M"), 2 * 1024 * 1024);

        // And a suffix with nothing numeric in front of it is zero, times the
        // multiplier, which is still zero.
        assert_eq!(size(c"abcM"), 0);
    }

    #[test]
    fn a_negative_size_is_rejected() {
        assert_eq!(size(c"-1"), INVALID);
        assert_eq!(size(c"-1M"), INVALID);
    }

    #[test]
    fn text_with_no_digits_parses_as_zero() {
        // strtoll answers 0 without setting errno, so the C accepted this as a
        // size of zero rather than reporting it. Every caller then stores 0,
        // which the VM reads as "use the default".
        assert_eq!(size(c""), 0);
        assert_eq!(size(c"abc"), 0);
        assert_eq!(size(c"M"), 0);
    }

    #[test]
    fn arity_is_taken_from_the_option_table() {
        // A value-taking option consumes the next argument...
        assert_eq!(find_parameter_arity(b"--logLevel"), 1);
        assert_eq!(find_parameter_arity(b"-logLevel"), 1);
        assert_eq!(find_parameter_arity(b"--edenSize"), 1);

        // ...unless the value is already attached.
        assert_eq!(find_parameter_arity(b"--logLevel=4"), 0);

        // Flags never do.
        assert_eq!(find_parameter_arity(b"--headless"), 0);
        assert_eq!(find_parameter_arity(b"--help"), 0);

        // Neither does anything the table does not know, which is how an
        // unknown option still lets the image name be found.
        assert_eq!(find_parameter_arity(b"--nosuchoption"), 0);
        assert_eq!(find_parameter_arity(b"notanoption"), 0);
        assert_eq!(find_parameter_arity(b"-"), 0);
    }

    /// Runs [`find_image_name_index`] over a borrowed argv.
    fn image_index(args: &[&CStr]) -> c_int {
        let raw: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        // SAFETY: `raw` outlives the call and has exactly `args.len()` entries.
        unsafe { find_image_name_index(raw.len() as c_int, raw.as_ptr()) }
    }

    #[test]
    fn the_image_is_the_first_argument_that_is_not_an_option() {
        // argv[0] is skipped.
        assert_eq!(image_index(&[c"pharo", c"my.image", c"eval", c"1"]), 1);
        assert_eq!(image_index(&[c"pharo", c"--headless", c"my.image"]), 2);

        // A value-taking option swallows the next argument, so it is not the
        // image. Getting this wrong would make `--logLevel 4 x.image` load an
        // image called "4".
        assert_eq!(
            image_index(&[c"pharo", c"--logLevel", c"4", c"my.image"]),
            3
        );
        assert_eq!(image_index(&[c"pharo", c"--logLevel=4", c"my.image"]), 2);
    }

    #[test]
    fn a_bare_double_dash_ends_the_vm_arguments() {
        // No image is named, so the default is used and everything after the
        // marker goes to the image.
        assert_eq!(
            image_index(&[c"pharo", c"--headless", c"--", c"eval", c"1"]),
            2
        );
    }

    #[test]
    fn an_argv_with_no_image_answers_argc() {
        assert_eq!(image_index(&[c"pharo"]), 1);
        assert_eq!(image_index(&[c"pharo", c"--headless"]), 2);
        assert_eq!(image_index(&[c"pharo", c"--logLevel", c"4"]), 3);
    }

    #[test]
    fn strtol_like_matches_the_c_conversion() {
        assert_eq!(strtol_like(Some(c"4")), 4);
        assert_eq!(strtol_like(Some(c"-4")), -4);
        assert_eq!(strtol_like(Some(c"+4")), 4);
        assert_eq!(strtol_like(Some(c"  7")), 7);
        // Trailing junk stops the scan rather than failing it.
        assert_eq!(strtol_like(Some(c"4abc")), 4);
        // No digits at all is 0, which is why --logLevel rejects 0 and
        // garbage with the same message.
        assert_eq!(strtol_like(Some(c"abc")), 0);
        assert_eq!(strtol_like(Some(c"")), 0);
        assert_eq!(strtol_like(None), 0);
    }

    #[test]
    fn the_usage_text_names_the_vm_and_ends_without_padding() {
        let text = usage_text();
        assert!(text.starts_with(&format!("Usage: {} [<option>...]", vm_name())));
        assert!(text.ends_with("Precede <arguments> by `--' to use default image.\n"));

        // The two quirks that a well-meaning tidy-up would silently remove.
        assert!(
            text.contains("indexable object\"")
                || text.contains("indexable object  --minPermSpaceSize"),
            "the missing newline after --maxSlotsForNewSpaceAlloc is preserved"
        );
        assert!(text.contains('\t'), "the stray tabs are preserved");
    }

    #[test]
    fn the_usage_text_lists_every_option_that_has_a_handler() {
        // Not every spec appears -- `h`, `interactive` and `vm-display-null`
        // are deliberately undocumented -- but anything that takes a value
        // should be findable by a reader of --help.
        let text = usage_text();
        for spec in PARAMETER_SPECS.iter().filter(|s| s.has_argument) {
            assert!(
                text.contains(&format!("--{}", spec.name)),
                "--{} takes a value but is not in the usage text",
                spec.name
            );
        }
    }
}
