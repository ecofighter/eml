use std::fs;
use std::path::Path;

use eml_tls::fixtures::{CA_PEM, CERT_PEM, KEY_PEM};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

#[test]
fn the_fixtures_parse() {
    for pem in [CA_PEM, CERT_PEM] {
        let certs: Vec<_> = CertificateDer::pem_slice_iter(pem)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(certs.len(), 1);
    }
    // PKCS#8 の形でなければ `PRIVATE KEY` 以外の見出しとして弾かれる
    PrivatePkcs8KeyDer::from_pem_slice(KEY_PEM).unwrap();
}

// UI テストは自分のディレクトリの `ca.pem` を読むので、写しが fixtures とずれると
// handshake の失敗の理由が証明書の不一致に化ける
#[test]
fn every_ca_copy_is_the_fixture() {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui");
    let mut found = Vec::new();
    let mut stack = vec![ui.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "ca.pem") {
                assert_eq!(fs::read(&path).unwrap(), CA_PEM, "{}", path.display());
                found.push(path.strip_prefix(&ui).unwrap().to_path_buf());
            }
        }
    }
    for expected in ["run/tls/ca.pem", "run-fail/tls/ca.pem", "run/http/ca.pem", "run-fail/http/ca.pem"] {
        assert!(found.contains(&expected.into()), "{expected} がない");
    }
}
