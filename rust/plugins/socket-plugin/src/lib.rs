//! `SocketPlugin`, in Rust: the full BSD-sockets plugin -- TCP and UDP
//! create/connect/listen/accept/send/receive/close, socket options, the DNS
//! resolver with its semaphore, and the IPv6-capable address API -- behind the
//! same 60 primitives the C plugin exported.
//!
//! The port replaces two C files: the Slang-generated `SocketPlugin.c`
//! (primitive shims) becomes this file, and the hand-written
//! `SocketPluginImpl.c` (once `sqUnixSocket.c`) becomes [`sock`], [`resolver`],
//! [`address`] and [`options`].
//!
//! # Structure
//!
//! Each primitive here mirrors its generated C shim line for line: the same
//! stack offsets, the same type pre-checks with `PrimErrBadArgument`, the same
//! explicit `pop`/`popthenPush` counts (via [`Answered`], so the SDK does not
//! add its own answer on top). The `success(false)` failures of the
//! implementation layer surface as `Err(PrimErr::GenericFailure)`, the same
//! code `primitiveFail()` set.
//!
//! # The socket handle
//!
//! The image sees a socket as a ByteArray of `sizeof(SQSocket)` bytes: session
//! ID, socket type, and a pointer to private heap state. The outer record's
//! layout is unchanged (other plugins size ByteArrays from it); the pointed-to
//! state is a Rust type now -- see [`sock`] for the one field of it that is
//! still ABI.

// Primitive names are fixed by the image's <primitive:module:> pragmas.
#![allow(non_snake_case)]

mod address;
mod aio;
mod options;
mod resolver;
mod sock;
mod vm_ref;

use core::mem;
use core::slice;

use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, sqInt, Interp, IntoReturn, Oop, PrimErr, PrimResult,
    VirtualMachine,
};

use sock::SQSocket;

pharo_plugin!("SocketPlugin", init = socket_init, shutdown = socket_shutdown);

/// `socketInit`: nothing to do on Unix (the C only ran `WSAStartup` on
/// Windows, which this port does not target).
fn socket_init() -> bool {
    true
}

/// `shutdownModule` -> `socketShutdown` -> `sqNetworkShutdown`, unless a DNS
/// worker is still running.
///
/// The refusal is the quiescence rule `CLAUDE.md` §1 and §3 trap 3 state:
/// `Smalltalk vm unloadModule: 'SocketPlugin'` is reachable from ordinary image
/// code and ends in `dlclose`, and a `pharo-dns` worker parked in
/// `getaddrinfo` is executing this library's text and is about to touch its
/// statics. `ioUnloadModule` honours a 0 by leaving the module loaded
/// (`rust/pharo-platform/src/named_prims.rs`, the `shutdown_module(entry) == 0`
/// arm), so answering 0 is how a plugin says "not yet".
///
/// It refuses *before* tearing anything down, so a refused unload leaves a
/// working module rather than a loaded one with its network shut. The one
/// consequence worth stating: `ioShutdownAllModules` ignores the answer, so a
/// lookup in flight at that moment means `sqNetworkShutdown` is skipped for
/// this module. Nothing in this tree calls `ioShutdownAllModules` -- it is
/// exported for the image -- and the process is on its way out when it does.
fn socket_shutdown() -> bool {
    if !resolver::is_quiescent() {
        return false;
    }
    resolver::network_shutdown();
    true
}

/// `moduleUnloaded`: the C exported it and answered 0; kept for parity.
#[no_mangle]
pub extern "C" fn moduleUnloaded(_a_module_name: *mut core::ffi::c_char) -> sqInt {
    0
}

// ---------------------------------------------------------------------------
// Answering the C way
// ---------------------------------------------------------------------------

/// Marker: the primitive already popped/pushed exactly what the generated C
/// popped and pushed. Answering this keeps the SDK's `IntoReturn` from adding
/// a `methodReturn*` (which pops `argumentCount` items) on top -- the C shims
/// use literal counts, and this port reproduces them literally.
struct Answered;

impl IntoReturn for Answered {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Raw-proxy helpers (entries the safe API does not cover)
// ---------------------------------------------------------------------------

fn vt(vm: &Interp) -> &VirtualMachine {
    // SAFETY: as_raw() is the VM's process-lifetime proxy table.
    unsafe { &*vm.as_raw() }
}

/// `popthenPush(n, oop)`: the C shims' way of answering a value.
fn pop_then_push(vm: &Interp, n: sqInt, oop: Oop) -> PrimResult<()> {
    let f = vt(vm).popthenPush.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    unsafe { f(n, oop.0) };
    Ok(())
}

/// `pop(n)`: the C shims' way of answering the receiver.
fn pop_n(vm: &Interp, n: sqInt) -> PrimResult<()> {
    let f = vt(vm).pop.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    unsafe { f(n) };
    Ok(())
}

/// `storePointerofObjectwithValue`, for filling result Arrays.
fn store_pointer(vm: &Interp, index: sqInt, object: Oop, value: Oop) -> PrimResult<()> {
    let f = vt(vm)
        .storePointerofObjectwithValue
        .ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    unsafe { f(index, object.0, value.0) };
    Ok(())
}

/// `pushRemappableOop` / `popRemappableOop`: kept from the C for the one
/// place it protected a fresh oop across an allocation.
fn push_remappable(vm: &Interp, oop: Oop) -> PrimResult<()> {
    let f = vt(vm).pushRemappableOop.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    unsafe { f(oop.0) };
    Ok(())
}

fn pop_remappable(vm: &Interp) -> PrimResult<Oop> {
    let f = vt(vm).popRemappableOop.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    Ok(Oop(unsafe { f() }))
}

fn is_words(vm: &Interp, oop: Oop) -> PrimResult<bool> {
    let f = vt(vm).isWords.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    Ok(unsafe { f(oop.0) } != 0)
}

fn first_indexable(vm: &Interp, oop: Oop) -> PrimResult<*mut u8> {
    let f = vt(vm).firstIndexableField.ok_or(PrimErr::Unsupported)?;
    // SAFETY: proxy entry with the header's signature.
    let p = unsafe { f(oop.0) };
    if p.is_null() {
        return Err(PrimErr::BadArgument);
    }
    Ok(p.cast::<u8>())
}

// ---------------------------------------------------------------------------
// Object-access helpers shared by the primitives
// ---------------------------------------------------------------------------

/// `socketValueOf:`: the record pointer inside a socket-handle ByteArray.
/// Fails with `PrimErrBadArgument` exactly as the C's inlined check did.
fn socket_record(vm: &Interp, oop: Oop) -> PrimResult<*mut SQSocket> {
    if !vm.is_bytes(oop)? || vm.byte_size_of(oop)? != mem::size_of::<SQSocket>() as sqInt {
        return Err(PrimErr::BadArgument);
    }
    Ok(first_indexable(vm, oop)?.cast::<SQSocket>())
}

/// Runs `f` over a mutable view of a byte object's contents, for the
/// result-writing primitives.
///
/// The C wrote through `firstIndexableField` with no immutability check;
/// this reproduces that (the safe `write_bytes` would refuse read-only
/// objects, a behaviour difference). Scoping the slice to a closure keeps it
/// from outliving the call -- `f` must not allocate object memory.
fn with_bytes_mut<R>(vm: &Interp, oop: Oop, f: impl FnOnce(&mut [u8]) -> R) -> PrimResult<R> {
    if !vm.is_bytes(oop)? {
        return Err(PrimErr::BadArgument);
    }
    let len = usize::try_from(vm.byte_size_of(oop)?)?;
    let ptr = first_indexable(vm, oop)?;
    // SAFETY: `ptr` spans `len` bytes owned by the object; nothing moves it
    // for the duration of the closure (which does not allocate), and no other
    // slice into this object exists concurrently.
    Ok(f(unsafe { slice::from_raw_parts_mut(ptr, len) }))
}

/// The C shims' send/receive buffer computation: the array must be words or
/// bytes (failure code 1, from `success(...)`), elements are 4 bytes for word
/// arrays, and `start`/`count` are 1-origin bounds-checked against
/// `slotSizeOf`. Answers the raw start pointer and the element size.
fn transfer_buffer(
    vm: &Interp,
    array: Oop,
    start_index: sqInt,
    count: sqInt,
) -> PrimResult<(*mut u8, sqInt)> {
    if !vm.is_words_or_bytes(array)? {
        return Err(PrimErr::GenericFailure);
    }
    let element_size: sqInt = if is_words(vm, array)? { 4 } else { 1 };
    let slots = vm.slot_size_of(array)?;
    if !(start_index >= 1 && count >= 0 && start_index + count - 1 <= slots) {
        return Err(PrimErr::GenericFailure);
    }
    let base = first_indexable(vm, array)?;
    // SAFETY: (start-1)*element_size is within the object, checked above.
    Ok((
        unsafe { base.offset(((start_index - 1) * element_size) as isize) },
        element_size,
    ))
}

/// `intToNetAddress:`: a fresh 4-byte ByteArray holding `addr` big-endian.
fn int_to_net_address_oop(vm: &Interp, addr: u32) -> PrimResult<Oop> {
    let oop = vm.instantiate(vm.class_byte_array()?, 4)?;
    vm.write_bytes(oop, 0, &address::int_to_net_address(addr))?;
    Ok(oop)
}

/// `netAddressToInt:` applied to a byte object: fails (code 1, as the C's
/// `primitiveFail()`) unless it is exactly 4 bytes.
fn net_address_arg(vm: &Interp, oop: Oop) -> PrimResult<u32> {
    address::net_address_to_int(vm.bytes_of(oop)?).ok_or(PrimErr::GenericFailure)
}

fn bool_oop(vm: &Interp, value: bool) -> PrimResult<Oop> {
    if value {
        vm.true_object()
    } else {
        vm.false_object()
    }
}

// ---------------------------------------------------------------------------
// Network initialisation
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveInitializeNetwork(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let resolver_sema_index = vm.stack_integer(0)?;
    if resolver::network_init(resolver_sema_index) != 0 {
        return Err(PrimErr::GenericFailure); // success(err == 0)
    }
    pop_n(vm, 1)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Resolver: classic synchronous lookups
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverAbortLookup(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    resolver::resolver_abort();
    // The C touches nothing on the stack here.
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverAddressLookupResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let size = resolver::addr_lookup_result_size()?;
    let result = vm.instantiate(vm.class_string()?, size as sqInt)?;
    with_bytes_mut(vm, result, resolver::addr_lookup_result)??;
    pop_then_push(vm, 1, result)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverError(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let error = resolver::resolver_error();
    pop_then_push(vm, 1, vm.integer(error as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverLocalAddress(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let addr = resolver::resolver_local_address()?;
    let result = int_to_net_address_oop(vm, addr)?;
    pop_then_push(vm, 1, result)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverNameLookupResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let addr = resolver::name_lookup_result()?;
    let result = int_to_net_address_oop(vm, addr)?;
    pop_then_push(vm, 1, result)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverStartAddressLookup(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let address_oop = vm.stack_value(0)?;
    if !vm.is_bytes(address_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let addr = net_address_arg(vm, address_oop)?;
    resolver::start_addr_lookup(addr)?;
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverStartNameLookup(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let name_oop = vm.stack_value(0)?;
    if !vm.is_bytes(name_oop)? {
        return Err(PrimErr::BadArgument);
    }
    resolver::start_name_lookup(vm.bytes_of(name_oop)?)?;
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverStatus(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let status = resolver::resolver_status();
    pop_then_push(vm, 1, vm.integer(status as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverHostNameSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let size = resolver::host_name_size()?;
    pop_then_push(vm, 1, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverHostNameResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let name_oop = vm.stack_value(0)?;
    if !vm.is_bytes(name_oop)? {
        return Err(PrimErr::BadArgument);
    }
    with_bytes_mut(vm, name_oop, resolver::host_name_result)??;
    pop_n(vm, 1)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Resolver: getaddrinfo / getnameinfo
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveResolverGetAddressInfo(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let host_oop = vm.stack_value(5)?;
    let serv_oop = vm.stack_value(4)?;
    if !vm.is_bytes(host_oop)? || !vm.is_bytes(serv_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let flags = vm.stack_integer(3)?;
    let family = vm.stack_integer(2)?;
    let type_ = vm.stack_integer(1)?;
    let protocol = vm.stack_integer(0)?;
    let host = vm.bytes_of(host_oop)?;
    let serv = vm.bytes_of(serv_oop)?;
    resolver::get_address_info(host, serv, flags, family, type_, protocol)?;
    pop_n(vm, 6)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let size = resolver::gai_size()?;
    pop_then_push(vm, 1, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let addr_oop = vm.stack_value(0)?;
    if !vm.is_bytes(addr_oop)? {
        return Err(PrimErr::BadArgument);
    }
    with_bytes_mut(vm, addr_oop, resolver::gai_result)??;
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoFamily(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let family = resolver::gai_family()?;
    pop_then_push(vm, 1, vm.integer(family)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoType(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let type_ = resolver::gai_type()?;
    pop_then_push(vm, 1, vm.integer(type_)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoProtocol(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let protocol = resolver::gai_protocol()?;
    pop_then_push(vm, 1, vm.integer(protocol)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetAddressInfoNext(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let more = resolver::gai_next()?;
    let answer = bool_oop(vm, more)?;
    pop_then_push(vm, 1, answer)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveResolverGetNameInfo(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let flags = vm.stack_integer(0)?;
    let socket_address = vm.stack_value(1)?;
    // The C reads the argument's bytes with no kind check (undefined
    // behaviour on a non-bytes object); failing cleanly instead.
    let addr = vm.bytes_of(socket_address)?;
    resolver::get_name_info(addr, flags)?;
    pop_n(vm, 2)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetNameInfoHostSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let size = resolver::ni_host_size()?;
    pop_then_push(vm, 1, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetNameInfoHostResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let name_oop = vm.stack_value(0)?;
    if !vm.is_bytes(name_oop)? {
        return Err(PrimErr::BadArgument);
    }
    with_bytes_mut(vm, name_oop, resolver::ni_host_result)??;
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetNameInfoServiceSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let size = resolver::ni_service_size()?;
    pop_then_push(vm, 1, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveResolverGetNameInfoServiceResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let name_oop = vm.stack_value(0)?;
    if !vm.is_bytes(name_oop)? {
        return Err(PrimErr::BadArgument);
    }
    with_bytes_mut(vm, name_oop, resolver::ni_service_result)??;
    pop_n(vm, 1)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Socket address objects (header + raw sockaddr)
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketAddressGetPort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let addr_oop = vm.stack_value(0)?;
    // As in GetNameInfo: the C did not check the kind before reading bytes.
    let addr = vm.bytes_of(addr_oop)?;
    let port = address::get_port(addr, resolver::current_session())
        .ok_or(PrimErr::GenericFailure)?;
    pop_then_push(vm, 1, vm.integer(port as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketAddressSetPort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let port_number = vm.stack_integer(0)?;
    let addr_oop = vm.stack_value(1)?;
    // htons truncates the port to 16 bits, as the C's cast did.
    let stored = with_bytes_mut(vm, addr_oop, |addr| {
        address::set_port(addr, resolver::current_session(), port_number as u16)
    })?;
    if !stored {
        return Err(PrimErr::GenericFailure);
    }
    pop_n(vm, 1)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Socket creation and acceptance
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketCreate(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let net_type = vm.stack_integer(4)?;
    let socket_type = vm.stack_integer(3)?;
    let recv_buf_size = vm.stack_integer(2)?;
    let send_buf_size = vm.stack_integer(1)?;
    let sema_index = vm.stack_integer(0)?;
    let socket_oop = vm.instantiate(vm.class_byte_array()?, mem::size_of::<SQSocket>() as sqInt)?;
    let record = socket_record(vm, socket_oop)?;
    // SAFETY: record points into the just-instantiated ByteArray; create does
    // not allocate object memory.
    unsafe {
        sock::create(
            record,
            net_type,
            socket_type,
            recv_buf_size,
            send_buf_size,
            sema_index,
            sema_index,
            sema_index,
        )?;
    }
    pop_then_push(vm, 6, socket_oop)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketCreate3Semaphores(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let net_type = vm.stack_integer(6)?;
    let socket_type = vm.stack_integer(5)?;
    let recv_buf_size = vm.stack_integer(4)?;
    let send_buf_size = vm.stack_integer(3)?;
    let sema_index = vm.stack_integer(2)?;
    let read_sema = vm.stack_integer(1)?;
    let write_sema = vm.stack_integer(0)?;
    let socket_oop = vm.instantiate(vm.class_byte_array()?, mem::size_of::<SQSocket>() as sqInt)?;
    let record = socket_record(vm, socket_oop)?;
    // SAFETY: as in primitiveSocketCreate.
    unsafe {
        sock::create(
            record,
            net_type,
            socket_type,
            recv_buf_size,
            send_buf_size,
            sema_index,
            read_sema,
            write_sema,
        )?;
    }
    pop_then_push(vm, 8, socket_oop)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketCreateRAW(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let net_type = vm.stack_integer(6)?;
    let proto_type = vm.stack_integer(5)?;
    let recv_buf_size = vm.stack_integer(4)?;
    let send_buf_size = vm.stack_integer(3)?;
    let sema_index = vm.stack_integer(2)?;
    let read_sema = vm.stack_integer(1)?;
    let write_sema = vm.stack_integer(0)?;
    let socket_oop = vm.instantiate(vm.class_byte_array()?, mem::size_of::<SQSocket>() as sqInt)?;
    let record = socket_record(vm, socket_oop)?;
    // SAFETY: as in primitiveSocketCreate.
    unsafe {
        sock::create_raw(
            record,
            net_type,
            proto_type,
            recv_buf_size,
            send_buf_size,
            sema_index,
            read_sema,
            write_sema,
        )?;
    }
    pop_then_push(vm, 8, socket_oop)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketAccept(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let _recv_buf_size = vm.stack_integer(2)?;
    let _send_buf_size = vm.stack_integer(1)?; // accepted and ignored, as in C
    let sema_index = vm.stack_integer(0)?;
    let sock_handle = vm.stack_value(3)?;
    // Reordered from the C shim: the new ByteArray is instantiated before the
    // server's record pointer is taken (Spur's instantiate never collects,
    // but this removes the stale-pointer question entirely).
    let socket_oop = vm.instantiate(vm.class_byte_array()?, mem::size_of::<SQSocket>() as sqInt)?;
    let server = socket_record(vm, sock_handle)?;
    let record = socket_record(vm, socket_oop)?;
    // SAFETY: both records point into live ByteArrays; accept_from does not
    // allocate object memory.
    unsafe { sock::accept_from(record, server, sema_index, sema_index, sema_index)? };
    pop_then_push(vm, 5, socket_oop)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketAccept3Semaphores(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let _recv_buf_size = vm.stack_integer(4)?;
    let _send_buf_size = vm.stack_integer(3)?; // accepted and ignored, as in C
    let sema_index = vm.stack_integer(2)?;
    let read_sema = vm.stack_integer(1)?;
    let write_sema = vm.stack_integer(0)?;
    let sock_handle = vm.stack_value(5)?;
    let socket_oop = vm.instantiate(vm.class_byte_array()?, mem::size_of::<SQSocket>() as sqInt)?;
    let server = socket_record(vm, sock_handle)?;
    let record = socket_record(vm, socket_oop)?;
    // SAFETY: as in primitiveSocketAccept.
    unsafe { sock::accept_from(record, server, sema_index, read_sema, write_sema)? };
    pop_then_push(vm, 7, socket_oop)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Connecting, listening, binding
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketConnectToPort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let address_oop = vm.stack_value(1)?;
    if !vm.is_bytes(address_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let port = vm.stack_integer(0)?;
    let socket = vm.stack_value(2)?;
    let addr = net_address_arg(vm, address_oop)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: record points into a live ByteArray; no allocation follows.
    unsafe { sock::connect_to_port(record, addr, port)? };
    pop_n(vm, 3)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketConnectTo(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(1)?;
    let socket_address = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    let addr = vm.bytes_of(socket_address)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::connect_to_address(record, addr)? };
    pop_n(vm, 2)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketBindToPort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let address_oop = vm.stack_value(1)?;
    if !vm.is_bytes(address_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let port = vm.stack_integer(0)?;
    let socket = vm.stack_value(2)?;
    let addr = net_address_arg(vm, address_oop)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::bind_to_port(record, addr, port)? };
    pop_n(vm, 3)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketBindTo(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(1)?;
    let socket_address = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    let addr = vm.bytes_of(socket_address)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::bind_to_address(record, addr)? };
    pop_n(vm, 2)?;
    Ok(Answered)
}

/// Body shared with `primitiveSocketListenWithOrWithoutBacklog`.
fn listen_on_port_body(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let port = vm.stack_integer(0)?;
    let socket = vm.stack_value(1)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::listen_on_port(record, port)? };
    pop_n(vm, 2)?;
    Ok(Answered)
}

/// Body shared with `primitiveSocketListenWithOrWithoutBacklog`.
fn listen_on_port_backlog_body(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let port = vm.stack_integer(1)?;
    let backlog = vm.stack_integer(0)?;
    let socket = vm.stack_value(2)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::listen_on_port_backlog(record, port, backlog)? };
    pop_n(vm, 3)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketListenOnPort(vm: &Interp) -> PrimResult<Answered> {
    listen_on_port_body(vm)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketListenOnPortBacklog(vm: &Interp) -> PrimResult<Answered> {
    listen_on_port_backlog_body(vm)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketListenOnPortBacklogInterface(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let port = vm.stack_integer(2)?;
    let backlog = vm.stack_integer(1)?;
    let interface_oop = vm.stack_value(0)?;
    if !vm.is_bytes(interface_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let socket = vm.stack_value(3)?;
    let record = socket_record(vm, socket)?;
    let addr = net_address_arg(vm, interface_oop)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::listen_on_port_backlog_interface(record, port, backlog, addr)? };
    pop_n(vm, 4)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketListenWithBacklog(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let backlog_size = vm.stack_integer(0)?;
    let socket = vm.stack_value(1)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectToPort.
    unsafe { sock::listen_backlog(record, backlog_size)? };
    pop_n(vm, 2)?;
    Ok(Answered)
}

/// "Backward compatibility": dispatches on the argument count, as the C's
/// "wierdass dual prim" did.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketListenWithOrWithoutBacklog(vm: &Interp) -> PrimResult<Answered> {
    if vm.argument_count()? == 2 {
        listen_on_port_body(vm)
    } else {
        listen_on_port_backlog_body(vm)
    }
}

// ---------------------------------------------------------------------------
// Status, errors, teardown
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketConnectionStatus(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: record points into a live ByteArray; no allocation while used.
    let status = unsafe { sock::connection_status(record)? };
    pop_then_push(vm, 2, vm.integer(status as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketError(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let error = unsafe { sock::socket_error(record)? };
    pop_then_push(vm, 2, vm.integer(error as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketCloseConnection(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    unsafe { sock::close_connection(record)? };
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketAbortConnection(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    unsafe { sock::abort_connection(record)? };
    pop_n(vm, 1)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketDestroy(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    unsafe { sock::destroy(record)? };
    pop_n(vm, 1)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Addresses and ports of a connected socket
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketLocalAddress(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus; the record pointer is
    // not used after the allocation in int_to_net_address_oop.
    let addr = unsafe { sock::local_address(record)? };
    let result = int_to_net_address_oop(vm, addr)?;
    pop_then_push(vm, 2, result)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketRemoteAddress(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketLocalAddress.
    let addr = unsafe { sock::remote_address(record)? };
    let result = int_to_net_address_oop(vm, addr)?;
    pop_then_push(vm, 2, result)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketLocalPort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let port = unsafe { sock::local_port(record)? };
    pop_then_push(vm, 2, vm.integer(port)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketRemotePort(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let port = unsafe { sock::remote_port(record)? };
    pop_then_push(vm, 2, vm.integer(port)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketLocalAddressSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let size = unsafe { sock::local_address_size(record)? };
    pop_then_push(vm, 2, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketLocalAddressResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(1)?;
    let socket_address = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus; the destination and the
    // record are distinct objects.
    with_bytes_mut(vm, socket_address, |dest| unsafe {
        sock::local_address_result(record, dest)
    })??;
    pop_n(vm, 2)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketRemoteAddressSize(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let size = unsafe { sock::remote_address_size(record)? };
    pop_then_push(vm, 2, vm.integer(size as sqInt)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketRemoteAddressResult(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(1)?;
    let socket_address = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketLocalAddressResult.
    with_bytes_mut(vm, socket_address, |dest| unsafe {
        sock::remote_address_result(record, dest)
    })??;
    pop_n(vm, 2)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Data transfer
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketReceiveDataAvailable(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let available = unsafe { sock::receive_data_available(record)? };
    let answer = bool_oop(vm, available)?;
    pop_then_push(vm, 2, answer)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketSendDone(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    // SAFETY: as in primitiveSocketConnectionStatus.
    let done = unsafe { sock::send_done(record)? };
    let answer = bool_oop(vm, done)?;
    pop_then_push(vm, 2, answer)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketReceiveDataBufCount(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let start_index = vm.stack_integer(1)?;
    let count = vm.stack_integer(0)?;
    let socket = vm.stack_value(3)?;
    let array = vm.stack_value(2)?;
    let record = socket_record(vm, socket)?;
    let (buf, element_size) = transfer_buffer(vm, array, start_index, count)?;
    // SAFETY: buf spans count*element_size bytes inside the array (checked by
    // transfer_buffer); recv writes into it with no allocation in between,
    // exactly as the C read straight into object memory.
    let received =
        unsafe { sock::receive_data(record, buf, (count * element_size) as usize)? };
    pop_then_push(vm, 5, vm.integer(received as sqInt / element_size)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketSendDataBufCount(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let start_index = vm.stack_integer(1)?;
    let count = vm.stack_integer(0)?;
    let socket = vm.stack_value(3)?;
    let array = vm.stack_value(2)?;
    let record = socket_record(vm, socket)?;
    let (buf, element_size) = transfer_buffer(vm, array, start_index, count)?;
    // SAFETY: as in primitiveSocketReceiveDataBufCount, reading.
    let sent = unsafe { sock::send_data(record, buf, (count * element_size) as usize)? };
    pop_then_push(vm, 5, vm.integer(sent as sqInt / element_size)?)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketReceiveUDPDataBufCount(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let start_index = vm.stack_integer(1)?;
    let count = vm.stack_integer(0)?;
    let socket = vm.stack_value(3)?;
    let array = vm.stack_value(2)?;
    let record = socket_record(vm, socket)?;
    let (buf, element_size) = transfer_buffer(vm, array, start_index, count)?;
    // SAFETY: as in primitiveSocketReceiveDataBufCount.
    let (received, addr, port, more) =
        unsafe { sock::receive_udp(record, buf, (count * element_size) as usize)? };
    // The C protects the fresh net-address oop across the Array allocation
    // with the remap stack; kept, though Spur's instantiate never collects.
    let address_oop = int_to_net_address_oop(vm, addr)?;
    push_remappable(vm, address_oop)?;
    let results = vm.instantiate(vm.class_array()?, 4)?;
    store_pointer(vm, 0, results, vm.integer(received as sqInt / element_size)?)?;
    let address_oop = pop_remappable(vm)?;
    store_pointer(vm, 1, results, address_oop)?;
    store_pointer(vm, 2, results, vm.integer(port)?)?;
    let more_oop = bool_oop(vm, more)?;
    store_pointer(vm, 3, results, more_oop)?;
    pop_then_push(vm, 5, results)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketSendUDPDataBufCount(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let host_oop = vm.stack_value(3)?;
    if !vm.is_bytes(host_oop)? {
        return Err(PrimErr::BadArgument);
    }
    let port_number = vm.stack_integer(2)?;
    let start_index = vm.stack_integer(1)?;
    let count = vm.stack_integer(0)?;
    let socket = vm.stack_value(5)?;
    let array = vm.stack_value(4)?;
    let record = socket_record(vm, socket)?;
    let (buf, element_size) = transfer_buffer(vm, array, start_index, count)?;
    let addr = net_address_arg(vm, host_oop)?;
    // SAFETY: as in primitiveSocketSendDataBufCount.
    let sent = unsafe {
        sock::send_udp_to(record, addr, port_number, buf, (count * element_size) as usize)?
    };
    pop_then_push(vm, 7, vm.integer(sent as sqInt / element_size)?)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketGetOptions(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(1)?;
    let option_name_oop = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    if !vm.is_bytes(option_name_oop)? {
        return Err(PrimErr::GenericFailure); // the C's success(isBytes(...))
    }
    // The name is read where it lies: `get_options` is a getsockopt call and
    // the borrow ends with it, before the Array below is allocated.
    // SAFETY: record points into a live ByteArray, used before any allocation.
    let (error_code, value) = unsafe { sock::get_options(record, vm.bytes_of(option_name_oop)?)? };
    let results = vm.instantiate(vm.class_array()?, 2)?;
    store_pointer(vm, 0, results, vm.integer(error_code)?)?;
    store_pointer(vm, 1, results, vm.integer(value)?)?;
    pop_then_push(vm, 3, results)?;
    Ok(Answered)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSocketSetOptions(vm: &Interp) -> PrimResult<Answered> {
    vm_ref::remember(vm);
    let socket = vm.stack_value(2)?;
    let option_name_oop = vm.stack_value(1)?;
    let option_value_oop = vm.stack_value(0)?;
    let record = socket_record(vm, socket)?;
    if !vm.is_bytes(option_name_oop)? || !vm.is_bytes(option_value_oop)? {
        return Err(PrimErr::GenericFailure); // the C's success(isBytes(...))
    }
    // Both read where they lie, as in primitiveSocketGetOptions.
    // SAFETY: as in primitiveSocketGetOptions.
    let (error_code, value) = unsafe {
        sock::set_options(
            record,
            vm.bytes_of(option_name_oop)?,
            vm.bytes_of(option_value_oop)?,
        )?
    };
    let results = vm.instantiate(vm.class_array()?, 2)?;
    store_pointer(vm, 0, results, vm.integer(error_code)?)?;
    store_pointer(vm, 1, results, vm.integer(value)?)?;
    pop_then_push(vm, 4, results)?;
    Ok(Answered)
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::{Mutex, MutexGuard};

    static NET_LOCK: Mutex<()> = Mutex::new(());

    /// Serialises tests: the network session, the resolver state and the test
    /// aio registry are all process-wide, as they are in the real plugin.
    pub fn net_lock() -> MutexGuard<'static, ()> {
        NET_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
