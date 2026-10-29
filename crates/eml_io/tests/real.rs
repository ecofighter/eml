//! 本物の driver の時計、待ち、プールの仕事、ストリーム。時間は下限だけを確かめる。

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use eml_io::RealDriver;
use eml_rt::{Completion, Done, IoDriver, Op, OpKey};

#[cfg(unix)]
use eml_rt::RawBuf;

/// `key` の完了が届くまで待つ。ほかの操作の完了は `stash` に残す。1 s で届かなければ落ちる。
fn complete(driver: &mut RealDriver, stash: &mut Vec<Completion>, key: OpKey) -> Completion {
    let start = Instant::now();
    loop {
        driver.completions(stash);
        if let Some(i) = stash.iter().position(|c| c.key == key) {
            return stash.swap_remove(i);
        }
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "op {key} never completed"
        );
        assert!(driver.wait(None).unwrap());
    }
}

#[cfg(unix)]
fn raw(bytes: &mut [u8]) -> RawBuf {
    RawBuf {
        ptr: bytes.as_mut_ptr(),
        cap: bytes.len(),
    }
}

#[test]
fn a_real_clock_does_not_go_back() {
    let driver = RealDriver::new().unwrap();
    let first = driver.now();
    assert!(driver.now() >= first);
}

/// 早く戻ってよい `wait` を、呼ぶ側が確かめ直して繰り返せば、期限に着く。
#[test]
fn a_real_clock_reaches_a_deadline() {
    let mut driver = RealDriver::new().unwrap();
    let deadline = driver.now().as_millis() as u64 + 6;
    loop {
        assert!(driver.wait(Some(deadline)).unwrap());
        if driver.now() >= Duration::from_millis(deadline) {
            break;
        }
    }
}

#[test]
fn a_timed_wait_returns_at_or_after_its_deadline() {
    let mut driver = RealDriver::new().unwrap();
    let deadline = driver.now().as_millis() as u64 + 20;
    // 1回の `wait` は短く切られるので、期限に着くまで繰り返す
    while driver.now() < Duration::from_millis(deadline) {
        assert!(driver.wait(Some(deadline)).unwrap());
    }
    assert!(driver.now() >= Duration::from_millis(20));
}

#[test]
fn a_real_driver_without_work_or_deadline_cannot_progress() {
    let mut driver = RealDriver::new().unwrap();
    assert!(!driver.wait(None).unwrap());
}

/// 保留の操作がある間に別のスレッドが waker を鳴らしても、`wait` は壊れずに戻る。1回の待ちは `IDLE_TICK` で
/// 切れるので、起こしで戻ることそのものは `real.rs` の単体テストが確かめる。
#[test]
fn a_wake_from_another_thread_leaves_a_wait_working() {
    let mut driver = RealDriver::new().unwrap();
    #[cfg(unix)]
    let _ends = {
        let (a, b) = driver.pair().unwrap();
        driver.submit(Op::Readable { handle: a });
        (a, b)
    };
    #[cfg(windows)]
    driver.submit(Op::Job(Box::new(|| {
        std::thread::sleep(Duration::from_millis(200));
        Ok(0)
    })));
    let waker = driver.waker();
    let ringer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        waker.wake();
    });
    let start = Instant::now();
    assert!(driver.wait(None).unwrap());
    assert!(start.elapsed() < Duration::from_secs(1));
    ringer.join().unwrap();
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
}

/// 走っている仕事を持つ driver は、落とすときに仕事の終わりを待つ。
#[test]
fn dropping_a_driver_waits_for_a_running_job() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let finished = Arc::new(AtomicBool::new(false));
    let mut driver = RealDriver::new().unwrap();
    let flag = Arc::clone(&finished);
    driver.submit(Op::Job(Box::new(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::SeqCst);
        Ok(0)
    })));
    let start = Instant::now();
    drop(driver);
    assert!(finished.load(Ordering::SeqCst));
    assert!(start.elapsed() >= Duration::from_millis(150));
}

#[test]
fn a_job_completes_with_its_value() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = driver.submit(Op::Job(Box::new(|| Ok(42))));
    assert_eq!(driver.pending(), 1);
    let completion = complete(&mut driver, &mut stash, key);
    assert_eq!(completion.result.unwrap(), Done::Size(42));
    assert_eq!(driver.pending(), 0);
}

/// プールのスレッドは32本までで、それを超える仕事はスレッドが空くのを待ってから動く。
#[test]
fn a_job_pool_is_capped() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let keys: Vec<OpKey> = (0..33)
        .map(|i| {
            driver.submit(Op::Job(Box::new(move || {
                std::thread::sleep(Duration::from_millis(20));
                Ok(i)
            })))
        })
        .collect();
    for (i, key) in keys.into_iter().enumerate() {
        assert_eq!(
            complete(&mut driver, &mut stash, key).result.unwrap(),
            Done::Size(i)
        );
    }
    assert_eq!(driver.pending(), 0);
}

#[cfg(unix)]
#[test]
fn a_readable_probe_stays_pending_without_data() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: a });
    let deadline = driver.now().as_millis() as u64 + 20;
    while driver.now() < Duration::from_millis(deadline) {
        assert!(driver.wait(Some(deadline)).unwrap());
    }
    let mut out = Vec::new();
    driver.completions(&mut out);
    assert!(out.is_empty());
    assert_eq!(driver.pending(), 1);

    let mut byte = [7u8];
    let write = driver.submit(Op::Write {
        handle: b,
        buf: raw(&mut byte),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(1)
    );
    assert_eq!(
        complete(&mut driver, &mut stash, probe).result.unwrap(),
        Done::Size(0)
    );
    assert_eq!(driver.pending(), 0);
}

#[cfg(unix)]
#[test]
fn a_read_after_a_probe_returns_the_bytes() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: a });
    let mut data = *b"hello";
    let write = driver.submit(Op::Write {
        handle: b,
        buf: raw(&mut data),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(5)
    );
    assert_eq!(
        complete(&mut driver, &mut stash, probe).result.unwrap(),
        Done::Size(0)
    );
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: a,
        buf: raw(&mut room),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(5)
    );
    assert_eq!(&room[..5], b"hello");
}

#[cfg(unix)]
#[test]
fn a_write_is_read_by_the_peer() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = driver.pair().unwrap();
    let mut data = *b"from a";
    let write = driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut data),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(6)
    );
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: b,
        buf: raw(&mut room),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(6)
    );
    assert_eq!(&room[..6], b"from a");
}

#[cfg(unix)]
#[test]
fn a_cancelled_probe_completes_with_an_error() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, _b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: a });
    driver.cancel(probe);
    assert!(complete(&mut driver, &mut stash, probe).result.is_err());
    assert_eq!(driver.pending(), 0);
}

#[cfg(unix)]
#[test]
fn a_closed_peer_reads_eof() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: a });
    driver.close(b);
    assert_eq!(
        complete(&mut driver, &mut stash, probe).result.unwrap(),
        Done::Size(0)
    );
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: a,
        buf: raw(&mut room),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(0)
    );
}

/// 相手が読まない端に書くと、2回目の書きは送りの buffer が空くまで終わらない。`quiesce` は終わらない読みと書きを
/// 取り消し、走っている仕事を待ちきる。
#[cfg(unix)]
#[test]
fn quiesce_drains_a_pending_probe_a_full_write_and_a_running_job() {
    let mut driver = RealDriver::new().unwrap();
    let (a, _b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: a });
    let mut first = vec![1u8; 1 << 20];
    let mut second = vec![2u8; 1 << 20];
    driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut first),
    });
    let full = driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut second),
    });
    let job = driver.submit(Op::Job(Box::new(|| {
        std::thread::sleep(Duration::from_millis(30));
        Ok(0)
    })));
    let mut out = Vec::new();
    driver.completions(&mut out);
    let finished: Vec<OpKey> = out.iter().map(|c| c.key).collect();
    for key in [probe, full, job] {
        assert!(!finished.contains(&key), "op {key} finished early");
    }
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    drop(second);
    drop(first);
}

#[cfg(windows)]
#[test]
fn pair_is_unsupported() {
    let mut driver = RealDriver::new().unwrap();
    assert_eq!(driver.pair().unwrap_err().kind(), ErrorKind::Unsupported);
}

#[test]
fn an_op_on_an_unknown_handle_fails() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = driver.submit(Op::Readable { handle: 99 });
    assert_eq!(
        complete(&mut driver, &mut stash, key)
            .result
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
}

/// プールの仕事は、SIGINT と SIGTERM を塞いだスレッドで動く。塞ぐのは `submit` の間だけなので、呼んだスレッドは
/// 元の塞ぎに戻る。
#[cfg(unix)]
#[test]
fn pool_threads_block_interrupt_and_terminate() {
    fn blocked(signal: libc::c_int) -> bool {
        // SAFETY: 集合はこの関数の局所変数で、`pthread_sigmask` は読み書きするだけである
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut set),
                0
            );
            libc::sigismember(&set, signal) == 1
        }
    }
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = driver.submit(Op::Job(Box::new(|| {
        Ok(usize::from(blocked(libc::SIGINT) && blocked(libc::SIGTERM)))
    })));
    assert!(!blocked(libc::SIGINT) && !blocked(libc::SIGTERM));
    assert_eq!(
        complete(&mut driver, &mut stash, key).result.unwrap(),
        Done::Size(1)
    );
}

/// `127.0.0.1` の `port`。本物のソケットのテストは綴りだけを使い、機械の `/etc/hosts` に頼らない。
#[cfg(unix)]
fn loopback(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

/// `127.0.0.1` の空いているポートで待ち受け、リスナーとポートを返す。
#[cfg(unix)]
fn listen(driver: &mut RealDriver, stash: &mut Vec<Completion>) -> (eml_rt::IoHandle, u16) {
    let key = driver.submit(Op::Listen { addr: loopback(0) });
    match complete(driver, stash, key).result.unwrap() {
        Done::Listener { handle, port } => (handle, port),
        other => panic!("a listen ended with {other:?}"),
    }
}

#[cfg(unix)]
fn stream(completion: Completion) -> (eml_rt::IoHandle, std::net::SocketAddr) {
    match completion.result.unwrap() {
        Done::Stream { handle, peer } => (handle, peer),
        other => panic!("a stream op ended with {other:?}"),
    }
}

/// 待ち受けて、受けた端と接続した端を返す。
#[cfg(unix)]
fn connection(
    driver: &mut RealDriver,
    stash: &mut Vec<Completion>,
) -> (eml_rt::IoHandle, eml_rt::IoHandle) {
    let (listener, port) = listen(driver, stash);
    let accept = driver.submit(Op::Accept { listener });
    let connect = driver.submit(Op::Connect {
        host: "127.0.0.1".to_string(),
        port,
    });
    let (client, _) = stream(complete(driver, stash, connect));
    let (server, _) = stream(complete(driver, stash, accept));
    driver.close(listener);
    (server, client)
}

/// 待ち受ける者のいないポート。
#[cfg(unix)]
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind(loopback(0)).unwrap();
    listener.local_addr().unwrap().port()
}

#[cfg(unix)]
#[test]
fn a_listener_on_port_zero_gets_a_port() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    assert_ne!(port, 0);
    assert!(std::net::TcpStream::connect(loopback(port)).is_ok());
}

#[cfg(unix)]
#[test]
fn a_second_listener_on_the_same_port_is_addr_in_use() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    let key = driver.submit(Op::Listen {
        addr: loopback(port),
    });
    let error = complete(&mut driver, &mut stash, key).result.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
}

#[cfg(unix)]
#[test]
fn an_accept_returns_the_connected_stream_and_its_peer() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = driver.submit(Op::Accept { listener });
    let connect = driver.submit(Op::Connect {
        host: "127.0.0.1".to_string(),
        port,
    });
    let connected = complete(&mut driver, &mut stash, connect);
    assert!(!connected.cancelled);
    let (client, server_addr) = stream(connected);
    let (server, client_addr) = stream(complete(&mut driver, &mut stash, accept));
    assert_eq!(server_addr, loopback(port));
    assert!(client_addr.ip().is_loopback());
    assert_ne!(client_addr.port(), port);

    let mut data = *b"hi";
    let write = driver.submit(Op::Write {
        handle: client,
        buf: raw(&mut data),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(2)
    );
    let probe = driver.submit(Op::Readable { handle: server });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 8];
    let read = driver.submit(Op::Read {
        handle: server,
        buf: raw(&mut room),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(2)
    );
    assert_eq!(&room[..2], b"hi");
    assert!(driver.set_nodelay(client, true).is_ok());
}

#[cfg(unix)]
#[test]
fn a_connect_to_a_closed_port_is_refused() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = driver.submit(Op::Connect {
        host: "127.0.0.1".to_string(),
        port: closed_port(),
    });
    let completion = complete(&mut driver, &mut stash, key);
    assert!(!completion.cancelled);
    assert_eq!(
        completion.result.unwrap_err().kind(),
        ErrorKind::ConnectionRefused
    );
}

/// 閉じた相手への書きは、送りの buffer に入るうちは成功し、相手の reset が届いた後に失敗する。
#[cfg(unix)]
#[test]
fn a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (server, client) = connection(&mut driver, &mut stash);
    driver.close(server);
    let mut data = [7u8; 1024];
    for _ in 0..1000 {
        let write = driver.submit(Op::Write {
            handle: client,
            buf: raw(&mut data),
        });
        match complete(&mut driver, &mut stash, write).result {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    matches!(e.kind(), ErrorKind::BrokenPipe | ErrorKind::ConnectionReset),
                    "{e}"
                );
                return;
            }
        }
    }
    panic!("every write to a closed peer succeeded");
}

#[cfg(unix)]
#[test]
fn a_shutdown_write_gives_the_peer_eof() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (server, client) = connection(&mut driver, &mut stash);
    driver.shutdown_write(client).unwrap();
    let probe = driver.submit(Op::Readable { handle: server });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 8];
    let read = driver.submit(Op::Read {
        handle: server,
        buf: raw(&mut room),
    });
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(0)
    );
    assert_eq!(
        driver.shutdown_write(99).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[cfg(unix)]
#[test]
fn a_cancelled_accept_completes_with_an_error() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, _) = listen(&mut driver, &mut stash);
    let accept = driver.submit(Op::Accept { listener });
    driver.cancel(accept);
    let completion = complete(&mut driver, &mut stash, accept);
    assert!(completion.result.is_err());
    assert!(completion.cancelled);
    assert_eq!(driver.pending(), 0);
}

/// 長い書きは短く終わった送りを driver の中で出し直し、すべてを書いてから1つの完了で終わる。相手はスレッドで読む。
#[cfg(unix)]
#[test]
fn a_long_write_completes_once_with_its_whole_length() {
    use std::io::Read;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = driver.submit(Op::Accept { listener });
    let reader = std::thread::spawn(move || {
        let mut peer = std::net::TcpStream::connect(loopback(port)).unwrap();
        let mut all = Vec::new();
        peer.read_to_end(&mut all).unwrap();
        all
    });
    let (server, _) = stream(complete(&mut driver, &mut stash, accept));
    let mut data: Vec<u8> = (0..8 << 20).map(|i: u32| i as u8).collect();
    let write = driver.submit(Op::Write {
        handle: server,
        buf: raw(&mut data),
    });
    let completion = complete(&mut driver, &mut stash, write);
    assert_eq!(completion.result.unwrap(), Done::Size(8 << 20));
    assert_eq!(driver.pending(), 0);
    driver.completions(&mut stash);
    assert!(stash.is_empty());
    driver.close(server);
    // 閉じたハンドルは次の待ちで落ちる
    driver.wait(Some(0)).unwrap();
    assert!(reader.join().unwrap() == data);
}

/// 読まない相手への長い書きを取り消すと、出し直しをやめ、完了がちょうど1つ届く。
#[cfg(unix)]
#[test]
fn a_cancelled_long_write_stops_resubmitting() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = driver.submit(Op::Accept { listener });
    let _peer = std::net::TcpStream::connect(loopback(port)).unwrap();
    let (server, _) = stream(complete(&mut driver, &mut stash, accept));
    let mut data = vec![1u8; 8 << 20];
    let write = driver.submit(Op::Write {
        handle: server,
        buf: raw(&mut data),
    });
    driver
        .wait(Some(driver.now().as_millis() as u64 + 5))
        .unwrap();
    driver.completions(&mut stash);
    assert!(
        stash.is_empty(),
        "a write to a peer that does not read finished"
    );
    driver.cancel(write);
    let completion = complete(&mut driver, &mut stash, write);
    assert!(completion.result.is_err());
    assert!(completion.cancelled);
    assert_eq!(driver.pending(), 0);
    driver.completions(&mut stash);
    assert!(stash.is_empty());
}

/// `quiesce` は待っている受けを取り消し、捨てた完了が作ったストリームを閉じる。相手は EOF を読む。
#[cfg(unix)]
#[test]
fn quiesce_drains_a_pending_accept_and_closes_discarded_streams() {
    use std::io::Read;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, _) = listen(&mut driver, &mut stash);
    driver.submit(Op::Accept { listener });
    let server = std::net::TcpListener::bind(loopback(0)).unwrap();
    driver.submit(Op::Connect {
        host: "127.0.0.1".to_string(),
        port: server.local_addr().unwrap().port(),
    });
    let (mut peer, _) = server.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    let mut room = [0u8; 4];
    assert_eq!(peer.read(&mut room).unwrap(), 0);
}

/// テストの解決器が待つ門。`open` は通してよい解決の数である。
#[cfg(unix)]
struct Gate {
    open: std::sync::Mutex<usize>,
    turn: std::sync::Condvar,
    /// 始まった解決の名前。始まった順である。
    started: std::sync::Mutex<Vec<String>>,
}

#[cfg(unix)]
impl Gate {
    const fn new() -> Gate {
        Gate {
            open: std::sync::Mutex::new(0),
            turn: std::sync::Condvar::new(),
            started: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// `block` で始まる名前は門で待ち、ほかの名前はすぐに `127.0.0.1` を返す。
    fn lookup(&self, host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        if host.starts_with("block") {
            self.started.lock().unwrap().push(host.to_string());
            let mut open = self.open.lock().unwrap();
            while *open == 0 {
                open = self.turn.wait(open).unwrap();
            }
            *open -= 1;
        }
        Ok(vec![loopback(port)])
    }

    fn release(&self, n: usize) {
        *self.open.lock().unwrap() += n;
        self.turn.notify_all();
    }

    fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    /// `n` 個の解決が始まるまで待つ。1 s で始まらなければ落ちる。
    fn wait_started(&self, n: usize) {
        let start = Instant::now();
        while self.started.lock().unwrap().len() < n {
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "{:?}",
                self.started()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(unix)]
#[test]
fn a_connect_by_name_tries_the_looked_up_addresses_in_order() {
    static CLOSED: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    fn lookup(_host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        let closed = CLOSED.load(std::sync::atomic::Ordering::SeqCst);
        Ok(vec![loopback(closed), loopback(port)])
    }
    CLOSED.store(closed_port(), std::sync::atomic::Ordering::SeqCst);
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = driver.submit(Op::Accept { listener });
    let connect = driver.submit(Op::Connect {
        host: "two.test".to_string(),
        port,
    });
    let (_, peer) = stream(complete(&mut driver, &mut stash, connect));
    assert_eq!(peer, loopback(port));
    stream(complete(&mut driver, &mut stash, accept));
}

#[cfg(unix)]
#[test]
fn a_cancelled_lookup_completes_at_once() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let key = driver.submit(Op::Connect {
        host: "block".to_string(),
        port: 1,
    });
    assert_eq!(driver.pending(), 1);
    driver.cancel(key);
    let mut out = Vec::new();
    driver.completions(&mut out);
    GATE.release(1);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].key, key);
    assert_eq!(out[0].result.as_ref().unwrap_err().to_string(), "cancelled");
    assert!(out[0].cancelled);
    assert_eq!(driver.pending(), 0);
}

#[cfg(unix)]
#[test]
fn quiesce_does_not_wait_for_a_lookup() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    driver.submit(Op::Connect {
        host: "block".to_string(),
        port: 1,
    });
    GATE.wait_started(1);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    let mut out = Vec::new();
    driver.completions(&mut out);
    assert!(out.is_empty());
    GATE.release(1);
}

/// 走る解決は8つまでで、9つ目は前の解決のどれかが戻ってから始まる。
#[cfg(unix)]
#[test]
fn live_lookups_beyond_the_limit_wait_in_order() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    let keys: Vec<OpKey> = (0..9)
        .map(|i| {
            driver.submit(Op::Connect {
                host: format!("block-{i}"),
                port,
            })
        })
        .collect();
    assert_eq!(driver.pending(), 9);
    GATE.wait_started(8);
    std::thread::sleep(Duration::from_millis(20));
    driver.completions(&mut stash);
    assert!(!GATE.started().contains(&"block-8".to_string()));
    GATE.release(1);
    let start = Instant::now();
    while GATE.started().len() < 9 {
        assert!(start.elapsed() < Duration::from_secs(1));
        driver.wait(None).unwrap();
        driver.completions(&mut stash);
    }
    assert_eq!(GATE.started()[8], "block-8");
    GATE.release(8);
    for key in keys {
        stream(complete(&mut driver, &mut stash, key));
    }
    assert_eq!(driver.pending(), 0);
}

/// 手放した解決は走る解決の上限に数えないので、詰まった解決があっても新しい解決は進む。
#[cfg(unix)]
#[test]
fn abandoned_lookups_do_not_hold_the_limit() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    for i in 0..8 {
        let key = driver.submit(Op::Connect {
            host: format!("block-{i}"),
            port,
        });
        driver.cancel(key);
    }
    driver.completions(&mut stash);
    assert_eq!(stash.len(), 8);
    assert!(stash.iter().all(|c| c.cancelled));
    stash.clear();
    let key = driver.submit(Op::Connect {
        host: "fast".to_string(),
        port,
    });
    let (_, peer) = stream(complete(&mut driver, &mut stash, key));
    assert_eq!(peer, loopback(port));
    GATE.release(8);
}

/// 手放した解決が64たまっている間は、新しい解決はその場で失敗する。詰まった解決が戻れば、また解決できる。
#[cfg(unix)]
#[test]
fn a_lookup_fails_while_64_abandoned_lookups_are_stuck() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    for i in 0..64 {
        let key = driver.submit(Op::Connect {
            host: format!("block-{i}"),
            port,
        });
        driver.cancel(key);
    }
    driver.completions(&mut stash);
    assert_eq!(stash.len(), 64);
    stash.clear();
    let key = driver.submit(Op::Connect {
        host: "fast".to_string(),
        port,
    });
    let completion = complete(&mut driver, &mut stash, key);
    assert_eq!(
        completion.result.unwrap_err().to_string(),
        "too many name lookups are stuck"
    );
    assert!(!completion.cancelled);
    GATE.release(64);
    // 戻ったスレッドの結果は driver が次の受け付けで受け取るので、届くまで試し直す
    let start = Instant::now();
    loop {
        let key = driver.submit(Op::Connect {
            host: "fast".to_string(),
            port,
        });
        match complete(&mut driver, &mut stash, key).result {
            Ok(done) => {
                assert!(matches!(done, Done::Stream { .. }));
                break;
            }
            Err(e) => assert_eq!(e.to_string(), "too many name lookups are stuck"),
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(windows)]
#[test]
fn listen_accept_and_connect_are_unsupported() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let ops = [
        Op::Listen {
            addr: std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        },
        Op::Accept { listener: 0 },
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port: 80,
        },
    ];
    for op in ops {
        let key = driver.submit(op);
        let error = complete(&mut driver, &mut stash, key).result.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
    assert_eq!(
        driver.shutdown_write(0).unwrap_err().kind(),
        ErrorKind::Unsupported
    );
}
