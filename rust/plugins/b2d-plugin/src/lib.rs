//! `B2DPlugin` — the Balloon 2D vector-graphics engine — in Rust.
//!
//! A mechanical port of the Slang-generated `plugins/B2DPlugin/src/common/`
//! `B2DPlugin.c` (VMMaker.oscog-eem.2480). The engine's entire state lives in
//! an image-side `Bitmap` (the *work buffer*) addressed through fixed word
//! offsets — that layout is ABI and is mirrored constant for constant in
//! [`consts`]. The rasterizer core lives in [`engine`], [`edges`], [`fills`],
//! [`shapes`] and [`scan`], keeping the C's function names so the two sources
//! read side by side; this file is the oop-facing half: the interpreter-proxy
//! plumbing, the state loaders, and the 43 exported primitives.
//!
//! The primitives manage the Smalltalk stack themselves (`pop`, `pushBool`,
//! `popthenPush`) and fail through `primitiveFailFor` with the plugin's own
//! `GEF*` codes, exactly as the C does; the `#[pharo_primitive]` wrapper only
//! contributes the accessor-depth export and the panic fence.

#![allow(non_snake_case)] // primitive and helper names are fixed by the C

pub mod consts;
mod edges;
pub mod engine;
mod fills;
mod scan;
pub mod shapes;

#[cfg(test)]
mod tests;

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_uint, c_void};
use std::sync::atomic::Ordering;

use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, Interp, IntoReturn, PrimResult, Section, VirtualMachine,
};

use consts::*;
use engine::{Engine, Host, SqInt};
use shapes::PointsRef;

// The C's getModuleName answers this exact string ("(e)" = external); the VM
// compares the requested module name as a prefix, so the version tail rides
// along just as it does for the C build.
pharo_plugin!(
    "B2DPlugin VMMaker.oscog-eem.2480 (e)",
    init = initialiseModule_hook
);

/// Returned by primitives that have already answered (or failed) through the
/// proxy themselves; tells the wrapper to leave the stack alone.
struct Managed;

impl IntoReturn for Managed {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The C file-level statics that survive between primitive calls
// ---------------------------------------------------------------------------

struct Globals {
    /// `bbPluginName[256]` — NUL-terminated, defaults to "BitBltPlugin".
    bbPluginName: [u8; 256],
    /// `loadBBFn` fetched via `ioLoadFunctionFrom`.
    loadBBFn: *const c_void,
    /// `copyBitsFn` fetched via `ioLoadFunctionFrom`.
    copyBitsFn: *const c_void,
    /// `doProfileStats`.
    doProfileStats: bool,
    /// `engine` — the engine oop of the current call.
    engine: SqInt,
    /// `formArray` — the forms array oop of the current call.
    formArray: SqInt,
}

const fn initial_bb_plugin_name() -> [u8; 256] {
    let mut name = [0u8; 256];
    let s = b"BitBltPlugin";
    let mut i = 0;
    while i < s.len() {
        name[i] = s[i];
        i += 1;
    }
    name
}

struct GlobalsCell(UnsafeCell<Globals>);

// SAFETY: the VM calls every plugin entry point on the single interpreter
// thread — the same contract the C statics rely on. Nothing here is touched
// from any other thread.
unsafe impl Sync for GlobalsCell {}

static GLOBALS: GlobalsCell = GlobalsCell(UnsafeCell::new(Globals {
    bbPluginName: initial_bb_plugin_name(),
    loadBBFn: core::ptr::null(),
    copyBitsFn: core::ptr::null(),
    doProfileStats: false,
    engine: 0,
    formArray: 0,
}));

/// Short-lived access to the plugin globals. Never call back into the engine
/// or the proxy from inside the closure.
///
/// There is no mutex here to poison -- the cell is a bare `UnsafeCell`
/// asserted `Sync` on the single-interpreter-thread contract -- so the
/// [`Section`] is the whole of the fail-fast story for these fields, and it is
/// the two-line wrap `pharo_vm_plugin::poison` documents. It earns its place:
/// `initialiseModule` writes `loadBBFn` and `copyBitsFn` as a pair and
/// `moduleUnloaded` nulls them as a pair, so a panic between the two writes
/// leaves one live pointer into a `dlclose`d BitBltPlugin for the engine to
/// call. Poisoning the module is the only answer that stops that call.
fn with_globals<R>(f: impl FnOnce(&mut Globals) -> R) -> R {
    let _section = Section::enter();
    // SAFETY: single interpreter thread (see GlobalsCell), and no reentrant
    // use: closures passed here only read/write the plain fields.
    unsafe { f(&mut *GLOBALS.0.get()) }
}

// ---------------------------------------------------------------------------
// Interpreter proxy access
// ---------------------------------------------------------------------------

/// A thin typed view of the proxy entries this plugin uses — the same set the
/// C binds in `setInterpreter`. Entries are guaranteed present in proxy 1.15
/// (which the SDK pins), so a missing one is a deployment error and panics
/// into the primitive wrapper's fence.
#[derive(Clone, Copy)]
struct Vm(*mut VirtualMachine);

macro_rules! proxy_call {
    ($self:ident, $field:ident ( $($arg:expr),* )) => {{
        // SAFETY: the pointer is the VM's own proxy table, and the signature
        // is the one pinned in pharo-vm-plugin's proxy module.
        let f = unsafe { &*$self.0 }.$field.expect("proxy entry missing");
        unsafe { f($($arg),*) }
    }};
}

#[allow(dead_code)]
impl Vm {
    fn methodArgumentCount(self) -> SqInt {
        proxy_call!(self, methodArgumentCount())
    }
    fn stackValue(self, offset: SqInt) -> SqInt {
        proxy_call!(self, stackValue(offset))
    }
    fn stackObjectValue(self, offset: SqInt) -> SqInt {
        proxy_call!(self, stackObjectValue(offset))
    }
    fn stackIntegerValue(self, offset: SqInt) -> SqInt {
        proxy_call!(self, stackIntegerValue(offset))
    }
    fn positive32BitValueOf(self, oop: SqInt) -> SqInt {
        // unsigned int in C; zero-extends into sqInt.
        proxy_call!(self, positive32BitValueOf(oop)) as SqInt
    }
    fn positive32BitIntegerFor(self, value: SqInt) -> SqInt {
        proxy_call!(self, positive32BitIntegerFor(value as c_uint))
    }
    fn booleanValueOf(self, oop: SqInt) -> SqInt {
        proxy_call!(self, booleanValueOf(oop))
    }
    fn failed(self) -> bool {
        proxy_call!(self, failed()) != 0
    }
    fn pop(self, n: SqInt) -> SqInt {
        proxy_call!(self, pop(n))
    }
    fn popthenPush(self, n: SqInt, oop: SqInt) {
        proxy_call!(self, popthenPush(n, oop))
    }
    fn pushBool(self, b: SqInt) -> SqInt {
        proxy_call!(self, pushBool(b))
    }
    fn pushInteger(self, v: SqInt) -> SqInt {
        proxy_call!(self, pushInteger(v))
    }
    fn primitiveFail(self) -> SqInt {
        proxy_call!(self, primitiveFail())
    }
    fn primitiveFailFor(self, code: SqInt) -> SqInt {
        proxy_call!(self, primitiveFailFor(code))
    }
    fn fetchPointerofObject(self, index: SqInt, oop: SqInt) -> SqInt {
        proxy_call!(self, fetchPointerofObject(index, oop))
    }
    fn fetchIntegerofObject(self, index: SqInt, oop: SqInt) -> SqInt {
        proxy_call!(self, fetchIntegerofObject(index, oop))
    }
    fn fetchClassOf(self, oop: SqInt) -> SqInt {
        proxy_call!(self, fetchClassOf(oop))
    }
    fn classBitmap(self) -> SqInt {
        proxy_call!(self, classBitmap())
    }
    fn classPoint(self) -> SqInt {
        proxy_call!(self, classPoint())
    }
    fn nilObject(self) -> SqInt {
        proxy_call!(self, nilObject())
    }
    fn slotSizeOf(self, oop: SqInt) -> SqInt {
        proxy_call!(self, slotSizeOf(oop))
    }
    fn byteSizeOf(self, oop: SqInt) -> SqInt {
        proxy_call!(self, byteSizeOf(oop))
    }
    fn isWords(self, oop: SqInt) -> bool {
        proxy_call!(self, isWords(oop)) != 0
    }
    fn isBytes(self, oop: SqInt) -> bool {
        proxy_call!(self, isBytes(oop)) != 0
    }
    fn isArray(self, oop: SqInt) -> bool {
        proxy_call!(self, isArray(oop)) != 0
    }
    fn isPointers(self, oop: SqInt) -> bool {
        proxy_call!(self, isPointers(oop)) != 0
    }
    fn isImmediate(self, oop: SqInt) -> bool {
        proxy_call!(self, isImmediate(oop)) != 0
    }
    fn isIntegerObject(self, oop: SqInt) -> bool {
        proxy_call!(self, isIntegerObject(oop)) != 0
    }
    fn isFloatObject(self, oop: SqInt) -> bool {
        proxy_call!(self, isFloatObject(oop)) != 0
    }
    fn integerValueOf(self, oop: SqInt) -> SqInt {
        proxy_call!(self, integerValueOf(oop))
    }
    fn floatValueOf(self, oop: SqInt) -> f64 {
        proxy_call!(self, floatValueOf(oop))
    }
    fn firstIndexableField(self, oop: SqInt) -> *mut c_void {
        proxy_call!(self, firstIndexableField(oop))
    }
    fn storeIntegerofObjectwithValue(self, index: SqInt, oop: SqInt, value: SqInt) -> SqInt {
        proxy_call!(self, storeIntegerofObjectwithValue(index, oop, value))
    }
    fn storePointerofObjectwithValue(self, index: SqInt, oop: SqInt, value: SqInt) -> SqInt {
        proxy_call!(self, storePointerofObjectwithValue(index, oop, value))
    }
    fn makePointwithxValueyValue(self, x: SqInt, y: SqInt) -> SqInt {
        proxy_call!(self, makePointwithxValueyValue(x, y))
    }
    fn pushRemappableOop(self, oop: SqInt) {
        proxy_call!(self, pushRemappableOop(oop))
    }
    fn popRemappableOop(self) -> SqInt {
        proxy_call!(self, popRemappableOop())
    }
    fn topRemappableOop(self) -> SqInt {
        proxy_call!(self, topRemappableOop())
    }
    fn ioMicroMSecs(self) -> SqInt {
        proxy_call!(self, ioMicroMSecs())
    }
    fn ioLoadFunctionFrom(self, fn_name: &[u8], mod_name: &[u8]) -> *mut c_void {
        proxy_call!(
            self,
            ioLoadFunctionFrom(
                fn_name.as_ptr() as *mut c_char,
                mod_name.as_ptr() as *mut c_char
            )
        )
    }
}

// ---------------------------------------------------------------------------
// Module lifecycle
// ---------------------------------------------------------------------------

/// `initialiseModule` — resolves BitBlt's `loadBitBltFrom`/`copyBitsFromtoat`
/// through `ioLoadFunctionFrom` (link-time resolution is deliberately not
/// used). Answering false makes the VM reject the module, as the C does when
/// no BitBlt plugin is available.
fn initialiseModule_hook() -> bool {
    let vt = pharo_vm_plugin::__private::INTERP.load(Ordering::Acquire);
    if vt.is_null() {
        return false;
    }
    let vm = Vm(vt);
    let name = with_globals(|g| g.bbPluginName);
    let load_bb = vm.ioLoadFunctionFrom(b"loadBitBltFrom\0", &name);
    let copy_bits = vm.ioLoadFunctionFrom(b"copyBitsFromtoat\0", &name);
    with_globals(|g| {
        g.loadBBFn = load_bb;
        g.copyBitsFn = copy_bits;
    });
    !load_bb.is_null() && !copy_bits.is_null()
}

/// `moduleUnloaded:` — the VM tells us a module went away; if it was our
/// BitBlt provider, drop the dangling function pointers.
///
/// Kept panic-free by hand: this is a bare `extern "C"` export outside the
/// primitive fence.
///
/// # Safety
///
/// `aModuleName` must be null or a NUL-terminated C string, which is what the
/// VM's module machinery passes.
#[no_mangle]
pub unsafe extern "C" fn moduleUnloaded(aModuleName: *const c_char) -> SqInt {
    if aModuleName.is_null() {
        return 0;
    }
    let matches = with_globals(|g| {
        let mut i = 0usize;
        loop {
            // SAFETY: the VM passes a NUL-terminated module name; we stop at
            // its NUL or at our buffer's end.
            let c = unsafe { *aModuleName.add(i) } as u8;
            if i >= 256 {
                return false;
            }
            if c != g.bbPluginName[i] {
                return false;
            }
            if c == 0 {
                return true;
            }
            i += 1;
        }
    });
    if matches {
        // BitBlt just shut down. How nasty. (The C's words.)
        with_globals(|g| {
            g.loadBBFn = core::ptr::null();
            g.copyBitsFn = core::ptr::null();
        });
    }
    0
}

// ---------------------------------------------------------------------------
// The VM-backed Host
// ---------------------------------------------------------------------------

/// [`Host`] backed by the interpreter proxy and the plugin globals.
struct VmHost {
    vm: Vm,
}

impl Host for VmHost {
    fn bits_of_form(&mut self, x_index: SqInt) -> Option<(*const i32, SqInt)> {
        let form_array = with_globals(|g| g.formArray);
        // The C's exact check is `>` (not `>=`); the subsequent fetch goes
        // through the VM's own accessor either way.
        if x_index > self.vm.slotSizeOf(form_array) {
            return None;
        }
        let form_oop = self.vm.fetchPointerofObject(x_index, form_array);
        let bits_oop = self.vm.fetchPointerofObject(0, form_oop);
        let bits_len = self.vm.slotSizeOf(bits_oop);
        Some((
            self.vm.firstIndexableField(bits_oop) as *const i32,
            bits_len,
        ))
    }

    fn copyBitsFromtoat(&mut self, x0: SqInt, x1: SqInt, y_value: SqInt) {
        let mut f = with_globals(|g| g.copyBitsFn);
        if f.is_null() {
            // We need copyBits here, so try to load it implicitly.
            if !initialiseModule_hook() {
                return;
            }
            f = with_globals(|g| g.copyBitsFn);
            if f.is_null() {
                return;
            }
        }
        // SAFETY: the pointer came from ioLoadFunctionFrom for a symbol with
        // exactly this C signature (BitBltPlugin's copyBitsFromtoat).
        let f: unsafe extern "C" fn(SqInt, SqInt, SqInt) -> SqInt =
            unsafe { core::mem::transmute(f) };
        unsafe {
            f(x0, x1, y_value);
        }
    }

    fn ioMicroMSecs(&mut self) -> SqInt {
        self.vm.ioMicroMSecs()
    }
}

/// `loadBitBltFrom:` — hand the BitBlt oop to the loaded BitBlt plugin.
fn loadBitBltFrom(bb_obj: SqInt) -> SqInt {
    let mut f = with_globals(|g| g.loadBBFn);
    if f.is_null() {
        // We need copyBits here, so try to load it implicitly.
        if !initialiseModule_hook() {
            return 0;
        }
        f = with_globals(|g| g.loadBBFn);
        if f.is_null() {
            return 0;
        }
    }
    // SAFETY: symbol resolved by name from the BitBlt plugin; this is its C
    // signature.
    let f: unsafe extern "C" fn(SqInt) -> SqInt = unsafe { core::mem::transmute(f) };
    unsafe { f(bb_obj) }
}

/// A fresh per-call engine over the VM-backed host, with the profiling flag
/// the C keeps in `doProfileStats`.
fn new_engine(vm: Vm) -> Engine<VmHost> {
    let mut e = Engine::new(VmHost { vm });
    e.doProfileStats = with_globals(|g| g.doProfileStats);
    e
}

// ---------------------------------------------------------------------------
// State loading / storing (the oop-facing halves)
// ---------------------------------------------------------------------------

/// `loadWorkBufferFrom:` — 0 on success or a `GEF*` code.
fn loadWorkBufferFrom(vm: Vm, e: &mut Engine<VmHost>, wb_oop: SqInt) -> SqInt {
    if vm.isImmediate(wb_oop) {
        return GEFWorkBufferIsInteger;
    }
    if !vm.isWords(wb_oop) {
        return GEFWorkBufferIsPointers;
    }
    let slots = vm.slotSizeOf(wb_oop);
    if slots < GWMinimalSize {
        return GEFWorkBufferTooSmall;
    }
    // SAFETY: a words object's indexable fields are `slots` 32-bit cells, and
    // nothing in a primitive moves the object (no allocation happens while
    // the engine holds this pointer).
    unsafe {
        e.set_work_buffer(vm.firstIndexableField(wb_oop) as *mut i32, slots as usize);
    }
    e.attach_work_buffer_checked(slots)
}

/// `loadSpanBufferFrom:` — 0 on success or a `GEF*` code.
fn loadSpanBufferFrom(vm: Vm, e: &mut Engine<VmHost>, span_oop: SqInt) -> SqInt {
    if vm.fetchClassOf(span_oop) != vm.classBitmap() {
        return GEFClassMismatch;
    }
    let slots = vm.slotSizeOf(span_oop);
    // SAFETY: as in loadWorkBufferFrom.
    unsafe {
        e.set_span_buffer(vm.firstIndexableField(span_oop) as *mut u32, slots as usize);
    }
    // Leave the last entry unused to avoid complications (the C's comment).
    e.wb_put(GWSpanSize, slots - 1);
    0
}

/// `loadFormsFrom:` — validates every form and remembers the array.
fn loadFormsFrom(vm: Vm, array_oop: SqInt) -> bool {
    if !vm.isArray(array_oop) {
        return false;
    }
    with_globals(|g| g.formArray = array_oop);
    let count = vm.slotSizeOf(array_oop);
    for i in 0..count {
        let form_oop = vm.fetchPointerofObject(i, array_oop);
        if !vm.isPointers(form_oop) {
            return false;
        }
        if vm.slotSizeOf(form_oop) < 5 {
            return false;
        }
        let bm_bits = vm.fetchPointerofObject(0, form_oop);
        if vm.fetchClassOf(bm_bits) != vm.classBitmap() {
            return false;
        }
        let bm_bits_size = vm.slotSizeOf(bm_bits);
        let bm_width = vm.fetchIntegerofObject(1, form_oop);
        let bm_height = vm.fetchIntegerofObject(2, form_oop);
        let bm_depth = vm.fetchIntegerofObject(3, form_oop);
        if vm.failed() {
            return false;
        }
        if !((bm_width >= 0) && (bm_height >= 0)) {
            return false;
        }
        let ppw = 32 / bm_depth;
        let bm_raster = (bm_width + (ppw - 1)) / ppw;
        if bm_bits_size != bm_raster * bm_height {
            return false;
        }
    }
    true
}

/// `quickLoadEngineFrom:` — 0 on success or a `GEF*` code.
fn quickLoadEngineFrom(vm: Vm, e: &mut Engine<VmHost>, engine_oop: SqInt) -> SqInt {
    if vm.failed() {
        return GEFAlreadyFailed;
    }
    if vm.isImmediate(engine_oop) {
        return GEFEngineIsInteger;
    }
    if !vm.isPointers(engine_oop) {
        return GEFEngineIsWords;
    }
    if vm.slotSizeOf(engine_oop) < BEBalloonEngineSize {
        return GEFEngineTooSmall;
    }
    with_globals(|g| g.engine = engine_oop);
    let fail_code =
        loadWorkBufferFrom(vm, e, vm.fetchPointerofObject(BEWorkBufferIndex, engine_oop));
    if fail_code != 0 {
        return fail_code;
    }
    e.wb_put(GWStopReason, 0);
    e.objUsed = e.wb_at(GWObjUsed);
    e.engineStopped = false;
    0
}

/// `quickLoadEngineFrom:requiredState:`.
fn quickLoadEngineFromrequiredState(
    vm: Vm,
    e: &mut Engine<VmHost>,
    oop: SqInt,
    required_state: SqInt,
) -> SqInt {
    let failure_code = quickLoadEngineFrom(vm, e, oop);
    if failure_code != 0 {
        return failure_code;
    }
    if e.wb_at(GWState) == required_state {
        return 0;
    }
    e.wb_put(GWStopReason, GErrorBadState);
    GEFWrongState
}

/// `quickLoadEngineFrom:requiredState:or:`.
fn quickLoadEngineFromrequiredStateor(
    vm: Vm,
    e: &mut Engine<VmHost>,
    oop: SqInt,
    required_state: SqInt,
    alternative_state: SqInt,
) -> SqInt {
    let failure_code = quickLoadEngineFrom(vm, e, oop);
    if failure_code != 0 {
        return failure_code;
    }
    if e.wb_at(GWState) == required_state {
        return 0;
    }
    if e.wb_at(GWState) == alternative_state {
        return 0;
    }
    e.wb_put(GWStopReason, GErrorBadState);
    GEFWrongState
}

/// `storeEngineStateInto:` — only the objUsed write-back survives in the C.
fn storeEngineStateInto(e: &mut Engine<VmHost>) {
    let v = e.objUsed;
    e.wb_put(GWObjUsed, v);
}

/// `loadPoint:from:` — into a header point slot; fails the primitive on a
/// non-Point or non-numeric coordinate.
fn loadPointfrom(vm: Vm, e: &mut Engine<VmHost>, slot: SqInt, point_oop: SqInt) {
    if vm.fetchClassOf(point_oop) != vm.classPoint() {
        vm.primitiveFail();
        return;
    }
    let value = vm.fetchPointerofObject(0, point_oop);
    if !(vm.isIntegerObject(value) || vm.isFloatObject(value)) {
        vm.primitiveFail();
        return;
    }
    let x = if vm.isIntegerObject(value) {
        vm.integerValueOf(value)
    } else {
        vm.floatValueOf(value) as SqInt
    };
    e.wb_put(slot, x);
    let value = vm.fetchPointerofObject(1, point_oop);
    if !(vm.isIntegerObject(value) || vm.isFloatObject(value)) {
        vm.primitiveFail();
        return;
    }
    let y = if vm.isIntegerObject(value) {
        vm.integerValueOf(value)
    } else {
        vm.floatValueOf(value) as SqInt
    };
    e.wb_put(slot + 1, y);
}

/// `loadWordTransformFrom:into:length:` — a FloatArray source.
fn loadWordTransformFromintolength(
    vm: Vm,
    e: &mut Engine<VmHost>,
    transform_oop: SqInt,
    dest: SqInt,
    n: SqInt,
) {
    let src = vm.firstIndexableField(transform_oop) as *const u32;
    for i in 0..n {
        // SAFETY: slotSizeOf == n was checked by the caller.
        let bits = unsafe { *src.add(i as usize) };
        e.wb_f32_put(dest + i, f32::from_bits(bits));
    }
}

/// `loadArrayTransformFrom:into:length:` — an Array of numbers.
fn loadArrayTransformFromintolength(
    vm: Vm,
    e: &mut Engine<VmHost>,
    transform_oop: SqInt,
    dest: SqInt,
    n: SqInt,
) -> SqInt {
    for i in 0..n {
        let value = vm.fetchPointerofObject(i, transform_oop);
        if !(vm.isIntegerObject(value) || vm.isFloatObject(value)) {
            return vm.primitiveFail();
        }
        if vm.isIntegerObject(value) {
            e.wb_f32_put(dest + i, vm.integerValueOf(value) as f64 as f32);
        } else {
            e.wb_f32_put(dest + i, vm.floatValueOf(value) as f32);
        }
    }
    0
}

/// `loadTransformFrom:into:length:` — answers "have transform" (0 for nil),
/// or the value of `primitiveFail()` on a malformed source, as the C does.
fn loadTransformFromintolength(
    vm: Vm,
    e: &mut Engine<VmHost>,
    transform_oop: SqInt,
    dest: SqInt,
    n: SqInt,
) -> SqInt {
    if transform_oop == vm.nilObject() {
        return 0;
    }
    if vm.isImmediate(transform_oop) {
        return vm.primitiveFail();
    }
    if vm.slotSizeOf(transform_oop) != n {
        return vm.primitiveFail();
    }
    if vm.isWords(transform_oop) {
        loadWordTransformFromintolength(vm, e, transform_oop, dest, n);
    } else {
        loadArrayTransformFromintolength(vm, e, transform_oop, dest, n);
    }
    1
}

/// `loadEdgeTransformFrom:`.
fn loadEdgeTransformFrom(vm: Vm, e: &mut Engine<VmHost>, transform_oop: SqInt) -> SqInt {
    e.wb_put(GWHasEdgeTransform, 0);
    let okay = loadTransformFromintolength(vm, e, transform_oop, GWEdgeTransform, 6);
    if vm.failed() {
        return 0;
    }
    if okay == 0 {
        return 0;
    }
    e.wb_put(GWHasEdgeTransform, 1);
    let v = (e.wb_f32(GWEdgeTransform + 2) as f64 + e.wb_at(GWDestOffsetX) as f64) as f32;
    e.wb_f32_put(GWEdgeTransform + 2, v);
    let v = (e.wb_f32(GWEdgeTransform + 5) as f64 + e.wb_at(GWDestOffsetY) as f64) as f32;
    e.wb_f32_put(GWEdgeTransform + 5, v);
    1
}

/// `loadColorTransformFrom:`.
fn loadColorTransformFrom(vm: Vm, e: &mut Engine<VmHost>, transform_oop: SqInt) -> SqInt {
    e.wb_put(GWHasColorTransform, 0);
    let okay = loadTransformFromintolength(vm, e, transform_oop, GWColorTransform, 8);
    if okay == 0 {
        return 0;
    }
    e.wb_put(GWHasColorTransform, 1);
    for i in [1, 3, 5, 7] {
        let v = e.wb_f32(GWColorTransform + i) * 256.0f32;
        e.wb_f32_put(GWColorTransform + i, v);
    }
    okay
}

/// `loadEdgeStateFrom:` — the image hands back an updated external edge.
fn loadEdgeStateFrom(vm: Vm, e: &mut Engine<VmHost>, edge_oop: SqInt) -> SqInt {
    let edge = e.wb_at(GWLastExportedEdge);
    if vm.slotSizeOf(edge_oop) < ETBalloonEdgeDataSize {
        return 0;
    }
    let v = vm.fetchIntegerofObject(ETXValueIndex, edge_oop);
    e.objatput(edge, GEXValue, v);
    let v = vm.fetchIntegerofObject(ETYValueIndex, edge_oop);
    e.objatput(edge, GEYValue, v);
    let v = vm.fetchIntegerofObject(ETZValueIndex, edge_oop);
    e.objatput(edge, GEZValue, v);
    let v = vm.fetchIntegerofObject(ETLinesIndex, edge_oop);
    e.objatput(edge, GENumLines, v);
    edge
}

/// `storeEdgeStateFrom:into:`.
fn storeEdgeStateFrominto(vm: Vm, e: &mut Engine<VmHost>, edge: SqInt, edge_oop: SqInt) {
    if vm.slotSizeOf(edge_oop) < ETBalloonEdgeDataSize {
        vm.primitiveFail();
        return;
    }
    vm.storeIntegerofObjectwithValue(ETIndexIndex, edge_oop, e.objat(edge, GEObjectIndex));
    vm.storeIntegerofObjectwithValue(ETXValueIndex, edge_oop, e.objat(edge, GEXValue));
    vm.storeIntegerofObjectwithValue(ETYValueIndex, edge_oop, e.wb_at(GWCurrentY));
    vm.storeIntegerofObjectwithValue(ETZValueIndex, edge_oop, e.objat(edge, GEZValue));
    vm.storeIntegerofObjectwithValue(ETLinesIndex, edge_oop, e.objat(edge, GENumLines));
    e.wb_put(GWLastExportedEdge, edge);
}

/// `storeFillStateInto:`.
fn storeFillStateInto(vm: Vm, e: &mut Engine<VmHost>, fill_oop: SqInt) {
    let fill_index = e.wb_at(GWLastExportedFill);
    let left_x = e.wb_at(GWLastExportedLeftX);
    let right_x = e.wb_at(GWLastExportedRightX);
    if vm.slotSizeOf(fill_oop) < FTBalloonFillDataSize {
        vm.primitiveFail();
        return;
    }
    vm.storeIntegerofObjectwithValue(FTIndexIndex, fill_oop, e.objat(fill_index, GEObjectIndex));
    vm.storeIntegerofObjectwithValue(FTMinXIndex, fill_oop, left_x);
    vm.storeIntegerofObjectwithValue(FTMaxXIndex, fill_oop, right_x);
    vm.storeIntegerofObjectwithValue(FTYValueIndex, fill_oop, e.wb_at(GWCurrentY));
}

/// `loadRenderingState` — everything the rendering primitives need. Answers 0
/// or a failure code.
fn loadRenderingState(vm: Vm, e: &mut Engine<VmHost>) -> SqInt {
    if vm.methodArgumentCount() != 2 {
        return PrimErrBadNumArgs;
    }
    let fail_code = quickLoadEngineFrom(vm, e, vm.stackValue(2));
    if fail_code != 0 {
        return fail_code;
    }
    let fill_oop = vm.stackObjectValue(0);
    let edge_oop = vm.stackObjectValue(1);
    if vm.failed() {
        return PrimErrBadArgument;
    }
    let engine_oop = with_globals(|g| g.engine);
    let fail_code = loadSpanBufferFrom(vm, e, vm.fetchPointerofObject(BESpanIndex, engine_oop));
    if fail_code != 0 {
        return fail_code;
    }
    if loadBitBltFrom(vm.fetchPointerofObject(BEBitBltIndex, engine_oop)) == 0 {
        return GEFBitBltLoadFailed;
    }
    if !loadFormsFrom(vm, vm.fetchPointerofObject(BEFormsIndex, engine_oop)) {
        return GEFFormLoadFailed;
    }
    if vm.slotSizeOf(edge_oop) < ETBalloonEdgeDataSize {
        return GEFEdgeDataTooSmall;
    }
    if vm.slotSizeOf(fill_oop) < FTBalloonFillDataSize {
        return GEFFillDataTooSmall;
    }
    let state = e.wb_at(GWState);
    if (state == GEStateWaitingForEdge)
        || (state == GEStateWaitingForFill)
        || (state == GEStateWaitingChange)
    {
        return GEFWrongState;
    }
    0
}

/// `storeRenderingState` — including the stop-state export the image resumes
/// from; pops the 3 items and answers the stop reason.
fn storeRenderingState(vm: Vm, e: &mut Engine<VmHost>) {
    if vm.failed() {
        return;
    }
    if e.engineStopped {
        // Check the stop reason and store the required information.
        let edge_oop = vm.stackObjectValue(1);
        let fill_oop = vm.stackObjectValue(0);
        let reason = e.wb_at(GWStopReason);
        if reason == GErrorGETEntry {
            let edge = e.get_at(e.wb_at(GWGETStart)) as i32 as SqInt;
            storeEdgeStateFrominto(vm, e, edge, edge_oop);
            let v = e.wb_at(GWGETStart) + 1;
            e.wb_put(GWGETStart, v);
        }
        if reason == GErrorFillEntry {
            storeFillStateInto(vm, e, fill_oop);
        }
        if reason == GErrorAETEntry {
            let edge = e.aet_at(e.wb_at(GWAETStart)) as i32 as SqInt;
            storeEdgeStateFrominto(vm, e, edge, edge_oop);
        }
    }
    storeEngineStateInto(e);
    vm.pop(3);
    vm.pushInteger(e.wb_at(GWStopReason));
}

// --- compressed-shape validation -------------------------------------------

/// `checkCompressedFillIndexList:max:segments:`.
fn checkCompressedFillIndexListmaxsegments(
    vm: Vm,
    fill_list: SqInt,
    max_index: SqInt,
    n_segs: SqInt,
) -> bool {
    let length = vm.slotSizeOf(fill_list);
    let ptr = words_ref(vm, fill_list, length);
    let mut n_fills = 0;
    for i in 0..length {
        let w = ptr.int_at(i);
        let run_length = ((w as SqInt as usize) >> 16) as SqInt;
        let run_value = (w & 0xFFFF) as SqInt;
        if !((run_value >= 0) && (run_value <= max_index)) {
            return false;
        }
        n_fills += run_length;
    }
    n_fills == n_segs
}

/// `checkCompressedFills:`.
fn checkCompressedFills(vm: Vm, e: &Engine<VmHost>, index_list: SqInt) -> bool {
    // First check if the oops have the right format.
    if !vm.isWords(index_list) {
        return false;
    }
    let length = vm.slotSizeOf(index_list);
    let ptr = words_ref(vm, index_list, length);
    for i in 0..length {
        let fill_index = ptr.int_at(i);
        if !e.isFillOkay(fill_index as SqInt) {
            return false;
        }
    }
    true
}

/// `checkCompressedLineWidths:segments:`.
fn checkCompressedLineWidthssegments(vm: Vm, line_width_list: SqInt, n_segments: SqInt) -> bool {
    let length = vm.slotSizeOf(line_width_list);
    let ptr = words_ref(vm, line_width_list, length);
    let mut n_items = 0;
    for i in 0..length {
        let run_length = ((ptr.int_at(i) as SqInt as usize) >> 16) as SqInt;
        n_items += run_length;
    }
    n_items == n_segments
}

/// `checkCompressedPoints:segments:`.
fn checkCompressedPointssegments(vm: Vm, points: SqInt, n_segments: SqInt) -> bool {
    if !vm.isWords(points) {
        return false;
    }
    // Quadratic segments only: pSize = nSegments*3 (ShortPointArray) or *6
    // (PointArray).
    let p_size = vm.slotSizeOf(points);
    (p_size == n_segments * 3) || (p_size == n_segments * 6)
}

/// `checkCompressedShape:segments:leftFills:rightFills:lineWidths:lineFills:fillIndexList:`.
#[allow(clippy::too_many_arguments)]
fn checkCompressedShape(
    vm: Vm,
    e: &Engine<VmHost>,
    points: SqInt,
    n_segments: SqInt,
    left_fills: SqInt,
    right_fills: SqInt,
    line_widths: SqInt,
    line_fills: SqInt,
    fill_index_list: SqInt,
) -> bool {
    if !checkCompressedPointssegments(vm, points, n_segments) {
        return false;
    }
    if !checkCompressedFills(vm, e, fill_index_list) {
        return false;
    }
    let max_fill_index = vm.slotSizeOf(fill_index_list);
    if !checkCompressedFillIndexListmaxsegments(vm, left_fills, max_fill_index, n_segments) {
        return false;
    }
    if !checkCompressedFillIndexListmaxsegments(vm, right_fills, max_fill_index, n_segments) {
        return false;
    }
    if !checkCompressedFillIndexListmaxsegments(vm, line_fills, max_fill_index, n_segments) {
        return false;
    }
    if !checkCompressedLineWidthssegments(vm, line_widths, n_segments) {
        return false;
    }
    true
}

/// A [`PointsRef`] over a words object's indexable fields.
fn words_ref(vm: Vm, oop: SqInt, slots: SqInt) -> PointsRef {
    // SAFETY: the object is words-indexable with `slots` 32-bit fields, and
    // no allocation happens while the view is in use.
    unsafe { PointsRef::new(vm.firstIndexableField(oop) as *const i32, slots as usize) }
}

// --- oop-driven geometry loaders -------------------------------------------

/// `loadArrayPolygon:nPoints:fill:lineWidth:lineFill:` — an Array of Points.
fn loadArrayPolygonnPointsfilllineWidthlineFill(
    vm: Vm,
    e: &mut Engine<VmHost>,
    points: SqInt,
    n_points: SqInt,
    fill_index: SqInt,
    line_width: SqInt,
    line_fill: SqInt,
) {
    loadPointfrom(vm, e, GWPoint1, vm.fetchPointerofObject(0, points));
    if vm.failed() {
        return;
    }
    let mut x0 = e.point_x(GWPoint1);
    let mut y0 = e.point_y(GWPoint1);
    for i in 1..n_points {
        loadPointfrom(vm, e, GWPoint1, vm.fetchPointerofObject(i, points));
        if vm.failed() {
            return;
        }
        let x1 = e.point_x(GWPoint1);
        let y1 = e.point_y(GWPoint1);
        e.point_put(GWPoint1, x0, y0);
        e.point_put(GWPoint2, x1, y1);
        e.transformPoints(2);
        e.loadWideLinefromtolineFillleftFillrightFill(
            line_width, GWPoint1, GWPoint2, line_fill, fill_index, 0,
        );
        if e.engineStopped {
            return;
        }
        x0 = x1;
        y0 = y1;
    }
}

/// `loadArrayShape:nSegments:fill:lineWidth:lineFill:` — an Array of Points,
/// three per quadratic segment.
fn loadArrayShapenSegmentsfilllineWidthlineFill(
    vm: Vm,
    e: &mut Engine<VmHost>,
    points: SqInt,
    n_segments: SqInt,
    fill_index: SqInt,
    line_width: SqInt,
    line_fill: SqInt,
) {
    for i in 0..n_segments {
        let point_oop = vm.fetchPointerofObject(i * 3, points);
        loadPointfrom(vm, e, GWPoint1, point_oop);
        let point_oop = vm.fetchPointerofObject((i * 3) + 1, points);
        loadPointfrom(vm, e, GWPoint2, point_oop);
        let point_oop = vm.fetchPointerofObject((i * 3) + 2, points);
        loadPointfrom(vm, e, GWPoint3, point_oop);
        if vm.failed() {
            return;
        }
        e.transformPoints(3);
        let x0 = e.point_x(GWPoint1);
        let y0 = e.point_y(GWPoint1);
        let x1 = e.point_x(GWPoint2);
        let y1 = e.point_y(GWPoint2);
        let x2 = e.point_x(GWPoint3);
        let y2 = e.point_y(GWPoint3);
        // Check if we can use a line. NOTE: `x0 == y0 && x1 == y1` is what the
        // generated C tests (it looks like it meant `x0 == x1 && y0 == y1`);
        // reproduced verbatim for fidelity.
        if ((x0 == y0) && (x1 == y1)) || ((x1 == x2) && (y1 == y2)) {
            e.loadWideLinefromtolineFillleftFillrightFill(
                line_width, GWPoint1, GWPoint3, line_fill, fill_index, 0,
            );
        } else {
            // Need a bezier.
            let segs = e.loadAndSubdivideBezierFromviatoisWide(
                GWPoint1,
                GWPoint2,
                GWPoint3,
                (line_width != 0) && (line_fill != 0),
            );
            if e.engineStopped {
                return;
            }
            e.loadWideBezierlineFillleftFillrightFilln(
                line_width, line_fill, fill_index, 0, segs,
            );
        }
        if e.engineStopped {
            return;
        }
    }
}

/// `loadBitmapFill:colormap:tile:from:along:normal:xIndex:` — as inlined in
/// `primitiveAddBitmapFill` (the caller passes `xIndex - 1`). Answers the new
/// fill, or 0 with the failed flag / engineStopped set.
#[allow(clippy::too_many_arguments)]
fn loadBitmapFillcolormaptilefromalongnormalxIndex(
    vm: Vm,
    e: &mut Engine<VmHost>,
    form_oop: SqInt,
    cm_oop: SqInt,
    tile_flag: SqInt,
    x_index: SqInt,
) -> SqInt {
    let cm_size;
    let cm_bits;
    if cm_oop == vm.nilObject() {
        cm_size = 0;
        cm_bits = None;
    } else {
        if vm.fetchClassOf(cm_oop) != vm.classBitmap() {
            return vm.primitiveFail();
        }
        cm_size = vm.slotSizeOf(cm_oop);
        cm_bits = Some(words_ref(vm, cm_oop, cm_size));
    }
    if !vm.isPointers(form_oop) {
        return vm.primitiveFail();
    }
    if vm.slotSizeOf(form_oop) < 5 {
        return vm.primitiveFail();
    }
    let bm_bits = vm.fetchPointerofObject(0, form_oop);
    if vm.fetchClassOf(bm_bits) != vm.classBitmap() {
        return vm.primitiveFail();
    }
    let bm_bits_size = vm.slotSizeOf(bm_bits);
    let bm_width = vm.fetchIntegerofObject(1, form_oop);
    let bm_height = vm.fetchIntegerofObject(2, form_oop);
    let bm_depth = vm.fetchIntegerofObject(3, form_oop);
    if vm.failed() {
        return 0;
    }
    if !((bm_width >= 0) && (bm_height >= 0)) {
        return vm.primitiveFail();
    }
    if !((bm_depth == 32)
        || (bm_depth == 8)
        || (bm_depth == 16)
        || (bm_depth == 1)
        || (bm_depth == 2)
        || (bm_depth == 4))
    {
        return vm.primitiveFail();
    }
    if !((cm_size == 0) || (cm_size == (1 << bm_depth))) {
        return vm.primitiveFail();
    }
    let ppw = 32 / bm_depth;
    let bm_raster = (bm_width + (ppw - 1)) / ppw;
    if bm_bits_size != bm_raster * bm_height {
        return vm.primitiveFail();
    }
    let bm_fill = e.allocateBitmapFillcolormap(cm_size, cm_bits);
    if e.engineStopped {
        return 0;
    }
    e.objatput(bm_fill, GBBitmapWidth, bm_width);
    e.objatput(bm_fill, GBBitmapHeight, bm_height);
    e.objatput(bm_fill, GBBitmapDepth, bm_depth);
    e.objatput(bm_fill, GBBitmapRaster, bm_raster);
    e.objatput(bm_fill, GBBitmapSize, bm_bits_size);
    e.objatput(bm_fill, GBTileFlag, tile_flag);
    e.objatput(bm_fill, GEObjectIndex, x_index);
    e.loadFillOrientationfromalongnormalwidthheight(
        bm_fill, GWPoint1, GWPoint2, GWPoint3, bm_width, bm_height,
    );
    bm_fill
}

/// `loadGradientFill:from:along:normal:isRadial:`.
fn loadGradientFillfromalongnormalisRadial(
    vm: Vm,
    e: &mut Engine<VmHost>,
    ramp_oop: SqInt,
    is_radial: SqInt,
) -> SqInt {
    if vm.fetchClassOf(ramp_oop) != vm.classBitmap() {
        return vm.primitiveFail();
    }
    let ramp_width = vm.slotSizeOf(ramp_oop);
    let ramp = words_ref(vm, ramp_oop, ramp_width);
    let fill = e.allocateGradientFillrampWidthisRadial(ramp, ramp_width, is_radial != 0);
    if e.engineStopped {
        return 0;
    }
    e.loadFillOrientationfromalongnormalwidthheight(
        fill, GWPoint1, GWPoint2, GWPoint3, ramp_width, ramp_width,
    );
    fill
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// `primitiveAbortProcessing` — mark rendering completed.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAbortProcessing(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    e.wb_put(GWState, GEStateCompleted);
    storeEngineStateInto(&mut e);
    Ok(Managed)
}

/// `primitiveAddActiveEdgeEntry` — the image hands back an updated external
/// edge for insertion into the AET.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveAddActiveEdgeEntry(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateWaitingForEdge);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let edge_oop = vm.stackObjectValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let edge = loadEdgeStateFrom(vm, &mut e, edge_oop);
    if edge == 0 {
        vm.primitiveFailFor(GEFEdgeDataTooSmall);
        return Ok(Managed);
    }
    if !e.needAvailableSpace(1) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    if e.objat(edge, GENumLines) > 0 {
        e.insertEdgeIntoAET(edge);
    }
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    e.wb_put(GWState, GEStateAddingFromGET);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    if e.doProfileStats {
        e.incrementStatby(GWCountAddAETEntry, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeAddAETEntry, dt);
    }
    Ok(Managed)
}

/// `primitiveAddBezier`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddBezier(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut right_fill = vm.positive32BitValueOf(vm.stackValue(0));
    let mut left_fill = vm.positive32BitValueOf(vm.stackValue(1));
    let via_oop = vm.stackObjectValue(2);
    let end_oop = vm.stackObjectValue(3);
    let start_oop = vm.stackObjectValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !(e.isFillOkay(left_fill) && e.isFillOkay(right_fill)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    // (The C has an `if ((leftFill == rightFill) && 0)` early-out here that
    // can never fire.)
    loadPointfrom(vm, &mut e, GWPoint1, start_oop);
    loadPointfrom(vm, &mut e, GWPoint2, via_oop);
    loadPointfrom(vm, &mut e, GWPoint3, end_oop);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    e.transformPoints(3);
    let n_segments = e.loadAndSubdivideBezierFromviatoisWide(GWPoint1, GWPoint2, GWPoint3, false);
    e.needAvailableSpace(n_segments * GBBaseSize);
    if !e.engineStopped {
        left_fill = e.transformColor(left_fill);
        right_fill = e.transformColor(right_fill);
    }
    if !e.engineStopped {
        e.loadWideBezierlineFillleftFillrightFilln(0, 0, left_fill, right_fill, n_segments);
    }
    if e.engineStopped {
        // Make sure the stack is okay.
        e.wbStackClear();
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    vm.pop(5);
    Ok(Managed)
}

/// `primitiveAddBezierShape`.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveAddBezierShape(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut line_fill = vm.positive32BitValueOf(vm.stackValue(0));
    let mut line_width = vm.stackIntegerValue(1);
    let mut fill_index = vm.positive32BitValueOf(vm.stackValue(2));
    let n_segments = vm.stackIntegerValue(3);
    let points = vm.stackObjectValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let length = vm.slotSizeOf(points);
    let points_is_array;
    if vm.isWords(points) {
        // Either PointArray or ShortPointArray.
        points_is_array = false;
        if !((length == n_segments * 3) || (length == n_segments * 6)) {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
    } else {
        // Must be an Array of points.
        if !vm.isArray(points) {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
        if length != n_segments * 3 {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
        points_is_array = true;
    }
    let seg_size = if (line_width == 0) || (line_fill == 0) {
        GLBaseSize
    } else {
        GLWideSize
    };
    if !e.needAvailableSpace(seg_size * n_segments) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    if !(e.isFillOkay(line_fill) && e.isFillOkay(fill_index)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    line_fill = e.transformColor(line_fill);
    fill_index = e.transformColor(fill_index);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if ((line_fill == 0) || (line_width == 0)) && (fill_index == 0) {
        vm.pop(5);
        return Ok(Managed);
    }
    if line_width != 0 {
        line_width = e.transformWidth(line_width);
        if line_width < 1 {
            line_width = 1;
        }
    }
    if points_is_array {
        loadArrayShapenSegmentsfilllineWidthlineFill(
            vm, &mut e, points, n_segments, fill_index, line_width, line_fill,
        );
    } else {
        let pr = words_ref(vm, points, length);
        e.loadShapenSegmentsfilllineWidthlineFillpointsShort(
            pr,
            n_segments,
            fill_index,
            line_width,
            line_fill,
            (n_segments * 3) == length,
        );
    }
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    e.wb_put(GWNeedsFlush, 1);
    storeEngineStateInto(&mut e);
    vm.pop(5);
    Ok(Managed)
}

/// `primitiveAddBitmapFill`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddBitmapFill(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 7 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let x_index = vm.stackIntegerValue(0);
    if x_index <= 0 {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let nrm_oop = vm.stackObjectValue(1);
    let dir_oop = vm.stackObjectValue(2);
    let origin_oop = vm.stackObjectValue(3);
    let tile_flag = vm.booleanValueOf(vm.stackValue(4));
    let cm_oop = vm.stackObjectValue(5);
    let form_oop = vm.stackObjectValue(6);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(7), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    loadPointfrom(vm, &mut e, GWPoint1, origin_oop);
    loadPointfrom(vm, &mut e, GWPoint2, dir_oop);
    loadPointfrom(vm, &mut e, GWPoint3, nrm_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFBadPoint);
        return Ok(Managed);
    }
    let tile_flag1 = if tile_flag != 0 { 1 } else { 0 };
    let fill = loadBitmapFillcolormaptilefromalongnormalxIndex(
        vm,
        &mut e,
        form_oop,
        cm_oop,
        tile_flag1,
        x_index - 1,
    );
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    let oop = vm.positive32BitIntegerFor(fill);
    vm.popthenPush(8, oop);
    Ok(Managed)
}

/// `primitiveAddCompressedShape`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddCompressedShape(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 7 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let fill_index_list = vm.stackObjectValue(0);
    let line_fills = vm.stackObjectValue(1);
    let line_widths = vm.stackObjectValue(2);
    let right_fills = vm.stackObjectValue(3);
    let left_fills = vm.stackObjectValue(4);
    let n_segments = vm.stackIntegerValue(5);
    let points = vm.stackObjectValue(6);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(7), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !checkCompressedShape(
        vm,
        &e,
        points,
        n_segments,
        left_fills,
        right_fills,
        line_widths,
        line_fills,
        fill_index_list,
    ) {
        vm.primitiveFailFor(GEFEntityCheckFailed);
        return Ok(Managed);
    }
    let max_size = if GBBaseSize < GLBaseSize {
        GLBaseSize
    } else {
        GBBaseSize
    };
    if !e.needAvailableSpace(max_size * n_segments) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    // Then actually load the compressed shape.
    let points_short = vm.slotSizeOf(points) == n_segments * 3;
    let points_ref = words_ref(vm, points, vm.slotSizeOf(points));
    let left_ref = words_ref(vm, left_fills, vm.slotSizeOf(left_fills));
    let right_ref = words_ref(vm, right_fills, vm.slotSizeOf(right_fills));
    let widths_ref = words_ref(vm, line_widths, vm.slotSizeOf(line_widths));
    let line_fills_ref = words_ref(vm, line_fills, vm.slotSizeOf(line_fills));
    let index_list_ref = words_ref(vm, fill_index_list, vm.slotSizeOf(fill_index_list));
    e.loadCompressedShapesegmentsleftFillsrightFillslineWidthslineFillsfillIndexListpointShort(
        points_ref,
        n_segments,
        left_ref,
        right_ref,
        widths_ref,
        line_fills_ref,
        index_list_ref,
        points_short,
    );
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    e.wb_put(GWNeedsFlush, 1);
    storeEngineStateInto(&mut e);
    vm.pop(7);
    Ok(Managed)
}

/// `primitiveAddGradientFill`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddGradientFill(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let is_radial = vm.booleanValueOf(vm.stackValue(0));
    let nrm_oop = vm.stackValue(1);
    let dir_oop = vm.stackValue(2);
    let origin_oop = vm.stackValue(3);
    let ramp_oop = vm.stackValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    loadPointfrom(vm, &mut e, GWPoint1, origin_oop);
    loadPointfrom(vm, &mut e, GWPoint2, dir_oop);
    loadPointfrom(vm, &mut e, GWPoint3, nrm_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFBadPoint);
        return Ok(Managed);
    }
    let fill = loadGradientFillfromalongnormalisRadial(vm, &mut e, ramp_oop, is_radial);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    let oop = vm.positive32BitIntegerFor(fill);
    vm.popthenPush(6, oop);
    Ok(Managed)
}

/// `primitiveAddLine`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddLine(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 4 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut right_fill = vm.positive32BitValueOf(vm.stackValue(0));
    let mut left_fill = vm.positive32BitValueOf(vm.stackValue(1));
    let end_oop = vm.stackObjectValue(2);
    let start_oop = vm.stackObjectValue(3);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(4), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !(e.isFillOkay(left_fill) && e.isFillOkay(right_fill)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    loadPointfrom(vm, &mut e, GWPoint1, start_oop);
    loadPointfrom(vm, &mut e, GWPoint2, end_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFBadPoint);
        return Ok(Managed);
    }
    e.transformPoints(2);
    left_fill = e.transformColor(left_fill);
    right_fill = e.transformColor(right_fill);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    e.loadWideLinefromtolineFillleftFillrightFill(0, GWPoint1, GWPoint2, 0, left_fill, right_fill);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    vm.pop(4);
    Ok(Managed)
}

/// `primitiveAddOval`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddOval(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut border_index = vm.positive32BitValueOf(vm.stackValue(0));
    let mut border_width = vm.stackIntegerValue(1);
    let mut fill_index = vm.positive32BitValueOf(vm.stackValue(2));
    let end_oop = vm.stackObjectValue(3);
    let start_oop = vm.stackObjectValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !(e.isFillOkay(border_index) && e.isFillOkay(fill_index)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    fill_index = e.transformColor(fill_index);
    border_index = e.transformColor(border_index);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if (fill_index == 0) && ((border_index == 0) || (border_width <= 0)) {
        vm.pop(5);
        return Ok(Managed);
    }
    if !e.needAvailableSpace(16 * GBBaseSize) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    if (border_width > 0) && (border_index != 0) {
        border_width = e.transformWidth(border_width);
    } else {
        border_width = 0;
    }
    loadPointfrom(vm, &mut e, GWPoint1, start_oop);
    loadPointfrom(vm, &mut e, GWPoint2, end_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFBadPoint);
        return Ok(Managed);
    }
    e.loadOvallineFillleftFillrightFill(border_width, border_index, 0, fill_index);
    if e.engineStopped {
        e.wbStackClear();
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    e.wb_put(GWNeedsFlush, 1);
    storeEngineStateInto(&mut e);
    vm.pop(5);
    Ok(Managed)
}

/// `primitiveAddPolygon`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddPolygon(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut line_fill = vm.positive32BitValueOf(vm.stackValue(0));
    let mut line_width = vm.stackIntegerValue(1);
    let mut fill_index = vm.positive32BitValueOf(vm.stackValue(2));
    let n_points = vm.stackIntegerValue(3);
    let points = vm.stackObjectValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let length = vm.slotSizeOf(points);
    let points_is_array;
    if vm.isWords(points) {
        // Either PointArray or ShortPointArray.
        points_is_array = false;
        if !((length == n_points) || ((n_points * 2) == length)) {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
    } else {
        if !vm.isArray(points) {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
        if length != n_points {
            vm.primitiveFailFor(PrimErrBadArgument);
            return Ok(Managed);
        }
        points_is_array = true;
    }
    let seg_size = if (line_width == 0) || (line_fill == 0) {
        GLBaseSize
    } else {
        GLWideSize
    };
    if !e.needAvailableSpace(seg_size * n_points) {
        // NOTE: a bare primitiveFail here, unlike primitiveAddBezierShape.
        vm.primitiveFail();
        return Ok(Managed);
    }
    if !(e.isFillOkay(line_fill) && e.isFillOkay(fill_index)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    line_fill = e.transformColor(line_fill);
    fill_index = e.transformColor(fill_index);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if ((line_fill == 0) || (line_width == 0)) && (fill_index == 0) {
        vm.pop(5);
        return Ok(Managed);
    }
    if line_width != 0 {
        line_width = e.transformWidth(line_width);
    }
    if points_is_array {
        loadArrayPolygonnPointsfilllineWidthlineFill(
            vm, &mut e, points, n_points, fill_index, line_width, line_fill,
        );
    } else {
        let pr = words_ref(vm, points, length);
        e.loadPolygonnPointsfilllineWidthlineFillpointsShort(
            pr,
            n_points,
            fill_index,
            line_width,
            line_fill,
            n_points == length,
        );
    }
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    e.wb_put(GWNeedsFlush, 1);
    storeEngineStateInto(&mut e);
    vm.pop(5);
    Ok(Managed)
}

/// `primitiveAddRect`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveAddRect(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 5 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let mut border_index = vm.positive32BitValueOf(vm.stackValue(0));
    let mut border_width = vm.stackIntegerValue(1);
    let mut fill_index = vm.positive32BitValueOf(vm.stackValue(2));
    let end_oop = vm.stackObjectValue(3);
    let start_oop = vm.stackObjectValue(4);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(5), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !(e.isFillOkay(border_index) && e.isFillOkay(fill_index)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    border_index = e.transformColor(border_index);
    fill_index = e.transformColor(fill_index);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if (fill_index == 0) && ((border_index == 0) || (border_width == 0)) {
        vm.pop(5);
        return Ok(Managed);
    }
    if !e.needAvailableSpace(4 * GLBaseSize) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    if (border_width > 0) && (border_index != 0) {
        border_width = e.transformWidth(border_width);
    } else {
        border_width = 0;
    }
    loadPointfrom(vm, &mut e, GWPoint1, start_oop);
    loadPointfrom(vm, &mut e, GWPoint3, end_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFBadPoint);
        return Ok(Managed);
    }
    e.makeRectFromPoints();
    e.transformPoints(4);
    e.loadRectanglelineFillleftFillrightFill(border_width, border_index, 0, fill_index);
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    e.wb_put(GWNeedsFlush, 1);
    storeEngineStateInto(&mut e);
    vm.pop(5);
    Ok(Managed)
}

/// `primitiveChangedActiveEdgeEntry`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveChangedActiveEdgeEntry(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateWaitingChange);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let edge_oop = vm.stackObjectValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let edge = loadEdgeStateFrom(vm, &mut e, edge_oop);
    if edge == 0 {
        vm.primitiveFailFor(GEFEdgeDataTooSmall);
        return Ok(Managed);
    }
    if e.objat(edge, GENumLines) == 0 {
        e.removeFirstAETEntry();
    } else {
        e.resortFirstAETEntry();
        let v = e.wb_at(GWAETStart) + 1;
        e.wb_put(GWAETStart, v);
    }
    e.wb_put(GWState, GEStateUpdateEdges);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    if e.doProfileStats {
        e.incrementStatby(GWCountChangeAETEntry, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeChangeAETEntry, dt);
    }
    Ok(Managed)
}

/// `primitiveCopyBuffer` — copy a work buffer into a (possibly larger) one.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveCopyBuffer(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 2 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let buf2 = vm.stackValue(0);
    // Make sure the old buffer is properly initialized.
    let buf1 = vm.stackValue(1);
    let fail_code = loadWorkBufferFrom(vm, &mut e, buf1);
    if fail_code != 0 {
        vm.primitiveFailFor(fail_code);
        return Ok(Managed);
    }
    if vm.fetchClassOf(buf1) != vm.fetchClassOf(buf2) {
        vm.primitiveFailFor(GEFClassMismatch);
        return Ok(Managed);
    }
    let diff = vm.slotSizeOf(buf2) - vm.slotSizeOf(buf1);
    if diff < 0 {
        vm.primitiveFailFor(GEFSizeMismatch);
        return Ok(Managed);
    }
    let dst_len = vm.slotSizeOf(buf2) as usize;
    let dst_ptr = vm.firstIndexableField(buf2) as *mut i32;
    // SAFETY: buf2 is a words object of dst_len 32-bit cells (same class as
    // the validated buf1) and no allocation happens during the copy.
    let dst = unsafe { core::slice::from_raw_parts_mut(dst_ptr, dst_len) };
    let buffer_top = e.wb_at(GWBufferTop);
    for i in 0..buffer_top {
        dst[i as usize] = e.wb_i32(i);
    }
    dst[GWBufferTop as usize] = (e.wb_at(GWBufferTop) + diff) as i32;
    dst[GWSize as usize] = (e.wb_at(GWSize) + diff) as i32;
    let stack_size = e.wb_at(GWSize) - e.wb_at(GWBufferTop);
    for i in 0..stack_size {
        dst[(buffer_top + diff + i) as usize] = e.wb_i32(buffer_top + i);
    }
    let fail_code = loadWorkBufferFrom(vm, &mut e, buf2);
    if fail_code != 0 {
        vm.primitiveFailFor(fail_code);
        return Ok(Managed);
    }
    vm.pop(2);
    Ok(Managed)
}

/// `primitiveDisplaySpanBuffer` — blit the current scan line via BitBlt.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveDisplaySpanBuffer(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(0), GEStateBlitBuffer);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let engine_oop = with_globals(|g| g.engine);
    let failure_code =
        loadSpanBufferFrom(vm, &mut e, vm.fetchPointerofObject(BESpanIndex, engine_oop));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if loadBitBltFrom(vm.fetchPointerofObject(BEBitBltIndex, engine_oop)) == 0 {
        vm.primitiveFailFor(GEFBitBltLoadFailed);
        return Ok(Managed);
    }
    if (e.wb_at(GWCurrentY) & e.wb_at(GWAAScanMask)) == e.wb_at(GWAAScanMask) {
        let y = e.wb_at(GWCurrentY);
        e.displaySpanBufferAt(y);
        e.postDisplayAction();
    }
    if e.wb_at(GWState) != GEStateCompleted {
        e.wb_put(GWAETStart, 0);
        let v = e.wb_at(GWCurrentY) + 1;
        e.wb_put(GWCurrentY, v);
        e.wb_put(GWState, GEStateUpdateEdges);
    }
    storeEngineStateInto(&mut e);
    if e.doProfileStats {
        e.incrementStatby(GWCountDisplaySpan, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeDisplaySpan, dt);
    }
    Ok(Managed)
}

/// `primitiveDoProfileStats` — toggle profiling; answers the old value.
/// (The C performs no argument-count check here.)
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDoProfileStats(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let old_value = with_globals(|g| g.doProfileStats);
    let new_value = vm.stackObjectValue(0);
    let new_value = vm.booleanValueOf(new_value);
    if !vm.failed() {
        with_globals(|g| g.doProfileStats = new_value != 0);
        vm.pop(2);
        vm.pushBool(old_value as SqInt);
    }
    Ok(Managed)
}

/// `primitiveFinishedProcessing`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveFinishedProcessing(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let finished = e.finishedProcessing();
    storeEngineStateInto(&mut e);
    vm.pop(1);
    vm.pushBool(finished as SqInt);
    if e.doProfileStats {
        e.incrementStatby(GWCountFinishTest, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeFinishTest, dt);
    }
    Ok(Managed)
}

/// `primitiveGetAALevel`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetAALevel(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    vm.pop(1);
    vm.pushInteger(e.wb_at(GWAALevel));
    Ok(Managed)
}

/// `primitiveGetBezierStats`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetBezierStats(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(1));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let stat_oop = vm.stackObjectValue(0);
    if !(!vm.failed() && vm.isWords(stat_oop) && vm.slotSizeOf(stat_oop) >= 4) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let stats = vm.firstIndexableField(stat_oop) as *mut i32;
    for (i, idx) in [
        GWBezierMonotonSubdivisions,
        GWBezierHeightSubdivisions,
        GWBezierOverflowSubdivisions,
        GWBezierLineConversions,
    ]
    .iter()
    .enumerate()
    {
        // SAFETY: at least 4 slots, checked above.
        unsafe {
            *stats.add(i) = (*stats.add(i)).wrapping_add(e.wb_i32(*idx));
        }
    }
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveGetClipRect`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetClipRect(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(1));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let rect_oop = vm.stackObjectValue(0);
    if !(!vm.failed() && vm.isPointers(rect_oop) && vm.slotSizeOf(rect_oop) >= 2) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    // makePoint can allocate; keep rectOop remapped across it, as the C does.
    let min_x = e.wb_at(GWClipMinX);
    let min_y = e.wb_at(GWClipMinY);
    let max_x = e.wb_at(GWClipMaxX);
    let max_y = e.wb_at(GWClipMaxY);
    vm.pushRemappableOop(rect_oop);
    let point_oop = vm.makePointwithxValueyValue(min_x, min_y);
    let top = vm.topRemappableOop();
    vm.storePointerofObjectwithValue(0, top, point_oop);
    let point_oop = vm.makePointwithxValueyValue(max_x, max_y);
    let rect_oop = vm.popRemappableOop();
    vm.storePointerofObjectwithValue(1, rect_oop, point_oop);
    vm.popthenPush(2, rect_oop);
    Ok(Managed)
}

/// `primitiveGetCounts`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetCounts(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(1));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let stat_oop = vm.stackObjectValue(0);
    if !(!vm.failed() && vm.isWords(stat_oop) && vm.slotSizeOf(stat_oop) >= 9) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let stats = vm.firstIndexableField(stat_oop) as *mut i32;
    for (i, idx) in [
        GWCountInitializing,
        GWCountFinishTest,
        GWCountNextGETEntry,
        GWCountAddAETEntry,
        GWCountNextFillEntry,
        GWCountMergeFill,
        GWCountDisplaySpan,
        GWCountNextAETEntry,
        GWCountChangeAETEntry,
    ]
    .iter()
    .enumerate()
    {
        // SAFETY: at least 9 slots, checked above.
        unsafe {
            *stats.add(i) = (*stats.add(i)).wrapping_add(e.wb_i32(*idx));
        }
    }
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveGetDepth`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetDepth(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    vm.pop(1);
    vm.pushInteger(e.wb_at(GWCurrentZ));
    Ok(Managed)
}

/// `primitiveGetFailureReason` — deliberately does not use
/// `quickLoadEngineFrom:` so the stop reason survives.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetFailureReason(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let engine_oop = vm.stackValue(0);
    with_globals(|g| g.engine = engine_oop);
    if vm.isImmediate(engine_oop) {
        vm.primitiveFailFor(GEFEngineIsInteger);
        return Ok(Managed);
    }
    if !vm.isPointers(engine_oop) {
        vm.primitiveFailFor(GEFEngineIsWords);
        return Ok(Managed);
    }
    if vm.slotSizeOf(engine_oop) < BEBalloonEngineSize {
        vm.primitiveFailFor(GEFEngineTooSmall);
        return Ok(Managed);
    }
    let fail_code =
        loadWorkBufferFrom(vm, &mut e, vm.fetchPointerofObject(BEWorkBufferIndex, engine_oop));
    if fail_code != 0 {
        vm.primitiveFailFor(fail_code);
        return Ok(Managed);
    }
    vm.pop(1);
    vm.pushInteger(e.wb_at(GWStopReason));
    Ok(Managed)
}

/// `primitiveGetOffset`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetOffset(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let x = e.wb_at(GWDestOffsetX);
    let y = e.wb_at(GWDestOffsetY);
    let point_oop = vm.makePointwithxValueyValue(x, y);
    vm.popthenPush(1, point_oop);
    Ok(Managed)
}

/// `primitiveGetTimes`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveGetTimes(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(1));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let stat_oop = vm.stackObjectValue(0);
    if !(!vm.failed() && vm.isWords(stat_oop) && vm.slotSizeOf(stat_oop) >= 9) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let stats = vm.firstIndexableField(stat_oop) as *mut i32;
    for (i, idx) in [
        GWTimeInitializing,
        GWTimeFinishTest,
        GWTimeNextGETEntry,
        GWTimeAddAETEntry,
        GWTimeNextFillEntry,
        GWTimeMergeFill,
        GWTimeDisplaySpan,
        GWTimeNextAETEntry,
        GWTimeChangeAETEntry,
    ]
    .iter()
    .enumerate()
    {
        // SAFETY: at least 9 slots, checked above.
        unsafe {
            *stats.add(i) = (*stats.add(i)).wrapping_add(e.wb_i32(*idx));
        }
    }
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveInitializeBuffer` — lay out a fresh work buffer.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveInitializeBuffer(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFail();
        return Ok(Managed);
    }
    let wb_oop = vm.stackValue(0);
    if !vm.isWords(wb_oop) {
        vm.primitiveFail();
        return Ok(Managed);
    }
    let size = vm.slotSizeOf(wb_oop);
    if size < GWMinimalSize {
        vm.primitiveFail();
        return Ok(Managed);
    }
    // SAFETY: words object with `size` 32-bit cells; nothing allocates while
    // the engine writes the header.
    unsafe {
        e.set_work_buffer(vm.firstIndexableField(wb_oop) as *mut i32, size as usize);
    }
    e.initializeBuffer(size);
    vm.popthenPush(2, wb_oop);
    Ok(Managed)
}

/// `primitiveInitializeProcessing` — build and sort the GET.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveInitializeProcessing(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(0), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let engine_oop = with_globals(|g| g.engine);
    let failure_code =
        loadSpanBufferFrom(vm, &mut e, vm.fetchPointerofObject(BESpanIndex, engine_oop));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    e.initializeGETProcessing();
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    e.wb_put(GWState, GEStateAddingFromGET);
    if !vm.failed() {
        storeEngineStateInto(&mut e);
    }
    if e.doProfileStats {
        e.incrementStatby(GWCountInitializing, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeInitializing, dt);
    }
    Ok(Managed)
}

/// `primitiveMergeFillFrom` — merge an externally rendered fill's bitmap into
/// the span buffer.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveMergeFillFrom(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 2 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(2), GEStateWaitingForFill);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let engine_oop = with_globals(|g| g.engine);
    let failure_code =
        loadSpanBufferFrom(vm, &mut e, vm.fetchPointerofObject(BESpanIndex, engine_oop));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let fill_oop = vm.stackObjectValue(0);
    // Check the bitmap.
    let bits_oop = vm.stackObjectValue(1);
    if !(!vm.failed() && vm.fetchClassOf(bits_oop) == vm.classBitmap()) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    if vm.slotSizeOf(fill_oop) < FTBalloonFillDataSize {
        vm.primitiveFailFor(GEFFillDataTooSmall);
        return Ok(Managed);
    }
    let value = vm.fetchIntegerofObject(FTIndexIndex, fill_oop);
    if e.objat(e.wb_at(GWLastExportedFill), GEObjectIndex) != value {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    let value = vm.fetchIntegerofObject(FTMinXIndex, fill_oop);
    if e.wb_at(GWLastExportedLeftX) != value {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    let value = vm.fetchIntegerofObject(FTMaxXIndex, fill_oop);
    if e.wb_at(GWLastExportedRightX) != value {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    if vm.slotSizeOf(bits_oop) < e.wb_at(GWLastExportedRightX) - e.wb_at(GWLastExportedLeftX) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    if vm.failed() {
        return Ok(Managed);
    }
    let bits_len = vm.slotSizeOf(bits_oop) as usize;
    let bits_ptr = vm.firstIndexableField(bits_oop) as *const i32;
    // SAFETY: a Bitmap's indexable fields are bits_len 32-bit cells; no
    // allocation happens during the merge.
    let bits = unsafe { core::slice::from_raw_parts(bits_ptr, bits_len) };
    let lx = e.wb_at(GWLastExportedLeftX);
    let rx = e.wb_at(GWLastExportedRightX);
    e.fillBitmapSpanfromto(bits, lx, rx);
    e.wb_put(GWState, GEStateScanningAET);
    storeEngineStateInto(&mut e);
    vm.pop(2);
    if e.doProfileStats {
        e.incrementStatby(GWCountMergeFill, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeMergeFill, dt);
    }
    Ok(Managed)
}

/// `primitiveNeedsFlush`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveNeedsFlush(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 0 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(0));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let need_flush = e.wb_at(GWNeedsFlush) != 0;
    storeEngineStateInto(&mut e);
    vm.pop(1);
    vm.pushBool(need_flush as SqInt);
    Ok(Managed)
}

/// `primitiveNeedsFlushPut`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveNeedsFlushPut(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFrom(vm, &mut e, vm.stackValue(1));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let need_flush = vm.booleanValueOf(vm.stackValue(0));
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    if need_flush == 1 {
        e.wb_put(GWNeedsFlush, 1);
    } else {
        e.wb_put(GWNeedsFlush, 0);
    }
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveNextActiveEdgeEntry` — advance the update phase up to the next
/// external edge; answers whether the AET scan is finished.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveNextActiveEdgeEntry(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code = quickLoadEngineFromrequiredStateor(
        vm,
        &mut e,
        vm.stackValue(1),
        GEStateUpdateEdges,
        GEStateCompleted,
    );
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let edge_oop = vm.stackObjectValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let mut has_edge = false;
    if e.wb_at(GWState) != GEStateCompleted {
        has_edge = e.findNextExternalUpdateFromAET();
        if has_edge {
            let edge = e.aet_at(e.wb_at(GWAETStart)) as i32 as SqInt;
            storeEdgeStateFrominto(vm, &mut e, edge, edge_oop);
            e.wb_put(GWState, GEStateWaitingChange);
        } else {
            e.wb_put(GWState, GEStateAddingFromGET);
        }
    }
    if vm.failed() {
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    vm.pop(2);
    vm.pushBool(!has_edge as SqInt);
    if e.doProfileStats {
        e.incrementStatby(GWCountNextAETEntry, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeNextAETEntry, dt);
    }
    Ok(Managed)
}

/// `primitiveNextFillEntry` — scan the AET fills up to the next external
/// fill; answers whether the scan finished.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveNextFillEntry(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateScanningAET);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let engine_oop = with_globals(|g| g.engine);
    let failure_code =
        loadSpanBufferFrom(vm, &mut e, vm.fetchPointerofObject(BESpanIndex, engine_oop));
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    if !loadFormsFrom(vm, vm.fetchPointerofObject(BEFormsIndex, engine_oop)) {
        vm.primitiveFailFor(GEFFormLoadFailed);
        return Ok(Managed);
    }
    if e.wb_at(GWClearSpanBuffer) != 0 {
        if (e.wb_at(GWCurrentY) & e.wb_at(GWAAScanMask)) == 0 {
            e.clearSpanBuffer();
        }
        e.wb_put(GWClearSpanBuffer, 0);
    }
    let fill_oop = vm.stackObjectValue(0);
    let has_fill = e.findNextExternalFillFromAET();
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if has_fill {
        storeFillStateInto(vm, &mut e, fill_oop);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    if has_fill {
        e.wb_put(GWState, GEStateWaitingForFill);
    } else {
        e.wbStackClear();
        e.wb_put(GWSpanEndAA, 0);
        e.wb_put(GWState, GEStateBlitBuffer);
    }
    storeEngineStateInto(&mut e);
    vm.pop(2);
    vm.pushBool(!has_fill as SqInt);
    if e.doProfileStats {
        e.incrementStatby(GWCountNextFillEntry, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeNextFillEntry, dt);
    }
    Ok(Managed)
}

/// `primitiveNextGlobalEdgeEntry` — advance the GET phase up to the next
/// external edge; answers whether the GET is exhausted.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveNextGlobalEdgeEntry(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let mut ge_profile_time = 0;
    if e.doProfileStats {
        ge_profile_time = vm.ioMicroMSecs();
    }
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateAddingFromGET);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let edge_oop = vm.stackObjectValue(0);
    let has_edge = e.findNextExternalEntryFromGET();
    if has_edge {
        let edge = e.get_at(e.wb_at(GWGETStart)) as i32 as SqInt;
        storeEdgeStateFrominto(vm, &mut e, edge, edge_oop);
        let v = e.wb_at(GWGETStart) + 1;
        e.wb_put(GWGETStart, v);
    }
    if vm.failed() {
        vm.primitiveFailFor(GEFWrongEdge);
        return Ok(Managed);
    }
    if has_edge {
        e.wb_put(GWState, GEStateWaitingForEdge);
    } else {
        // Start scanning the AET.
        e.wb_put(GWState, GEStateScanningAET);
        e.wb_put(GWClearSpanBuffer, 1);
        e.wb_put(GWAETStart, 0);
        e.wbStackClear();
    }
    storeEngineStateInto(&mut e);
    vm.pop(2);
    vm.pushBool(!has_edge as SqInt);
    if e.doProfileStats {
        e.incrementStatby(GWCountNextGETEntry, 1);
        let dt = vm.ioMicroMSecs() - ge_profile_time;
        e.incrementStatby(GWTimeNextGETEntry, dt);
    }
    Ok(Managed)
}

/// `primitiveRegisterExternalEdge`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveRegisterExternalEdge(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 6 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(6), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let right_fill_index = vm.positive32BitValueOf(vm.stackValue(0));
    let left_fill_index = vm.positive32BitValueOf(vm.stackValue(1));
    let initial_z = vm.stackIntegerValue(2);
    let initial_y = vm.stackIntegerValue(3);
    let initial_x = vm.stackIntegerValue(4);
    let index = vm.stackIntegerValue(5);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    if !e.allocateObjEntry(GEBaseEdgeSize) {
        vm.primitiveFailFor(GEFWorkTooBig);
        return Ok(Managed);
    }
    if !(e.isFillOkay(left_fill_index) && e.isFillOkay(right_fill_index)) {
        vm.primitiveFailFor(GEFWrongFill);
        return Ok(Managed);
    }
    let edge = e.objUsed;
    // Install type and length.
    e.objUsed = edge + GEBaseEdgeSize;
    e.objatput(edge, GEObjectType, GEPrimitiveEdge);
    e.objatput(edge, GEObjectLength, GEBaseEdgeSize);
    e.objatput(edge, GEObjectIndex, index);
    e.objatput(edge, GEXValue, initial_x);
    e.objatput(edge, GEYValue, initial_y);
    e.objatput(edge, GEZValue, initial_z);
    let v = e.transformColor(left_fill_index);
    e.objatput(edge, GEFillIndexLeft, v);
    let v = e.transformColor(right_fill_index);
    e.objatput(edge, GEFillIndexRight, v);
    if e.engineStopped {
        vm.primitiveFailFor(GEFEngineStopped);
        return Ok(Managed);
    }
    if !vm.failed() {
        storeEngineStateInto(&mut e);
        vm.pop(6);
    }
    Ok(Managed)
}

/// `primitiveRegisterExternalFill`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveRegisterExternalFill(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let index = vm.stackIntegerValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let mut fill = 0;
    while fill == 0 {
        if !e.allocateObjEntry(GEBaseEdgeSize) {
            vm.primitiveFailFor(GEFWorkTooBig);
            return Ok(Managed);
        }
        fill = e.objUsed;
        // Install type and length.
        e.objUsed = fill + GEBaseFillSize;
        e.objatput(fill, GEObjectType, GEPrimitiveFill);
        e.objatput(fill, GEObjectLength, GEBaseFillSize);
        e.objatput(fill, GEObjectIndex, index);
    }
    if !vm.failed() {
        storeEngineStateInto(&mut e);
        vm.pop(2);
        vm.pushInteger(fill);
    }
    Ok(Managed)
}

/// `primitiveRenderImage` — render everything, stopping at any external
/// entity; answers the stop reason.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveRenderImage(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let fail_code = loadRenderingState(vm, &mut e);
    if fail_code != 0 {
        vm.primitiveFailFor(fail_code);
        return Ok(Managed);
    }
    e.proceedRenderingScanline();
    if e.engineStopped {
        storeRenderingState(vm, &mut e);
        return Ok(Managed);
    }
    e.proceedRenderingImage();
    storeRenderingState(vm, &mut e);
    Ok(Managed)
}

/// `primitiveRenderScanline` — one scan line; answers the stop reason.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveRenderScanline(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    let fail_code = loadRenderingState(vm, &mut e);
    if fail_code != 0 {
        vm.primitiveFailFor(fail_code);
        return Ok(Managed);
    }
    e.proceedRenderingScanline();
    storeRenderingState(vm, &mut e);
    Ok(Managed)
}

/// `primitiveSetAALevel`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetAALevel(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let level = vm.stackIntegerValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    e.setAALevel(level);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetBitBltPlugin` — select which module provides copyBits.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveSetBitBltPlugin(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    // Must be a string to work.
    let plugin_name = vm.stackValue(0);
    if !vm.isBytes(plugin_name) {
        vm.primitiveFail();
        return Ok(Managed);
    }
    let length = vm.byteSizeOf(plugin_name);
    if length >= 256 {
        vm.primitiveFail();
        return Ok(Managed);
    }
    let ptr = vm.firstIndexableField(plugin_name) as *const u8;
    let mut need_reload = false;
    with_globals(|g| {
        for i in 0..length as usize {
            // SAFETY: `length` bytes are indexable, checked above.
            let c = unsafe { *ptr.add(i) };
            // Compare and store the plugin to be used.
            if g.bbPluginName[i] != c {
                g.bbPluginName[i] = c;
                need_reload = true;
            }
        }
        if g.bbPluginName[length as usize] != 0 {
            g.bbPluginName[length as usize] = 0;
            need_reload = true;
        }
    });
    if need_reload && !initialiseModule_hook() {
        vm.primitiveFail();
        return Ok(Managed);
    }
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetClipRect`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetClipRect(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let rect_oop = vm.stackObjectValue(0);
    if !(!vm.failed() && vm.isPointers(rect_oop) && vm.slotSizeOf(rect_oop) >= 2) {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    loadPointfrom(vm, &mut e, GWPoint1, vm.fetchPointerofObject(0, rect_oop));
    loadPointfrom(vm, &mut e, GWPoint2, vm.fetchPointerofObject(1, rect_oop));
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let v = e.point_x(GWPoint1) as SqInt;
    e.wb_put(GWClipMinX, v);
    let v = e.point_y(GWPoint1) as SqInt;
    e.wb_put(GWClipMinY, v);
    let v = e.point_x(GWPoint2) as SqInt;
    e.wb_put(GWClipMaxX, v);
    let v = e.point_y(GWPoint2) as SqInt;
    e.wb_put(GWClipMaxY, v);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetColorTransform`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetColorTransform(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let transform_oop = vm.stackObjectValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    loadColorTransformFrom(vm, &mut e, transform_oop);
    if vm.failed() {
        vm.primitiveFailFor(GEFEntityLoadFailed);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetDepth`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetDepth(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let depth = vm.stackIntegerValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    e.wb_put(GWCurrentZ, depth);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetEdgeTransform`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetEdgeTransform(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let transform_oop = vm.stackObjectValue(0);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    loadEdgeTransformFrom(vm, &mut e, transform_oop);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}

/// `primitiveSetOffset`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveSetOffset(ivm: &Interp) -> PrimResult<Managed> {
    let vm = Vm(ivm.as_raw());
    let mut e = new_engine(vm);
    if vm.methodArgumentCount() != 1 {
        vm.primitiveFailFor(PrimErrBadNumArgs);
        return Ok(Managed);
    }
    let failure_code =
        quickLoadEngineFromrequiredState(vm, &mut e, vm.stackValue(1), GEStateUnlocked);
    if failure_code != 0 {
        vm.primitiveFailFor(failure_code);
        return Ok(Managed);
    }
    let point_oop = vm.stackValue(0);
    if vm.fetchClassOf(point_oop) != vm.classPoint() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    loadPointfrom(vm, &mut e, GWPoint1, point_oop);
    if vm.failed() {
        vm.primitiveFailFor(PrimErrBadArgument);
        return Ok(Managed);
    }
    let v = e.point_x(GWPoint1) as SqInt;
    e.wb_put(GWDestOffsetX, v);
    let v = e.point_y(GWPoint1) as SqInt;
    e.wb_put(GWDestOffsetY, v);
    storeEngineStateInto(&mut e);
    vm.pop(1);
    Ok(Managed)
}
