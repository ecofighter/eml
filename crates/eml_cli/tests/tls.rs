//! 本物のdriverの上で`Std.Tls`は、サーバーの証明書に署名したCAをプログラムが渡しても、ローカルのTLSのserverの
//! 証明書を固定の文言で拒む。本物のdriverのrunは現実の時刻で確かめ、そこではテスト用の証明書は期限切れである (docs/spec/vm.mdの
//! 「時計と driver」)。本物のdriverのrunが、有効期間の中の固定の時刻で確かめるテストの世界の信頼を使えば、この
//! テストは失敗する。
//!
//! `eml_tls` は `tls` の feature が足す依存で、dev-dependency にはしない。dev-dependency にすると、tls を外した
//! ビルドの `cargo tree` にも `eml_tls` が現れる (docs/implementation/testing.md の「feature の組み合わせの確認」)。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_cli::{
    Clock, Control, MemorySource, RunConfig, Session, capture_host, default_limits, execute,
};
use eml_tls::{Conn, FAILED, fixtures};

/// クライアントのalertが届くまでhandshakeに応える。失敗を記録したか、クライアントが閉じたら届いたとする。
fn serve(listener: TcpListener) {
    let (mut tcp, _) = listener.accept().unwrap();
    // 止まったクライアントでテストが固まらず、失敗になるようにする
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut conn = Conn::server(fixtures::CERT_PEM, fixtures::KEY_PEM).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        conn.pull_tls(&mut out);
        tcp.write_all(&out).unwrap();
        out.clear();
        let n = tcp.read(&mut buf).unwrap();
        if n == 0 {
            return;
        }
        conn.push_tls(&buf[..n]);
        if conn.status() & FAILED != 0 {
            return;
        }
    }
}

#[test]
fn a_real_socket_handshake_rejects_the_expired_fixtures() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || serve(listener));

    let src = format!(
        r#"import Async
import Fs
import Tls
main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    try_io (fn () -> Tls.close (Tls.connect_with_ca (Fs.read_text "ca.pem") "127.0.0.1" {port})))
  match r with
    | Ok () -> println "connected"
    | Err e ->
      println (show e.kind)
      println e.message
"#
    );
    let compiled = Session::load("tls.em", &src, &MemorySource(&[])).compile();
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let program = compiled.program.unwrap();
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_control(Arc::new(Control::new(default_limits())));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../eml_tls/fixtures");
    let (host, out) = capture_host(root);
    let result = execute(&program, &eml_cli::Linked::default(), &config, host);
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        out.contents(),
        "Other\ncannot complete the TLS handshake with 127.0.0.1: the server certificate was rejected\n"
    );
    server.join().unwrap();
}
