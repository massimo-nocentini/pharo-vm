//! Decides which peer-certificate accessor to bind.
//!
//! OpenSSL 3.0 renamed `SSL_get_peer_certificate` to
//! `SSL_get1_peer_certificate` (the old name survives only as a preprocessor
//! define), and `openssl-sys` mirrors that split: each name exists only for
//! the versions that export it. Which one this crate must call therefore
//! depends on the OpenSSL that `openssl-sys` found, which its build script
//! reports through `DEP_OPENSSL_VERSION_NUMBER` (the hex `OPENSSL_VERSION_NUMBER`).

fn main() {
    // Declare the custom cfg so `-D warnings` builds do not trip
    // `unexpected_cfgs`. Cargo older than 1.80 ignores this line with a
    // warning, which is harmless.
    println!("cargo:rustc-check-cfg=cfg(sq_ossl300)");

    // When OpenSSL is linked statically (the default `vendored` feature),
    // its several thousand symbols would otherwise be re-exported by the
    // cdylib; if the VM process also maps the system libssl, dynamic-linker
    // interposition could then mix two OpenSSL builds. Hide everything that
    // comes from static archives -- the plugin's own exports are unaffected.
    // GNU-ld syntax, so Linux only; the other platforms keep the C plugin.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-libs,ALL");
    }
    if let Ok(v) = std::env::var("DEP_OPENSSL_VERSION_NUMBER") {
        if let Ok(n) = u64::from_str_radix(&v, 16) {
            if n >= 0x3000_0000 {
                println!("cargo:rustc-cfg=sq_ossl300");
            }
        }
    }
}
