//! The module-level contract, checkable without a VM or an SDL3.

use std::ffi::CStr;

#[test]
fn the_module_name_matches_the_library_name() {
    let name = unsafe { CStr::from_ptr(SDL3Plugin::getModuleName()) };
    assert_eq!(name.to_str().unwrap(), "SDL3Plugin");
}

#[test]
fn set_interpreter_rejects_a_null_proxy() {
    assert_eq!(SDL3Plugin::setInterpreter(std::ptr::null_mut()), 0);
}

#[test]
fn initialising_without_sdl_declines_rather_than_pretending() {
    // The Linux case today: cmake/importSDL2.cmake downloads SDL3 only for
    // Windows and macOS, so a Linux bundle has none and the module must
    // decline, leaving the image on its SDL2 FFI binding.
    let answer = SDL3Plugin::initialiseModule();
    assert_eq!(answer == 1, SDL3Plugin::ffi::sdl().is_ok());
}
