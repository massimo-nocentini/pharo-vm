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

use pharo_vm_plugin::handles::Registry;
use pharo_vm_plugin::{sqInt, PrimErr, PrimResult};

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
    WINDOWS.with(handle, Window::as_ptr).and_then(f)
}

/// Runs `f` on the renderer `handle` names.
pub fn with_renderer<R>(
    handle: sqInt,
    f: impl FnOnce(*mut SDL_Renderer) -> PrimResult<R>,
) -> PrimResult<R> {
    RENDERERS.with(handle, Renderer::as_ptr).and_then(f)
}

/// Runs `f` on the texture `handle` names.
pub fn with_texture<R>(
    handle: sqInt,
    f: impl FnOnce(*mut SDL_Texture) -> PrimResult<R>,
) -> PrimResult<R> {
    TEXTURES.with(handle, Texture::as_ptr).and_then(f)
}

/// Forgets every texture SDL is about to destroy along with `renderer`.
fn forget_textures_of(renderer: *mut SDL_Renderer) {
    let doomed: Vec<sqInt> = TEXTURES.handles_where(|t| t.renderer == renderer);
    for h in doomed {
        let _ = TEXTURES.remove(h);
    }
}

/// Destroys the texture `handle` names.
pub fn destroy_texture(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let texture = TEXTURES.remove(handle)?;
    sc!(s, SDL_DestroyTexture(texture.ptr));
    Ok(())
}

/// Destroys the renderer `handle` names, and forgets its textures.
pub fn destroy_renderer(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let renderer = RENDERERS.remove(handle)?;
    forget_textures_of(renderer.ptr);
    sc!(s, SDL_DestroyRenderer(renderer.ptr));
    Ok(())
}

/// Destroys the window `handle` names, and forgets its renderer and that
/// renderer's textures.
pub fn destroy_window(handle: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let window = WINDOWS.remove(handle)?;
    let doomed: Vec<sqInt> = RENDERERS.handles_where(|r| r.window == window.ptr);
    for h in doomed {
        if let Ok(r) = RENDERERS.remove(h) {
            forget_textures_of(r.ptr);
        }
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
