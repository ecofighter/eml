//! 本物の driver の時計、待ち、プールの仕事、ストリーム。時間は下限だけを確かめる。

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use eml_io::RealDriver;
use eml_rt::{Completion, Done, IoDriver, Op, OpKey, RawBuf, Submit};

/// リセットされた端の読みと覗きの誤りの種類。OS の誤りの写し方は docs/spec/vm.md の「時計と driver」にある。
const RESET_READ: &[ErrorKind] = &[ErrorKind::ConnectionReset];

/// 閉じた相手かリセットした相手への書きの誤りの種類。unix は相手のリセットが届いた時点で `BrokenPipe` か
/// `ConnectionReset` になり、Windows は `ConnectionReset` になる。
const PEER_GONE_WRITE: &[ErrorKind] = &[ErrorKind::BrokenPipe, ErrorKind::ConnectionReset];

/// テストが振る鍵。1 から数え、どの driver にも同じ鍵を2度渡さない。
fn next_key() -> OpKey {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// 受け付けた操作。`submit` がその場で終えたら、その結果を持つ。
struct Started {
    key: OpKey,
    done: Option<std::io::Result<Done>>,
}

fn submit(driver: &mut RealDriver, op: Op) -> Started {
    let key = next_key();
    let done = match driver.submit(key, op) {
        Submit::Done(result) => Some(result),
        Submit::Pending => None,
    };
    Started { key, done }
}

/// 受け付けで終わらない操作を出し、その鍵を返す。
fn pending(driver: &mut RealDriver, op: Op) -> OpKey {
    let started = submit(driver, op);
    assert!(
        started.done.is_none(),
        "op {} finished at submit",
        started.key
    );
    started.key
}

/// 操作の完了。受け付けで終わっていればその結果を、そうでなければ `wait_for` で待った完了を返す。
fn complete(driver: &mut RealDriver, stash: &mut Vec<Completion>, op: Started) -> Completion {
    match op.done {
        Some(result) => Completion {
            key: op.key,
            result,
            cancelled: false,
        },
        None => wait_for(driver, stash, op.key),
    }
}

/// 操作を出して、終わった結果を返す。
fn run_op(driver: &mut RealDriver, stash: &mut Vec<Completion>, op: Op) -> std::io::Result<Done> {
    let started = submit(driver, op);
    complete(driver, stash, started).result
}

/// 保留の `key` の完了が届くまで待つ。ほかの操作の完了は `stash` に残す。1 s で届かなければ落ちる。
fn wait_for(driver: &mut RealDriver, stash: &mut Vec<Completion>, key: OpKey) -> Completion {
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
    // `wait` は早く戻ってよいので、期限に着くまで繰り返す
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

/// 保留の操作がある間に別のスレッドが waker を鳴らしても、`wait` は壊れずに戻る。期限のない待ちから起こしで戻る
/// ことは、保留のない待ちで `a_wake_ends_a_wait_with_a_far_deadline` が確かめる。
#[test]
fn a_wake_from_another_thread_leaves_a_wait_working() {
    let mut driver = RealDriver::new().unwrap();
    let (a, _b) = connection(&mut driver, &mut Vec::new());
    submit(&mut driver, Op::Readable { handle: a });
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

/// 走っている仕事を持つ driver は、落とすときに仕事の終わりを待たない。仕事は driver が消えた後も走り終える。
#[test]
fn dropping_a_driver_does_not_wait_for_a_running_job() {
    use std::sync::mpsc;
    let (started_tx, started) = mpsc::channel();
    let (gate, gate_rx) = mpsc::channel::<()>();
    let (end_tx, end) = mpsc::channel();
    let mut driver = RealDriver::new().unwrap();
    submit(
        &mut driver,
        Op::Job(Box::new(move || {
            started_tx.send(()).unwrap();
            // `drop` が待てば門は開かず、10 s の見張りで false を送る
            end_tx
                .send(gate_rx.recv_timeout(Duration::from_secs(10)).is_ok())
                .unwrap();
            Ok(0)
        })),
    );
    started.recv().unwrap();
    drop(driver);
    gate.send(()).unwrap();
    assert!(end.recv().unwrap());
}

#[test]
fn a_job_completes_with_its_value() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = submit(&mut driver, Op::Job(Box::new(|| Ok(42))));
    assert!(key.done.is_none());
    assert_eq!(driver.pending(), 1);
    let completion = complete(&mut driver, &mut stash, key);
    assert_eq!(completion.result.unwrap(), Done::Size(42));
    assert_eq!(driver.pending(), 0);
}

/// プールのスレッドは32本までで、それを超える仕事は driver の中の列で待ち、スレッドが空いてから動く。32 の仕事が
/// 門で止まっている間も、33 本目の `submit` は呼んだスレッドを止めずに戻る。
#[test]
fn a_job_pool_is_capped() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, mpsc};

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (started_tx, started) = mpsc::channel();
    let finished = Arc::new(AtomicUsize::new(0));
    let mut gates = Vec::new();
    let mut keys = Vec::new();
    for i in 0..32 {
        let started_tx = started_tx.clone();
        let finished = Arc::clone(&finished);
        let (gate, gate_rx) = mpsc::channel::<()>();
        gates.push(gate);
        keys.push(submit(
            &mut driver,
            Op::Job(Box::new(move || {
                started_tx.send(()).unwrap();
                // 門が開かなければ、10 s の見張りの後に誤りで終わる
                let opened = gate_rx.recv_timeout(Duration::from_secs(10)).is_ok();
                finished.fetch_add(1, Ordering::SeqCst);
                if opened {
                    Ok(i)
                } else {
                    Err(std::io::Error::other("the gate never opened"))
                }
            })),
        ));
    }
    for _ in 0..32 {
        started.recv().unwrap();
    }
    // 空いたスレッドがないので、33 本目は列で待つ。ここへ戻れたことが、`submit` が止まらない証拠である。
    // 列の仕事はほかの仕事が終わってからしか動かないので、動いたときには終わった仕事がある
    let seen = Arc::clone(&finished);
    let last = submit(
        &mut driver,
        Op::Job(Box::new(move || {
            Ok(if seen.load(Ordering::SeqCst) > 0 {
                32
            } else {
                usize::MAX
            })
        })),
    );
    assert!(last.done.is_none());
    keys.push(last);
    for gate in gates {
        gate.send(()).unwrap();
    }
    for (i, key) in keys.into_iter().enumerate() {
        assert_eq!(
            complete(&mut driver, &mut stash, key).result.unwrap(),
            Done::Size(i)
        );
    }
    assert_eq!(driver.pending(), 0);
}

#[test]
fn a_readable_probe_stays_pending_without_data() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let probe = submit(&mut driver, Op::Readable { handle: a });
    let deadline = driver.now().as_millis() as u64 + 20;
    while driver.now() < Duration::from_millis(deadline) {
        assert!(driver.wait(Some(deadline)).unwrap());
    }
    let mut out = Vec::new();
    driver.completions(&mut out);
    assert!(out.is_empty());
    assert_eq!(driver.pending(), 1);

    let mut byte = [7u8];
    let write = submit(
        &mut driver,
        Op::Write {
            handle: b,
            buf: raw(&mut byte),
        },
    );
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

/// 読めるデータのある端の `Readable` は受け付けで終わり、完了を残さない。データは読み進めないので、続く読みが読む。
/// 書いた直後に届いていることは OS が約束しないので、1つ目の `Readable` で届いたと分かってから2つ目を出す。
#[test]
fn a_probe_with_data_finishes_at_submit() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let mut data = *b"hi";
    let write = Op::Write {
        handle: b,
        buf: raw(&mut data),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, write).unwrap(),
        Done::Size(2)
    );
    let first = submit(&mut driver, Op::Readable { handle: a });
    complete(&mut driver, &mut stash, first).result.unwrap();
    let probe = driver.submit(next_key(), Op::Readable { handle: a });
    assert!(
        matches!(probe, Submit::Done(Ok(Done::Size(0)))),
        "{probe:?}"
    );
    driver.completions(&mut stash);
    assert!(stash.is_empty());
    assert_eq!(driver.pending(), 0);
    let mut room = [0u8; 8];
    let read = Op::Read {
        handle: a,
        buf: raw(&mut room),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, read).unwrap(),
        Done::Size(2)
    );
    assert_eq!(&room[..2], b"hi");
}

#[test]
fn a_read_after_a_probe_returns_the_bytes() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let probe = submit(&mut driver, Op::Readable { handle: a });
    let mut data = *b"hello";
    let write = submit(
        &mut driver,
        Op::Write {
            handle: b,
            buf: raw(&mut data),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(5)
    );
    assert_eq!(
        complete(&mut driver, &mut stash, probe).result.unwrap(),
        Done::Size(0)
    );
    let mut room = [0u8; 16];
    let read = submit(
        &mut driver,
        Op::Read {
            handle: a,
            buf: raw(&mut room),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(5)
    );
    assert_eq!(&room[..5], b"hello");
}

#[test]
fn a_write_is_read_by_the_peer() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let mut data = *b"from a";
    let write = submit(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(6)
    );
    let mut room = [0u8; 16];
    let read = submit(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(6)
    );
    assert_eq!(&room[..6], b"from a");
}

#[test]
fn a_cancelled_probe_completes_with_an_error() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, _b) = connection(&mut driver, &mut stash);
    let probe = pending(&mut driver, Op::Readable { handle: a });
    driver.cancel(probe);
    assert!(wait_for(&mut driver, &mut stash, probe).result.is_err());
    assert_eq!(driver.pending(), 0);
}

/// リセットされた端の `Readable` は、受け付けの覗きでリセットの誤りを返して終わる。覗きが誤りを消費するので、
/// 誤りを `Ok(0)` にすると次の読みが EOF に見える。閉じた直後にリセットが届いていることは OS が約束しないので、
/// 1つ目が保留なら、その完了で届いたと分かってから2つ目を出す。
#[test]
fn a_probe_on_a_reset_stream_fails_at_submit() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let listener = std::net::TcpListener::bind(loopback(0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let connect = submit(
        &mut driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port,
        },
    );
    let (a, _) = stream(complete(&mut driver, &mut stash, connect));
    let (peer, _) = listener.accept().unwrap();
    // linger 0 で閉じた端は、FIN でなく RST を送る
    reset_on_close(&peer);
    drop(peer);
    let first = submit(&mut driver, Op::Readable { handle: a });
    let probe = match first.done {
        Some(result) => result,
        // 完了型の backend は、待っている覗きの終わりでリセットを読みうる
        None => match wait_for(&mut driver, &mut stash, first.key).result {
            Err(e) => Err(e),
            Ok(_) => match driver.submit(next_key(), Op::Readable { handle: a }) {
                Submit::Done(result) => result,
                Submit::Pending => panic!("a probe on a reset stream stayed pending"),
            },
        },
    };
    match probe {
        Err(e) => assert!(RESET_READ.contains(&e.kind()), "{e}"),
        Ok(done) => panic!("a probe on a reset stream ended with {done:?}"),
    }
    driver.completions(&mut stash);
    assert!(stash.is_empty());
    assert_eq!(driver.pending(), 0);
}

#[test]
fn a_closed_peer_reads_eof() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let probe = submit(&mut driver, Op::Readable { handle: a });
    driver.close(b);
    assert_eq!(
        complete(&mut driver, &mut stash, probe).result.unwrap(),
        Done::Size(0)
    );
    let mut room = [0u8; 16];
    let read = submit(
        &mut driver,
        Op::Read {
            handle: a,
            buf: raw(&mut room),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(0)
    );
}

/// 相手が読まない端への書きは、送りの buffer が空くまで終わらない。`quiesce` は終わらない読みと書きを取り消し、
/// 走っている仕事を待ちきる。
#[test]
fn quiesce_drains_a_pending_probe_a_full_write_and_a_running_job() {
    let mut driver = RealDriver::new().unwrap();
    let (a, _b) = connection(&mut driver, &mut Vec::new());
    let probe = pending(&mut driver, Op::Readable { handle: a });
    let mut chunk = vec![1u8; PENDING_CHUNK];
    let full = write_until_pending(&mut driver, a, &mut chunk);
    let job = pending(
        &mut driver,
        Op::Job(Box::new(|| {
            std::thread::sleep(Duration::from_millis(30));
            Ok(0)
        })),
    );
    look(&mut driver);
    let mut out = Vec::new();
    driver.completions(&mut out);
    let finished: Vec<OpKey> = out.iter().map(|c| c.key).collect();
    for key in [probe, full, job] {
        assert!(!finished.contains(&key), "op {key} finished early");
    }
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    drop(chunk);
}

#[test]
fn an_op_on_an_unknown_handle_fails() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = submit(&mut driver, Op::Readable { handle: 99 });
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
    let key = submit(
        &mut driver,
        Op::Job(Box::new(|| {
            Ok(usize::from(blocked(libc::SIGINT) && blocked(libc::SIGTERM)))
        })),
    );
    assert!(!blocked(libc::SIGINT) && !blocked(libc::SIGTERM));
    assert_eq!(
        complete(&mut driver, &mut stash, key).result.unwrap(),
        Done::Size(1)
    );
}

/// `127.0.0.1` の `port`。本物のソケットのテストは綴りだけを使い、機械の `/etc/hosts` に頼らない。
fn loopback(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

/// `127.0.0.1` の空いているポートで待ち受け、リスナーとポートを返す。
fn listen(driver: &mut RealDriver, stash: &mut Vec<Completion>) -> (eml_rt::IoHandle, u16) {
    let key = submit(driver, Op::Listen { addr: loopback(0) });
    match complete(driver, stash, key).result.unwrap() {
        Done::Listener { handle, port } => (handle, port),
        other => panic!("a listen ended with {other:?}"),
    }
}

fn stream(completion: Completion) -> (eml_rt::IoHandle, std::net::SocketAddr) {
    match completion.result.unwrap() {
        Done::Stream { handle, peer } => (handle, peer),
        other => panic!("a stream op ended with {other:?}"),
    }
}

/// 待ち受けて、受けた端と接続した端を返す。
fn connection(
    driver: &mut RealDriver,
    stash: &mut Vec<Completion>,
) -> (eml_rt::IoHandle, eml_rt::IoHandle) {
    let (listener, port) = listen(driver, stash);
    let accept = submit(driver, Op::Accept { listener });
    let connect = submit(
        driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port,
        },
    );
    let (client, _) = stream(complete(driver, stash, connect));
    let (server, _) = stream(complete(driver, stash, accept));
    driver.close(listener);
    (server, client)
}

/// 待ち受ける者のいないポート。
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind(loopback(0)).unwrap();
    listener.local_addr().unwrap().port()
}

/// 受け入れの列が詰まった待ち受けと、列を詰めている接続。列が詰まると Linux も macOS も SYN を黙って捨てるので、
/// ここへの接続は拒まれもつながりもせず、進行中のまま残る。閉じたポートへの接続は、拒まれるのが速く、同じ `wait`
/// の中で終わることがあるので、待つ接続には使えない。どちらもテストの間は生かしておく。
/// Windows も、列が満ちた待ち受けへの `ConnectEx` を保留のまま残す(拒まない)。
fn full_listener() -> (std::net::TcpListener, Vec<std::net::TcpStream>) {
    let listener = std::net::TcpListener::bind(loopback(0)).unwrap();
    // std は backlog を選べないので、unix では待ち受けている socket に backlog 0 で `listen` を出し直す。Windows は
    // std の既定の列を詰める。0 の読み方は OS で違うので、列の長さは決め打ちせず、つながらない接続が出るまで詰める
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: `listener` が生きている socket の fd を持っている
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
    }
    let addr = listener.local_addr().unwrap();
    let mut fillers = Vec::new();
    loop {
        assert!(fillers.len() < 1024, "the accept queue never filled");
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)) {
            Ok(filler) => fillers.push(filler),
            Err(e) if e.kind() == ErrorKind::TimedOut => return (listener, fillers),
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn a_listener_on_port_zero_gets_a_port() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    assert_ne!(port, 0);
    assert!(std::net::TcpStream::connect(loopback(port)).is_ok());
}

#[test]
fn a_second_listener_on_the_same_port_is_addr_in_use() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    let key = submit(
        &mut driver,
        Op::Listen {
            addr: loopback(port),
        },
    );
    let error = complete(&mut driver, &mut stash, key).result.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
}

#[test]
fn an_accept_returns_the_connected_stream_and_its_peer() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = submit(&mut driver, Op::Accept { listener });
    let connect = submit(
        &mut driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port,
        },
    );
    let connected = complete(&mut driver, &mut stash, connect);
    assert!(!connected.cancelled);
    let (client, server_addr) = stream(connected);
    let (server, client_addr) = stream(complete(&mut driver, &mut stash, accept));
    assert_eq!(server_addr, loopback(port));
    assert!(client_addr.ip().is_loopback());
    assert_ne!(client_addr.port(), port);

    let mut data = *b"hi";
    let write = submit(
        &mut driver,
        Op::Write {
            handle: client,
            buf: raw(&mut data),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(2)
    );
    let probe = submit(&mut driver, Op::Readable { handle: server });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 8];
    let read = submit(
        &mut driver,
        Op::Read {
            handle: server,
            buf: raw(&mut room),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(2)
    );
    assert_eq!(&room[..2], b"hi");
    assert!(driver.set_nodelay(client, true).is_ok());
}

#[test]
fn a_connect_to_a_closed_port_is_refused() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = submit(
        &mut driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port: closed_port(),
        },
    );
    let completion = complete(&mut driver, &mut stash, key);
    assert!(!completion.cancelled);
    assert_eq!(
        completion.result.unwrap_err().kind(),
        ErrorKind::ConnectionRefused
    );
}

/// 閉じた相手への書きは、送りの buffer に入るうちは成功し、相手の reset が届いた後に失敗する。
#[test]
fn a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (server, client) = connection(&mut driver, &mut stash);
    driver.close(server);
    let mut data = [7u8; 1024];
    for _ in 0..1000 {
        let write = submit(
            &mut driver,
            Op::Write {
                handle: client,
                buf: raw(&mut data),
            },
        );
        match complete(&mut driver, &mut stash, write).result {
            Ok(_) => {}
            Err(e) => {
                assert!(PEER_GONE_WRITE.contains(&e.kind()), "{e}");
                return;
            }
        }
    }
    panic!("every write to a closed peer succeeded");
}

#[test]
fn a_shutdown_write_gives_the_peer_eof() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (server, client) = connection(&mut driver, &mut stash);
    driver.shutdown_write(client).unwrap();
    let probe = submit(&mut driver, Op::Readable { handle: server });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 8];
    let read = submit(
        &mut driver,
        Op::Read {
            handle: server,
            buf: raw(&mut room),
        },
    );
    assert_eq!(
        complete(&mut driver, &mut stash, read).result.unwrap(),
        Done::Size(0)
    );
    assert_eq!(
        driver.shutdown_write(99).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn a_cancelled_accept_completes_with_an_error() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, _) = listen(&mut driver, &mut stash);
    let accept = pending(&mut driver, Op::Accept { listener });
    driver.cancel(accept);
    let completion = wait_for(&mut driver, &mut stash, accept);
    assert!(completion.result.is_err());
    assert!(completion.cancelled);
    assert_eq!(driver.pending(), 0);
}

/// 長い書きは短く終わった送りを driver の中で出し直し、すべてを書いてから1つの完了で終わる。相手はスレッドで読む。
#[test]
fn a_long_write_completes_once_with_its_whole_length() {
    use std::io::Read;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = submit(&mut driver, Op::Accept { listener });
    let reader = std::thread::spawn(move || {
        let mut peer = std::net::TcpStream::connect(loopback(port)).unwrap();
        let mut all = Vec::new();
        peer.read_to_end(&mut all).unwrap();
        all
    });
    let (server, _) = stream(complete(&mut driver, &mut stash, accept));
    let mut data: Vec<u8> = (0..8 << 20).map(|i: u32| i as u8).collect();
    let write = submit(
        &mut driver,
        Op::Write {
            handle: server,
            buf: raw(&mut data),
        },
    );
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

/// 待っている長い書きを取り消すと、完了がちょうど1つ届き、その後には何も届かない。完了は、取り消しの誤り `cancelled`
/// (`cancelled == true`) か、`cancelled == false` で全長を書いた `Ok(Size(len))` のどちらか。取り消しは、すでに終わった
/// 書きには負けうる。Windows は取り消しが効く前に、相手が読んだ分の空きで書き全体を終えることがある。
/// 相手が少し読んで送りの buffer が空いた後、次の `wait` より前に取り消し、取り消しの完了を待つ。OS が取り消しの終わりを
/// 後で知らせる backend もあるので、1回の `wait` では足りない。
#[test]
fn a_cancelled_pending_long_write_completes_once() {
    use std::io::Read;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let accept = submit(&mut driver, Op::Accept { listener });
    let mut peer = std::net::TcpStream::connect(loopback(port)).unwrap();
    let (server, _) = stream(complete(&mut driver, &mut stash, accept));
    let mut data = vec![1u8; PENDING_CHUNK];
    let write = write_until_pending(&mut driver, server, &mut data);
    poll_for(&mut driver, 5);
    driver.completions(&mut stash);
    assert!(
        stash.is_empty(),
        "a write to a peer that does not read finished"
    );
    // 相手が少し読むと送りの buffer が空き、次の `wait` で待っている送りが進められる状態になる
    peer.set_nonblocking(true).unwrap();
    let mut room = vec![0u8; 1 << 20];
    let mut drained = 0;
    while drained < 1 << 20 {
        match peer.read(&mut room) {
            Ok(n) => drained += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => panic!("{e}"),
        }
    }
    assert!(drained > 0);
    driver.cancel(write);
    let completion = wait_for(&mut driver, &mut stash, write);
    assert!(stash.is_empty(), "{:?}", keys(&stash));
    match completion.result {
        Ok(done) => {
            assert_eq!(done, Done::Size(PENDING_CHUNK));
            assert!(!completion.cancelled);
        }
        Err(e) => {
            assert_eq!(e.to_string(), "cancelled");
            assert!(completion.cancelled);
        }
    }
    assert_eq!(driver.pending(), 0);
    driver.completions(&mut stash);
    assert!(stash.is_empty());
}

/// 名前の接続の最初の番地の試行が終わる前に取り消すと、次の番地を試さずに終わる。
#[test]
fn a_cancelled_connect_does_not_try_the_next_address() {
    use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

    static FULL: AtomicU16 = AtomicU16::new(0);
    static LOOKED_UP: AtomicBool = AtomicBool::new(false);
    fn lookup(_host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        LOOKED_UP.store(true, Ordering::SeqCst);
        Ok(vec![loopback(FULL.load(Ordering::SeqCst)), loopback(port)])
    }
    let (full, _fillers) = full_listener();
    FULL.store(full.local_addr().unwrap().port(), Ordering::SeqCst);
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let mut stash = Vec::new();
    let (_, port) = listen(&mut driver, &mut stash);
    let connect = pending(
        &mut driver,
        Op::Connect {
            host: "two.test".to_string(),
            port,
        },
    );
    // 解決の結果が届くのを待ち、1回の `wait` で受け取って最初の番地の試行を出す
    let start = Instant::now();
    while !LOOKED_UP.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(Duration::from_millis(5));
    look(&mut driver);
    driver.completions(&mut stash);
    assert!(stash.is_empty());
    driver.cancel(connect);
    look(&mut driver);
    driver.completions(&mut stash);
    assert_eq!(keys(&stash), [connect]);
    let completion = stash.pop().unwrap();
    assert_eq!(completion.result.unwrap_err().to_string(), "cancelled");
    assert!(completion.cancelled);
    assert_eq!(driver.pending(), 0);
}

/// 完了を受け取らずに `ms` ミリ秒だけ待つ。その間に終わった操作は、次の `completions` まで残る。
fn poll_for(driver: &mut RealDriver, ms: u64) {
    let deadline = driver.now().as_millis() as u64 + ms;
    while driver.now() < Duration::from_millis(deadline) {
        driver.wait(Some(deadline)).unwrap();
    }
}

/// 期限の過ぎた `wait` で、待たずに OS を1回見る。
fn look(driver: &mut RealDriver) {
    let now = driver.now().as_millis() as u64;
    assert!(driver.wait(Some(now)).unwrap());
}

fn keys(completions: &[Completion]) -> Vec<OpKey> {
    completions.iter().map(|c| c.key).collect()
}

/// `quiesce` は待っている受けを取り消し、捨てた完了が作ったストリームを閉じる。相手は EOF を読む。
#[test]
fn quiesce_drains_a_pending_accept_and_closes_discarded_streams() {
    use std::io::Read;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, _) = listen(&mut driver, &mut stash);
    submit(&mut driver, Op::Accept { listener });
    let server = std::net::TcpListener::bind(loopback(0)).unwrap();
    submit(
        &mut driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port: server.local_addr().unwrap().port(),
        },
    );
    let (mut peer, _) = server.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    // 接続を driver の中で終わらせ、完了は受け取らない。`quiesce` はその完了を捨て、中身のストリームを閉じる
    poll_for(&mut driver, 20);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    let mut room = [0u8; 4];
    assert_eq!(peer.read(&mut room).unwrap(), 0);
}

/// テストの解決器が待つ門。`open` は通してよい解決の数である。
struct Gate {
    open: std::sync::Mutex<usize>,
    turn: std::sync::Condvar,
    /// 始まった解決の名前。始まった順である。
    started: std::sync::Mutex<Vec<String>>,
}

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
    let accept = submit(&mut driver, Op::Accept { listener });
    let connect = submit(
        &mut driver,
        Op::Connect {
            host: "two.test".to_string(),
            port,
        },
    );
    let (_, peer) = stream(complete(&mut driver, &mut stash, connect));
    assert_eq!(peer, loopback(port));
    stream(complete(&mut driver, &mut stash, accept));
}

#[test]
fn a_cancelled_lookup_completes_at_once() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    let key = pending(
        &mut driver,
        Op::Connect {
            host: "block".to_string(),
            port: 1,
        },
    );
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

#[test]
fn quiesce_does_not_wait_for_a_lookup() {
    static GATE: Gate = Gate::new();
    fn lookup(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
        GATE.lookup(host, port)
    }
    let mut driver = RealDriver::with_lookup(lookup).unwrap();
    submit(
        &mut driver,
        Op::Connect {
            host: "block".to_string(),
            port: 1,
        },
    );
    GATE.wait_started(1);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    let mut out = Vec::new();
    driver.completions(&mut out);
    assert!(out.is_empty());
    GATE.release(1);
}

/// 走る解決は8つまでで、9つ目は前の解決のどれかが戻ってから始まる。
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
            pending(
                &mut driver,
                Op::Connect {
                    host: format!("block-{i}"),
                    port,
                },
            )
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
        // 9つ目の解決のスレッドが始まっても driver を起こす者はいないので、期限で戻る待ちにする
        poll_for(&mut driver, 5);
        driver.completions(&mut stash);
    }
    assert_eq!(GATE.started()[8], "block-8");
    GATE.release(8);
    for key in keys {
        stream(wait_for(&mut driver, &mut stash, key));
    }
    assert_eq!(driver.pending(), 0);
}

/// 手放した解決は走る解決の上限に数えないので、詰まった解決があっても新しい解決は進む。
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
        let key = pending(
            &mut driver,
            Op::Connect {
                host: format!("block-{i}"),
                port,
            },
        );
        driver.cancel(key);
    }
    driver.completions(&mut stash);
    assert_eq!(stash.len(), 8);
    assert!(stash.iter().all(|c| c.cancelled));
    stash.clear();
    let key = submit(
        &mut driver,
        Op::Connect {
            host: "fast".to_string(),
            port,
        },
    );
    let (_, peer) = stream(complete(&mut driver, &mut stash, key));
    assert_eq!(peer, loopback(port));
    GATE.release(8);
}

/// 手放した解決が64たまっている間は、新しい解決はその場で失敗する。詰まった解決が戻れば、また解決できる。
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
        let key = pending(
            &mut driver,
            Op::Connect {
                host: format!("block-{i}"),
                port,
            },
        );
        driver.cancel(key);
    }
    driver.completions(&mut stash);
    assert_eq!(stash.len(), 64);
    stash.clear();
    let key = submit(
        &mut driver,
        Op::Connect {
            host: "fast".to_string(),
            port,
        },
    );
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
        let key = submit(
            &mut driver,
            Op::Connect {
                host: "fast".to_string(),
                port,
            },
        );
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

/// `Readable` が終わった後の読みはその場で読めるので、`Submit::Done` で返り、完了を残さない。書いた直後の読みが
/// その場で終わることは OS が約束しないので、読めると分かってから読む。
#[test]
fn a_read_after_a_finished_probe_returns_done() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let mut data = *b"hello";
    let write = Op::Write {
        handle: b,
        buf: raw(&mut data),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, write).unwrap(),
        Done::Size(5)
    );
    let probe = submit(&mut driver, Op::Readable { handle: a });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 16];
    let read = driver.submit(
        next_key(),
        Op::Read {
            handle: a,
            buf: raw(&mut room),
        },
    );
    assert!(matches!(read, Submit::Done(Ok(Done::Size(5)))), "{read:?}");
    driver.completions(&mut stash);
    assert!(stash.is_empty());
    assert_eq!(driver.pending(), 0);
    assert_eq!(&room[..5], b"hello");
}

/// 保留のない遠い期限の `wait` は、別のスレッドの起こしで期限より前に戻る。実時間の上限は比べない。
#[test]
fn a_wake_ends_a_wait_with_a_far_deadline() {
    let mut driver = RealDriver::new().unwrap();
    let deadline = driver.now().as_millis() as u64 + 5000;
    let waker = driver.waker();
    let ringer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        waker.wake();
    });
    assert!(driver.wait(Some(deadline)).unwrap());
    assert!(driver.now() < Duration::from_millis(deadline));
    ringer.join().unwrap();
}

/// 期限 `u64::MAX` は期限なしの待ちだが、保留がなくても `Ok(false)` を返さず、起こしで戻る。
#[test]
fn a_wait_with_no_ops_and_a_far_deadline_wakes() {
    let mut driver = RealDriver::new().unwrap();
    let waker = driver.waker();
    let ringer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        waker.wake();
    });
    assert!(driver.wait(Some(u64::MAX)).unwrap());
    ringer.join().unwrap();
}

/// driver は刻みを持たないので、保留のない `wait` は期限まで眠る。時間は下限だけを比べる。
#[test]
fn a_wait_without_ops_sleeps_until_its_deadline() {
    let mut driver = RealDriver::new().unwrap();
    let deadline = driver.now().as_millis() as u64 + 200;
    let start = Instant::now();
    assert!(driver.wait(Some(deadline)).unwrap());
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(150), "{elapsed:?}");
}

/// 完了を受け取った後の取り消しは、同じ鍵で後から出した操作に効かない。
#[test]
fn a_cancel_after_completion_does_not_touch_the_next_op_of_the_key() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, b) = connection(&mut driver, &mut stash);
    let mut data = *b"x";
    let write = Op::Write {
        handle: b,
        buf: raw(&mut data),
    };
    run_op(&mut driver, &mut stash, write).unwrap();
    let key = next_key();
    let probe = match driver.submit(key, Op::Readable { handle: a }) {
        Submit::Done(result) => result,
        Submit::Pending => wait_for(&mut driver, &mut stash, key).result,
    };
    probe.unwrap();
    let mut room = [0u8; 4];
    let drain = Op::Read {
        handle: a,
        buf: raw(&mut room),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, drain).unwrap(),
        Done::Size(1)
    );
    driver.cancel(key);
    let again = driver.submit(
        key,
        Op::Read {
            handle: a,
            buf: raw(&mut room),
        },
    );
    assert!(matches!(again, Submit::Pending), "{again:?}");
    let mut more = *b"yz";
    let write = Op::Write {
        handle: b,
        buf: raw(&mut more),
    };
    run_op(&mut driver, &mut stash, write).unwrap();
    let completion = wait_for(&mut driver, &mut stash, key);
    assert!(!completion.cancelled);
    assert_eq!(completion.result.unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"yz");
}

/// 受け付けで終わる操作だけが続いても、落とされたハンドルは次の `submit` で閉じる。`wait` も `completions` も
/// 呼ばない run で、落としたリスナーとストリームの fd が積もらないためである。
#[test]
fn a_submit_closes_dropped_handles() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    driver.bin().dropped.lock().unwrap().push(listener);
    let again = submit(&mut driver, Op::Listen { addr: loopback(0) });
    assert!(again.done.is_some());
    // std の接続は Windows で拒まれた SYN を送り直して遅いので、ループバックで送り直さない driver で接続する
    let connect = Op::Connect {
        host: "127.0.0.1".to_string(),
        port,
    };
    let error = run_op(&mut driver, &mut stash, connect).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::ConnectionRefused);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a pending key is submitted again")]
fn a_pending_key_submitted_twice_panics_in_debug() {
    let mut driver = RealDriver::new().unwrap();
    let key = pending(&mut driver, Op::Job(Box::new(|| Ok(0))));
    driver.submit(key, Op::Job(Box::new(|| Ok(0))));
}

/// 閉じたときに FIN でなく RST を送るよう、`s` に linger 0 を立てる。std の `TcpStream::set_linger` は安定していない。
#[cfg(unix)]
fn reset_on_close(s: &std::net::TcpStream) {
    use std::os::fd::AsRawFd;

    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: `s` の fd は呼び出しの間開いている。`linger` はこの関数の局所変数で、その大きさを長さとして渡す
    let r = unsafe {
        libc::setsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            (&raw const linger).cast(),
            std::mem::size_of::<libc::linger>() as libc::socklen_t,
        )
    };
    assert_eq!(r, 0, "{}", std::io::Error::last_os_error());
}

#[cfg(windows)]
fn reset_on_close(s: &std::net::TcpStream) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        LINGER, SO_LINGER, SOL_SOCKET, WSAGetLastError, setsockopt,
    };

    let linger = LINGER {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: `s` の socket は呼び出しの間開いている。`linger` はこの関数の局所変数で、その大きさを長さとして渡す
    let r = unsafe {
        setsockopt(
            s.as_raw_socket() as usize,
            SOL_SOCKET,
            SO_LINGER,
            (&raw const linger).cast(),
            std::mem::size_of::<LINGER>() as i32,
        )
    };
    assert_eq!(
        r,
        0,
        "{}",
        std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
    );
}

/// 相手が読まない端 `handle` に、`chunk` の書きを保留になるまで出し、保留の書きの鍵を返す。送りと受けの buffer の和は
/// 有限なのでどこかで保留になり、その大きさには頼らない。`WRITES_UNTIL_PENDING` 回で保留にならなければ落ちる。
/// 保留の書きが終わるまで、呼ぶ側が `chunk` を生かしておく。macOS は相手の窓が満ちた後にも送りの buffer を広げうるので、小さな `chunk` の保留の
/// 書きは、相手が読まなくても少し後に終わりうる。保留のまま残ってほしいテストは `PENDING_CHUNK` を渡す。
/// 保留の書きの残りが、広がった送りの buffer にも収まらない大きさ。
const PENDING_CHUNK: usize = 8 << 20;

/// `write_until_pending` が出す書きの数の上限。unix では1つ目が保留になる。Windows の送りは buffer に空きがあれば
/// 大きな書きを丸ごと受けうるので余裕を持たせた。Windows で測ったときは、3つ目の 8 MiB の書きが初めてブロックした。
const WRITES_UNTIL_PENDING: usize = 8;

fn write_until_pending(
    driver: &mut RealDriver,
    handle: eml_rt::IoHandle,
    chunk: &mut [u8],
) -> OpKey {
    for _ in 0..WRITES_UNTIL_PENDING {
        let key = next_key();
        match driver.submit(
            key,
            Op::Write {
                handle,
                buf: raw(chunk),
            },
        ) {
            Submit::Done(result) => {
                result.unwrap();
            }
            Submit::Pending => return key,
        }
    }
    panic!("{WRITES_UNTIL_PENDING} writes to a peer that does not read never went pending");
}

/// std のリスナーへ driver で接続し、driver の端と std の相手を返す。
fn connect_to_std(
    driver: &mut RealDriver,
    stash: &mut Vec<Completion>,
) -> (eml_rt::IoHandle, std::net::TcpStream) {
    let listener = std::net::TcpListener::bind(loopback(0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let connect = submit(
        driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port,
        },
    );
    let (a, _) = stream(complete(driver, stash, connect));
    let (peer, _) = listener.accept().unwrap();
    (a, peer)
}

/// 相手が接続して書いた後に受けたストリームの `Readable` は、受けの前に届いていたデータで終わる。受けた後に新しい
/// データが来なくても、登録の時点で読めることが分かる。
#[test]
fn a_probe_on_a_stream_that_had_data_before_accept_finishes() {
    use std::io::Write;

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let mut peer = std::net::TcpStream::connect(loopback(port)).unwrap();
    peer.write_all(b"x").unwrap();
    let accept = submit(&mut driver, Op::Accept { listener });
    let (server, _) = stream(complete(&mut driver, &mut stash, accept));
    let probe = submit(&mut driver, Op::Readable { handle: server });
    complete(&mut driver, &mut stash, probe).result.unwrap();
    let mut room = [0u8; 4];
    let read = Op::Read {
        handle: server,
        buf: raw(&mut room),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, read).unwrap(),
        Done::Size(1)
    );
}

/// 1つのストリームで、待つ `Readable` と待つ書きを同時に持てる。相手が書いて読めば、両方が終わる。待ち手は向きごとに
/// 1つずつある。
#[test]
fn a_pending_read_and_a_pending_write_on_one_stream_both_finish() {
    use std::io::{Read, Write};

    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, mut peer) = connect_to_std(&mut driver, &mut stash);
    let mut chunk = vec![5u8; PENDING_CHUNK];
    let write = write_until_pending(&mut driver, a, &mut chunk);
    let probe = pending(&mut driver, Op::Readable { handle: a });
    peer.write_all(b"x").unwrap();
    // 相手は EOF まで読み続け、送りの buffer を空ける
    let reader = std::thread::spawn(move || {
        let mut sink = Vec::new();
        peer.read_to_end(&mut sink).unwrap();
    });
    wait_for(&mut driver, &mut stash, probe).result.unwrap();
    assert_eq!(
        wait_for(&mut driver, &mut stash, write).result.unwrap(),
        Done::Size(chunk.len())
    );
    // 読まないデータを残して閉じると、相手には EOF でなくリセットが届く
    let mut room = [0u8; 4];
    let read = Op::Read {
        handle: a,
        buf: raw(&mut room),
    };
    assert_eq!(
        run_op(&mut driver, &mut stash, read).unwrap(),
        Done::Size(1)
    );
    driver.close(a);
    reader.join().unwrap();
    drop(chunk);
}

/// 2本つないでから受けを1つずつ2回出すと、どちらも終わる。2本が1つの event で知らされても、受けは記憶によらずに
/// 試す。
#[test]
fn queued_connections_are_accepted_one_per_op() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (listener, port) = listen(&mut driver, &mut stash);
    let _first = std::net::TcpStream::connect(loopback(port)).unwrap();
    let _second = std::net::TcpStream::connect(loopback(port)).unwrap();
    for _ in 0..2 {
        let accept = submit(&mut driver, Op::Accept { listener });
        stream(complete(&mut driver, &mut stash, accept));
    }
    assert_eq!(driver.pending(), 0);
}

/// 閉じたポートへの接続は `ConnectionRefused` で1回だけ終わる。再試行で `connect` を出し直さないので、後から
/// 完了が来ない。
#[test]
fn a_refused_connect_completes_once() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let key = submit(
        &mut driver,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port: closed_port(),
        },
    );
    let completion = complete(&mut driver, &mut stash, key);
    assert_eq!(
        completion.result.unwrap_err().kind(),
        ErrorKind::ConnectionRefused
    );
    assert_eq!(driver.pending(), 0);
    poll_for(&mut driver, 20);
    driver.completions(&mut stash);
    assert!(stash.is_empty(), "{:?}", keys(&stash));
}

/// 相手がリセットすると、送りの buffer が空くのを待っていた書きは誤りで終わる。
#[test]
fn a_reset_ends_a_pending_write_with_an_error() {
    let mut driver = RealDriver::new().unwrap();
    let mut stash = Vec::new();
    let (a, peer) = connect_to_std(&mut driver, &mut stash);
    let mut chunk = vec![6u8; PENDING_CHUNK];
    let write = write_until_pending(&mut driver, a, &mut chunk);
    reset_on_close(&peer);
    drop(peer);
    let error = wait_for(&mut driver, &mut stash, write).result.unwrap_err();
    assert!(PEER_GONE_WRITE.contains(&error.kind()), "{error}");
    drop(chunk);
}

/// シグナルで OS の待ちが割り込まれても、`wait` は壊れずに戻り、driver はそのまま使える。`SIGUSR1` の handler は何も
/// しない関数で、プロセスに残しても害がない。戻すと、遅れて届いた1つが既定の動き (プロセスの終わり) になりうる。
#[cfg(unix)]
#[test]
fn a_signal_during_a_wait_leaves_the_driver_working() {
    use std::os::unix::thread::JoinHandleExt;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    extern "C" fn ignore(_: libc::c_int) {}
    // SAFETY: `action` はこの関数の局所変数で、handler は何もしない `extern "C"` の関数である。`sa_flags` が 0 なので
    // 割り込まれた呼び出しは `EINTR` で戻る
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = ignore as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        assert_eq!(
            libc::sigaction(libc::SIGUSR1, &action, std::ptr::null_mut()),
            0
        );
    }
    let waited = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready) = mpsc::channel();
    let flag = Arc::clone(&waited);
    let waiter = std::thread::spawn(move || {
        let mut driver = RealDriver::new().unwrap();
        let mut stash = Vec::new();
        let (a, _b) = connection(&mut driver, &mut stash);
        let probe = pending(&mut driver, Op::Readable { handle: a });
        ready_tx.send(()).unwrap();
        assert!(driver.wait(Some(u64::MAX)).unwrap());
        flag.store(true, Ordering::SeqCst);
        driver.cancel(probe);
        assert!(wait_for(&mut driver, &mut stash, probe).cancelled);
        let job = submit(&mut driver, Op::Job(Box::new(|| Ok(3))));
        assert_eq!(
            complete(&mut driver, &mut stash, job).result.unwrap(),
            Done::Size(3)
        );
    });
    ready.recv().unwrap();
    // 待ちに入る前に届いたシグナルは待ちを割り込まないので、待ちが戻るまで叩き続ける
    while !waited.load(Ordering::SeqCst) && !waiter.is_finished() {
        // SAFETY: スレッドはまだ join していないので、その `pthread_t` は有効である
        let r = unsafe { libc::pthread_kill(waiter.as_pthread_t(), libc::SIGUSR1) };
        // 確かめた後にスレッドが抜けていれば、OS によっては ESRCH を返す
        assert!(r == 0 || r == libc::ESRCH, "pthread_kill: {r}");
        std::thread::sleep(Duration::from_millis(5));
    }
    waiter.join().unwrap();
}

/// driver を落とした後に waker を鳴らしても害がない。waker は鳴らす OS の物体を自分で持つ。
#[test]
fn a_wake_after_the_driver_drops_is_harmless() {
    let driver = RealDriver::new().unwrap();
    let waker = driver.waker();
    drop(driver);
    std::thread::spawn(move || {
        waker.wake();
        waker.wake();
    })
    .join()
    .unwrap();
}

/// 待つ前に何度鳴らしても、終わるのは次の待ち1つだけである。続く待ちは期限まで眠る。時間は下限だけを比べる。
#[test]
fn wakes_before_a_wait_end_only_that_wait() {
    let mut driver = RealDriver::new().unwrap();
    let waker = driver.waker();
    for _ in 0..100 {
        waker.wake();
    }
    assert!(driver.wait(Some(u64::MAX)).unwrap());
    let deadline = driver.now().as_millis() as u64 + 100;
    let start = Instant::now();
    assert!(driver.wait(Some(deadline)).unwrap());
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(75), "{elapsed:?}");
}
