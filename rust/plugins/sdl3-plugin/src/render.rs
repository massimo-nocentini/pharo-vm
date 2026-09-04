//! Renderers and textures: the path a Pharo Form takes to the screen.
//!
//! The intended sequence, and the reason the texture primitives look the way
//! they do:
//!
//! ```text
//! renderer := primitiveCreateRenderer(window, nil)
//! texture  := primitiveCreateTexture(renderer, ARGB8888, STREAMING, w, h)
//! ... each frame ...
//! primitiveUpdateTextureFromBits(texture, nil, form bits, w * 4)
//! primitiveRenderClear(renderer)
//! primitiveRenderTexture(renderer, texture, nil, nil)
//! primitiveRenderPresent(renderer)
//! ```

use core::ffi::c_void;

use pharo_vm_plugin::handles::Handle;
use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{check, sc, sdl, SDL_FRect, SDL_Rect};
use crate::resources::{
    as_c_int, as_c_int_positive, as_u32, as_u8, destroy_renderer, destroy_texture, with_renderer,
    with_texture, with_window, Renderer, Texture, RENDERERS, TEXTURES,
};

/// `SDL_CreateRenderer`. `name` may be nil to let SDL choose a driver.
#[pharo_primitive]
fn primitiveCreateRenderer(vm: &Interp, window: sqInt, name: Oop) -> PrimResult<Handle<Renderer>> {
    let s = sdl()?;
    let name = if vm.is_nil(name)? {
        None
    } else {
        Some(vm.c_string_value(name)?)
    };
    let name_ptr = name.as_ref().map_or(core::ptr::null(), |n| n.as_ptr());
    let (ptr, window_ptr) =
        with_window(window, |w| Ok((sc!(s, SDL_CreateRenderer(w, name_ptr)), w)))?;
    RENDERERS.insert(Renderer::adopt(ptr, window_ptr)?)
}

/// `SDL_DestroyRenderer`. Also invalidates handles on its textures.
#[pharo_primitive]
fn primitiveDestroyRenderer(_vm: &Interp, renderer: sqInt) -> PrimResult<()> {
    destroy_renderer(renderer)
}

/// Is this still a live renderer handle?
#[pharo_primitive]
fn primitiveRendererIsLive(_vm: &Interp, renderer: sqInt) -> PrimResult<bool> {
    Ok(RENDERERS.is_live(renderer))
}

/// `SDL_SetRenderDrawColor`. Components run 0 to 255.
#[pharo_primitive]
fn primitiveSetRenderDrawColor(
    _vm: &Interp,
    renderer: sqInt,
    red: sqInt,
    green: sqInt,
    blue: sqInt,
    alpha: sqInt,
) -> PrimResult<()> {
    let s = sdl()?;
    let (r, g, b, a) = (as_u8(red)?, as_u8(green)?, as_u8(blue)?, as_u8(alpha)?);
    with_renderer(renderer, |rp| {
        check(sc!(s, SDL_SetRenderDrawColor(rp, r, g, b, a)))
    })
}

/// `SDL_RenderClear`.
#[pharo_primitive]
fn primitiveRenderClear(_vm: &Interp, renderer: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    with_renderer(renderer, |rp| check(sc!(s, SDL_RenderClear(rp))))
}

/// `SDL_RenderPresent`.
#[pharo_primitive]
fn primitiveRenderPresent(_vm: &Interp, renderer: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    with_renderer(renderer, |rp| check(sc!(s, SDL_RenderPresent(rp))))
}

/// `SDL_GetRenderOutputSize`, as `width @ height`.
#[pharo_primitive]
fn primitiveGetRenderOutputSize(vm: &Interp, renderer: sqInt) -> PrimResult<Oop> {
    let s = sdl()?;
    let (mut w, mut h) = (0, 0);
    with_renderer(renderer, |rp| {
        check(sc!(s, SDL_GetRenderOutputSize(rp, &mut w, &mut h)))
    })?;
    vm.point(w as sqInt, h as sqInt)
}

/// `SDL_SetRenderVSync`. 1 synchronises with the display, 0 does not.
#[pharo_primitive]
fn primitiveSetRenderVSync(_vm: &Interp, renderer: sqInt, vsync: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let v = as_c_int(vsync)?;
    with_renderer(renderer, |rp| check(sc!(s, SDL_SetRenderVSync(rp, v))))
}

/// `SDL_GetRenderVSync`.
#[pharo_primitive]
fn primitiveGetRenderVSync(_vm: &Interp, renderer: sqInt) -> PrimResult<isize> {
    let s = sdl()?;
    let mut v = 0;
    with_renderer(renderer, |rp| {
        check(sc!(s, SDL_GetRenderVSync(rp, &mut v)))
    })?;
    Ok(v as isize)
}

/// Reads an `SDL_FRect` from a 16-byte ByteArray of four native-endian
/// `f32`s, or `None` for nil.
///
/// `primitiveFRectPut` fills one of these, so the image never has to know how
/// a C `float` is laid out.
fn frect_from(vm: &Interp, oop: Oop) -> PrimResult<Option<SDL_FRect>> {
    if vm.is_nil(oop)? {
        return Ok(None);
    }
    let bytes = vm.bytes_of(oop)?;
    if bytes.len() != 16 {
        return Err(PrimErr::BadArgument);
    }
    let f = |i: usize| f32::from_ne_bytes(bytes[i..i + 4].try_into().expect("4 bytes"));
    Ok(Some(SDL_FRect {
        x: f(0),
        y: f(4),
        w: f(8),
        h: f(12),
    }))
}

/// Reads an `SDL_Rect` from a 16-byte ByteArray of four native-endian `i32`s,
/// or `None` for nil.
fn rect_from(vm: &Interp, oop: Oop) -> PrimResult<Option<SDL_Rect>> {
    if vm.is_nil(oop)? {
        return Ok(None);
    }
    let bytes = vm.bytes_of(oop)?;
    if bytes.len() != 16 {
        return Err(PrimErr::BadArgument);
    }
    let i = |n: usize| i32::from_ne_bytes(bytes[n..n + 4].try_into().expect("4 bytes"));
    Ok(Some(SDL_Rect {
        x: i(0),
        y: i(4),
        w: i(8),
        h: i(12),
    }))
}

/// Fills a 16-byte ByteArray with an `SDL_FRect`'s four `f32`s.
///
/// Exists so the image can build a rectangle without knowing that SDL's render
/// calls take single-precision floats while `SDL_UpdateTexture` takes integers.
#[pharo_primitive]
fn primitiveFRectPut(
    vm: &Interp,
    rect: Oop,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> PrimResult<()> {
    if usize::try_from(vm.byte_size_of(rect)?)? != 16 {
        return Err(PrimErr::BadArgument);
    }
    let mut bytes = [0u8; 16];
    for (i, v) in [x, y, width, height].iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&(*v as f32).to_ne_bytes());
    }
    vm.write_bytes(rect, 0, &bytes)
}

/// Fills a 16-byte ByteArray with an `SDL_Rect`'s four `i32`s.
#[pharo_primitive]
fn primitiveRectPut(
    vm: &Interp,
    rect: Oop,
    x: sqInt,
    y: sqInt,
    width: sqInt,
    height: sqInt,
) -> PrimResult<()> {
    if usize::try_from(vm.byte_size_of(rect)?)? != 16 {
        return Err(PrimErr::BadArgument);
    }
    let mut bytes = [0u8; 16];
    for (i, v) in [x, y, width, height].iter().enumerate() {
        let n = i32::try_from(*v).map_err(|_| PrimErr::BadArgument)?;
        bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_ne_bytes());
    }
    vm.write_bytes(rect, 0, &bytes)
}

/// `SDL_RenderTexture`. `source` and `destination` are `SDL_FRect` ByteArrays
/// or nil for "the whole thing".
#[pharo_primitive]
fn primitiveRenderTexture(
    vm: &Interp,
    renderer: sqInt,
    texture: sqInt,
    source: Oop,
    destination: Oop,
) -> PrimResult<()> {
    let s = sdl()?;
    let src = frect_from(vm, source)?;
    let dst = frect_from(vm, destination)?;
    let t = with_texture(texture, Ok)?;
    with_renderer(renderer, |rp| {
        check(sc!(
            s,
            SDL_RenderTexture(
                rp,
                t,
                src.as_ref().map_or(core::ptr::null(), |r| r as *const _),
                dst.as_ref().map_or(core::ptr::null(), |r| r as *const _),
            )
        ))
    })
}

/// `SDL_CreateTexture`. `format` is an `SDL_PixelFormat`, `access` an
/// `SDL_TextureAccess` (0 static, 1 streaming, 2 target).
#[pharo_primitive]
fn primitiveCreateTexture(
    _vm: &Interp,
    renderer: sqInt,
    format: sqInt,
    access: sqInt,
    width: sqInt,
    height: sqInt,
) -> PrimResult<Handle<Texture>> {
    let s = sdl()?;
    let format = as_u32(format)?;
    let access = as_c_int(access)?;
    if !(0..=2).contains(&access) {
        return Err(PrimErr::BadArgument);
    }
    let (w, h) = (as_c_int_positive(width)?, as_c_int_positive(height)?);
    let (ptr, renderer_ptr) = with_renderer(renderer, |rp| {
        Ok((sc!(s, SDL_CreateTexture(rp, format, access, w, h)), rp))
    })?;
    TEXTURES.insert(Texture::adopt(ptr, renderer_ptr)?)
}

/// `SDL_DestroyTexture`.
#[pharo_primitive]
fn primitiveDestroyTexture(_vm: &Interp, texture: sqInt) -> PrimResult<()> {
    destroy_texture(texture)
}

/// Is this still a live texture handle?
#[pharo_primitive]
fn primitiveTextureIsLive(_vm: &Interp, texture: sqInt) -> PrimResult<bool> {
    Ok(TEXTURES.is_live(texture))
}

/// `SDL_UpdateTexture`, reading the pixels straight out of an image object.
///
/// `rectangle` is an `SDL_Rect` ByteArray or nil for the whole texture;
/// `pitch` is the source's bytes per row. The image object must hold at least
/// `pitch * height` bytes, where `height` comes from the rectangle -- SDL does
/// not check, and a `pitch` one row too large would have it read past the end
/// of the object.
///
/// When `rectangle` is nil the height cannot be known here, so the check falls
/// back to "at least `pitch` bytes"; pass a rectangle when the image object is
/// sized exactly.
#[pharo_primitive]
fn primitiveUpdateTextureFromBits(
    vm: &Interp,
    texture: sqInt,
    rectangle: Oop,
    bits: Oop,
    pitch: sqInt,
) -> PrimResult<()> {
    let s = sdl()?;
    let rect = rect_from(vm, rectangle)?;
    let pitch = as_c_int_positive(pitch)?;
    if pitch == 0 {
        return Err(PrimErr::BadArgument);
    }

    let (ptr, available) = vm.indexable_bytes_ptr(bits)?;
    let rows = rect.map_or(1, |r| r.h.max(0));
    let needed = usize::try_from(pitch)
        .ok()
        .and_then(|p| p.checked_mul(usize::try_from(rows).ok()?))
        .ok_or(PrimErr::LimitExceeded)?;
    if available < needed {
        return Err(PrimErr::BadIndex);
    }

    with_texture(texture, |t| {
        check(sc!(
            s,
            SDL_UpdateTexture(
                t,
                rect.as_ref().map_or(core::ptr::null(), |r| r as *const _),
                ptr.cast::<c_void>(),
                pitch,
            )
        ))
    })
}

/// `SDL_SetTextureBlendMode`.
#[pharo_primitive]
fn primitiveSetTextureBlendMode(_vm: &Interp, texture: sqInt, mode: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let mode = as_u32(mode)?;
    with_texture(texture, |t| {
        check(sc!(s, SDL_SetTextureBlendMode(t, mode)))
    })
}

/// `SDL_SetTextureScaleMode`: 0 nearest, 1 linear, 2 pixel-art.
#[pharo_primitive]
fn primitiveSetTextureScaleMode(_vm: &Interp, texture: sqInt, mode: sqInt) -> PrimResult<()> {
    let s = sdl()?;
    let mode = as_c_int(mode)?;
    if !(0..=2).contains(&mode) {
        return Err(PrimErr::BadArgument);
    }
    with_texture(texture, |t| {
        check(sc!(s, SDL_SetTextureScaleMode(t, mode)))
    })
}
