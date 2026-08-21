//! Generates the raw bindings in `wrapper.h` against *this build's* headers.
//!
//! The bindings are deliberately not checked in. `sqInt` is not a fixed type:
//! `include/pharovm/common/memoryAccess.h` picks `int`, `long` or `long long`
//! depending on `SQ_IMAGE32`, `SQ_HOST64` and `SIZEOF_LONG`, all of which come
//! from the `config.h` that CMake generates per configuration. Checked-in
//! bindings would silently be wrong for 32-bit or for a host where
//! `sizeof(long) != 8`.
//!
//! CMake therefore hands us the exact include paths and preprocessor
//! definitions it uses for the C sources; see `cmake/rust.cmake`.

use std::env;
use std::path::PathBuf;

/// Separator used by `cmake/rust.cmake` when packing CMake lists into
/// environment variables. Not CMake's native `;`, which would need escaping to
/// survive the trip through the environment; `|` never occurs in include paths
/// or preprocessor definitions.
const LIST_SEP: char = '|';

/// Types and functions Rust is allowed to see. Extend per wave, and keep it
/// tight: an unrestricted bindgen run over `pharo.h` pulls in the whole libc
/// and generated-interpreter surface.
const ALLOWED_TYPES: &[&str] = &[
    // Wave 0
    "VMErrorCode",
    // The interpreter proxy: the plugin ABI, and needed by the sqNamedPrims /
    // sqVirtualMachine work in Wave 2.
    "VirtualMachine",
    // Wave 1. Shared field-for-field with parameters.c and parameters.m, which
    // are still C, so the layout is a live ABI.
    "VMParameterVector",
    // Wave 2. `defaultFileAccessHandler` is filled in by Rust but read and
    // called through by the generated interpreter (the `sqImageFile*` macros in
    // imageAccess.h expand there), so the layout is a live ABI too.
    "FileAccessHandler",
    // The width of the VM's object-pointer-sized integer is decided by
    // config.h; never assume it, always take it from the headers.
    "sqInt",
    // Wave 5. Passed by pointer between this crate and the still-C FFI worker
    // and callback code, so the layout is a live ABI.
    "Semaphore",
    // Wave 7. The unsigned counterpart of sqInt, used for heap sizes and
    // addresses in the memory-mapping entry points the interpreter calls.
    "usqInt",
    // Wave 9. Filled in here and read by the generated interpreter and by
    // src/client.c, so the layout is a live ABI.
    "VMParameters",
];

/// Only functions Rust *calls into C* belong here.
///
/// A function this workspace **implements** (i.e. that `pharo-platform`
/// exports with `#[no_mangle]` to replace a `src/*.c` definition) must never be
/// allowlisted: binding it would emit an `extern "C"` declaration of a symbol
/// we also define, which is at best redundant and at worst a signature
/// mismatch the linker cannot catch. `vm_error_code_to_string` is the Wave 0
/// example -- deliberately absent.
const ALLOWED_FUNCTIONS: &[&str] = &[
    // Wave 2. `src/debug.c` stays C, and a port that stopped logging would be
    // a behaviour change; these are the two entry points behind the logError /
    // logErrorFromErrno macros.
    "logMessage",
    "logMessageFromErrno",
    // Wave 3. Both live in src/utils.c, which is still C. They return
    // NULL-terminated `char **` arrays owned by the callee.
    "getPluginPaths",
    "getSystemSearchPaths",
    // Wave 4. `error` logs and then aborts; it is declared void in C but never
    // returns.
    "error",
    // Wave 5. Both are in the generated interpreter: signalSemaphoreWithIndex
    // queues a signal for the image's semaphore at that index, and `failed`
    // reports whether the last primitive failed.
    "signalSemaphoreWithIndex",
    "failed",
    // Wave 8. The interpreter proxy handed to each plugin's setInterpreter.
    "sqGetInterpreterProxy",
    // Wave 9. All still C: logLevel is in src/debug.c, the rest in src/utils.c.
    "logLevel",
    "getVMVersion",
    "getSourceVersion",
    "setVMPath",
    "getFullPath",
];

/// Global C variables. Note that enum *variants* do not belong here: with the
/// `NewType` enum style they are emitted as associated constants on the type
/// itself (`VMErrorCode::VM_SUCCESS`), so allowlisting the type is enough.
const ALLOWED_VARS: &[&str] = &[
    // Wave 9. Both are string macros in the generated config.h, so they cannot
    // be restated in Rust without drifting from the C build's identity.
    "VM_NAME",
    "DEFAULT_IMAGE_NAME",
    // Wave 3. `moduleNameBuffer` is `char[FILENAME_MAX]` and is an exported
    // symbol, so the length has to come from the same stdio.h the C saw.
    "FILENAME_MAX",
];

fn env_list(key: &str) -> Option<Vec<String>> {
    println!("cargo:rerun-if-env-changed={key}");
    let raw = env::var(key).ok()?;
    Some(
        raw.split(LIST_SEP)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");

    let Some(include_dirs) = env_list("PHAROVM_INCLUDE_DIRS") else {
        panic!(
            "PHAROVM_INCLUDE_DIRS is not set.\n\
             \n\
             pharo-vm-sys cannot be built on its own: it binds headers whose types\n\
             depend on the config.h CMake generates, and on the interp.h VMMaker\n\
             generates. Build through CMake instead:\n\
             \n\
             \x20   cmake -S . -B build -DUSE_RUST_PLATFORM=ON && cmake --build build\n\
             \n\
             cmake/rust.cmake exports the include paths and definitions this crate\n\
             needs; running cargo by hand outside that environment cannot work."
        );
    };

    // Definitions arrive as CMake writes them: `NAME` or `NAME=VALUE`, without
    // a leading -D.
    let compile_defs = env_list("PHAROVM_COMPILE_DEFS").unwrap_or_default();

    let mut clang_args: Vec<String> = Vec::new();
    for dir in &include_dirs {
        println!("cargo:rerun-if-changed={dir}");
        clang_args.push(format!("-I{dir}"));
    }
    for def in &compile_defs {
        clang_args.push(format!("-D{def}"));
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        // The allowlists below are what keep the surface small; recursion stays
        // on so that allowlisted functions still get their parameter types.
        .layout_tests(true)
        // This crate is #![no_std]: emit ::core::ffi paths, not ::std::os::raw.
        .use_core()
        // Rust enums cannot soundly represent a C enum that might carry an
        // unlisted value, so use a newtype with associated constants.
        .default_enum_style(bindgen::EnumVariation::NewType {
            is_bitfield: false,
            is_global: false,
        })
        .derive_debug(true)
        .derive_default(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    let bindings = ALLOWED_TYPES
        .iter()
        .fold(bindings, |b, t| b.allowlist_type(t));
    let bindings = ALLOWED_FUNCTIONS
        .iter()
        .fold(bindings, |b, f| b.allowlist_function(f));
    let bindings = ALLOWED_VARS
        .iter()
        .fold(bindings, |b, v| b.allowlist_var(v));

    let bindings = bindings.generate().unwrap_or_else(|e| {
        panic!(
            "bindgen failed against the VM headers: {e}\n\
             clang args were: {}\n\
             \n\
             The usual cause is that the generated interpreter headers are missing:\n\
             pharo.h includes the VMMaker-generated interp.h. Build the C sources\n\
             first (target `generate-sources`), or point the build at a\n\
             pre-generated tree via GENERATED_SOURCE_DIR.",
            clang_args.join(" ")
        )
    });

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR always set by cargo"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("could not write bindings.rs into OUT_DIR");
}
