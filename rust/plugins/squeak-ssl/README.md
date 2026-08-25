# SqueakSSL, in Rust

Replaces the Unix build of the **SqueakSSL** plugin — the Slang-generated
primitive shims in `plugins/SqueakSSL/src/common/SqueakSSL.c` plus the
OpenSSL implementation in `plugins/SqueakSSL/src/unix/sqUnixSSL.c` — behind
the same ten primitives. **macOS and Windows keep their C implementations**
(`sqMacSSL.c` over Security.framework, `sqWin32SSL.c` over SChannel); this
crate only ever stood in for the OpenSSL one, and declines nothing at load
time because on those platforms the C plugin is what gets built.

The plugin does TLS over memory BIOs: the image owns the socket and shuttles
raw TLS bytes through `primitiveConnect`/`primitiveAccept` (the handshake
pumps), `primitiveEncrypt`/`primitiveDecrypt` (the record layer), with
sessions as small-integer handles into a global table and their knobs behind
get/set int/string property primitives.

## The contract is unchanged

Same module name, same ten primitives, same argument order, same accessor
depths (checked byte-for-byte against the C's `...AccessorDepth` exports,
including `primitiveCreate`'s implicit −1), same handle semantics (first
handle is 1, slots reused first-fit, table grown by 100), and the same return
codes from `SqueakSSL.h` — `SQSSL_INVALID_STATE` for a bad handle or a
wrong-state call, `SQSSL_NEED_MORE_DATA` from a starved pump,
`SQSSL_NO_CERTIFICATE`/`SQSSL_OTHER_ISSUE` in `SQSSL_PROP_CERTSTATE`, version
3 from `SQSSL_PROP_VERSION`.

## What changed underneath

**OpenSSL through `openssl-sys`, not the safe wrapper.** The C's usage — two
memory BIOs plumbed into an `SSL` with `SSL_set_bio`, `SSL_connect`/
`SSL_accept` pumped by hand — is exactly the raw API's shape and not the safe
crate's stream-oriented one, so `src/ssl.rs` transcribes the C call for call:
`BIO_new(BIO_s_mem())`, `SSL_CTX_new(TLS_method())` (`SSLv23_method` is its
1.1+ alias), `SSL_OP_NO_SSLv2|SSL_OP_NO_SSLv3`, the
`"!ADH:HIGH:MEDIUM:@STRENGTH"` cipher list, PEM cert/key loading from the
`SQSSL_PROP_CERTNAME` file, `SSL_CTX_set_default_verify_paths`, SNI via
`SSL_set_tlsext_host_name`, and peer-name extraction via `X509_check_ip_asc`
→ `X509_check_host(..., X509_CHECK_FLAG_SINGLE_LABEL_SUBDOMAINS, ...)` →
subject-CN fallback. Three functions `openssl-sys` does not bind
(`BIO_ctrl_pending`, `X509_NAME_get_text_by_NID`, `ERR_error_string_n`) are
declared locally against the same library. The safe `openssl` crate appears
only as a dev-dependency, to mint the tests' throwaway certificate.

**OpenSSL 1.1+/3.x only.** The C's `OPENSSL_VERSION_NUMBER < 0x10100000L`
paths — `SSL_library_init` and the `ASN1_STRING_get0_data` fallback define —
are dropped; initialisation is `OPENSSL_init_ssl`. The 3.0 rename of
`SSL_get_peer_certificate` to `SSL_get1_peer_certificate` is bridged by
`build.rs` from the version `openssl-sys` reports.

**Static OpenSSL by default.** The C plugin links the system
`libssl`/`libcrypto`; this crate's default `vendored` feature instead builds
OpenSSL from source (`openssl-src`) and links it statically, so
`cargo build -p squeak-ssl` needs no OpenSSL development headers. Build with
`--no-default-features` to link the system OpenSSL exactly as the C did
(requires headers, via pkg-config or `OPENSSL_DIR`). In the vendored cdylib
every OpenSSL symbol is hidden (`--exclude-libs,ALL`), so a system libssl
mapped elsewhere in the VM process cannot interpose against it.

**Undefined behaviour removed** (each marked with a comment at the site):

* `sslFromHandle` checks only `handle < handleMax` before indexing, so a
  negative handle reads out of bounds. Here it is simply invalid.
* `sqSetupSSL` calls `SSL_CTX_set_options` on the new context *before*
  checking it for NULL, and passes `SSL_new`'s result to `SSL_set_bio`
  without any check. Both are checked first here; the observable outcome
  (`SQSSL_GENERIC_ERROR`) is the same.
* A peer certificate without a commonName leaves the C's stack buffer
  uninitialized — `X509_NAME_get_text_by_NID`'s result is never checked —
  and `strndup` then copies garbage into the peer name. Here that case
  answers the empty string.
* The pumps' dead `if (n < 0)` after `if (n < srcLen)` is dropped.

**Smaller mechanical differences.**

* The C hands the destination ByteArray's memory straight to
  `BIO_read`/`SSL_read`; this port stages through a Rust buffer and copies
  back exactly the bytes produced. One consequence: an *immutable*
  destination now fails the primitive cleanly instead of being written
  through.
* Wrong arity or a non-integer argument fails with the SDK's specific codes
  (`BadNumArgs`, `BadArgument`) where the generated C used the generic
  `primitiveFail()`. The `SQSSL_*` codes the image actually inspects are
  untouched.
* `logTrace` calls are dropped (`SQSSL_PROP_LOGLEVEL` is still stored and
  answered, and was never consulted by the C either); the
  `ERR_print_errors_fp` calls become an explicit drain of OpenSSL's error
  queue to stderr — draining also keeps stale entries from confusing a later
  `SSL_get_error`.

**Faithful oddities, deliberately kept.**

* `sqCopyBioSSL` answers −1 when the destination is smaller than the pending
  data — colliding with `SQSSL_NEED_MORE_DATA` rather than using
  `SQSSL_BUFFER_TOO_SMALL`. The data stays in the BIO for a retry.
* When `SSL_connect` completes under TLS 1.3, the client's Finished is
  already in the write BIO but `primitiveConnect` answers 0 without copying
  it out; it leaves with the first `primitiveEncrypt`. The tests reproduce
  the image's behaviour around this.
* `OPENSSL_init_ssl` runs on every session setup: the C guards it with an
  `initialized` flag it never sets. (The call is documented idempotent.)
* SNI is (re)asserted on every connect pump, not just the first.
* Any verification failure is folded to `SQSSL_OTHER_ISSUE` — the C carries
  a FIXME to report the real reason and never does.
* `primitiveGetIntProperty` on an invalid handle or unknown property
  *succeeds* with 0; only the setters fail.
* A zero-length `primitiveSetStringProperty` value stores NULL, i.e. clears
  the property back to `nil`, and a stored value stops at its first NUL byte
  (the C `strndup`s the whole ByteArray).

## Verification

There is no VM in the development environment, so the core is factored into
VM-free functions over byte buffers (`src/ssl.rs`) and exercised in-process,
client against server — 17 tests:

* **Handshake and data** (`tests/handshake.rs`): full client/server
  handshake against a per-run self-signed certificate (nothing checked in);
  plaintext roundtrips both directions; a 100 kB payload crosses TLS record
  boundaries through repeated decrypts; state progression
  UNUSED→CONNECTING/ACCEPTING→CONNECTED as the image polls it.
* **Peer naming**: with `SQSSL_PROP_SERVERNAME` matching the cert's sAN the
  peer name answers the server name (the image's
  `self peerName = self serverName` check); without a server name it falls
  back to the CN; with a wrong server name the handshake still completes but
  the peer name stays empty.
* **Certificate status**: the self-signed cert surfaces
  `SQSSL_OTHER_ISSUE` on the client, the absent client cert
  `SQSSL_NO_CERTIFICATE` on the server; accepting without a cert installed
  fails with a code below −1, never a crash.
* **Handle and property semantics** (unit tests in `src/ssl.rs`): the C's
  handle numbering, first-fit reuse and grow-by-100 (as a pure function);
  every invalid-handle code; encrypt/decrypt before the handshake; the
  role-mixing `SQSSL_INVALID_STATE`; the −1 too-small-buffer quirk with the
  data preserved for retry; int/string property tables including NUL
  truncation and clear-to-nil.

## Not verified

* **Image-side differential testing.** No VM here; the primitive glue
  (argument shapes, `signed32BitValueOf` for the set-int value, nil vs
  String answers from `primitiveGetStringProperty`) compiles against the SDK
  but has not run under a real interpreter.
* **Verification against a real CA.** All tests use a self-signed cert, so
  `SSL_CTX_set_default_verify_paths` succeeding into `SQSSL_OK` cert state
  has not been observed. Note the vendored OpenSSL looks in *its own*
  compiled-in default paths, which need not match the distro's; it honours
  `SSL_CERT_FILE`/`SSL_CERT_DIR`, and `--no-default-features` builds use the
  system paths as the C did.
* **Server-side client-certificate verification** (the C would surface it
  through the same accept path) and encrypted/passphrase-protected key files
  (the C would block on a terminal prompt; untested here as there).
* **CMake wiring.** The crate builds and exports the exact C symbol surface
  (checked with `nm`), but the build still compiles the C plugin; switching
  the Unix build over is a separate, reviewable change. The `sq*SSL` support
  functions from `SqueakSSL.h` are internal to the plugin and consumed by
  nothing else in the tree, so they are not re-exported as C symbols.
