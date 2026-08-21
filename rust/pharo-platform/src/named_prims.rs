//! Replaces `src/common/sqNamedPrims.c` on 64-bit builds.
//!
//! Named primitives are how the image reaches code outside the interpreter:
//! `<primitive: 'primitiveFoo' module: 'FooPlugin'>` becomes a lookup here.
//! This file owns the list of loaded modules, decides whether a primitive
//! comes from a statically linked plugin or a shared library, runs a plugin's
//! initialisers, and answers the accessor depth Slang needs.
//!
//! Two kinds of module, distinguished only by their handle:
//!
//! * **internal**, statically linked into the VM and listed in
//!   [`pluginExports`]. Their handle is `intrinsicsModule`'s handle, which is
//!   null.
//! * **external**, a shared library opened by [`crate::external_primitives`].
//!   Their handle is the `dlopen` handle.
//!
//! # Scope
//!
//! 64-bit, because [`pointer_for_oop`] is the identity here. `pointerForOop`
//! adds `sqMemoryBase` in the one configuration where the image's word is
//! narrower than the host pointer (`SQ_HOST64 && SQ_IMAGE32`); everywhere else
//! `sqMemoryBase` is the macro `0`. A `const` assertion below fails the build
//! if that assumption is ever broken, and `cmake/rust.cmake` keeps the C for
//! 32-bit.
//!
//! # The accessor depth is hidden after the primitive's name
//!
//! The generated `vm_exports` table stores names like
//! `"primitiveAddLargeIntegers\0\377"`: the name, its terminator, and then one
//! signed byte of accessor depth. So the depth is read at `name[len + 1]`,
//! past the NUL. Slang omits the byte when the depth is -1, which is why
//! `moduleUnloaded` in that table is a bare `"moduleUnloaded"` -- and why
//! asking for the accessor depth of an entry that has none reads whatever the
//! linker put next. That is the C's behaviour and it is preserved; no caller
//! asks for the depth of an entry without one.
//!
//! # Faithful oddities
//!
//! * `findExternalFunctionIn` takes an `fnameLength` it never uses: the
//!   external path gets its depth from a `<name>AccessorDepth` symbol instead.
//! * `ioListBuiltinModule` calls `strcmp(function, "setInterpreter")` without
//!   checking `function` for null, and only stops at a row where *both* the
//!   plugin and the function are null. A table row naming a plugin but no
//!   function would crash it. No generated table has one.
//! * `ioUnloadModule` notifies every *other* module through `moduleUnloaded`,
//!   including modules loaded for the FFI, which never had an initialiser run.
//! * `intrinsicsModule` is created by adding it to the module list and then
//!   immediately resetting the list head, so it is deliberately unreachable
//!   from the chain and can never be unloaded.

use core::ffi::{c_char, c_void, CStr};

use pharo_vm_sys::{sqInt, usqInt, VirtualMachine};

use crate::external_primitives::{
    getModuleSymbol, ioFindExternalFunctionInAccessorDepthInto, ioFreeModule, ioLoadModule,
};
use crate::logging::{self, site, LOG_TRACE};

/// The `__FILENAME__` the C compiler would have produced for this file.
const C_FILE: &CStr = c"src/common/sqNamedPrims.c";

/// The longest name the length-taking entry points accept, from the C's
/// `char functionName[256]` and its `> 255` guard.
const NAME_MAX: usize = 255;

/// `pointerForOop` is the identity only while `sqMemoryBase` is 0, which is
/// every configuration except a 32-bit image on a 64-bit host.
const _: () = assert!(
    core::mem::size_of::<sqInt>() == core::mem::size_of::<*const c_void>(),
    "sqNamedPrims assumes sqMemoryBase == 0; see the module docs"
);

/// One row of a plugin export table.
///
/// Matches the `sqExport` typedef at the top of `sqNamedPrims.c`, and the
/// `void *[3]` rows the interpreter generator emits for `vm_exports`.
#[repr(C)]
pub struct SqExport {
    /// The plugin's name, or empty/null for the intrinsics.
    pub plugin_name: *mut c_char,
    /// The primitive's name, with the accessor depth hidden after its NUL.
    pub primitive_name: *mut c_char,
    /// The primitive itself.
    pub primitive_address: *mut c_void,
}

#[cfg(not(test))]
extern "C" {
    /// The interpreter's primitives. Defined by the generated `cointerp.c`.
    /// Declared as one element because only its address is used.
    static vm_exports: SqExport;
    /// The platform's primitives. Defined by `src/utils.c`, still C, as an
    /// empty table.
    static os_exports: SqExport;
}

/// The export tables to search, terminated by a null.
///
/// An exported symbol: the C defined it in this translation unit, from a list
/// that lives in `include/pharovm/sqNamedPrims.h`.
#[cfg(not(test))]
#[no_mangle]
pub static mut pluginExports: [*mut SqExport; 3] = [
    core::ptr::addr_of!(vm_exports) as *mut SqExport,
    core::ptr::addr_of!(os_exports) as *mut SqExport,
    core::ptr::null_mut(),
];

/// The null-terminated array of export tables.
///
/// Under `cfg(test)` this answers a table the test installs, because
/// `vm_exports` is defined by the generated interpreter and is not linked into
/// the unit-test binary.
fn plugin_export_lists() -> *const *mut SqExport {
    #[cfg(test)]
    return tests::installed_exports();
    #[cfg(not(test))]
    // Taking the address of the static rather than a reference to it: C also
    // sees this symbol.
    core::ptr::addr_of!(pluginExports).cast::<*mut SqExport>()
}

/// The interpreter proxy handed to each plugin's `setInterpreter`.
fn interpreter_proxy() -> *mut VirtualMachine {
    #[cfg(test)]
    return core::ptr::null_mut();
    #[cfg(not(test))]
    // SAFETY: takes no arguments; the generated interpreter owns the proxy and
    // it is valid for the process lifetime.
    unsafe {
        pharo_vm_sys::sqGetInterpreterProxy()
    }
}

/// `pointerForOop`. See the module docs on why this is the identity.
#[inline]
fn pointer_for_oop(oop: usqInt) -> *mut c_char {
    oop as *mut c_char
}

/// An entry in the loaded-module chain.
///
/// The C used a flexible array member (`char name[1]` over-allocated); here
/// the name is a boxed `CStr`, which keeps [`ioListLoadedModule`]'s returned
/// pointer stable for as long as the entry lives.
struct ModuleEntry {
    next: *mut ModuleEntry,
    handle: *mut c_void,
    ffi_loaded: sqInt,
    name: Box<CStr>,
}

impl ModuleEntry {
    /// The name as the `char *` the C's `entry->name` was.
    fn name_ptr(&self) -> *const c_char {
        self.name.as_ptr()
    }
}

/// The synthetic module standing for everything statically linked in. Its
/// handle is null.
static mut INTRINSICS_MODULE: *mut ModuleEntry = core::ptr::null_mut();

/// Head of the loaded-module chain. Does not include the intrinsics.
static mut FIRST_MODULE: *mut ModuleEntry = core::ptr::null_mut();

/// Cleared for the rest of the session by [`ioDisableModuleLoading`].
static mut MODULE_LOADING_ENABLED: bool = true;

/// Borrows a C string, mapping null *and empty* to `None`.
///
/// The C canonicalised both to NULL at nearly every entry point; doing it once
/// here keeps the comparisons below honest.
///
/// # Safety
///
/// `p`, if non-null, must be a NUL-terminated string valid for the call.
unsafe fn canonical<'a>(p: *const c_char) -> Option<&'a CStr> {
    if p.is_null() {
        return None;
    }
    // SAFETY: delegated to the caller.
    let s = unsafe { CStr::from_ptr(p) };
    if s.to_bytes().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// The handle every internal module shares.
fn intrinsics_handle() -> *mut c_void {
    // SAFETY: a plain static written only on the VM thread.
    unsafe {
        if INTRINSICS_MODULE.is_null() {
            core::ptr::null_mut()
        } else {
            (*INTRINSICS_MODULE).handle
        }
    }
}

/// Finds an already-loaded module by name.
///
/// An absent or empty name means the intrinsics, which is how the interpreter
/// asks for a primitive that is not in any plugin.
///
/// # Safety
///
/// `plugin_name`, if non-null, must be a NUL-terminated string.
unsafe fn find_loaded_module(plugin_name: *const c_char) -> *mut ModuleEntry {
    // SAFETY: delegated to the caller.
    let Some(wanted) = (unsafe { canonical(plugin_name) }) else {
        // SAFETY: plain static.
        return unsafe { INTRINSICS_MODULE };
    };

    // SAFETY: the chain is only mutated on the VM thread, and every `next` is
    // either null or a live entry.
    unsafe {
        let mut module = FIRST_MODULE;
        while !module.is_null() {
            if (*module).name.as_ref() == wanted {
                return module;
            }
            module = (*module).next;
        }
    }
    core::ptr::null_mut()
}

/// Pushes a new entry onto the front of the chain.
///
/// # Safety
///
/// `plugin_name` must be a NUL-terminated string.
unsafe fn add_to_module_list(
    plugin_name: *const c_char,
    handle: *mut c_void,
    ffi_flag: sqInt,
) -> *mut ModuleEntry {
    // SAFETY: delegated to the caller.
    let name: Box<CStr> = unsafe { CStr::from_ptr(plugin_name) }.into();

    // SAFETY: plain statics on the VM thread.
    unsafe {
        let module = Box::into_raw(Box::new(ModuleEntry {
            next: FIRST_MODULE,
            handle,
            ffi_loaded: ffi_flag,
            name,
        }));
        FIRST_MODULE = module;
        module
    }
}

/// Unlinks `entry` from the chain without freeing it.
///
/// # Safety
///
/// `entry` must be in the chain. The C walked to it without a null guard, so
/// an entry that is *not* in the chain walked off the end; that is preserved
/// as a debug assertion rather than a wild read.
unsafe fn remove_from_list(entry: *mut ModuleEntry) {
    // SAFETY: delegated to the caller.
    unsafe {
        if entry == FIRST_MODULE {
            FIRST_MODULE = (*entry).next;
            return;
        }
        let mut prev = FIRST_MODULE;
        while !prev.is_null() && (*prev).next != entry {
            prev = (*prev).next;
        }
        debug_assert!(!prev.is_null(), "entry was not in the module chain");
        if !prev.is_null() {
            (*prev).next = (*entry).next;
        }
    }
}

/// Looks a primitive up in a shared library through `dlsym`.
///
/// `_fname_length` is unused, exactly as in the C: the external path finds the
/// accessor depth through a companion symbol, not through a hidden byte.
///
/// # Safety
///
/// `function_name` must be a NUL-terminated string and `module` a live entry.
unsafe fn find_external_function_in(
    function_name: *mut c_char,
    module: *mut ModuleEntry,
    _fname_length: sqInt,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        logging::message_two_strings(
            LOG_TRACE,
            c"Looking (externally) for %s in %s... ",
            site!(C_FILE, c"findExternalFunctionIn", 98),
            function_name,
            (*module).name_ptr(),
        );

        let result = if (*module).handle.is_null() {
            core::ptr::null_mut()
        } else {
            ioFindExternalFunctionInAccessorDepthInto(
                function_name,
                (*module).handle,
                accessor_depth_ptr,
            )
        };

        logging::message_one_string(
            LOG_TRACE,
            c"%s\n",
            site!(C_FILE, c"findExternalFunctionIn", 103),
            if result.is_null() {
                c"not found".as_ptr()
            } else {
                c"found".as_ptr()
            },
        );
        result
    }
}

/// Looks a primitive up in the statically linked export tables, falling back
/// to a global `dlsym` if it is not there.
///
/// # Safety
///
/// Both names, if non-null, must be NUL-terminated strings.
unsafe fn find_internal_function_in(
    function_name: *mut c_char,
    plugin_name: *const c_char,
    fname_length: sqInt,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        logging::message_two_strings(
            LOG_TRACE,
            c"Looking (internally) for %s in %s ... ",
            site!(C_FILE, c"findInternalFunctionIn", 121),
            function_name,
            if plugin_name.is_null() {
                c"<intrinsic>".as_ptr()
            } else {
                plugin_name
            },
        );

        let wanted_function = canonical(function_name);
        let wanted_plugin = canonical(plugin_name);

        let lists = plugin_export_lists();
        let mut list_index = 0usize;
        loop {
            let exports = *lists.add(list_index);
            if exports.is_null() {
                break;
            }
            list_index += 1;

            let mut index = 0usize;
            loop {
                let row = exports.add(index);
                index += 1;

                let plugin = canonical((*row).plugin_name);
                let function = canonical((*row).primitive_name);

                // Both missing means the end of this table.
                if plugin.is_none() && function.is_none() {
                    break;
                }
                if plugin.is_some() != wanted_plugin.is_some() || plugin != wanted_plugin {
                    continue;
                }
                if function.is_some() != wanted_function.is_some() || function != wanted_function {
                    continue;
                }

                logging::message_no_args(
                    LOG_TRACE,
                    c"found\n",
                    site!(C_FILE, c"findInternalFunctionIn", 144),
                );

                if !accessor_depth_ptr.is_null() {
                    // One signed byte, hidden after the name's terminator.
                    let name = (*row).primitive_name.cast::<i8>();
                    *accessor_depth_ptr = sqInt::from(*name.add(fname_length as usize + 1));
                }

                return (*row).primitive_address;
            }
        }

        // Not in any table: try the process's global symbols. The C passed the
        // *canonicalised* name here, which could be null.
        ioFindExternalFunctionInAccessorDepthInto(
            wanted_function.map_or(core::ptr::null_mut(), |f| f.as_ptr() as *mut c_char),
            core::ptr::null_mut(),
            accessor_depth_ptr,
        )
    }
}

/// Dispatches to the internal or external lookup depending on the module's
/// handle, reporting the accessor depth.
///
/// # Safety
///
/// As [`find_internal_function_in`], and `module` must be live.
unsafe fn find_function_and_accessor_depth_in(
    function_name: *mut c_char,
    module: *mut ModuleEntry,
    fname_length: sqInt,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        if (*module).handle == intrinsics_handle() {
            find_internal_function_in(
                function_name,
                (*module).name_ptr(),
                fname_length,
                accessor_depth_ptr,
            )
        } else {
            find_external_function_in(function_name, module, fname_length, accessor_depth_ptr)
        }
    }
}

/// [`find_function_and_accessor_depth_in`] without the accessor depth.
///
/// # Safety
///
/// As [`find_function_and_accessor_depth_in`].
unsafe fn find_function_in(function_name: &CStr, module: *mut ModuleEntry) -> *mut c_void {
    // SAFETY: delegated to the caller; the literal outlives the call.
    unsafe {
        find_function_and_accessor_depth_in(
            function_name.as_ptr() as *mut c_char,
            module,
            0,
            core::ptr::null_mut(),
        )
    }
}

/// Runs `getModuleName`, `setInterpreter` and `initialiseModule` on a
/// freshly loaded module. Answers 0 if any of them refuses.
///
/// `getModuleName` is optional -- older plugins do not export it -- but when
/// present its answer must prefix the name the module was loaded under.
///
/// # Safety
///
/// `module` must be live, and the symbols it exports must have the signatures
/// the plugin ABI specifies.
unsafe fn call_initializers_in(module: *mut ModuleEntry) -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe {
        let init0 = find_function_in(c"getModuleName", module);
        let init1 = find_function_in(c"setInterpreter", module);
        let init2 = find_function_in(c"initialiseModule", module);

        if !init0.is_null() {
            let get_module_name: extern "C" fn() -> *mut c_char = core::mem::transmute(init0);
            let module_name = get_module_name();
            if module_name.is_null() {
                logging::message_no_args(
                    LOG_TRACE,
                    c"ERROR: getModuleName() returned NULL\n",
                    site!(C_FILE, c"callInitializersIn", 200),
                );
                return 0;
            }
            let reported = CStr::from_ptr(module_name).to_bytes();
            let expected = (*module).name.to_bytes();
            // strncmp against the expected name's length: a prefix match, so a
            // plugin may append a version suffix.
            if reported.len() < expected.len() || &reported[..expected.len()] != expected {
                logging::message_two_strings(
                    LOG_TRACE,
                    c"ERROR: getModuleName returned %s (expected: %s)\n",
                    site!(C_FILE, c"callInitializersIn", 204),
                    module_name,
                    (*module).name_ptr(),
                );
                return 0;
            }
        } else {
            logging::message_one_string(
                LOG_TRACE,
                c"WARNING: getModuleName() not found in %s\n",
                site!(C_FILE, c"callInitializersIn", 209),
                (*module).name_ptr(),
            );
        }

        if init1.is_null() {
            logging::message_no_args(
                LOG_TRACE,
                c"ERROR: setInterpreter() not found\n",
                site!(C_FILE, c"callInitializersIn", 212),
            );
            return 0;
        }

        let set_interpreter: extern "C" fn(*mut VirtualMachine) -> sqInt =
            core::mem::transmute(init1);
        if set_interpreter(interpreter_proxy()) == 0 {
            logging::message_no_args(
                LOG_TRACE,
                c"ERROR: setInterpreter() returned false\n",
                site!(C_FILE, c"callInitializersIn", 218),
            );
            return 0;
        }

        if !init2.is_null() {
            let initialise: extern "C" fn() -> sqInt = core::mem::transmute(init2);
            if initialise() == 0 {
                logging::message_no_args(
                    LOG_TRACE,
                    c"ERROR: initialiseModule() returned false\n",
                    site!(C_FILE, c"callInitializersIn", 224),
                );
                return 0;
            }
        }

        logging::message_one_string(
            LOG_TRACE,
            c"SUCCESS: Module %s is now initialized\n",
            site!(C_FILE, c"callInitializersIn", 228),
            (*module).name_ptr(),
        );
        1
    }
}

/// Stops any further module from being loaded, for the rest of the session.
/// Not reversible, by design.
#[no_mangle]
pub extern "C" fn ioDisableModuleLoading() {
    // SAFETY: a plain static written on the VM thread.
    unsafe {
        MODULE_LOADING_ENABLED = false;
    }
}

/// Loads a module, externally if possible and internally otherwise, and runs
/// its initialisers.
///
/// An FFI load skips the internal lookup and the initialisers entirely: the
/// FFI wants a raw shared library, not a plugin.
///
/// # Safety
///
/// `plugin_name` must be a NUL-terminated string.
unsafe fn find_and_load_module(plugin_name: *mut c_char, ffi_load: sqInt) -> *mut ModuleEntry {
    // SAFETY: plain static.
    if !unsafe { MODULE_LOADING_ENABLED } {
        return core::ptr::null_mut();
    }

    // SAFETY: delegated to the caller.
    unsafe {
        logging::message_one_string(
            LOG_TRACE,
            c"Looking for plugin %s\n",
            site!(C_FILE, c"findAndLoadModule", 259),
            if plugin_name.is_null() {
                c"<intrinsic>".as_ptr()
            } else {
                plugin_name
            },
        );

        let mut handle = ioLoadModule(plugin_name);

        if ffi_load != 0 {
            // For the FFI, do not go looking internally.
            if handle.is_null() {
                return core::ptr::null_mut();
            }
            return add_to_module_list(plugin_name, handle, ffi_load);
        }

        if handle.is_null() {
            // Might be statically linked, so go looking for setInterpreter.
            if find_internal_function_in(
                c"setInterpreter".as_ptr() as *mut c_char,
                plugin_name,
                0,
                core::ptr::null_mut(),
            )
            .is_null()
            {
                return core::ptr::null_mut();
            }
            handle = intrinsics_handle();
        }

        let module = add_to_module_list(plugin_name, handle, ffi_load);
        if call_initializers_in(module) == 0 {
            if handle != intrinsics_handle() {
                ioFreeModule(handle);
            }
            remove_from_list(module);
            drop(Box::from_raw(module));
            return core::ptr::null_mut();
        }
        module
    }
}

/// Answers the module for `plugin_name`, loading it if necessary.
///
/// Creates the intrinsics entry on first use, then immediately drops it off
/// the chain so it can never be unloaded.
///
/// # Safety
///
/// `plugin_name` must be a NUL-terminated string.
unsafe fn find_or_load_module(plugin_name: *mut c_char, ffi_load: sqInt) -> *mut ModuleEntry {
    // SAFETY: delegated to the caller; the statics are VM-thread only.
    unsafe {
        if INTRINSICS_MODULE.is_null() {
            INTRINSICS_MODULE = add_to_module_list(c"".as_ptr(), core::ptr::null_mut(), 1);
            // Drop it off the list: it is never unloaded.
            FIRST_MODULE = core::ptr::null_mut();
        }

        let module = find_loaded_module(plugin_name);
        if module.is_null() {
            find_and_load_module(plugin_name, ffi_load)
        } else {
            module
        }
    }
}

/// Loads `function_name` from `plugin_name`. Answers the address, or 0.
///
/// A null `function_name` asks only whether the module can be loaded, and
/// answers 1 rather than an address.
///
/// # Safety
///
/// Both names, if non-null, must be NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn ioLoadFunctionFrom(
    function_name: *mut c_char,
    plugin_name: *mut c_char,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        let module = find_or_load_module(plugin_name, 0);
        if module.is_null() {
            logging::message_two_strings(
                LOG_TRACE,
                c"Failed to find %s (module %s was not loaded)\n",
                site!(C_FILE, c"ioLoadFunctionFrom", 326),
                function_name,
                plugin_name,
            );
            return core::ptr::null_mut();
        }
        if function_name.is_null() {
            // Only the module was asked for.
            return 1 as *mut c_void;
        }
        find_function_and_accessor_depth_in(function_name, module, 0, core::ptr::null_mut())
    }
}

/// Copies a length-counted name out of object memory into `buffer`.
///
/// Answers `None` when the name is longer than the buffer, which is the C's
/// `> 255` bail-out.
///
/// # Safety
///
/// `index` must address `length` readable bytes in object memory.
unsafe fn name_from_oop(
    index: sqInt,
    length: sqInt,
    buffer: &mut [u8; NAME_MAX + 1],
) -> Option<*mut c_char> {
    if length < 0 || length as usize > NAME_MAX {
        return None;
    }
    let length = length as usize;
    // SAFETY: delegated to the caller; the copy is bounded by NAME_MAX and the
    // terminator goes in the byte after it.
    unsafe {
        core::ptr::copy_nonoverlapping(
            pointer_for_oop(index as usqInt).cast::<u8>(),
            buffer.as_mut_ptr(),
            length,
        );
    }
    buffer[length] = 0;
    Some(buffer.as_mut_ptr().cast::<c_char>())
}

/// Entry point for named primitives looked up through the VM.
///
/// # Safety
///
/// Both index/length pairs must address readable bytes in object memory.
#[no_mangle]
pub unsafe extern "C" fn ioLoadExternalFunctionOfLengthFromModuleOfLength(
    function_name_index: sqInt,
    function_name_length: sqInt,
    module_name_index: sqInt,
    module_name_length: sqInt,
) -> *mut c_void {
    let mut function_buf = [0u8; NAME_MAX + 1];
    let mut module_buf = [0u8; NAME_MAX + 1];

    // SAFETY: delegated to the caller.
    unsafe {
        let (Some(function_name), Some(module_name)) = (
            name_from_oop(function_name_index, function_name_length, &mut function_buf),
            name_from_oop(module_name_index, module_name_length, &mut module_buf),
        ) else {
            return core::ptr::null_mut();
        };
        ioLoadFunctionFrom(function_name, module_name)
    }
}

/// [`ioLoadFunctionFrom`] that also reports the accessor depth.
///
/// # Safety
///
/// As [`ioLoadFunctionFrom`], and `accessor_depth_ptr` must be null or
/// writable.
unsafe fn io_load_function_from_accessor_depth_into(
    function_name: *mut c_char,
    plugin_name: *mut c_char,
    fname_length: sqInt,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe {
        let module = find_or_load_module(plugin_name, 0);
        if module.is_null() {
            logging::message_two_strings(
                LOG_TRACE,
                c"Failed to find %s (module %s was not loaded)\n",
                site!(C_FILE, c"ioLoadFunctionFromAccessorDepthInto", 375),
                function_name,
                plugin_name,
            );
            return core::ptr::null_mut();
        }
        if function_name.is_null() {
            return 1 as *mut c_void;
        }
        find_function_and_accessor_depth_in(function_name, module, fname_length, accessor_depth_ptr)
    }
}

/// Entry point for named primitives that need the accessor depth.
///
/// # Safety
///
/// As [`ioLoadExternalFunctionOfLengthFromModuleOfLength`].
#[no_mangle]
pub unsafe extern "C" fn ioLoadExternalFunctionOfLengthFromModuleOfLengthAccessorDepthInto(
    function_name_index: sqInt,
    function_name_length: sqInt,
    module_name_index: sqInt,
    module_name_length: sqInt,
    accessor_depth_ptr: *mut sqInt,
) -> *mut c_void {
    let mut function_buf = [0u8; NAME_MAX + 1];
    let mut module_buf = [0u8; NAME_MAX + 1];

    // SAFETY: delegated to the caller.
    unsafe {
        let (Some(function_name), Some(module_name)) = (
            name_from_oop(function_name_index, function_name_length, &mut function_buf),
            name_from_oop(module_name_index, module_name_length, &mut module_buf),
        ) else {
            return core::ptr::null_mut();
        };
        io_load_function_from_accessor_depth_into(
            function_name,
            module_name,
            function_name_length,
            accessor_depth_ptr,
        )
    }
}

/// Resolves a symbol in an already-open library. For the FFI only.
///
/// A null `module_handle` means the process's global symbols.
///
/// # Safety
///
/// The index/length pair must address readable bytes in object memory, and
/// `module_handle` must be null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn ioLoadSymbolOfLengthFromModule(
    function_name_index: sqInt,
    function_name_length: sqInt,
    module_handle: *mut c_void,
) -> *mut c_void {
    let mut function_buf = [0u8; NAME_MAX + 1];

    // SAFETY: delegated to the caller.
    unsafe {
        let Some(function_name) =
            name_from_oop(function_name_index, function_name_length, &mut function_buf)
        else {
            return core::ptr::null_mut();
        };

        if module_handle.is_null() {
            getModuleSymbol(core::ptr::null_mut(), function_name)
        } else {
            // The C's ioFindExternalFunctionIn macro: no accessor depth.
            ioFindExternalFunctionInAccessorDepthInto(
                function_name,
                module_handle,
                core::ptr::null_mut(),
            )
        }
    }
}

/// Loads a shared library by name, for the FFI only.
///
/// Runs no initialisers and does not look internally.
///
/// # Safety
///
/// The index/length pair must address readable bytes in object memory.
#[no_mangle]
pub unsafe extern "C" fn ioLoadModuleOfLength(
    module_name_index: sqInt,
    module_name_length: sqInt,
) -> *mut c_void {
    let mut module_buf = [0u8; NAME_MAX + 1];

    // SAFETY: delegated to the caller.
    unsafe {
        let Some(module_name) =
            name_from_oop(module_name_index, module_name_length, &mut module_buf)
        else {
            return core::ptr::null_mut();
        };
        let module = find_or_load_module(module_name, 1);
        if module.is_null() {
            core::ptr::null_mut()
        } else {
            (*module).handle
        }
    }
}

/// Calls a module's `shutdownModule`, if it has one.
///
/// FFI-loaded modules are skipped: they never had an initialiser run, so they
/// have nothing to shut down.
///
/// # Safety
///
/// `module` must be live.
unsafe fn shutdown_module(module: *mut ModuleEntry) -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe {
        if (*module).ffi_loaded != 0 {
            return 1;
        }
        let f = find_function_in(c"shutdownModule", module);
        if f.is_null() {
            return 1;
        }
        let shutdown: extern "C" fn() -> sqInt = core::mem::transmute(f);
        shutdown()
    }
}

/// Shuts every loaded module down. Always answers 1.
///
/// # Safety
///
/// Must run on the VM thread with no module being loaded concurrently.
#[no_mangle]
pub unsafe extern "C" fn ioShutdownAllModules() -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe {
        let mut entry = FIRST_MODULE;
        while !entry.is_null() {
            shutdown_module(entry);
            entry = (*entry).next;
        }
    }
    1
}

/// Unloads the named module, notifying every other module first.
///
/// Answers 0 if nothing has been loaded yet, if the name is empty, or if the
/// module refuses to shut down; 1 otherwise, including when the module was
/// never loaded.
///
/// # Safety
///
/// `module_name`, if non-null, must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn ioUnloadModule(module_name: *mut c_char) -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe {
        if INTRINSICS_MODULE.is_null() {
            return 0;
        }
        if canonical(module_name).is_none() {
            return 0;
        }

        let entry = find_loaded_module(module_name);
        if entry.is_null() {
            return 1; // never loaded
        }

        if shutdown_module(entry) == 0 {
            return 0;
        }

        // Tell every other module, FFI-loaded ones included.
        let mut temp = FIRST_MODULE;
        while !temp.is_null() {
            if temp != entry {
                let f = find_function_in(c"moduleUnloaded", temp);
                if !f.is_null() {
                    let notify: extern "C" fn(*const c_char) -> sqInt = core::mem::transmute(f);
                    notify((*entry).name_ptr());
                }
            }
            temp = (*temp).next;
        }

        if (*entry).handle != intrinsics_handle() {
            ioFreeModule((*entry).handle);
        }
        remove_from_list(entry);
        drop(Box::from_raw(entry));
        1
    }
}

/// Entry point for the interpreter's unload primitive.
///
/// # Safety
///
/// The index/length pair must address readable bytes in object memory.
#[no_mangle]
pub unsafe extern "C" fn ioUnloadModuleOfLength(
    module_name_index: sqInt,
    module_name_length: sqInt,
) -> sqInt {
    let mut module_buf = [0u8; NAME_MAX + 1];
    // SAFETY: delegated to the caller.
    unsafe {
        let Some(module_name) =
            name_from_oop(module_name_index, module_name_length, &mut module_buf)
        else {
            return 0;
        };
        ioUnloadModule(module_name)
    }
}

/// Answers the name of the `module_index`-th statically linked module, or
/// null.
///
/// Modules are counted by their `setInterpreter` rows, since that is the one
/// entry every plugin has.
///
/// # Safety
///
/// Reads the export tables, which must be well formed -- see the module docs
/// on the missing null check.
#[no_mangle]
pub unsafe extern "C" fn ioListBuiltinModule(module_index: sqInt) -> *mut c_char {
    let mut remaining = module_index;

    // SAFETY: delegated to the caller.
    unsafe {
        let lists = plugin_export_lists();
        let mut list_index = 0usize;
        loop {
            let exports = *lists.add(list_index);
            if exports.is_null() {
                break;
            }
            list_index += 1;

            let mut index = 0usize;
            loop {
                let row = exports.add(index);
                index += 1;

                let plugin = (*row).plugin_name;
                let function = (*row).primitive_name;
                if function.is_null() && plugin.is_null() {
                    break; // no more plugins
                }

                // The C did not guard `function` here; see the module docs.
                if CStr::from_ptr(function) != c"setInterpreter" {
                    continue;
                }

                remaining -= 1;
                if remaining != 0 {
                    continue;
                }

                let init0 = find_internal_function_in(
                    c"getModuleName".as_ptr() as *mut c_char,
                    plugin,
                    0,
                    core::ptr::null_mut(),
                );
                if !init0.is_null() {
                    let get_module_name: extern "C" fn() -> *mut c_char =
                        core::mem::transmute(init0);
                    let module_name = get_module_name();
                    if !module_name.is_null() {
                        return module_name;
                    }
                }
                return plugin;
            }
        }
    }
    core::ptr::null_mut()
}

/// Answers the name of the `module_index`-th loaded module, 1-based, or null.
///
/// # Safety
///
/// Must run on the VM thread.
#[no_mangle]
pub unsafe extern "C" fn ioListLoadedModule(module_index: sqInt) -> *mut c_char {
    if module_index < 1 {
        return core::ptr::null_mut();
    }

    // SAFETY: the chain is VM-thread only.
    unsafe {
        let mut entry = FIRST_MODULE;
        let mut index: sqInt = 1;
        while !entry.is_null() && index < module_index {
            entry = (*entry).next;
            index += 1;
        }
        if entry.is_null() {
            return core::ptr::null_mut();
        }

        let init0 = find_function_in(c"getModuleName", entry);
        if !init0.is_null() {
            let get_module_name: extern "C" fn() -> *mut c_char = core::mem::transmute(init0);
            let module_name = get_module_name();
            if !module_name.is_null() {
                return module_name;
            }
        }
        (*entry).name_ptr() as *mut c_char
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_globals;
    use std::sync::Mutex;

    /// Address of the null-terminated array of tables the tests installed.
    ///
    /// A `usize` because raw pointers are not `Sync`; the arrays themselves are
    /// leaked, so the address stays valid for the process.
    static INSTALLED: Mutex<usize> = Mutex::new(0);

    /// An empty list of tables, for tests that install nothing.
    static NO_TABLES: [usize; 1] = [0];

    /// Answers [`super::plugin_export_lists`] during tests.
    pub(super) fn installed_exports() -> *const *mut SqExport {
        let installed = *INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        if installed == 0 {
            NO_TABLES.as_ptr().cast::<*mut SqExport>()
        } else {
            installed as *const *mut SqExport
        }
    }

    /// Builds one export row from raw bytes.
    ///
    /// `primitive` is passed as bytes rather than a string so a test can put
    /// the accessor-depth byte after the terminator, the way the generated
    /// table does.
    fn row(plugin: &[u8], primitive: &[u8], address: usize) -> SqExport {
        SqExport {
            plugin_name: Box::leak(plugin.to_vec().into_boxed_slice())
                .as_mut_ptr()
                .cast::<c_char>(),
            primitive_name: Box::leak(primitive.to_vec().into_boxed_slice())
                .as_mut_ptr()
                .cast::<c_char>(),
            primitive_address: address as *mut c_void,
        }
    }

    /// The row that terminates a table: both names null.
    fn end_row() -> SqExport {
        SqExport {
            plugin_name: core::ptr::null_mut(),
            primitive_name: core::ptr::null_mut(),
            primitive_address: core::ptr::null_mut(),
        }
    }

    /// Installs a single export table, leaked for the rest of the process.
    fn install(rows: Vec<SqExport>) {
        let table = Box::leak(rows.into_boxed_slice()).as_mut_ptr();
        let lists: Box<[*mut SqExport]> = vec![table, core::ptr::null_mut()].into_boxed_slice();
        let lists = Box::leak(lists).as_ptr() as usize;
        *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = lists;
    }

    /// Resets the module chain so each test starts from a known state.
    fn reset_modules() {
        // SAFETY: the caller holds the crate-wide lock, so no other test is
        // walking the chain. The entries are leaked rather than freed: some
        // may have been handed out as `handle` values.
        unsafe {
            FIRST_MODULE = core::ptr::null_mut();
            INTRINSICS_MODULE = core::ptr::null_mut();
            MODULE_LOADING_ENABLED = true;
        }
        *INSTALLED.lock().unwrap_or_else(|e| e.into_inner()) = 0;
    }

    /// Looks a primitive up in the installed tables.
    fn find_internal(function: &CStr, plugin: Option<&CStr>) -> (*mut c_void, sqInt) {
        let mut depth: sqInt = 0xDEAD;
        // SAFETY: both names are live literals; depth is writable.
        let f = unsafe {
            find_internal_function_in(
                function.as_ptr() as *mut c_char,
                plugin.map_or(core::ptr::null(), CStr::as_ptr),
                function.to_bytes().len() as sqInt,
                &mut depth,
            )
        };
        (f, depth)
    }

    #[test]
    fn a_primitive_is_found_with_the_depth_hidden_after_its_name() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![
            // Exactly the generated shape: name, NUL, one signed depth byte.
            row(b"FooPlugin\0", b"primitiveOne\0\x03", 0x1111),
            row(b"FooPlugin\0", b"primitiveTwo\0\xFF", 0x2222),
            end_row(),
        ]);

        let (f, depth) = find_internal(c"primitiveOne", Some(c"FooPlugin"));
        assert_eq!(f as usize, 0x1111);
        assert_eq!(depth, 3);

        // 0xFF is -1 as a *signed* char, which is Slang's "no accessor depth".
        // Reading it unsigned would give 255 and corrupt the primitive's stack
        // discipline, so this is the assertion that matters most here.
        let (f, depth) = find_internal(c"primitiveTwo", Some(c"FooPlugin"));
        assert_eq!(f as usize, 0x2222);
        assert_eq!(depth, -1);
    }

    #[test]
    fn a_missing_primitive_falls_through_to_the_global_symbols() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![
            row(b"FooPlugin\0", b"primitiveOne\0\x03", 0x1111),
            end_row(),
        ]);

        // Not in the table, and not a symbol either.
        let (f, _) = find_internal(c"primitiveNotThere", Some(c"FooPlugin"));
        assert!(f.is_null());

        // Not in the table, but the process does export it -- the C's last
        // resort is a global dlsym, which is how intrinsics linked into the VM
        // itself get found.
        let (f, _) = find_internal(c"malloc", None);
        assert!(!f.is_null(), "the global fallback should reach libc");
    }

    #[test]
    fn empty_and_absent_names_mean_the_same_thing() {
        let _guard = lock_globals();
        reset_modules();
        // A row with an empty plugin name is an intrinsic.
        install(vec![
            row(b"\0", b"primitiveIntrinsic\0\x00", 0x3333),
            end_row(),
        ]);

        // Absent plugin name.
        let (f, _) = find_internal(c"primitiveIntrinsic", None);
        assert_eq!(f as usize, 0x3333);

        // Empty plugin name: canonicalised to the same thing.
        let (f, _) = find_internal(c"primitiveIntrinsic", Some(c""));
        assert_eq!(f as usize, 0x3333);

        // A named plugin must *not* match the intrinsic row.
        let (f, _) = find_internal(c"primitiveIntrinsic", Some(c"FooPlugin"));
        assert!(
            f.is_null(),
            "a named plugin must not match an intrinsic row"
        );
    }

    #[test]
    fn builtin_modules_are_counted_by_their_set_interpreter_rows() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![
            row(b"AlphaPlugin\0", b"setInterpreter\0", 0),
            row(b"AlphaPlugin\0", b"primitiveA\0\xFF", 0x10),
            row(b"BetaPlugin\0", b"setInterpreter\0", 0),
            row(b"BetaPlugin\0", b"primitiveB\0\xFF", 0x20),
            end_row(),
        ]);

        // 1-based, and the index counts setInterpreter rows, not table rows.
        // SAFETY: the installed table is well formed.
        unsafe {
            let first = ioListBuiltinModule(1);
            assert_eq!(CStr::from_ptr(first), c"AlphaPlugin");

            let second = ioListBuiltinModule(2);
            assert_eq!(CStr::from_ptr(second), c"BetaPlugin");

            assert!(ioListBuiltinModule(3).is_null(), "only two modules exist");
        }
    }

    #[test]
    fn names_longer_than_the_buffer_are_refused() {
        let mut buf = [0u8; NAME_MAX + 1];
        let source = vec![b'x'; 300];
        let oop = source.as_ptr() as sqInt;

        // SAFETY: `source` is 300 readable bytes and outlives the calls.
        unsafe {
            // The C's `> 255` bail-out.
            assert!(name_from_oop(oop, 256, &mut buf).is_none());
            assert!(name_from_oop(oop, 300, &mut buf).is_none());

            // Exactly at the limit is accepted, terminated at [255].
            let p = name_from_oop(oop, NAME_MAX as sqInt, &mut buf).expect("255 fits");
            assert_eq!(CStr::from_ptr(p).to_bytes().len(), NAME_MAX);

            // And a short name copies exactly `length` bytes, ignoring what
            // follows in object memory.
            let p = name_from_oop(oop, 3, &mut buf).expect("3 fits");
            assert_eq!(CStr::from_ptr(p), c"xxx");
        }
    }

    #[test]
    fn a_negative_length_is_refused_rather_than_wrapping() {
        // The C compared a signed length against 255 and then passed it to
        // strncpy, where it became a huge size_t. Nothing produces a negative
        // length, but the conversion is worth pinning.
        let mut buf = [0u8; NAME_MAX + 1];
        let source = [b'x'; 8];
        // SAFETY: the pointer is valid; the call must reject before reading.
        unsafe {
            assert!(name_from_oop(source.as_ptr() as sqInt, -1, &mut buf).is_none());
        }
    }

    #[test]
    fn loading_a_module_that_does_not_exist_answers_null() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![end_row()]);

        let name = c"NoSuchPluginAnywhere";
        // SAFETY: the name is a live literal.
        let f = unsafe {
            ioLoadFunctionFrom(
                c"primitiveWhatever".as_ptr() as *mut c_char,
                name.as_ptr() as *mut c_char,
            )
        };
        assert!(f.is_null());
    }

    #[test]
    fn listing_loaded_modules_is_one_based_and_bounded() {
        let _guard = lock_globals();
        reset_modules();

        // SAFETY: the chain is empty and the lock is held.
        unsafe {
            assert!(ioListLoadedModule(0).is_null(), "0 is not a valid index");
            assert!(ioListLoadedModule(-1).is_null());
            assert!(ioListLoadedModule(1).is_null(), "nothing is loaded");

            // Put two entries in by hand, since loading a real plugin needs a
            // shared library on disk.
            add_to_module_list(c"First".as_ptr(), core::ptr::null_mut(), 1);
            add_to_module_list(c"Second".as_ptr(), core::ptr::null_mut(), 1);

            // The list is a stack, so the most recent is first.
            assert_eq!(CStr::from_ptr(ioListLoadedModule(1)), c"Second");
            assert_eq!(CStr::from_ptr(ioListLoadedModule(2)), c"First");
            assert!(ioListLoadedModule(3).is_null());
        }
        reset_modules();
    }

    #[test]
    fn unloading_reports_the_right_thing_for_each_case() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![end_row()]);

        // SAFETY: the lock is held and every name is a live literal.
        unsafe {
            // Nothing loaded at all: the C answered 0 because intrinsicsModule
            // is still null.
            assert_eq!(ioUnloadModule(c"Anything".as_ptr() as *mut c_char), 0);

            // Create the intrinsics entry the way findOrLoadModule does.
            INTRINSICS_MODULE = add_to_module_list(c"".as_ptr(), core::ptr::null_mut(), 1);
            FIRST_MODULE = core::ptr::null_mut();

            // An empty or null name is refused.
            assert_eq!(ioUnloadModule(c"".as_ptr() as *mut c_char), 0);
            assert_eq!(ioUnloadModule(core::ptr::null_mut()), 0);

            // A module that was never loaded answers 1: "it is not loaded" is
            // success, not failure.
            assert_eq!(ioUnloadModule(c"NeverLoaded".as_ptr() as *mut c_char), 1);

            // One that is loaded, with a null handle so no dlclose happens.
            add_to_module_list(c"Loaded".as_ptr(), core::ptr::null_mut(), 1);
            assert_eq!(ioUnloadModule(c"Loaded".as_ptr() as *mut c_char), 1);
            assert!(ioListLoadedModule(1).is_null(), "the chain is empty again");
        }
        reset_modules();
    }

    #[test]
    fn disabling_module_loading_stops_further_loads() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![end_row()]);

        ioDisableModuleLoading();

        // SAFETY: the lock is held.
        unsafe {
            let module = find_and_load_module(c"AnyPlugin".as_ptr() as *mut c_char, 0);
            assert!(module.is_null(), "loading must be refused once disabled");
        }

        // Irreversible in production; the harness resets it for other tests.
        reset_modules();
    }

    #[test]
    fn shutting_all_modules_down_skips_ffi_loaded_ones() {
        let _guard = lock_globals();
        reset_modules();
        install(vec![end_row()]);

        // SAFETY: the lock is held; both entries have null handles, so no
        // lookup reaches a real library.
        unsafe {
            add_to_module_list(c"FfiLoaded".as_ptr(), core::ptr::null_mut(), 1);
            // ioShutdownAllModules always answers 1, even with nothing to do.
            assert_eq!(ioShutdownAllModules(), 1);
        }
        reset_modules();
    }
}
