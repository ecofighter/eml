use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};

// aws-lc の C のコードが組まれてリンクされ、鍵の共有と乱数で実際に呼ばれることを確かめる
#[test]
fn a_client_hello_comes_out_of_aws_lc() {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from("tls.localhost").unwrap();
    let mut conn = ClientConnection::new(Arc::new(config), name).unwrap();
    let mut out = Vec::new();
    conn.write_tls(&mut out).unwrap();
    assert_eq!(out[0], 0x16); // handshake のレコード
    assert!(conn.is_handshaking());
}
