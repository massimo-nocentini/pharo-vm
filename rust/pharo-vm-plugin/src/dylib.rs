//! Opening a shared library the VM ships beside itself.
//!
//! A plugin that wraps a third-party library -- Cairo, SDL -- has a choice
//! about how it binds to it. Linking at build time is the usual answer, and
//! the wrong one here: the Pharo build *downloads* those libraries as
//! ready-made binaries (see `cmake/importCairo.cmake`, `importSDL2.cmake`),
//! so they arrive as runtime objects with no headers and no `.pc` file, and
//! nothing in a normal build environment would satisfy a linker asking for
//! them. Requiring `libcairo-dev` to compile a VM whose bundle already
//! contains Cairo would be a strange trade.
//!
//! So the binding is a `dlopen`, exactly as the image-side FFI does it today,
//! and it looks in the same places the VM looks for its own plugins: beside
//! the executable first, then wherever the system loader searches.
//!
//! Requires the `dylib` feature; the crate has no dependencies without it.

use std::ffi::OsStr;
use std::path::PathBuf;

pub use libloading::{Library, Symbol};

/// Opens the first of `names` that can be found and loaded.
///
/// Each name is tried beside the VM executable before being handed to the
/// system loader, so a bundled copy always wins over one installed
/// system-wide -- the same precedence the bundle itself implies.
///
/// Answers the library and the name that worked. Answers `None` if none of
/// them could be loaded, which a plugin should report by declining to
/// initialise rather than by failing every primitive later.
///
/// # Safety
///
/// Loading a shared library runs its initialisers, which can do anything. The
/// caller is asserting that these names denote the library it means, and that
/// the symbols it goes on to fetch have the signatures it declares.
pub unsafe fn open_first(names: &[&str]) -> Option<(Library, String)> {
    for name in names {
        for candidate in candidates(name) {
            // SAFETY: delegated to this function's own contract.
            if let Ok(lib) = unsafe { Library::new(&candidate) } {
                return Some((lib, candidate.to_string_lossy().into_owned()));
            }
        }
    }
    None
}

/// Where to look for `name`, in order.
fn candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = executable_dir() {
        out.push(dir.join(name));
        // The macOS bundle splits the two: the executable is in
        // `Contents/MacOS` and everything loadable in `Contents/MacOS/Plugins`
        // (CMakeLists.txt sets LIBRARY_OUTPUT_DIRECTORY there).
        out.push(dir.join("Plugins").join(name));
        out.push(dir.join("lib").join(name));
        if let Some(parent) = dir.parent() {
            out.push(parent.join("lib").join(name));
        }
    }
    // Bare name last: let the system loader apply its own search.
    out.push(PathBuf::from(name));
    out
}

fn executable_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(std::path::Path::to_path_buf)
}

/// Fetches a symbol, answering `None` when the library does not export it.
///
/// Every binding in this crate's users is optional for a reason: these
/// libraries are versioned independently of the VM, and a plugin built
/// against a newer Cairo must still load beside an older one. A missing
/// symbol should cost one primitive, not the whole module.
///
/// # Safety
///
/// `T` must be the symbol's actual type. Nothing checks this.
#[must_use]
pub unsafe fn symbol<T: Copy>(lib: &Library, name: &str) -> Option<T> {
    let mut with_nul = Vec::with_capacity(name.len() + 1);
    with_nul.extend_from_slice(name.as_bytes());
    with_nul.push(0);
    // SAFETY: delegated to this function's own contract. The symbol is copied
    // out rather than borrowed, which is sound because the library is never
    // unloaded -- see the note on `Library` in the plugins that use this.
    let sym: Symbol<'_, T> = unsafe { lib.get(&with_nul) }.ok()?;
    Some(*sym)
}

/// Keeps a library loaded for the life of the process.
///
/// A plugin resolves function pointers out of the library once and hands them
/// around as plain `fn` values; if the `Library` were ever dropped those
/// pointers would dangle. Leaking is the honest way to say that the mapping is
/// permanent -- the VM never unloads a plugin's dependencies either.
pub fn leak(lib: Library) -> &'static Library {
    Box::leak(Box::new(lib))
}

/// A conventional set of file names for `stem` on the current platform.
///
/// `soname` is the versioned name the loader normally wants (`2` for Cairo's
/// `libcairo.so.2`); the unversioned name is tried after it, for a bundle that
/// ships only the development symlink.
#[must_use]
pub fn library_names(stem: &str, soname: &str) -> Vec<String> {
    if cfg!(target_os = "windows") {
        vec![
            format!("{stem}-{soname}.dll"),
            format!("{stem}.dll"),
            format!("lib{stem}-{soname}.dll"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            format!("lib{stem}.{soname}.dylib"),
            format!("lib{stem}.dylib"),
        ]
    } else {
        vec![
            format!("lib{stem}.so.{soname}"),
            format!("lib{stem}.so"),
        ]
    }
}

/// Is `path` something [`open_first`] would have tried? For tests.
#[must_use]
#[doc(hidden)]
pub fn would_try(name: &str, path: &OsStr) -> bool {
    candidates(name).iter().any(|c| c.as_os_str() == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_name_is_tried_last_so_a_bundled_copy_wins() {
        let tried = candidates("libcairo.so.2");
        assert_eq!(*tried.last().unwrap(), PathBuf::from("libcairo.so.2"));
        assert!(tried.len() > 1, "expected the executable's directory too");
    }

    #[test]
    fn the_executables_directory_is_tried_first() {
        let dir = executable_dir().expect("a test binary has a path");
        assert_eq!(candidates("x.so")[0], dir.join("x.so"));
    }

    #[test]
    fn names_are_platform_shaped() {
        let names = library_names("cairo", "2");
        assert!(!names.is_empty());
        if cfg!(target_os = "linux") {
            assert_eq!(names[0], "libcairo.so.2");
            assert_eq!(names[1], "libcairo.so");
        }
    }

    #[test]
    fn opening_a_library_that_does_not_exist_answers_none() {
        // SAFETY: nothing by this name exists, so no initialisers run.
        let opened = unsafe { open_first(&["libdefinitely-not-here.so.99"]) };
        assert!(opened.is_none());
    }
}
