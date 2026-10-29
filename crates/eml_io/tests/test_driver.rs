//! テストの driver の性質。時計は終わる操作がないときだけ進み、決着の組は seed で順だけが変わる。

use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eml_io::TestDriver;
use eml_rt::{Completion, Done, IoDriver, IoHandle, Op, OpKey, RawBuf};

fn raw(bytes: &mut [u8]) -> RawBuf {
    RawBuf {
        ptr: bytes.as_mut_ptr(),
        cap: bytes.len(),
    }
}

fn collect(driver: &mut TestDriver) -> Vec<Completion> {
    let mut out = Vec::new();
    driver.completions(&mut out);
    out
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
    let (a, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: b });
    assert!(driver.wait(Some(10)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(10));
    assert_eq!(driver.pending(), 1);

    let mut data = *b"hi";
    driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut data),
    });
    assert!(driver.wait(Some(20)).unwrap());
    assert_eq!(driver.now(), Duration::from_millis(10));
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [1, probe]);
    assert!(done.iter().all(|c| c.result.is_ok()));

    let mut room = [0u8; 8];
    let read = driver.submit(Op::Read {
        handle: b,
        buf: raw(&mut room),
    });
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [read]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"hi");
}

/// seed は1回の決着で終わった操作の順だけを変え、組の中身は変えない。
#[test]
fn a_seed_does_not_change_which_ops_finish() {
    let run = |seed| {
        let mut driver = TestDriver::new(seed);
        let (a, b) = driver.pair().unwrap();
        let mut data = *b"x";
        for _ in 0..8 {
            driver.submit(Op::Job(Box::new(|| Ok(7))));
        }
        driver.submit(Op::Readable { handle: a });
        driver.submit(Op::Write {
            handle: a,
            buf: raw(&mut data),
        });
        driver.submit(Op::Readable { handle: b });
        let mut done = keys(&collect(&mut driver));
        let order = done.clone();
        done.sort_unstable();
        (done, order, driver.pending())
    };
    let (plain, plain_order, plain_pending) = run(None);
    assert_eq!(plain, plain_order);
    assert_eq!(plain_pending, 1);
    let mut shuffled = false;
    for seed in 0..4 {
        let (set, order, pending) = run(Some(seed));
        assert_eq!(set, plain);
        assert_eq!(pending, plain_pending);
        shuffled |= order != plain_order;
    }
    assert!(shuffled);
}

/// 決着は bin を先に空にするので、同じ決着の読みは相手が閉じた後の状態を見る。
#[test]
fn the_bin_is_emptied_before_ops_settle() {
    let mut driver = TestDriver::new(None);
    let (a, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: b });
    assert!(collect(&mut driver).is_empty());
    driver.bin().dropped.lock().unwrap().push(a);
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(0));

    let mut room = [0u8; 4];
    let read = driver.submit(Op::Read {
        handle: b,
        buf: raw(&mut room),
    });
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [read]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(0));
}

/// 相手が閉じた後も、残りを読みきってから EOF を受ける。
#[test]
fn a_closed_peer_leaves_its_bytes_before_eof() {
    let mut driver = TestDriver::new(None);
    let (a, b) = driver.pair().unwrap();
    let mut data = *b"abc";
    driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut data),
    });
    collect(&mut driver);
    driver.close(a);
    let mut room = [0u8; 2];
    for expected in [2, 1, 0] {
        driver.submit(Op::Read {
            handle: b,
            buf: raw(&mut room),
        });
        let done = collect(&mut driver);
        assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(expected));
    }
}

#[test]
fn a_write_to_a_closed_peer_is_a_broken_pipe() {
    let mut driver = TestDriver::new(None);
    let (a, b) = driver.pair().unwrap();
    driver.close(b);
    let mut data = *b"x";
    driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut data),
    });
    let done = collect(&mut driver);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().kind(),
        ErrorKind::BrokenPipe
    );
}

/// 返す前に取り消した操作は次の決着で誤りとして終わり、返した後の取り消しは何もしない。
#[test]
fn a_cancel_before_return_ends_the_op_at_the_next_settle() {
    let mut driver = TestDriver::new(None);
    let (_, b) = driver.pair().unwrap();
    let probe = driver.submit(Op::Readable { handle: b });
    assert!(collect(&mut driver).is_empty());
    driver.cancel(probe);
    assert_eq!(driver.pending(), 1);
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [probe]);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "cancelled"
    );
    driver.cancel(probe);
    assert!(collect(&mut driver).is_empty());
    assert_eq!(driver.pending(), 0);
}

/// すでに終わってまだ渡していない操作は、取り消しても終わったときの結果のまま渡す。
#[test]
fn a_cancel_after_the_op_finished_keeps_its_result() {
    let mut driver = TestDriver::new(None);
    let (a, _b) = driver.pair().unwrap();
    let mut data = *b"abc";
    let write = driver.submit(Op::Write {
        handle: a,
        buf: raw(&mut data),
    });
    assert!(driver.wait(Some(5)).unwrap());
    driver.cancel(write);
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [write]);
    assert_eq!(*done[0].result.as_ref().unwrap(), Done::Size(3));
}

/// 保留の操作は取り消して捨てるが、仕事は取り消せないので動かしきる。
#[test]
fn quiesce_drops_every_pending_op_and_runs_jobs() {
    let mut driver = TestDriver::new(None);
    let (_, b) = driver.pair().unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    driver.submit(Op::Readable { handle: b });
    let flag = Arc::clone(&ran);
    driver.submit(Op::Job(Box::new(move || {
        flag.store(true, Ordering::Relaxed);
        Ok(0)
    })));
    driver.quiesce();
    assert!(ran.load(Ordering::Relaxed));
    assert_eq!(driver.pending(), 0);
    assert!(collect(&mut driver).is_empty());
}

fn localhost(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

/// 終わった1つの完了の結果。
fn only(driver: &mut TestDriver) -> std::io::Result<Done> {
    let mut done = collect(driver);
    assert_eq!(done.len(), 1);
    done.pop().unwrap().result
}

fn listen(driver: &mut TestDriver, port: u16) -> std::io::Result<(IoHandle, u16)> {
    driver.submit(Op::Listen {
        addr: localhost(port),
    });
    only(driver).map(|done| match done {
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
    driver.submit(connect("127.0.0.1", port));
    let (client, _) = stream(only(driver));
    driver.submit(Op::Accept { listener });
    let (server, _) = stream(only(driver));
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
    driver.submit(connect("127.0.0.1", 8080));
    assert_eq!(
        only(&mut driver).unwrap_err().kind(),
        ErrorKind::ConnectionRefused
    );
}

/// IP アドレスの綴りと `localhost` は仮想のホストを指し、ほかの名前は見つからない。
#[test]
fn a_virtual_name_other_than_localhost_is_not_found() {
    let mut driver = TestDriver::new(None);
    let (_, port) = listen(&mut driver, 0).unwrap();
    driver.submit(connect("example.com", port));
    assert_eq!(only(&mut driver).unwrap_err().kind(), ErrorKind::NotFound);
    for host in ["localhost", "10.1.2.3", "::1"] {
        driver.submit(connect(host, port));
        assert_eq!(stream(only(&mut driver)).1, localhost(port));
    }
}

/// 受けは接続が来るまで待つ。決着は受け付けの順に1回ずつ見るので、先に待っていた受けは、接続が backlog に入った次の
/// 決着で終わる。受けた端の相手は接続した側のポートで、接続した端の相手はリスナーのポートである。
#[test]
fn an_accept_waits_for_a_connect() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let accept = driver.submit(Op::Accept { listener });
    assert!(collect(&mut driver).is_empty());
    assert!(!driver.wait(None).unwrap());
    let connect = driver.submit(connect("localhost", port));
    let mut done = collect(&mut driver);
    assert_eq!(keys(&done), [connect]);
    let (client, server_addr) = stream(done.pop().unwrap().result);
    let mut done = collect(&mut driver);
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
        driver.submit(connect("127.0.0.1", port));
    }
    assert_eq!(collect(&mut driver).len(), 1024);
    let waiting = driver.submit(connect("127.0.0.1", port));
    assert!(collect(&mut driver).is_empty());
    assert_eq!(driver.pending(), 1);
    let accept = driver.submit(Op::Accept { listener });
    assert_eq!(keys(&collect(&mut driver)), [accept]);
    assert_eq!(keys(&collect(&mut driver)), [waiting]);
}

/// 書く側を閉じた端の相手は、残りを読んでから EOF を受け、それでも書き返せる。閉じた側の書きは broken pipe である。
#[test]
fn a_shutdown_write_gives_eof_and_the_peer_can_reply() {
    let mut driver = TestDriver::new(None);
    let (server, client) = connection(&mut driver);
    let mut data = *b"hi";
    driver.submit(Op::Write {
        handle: client,
        buf: raw(&mut data),
    });
    assert_eq!(only(&mut driver).unwrap(), Done::Size(2));
    driver.shutdown_write(client).unwrap();
    let mut room = [0u8; 4];
    for expected in [2, 0] {
        driver.submit(Op::Read {
            handle: server,
            buf: raw(&mut room),
        });
        assert_eq!(only(&mut driver).unwrap(), Done::Size(expected));
    }
    assert_eq!(&room[..2], b"hi");
    let mut reply = *b"ok";
    driver.submit(Op::Write {
        handle: server,
        buf: raw(&mut reply),
    });
    assert_eq!(only(&mut driver).unwrap(), Done::Size(2));
    driver.submit(Op::Read {
        handle: client,
        buf: raw(&mut room),
    });
    assert_eq!(only(&mut driver).unwrap(), Done::Size(2));
    assert_eq!(&room[..2], b"ok");
    driver.submit(Op::Write {
        handle: client,
        buf: raw(&mut data),
    });
    assert_eq!(only(&mut driver).unwrap_err().kind(), ErrorKind::BrokenPipe);
    assert!(driver.set_nodelay(client, true).is_ok());
}

/// リスナーを閉じると、まだ受けていない接続の端も閉じ、接続した側は EOF を読む。ポートは空く。
#[test]
fn closing_a_listener_closes_its_backlog() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    driver.submit(connect("127.0.0.1", port));
    let (client, _) = stream(only(&mut driver));
    driver.close(listener);
    let mut room = [0u8; 4];
    driver.submit(Op::Read {
        handle: client,
        buf: raw(&mut room),
    });
    assert_eq!(only(&mut driver).unwrap(), Done::Size(0));
    driver.submit(connect("127.0.0.1", port));
    assert_eq!(
        only(&mut driver).unwrap_err().kind(),
        ErrorKind::ConnectionRefused
    );
    assert_eq!(listen(&mut driver, port).unwrap().1, port);
}

/// `quiesce` は、終わったがまだ渡していない完了が作ったリスナーとストリームを閉じる。
#[test]
fn quiesce_closes_the_handles_of_discarded_completions() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    driver.submit(Op::Listen {
        addr: localhost(50000),
    });
    driver.submit(connect("127.0.0.1", port));
    // 決着させるが、完了は渡さない
    assert!(driver.wait(None).unwrap());
    assert_eq!(driver.pending(), 2);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    assert_eq!(listen(&mut driver, 50000).unwrap().1, 50000);
    // 接続した側の端が閉じたので、受けた端は EOF を読む
    driver.submit(Op::Accept { listener });
    let (server, _) = stream(only(&mut driver));
    let mut room = [0u8; 4];
    driver.submit(Op::Read {
        handle: server,
        buf: raw(&mut room),
    });
    assert_eq!(only(&mut driver).unwrap(), Done::Size(0));
}

/// 決着の前に取り消した操作の完了は `cancelled` で、自分で終わった操作の完了はそうでない。
#[test]
fn a_cancel_before_settle_marks_the_completion_cancelled() {
    let mut driver = TestDriver::new(None);
    let (listener, port) = listen(&mut driver, 0).unwrap();
    let accept = driver.submit(Op::Accept { listener });
    assert!(collect(&mut driver).is_empty());
    driver.cancel(accept);
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [accept]);
    assert!(done[0].result.is_err() && done[0].cancelled);
    let cancelled = driver.submit(connect("127.0.0.1", port));
    driver.cancel(cancelled);
    let done = collect(&mut driver);
    assert!(done[0].cancelled);
    driver.submit(Op::Accept { listener });
    let done = collect(&mut driver);
    assert!(done.is_empty(), "a cancelled connect reached the backlog");
    let connected = driver.submit(connect("127.0.0.1", port));
    let mut done = collect(&mut driver);
    done.extend(collect(&mut driver));
    assert_eq!(done.len(), 2);
    assert!(done.iter().all(|c| c.result.is_ok() && !c.cancelled));
    assert!(keys(&done).contains(&connected));
}
