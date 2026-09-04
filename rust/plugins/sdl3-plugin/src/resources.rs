//! Windows, renderers and textures, held behind integer handles.
//!
//! Same reasoning as the Cairo plugin's registry: the image never sees an
//! `SDL_Window *`, so a stale reference fails a primitive instead of
//! dereferencing freed memory inside the VM.
//!
//! Destruction order matters here in a way it does not for Cairo. SDL's
//! objects are not reference counted: destroying a renderer destroys the
//! textures made from it, and destroying a window destroys its renderer. So a
//! handle can be invalidated by something other than its own destroy
//! primitive, and `destroy_window` walks the registries to invalidate what SDL
//! is about to free -- otherwise a later `primitiveRenderClear` would hand SDL
//! a dangling renderer and the "handles cannot dangle" promise would be a lie.
//!
//! Handles carry a type tag declared in [`resource_tags!`] below, so a texture
//! handle passed to a renderer primitive fails with `BadArgument` instead of
//! resolving onto the renderer that happens to occupy the same slot.

use pharo_vm_plugin::handles::{Handle, Registry};
use pharo_vm_plugin::{resource_tags, sqInt, PrimErr, PrimResult};

use crate::ffi::{sc, sdl, SDL_Renderer, SDL_Texture, SDL_Window};

/// A window, and nothing else: SDL owns everything about it.
pub struct Window {
    ptr: *mut SDL_Window,
}

/// A renderer, and the window it belongs to.
pub struct Renderer {
    ptr: *mut SDL_Renderer,
    /// Whose destruction takes this renderer with it.
    window: *mut SDL_Window,
}

/// A texture, and the renderer it belongs to.
pub struct Texture {
    ptr: *mut SDL_Texture,
    /// Whose destruction takes this texture with it.
    renderer: *mut SDL_Renderer,
}

// SAFETY: raw pointers into SDL's heap. SDL requires video and event calls on
// the thread that initialised video, which for the VM is the interpreter
// thread; every primitive runs there and nowhere else. `Send` is asserted only
// so these can live in a `static Registry`. Nothing here spawns a thread.
unsafe impl Send for Window {}
unsafe impl Send for Renderer {}
unsafe impl Send for Texture {}

// This library's handle tags, declared once, next to the statics they tell
// apart. Without them all three registries shared one encoding and a texture
// handle passed to a window primitive resolved. Unique only within this
// library, which is all that is needed: nothing else decodes these.
resource_tags! {
    Window = 1,
    Renderer = 2,
    Texture = 3,
}

/// Windows the image holds handles on.
pub static WINDOWS: Registry<Window> = Registry::new();
/// Renderers the image holds handles on.
pub static RENDERERS: Registry<Renderer> = Registry::new();
/// Textures the image holds handles on.
pub static TEXTURES: Registry<Texture> = Registry::new();

impl Window {
    /// Takes ownership of a window SDL just created.
    pub(crate) fn adopt(ptr: *mut SDL_Window) -> PrimResult<Self> {
        if ptr.is_null() {
            // SDL answers NULL and leaves the reason in SDL_GetError, which
            // the image can read with `primitiveGetError`.
            return Err(PrimErr::OperationFailed);
        }
        Ok(Self { ptr })
    }

    /// The raw window, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut SDL_Window {
        self.ptr
    }
}

impl Renderer {
    /// Takes ownership of a renderer SDL just created for `window`.
    pub(crate) fn adopt(ptr: *mut SDL_Renderer, window: *mut SDL_Window) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::OperationFailed);
        }
        Ok(Self { ptr, window })
    }

    /// The raw renderer, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut SDL_Renderer {
        self.ptr
    }
}

impl Texture {
    /// Takes ownership of a texture SDL just created on `renderer`.
    pub(crate) fn adopt(ptr: *mut SDL_Texture, renderer: *mut SDL_Renderer) -> PrimResult<Self> {
        if ptr.is_null() {
            return Err(PrimErr::OperationFailed);
        }
        Ok(Self { ptr, renderer })
    }

    /// The raw texture, for the duration of one primitive.
    #[must_use]
    pub fn as_ptr(&self) -> *mut SDL_Texture {
        self.ptr
    }
}

/// Runs `f` on the window `handle` names.
pub fn with_window<R>(
    handle: sqInt,
    f: impl FnOnce(*mut SDL_Window) -> PrimResult<R>,
) -> PrimResult<R> {
    WINDOWS
        .with(Handle::decode(handle)?, Window::as_ptr)
        .and_then(f)
}

/// Runs `f` on the renderer `handle` names.
pub fn with_renderer<R>(
    handle: sqInt,
    f: impl FnOnce(*mut SDL_Renderer) -> PrimResult<R>,
) -> PrimResult<R> {
    RENDERERS
        .with(Handle::decode(handle)?, Renderer::as_ptr)
        .and_then(f)
}

/// Runs `f` on the texture `handle` names.
pub fn with_texture<R>(
    handle: sqInt,
    f: impl FnOnce(*mut SDL_Texture) -> PrimResult<R>,
) -> PrimResult<R> {
    TEXTURES
        .with(Handle::decode(handle)?, Texture::as_ptr)
        .and_then(f)
}

/// Forgets every texture SDL is about to destroy along with `renderer`.
///
/// Dropped rather than destroyed: SDL frees a renderer's textures as part of
/// destroying it, so `SDL_DestroyTexture` here would be the second free. And
/// `remove_where` sweeps by slot index, so a texture whose slot has no
/// encodable handle is forgotten with the rest instead of being left for
/// `release_all` to destroy after SDL already has.
fn forget_textures_of(renderer: *mut SDL_Renderer) {
    drop(TEXTURES.remove_where(|t| t.renderer == renderer));
}

/// Destroys the texture `handle` names.
pub fn destroy_texture(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let texture = TEXTURES.remove(Handle::decode(handle)?)?;
    sc!(s, SDL_DestroyTexture(texture.ptr));
    Ok(())
}

/// Destroys the renderer `handle` names, and forgets its textures.
pub fn destroy_renderer(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let renderer = RENDERERS.remove(Handle::decode(handle)?)?;
    forget_textures_of(renderer.ptr);
    sc!(s, SDL_DestroyRenderer(renderer.ptr));
    Ok(())
}

/// Destroys the window `handle` names, and forgets its renderer and that
/// renderer's textures.
pub fn destroy_window(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let window = WINDOWS.remove(Handle::decode(handle)?)?;
    for r in RENDERERS.remove_where(|r| r.window == window.ptr) {
        forget_textures_of(r.ptr);
    }
    sc!(s, SDL_DestroyWindow(window.ptr));
    Ok(())
}

/// Releases everything, for the module's shutdown hook.
///
/// Textures first, then renderers, then windows: the reverse of the ownership
/// order, so nothing is destroyed twice by SDL's own cascade.
pub fn release_all() {
    let Ok(s) = sdl() else { return };
    for t in TEXTURES.drain() {
        if let Some(f) = s.SDL_DestroyTexture {
            // SAFETY: a live texture this registry owned.
            unsafe { f(t.ptr) };
        }
    }
    for r in RENDERERS.drain() {
        if let Some(f) = s.SDL_DestroyRenderer {
            // SAFETY: a live renderer this registry owned.
            unsafe { f(r.ptr) };
        }
    }
    for w in WINDOWS.drain() {
        if let Some(f) = s.SDL_DestroyWindow {
            // SAFETY: a live window this registry owned.
            unsafe { f(w.ptr) };
        }
    }
}

/// Narrows an image integer to a C `int`.
pub fn as_c_int(value: sqInt) -> PrimResult<core::ffi::c_int> {
    core::ffi::c_int::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Narrows an image integer to a C `int` that must be non-negative.
pub fn as_c_int_positive(value: sqInt) -> PrimResult<core::ffi::c_int> {
    let v = as_c_int(value)?;
    if v < 0 {
        return Err(PrimErr::BadArgument);
    }
    Ok(v)
}

/// Narrows an image integer to a `u8`, as SDL's colour components are.
pub fn as_u8(value: sqInt) -> PrimResult<u8> {
    u8::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Narrows an image integer to a `u32` flag word.
pub fn as_u32(value: sqInt) -> PrimResult<u32> {
    u32::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Narrows an image integer to the `u64` SDL's window flags are.
pub fn as_u64(value: sqInt) -> PrimResult<u64> {
    u64::try_from(value).map_err(|_| PrimErr::BadArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_from_one_registry_is_refused_by_the_others() {
        // All three registries used to share one encoding, so a texture handle
        // passed to a window primitive resolved onto whatever window occupied
        // the same slot -- and SDL would have been handed the wrong pointer.
        // No SDL call happens here: the tag is checked while the integer is
        // decoded, before the registry is locked.
        let window = WINDOWS
            .insert(Window {
                ptr: core::ptr::null_mut(),
            })
            .expect("a slot");
        let renderer = RENDERERS
            .insert(Renderer {
                ptr: core::ptr::null_mut(),
                window: core::ptr::null_mut(),
            })
            .expect("a slot");
        let texture = TEXTURES
            .insert(Texture {
                ptr: core::ptr::null_mut(),
                renderer: core::ptr::null_mut(),
            })
            .expect("a slot");

        assert_ne!(window.raw(), renderer.raw());
        assert_ne!(window.raw(), texture.raw());
        assert_ne!(renderer.raw(), texture.raw());

        assert_eq!(
            with_window(texture.raw(), |_| Ok(())),
            Err(PrimErr::BadArgument),
            "a texture handle must not resolve as a window"
        );
        assert_eq!(
            with_renderer(window.raw(), |_| Ok(())),
            Err(PrimErr::BadArgument)
        );
        assert_eq!(
            with_texture(renderer.raw(), |_| Ok(())),
            Err(PrimErr::BadArgument)
        );
        assert!(!WINDOWS.is_live(texture.raw()));

        assert!(with_window(window.raw(), |p| Ok(p.is_null())).unwrap());
        assert!(with_renderer(renderer.raw(), |p| Ok(p.is_null())).unwrap());
        assert!(with_texture(texture.raw(), |p| Ok(p.is_null())).unwrap());

        // Removed by hand rather than through `destroy_*`, which would need a
        // loaded SDL and would hand it these null pointers.
        WINDOWS.remove(window).expect("still there");
        RENDERERS.remove(renderer).expect("still there");
        TEXTURES.remove(texture).expect("still there");
    }

    #[test]
    fn a_destroyed_handle_is_not_found_rather_than_wrong_kind() {
        let window = WINDOWS
            .insert(Window {
                ptr: core::ptr::null_mut(),
            })
            .expect("a slot");
        let raw = window.raw();
        WINDOWS.remove(window).expect("still there");

        assert_eq!(with_window(raw, |_| Ok(())), Err(PrimErr::NotFound));
        assert_ne!(with_window(raw, |_| Ok(())), Err(PrimErr::BadArgument));
        assert!(!WINDOWS.is_live(raw));
    }
}
