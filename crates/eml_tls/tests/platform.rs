//! 本物のOSの検証器に届くテスト。handshakeは`NOW`の時刻で確かめるので、結果は日付に左右されない
//! (docs/implementation/testing.mdの「OS ごとのテスト」)。

use eml_tls::fixtures::{CA_PEM, CERT_PEM, KEY_PEM, NOW, OTHER_CA_PEM};
use eml_tls::{Conn, FAILED, Name, Trust};

// どちらも出すものがなくなるまで、暗号文を相手に渡す
fn pump(a: &mut Conn, b: &mut Conn) {
    loop {
        let mut ab = Vec::new();
        a.pull_tls(&mut ab);
        b.push_tls(&ab);
        let mut ba = Vec::new();
        b.pull_tls(&mut ba);
        a.push_tls(&ba);
        if ab.is_empty() && ba.is_empty() {
            break;
        }
    }
}

fn handshake(name: &str, trust: &Trust) -> Conn {
    let mut client = Conn::client(&Name::parse(name).unwrap(), trust).unwrap();
    let mut server = Conn::server(CERT_PEM, KEY_PEM).unwrap();
    pump(&mut client, &mut server);
    client
}

#[test]
fn an_extra_root_is_trusted() {
    let trust = Trust::platform_at(CA_PEM, NOW).unwrap();
    for name in ["tls.localhost", "127.0.0.1"] {
        let c = handshake(name, &trust);
        assert_eq!(c.status(), 0, "{name}: {}", c.error_message());
    }
}

#[test]
fn an_unknown_ca_is_rejected() {
    let c = handshake(
        "tls.localhost",
        &Trust::platform_at(OTHER_CA_PEM, NOW).unwrap(),
    );
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.error_message(), "the server certificate was rejected");
}

#[test]
fn a_name_not_in_the_certificate_is_rejected() {
    let c = handshake("example.com", &Trust::platform_at(CA_PEM, NOW).unwrap());
    assert_eq!(c.status() & FAILED, FAILED);
    assert_eq!(c.error_message(), "the server certificate was rejected");
}

/// これがあれば、テストは子として本体を流す
#[cfg(all(unix, not(target_vendor = "apple")))]
const CHILD: &str = "EML_TLS_NO_ROOTS_CHILD";

/// 子の中では`body`を流す。親では、nixのsandboxと同じくシステムの根の証明書がない子 (`SSL_CERT_FILE`が`/dev/null`で、
/// `SSL_CERT_DIR`がない) を起こして同じテストだけを流させ、子の中でテストが通ったことを確かめる
#[cfg(all(unix, not(target_vendor = "apple")))]
fn without_system_roots(path: &str, body: impl FnOnce()) {
    use std::process::Command;

    if std::env::var_os(CHILD).is_some() {
        body();
        return;
    }
    // libtestのテストの道はcrateの名前を含まない
    let name = path
        .strip_prefix("integration::")
        .expect("a path in the integration tests");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("SSL_CERT_FILE", "/dev/null")
        .env_remove("SSL_CERT_DIR")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("1 passed"), "{stdout}{stderr}");
}

#[test]
#[cfg(all(unix, not(target_vendor = "apple")))]
fn no_system_roots_and_no_ca_is_no_roots() {
    without_system_roots(
        concat!(module_path!(), "::no_system_roots_and_no_ca_is_no_roots"),
        || {
            assert_eq!(Trust::platform(b"").unwrap_err(), eml_tls::Error::NoRoots);
        },
    );
}

#[test]
#[cfg(all(unix, not(target_vendor = "apple")))]
fn no_system_roots_with_a_ca_handshakes() {
    without_system_roots(
        concat!(module_path!(), "::no_system_roots_with_a_ca_handshakes"),
        || {
            let c = handshake("tls.localhost", &Trust::platform_at(CA_PEM, NOW).unwrap());
            assert_eq!(c.status(), 0, "{}", c.error_message());
        },
    );
}
