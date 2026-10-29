//! テストの driver の性質。時計は終わる操作がないときだけ進み、受け付けで決まる操作はその場で終わり、`wait` の
//! 決着の組は seed で順だけが変わる。

use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use eml_io::TestDriver;
use eml_rt::{Completion, Done, IoDriver, IoHandle, Op, OpKey, RawBuf, Submit};

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

/// 受け付けで決まらない操作を出し、その鍵を返す。
fn pending(driver: &mut TestDriver, op: Op) -> OpKey {
    let key = next_key();
    match driver.submit(key, op) {
        Submit::Pending => key,
        Submit::Done(result) => panic!("op {key} finished at submit with {result:?}"),
    }
}

fn collect(driver: &mut TestDriver) -> Vec<Completion> {
    let mut out = Vec::new();
    driver.completions(&mut out);
    out
}

/// 時計を進めずに決着させ、届いた完了を受け取る。
fn settle(driver: &mut TestDriver) -> Vec<Completion> {
    let now = driver.now().as_millis() as u64;
    assert!(driver.wait(Some(now)).unwrap());
    collect(driver)
}

fn keys(completions: &[Completion]) -> Vec<OpKey> {
    completions.iter().map(|c| c.key).collect()
}

#[test]
fn a_virtual_clock_advances_only_forward() {
    let mut driver = TestDriver::new(None);
    assert_eq!(driver.now(), Duration::ZERO);
    assert!(driver.wait(Some(5)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(5));
    assert!(driver.wait(Some(3)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(5));
}

#[test]
fn a_wait_without_work_or_deadline_cannot_progress() {
    let mut driver = TestDriver::new(None);
    assert!(!driver.wait(None).unwrap());
}

/// 終わる操作があれば時計は止まったままで、なければ期限ちょうどに進む。
#[test]
fn a_wait_advances_the_clock_only_when_no_op_finishes() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let probe = pending(&mut driver, Op::Readable { handle: b });
    assert!(driver.wait(Some(10)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(10));
    assert_eq!(driver.pending(), 1);

    let mut data = *b"hi";
    let write = run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    );
    assert_eq!(write.unwrap(), Done::Size(2));
    assert!(driver.wait(Some(20)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(10));
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert!(done.iter().all(|c| c.result.is_ok()));

    let mut room = [0u8; 8];
    let read = run_op(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert_eq!(read.unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"hi");
}

/// seed は1回の決着で終わった操作の順だけを変え、組の中身は変えない。受け付けで決まる操作は決着の組に入らないので、
/// 組は受け付けで決まらない操作で作る。
#[test]
fn a_seed_does_not_change_which_ops_finish() {
    let run = |seed| {
        let mut driver = TestDriver::new(seed);
        let ends: Vec<(IoHandle, IoHandle)> = (0..3).map(|_| connection(&mut driver)).collect();
        let (listener, port) = listen(&mut driver, 0).unwrap();
        let mut waiting: Vec<OpKey> = ends
            .iter()
            .map(|&(_, b)| pending(&mut driver, Op::Readable { handle: b }))
            .collect();
        waiting.push(pending(&mut driver, Op::Accept { listener }));
        let mut data = *b"x";
        for &(a, _) in &ends {
            let write = run_op(
                &mut driver,
                Op::Write {
                    handle: a,
                    buf: raw(&mut data),
                },
            );
            assert_eq!(write.unwrap(), Done::Size(1));
        }
        stream(run_op(&mut driver, connect("127.0.0.1", port)));
        // 鍵は run ごとに違うので、受け付けの順の番号で比べる
        let order: Vec<usize> = settle(&mut driver)
            .iter()
            .map(|c| waiting.iter().position(|&k| k == c.key).unwrap())
            .collect();
        let mut set = order.clone();
        set.sort_unstable();
        (set, order, driver.pending())
    };
    let (plain, plain_order, plain_pending) = run(None);
    assert_eq!(plain, [0, 1, 2, 3]);
    assert_eq!(plain, plain_order);
    assert_eq!(plain_pending, 0);
    let mut shuffled = false;
    for seed in 1..=3 {
        let (set, order, pending) = run(Some(seed));
        assert_eq!(set, plain);
        assert_eq!(pending, plain_pending);
        shuffled |= order != plain_order;
    }
    assert!(shuffled);
}

/// 決着は bin を先に空にするので、同じ決着の読みは相手が閉じた後の状態を見る。受け付けも同じである。
#[test]
fn the_bin_is_emptied_before_ops_settle() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let probe = pending(&mut driver, Op::Readable { handle: b });
    assert!(settle(&mut driver).is_empty());
    driver.bin().dropped.lock().unwrap().push(a);
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(0));

    let mut room = [0u8; 4];
    let read = run_op(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert_eq!(read.unwrap(), Done::Size(0));
}

/// 相手が閉じた後も、残りを読みきってから EOF を受ける。
#[test]
fn a_closed_peer_leaves_its_bytes_before_eof() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let mut data = *b"abc";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    )
    .unwrap();
    driver.close(a);
    let mut room = [0u8; 2];
    for expected in [2, 1, 0] {
        let read = run_op(
            &mut driver,
            Op::Read {
                handle: b,
                buf: raw(&mut room),
            },
        );
        assert_eq!(read.unwrap(), Done::Size(expected));
    }
}

#[test]
fn a_write_to_a_closed_peer_is_a_broken_pipe() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    driver.close(b);
    let mut data = *b"x";
    let write = run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    );
    assert_eq!(write.unwrap_err().kind(), ErrorKind::BrokenPipe);
}

/// 返す前に取り消した操作は次の決着で誤りとして終わり、返した後の取り消しは何もしない。
#[test]
fn a_cancel_before_return_ends_the_op_at_the_next_settle() {
    let mut driver = TestDriver::new(None);
    let (_, b) = connection(&mut driver);
    let probe = pending(&mut driver, Op::Readable { handle: b });
    assert!(settle(&mut driver).is_empty());
    driver.cancel(probe);
    assert_eq!(driver.pending(), 1);
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "cancelled"
    );
    driver.cancel(probe);
    assert!(settle(&mut driver).is_empty());
    assert_eq!(driver.pending(), 0);
}

/// すでに終わってまだ渡していない操作は、取り消しても終わったときの結果のまま渡す。
#[test]
fn a_cancel_after_the_op_finished_keeps_its_result() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let probe = pending(&mut driver, Op::Readable { handle: b });
    let mut data = *b"abc";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    )
    .unwrap();
    assert!(driver.wait(Some(5)).unwrap());
    driver.cancel(probe);
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(0));
    assert!(!done[0].cancelled);
}

/// 保留の操作は取り消して捨てるが、仕事は取り消せないので動かしきる。
#[test]
fn quiesce_drops_every_pending_op_and_runs_jobs() {
    let mut driver = TestDriver::new(None);
    let (_, b) = connection(&mut driver);
    let ran = Arc::new(AtomicBool::new(false));
    pending(&mut driver, Op::Readable { handle: b });
    let flag = Arc::clone(&ran);
    pending(
        &mut driver,
        Op::Job(Box::new(move || {
            flag.store(true, Ordering::Relaxed);
            Ok(0)
        })),
    );
    driver.quiesce();
    assert!(ran.load(Ordering::Relaxed));
    assert_eq!(driver.pending(), 0);
    assert!(collect(&mut driver).is_empty());
}

/// `completions` は決着させない。相手が書いた後の保留の読みも、`wait` が決着させるまで届かない。
#[test]
fn completions_do_not_settle() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let mut room = [0u8; 4];
    let read = pending(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    let mut data = *b"hi";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    )
    .unwrap();
    for _ in 0..3 {
        assert!(collect(&mut driver).is_empty());
    }
    let before = driver.now();
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [read]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(2));
    assert_eq!(driver.now(), before);
}

/// 受け付けで決まる操作は `Submit::Done` で返り、完了を残さず、保留にも数えない。
#[test]
fn an_op_decided_at_submit_returns_done_and_leaves_no_completion() {
    let mut driver = TestDriver::new(None);
    let (a, _b) = connection(&mut driver);
    let before = driver.immediate();
    let mut data = *b"abc";
    let submitted = driver.submit(
        next_key(),
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    );
    assert!(
        matches!(submitted, Submit::Done(Ok(Done::Size(3)))),
        "{submitted:?}"
    );
    assert!(collect(&mut driver).is_empty());
    assert_eq!(driver.pending(), 0);
    assert_eq!(driver.immediate(), before + 1);
}

/// データのない読みは保留になり、相手が書いた後の `wait` で終わる。
#[test]
fn a_read_without_data_stays_pending_until_a_write() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let mut room = [0u8; 4];
    let read = pending(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert!(settle(&mut driver).is_empty());
    let mut data = *b"ok";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    )
    .unwrap();
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [read]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"ok");
}

/// 完了を受け取った後の取り消しは、同じ鍵で後から出した操作に効かない。
#[test]
fn a_cancel_after_completion_does_not_touch_the_next_op_of_the_key() {
    let mut driver = TestDriver::new(None);
    let (a, b) = connection(&mut driver);
    let key = pending(&mut driver, Op::Readable { handle: b });
    let mut data = *b"x";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut data),
        },
    )
    .unwrap();
    assert_eq!(keys(&settle(&mut driver)), [key]);
    let mut room = [0u8; 4];
    let drained = run_op(
        &mut driver,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert_eq!(drained.unwrap(), Done::Size(1));
    driver.cancel(key);
    let again = driver.submit(
        key,
        Op::Read {
            handle: b,
            buf: raw(&mut room),
        },
    );
    assert!(matches!(again, Submit::Pending), "{again:?}");
    let mut more = *b"yz";
    run_op(
        &mut driver,
        Op::Write {
            handle: a,
            buf: raw(&mut more),
        },
    )
    .unwrap();
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [key]);
    assert!(!done[0].cancelled);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(2));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a pending key is submitted again")]
fn a_pending_key_submitted_twice_panics_in_debug() {
    let mut driver = TestDriver::new(None);
    let (_, b) = connection(&mut driver);
    let key = pending(&mut driver, Op::Readable { handle: b });
    driver.submit(key, Op::Readable { handle: b });
}

fn localhost(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

fn listen(driver: &mut TestDriver, port: u16) -> std::io::Result<(IoHandle, u16)> {
    let listened = run_op(
        driver,
        Op::Listen {
            addr: localhost(port),
        },
    );
    listened.map(|done| match done {
        Done::Listener { handle, port } => (handle, port),
        other => panic!("a listen ended with {other:?}"),
    })
}

fn stream(result: std::io::Result<Done>) -> (IoHandle, std::net::SocketAddr) {
    match result.unwrap() {
        Done::Stream { handle, peer } => (handle, peer),
        other => panic!("a stream op ended with {other:?}"),
    }
}

fn connect(host: &str, port: u16) -> Op {
    Op::Connect {
        host: host.to_string(),
        port,
    }
}

/// 待ち受けて、受けた端と接続した端を返す。
fn connection(driver: &mut TestDriver) -> (IoHandle, IoHandle) {
    let (listener, port) = listen(driver, 0).unwrap();
    let (client, _) = stream(run_op(driver, connect("127.0.0.1", port)));
    let (server, _) = stream(run_op(driver, Op::Accept { listener }));
    (server, client)
}

#[test]
fn a_virtual_listen_on_port_zero_counts_from_49152() {
    let mut driver = TestDriver::new(None);
    assert_eq!(listen(&mut driver, 0).unwrap().1, 49152);
    assert_eq!(listen(&mut driver, 0).unwrap().1, 49153);
}

/// ポートの数は使っているポートを飛ばし、65535 の次は 49152 に戻る。すべて使っていれば空きはない。
#[test]
fn a_virtual_port_counter_skips_ports_in_use_and_wraps() {
    let mut driver = TestDriver::new(None);
    listen(&mut driver, 49153).unwrap();
    let (first, port) = listen(&mut driver, 0).unwrap();
    assert_eq!(port, 49152);
    assert_eq!(listen(&mut driver, 0).unwrap().1, 49154);
    for expected in 49155..=65535 {
        assert_eq!(listen(&mut driver, 0).unwrap().1, expected);
    }
    let error = listen(&mut driver, 0).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
    driver.close(first);
    assert_eq!(listen(&mut driver, 0).unwrap().1, 49152);
}

#[test]
fn a_virtual_port_in_use_is_addr_in_use() {
    let mut driver = TestDriver::new(None);
    let (_, port) = listen(&mut driver, 0).unwrap();
    assert_eq!(
        listen(&mut driver, port).unwrap_err().kind(),
        ErrorKind::AddrInUse
    );
    assert_eq!(listen(&mut driver, 8080).unwrap().1, 8080);
}

#[test]
fn a_virtual_connect_without_a_listener_is_refused() {
    let mut driver = TestDriver::new(None);
    assert_eq!(
        run_op(&mut driver, connect("127.0.0.1", 8080))
            .unwrap_err()
            .kind(),
        ErrorKind::ConnectionRefused
    );
}

/// IP アドレスの綴りと `localhost` は仮想のホストを指し、ほかの名前は見つからない。
#[test]
fn a_virtual_name_other_than_localhost_is_not_found() {
    let mut driver = TestDriver::new(None);
    let (_, port) = listen(&mut driver, 0).unwrap();
    assert_eq!(
        run_op(&mut driver, connect("example.com", port))
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    for host in ["localhost", "10.1.2.3", "::1"] {
        assert_eq!(
            stream(run_op(&mut driver, connect(host, port))).1,
            localhost(port)
        );
    }
}

/// 受けは接続が来るまで待つ。先に待っていた受けは、接続が backlog に入った後の決着で終わる。受けた端の相手は接続した
/// 側のポートで、接続した端の相手はリスナーのポートである。
#[test]
fn an_accept_waits_for_a_connect() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let accept = pending(&mut driver, Op::Accept { listener });
    assert!(collect(&mut driver).is_empty());
    assert!(!driver.wait(None).unwrap());
    let (client, server_addr) = stream(run_op(&mut driver, connect("localhost", port)));
    let mut done = settle(&mut driver);
    assert_eq!(keys(&done), [accept]);
    let (server, client_addr) = stream(done.pop().unwrap().result);
    assert_eq!(server_addr, localhost(port));
    assert_eq!(client_addr, localhost(port + 1));
    assert_ne!(client, server);
}

/// backlog が満ちていれば、接続は受けが席を空けるまで待つ。
#[test]
fn a_connect_to_a_full_backlog_waits_for_an_accept() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    for _ in 0..1024 {
        stream(run_op(&mut driver, connect("127.0.0.1", port)));
    }
    let waiting = pending(&mut driver, connect("127.0.0.1", port));
    assert!(settle(&mut driver).is_empty());
    assert_eq!(driver.pending(), 1);
    stream(run_op(&mut driver, Op::Accept { listener }));
    assert_eq!(keys(&settle(&mut driver)), [waiting]);
}

/// 書く側を閉じた端の相手は、残りを読んでから EOF を受け、それでも書き返せる。閉じた側の書きは broken pipe である。
#[test]
fn a_shutdown_write_gives_eof_and_the_peer_can_reply() {
    let mut driver = TestDriver::new(None);
    let (server, client) = connection(&mut driver);
    let mut data = *b"hi";
    let write = |driver: &mut TestDriver, handle: IoHandle, data: &mut [u8]| {
        run_op(
            driver,
            Op::Write {
                handle,
                buf: raw(data),
            },
        )
    };
    assert_eq!(
        write(&mut driver, client, &mut data).unwrap(),
        Done::Size(2)
    );
    driver.shutdown_write(client).unwrap();
    let mut room = [0u8; 4];
    for expected in [2, 0] {
        let read = run_op(
            &mut driver,
            Op::Read {
                handle: server,
                buf: raw(&mut room),
            },
        );
        assert_eq!(read.unwrap(), Done::Size(expected));
    }
    assert_eq!(&room[..2], b"hi");
    let mut reply = *b"ok";
    assert_eq!(
        write(&mut driver, server, &mut reply).unwrap(),
        Done::Size(2)
    );
    let read = run_op(
        &mut driver,
        Op::Read {
            handle: client,
            buf: raw(&mut room),
        },
    );
    assert_eq!(read.unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"ok");
    assert_eq!(
        write(&mut driver, client, &mut data).unwrap_err().kind(),
        ErrorKind::BrokenPipe
    );
    assert!(driver.set_nodelay(client, true).is_ok());
}

/// リスナーを閉じると、まだ受けていない接続の端も閉じ、接続した側は EOF を読む。ポートは空く。
#[test]
fn closing_a_listener_closes_its_backlog() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let (client, _) = stream(run_op(&mut driver, connect("127.0.0.1", port)));
    driver.close(listener);
    let mut room = [0u8; 4];
    let read = run_op(
        &mut driver,
        Op::Read {
            handle: client,
            buf: raw(&mut room),
        },
    );
    assert_eq!(read.unwrap(), Done::Size(0));
    assert_eq!(
        run_op(&mut driver, connect("127.0.0.1", port))
            .unwrap_err()
            .kind(),
        ErrorKind::ConnectionRefused
    );
    assert_eq!(listen(&mut driver, port).unwrap().1, port);
}

/// `quiesce` は、終わったがまだ渡していない完了が作ったストリームを閉じる。受け付けで終わる操作は完了を残さないので、
/// 決着で終わる受けで確かめる。
#[test]
fn quiesce_closes_the_handles_of_discarded_completions() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    pending(&mut driver, Op::Accept { listener });
    let (client, _) = stream(run_op(&mut driver, connect("127.0.0.1", port)));
    // 決着させるが、完了は渡さない
    assert!(driver.wait(None).unwrap());
    assert_eq!(driver.pending(), 1);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    // 受けた側の端が閉じたので、接続した端は EOF を読む
    let mut room = [0u8; 4];
    let read = run_op(
        &mut driver,
        Op::Read {
            handle: client,
            buf: raw(&mut room),
        },
    );
    assert_eq!(read.unwrap(), Done::Size(0));
}

/// 決着の前に取り消した操作の完了は `cancelled` で、自分で終わった操作の完了はそうでない。
#[test]
fn a_cancel_before_settle_marks_the_completion_cancelled() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let accept = pending(&mut driver, Op::Accept { listener });
    assert!(settle(&mut driver).is_empty());
    driver.cancel(accept);
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [accept]);
    assert!(done[0].result.is_err() && done[0].cancelled);
    let (_, b) = connection(&mut driver);
    let probe = pending(&mut driver, Op::Readable { handle: b });
    driver.cancel(probe);
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert!(done[0].cancelled);
    let accepted = pending(&mut driver, Op::Accept { listener });
    stream(run_op(&mut driver, connect("127.0.0.1", port)));
    let done = settle(&mut driver);
    assert_eq!(keys(&done), [accepted]);
    assert!(done.iter().all(|c| c.result.is_ok() && !c.cancelled));
}
