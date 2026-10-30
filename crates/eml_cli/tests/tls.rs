//! 本物の driver の上で `Std.Tls` が、ローカルの TLS の server と往復する。Windows の本物の driver にはソケットが
//! ないので unix だけで流す (docs/spec/stdlib.md の「`Std.Tls`」)。
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

/// 1行を受け取り、`pong` を返して close_notify を送り、相手が閉じるまで読んで、受け取った行を返す。
fn serve(listener: TcpListener) -> String {
    let (mut tcp, _) = listener.accept().unwrap();
    // 止まったクライアントでテストが固まらず、失敗になるようにする
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut conn = Conn::server(fixtures::CERT_PEM, fixtures::KEY_PEM).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    let mut line = Vec::new();
    while !line.contains(&b'\n') {
        conn.pull_tls(&mut out);
        tcp.write_all(&out).unwrap();
        out.clear();
        let n = tcp.read(&mut buf).unwrap();
        assert_ne!(n, 0, "the client closed before it sent a line");
        conn.push_tls(&buf[..n]);
        conn.read_plain(&mut line);
        assert_eq!(conn.status() & FAILED, 0, "{}", conn.error_message());
    }
    conn.write_plain(b"pong\n");
    conn.send_close_notify();
    conn.pull_tls(&mut out);
    tcp.write_all(&out).unwrap();
    while tcp.read(&mut buf).unwrap() != 0 {}
    String::from_utf8(line).unwrap()
}

#[test]
fn a_client_talks_to_a_local_tls_server() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || serve(listener));

    let src = format!(
        r#"import Async
import Bytes
import Fs
import Tls
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let s = Tls.connect_with_ca (Fs.read_text "ca.pem") "localhost" {port}
  let s = Tls.write_line "ping" s
  let (line, s) = Tls.read_line s
  match line with
    | Some text -> println "Some \{{text}}"
    | None -> println "None"
  let (rest, s) = Tls.read_chunk (Bytes.empty ()) s
  println (show (Bytes.length rest))
  Tls.close s)
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
    let result = execute(&program, &config, host);
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(out.contents(), "Some pong\n0\n");
    assert_eq!(server.join().unwrap(), "ping\n");
}
