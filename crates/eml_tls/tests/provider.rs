use eml_tls::fixtures::CA_PEM;
use eml_tls::{Conn, HANDSHAKING, HAS_OUTGOING, Name, Trust};

// aws-lc の C のコードが組まれてリンクされ、鍵の共有と乱数で実際に呼ばれることを確かめる
#[test]
fn a_client_hello_comes_out_of_aws_lc() {
    let name = Name::parse("tls.localhost").unwrap();
    let mut conn = Conn::client(&name, &Trust::fixtures(CA_PEM).unwrap()).unwrap();
    assert_eq!(conn.status(), HANDSHAKING | HAS_OUTGOING);
    let mut out = Vec::new();
    conn.pull_tls(&mut out);
    assert_eq!(out[0], 0x16); // handshake のレコード
    assert_eq!(conn.status(), HANDSHAKING);
}
