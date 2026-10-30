//! `bench/scripts.sh` の行 `https-get` の相手になる、ループバックの HTTPS の server
//! (docs/implementation/benchmarks.md の「スクリプトの計測」)。
//!
//! `https_server MIB` は `127.0.0.1` の空いたポートで待ち受け、ポートを標準出力の1行目に出す。どの要求にも
//! `Content-Length` 付きで MIB MiB の本文を返す。止めるのは呼ぶ側の signal である。鍵と証明書は `fixtures` の
//! ものを使い、何も認証しない。

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_tls::fixtures::{CERT_PEM, KEY_PEM};
use rustls::crypto::CryptoProvider;
use rustls::crypto::aws_lc_rs::{cipher_suite, default_provider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// 本文の行の長さ。行の番号を15桁で書くので、最後の行から本文の長さと並びの崩れが読める
const LINE: usize = 16;

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .expect("usage: https_server MIB");
    // 計測の最中に本文を作らないよう、ポートを出す前に作る
    let mut body = Vec::with_capacity(mib << 20);
    for i in 0..(mib << 20) / LINE {
        writeln!(body, "{i:015}").unwrap();
    }
    let body: Arc<[u8]> = body.into();
    let chain = CertificateDer::pem_slice_iter(CERT_PEM)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY_PEM).unwrap();
    // eml の rustls と Python の OpenSSL が別の暗号を選ぶと、比に暗号の速さの差が混ざる。そこで暗号を1つに絞る
    let provider = CryptoProvider {
        cipher_suites: vec![cipher_suite::TLS13_AES_256_GCM_SHA384],
        ..default_provider()
    };
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    println!("{}", listener.local_addr().unwrap().port());
    for tcp in listener.incoming() {
        let (config, body) = (config.clone(), body.clone());
        // クライアントが閉じるのを待つ間も次の接続を受けられるよう、接続ごとにスレッドを立てる
        thread::spawn(move || {
            if let Err(e) = tcp.and_then(|tcp| serve(config, tcp, &body)) {
                eprintln!("https_server: {e}");
            }
        });
    }
}

fn serve(config: Arc<ServerConfig>, tcp: TcpStream, body: &[u8]) -> io::Result<()> {
    // 止まったクライアントのスレッドを残さない
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    let conn = ServerConnection::new(config).map_err(io::Error::other)?;
    let mut tls = StreamOwned::new(conn, tcp);
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = tls.read(&mut buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.extend_from_slice(&buf[..n]);
    }
    write!(
        tls,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    for chunk in body.chunks(1 << 20) {
        tls.write_all(chunk)?;
    }
    tls.conn.send_close_notify();
    tls.flush()?;
    // 書く側だけを閉じる。EOF を待つクライアントはすぐ終わり、後から close_notify を送るクライアントも reset を
    // 受けない。読む側は、クライアントが閉じるか時間が切れるまで開けておく
    tls.sock.shutdown(Shutdown::Write)?;
    tls.sock.set_read_timeout(Some(Duration::from_secs(2)))?;
    while tls.sock.read(&mut buf).is_ok_and(|n| n > 0) {}
    Ok(())
}
