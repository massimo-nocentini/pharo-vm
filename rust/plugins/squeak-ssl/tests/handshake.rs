//! Client-against-server TLS handshakes with no VM and no network.
//!
//! The image drives SqueakSSL by shuttling opaque byte buffers between two
//! endpoints; these tests do exactly that in one process, with both endpoints
//! served by this plugin's core. The server's certificate is a throwaway
//! self-signed one generated per test run -- nothing is checked in.

use std::path::PathBuf;
use std::sync::OnceLock;

use SqueakSSL::ssl::{
    accept_ssl, connect_ssl, create_ssl, decrypt_ssl, destroy_ssl, encrypt_ssl,
    get_int_property_ssl, get_string_property_ssl, set_string_property_ssl, SQSSL_ACCEPTING,
    SQSSL_CONNECTED, SQSSL_CONNECTING, SQSSL_NEED_MORE_DATA, SQSSL_NO_CERTIFICATE,
    SQSSL_OTHER_ISSUE, SQSSL_PROP_CERTNAME, SQSSL_PROP_CERTSTATE, SQSSL_PROP_PEERNAME,
    SQSSL_PROP_SERVERNAME, SQSSL_PROP_SSLSTATE, SQSSL_UNUSED,
};

/// Big enough for any single handshake flight or a few TLS records.
const BUF: usize = 1 << 16;

/// CN and sAN of the test certificate.
const HOST: &str = "localhost";

/// Writes a self-signed certificate for [`HOST`] plus its private key into
/// one PEM file -- the layout `SQSSL_PROP_CERTNAME` expects -- and answers
/// the path. Generated once per test binary.
fn cert_file() -> &'static str {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        use openssl::asn1::Asn1Time;
        use openssl::bn::BigNum;
        use openssl::hash::MessageDigest;
        use openssl::nid::Nid;
        use openssl::pkey::PKey;
        use openssl::rsa::Rsa;
        use openssl::x509::extension::SubjectAlternativeName;
        use openssl::x509::{X509NameBuilder, X509};

        let key = PKey::from_rsa(Rsa::generate(2048).expect("generate key")).expect("wrap key");

        let mut name = X509NameBuilder::new().expect("name builder");
        name.append_entry_by_nid(Nid::COMMONNAME, HOST).expect("CN");
        let name = name.build();

        let mut cert = X509::builder().expect("cert builder");
        cert.set_version(2).expect("version");
        let serial = BigNum::from_u32(1)
            .and_then(|bn| bn.to_asn1_integer())
            .expect("serial");
        cert.set_serial_number(&serial).expect("set serial");
        cert.set_subject_name(&name).expect("subject");
        cert.set_issuer_name(&name).expect("issuer");
        cert.set_pubkey(&key).expect("pubkey");
        cert.set_not_before(&Asn1Time::days_from_now(0).expect("time"))
            .expect("not before");
        cert.set_not_after(&Asn1Time::days_from_now(1).expect("time"))
            .expect("not after");
        let san = SubjectAlternativeName::new()
            .dns(HOST)
            .build(&cert.x509v3_context(None, None))
            .expect("sAN");
        cert.append_extension(san).expect("append sAN");
        cert.sign(&key, MessageDigest::sha256()).expect("sign");
        let cert = cert.build();

        let mut pem = cert.to_pem().expect("cert PEM");
        pem.extend(key.private_key_to_pem_pkcs8().expect("key PEM"));

        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("squeak-ssl-test.pem");
        std::fs::write(&path, pem).expect("write PEM file");
        path
    })
    .to_str()
    .expect("UTF-8 path")
}

fn state_of(handle: isize) -> isize {
    get_int_property_ssl(handle, SQSSL_PROP_SSLSTATE)
}

fn string_property(handle: isize, prop: i32) -> Option<String> {
    get_string_property_ssl(handle, prop).map(|bytes| String::from_utf8(bytes).expect("UTF-8"))
}

/// A fresh server session with the test certificate installed.
fn make_server() -> isize {
    let server = create_ssl();
    assert_eq!(
        set_string_property_ssl(server, SQSSL_PROP_CERTNAME, cert_file().as_bytes()),
        1
    );
    server
}

/// Drives the handshake to completion the way the image would, delivering
/// `msg` from client to server along the way, and answers the bytes the
/// server decrypted.
///
/// The first application write is part of the pump because under TLS 1.3 the
/// client's Finished stays parked in its write BIO when connect answers 0
/// (the C's faithful quirk) and only leaves with the first encrypt; the
/// server cannot reach CONNECTED until it arrives.
fn establish_and_send(client: isize, server: isize, msg: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; BUF];
    let first = connect_ssl(client, &[], &mut buf);
    assert!(first > 0, "first connect flight answered {first}");
    let c2s = buf[..first as usize].to_vec();
    pump_to_completion(client, server, msg, c2s, Vec::new())
}

/// The pump itself, resumable from any mid-handshake position: `c2s`/`s2c`
/// are the bytes currently in flight in each direction.
fn pump_to_completion(
    client: isize,
    server: isize,
    msg: &[u8],
    mut c2s: Vec<u8>,
    mut s2c: Vec<u8>,
) -> Vec<u8> {
    let mut buf = vec![0u8; BUF];
    let mut sent = false;
    let mut received: Vec<u8> = Vec::new();

    for _ in 0..32 {
        // Server side: pump the handshake, then decrypt what arrives.
        if state_of(server) != SQSSL_CONNECTED {
            let r = accept_ssl(server, &c2s, &mut buf);
            c2s.clear();
            assert!(r >= SQSSL_NEED_MORE_DATA, "accept answered {r}");
            if r > 0 {
                s2c.extend_from_slice(&buf[..r as usize]);
            }
        } else if !c2s.is_empty() || (sent && received.len() < msg.len()) {
            let r = decrypt_ssl(server, &c2s, &mut buf);
            c2s.clear();
            assert!(r >= 0, "server decrypt answered {r}");
            received.extend_from_slice(&buf[..r as usize]);
        }

        // Client side.
        if state_of(client) != SQSSL_CONNECTED {
            let r = connect_ssl(client, &s2c, &mut buf);
            s2c.clear();
            assert!(r >= SQSSL_NEED_MORE_DATA, "connect answered {r}");
            if r > 0 {
                c2s.extend_from_slice(&buf[..r as usize]);
            }
        } else {
            if !s2c.is_empty() {
                // Post-handshake bytes (TLS 1.3 session tickets) reach the
                // image as ordinary ciphertext; it feeds them to decrypt.
                let r = decrypt_ssl(client, &s2c, &mut buf);
                s2c.clear();
                assert!(r >= 0, "client decrypt answered {r}");
            }
            if !sent && c2s.is_empty() {
                let r = encrypt_ssl(client, msg, &mut buf);
                assert!(r > 0, "encrypt answered {r}");
                c2s.extend_from_slice(&buf[..r as usize]);
                sent = true;
            }
        }

        if state_of(client) == SQSSL_CONNECTED
            && state_of(server) == SQSSL_CONNECTED
            && sent
            && received.len() >= msg.len()
            && c2s.is_empty()
            && s2c.is_empty()
        {
            return received;
        }
    }
    panic!(
        "handshake did not converge: client state {}, server state {}",
        state_of(client),
        state_of(server)
    );
}

/// The whole point: handshake completes and data goes both ways intact.
#[test]
fn handshake_completes_and_data_roundtrips() {
    let client = create_ssl();
    let server = make_server();
    set_string_property_ssl(client, SQSSL_PROP_SERVERNAME, HOST.as_bytes());

    let request = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let received = establish_and_send(client, server, request);
    assert_eq!(received, request);

    // And the other direction: server encrypts, client decrypts.
    let reply = b"HTTP/1.1 200 OK\r\n\r\nhello from the server";
    let mut buf = vec![0u8; BUF];
    let n = encrypt_ssl(server, reply, &mut buf);
    assert!(n > 0, "server encrypt answered {n}");
    let ciphertext = buf[..n as usize].to_vec();
    assert_ne!(&ciphertext[..], &reply[..], "ciphertext must not be plaintext");
    let n = decrypt_ssl(client, &ciphertext, &mut buf);
    assert_eq!(&buf[..n as usize], reply);

    assert_eq!(destroy_ssl(client), 1);
    assert_eq!(destroy_ssl(server), 1);
}

/// A payload spanning several TLS records: SSL_read hands back one record
/// per call, so the image (and this test) loops decrypt with empty input.
#[test]
fn large_payload_crosses_record_boundaries() {
    let client = create_ssl();
    let server = make_server();
    set_string_property_ssl(client, SQSSL_PROP_SERVERNAME, HOST.as_bytes());
    establish_and_send(client, server, b"warm-up");

    let msg: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let mut cipher = vec![0u8; msg.len() + (1 << 14)];
    let n = encrypt_ssl(client, &msg, &mut cipher);
    assert!(n > 0, "encrypt answered {n}");

    let mut received = Vec::new();
    let mut buf = vec![0u8; BUF];
    let mut src: &[u8] = &cipher[..n as usize];
    while received.len() < msg.len() {
        let r = decrypt_ssl(server, src, &mut buf);
        assert!(r > 0, "decrypt stalled at {} bytes ({r})", received.len());
        received.extend_from_slice(&buf[..r as usize]);
        src = &[];
    }
    assert_eq!(received, msg);

    destroy_ssl(client);
    destroy_ssl(server);
}

/// The states the image polls via SQSSL_PROP_SSLSTATE, in order.
#[test]
fn states_progress_unused_connecting_connected() {
    let client = create_ssl();
    let server = make_server();
    set_string_property_ssl(client, SQSSL_PROP_SERVERNAME, HOST.as_bytes());

    assert_eq!(state_of(client), SQSSL_UNUSED);
    assert_eq!(state_of(server), SQSSL_UNUSED);

    let mut buf = vec![0u8; BUF];
    let n = connect_ssl(client, &[], &mut buf);
    assert!(n > 0);
    assert_eq!(state_of(client), SQSSL_CONNECTING);
    let hello = buf[..n as usize].to_vec();
    let r = accept_ssl(server, &hello, &mut buf);
    assert!(r > 0 || r == SQSSL_NEED_MORE_DATA);
    assert_eq!(state_of(server), SQSSL_ACCEPTING);

    // Resume from mid-handshake: the server's flight is in transit.
    let s2c = if r > 0 { buf[..r as usize].to_vec() } else { Vec::new() };
    pump_to_completion(client, server, b"ping", Vec::new(), s2c);
    assert_eq!(state_of(client), SQSSL_CONNECTED);
    assert_eq!(state_of(server), SQSSL_CONNECTED);

    destroy_ssl(client);
    destroy_ssl(server);
}

/// With the server name set and matching the certificate's sAN, the peer
/// name comes back equal to the server name -- the image checks
/// `self peerName = self serverName`. A self-signed certificate cannot
/// verify, so the cert state must carry SQSSL_OTHER_ISSUE; the server saw no
/// client certificate at all, so its cert state is SQSSL_NO_CERTIFICATE.
#[test]
fn peer_name_matches_and_self_signed_cert_is_flagged() {
    let client = create_ssl();
    let server = make_server();
    set_string_property_ssl(client, SQSSL_PROP_SERVERNAME, HOST.as_bytes());
    establish_and_send(client, server, b"ping");

    assert_eq!(string_property(client, SQSSL_PROP_PEERNAME).as_deref(), Some(HOST));
    assert_eq!(string_property(client, SQSSL_PROP_SERVERNAME).as_deref(), Some(HOST));
    assert_eq!(
        get_int_property_ssl(client, SQSSL_PROP_CERTSTATE),
        SQSSL_OTHER_ISSUE,
        "a self-signed certificate must fail verification"
    );

    assert_eq!(
        get_int_property_ssl(server, SQSSL_PROP_CERTSTATE),
        SQSSL_NO_CERTIFICATE,
        "the client presented no certificate"
    );
    // No client certificate: the server's peer name stays the empty string.
    assert_eq!(string_property(server, SQSSL_PROP_PEERNAME).as_deref(), Some(""));

    destroy_ssl(client);
    destroy_ssl(server);
}

/// Without a server name, the C falls back to the certificate subject's
/// commonName.
#[test]
fn peer_name_falls_back_to_the_certificate_cn() {
    let client = create_ssl();
    let server = make_server();
    establish_and_send(client, server, b"ping");

    assert_eq!(string_property(client, SQSSL_PROP_PEERNAME).as_deref(), Some(HOST));
    assert_eq!(string_property(client, SQSSL_PROP_SERVERNAME), None);

    destroy_ssl(client);
    destroy_ssl(server);
}

/// A server name the certificate does not cover: the handshake still
/// completes (the C never aborts on a name mismatch), but the peer name
/// stays empty, which is how the image detects the mismatch.
#[test]
fn wrong_server_name_leaves_peer_name_empty() {
    let client = create_ssl();
    let server = make_server();
    set_string_property_ssl(client, SQSSL_PROP_SERVERNAME, b"wrong.example");
    establish_and_send(client, server, b"ping");

    assert_eq!(string_property(client, SQSSL_PROP_PEERNAME).as_deref(), Some(""));
    assert_eq!(
        get_int_property_ssl(client, SQSSL_PROP_CERTSTATE),
        SQSSL_OTHER_ISSUE
    );

    destroy_ssl(client);
    destroy_ssl(server);
}

/// An accept without a certificate installed fails with the C's generic
/// error once the ClientHello arrives (the CertificateRequest cannot be
/// answered without a key), never crashing.
#[test]
fn accepting_without_a_certificate_fails_cleanly() {
    let client = create_ssl();
    let server = create_ssl(); // no SQSSL_PROP_CERTNAME
    let mut buf = vec![0u8; BUF];
    let n = connect_ssl(client, &[], &mut buf);
    assert!(n > 0);
    let hello = buf[..n as usize].to_vec();
    let r = accept_ssl(server, &hello, &mut buf);
    assert!(r < SQSSL_NEED_MORE_DATA, "accept without a cert answered {r}");

    destroy_ssl(client);
    destroy_ssl(server);
}
