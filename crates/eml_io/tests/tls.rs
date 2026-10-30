//! テストの driver の TLS の終端器。`tls.localhost` への接続を Rust の `eml_tls::Conn` のクライアントで動かし、
//! 平文の側をテストが受けた端で読み書きする (docs/spec/vm.md の「時計と driver」)。

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use eml_io::TestDriver;
use eml_rt::{Done, IoDriver, IoHandle, Op, OpKey, RawBuf, Submit};
use eml_tls::fixtures::CA_PEM;
use eml_tls::{Conn, FAILED, HANDSHAKING, PEER_CLOSED, TRUNCATED};

fn raw(bytes: &mut [u8]) -> RawBuf {
    RawBuf {
        ptr: bytes.as_mut_ptr(),
        cap: bytes.len(),
    }
}

/// テストが振る鍵。1 から数え、どの driver にも同じ鍵を2度渡さない。
fn next_key() -> OpKey {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// 受け付けで決まる操作を出し、その結果を返す。
fn run_op(driver: &mut TestDriver, op: Op) -> std::io::Result<Done> {
    match driver.submit(next_key(), op) {
        Submit::Done(result) => result,
        Submit::Pending => panic!("an op decided at submit is pending"),
    }
}

/// 時計を進めずに決着させる。終端器はこの中で汲む。
fn settle(driver: &mut TestDriver) -> Vec<OpKey> {
    let now = driver.now().as_millis() as u64;
    assert!(driver.wait(Some(now)).unwrap());
    let mut out = Vec::new();
    driver.completions(&mut out);
    out.iter().map(|c| c.key).collect()
}

/// 1回読む。受け付けで終わらなければ1回だけ決着させ、その読みが終わることを確かめる。
fn read(driver: &mut TestDriver, handle: IoHandle) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0; 64 * 1024];
    let key = next_key();
    let result = match driver.submit(
        key,
        Op::Read {
            handle,
            buf: raw(&mut buf),
        },
    ) {
        Submit::Done(result) => result,
        Submit::Pending => {
            let mut out = Vec::new();
            let now = driver.now().as_millis() as u64;
            assert!(driver.wait(Some(now)).unwrap());
            driver.completions(&mut out);
            assert_eq!(out.iter().map(|c| c.key).collect::<Vec<_>>(), [key]);
            out.pop().unwrap().result
        }
    };
    let Done::Size(n) = result? else {
        panic!("a read gives a size")
    };
    buf.truncate(n);
    Ok(buf)
}

fn write(driver: &mut TestDriver, handle: IoHandle, bytes: &[u8]) -> std::io::Result<()> {
    let mut bytes = bytes.to_vec();
    run_op(
        driver,
        Op::Write {
            handle,
            buf: raw(&mut bytes),
        },
    )
    .map(|_| ())
}

/// クライアントの出す暗号文を書く。
fn flush(driver: &mut TestDriver, handle: IoHandle, conn: &mut Conn) {
    let mut out = Vec::new();
    conn.pull_tls(&mut out);
    if !out.is_empty() {
        write(driver, handle, &out).unwrap();
    }
}

/// handshake を終えるか失敗するまで、暗号文を読んで渡し、出すものを書く。
fn handshake(driver: &mut TestDriver, handle: IoHandle, conn: &mut Conn) {
    flush(driver, handle, conn);
    while conn.status() & HANDSHAKING != 0 && conn.status() & FAILED == 0 {
        let bytes = read(driver, handle).unwrap();
        if bytes.is_empty() {
            conn.push_eof();
            break;
        }
        conn.push_tls(&bytes);
        flush(driver, handle, conn);
    }
}

/// EOF まで読んで平文を集める。
fn read_to_eof(driver: &mut TestDriver, handle: IoHandle, conn: &mut Conn) -> Vec<u8> {
    let mut plain = Vec::new();
    loop {
        let bytes = read(driver, handle).unwrap();
        if bytes.is_empty() {
            conn.push_eof();
            conn.read_plain(&mut plain);
            return plain;
        }
        conn.push_tls(&bytes);
        conn.read_plain(&mut plain);
    }
}

fn listen(driver: &mut TestDriver, port: u16) -> std::io::Result<(IoHandle, u16)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match run_op(driver, Op::Listen { addr })? {
        Done::Listener { handle, port } => Ok((handle, port)),
        _ => panic!("a listen gives a listener"),
    }
}

fn connect(
    driver: &mut TestDriver,
    host: &str,
    port: u16,
) -> std::io::Result<(IoHandle, SocketAddr)> {
    let op = Op::Connect {
        host: host.to_owned(),
        port,
    };
    match run_op(driver, op)? {
        Done::Stream { handle, peer } => Ok((handle, peer)),
        _ => panic!("a connect gives a stream"),
    }
}

fn accept(driver: &mut TestDriver, listener: IoHandle) -> (IoHandle, SocketAddr) {
    match run_op(driver, Op::Accept { listener }).unwrap() {
        Done::Stream { handle, peer } => (handle, peer),
        _ => panic!("an accept gives a stream"),
    }
}

/// 待ち受け、`host` に接続し、受ける。クライアントの暗号文の端と、受けた平文の端を返す。
fn connected(driver: &mut TestDriver, host: &str) -> (IoHandle, IoHandle) {
    let (listener, port) = listen(driver, 0).unwrap();
    let (client, _) = connect(driver, host, port).unwrap();
    let (server, _) = accept(driver, listener);
    (client, server)
}

fn client() -> Conn {
    Conn::client("tls.localhost", CA_PEM).unwrap()
}

#[test]
fn a_tls_connect_reaches_the_plaintext_listener() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    assert_eq!(port, 49152);
    let (client_end, peer) = connect(&mut driver, "tls.localhost", port).unwrap();
    assert_eq!(peer, "127.0.0.1:49152".parse().unwrap());
    let (server, peer) = accept(&mut driver, listener);
    assert_eq!(peer, "127.0.0.1:49153".parse().unwrap());
    let mut conn = client();
    handshake(&mut driver, client_end, &mut conn);
    assert_eq!(conn.status() & (HANDSHAKING | FAILED), 0);
    conn.write_plain(b"hello");
    flush(&mut driver, client_end, &mut conn);
    assert_eq!(read(&mut driver, server).unwrap(), b"hello");
}

#[test]
fn a_large_body_and_close_notify_reach_the_client() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls.localhost");
    let mut conn = client();
    handshake(&mut driver, client_end, &mut conn);
    write(&mut driver, server, &vec![b'x'; 200 * 1024]).unwrap();
    driver.close(server);
    let body = read_to_eof(&mut driver, client_end, &mut conn);
    assert_eq!(body.len(), 204800);
    assert_eq!(conn.status(), PEER_CLOSED);
    conn.send_close_notify();
    flush(&mut driver, client_end, &mut conn);
    driver.close(client_end);
    settle(&mut driver);
    listen(&mut driver, 49153).unwrap();
    listen(&mut driver, 49154).unwrap();
}

#[test]
fn a_half_close_lets_the_server_reply() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls.localhost");
    let mut conn = client();
    handshake(&mut driver, client_end, &mut conn);
    conn.write_plain(b"req");
    conn.send_close_notify();
    flush(&mut driver, client_end, &mut conn);
    driver.shutdown_write(client_end).unwrap();
    assert_eq!(read(&mut driver, server).unwrap(), b"req");
    assert_eq!(read(&mut driver, server).unwrap(), b"");
    write(&mut driver, server, b"done").unwrap();
    driver.close(server);
    assert_eq!(read_to_eof(&mut driver, client_end, &mut conn), b"done");
    assert_eq!(conn.status() & (PEER_CLOSED | TRUNCATED), PEER_CLOSED);
}

#[test]
fn the_cut_host_closes_without_close_notify() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls-cut.localhost");
    let mut conn = client();
    handshake(&mut driver, client_end, &mut conn);
    write(&mut driver, server, b"partial").unwrap();
    driver.close(server);
    assert_eq!(read_to_eof(&mut driver, client_end, &mut conn), b"partial");
    assert_eq!(conn.status() & (PEER_CLOSED | TRUNCATED), TRUNCATED);
}

#[test]
fn a_rejected_certificate_closes_the_plaintext_end() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls.localhost");
    let mut conn = Conn::client("tls.localhost", b"").unwrap();
    handshake(&mut driver, client_end, &mut conn);
    assert_ne!(conn.status() & FAILED, 0);
    settle(&mut driver);
    assert_eq!(read(&mut driver, server).unwrap(), b"");
    assert_eq!(read(&mut driver, client_end).unwrap(), b"");
}

#[test]
fn a_server_close_before_any_client_byte_gives_eof() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls.localhost");
    driver.close(server);
    let mut conn = client();
    flush(&mut driver, client_end, &mut conn);
    assert_eq!(read(&mut driver, client_end).unwrap(), b"");
    assert_ne!(conn.status() & HANDSHAKING, 0);
}

#[test]
fn a_server_close_during_the_handshake_waits_for_it() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let (client_end, _) = connect(&mut driver, "tls.localhost", port).unwrap();
    let mut conn = client();
    flush(&mut driver, client_end, &mut conn);
    let flight = read(&mut driver, client_end).unwrap();
    assert!(!flight.is_empty());
    let (server, _) = accept(&mut driver, listener);
    driver.close(server);
    conn.push_tls(&flight);
    flush(&mut driver, client_end, &mut conn);
    assert_eq!(conn.status() & (HANDSHAKING | FAILED), 0);
    assert_eq!(read_to_eof(&mut driver, client_end, &mut conn), b"");
    assert_eq!(conn.status() & (PEER_CLOSED | TRUNCATED), PEER_CLOSED);
}

#[test]
fn a_client_close_leaves_the_server_its_bytes_then_eof() {
    let mut driver = TestDriver::new(None);
    let (client_end, server) = connected(&mut driver, "tls.localhost");
    let mut conn = client();
    handshake(&mut driver, client_end, &mut conn);
    conn.write_plain(b"req");
    flush(&mut driver, client_end, &mut conn);
    driver.close(client_end);
    assert_eq!(read(&mut driver, server).unwrap(), b"req");
    assert_eq!(read(&mut driver, server).unwrap(), b"");
    let err = write(&mut driver, server, b"late").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::BrokenPipe);
    listen(&mut driver, 49153).unwrap();
    listen(&mut driver, 49154).unwrap();
}

#[test]
fn a_tls_name_without_a_listener_is_refused() {
    let mut driver = TestDriver::new(None);
    let err = connect(&mut driver, "tls.localhost", 49152).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ConnectionRefused);
}
