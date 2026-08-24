//! Replaces `src/client.c` on Unix, except on 32-bit x86 and PowerPC.
//!
//! The VM's startup path. `vm_main` is what `src/unix/unixMain.c` calls: parse
//! the command line, pick an image, hand the sizes to the interpreter, read
//! the image in, and start interpreting. `interpret()` does not return, so
//! everything here runs exactly once.
//!
//! # Scope
//!
//! The C opens with two architecture-specific floating-point setup macros:
//!
//! * `fldcw(0x12bf)` sets the x87 control word -- signed infinity, round to
//!   nearest, REAL8 precision, interrupts and signals off. Compiled only for
//!   32-bit x86, since x86-64 uses SSE and has no x87 control word to set.
//! * `mtfsfi(0)` does the equivalent on PowerPC.
//!
//! Everywhere else both expand to nothing. Rather than write inline assembly
//! that no test here can exercise -- and getting the x87 precision control
//! wrong changes float results rather than crashing -- `cmake/rust.cmake`
//! keeps the C on those two architectures. Windows and Apple keep it too: the
//! Apple build reaches `vm_main` through a different front end.
//!
//! # Faithful oddities
//!
//! * `vm_main_with_parameters` prints the usage and answers **0** when no
//!   image was found, so "I could not find an image" is a successful exit.
//!   The `logError` that would have explained it is commented out in the C.
//! * `loadPharoImage` reports a missing image with `logErrorFromErrno`, but
//!   nothing set `errno`: the check is `sqImageFileExists`, and a `stat` that
//!   answered "no" leaves whatever errno was already there. The message is
//!   therefore usually unrelated to the failure.
//! * `vm_init` answers `loadPharoImage`'s result, which is `false` (0) rather
//!   than an error code when the image is missing, and `runVMThread` turns
//!   that into a `(void *)-1` nobody reads.
//! * `LOG_SIZEOF` prints each size with `%ld`, so the eight startup lines
//!   report `long`s. Reproduced, since those lines are in every debug log.

use core::ffi::{c_char, c_int, c_void, CStr};

use pharo_vm_sys::{sqInt, usqInt, VMErrorCode, VMParameters};

/// `sqLong` from `memoryAccess.h`, the VM's at-least-64-bit signed integer.
///
/// A `#define` rather than a typedef, so bindgen cannot emit it. It is `long`
/// when `SIZEOF_LONG` is 8 and `long long` otherwise, which is exactly what
/// these two aliases say -- and a `const` assertion below keeps them honest.
#[cfg(target_pointer_width = "64")]
type SqLong = core::ffi::c_long;
/// See the 64-bit definition above.
#[cfg(not(target_pointer_width = "64"))]
type SqLong = core::ffi::c_longlong;

const _: () = assert!(
    core::mem::size_of::<SqLong>() >= 8,
    "sqLong must hold at least 64 bits"
);

use crate::logging::{self, site, LOG_DEBUG, LOG_ERROR, LOG_INFO};

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/client.c";

/// `FILENAME_MAX` from stdio.h.
const FILENAME_MAX: usize = pharo_vm_sys::FILENAME_MAX as usize;

extern "C" {
    /// Serves the worker queue on the main thread. Declared by
    /// `pharoClient.h`, defined per platform.
    #[cfg(pharo_vm_in_worker_thread)]
    fn runMainThreadWorker() -> sqInt;
    /// Sets the old-space limit. Declared here because no header declares it;
    /// `client.c` did the same.
    fn setMaxOldSpaceSize(limit: usqInt) -> sqInt;
    /// Sets the JIT's code-zone size.
    fn setDesiredCogCodeSize(size: sqInt);
    /// Sets the scavenger's eden size.
    fn setDesiredEdenBytes(bytes: SqLong);
    /// Sets the minimum permanent-space size.
    fn setMinimalPermSpaceSize(min: sqInt);
    /// Sets the size of one stack page.
    fn setDesiredStackPageBytes(bytes: SqLong);
    /// Switches off the search for a segment that already has pinned objects.
    fn setAvoidSearchingSegmentsWithPinnedObjects(value: sqInt);
    /// Caps how many slots a single young indexable object may take.
    fn setMaxSlotsForNewSpaceAlloc(value: usqInt);
}

/// Non-zero when the VM is running on a worker thread rather than the main
/// one. Always 0 unless the build has `PHARO_VM_IN_WORKER_THREAD`.
#[no_mangle]
pub static mut vmRunOnWorkerThread: c_int = 0;

/// Answers [`vmRunOnWorkerThread`].
///
/// The C's comment asks for this to move into the parameters struct; it has
/// not.
#[no_mangle]
pub extern "C" fn isVMRunOnWorkerThread() -> c_int {
    // SAFETY: written once during startup, before any other thread exists.
    unsafe { vmRunOnWorkerThread }
}

/// Initialises the VM from `parameters` and reads the image in.
///
/// Answers non-zero on success. The order matters: the interpreter's globals
/// have to exist before the sizes are set, and the sizes before the image is
/// read, because reading it allocates the spaces those sizes describe.
///
/// # Safety
///
/// `parameters` must point to a parsed [`VMParameters`], and this must run
/// once, on the thread that will interpret.
#[no_mangle]
pub unsafe extern "C" fn vm_init(parameters: *mut VMParameters) -> c_int {
    // SAFETY: delegated to the caller. Every call below is an interpreter or
    // platform entry point taking scalars or the strings in `parameters`.
    unsafe {
        pharo_vm_sys::initGlobalStructure();

        // The x87 and PowerPC floating-point setup lives here in the C; see
        // the module docs on why this file is not compiled for those targets.

        pharo_vm_sys::ioInitTime();

        // Record the interpreter's thread -- in worker mode vm_init runs on
        // the spawned VM thread, not the main one. The crash reporter's
        // runningInVMThread (src/unix/debugUnix.c) compares against this to
        // decide whether to dump the Smalltalk stacks. The C's
        // `ioCurrentOSThread()` is `pthread_self()` on every Unix
        // (include/pharovm/unix/sqPlatformSpecific.h).
        #[cfg(pharo_vm_in_worker_thread)]
        {
            crate::external_semaphores::ioVMThread = libc::pthread_self();
        }

        pharo_vm_sys::ioInitExternalSemaphores();

        pharo_vm_sys::setMaxStacksToPrint((*parameters).maxStackFramesToPrint as sqInt);
        setMaxOldSpaceSize((*parameters).maxOldSpaceSize as usqInt);
        setDesiredEdenBytes((*parameters).edenSize as SqLong);
        setMinimalPermSpaceSize((*parameters).minPermSpaceSize as sqInt);
        setDesiredStackPageBytes((*parameters).stackPageSize as SqLong);
        setMaxSlotsForNewSpaceAlloc((*parameters).maxSlotsForNewSpaceAlloc as usqInt);
        setAvoidSearchingSegmentsWithPinnedObjects(sqInt::from(
            (*parameters).avoidSearchingSegmentsWithPinnedObjects,
        ));

        if (*parameters).maxCodeSize > 0 {
            #[cfg(not(cogvm))]
            logging::message_no_args(
                LOG_ERROR,
                c"StackVM does not accept maxCodeSize",
                site!(C_FILE, c"vm_init", 82),
            );
            #[cfg(cogvm)]
            {
                logging::message_one_long(
                    LOG_INFO,
                    c"Setting codeSize to: %ld",
                    site!(C_FILE, c"vm_init", 84),
                    (*parameters).maxCodeSize as core::ffi::c_long,
                );
                setDesiredCogCodeSize((*parameters).maxCodeSize as sqInt);
            }
        }

        pharo_vm_sys::aioInit();

        pharo_vm_sys::setPharoCommandLineParameters(
            (*parameters).vmParameters.parameters,
            (*parameters).vmParameters.count as c_int,
            (*parameters).imageParameters.parameters,
            (*parameters).imageParameters.count as c_int,
        );

        load_pharo_image((*parameters).imageFileName)
    }
}

/// Enters the interpreter. Does not return.
#[no_mangle]
pub extern "C" fn vm_run_interpreter() {
    // SAFETY: the interpreter has been initialised by vm_init, and interpret()
    // runs until the image quits the process.
    unsafe {
        pharo_vm_sys::interpret();
    }
}

/// Reads the image named `file_name` and records its full path.
///
/// # Safety
///
/// `file_name` must be a NUL-terminated string.
unsafe fn load_pharo_image(file_name: *mut c_char) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe {
        if !crate::parameters::image_file_exists(file_name) {
            // errno is whatever it happened to be; see the module docs.
            logging::error_from_errno(
                c"Image file not found",
                site!(C_FILE, c"loadPharoImage", 218),
            );
            return 0;
        }

        pharo_vm_sys::readImageNamed(file_name);

        let mut full_image_name = [0u8; FILENAME_MAX];
        let resolved = pharo_vm_sys::getFullPath(
            file_name,
            full_image_name.as_mut_ptr().cast::<c_char>(),
            FILENAME_MAX as c_int,
        );
        // getFullPath answers null when realpath fails, and setImageName
        // strcpy's from it. The C had the same hole.
        pharo_vm_sys::setImageName(resolved);

        1
    }
}

/// Initialises and then interprets, on whichever thread called it.
///
/// # Safety
///
/// As [`vm_init`].
unsafe fn run_vm_thread(parameters: *mut VMParameters) {
    // SAFETY: delegated to the caller.
    unsafe {
        if vm_init(parameters) == 0 {
            logging::message_one_string(
                LOG_ERROR,
                c"Error opening image file: %s\n",
                site!(C_FILE, c"runVMThread", 239),
                (*parameters).imageFileName,
            );
            return;
        }

        pharo_vm_sys::registerCurrentThreadToHandleExceptions();
        vm_run_interpreter();
    }
}

/// Runs the VM on the calling thread. Always answers 0.
///
/// # Safety
///
/// As [`vm_init`].
unsafe fn run_on_main_thread(parameters: *mut VMParameters) -> c_int {
    logging::message_no_args(
        LOG_DEBUG,
        c"Running VM on main thread\n",
        site!(C_FILE, c"runOnMainThread", 253),
    );
    // SAFETY: delegated to the caller.
    unsafe { run_vm_thread(parameters) };
    0
}

/// Runs the VM on a new thread with four times the main thread's stack, and
/// serves the worker queue on this one.
///
/// The VM's own thread cannot grow its stack the way the main thread can, so
/// the size is taken from the main thread's attributes and multiplied.
///
/// # Safety
///
/// As [`vm_init`].
#[cfg(pharo_vm_in_worker_thread)]
unsafe fn run_on_worker_thread(parameters: *mut VMParameters) -> c_int {
    logging::message_no_args(
        LOG_DEBUG,
        c"Running VM on worker thread\n",
        site!(C_FILE, c"runOnWorkerThread", 266),
    );

    /// The trampoline pthread_create needs.
    extern "C" fn thread_main(p: *mut c_void) -> *mut c_void {
        // SAFETY: `p` is the VMParameters handed to pthread_create, which
        // outlives the process.
        unsafe { run_vm_thread(p.cast::<VMParameters>()) };
        core::ptr::null_mut()
    }

    // SAFETY: the attribute object is initialised before use, and every call
    // below is checked.
    unsafe {
        let mut attr = core::mem::zeroed::<libc::pthread_attr_t>();
        libc::pthread_attr_init(&mut attr);

        let mut size: usize = 0;
        libc::pthread_attr_getstacksize(&attr, &mut size);

        logging::message_one_long(
            LOG_DEBUG,
            c"Stack size: %ld\n",
            site!(C_FILE, c"runOnWorkerThread", 277),
            size as core::ffi::c_long,
        );

        if libc::pthread_attr_setstacksize(&mut attr, size * 4) != 0 {
            libc::perror(c"Setting thread stack size".as_ptr());
            libc::exit(-1);
        }

        let mut thread_id: libc::pthread_t = 0;
        if libc::pthread_create(
            &mut thread_id,
            &attr,
            thread_main,
            parameters.cast::<c_void>(),
        ) != 0
        {
            libc::perror(c"Spawning the VM thread".as_ptr());
            libc::exit(-1);
        }

        libc::pthread_detach(thread_id);
    }

    // SAFETY: declared by pharoClient.h; serves the worker queue and does not
    // return until the VM quits.
    unsafe { runMainThreadWorker() as c_int }
}

/// Starts the VM from an already-parsed [`VMParameters`].
///
/// # Safety
///
/// `parameters` must point to a parsed [`VMParameters`] that outlives the
/// call -- which, since the interpreter does not return, means the process.
#[no_mangle]
pub unsafe extern "C" fn vm_main_with_parameters(parameters: *mut VMParameters) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe {
        // Some cases need an explicit --interactive added for the image.
        if crate::parameters::vm_parameters_ensure_interactive_image_parameter(parameters)
            != VMErrorCode::VM_SUCCESS
        {
            return 1;
        }

        if (*parameters).isDefaultImage && !(*parameters).defaultImageFound {
            // Answering 0 here means "no image found" exits successfully. The
            // C's explanatory logError is commented out; see the module docs.
            crate::parameters::vm_printUsageTo(c_stdout().cast::<c_void>());
            return 0;
        }

        pharo_vm_sys::installErrorHandlers();

        pharo_vm_sys::setProcessArguments((*parameters).processArgc, (*parameters).processArgv);
        pharo_vm_sys::setProcessEnvironmentVector((*parameters).environmentVector);

        logging::message_one_string(
            LOG_INFO,
            c"Opening Image: %s\n",
            site!(C_FILE, c"vm_main_with_parameters", 123),
            (*parameters).imageFileName,
        );

        // Cached because working the machine-code location out is expensive.
        pharo_vm_sys::osCogStackPageHeadroom();

        let mut working_directory = [0u8; FILENAME_MAX + 1];
        let error = crate::path_utilities::vm_path_get_current_working_dir_into(
            working_directory.as_mut_ptr().cast::<c_char>(),
            working_directory.len(),
        );
        if error != VMErrorCode::VM_SUCCESS {
            logging::message_one_string(
                LOG_ERROR,
                c"Failed to obtain the current working directory: %s\n",
                site!(C_FILE, c"vm_main_with_parameters", 141),
                crate::error_code::vm_error_code_to_string(error),
            );
            return 1;
        }

        logging::message_one_string(
            LOG_DEBUG,
            c"Working Directory %s",
            site!(C_FILE, c"vm_main_with_parameters", 145),
            working_directory.as_ptr().cast::<c_char>(),
        );

        log_type_sizes();

        #[cfg(pharo_vm_in_worker_thread)]
        {
            vmRunOnWorkerThread = c_int::from((*parameters).isWorker);
            if vmRunOnWorkerThread != 0 {
                return run_on_worker_thread(parameters);
            }
        }

        run_on_main_thread(parameters)
    }
}

/// The eight `LOG_SIZEOF` lines the C emits at startup.
///
/// The macro expands to `logDebug("sizeof(" #expr "): %ld", sizeof(expr))`, so
/// each line carries the *source text* of the type and a `long`. Reproduced
/// literally, including that `sizeof` answers a `size_t` printed as `%ld`.
fn log_type_sizes() {
    use core::ffi::{c_long, c_longlong};

    let sizes: [(&'static CStr, usize, c_int); 8] = [
        (c"sizeof(int): %ld", core::mem::size_of::<c_int>(), 147),
        (c"sizeof(long): %ld", core::mem::size_of::<c_long>(), 148),
        (
            c"sizeof(long long): %ld",
            core::mem::size_of::<c_longlong>(),
            149,
        ),
        (
            c"sizeof(void*): %ld",
            core::mem::size_of::<*const c_void>(),
            150,
        ),
        (c"sizeof(sqInt): %ld", core::mem::size_of::<sqInt>(), 151),
        (c"sizeof(sqLong): %ld", core::mem::size_of::<SqLong>(), 152),
        (c"sizeof(float): %ld", core::mem::size_of::<f32>(), 153),
        (c"sizeof(double): %ld", core::mem::size_of::<f64>(), 154),
    ];

    for (fmt, size, line) in sizes {
        logging::message_one_long(
            LOG_DEBUG,
            fmt,
            site!(C_FILE, c"vm_main_with_parameters", line),
            size as core::ffi::c_long,
        );
    }
}

/// `stdout`, which the `libc` crate does not expose.
mod c_stdout_stream {
    extern "C" {
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
            link_name = "__stdoutp"
        )]
        static mut stdout: *mut libc::FILE;
    }

    /// The `FILE *` that C's `stdout` macro evaluates to.
    pub fn get() -> *mut libc::FILE {
        // SAFETY: initialised by the C runtime before main.
        unsafe { stdout }
    }
}

use c_stdout_stream::get as c_stdout;

/// The VM's entry point, called from `src/unix/unixMain.c`.
///
/// # Safety
///
/// `argv` must have `argc` entries and `env` must be a NULL-terminated array;
/// both must outlive the process, since the interpreter does not return.
#[no_mangle]
pub unsafe extern "C" fn vm_main(
    argc: c_int,
    argv: *const *const c_char,
    env: *const *const c_char,
) -> c_int {
    let mut parameters = core::mem::MaybeUninit::<VMParameters>::uninit();

    // SAFETY: vm_parameters_init writes every byte of the struct before
    // anything reads it.
    unsafe {
        crate::parameters::vm_parameters_init(parameters.as_mut_ptr());
        let parameters = parameters.assume_init_mut();

        parameters.environmentVector = env as *mut *const c_char;
        parameters.processArgc = argc;
        parameters.processArgv = argv as *mut *const c_char;

        let error = crate::parameters::vm_parameters_parse(argc, argv, parameters);
        if error != VMErrorCode::VM_SUCCESS {
            return if error == VMErrorCode::VM_ERROR_EXIT_WITH_SUCCESS {
                0
            } else {
                1
            };
        }

        // An interactive session with no image gets a file chooser, on the
        // platforms that have one.
        if parameters.isInteractiveSession
            && parameters.isDefaultImage
            && !parameters.defaultImageFound
            && !pharo_vm_sys::vm_file_dialog_is_nop()
        {
            let mut dialog = core::mem::zeroed::<pharo_vm_sys::VMFileDialog>();
            dialog.title = c"Select Pharo Image to Open".as_ptr();
            dialog.message = c"Choose an image file to execute".as_ptr();
            dialog.filterDescription = c"Pharo Images (*.image)".as_ptr();
            dialog.filterExtension = c".image".as_ptr();
            dialog.defaultFileNameAndPath =
                CStr::from_bytes_with_nul(pharo_vm_sys::DEFAULT_IMAGE_NAME)
                    .expect("NUL-terminated macro")
                    .as_ptr();

            // The C ignored the error code and looked only at `succeeded`.
            pharo_vm_sys::vm_file_dialog_run_modal_open(&mut dialog);
            if !dialog.succeeded {
                pharo_vm_sys::vm_file_dialog_destroy(&mut dialog);
                return 0;
            }

            parameters.imageFileName = libc::strdup(dialog.selectedFileName);
            parameters.isDefaultImage = false;
            pharo_vm_sys::vm_file_dialog_destroy(&mut dialog);
        }

        let exit_code = vm_main_with_parameters(parameters);
        crate::parameters::vm_parameters_destroy(parameters);
        exit_code
    }
}
