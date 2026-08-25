//! The SSL state machine: a port of `plugins/SqueakSSL/src/unix/sqUnixSSL.c`.
//!
//! Everything here is VM-free and operates on byte buffers, so the whole
//! handshake can be driven in-process by the tests: a client state and a
//! server state pumping each other's output.
//!
//! # Shape
//!
//! One [`SqSsl`] per session, owned by a global handle table exactly like the
//! C's `handleBuf`: handles are small integers starting at 1, index 0 is never
//! handed out, freed slots are reused first-fit, and the table grows by 100
//! slots at a time. The public functions mirror the C `sq*SSL` entry points
//! one for one, including their return codes.
//!
//! # OpenSSL, raw
//!
//! The calls go through `openssl-sys`, not the safe `openssl` wrapper,
//! because the C plugin's usage -- two memory BIOs plumbed into an `SSL` with
//! `SSL_set_bio`, the handshake pumped by hand -- is exactly the shape the
//! raw API has and not the shape the safe stream API has. Each unsafe block
//! is a 1:1 transcription of a line of the C. Targets OpenSSL 1.1+/3.x; the
//! C's `OPENSSL_VERSION_NUMBER < 0x10100000L` paths (`SSL_library_init`,
//! `ASN1_STRING_data`) are dropped.

use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};
use std::ptr;
use std::sync::Mutex;

use openssl_sys as ffi;

/// Missing from `openssl-sys`: declared here against the same libcrypto.
/// All three exist unchanged in OpenSSL 1.1 and 3.x.
mod extra {
    use super::{c_char, c_int};

    extern "C" {
        /// `size_t BIO_ctrl_pending(BIO *b)` -- bytes buffered in a BIO.
        pub fn BIO_ctrl_pending(b: *mut openssl_sys::BIO) -> usize;
        /// `int X509_NAME_get_text_by_NID(X509_NAME *, int, char *, int)`.
        pub fn X509_NAME_get_text_by_NID(
            name: *mut openssl_sys::X509_NAME,
            nid: c_int,
            buf: *mut c_char,
            len: c_int,
        ) -> c_int;
        /// `void ERR_error_string_n(unsigned long, char *, size_t)`.
        pub fn ERR_error_string_n(e: std::ffi::c_ulong, buf: *mut c_char, len: usize);
    }
}

// `BIO_set_close` is a macro over BIO_ctrl in every OpenSSL; these are its
// operands (from bio.h).
const BIO_CTRL_SET_CLOSE: c_int = 9;
const BIO_CLOSE: c_long = 1;

// ---------------------------------------------------------------------------
// The constants from SqueakSSL.h. The image knows these numbers.
// ---------------------------------------------------------------------------

/// Protocol version this plugin implements (`SQSSL_VERSION`).
pub const SQSSL_VERSION: isize = 3;

/// Connection states (`SQSSL_UNUSED` ...).
pub const SQSSL_UNUSED: isize = 0;
/// Server handshake in progress.
pub const SQSSL_ACCEPTING: isize = 1;
/// Client handshake in progress.
pub const SQSSL_CONNECTING: isize = 2;
/// Handshake finished; encrypt/decrypt are legal.
pub const SQSSL_CONNECTED: isize = 3;

/// Return codes from the core functions.
pub const SQSSL_OK: isize = 0;
/// The handshake needs more input before it can produce output.
pub const SQSSL_NEED_MORE_DATA: isize = -1;
/// The handle is invalid, or the session is in the wrong state for the call.
pub const SQSSL_INVALID_STATE: isize = -2;
/// Anything else that went wrong.
pub const SQSSL_GENERIC_ERROR: isize = -5;

/// Certificate status: no certificate was presented by the peer.
pub const SQSSL_NO_CERTIFICATE: isize = -1;
/// Certificate status: verification failed for an unclassified reason. The C
/// carries a FIXME to report the actual reason; it never does, and neither do
/// we.
pub const SQSSL_OTHER_ISSUE: isize = 0x0001;

/// Integer property IDs.
pub const SQSSL_PROP_VERSION: isize = 0;
/// Log level (stored, never consulted -- the Rust port does not log).
pub const SQSSL_PROP_LOGLEVEL: isize = 1;
/// The connection state, one of `SQSSL_UNUSED`..`SQSSL_CONNECTED`.
pub const SQSSL_PROP_SSLSTATE: isize = 2;
/// The certificate status bits.
pub const SQSSL_PROP_CERTSTATE: isize = 3;

/// String property IDs.
pub const SQSSL_PROP_PEERNAME: c_int = 0;
/// Path of the PEM file holding this side's certificate and private key.
pub const SQSSL_PROP_CERTNAME: c_int = 1;
/// Host name the client expects, sent as SNI and checked against the cert.
pub const SQSSL_PROP_SERVERNAME: c_int = 2;

/// `MAX_HOSTNAME_LENGTH` from the C: RFC 1035's limit on a domain name.
const MAX_HOSTNAME_LENGTH: usize = 253;

// The C's `enum sqMatchResult`, kept as the raw ints X509_check_host /
// X509_check_ip_asc answer.
const MATCH_FOUND: c_int = 1;
const NO_MATCH_DONE_YET: c_int = -1;
const INVALID_IP_STRING: c_int = -2;
const NO_SAN_PRESENT: c_int = -3;

/// Slots the handle table grows by, as in the C (`delta = 100`).
const HANDLE_DELTA: usize = 100;

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// One SSL session: the C's `struct sqSSL`.
struct SqSsl {
    state: isize,
    cert_flags: isize,
    loglevel: isize,

    cert_name: Option<CString>,
    peer_name: Option<CString>,
    server_name: Option<CString>,

    ctx: *mut ffi::SSL_CTX,
    ssl: *mut ffi::SSL,
    bio_read: *mut ffi::BIO,
    bio_write: *mut ffi::BIO,
}

// SAFETY: the raw pointers are owned exclusively by this session, the table's
// Mutex serializes all access, and OpenSSL 1.1+ objects may be used from any
// thread as long as they are not used concurrently.
unsafe impl Send for SqSsl {}

impl Drop for SqSsl {
    /// The C's `sqDestroySSL` teardown, in the same order.
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                ffi::SSL_CTX_free(self.ctx);
            }
            if !self.ssl.is_null() {
                // SSL_set_bio handed both BIOs to the SSL, so this frees them.
                ffi::SSL_free(self.ssl);
            } else {
                // SSL_new was never reached; free the BIOs by hand.
                ffi::BIO_free_all(self.bio_read);
                ffi::BIO_free_all(self.bio_write);
            }
        }
    }
}

/// The handle table: the C's `handleBuf`/`handleMax`.
///
/// A Mutex not because the VM is multi-threaded -- primitives all run on the
/// interpreter thread -- but because a Rust global must be `Sync`, and the
/// uncontended lock is cheap next to a TLS operation.
static TABLE: Mutex<Vec<Option<SqSsl>>> = Mutex::new(Vec::new());

/// Finds a free slot the way `sqCreateSSL` does: first-fit from index 1,
/// growing by [`HANDLE_DELTA`] when full. Index 0 is never used, so 0 can
/// mean "no handle" to the image.
///
/// Factored over any `Vec` so the numbering is unit-testable without OpenSSL.
fn allocate_handle<T>(table: &mut Vec<Option<T>>, value: T) -> usize {
    let mut handle = 1;
    while handle < table.len() && table[handle].is_some() {
        handle += 1;
    }
    if handle >= table.len() {
        let grown = table.len() + HANDLE_DELTA;
        table.resize_with(grown, || None);
    }
    table[handle] = Some(value);
    handle
}

/// The C's `sslFromHandle`, minus its undefined behaviour: the C indexes
/// `handleBuf[handle]` after checking only `handle < handleMax`, so a
/// negative handle reads out of bounds. Here anything outside the table is
/// simply invalid.
fn with_session<R>(handle: isize, f: impl FnOnce(&mut SqSsl) -> R) -> Option<R> {
    let mut table = TABLE.lock().expect("SSL handle table poisoned");
    let slot = usize::try_from(handle).ok()?;
    table.get_mut(slot)?.as_mut().map(f)
}

// ---------------------------------------------------------------------------
// OpenSSL helpers
// ---------------------------------------------------------------------------

/// The C's `ERR_print_errors_fp` calls: drain OpenSSL's per-thread error
/// queue, printing each entry to stderr. Draining matters beyond diagnostics
/// -- stale queue entries would make a later `SSL_get_error` misreport.
fn drain_error_queue() {
    unsafe {
        loop {
            let e = ffi::ERR_get_error();
            if e == 0 {
                break;
            }
            let mut buf = [0 as c_char; 256];
            extra::ERR_error_string_n(e, buf.as_mut_ptr(), buf.len());
            let msg = CStr::from_ptr(buf.as_ptr());
            eprintln!("SqueakSSL: {}", msg.to_string_lossy());
        }
    }
}

/// The C's `sqCopyBioSSL`: move whatever the BIO holds into `dst`.
///
/// Faithfully odd in two ways. When the pending bytes exceed the buffer it
/// answers -1, which collides with `SQSSL_NEED_MORE_DATA` rather than being
/// `SQSSL_BUFFER_TOO_SMALL`. And when the BIO is empty, `BIO_read` on a
/// memory BIO answers -1 (retry), which is how the handshake pumps end up
/// reporting `SQSSL_NEED_MORE_DATA` with no explicit check.
fn copy_bio(bio: *mut ffi::BIO, dst: &mut [u8]) -> isize {
    unsafe {
        let pending = extra::BIO_ctrl_pending(bio);
        if pending > dst.len() {
            return -1;
        }
        // The C passes the sqInt length straight to BIO_read's int parameter;
        // keep the truncation.
        ffi::BIO_read(bio, dst.as_mut_ptr().cast::<c_void>(), dst.len() as c_int) as isize
    }
}

/// Feeds `src` to the session's read BIO: the shared prologue of the C's
/// connect/accept/decrypt. Answers false on the C's `n < srcLen` failure.
/// (The C also checks `n < 0` afterwards, which is unreachable; dropped.)
fn feed_input(ssl: &mut SqSsl, src: &[u8]) -> bool {
    if src.is_empty() {
        return true;
    }
    let n = unsafe {
        ffi::BIO_write(
            ssl.bio_read,
            src.as_ptr().cast::<c_void>(),
            src.len() as c_int,
        )
    };
    n as isize >= src.len() as isize
}

/// The C's `sqSetupSSL`: context, options, ciphers, certificate, verify
/// paths, then the SSL with its two BIOs. Answers false on failure, which
/// the callers turn into `SQSSL_GENERIC_ERROR`.
///
/// The C takes a `server` flag it never uses -- the certificate is loaded
/// whenever `certName` is set, client or server -- so there is no parameter
/// here.
fn setup_ssl(ssl: &mut SqSsl) -> bool {
    unsafe {
        // The C guards this behind `if (!initialized)` but never sets
        // `initialized`, so it runs on every setup. OPENSSL_init_ssl is
        // documented idempotent, so simply do the same.
        ffi::OPENSSL_init_ssl(0, ptr::null());

        // SSLv23_method() is an alias for TLS_method() since 1.1.
        let method = ffi::TLS_method();
        ssl.ctx = ffi::SSL_CTX_new(method);
        // The C checks for a null context only *after* calling
        // SSL_CTX_set_options on it -- undefined behaviour on allocation
        // failure. Check first instead; same observable outcome.
        if ssl.ctx.is_null() {
            drain_error_queue();
            return false;
        }
        ffi::SSL_CTX_set_options(ssl.ctx, ffi::SSL_OP_NO_SSLv2 | ffi::SSL_OP_NO_SSLv3);

        // Return value ignored, as in the C.
        ffi::SSL_CTX_set_cipher_list(ssl.ctx, c"!ADH:HIGH:MEDIUM:@STRENGTH".as_ptr());

        // If a cert is provided, use it. The one PEM file holds both the
        // certificate and the private key.
        if let Some(cert_name) = &ssl.cert_name {
            if ffi::SSL_CTX_use_certificate_file(ssl.ctx, cert_name.as_ptr(), ffi::SSL_FILETYPE_PEM)
                <= 0
            {
                drain_error_queue();
                return false;
            }
            if ffi::SSL_CTX_use_PrivateKey_file(ssl.ctx, cert_name.as_ptr(), ffi::SSL_FILETYPE_PEM)
                <= 0
            {
                drain_error_queue();
                return false;
            }
        }

        // No root CA given; use the default verify paths.
        if ffi::SSL_CTX_set_default_verify_paths(ssl.ctx) <= 0 {
            drain_error_queue();
            return false;
        }

        ssl.ssl = ffi::SSL_new(ssl.ctx);
        // The C passes a null SSL straight to SSL_set_bio on allocation
        // failure; fail cleanly instead.
        if ssl.ssl.is_null() {
            drain_error_queue();
            return false;
        }
        // Hands ownership of both BIOs to the SSL: SSL_free frees them.
        ffi::SSL_set_bio(ssl.ssl, ssl.bio_read, ssl.bio_write);
    }
    true
}

/// `SSL_get_peer_certificate` was renamed in OpenSSL 3.0; build.rs sets the
/// cfg from the version `openssl-sys` linked. Both return an owned reference
/// the caller must `X509_free`.
unsafe fn peer_certificate(ssl: *const ffi::SSL) -> *mut ffi::X509 {
    #[cfg(sq_ossl300)]
    unsafe {
        ffi::SSL_get1_peer_certificate(ssl)
    }
    #[cfg(not(sq_ossl300))]
    unsafe {
        ffi::SSL_get_peer_certificate(ssl)
    }
}

/// The certificate subject's commonName, as the C reads it into its
/// `peerName[MAX_HOSTNAME_LENGTH + 1]` buffer.
///
/// The C never checks `X509_NAME_get_text_by_NID`'s result, so a certificate
/// without a CN leaves the stack buffer uninitialized and `strndup`s garbage
/// -- undefined behaviour. Here that case answers the empty string.
fn common_name_of(cert: *mut ffi::X509) -> CString {
    let mut buf = [0u8; MAX_HOSTNAME_LENGTH + 1];
    let rc = unsafe {
        extra::X509_NAME_get_text_by_NID(
            ffi::X509_get_subject_name(cert),
            ffi::NID_commonName,
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len() as c_int,
        )
    };
    if rc < 0 {
        return CString::default();
    }
    // The call NUL-terminates within the buffer; take up to the first NUL,
    // like the C's strndup of the buffer.
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len() - 1);
    CString::new(&buf[..len]).unwrap_or_default()
}

/// The post-handshake certificate check shared by connect and accept: record
/// the peer name and fold the verify result into `certFlags`.
///
/// `check_server_name` distinguishes the C's two copies of this code: the
/// client (connect) tries the serverName against the certificate's
/// sAN/commonName first; the server (accept) only ever reads the CN.
fn note_peer_certificate(ssl: &mut SqSsl, check_server_name: bool) {
    let cert = unsafe { peer_certificate(ssl.ssl) };
    if cert.is_null() {
        ssl.cert_flags = SQSSL_NO_CERTIFICATE;
        return;
    }

    if check_server_name {
        // Verify that the peer is the one we expect (by name, via cert); see
        // the long RFC 6125 comment in the C. On a match the serverName is
        // copied into peerName, so the image can test
        // `self peerName = self serverName`.
        ssl.peer_name = None;
        let mut matched = NO_MATCH_DONE_YET;
        if let Some(server_name) = &ssl.server_name {
            // The C measures with strnlen(serverName, MAX_HOSTNAME_LENGTH),
            // silently truncating an over-long name; keep that.
            let name_len = server_name.to_bytes().len().min(MAX_HOSTNAME_LENGTH);
            unsafe {
                // Try IP first; INVALID_IP_STRING means "not an IP literal",
                // so fall through to host-name matching.
                matched = ffi::X509_check_ip_asc(cert, server_name.as_ptr(), 0);
                if matched == INVALID_IP_STRING {
                    matched = ffi::X509_check_host(
                        cert,
                        server_name.as_ptr(),
                        name_len,
                        ffi::X509_CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS,
                        ptr::null_mut(),
                    );
                }
            }
            if matched == MATCH_FOUND {
                let truncated = &server_name.to_bytes()[..name_len];
                ssl.peer_name = CString::new(truncated).ok();
            }
        }
        // Fallback for a missing sAN or an unset serverName. Note that a
        // *failed* match (NO_MATCH_FOUND) leaves peerName unset, which the
        // image reads as the empty string.
        if matched == NO_MATCH_DONE_YET || matched == NO_SAN_PRESENT {
            ssl.peer_name = Some(common_name_of(cert));
        }
    } else {
        ssl.peer_name = Some(common_name_of(cert));
    }

    unsafe {
        ffi::X509_free(cert);
        // FIXME in the C: figure out the actual failure reason. It never
        // does; everything non-OK is SQSSL_OTHER_ISSUE.
        let result = ffi::SSL_get_verify_result(ssl.ssl);
        ssl.cert_flags = if result == ffi::X509_V_OK as c_long {
            SQSSL_OK
        } else {
            SQSSL_OTHER_ISSUE
        };
    }
}

// ---------------------------------------------------------------------------
// The sq*SSL entry points
// ---------------------------------------------------------------------------

/// `sqCreateSSL`: creates a session and answers its handle (never 0).
pub fn create_ssl() -> isize {
    let session = unsafe {
        let bio_read = ffi::BIO_new(ffi::BIO_s_mem());
        let bio_write = ffi::BIO_new(ffi::BIO_s_mem());
        // BIO_set_close(bio, BIO_CLOSE) as in the C -- the default for a
        // memory BIO, but kept for fidelity.
        ffi::BIO_ctrl(bio_read, BIO_CTRL_SET_CLOSE, BIO_CLOSE, ptr::null_mut());
        ffi::BIO_ctrl(bio_write, BIO_CTRL_SET_CLOSE, BIO_CLOSE, ptr::null_mut());
        SqSsl {
            state: SQSSL_UNUSED,
            cert_flags: 0,
            loglevel: 0,
            cert_name: None,
            peer_name: None,
            server_name: None,
            ctx: ptr::null_mut(),
            ssl: ptr::null_mut(),
            bio_read,
            bio_write,
        }
    };
    let mut table = TABLE.lock().expect("SSL handle table poisoned");
    allocate_handle(&mut table, session) as isize
}

/// `sqDestroySSL`: answers non-zero if the handle was valid.
pub fn destroy_ssl(handle: isize) -> isize {
    let mut table = TABLE.lock().expect("SSL handle table poisoned");
    let taken = usize::try_from(handle)
        .ok()
        .and_then(|slot| table.get_mut(slot)?.take());
    // Dropping the session frees its OpenSSL state.
    if taken.is_some() {
        1
    } else {
        0
    }
}

/// `sqConnectSSL`: start or continue the client handshake.
///
/// Answers the number of bytes written to `dst` for the server, 0 once the
/// connection is established, `SQSSL_NEED_MORE_DATA` for more input, or an
/// error code.
pub fn connect_ssl(handle: isize, src: &[u8], dst: &mut [u8]) -> isize {
    with_session(handle, |ssl| {
        if ssl.state != SQSSL_UNUSED && ssl.state != SQSSL_CONNECTING {
            return SQSSL_INVALID_STATE;
        }

        if ssl.state == SQSSL_UNUSED {
            ssl.state = SQSSL_CONNECTING;
            if !setup_ssl(ssl) {
                return SQSSL_GENERIC_ERROR;
            }
            unsafe { ffi::SSL_set_connect_state(ssl.ssl) };
        }

        if !feed_input(ssl, src) {
            return SQSSL_GENERIC_ERROR;
        }

        // If a server name is provided, use it for SNI. The C does this on
        // every pump, not just the first; after the ClientHello has gone out
        // it is a no-op.
        if let Some(server_name) = &ssl.server_name {
            unsafe { ffi::SSL_set_tlsext_host_name(ssl.ssl, server_name.as_ptr().cast_mut()) };
        }

        let result = unsafe { ffi::SSL_connect(ssl.ssl) };
        if result <= 0 {
            let error = unsafe { ffi::SSL_get_error(ssl.ssl, result) };
            if error != ffi::SSL_ERROR_WANT_READ {
                drain_error_queue();
                return SQSSL_GENERIC_ERROR;
            }
            return copy_bio(ssl.bio_write, dst);
        }

        // We are connected. Verify the cert.
        ssl.state = SQSSL_CONNECTED;
        note_peer_certificate(ssl, true);
        // Faithful quirk: any handshake bytes still in the write BIO (the
        // client's Finished, under TLS 1.3) are *not* copied out here; the C
        // returns 0 and they leave with the next encrypt's copy_bio.
        SQSSL_OK
    })
    .unwrap_or(SQSSL_INVALID_STATE)
}

/// `sqAcceptSSL`: start or continue the server handshake. Requires the
/// certificate name to be set. Same return convention as [`connect_ssl`].
pub fn accept_ssl(handle: isize, src: &[u8], dst: &mut [u8]) -> isize {
    with_session(handle, |ssl| {
        if ssl.state != SQSSL_UNUSED && ssl.state != SQSSL_ACCEPTING {
            return SQSSL_INVALID_STATE;
        }

        if ssl.state == SQSSL_UNUSED {
            ssl.state = SQSSL_ACCEPTING;
            if !setup_ssl(ssl) {
                return SQSSL_GENERIC_ERROR;
            }
            unsafe { ffi::SSL_set_accept_state(ssl.ssl) };
        }

        if !feed_input(ssl, src) {
            return SQSSL_GENERIC_ERROR;
        }

        let result = unsafe { ffi::SSL_accept(ssl.ssl) };
        if result <= 0 {
            let error = unsafe { ffi::SSL_get_error(ssl.ssl, result) };
            if error != ffi::SSL_ERROR_WANT_READ {
                drain_error_queue();
                return SQSSL_GENERIC_ERROR;
            }
            // Unlike connect, the C maps "no output yet" (0) explicitly to
            // NEED_MORE_DATA here.
            let count = copy_bio(ssl.bio_write, dst);
            return if count != 0 { count } else { SQSSL_NEED_MORE_DATA };
        }

        // We are connected. Verify the (client) cert, if one was sent.
        ssl.state = SQSSL_CONNECTED;
        note_peer_certificate(ssl, false);
        // The final flight (and, under TLS 1.3, the session tickets) goes
        // out with this copy, so success can answer a positive count.
        copy_bio(ssl.bio_write, dst)
    })
    .unwrap_or(SQSSL_INVALID_STATE)
}

/// `sqEncryptSSL`: encrypt `src` into `dst`. Requires an established
/// session. Answers the bytes written to `dst` or an error code.
pub fn encrypt_ssl(handle: isize, src: &[u8], dst: &mut [u8]) -> isize {
    with_session(handle, |ssl| {
        if ssl.state != SQSSL_CONNECTED {
            return SQSSL_INVALID_STATE;
        }
        // The C calls SSL_write unconditionally, zero-length input included,
        // and demands it report exactly srcLen bytes written.
        let nbytes = unsafe {
            ffi::SSL_write(ssl.ssl, src.as_ptr().cast::<c_void>(), src.len() as c_int)
        };
        if nbytes as isize != src.len() as isize {
            return SQSSL_GENERIC_ERROR;
        }
        copy_bio(ssl.bio_write, dst)
    })
    .unwrap_or(SQSSL_INVALID_STATE)
}

/// `sqDecryptSSL`: decrypt `src` into `dst`. Requires an established
/// session. Answers the bytes decrypted, 0 when a whole TLS record has not
/// arrived yet, or an error code.
pub fn decrypt_ssl(handle: isize, src: &[u8], dst: &mut [u8]) -> isize {
    with_session(handle, |ssl| {
        if ssl.state != SQSSL_CONNECTED {
            return SQSSL_INVALID_STATE;
        }
        if !feed_input(ssl, src) {
            return SQSSL_GENERIC_ERROR;
        }
        let nbytes = unsafe {
            ffi::SSL_read(ssl.ssl, dst.as_mut_ptr().cast::<c_void>(), dst.len() as c_int)
        };
        if nbytes <= 0 {
            let error = unsafe { ffi::SSL_get_error(ssl.ssl, nbytes) };
            // WANT_READ: an incomplete record. ZERO_RETURN: the peer closed
            // cleanly. WANT_X509_LOOKUP: mid-callback. All answer 0 bytes.
            if error != ffi::SSL_ERROR_WANT_READ
                && error != ffi::SSL_ERROR_ZERO_RETURN
                && error != ffi::SSL_ERROR_WANT_X509_LOOKUP
            {
                drain_error_queue();
                return SQSSL_GENERIC_ERROR;
            }
            return 0;
        }
        nbytes as isize
    })
    .unwrap_or(SQSSL_INVALID_STATE)
}

/// `sqGetStringPropertySSL`. `None` is the C's NULL, which the primitive
/// turns into `nil`; note that an unset peer name is the *empty string*, not
/// `nil`, while unset cert/server names are `nil`.
pub fn get_string_property_ssl(handle: isize, prop_id: c_int) -> Option<Vec<u8>> {
    with_session(handle, |ssl| match prop_id {
        SQSSL_PROP_PEERNAME => Some(
            ssl.peer_name
                .as_ref()
                .map_or_else(Vec::new, |name| name.to_bytes().to_vec()),
        ),
        SQSSL_PROP_CERTNAME => ssl.cert_name.as_ref().map(|name| name.to_bytes().to_vec()),
        SQSSL_PROP_SERVERNAME => ssl
            .server_name
            .as_ref()
            .map(|name| name.to_bytes().to_vec()),
        _ => None,
    })
    .flatten()
}

/// `sqSetStringPropertySSL`: answers non-zero if the property was set.
///
/// The C `strndup`s the whole ByteArray, so the stored value stops at the
/// first NUL byte, and a zero-length value stores NULL -- i.e. clears the
/// property back to `nil`. Only CERTNAME and SERVERNAME are settable.
pub fn set_string_property_ssl(handle: isize, prop_id: c_int, value: &[u8]) -> isize {
    with_session(handle, |ssl| {
        let property = if value.is_empty() {
            None
        } else {
            let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
            // Cannot fail: everything before `end` is non-NUL.
            CString::new(&value[..end]).ok()
        };
        match prop_id {
            SQSSL_PROP_CERTNAME => ssl.cert_name = property,
            SQSSL_PROP_SERVERNAME => ssl.server_name = property,
            _ => return 0,
        }
        1
    })
    .unwrap_or(0)
}

/// `sqGetIntPropertySSL`. An invalid handle or unknown property answers 0 --
/// and the primitive *succeeds* with 0, as in the C.
pub fn get_int_property_ssl(handle: isize, prop_id: isize) -> isize {
    with_session(handle, |ssl| match prop_id {
        SQSSL_PROP_SSLSTATE => ssl.state,
        SQSSL_PROP_CERTSTATE => ssl.cert_flags,
        SQSSL_PROP_VERSION => SQSSL_VERSION,
        SQSSL_PROP_LOGLEVEL => ssl.loglevel,
        _ => 0,
    })
    .unwrap_or(0)
}

/// `sqSetIntPropertySSL`: answers non-zero if the property was set. Only the
/// log level is settable.
pub fn set_int_property_ssl(handle: isize, prop_id: isize, value: isize) -> isize {
    with_session(handle, |ssl| match prop_id {
        SQSSL_PROP_LOGLEVEL => {
            ssl.loglevel = value;
            1
        }
        _ => 0,
    })
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The C's numbering: first handle is 1, slot 0 never handed out.
    #[test]
    fn handles_start_at_one() {
        let mut table: Vec<Option<u8>> = Vec::new();
        assert_eq!(allocate_handle(&mut table, 7), 1);
        assert_eq!(table.len(), HANDLE_DELTA);
        assert!(table[0].is_none());
        assert_eq!(allocate_handle(&mut table, 8), 2);
    }

    /// Freed slots are reused first-fit, as the C's linear scan does.
    #[test]
    fn freed_handles_are_reused_first_fit() {
        let mut table: Vec<Option<u8>> = Vec::new();
        for expected in 1..=5 {
            assert_eq!(allocate_handle(&mut table, 0), expected);
        }
        table[3] = None;
        assert_eq!(allocate_handle(&mut table, 0), 3);
        assert_eq!(allocate_handle(&mut table, 0), 6);
    }

    /// A full table grows by the C's delta and keeps allocating.
    #[test]
    fn table_grows_by_delta_when_full() {
        let mut table: Vec<Option<u8>> = Vec::new();
        for expected in 1..HANDLE_DELTA {
            assert_eq!(allocate_handle(&mut table, 0), expected);
        }
        // Slots 1..=DELTA-1 are busy: the next allocation is the first to
        // fall off the end, forcing the C's grow-by-delta.
        assert_eq!(allocate_handle(&mut table, 0), HANDLE_DELTA);
        assert_eq!(table.len(), 2 * HANDLE_DELTA);
    }

    /// The invalid-handle codes the image relies on, per SqueakSSL.h.
    #[test]
    fn invalid_handles_answer_the_c_error_codes() {
        let mut buf = [0u8; 16];
        for bad in [-1, 0, 999_999] {
            assert_eq!(connect_ssl(bad, &[], &mut buf), SQSSL_INVALID_STATE);
            assert_eq!(accept_ssl(bad, &[], &mut buf), SQSSL_INVALID_STATE);
            assert_eq!(encrypt_ssl(bad, &[], &mut buf), SQSSL_INVALID_STATE);
            assert_eq!(decrypt_ssl(bad, &[], &mut buf), SQSSL_INVALID_STATE);
            assert_eq!(destroy_ssl(bad), 0);
            assert_eq!(get_int_property_ssl(bad, SQSSL_PROP_VERSION), 0);
            assert_eq!(get_string_property_ssl(bad, SQSSL_PROP_PEERNAME), None);
            assert_eq!(set_int_property_ssl(bad, SQSSL_PROP_LOGLEVEL, 1), 0);
            assert_eq!(set_string_property_ssl(bad, SQSSL_PROP_CERTNAME, b"x"), 0);
        }
    }

    /// Encrypt and decrypt demand an established session.
    #[test]
    fn encrypt_and_decrypt_require_connected_state() {
        let handle = create_ssl();
        let mut buf = [0u8; 16];
        assert_eq!(encrypt_ssl(handle, b"data", &mut buf), SQSSL_INVALID_STATE);
        assert_eq!(decrypt_ssl(handle, b"data", &mut buf), SQSSL_INVALID_STATE);
        assert_eq!(destroy_ssl(handle), 1);
    }

    /// Destroy invalidates the handle; a second destroy fails.
    #[test]
    fn destroy_invalidates_the_handle() {
        let handle = create_ssl();
        assert_eq!(destroy_ssl(handle), 1);
        assert_eq!(destroy_ssl(handle), 0);
        let mut buf = [0u8; 16];
        assert_eq!(connect_ssl(handle, &[], &mut buf), SQSSL_INVALID_STATE);
    }

    /// Integer properties: version, log level, unknown IDs.
    #[test]
    fn int_properties_behave_like_the_c() {
        let handle = create_ssl();
        assert_eq!(get_int_property_ssl(handle, SQSSL_PROP_VERSION), 3);
        assert_eq!(get_int_property_ssl(handle, SQSSL_PROP_SSLSTATE), SQSSL_UNUSED);
        assert_eq!(get_int_property_ssl(handle, SQSSL_PROP_CERTSTATE), 0);
        assert_eq!(get_int_property_ssl(handle, SQSSL_PROP_LOGLEVEL), 0);
        assert_eq!(set_int_property_ssl(handle, SQSSL_PROP_LOGLEVEL, 4), 1);
        assert_eq!(get_int_property_ssl(handle, SQSSL_PROP_LOGLEVEL), 4);
        // Only the log level is settable; version is not.
        assert_eq!(set_int_property_ssl(handle, SQSSL_PROP_VERSION, 9), 0);
        assert_eq!(get_int_property_ssl(handle, 42), 0);
        assert_eq!(destroy_ssl(handle), 1);
    }

    /// String properties: peer name defaults to "", the others to nil (None);
    /// values stop at the first NUL; a zero-length write clears to nil.
    #[test]
    fn string_properties_behave_like_the_c() {
        let handle = create_ssl();
        assert_eq!(
            get_string_property_ssl(handle, SQSSL_PROP_PEERNAME),
            Some(Vec::new())
        );
        assert_eq!(get_string_property_ssl(handle, SQSSL_PROP_CERTNAME), None);
        assert_eq!(get_string_property_ssl(handle, SQSSL_PROP_SERVERNAME), None);

        assert_eq!(
            set_string_property_ssl(handle, SQSSL_PROP_SERVERNAME, b"example.org"),
            1
        );
        assert_eq!(
            get_string_property_ssl(handle, SQSSL_PROP_SERVERNAME).as_deref(),
            Some(b"example.org".as_slice())
        );

        // strndup semantics: the stored value stops at the first NUL byte.
        assert_eq!(
            set_string_property_ssl(handle, SQSSL_PROP_CERTNAME, b"cert.pem\0junk"),
            1
        );
        assert_eq!(
            get_string_property_ssl(handle, SQSSL_PROP_CERTNAME).as_deref(),
            Some(b"cert.pem".as_slice())
        );

        // A zero-length value clears the property back to nil.
        assert_eq!(set_string_property_ssl(handle, SQSSL_PROP_CERTNAME, b""), 1);
        assert_eq!(get_string_property_ssl(handle, SQSSL_PROP_CERTNAME), None);

        // The peer name is not settable, nor are unknown IDs.
        assert_eq!(set_string_property_ssl(handle, SQSSL_PROP_PEERNAME, b"x"), 0);
        assert_eq!(set_string_property_ssl(handle, 42, b"x"), 0);
        assert_eq!(destroy_ssl(handle), 1);
    }

    /// A wrong-role pump on a session in progress is an invalid state, as is
    /// pumping a session that is already connected.
    #[test]
    fn mixed_roles_are_an_invalid_state() {
        let handle = create_ssl();
        let mut buf = vec![0u8; 1 << 14];
        // Starting a client handshake moves the state to CONNECTING...
        let n = connect_ssl(handle, &[], &mut buf);
        assert!(n > 0, "first flight should produce a ClientHello, got {n}");
        assert_eq!(
            get_int_property_ssl(handle, SQSSL_PROP_SSLSTATE),
            SQSSL_CONNECTING
        );
        // ...after which accept refuses the session.
        assert_eq!(accept_ssl(handle, &[], &mut buf), SQSSL_INVALID_STATE);
        assert_eq!(destroy_ssl(handle), 1);
    }

    /// `copy_bio`'s faithful quirk: a destination smaller than the pending
    /// data answers -1 (colliding with NEED_MORE_DATA), and the data stays
    /// in the BIO for a retry with a bigger buffer.
    #[test]
    fn too_small_destination_answers_minus_one_and_preserves_data() {
        let handle = create_ssl();
        let mut tiny = [0u8; 4];
        assert_eq!(connect_ssl(handle, &[], &mut tiny), -1);
        let mut big = vec![0u8; 1 << 14];
        let n = connect_ssl(handle, &[], &mut big);
        assert!(n > 0, "retry with a big buffer should yield the flight");
        assert_eq!(destroy_ssl(handle), 1);
    }
}
