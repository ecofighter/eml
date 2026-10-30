use std::sync::Arc;

use eml_tls::fixtures::{CA_PEM, CERT_PEM, KEY_PEM};
use eml_tls::{Conn, Error, FAILED, HANDSHAKING, HAS_OUTGOING, PEER_CLOSED, TRUNCATED};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

fn client(name: &str) -> Conn {
    Conn::client(name, CA_PEM).unwrap()
}

fn server() -> Conn {
    Conn::server(CERT_PEM, KEY_PEM).unwrap()
}

fn pull(conn: &mut Conn) -> Vec<u8> {
    let mut out = Vec::new();
    conn.pull_tls(&mut out);
    out
}

// どちらも出すものがなくなるまで、暗号文を相手に渡す
fn pump(a: &mut Conn, b: &mut Conn) {
    loop {
        let ab = pull(a);
        b.push_tls(&ab);
        let ba = pull(b);
        a.push_tls(&ba);
        if ab.is_empty() && ba.is_empty() {
            break;
        }
    }
}

fn connected() -> (Conn, Conn) {
    let mut c = client("tls.localhost");
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status(), 0);
    assert_eq!(s.status(), 0);
    (c, s)
}

fn read_all(conn: &mut Conn) -> Vec<u8> {
    let mut out = Vec::new();
    conn.read_plain(&mut out);
    out
}

#[test]
fn handshake_then_bytes_both_ways() {
    let (mut c, mut s) = connected();
    c.write_plain(b"hello");
    s.write_plain(b"world");
    pump(&mut c, &mut s);
    assert_eq!(read_all(&mut s), b"hello");
    assert_eq!(read_all(&mut c), b"world");
    assert_eq!(c.status(), 0);
    assert_eq!(s.status(), 0);
}

#[test]
fn an_ip_name_checks_the_ip_san() {
    let mut c = client("127.0.0.1");
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status(), 0);
}

#[test]
fn the_leaf_is_valid_for_each_name() {
    for name in ["tls.localhost", "localhost", "127.0.0.1"] {
        let mut c = client(name);
        let mut s = server();
        pump(&mut c, &mut s);
        assert_eq!(c.status(), 0, "{name}: {}", c.error_message());
    }
    let mut c = client("example.com");
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.error_message(), "the server certificate was rejected");
}

#[test]
fn one_push_of_many_records_moves_all_plaintext() {
    let (mut c, mut s) = connected();
    let data: Vec<u8> = (0..200 * 1024).map(|i| (i % 251) as u8).collect();
    s.write_plain(&data);
    let records = pull(&mut s);
    c.push_tls(&records);
    assert_eq!(c.plaintext(), &data[..]);
}

#[test]
fn close_notify_is_peer_closed() {
    let (mut c, mut s) = connected();
    s.write_plain(b"last");
    s.send_close_notify();
    pump(&mut c, &mut s);
    assert_eq!(c.status(), PEER_CLOSED);
    assert_eq!(read_all(&mut c), b"last");
}

#[test]
fn eof_without_close_notify_is_truncated() {
    let (mut c, mut s) = connected();
    s.write_plain(b"last");
    pump(&mut c, &mut s);
    c.push_eof();
    assert_eq!(c.status(), TRUNCATED);
    assert_eq!(read_all(&mut c), b"last");
}

#[test]
fn eof_in_the_handshake_is_truncated() {
    let mut c = client("tls.localhost");
    pull(&mut c);
    c.push_eof();
    assert_eq!(c.status(), HANDSHAKING | TRUNCATED);
}

#[test]
fn an_unknown_ca_is_rejected_with_a_fixed_message() {
    let mut c = Conn::client("tls.localhost", b"").unwrap();
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.error_message(), "the server certificate was rejected");
    assert_eq!(s.status() & FAILED, FAILED);
}

#[test]
fn a_name_not_in_the_certificate_is_rejected() {
    let mut c = client("example.com");
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.error_message(), "the server certificate was rejected");
}

#[test]
fn the_alert_waits_until_pulled() {
    let mut c = client("example.com");
    let mut s = server();
    let hello = pull(&mut c);
    s.push_tls(&hello);
    let flight = pull(&mut s);
    c.push_tls(&flight);
    assert_eq!(c.status(), HANDSHAKING | HAS_OUTGOING | FAILED);
    assert!(!pull(&mut c).is_empty());
    assert_eq!(c.status(), HANDSHAKING | FAILED);
}

#[test]
fn setup_errors() {
    for name in ["", "a b"] {
        assert_eq!(Conn::client(name, CA_PEM).err(), Some(Error::ServerName));
    }
    let bad_der = b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    for ca in [&b"garbage"[..], &bad_der[..]] {
        assert_eq!(Conn::client("tls.localhost", ca).err(), Some(Error::CaPem));
    }
    assert_eq!(Conn::server(b"", KEY_PEM).err(), Some(Error::CertPem));
    assert_eq!(
        Conn::server(CERT_PEM, b"garbage").err(),
        Some(Error::KeyPem)
    );
    // 証明書の PEM には鍵がない
    assert_eq!(Conn::server(CERT_PEM, CA_PEM).err(), Some(Error::KeyPem));
    let texts = [
        Error::ServerName,
        Error::CaPem,
        Error::CertPem,
        Error::KeyPem,
        Error::Config,
    ]
    .map(|e| e.to_string());
    assert_eq!(
        texts,
        [
            "the server name is not valid",
            "the CA certificates are not valid PEM",
            "the certificate chain is not valid PEM",
            "the private key is not valid PEM",
            "the TLS configuration is not valid",
        ]
    );
}

#[test]
fn non_tls_bytes_fail() {
    let mut c = client("tls.localhost");
    pull(&mut c);
    c.push_tls(b"HTTP/1.1 400 Bad Request\r\n\r\n");
    assert_eq!(c.status() & FAILED, FAILED);
    assert!(!c.error_message().is_empty());
}

#[test]
fn plaintext_written_before_the_handshake_is_sent_after_it() {
    let mut c = client("tls.localhost");
    let mut s = server();
    let data = vec![7u8; 100_000];
    c.write_plain(&data);
    pump(&mut c, &mut s);
    assert_eq!(read_all(&mut s), data);
}

#[test]
fn a_byte_at_a_time() {
    let mut c = client("tls.localhost");
    let mut s = server();
    loop {
        let ab = pull(&mut c);
        for b in &ab {
            s.push_tls(std::slice::from_ref(b));
        }
        let ba = pull(&mut s);
        for b in &ba {
            c.push_tls(std::slice::from_ref(b));
        }
        if ab.is_empty() && ba.is_empty() {
            break;
        }
    }
    assert_eq!(c.status(), 0);
    assert_eq!(s.status(), 0);
}

#[test]
fn nothing_is_read_after_close_notify() {
    let (mut c, mut s) = connected();
    s.write_plain(b"before");
    s.send_close_notify();
    s.write_plain(b"after");
    let records = pull(&mut s);
    c.push_tls(&records);
    assert_eq!(c.status(), PEER_CLOSED);
    assert_eq!(read_all(&mut c), b"before");
}

#[test]
fn close_notify_after_a_failure_sends_nothing() {
    let mut c = client("example.com");
    let mut s = server();
    pump(&mut c, &mut s);
    assert_eq!(c.status() & FAILED, FAILED);
    c.send_close_notify();
    assert!(pull(&mut c).is_empty());
}

#[test]
fn write_after_close_notify_still_makes_records() {
    let (mut c, _s) = connected();
    c.send_close_notify();
    pull(&mut c);
    c.write_plain(b"late");
    assert_eq!(c.status(), HAS_OUTGOING);
}

#[test]
fn data_then_garbage_in_one_push_fails_and_keeps_the_data() {
    let (mut c, mut s) = connected();
    s.write_plain(b"good");
    let mut bytes = pull(&mut s);
    bytes.extend_from_slice(&[0xff; 64]);
    c.push_tls(&bytes);
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.plaintext(), b"good");
}

#[test]
fn a_tls12_only_server_handshakes() {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(CERT_PEM)
        .collect::<Result<_, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY_PEM).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let mut s = rustls::ServerConnection::new(Arc::new(config)).unwrap();
    let mut c = client("tls.localhost");
    loop {
        let ab = pull(&mut c);
        s.read_tls(&mut &ab[..]).unwrap();
        s.process_new_packets().unwrap();
        let mut ba = Vec::new();
        while s.wants_write() {
            s.write_tls(&mut ba).unwrap();
        }
        c.push_tls(&ba);
        if ab.is_empty() && ba.is_empty() {
            break;
        }
    }
    assert_eq!(c.status(), 0);
    assert_eq!(s.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_2));
    // 往復
    c.write_plain(b"ping");
    let records = pull(&mut c);
    s.read_tls(&mut &records[..]).unwrap();
    s.process_new_packets().unwrap();
    let mut got = [0u8; 4];
    std::io::Read::read_exact(&mut s.reader(), &mut got).unwrap();
    assert_eq!(&got, b"ping");
    std::io::Write::write_all(&mut s.writer(), b"pong").unwrap();
    let mut back = Vec::new();
    while s.wants_write() {
        s.write_tls(&mut back).unwrap();
    }
    c.push_tls(&back);
    assert_eq!(read_all(&mut c), b"pong");
}

#[test]
fn read_plain_moves_everything() {
    let (mut c, mut s) = connected();
    s.write_plain(b"one two");
    pump(&mut c, &mut s);
    c.consume(4);
    assert_eq!(c.plaintext(), b"two");
    let mut out = b"x".to_vec();
    assert_eq!(c.read_plain(&mut out), 3);
    assert_eq!(out, b"xtwo");
    assert!(c.plaintext().is_empty());
    assert_eq!(c.read_plain(&mut out), 0);
}

// VM は `Conn` を `alloc_foreign` で値のまま置くので、alignment が8を超えてはならない
#[test]
fn the_box_fits_alloc_foreign() {
    assert!(std::mem::align_of::<Conn>() <= 8);
}

// 自分が close_notify を送った後も、相手の壊れたレコードは panic でなく失敗になる
#[test]
fn a_bad_record_after_close_notify_fails() {
    let (mut c, _s) = connected();
    c.send_close_notify();
    pull(&mut c);
    let mut junk = vec![0x17, 3, 3, 0, 0x20];
    junk.extend_from_slice(&[0x5a; 32]);
    c.push_tls(&junk);
    assert_eq!(c.status() & FAILED, FAILED);
    assert!(!c.error_message().is_empty());
}
