//! 核の単体テスト。偽の backend の上で流し、Miri でも流す (docs/implementation/testing.md の「核の Miri」)。本物の
//! 時計を使わず、順は門で作る。出した仕事と解決は、本体が答えを積み、核が引き取るまで進めてから driver を落とす。

mod abort;
mod fake;

use std::io;
use std::net::SocketAddr;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use eml_rt::{Completion, Done, IoDriver, IoHandle, Op, OpKey, RawBuf, Submit};

use super::Driver;
use super::backend::{Arrival, Cancel, Event, Hold, Outcome};
use fake::{Call, Fake, FakeSock, Reply, StepKind};

/// 開くまで通さない門。`fn` の解決器は値を捕まえられないので、テストごとに `static` に置く。
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    const fn new() -> Gate {
        Gate {
            open: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }

    fn pass(&self) {
        let open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let (_guard, waited) = self
            .cv
            .wait_timeout_while(open, Duration::from_secs(10), |open| !*open)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(!waited.timed_out(), "the gate was never opened");
    }
}

fn no_lookup(_: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    Err(io::Error::other("this test looks up no names"))
}

fn driver() -> Driver<Fake> {
    with_lookup(no_lookup)
}

fn with_lookup(lookup: super::resolve::Lookup) -> Driver<Fake> {
    Driver::new(Fake::new(), lookup)
}

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// 待ち受けを作り、そのハンドルを返す。偽のソケットの番号は 0 から振られる。
fn listen(driver: &mut Driver<Fake>) -> IoHandle {
    match driver.submit(
        1000,
        Op::Listen {
            addr: addr("127.0.0.1:0"),
        },
    ) {
        Submit::Done(Ok(Done::Listener { handle, .. })) => handle,
        other => panic!("{other:?}"),
    }
}

fn buf(bytes: &mut [u8]) -> RawBuf {
    RawBuf {
        ptr: bytes.as_mut_ptr(),
        cap: bytes.len(),
    }
}

fn completions<B: super::Backend>(driver: &mut Driver<B>) -> Vec<Completion> {
    let mut out = Vec::new();
    driver.completions(&mut out);
    out
}

/// 保留がなくなるまで待ち、届いた完了をすべて返す。
fn settle<B: super::Backend>(driver: &mut Driver<B>) -> Vec<Completion> {
    let mut out = Vec::new();
    while driver.pending() > 0 {
        assert!(driver.wait(None).unwrap());
        driver.completions(&mut out);
    }
    out
}

fn error_of(submit: Submit) -> io::Error {
    match submit {
        Submit::Done(Err(e)) => e,
        other => panic!("{other:?}"),
    }
}

fn pending(submit: Submit) {
    assert!(matches!(submit, Submit::Pending), "{submit:?}");
}

fn retry(key: OpKey) -> Event<FakeSock> {
    Event {
        key,
        arrival: Arrival::Retry,
    }
}

/// `Wait` を除いた呼び出し。
fn calls<const S: bool>(fake: &Fake<S>) -> Vec<&Call> {
    fake.calls
        .iter()
        .filter(|c| !matches!(c, Call::Wait(_)))
        .collect()
}

#[test]
fn a_short_write_is_continued_under_the_same_key() {
    let mut data = b"hello world".to_vec();
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(
        1,
        [Reply::Take(3), Reply::Pending(Hold::None), Reply::Take(8)],
    );
    pending(driver.submit(
        1,
        Op::Write {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.events.push(retry(1));
    assert!(driver.wait(None).unwrap());
    let done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].key, 1);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(11));
    assert_eq!(driver.backend.sent, b"hello world");
    let sends = calls(&driver.backend)
        .into_iter()
        .filter(|c| **c == Call::Start(1, StepKind::Send))
        .count();
    assert_eq!(sends, 3);
    assert_eq!(driver.pending(), 0);
}

#[test]
fn an_aborted_accept_is_retried_under_the_same_key() {
    let peer = addr("192.0.2.7:5000");
    let mut driver = driver();
    let listener = listen(&mut driver);
    driver.backend.script(
        1,
        [
            Reply::Pending(Hold::None),
            Reply::Done(Err(io::ErrorKind::ConnectionAborted.into())),
            Reply::Done(Ok(Outcome::Accepted {
                sock: FakeSock(7),
                peer,
            })),
        ],
    );
    pending(driver.submit(1, Op::Accept { listener }));
    driver.backend.events.push(retry(1));
    let done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    match done[0].result.as_ref().unwrap() {
        Done::Stream { handle, peer: got } => {
            assert_ne!(*handle, listener);
            assert_eq!(*got, peer);
        }
        other => panic!("{other:?}"),
    }
    let accepts = calls(&driver.backend)
        .into_iter()
        .filter(|c| **c == Call::Start(1, StepKind::Accept))
        .count();
    assert_eq!(accepts, 3);
}

fn two_addresses(_: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    Ok(vec![addr("192.0.2.1:80"), addr("192.0.2.2:80")])
}

#[test]
fn a_refused_connect_tries_the_next_address() {
    let (a1, a2) = (addr("192.0.2.1:80"), addr("192.0.2.2:80"));
    let mut driver = with_lookup(two_addresses);
    driver.backend.script(
        1,
        [
            Reply::Done(Err(io::ErrorKind::ConnectionRefused.into())),
            Reply::Done(Ok(Outcome::Connected)),
        ],
    );
    pending(driver.submit(
        1,
        Op::Connect {
            host: "example".to_string(),
            port: 80,
        },
    ));
    let done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    assert!(matches!(
        done[0].result.as_ref().unwrap(),
        Done::Stream { peer, .. } if *peer == a2
    ));
    assert_eq!(
        calls(&driver.backend),
        [
            &Call::Socket(a1),
            &Call::Start(1, StepKind::Connect(a1)),
            &Call::Close(0),
            &Call::Socket(a2),
            &Call::Start(1, StepKind::Connect(a2)),
        ]
    );
}

#[test]
fn a_cancelled_connect_does_not_try_the_next_address() {
    let a1 = addr("192.0.2.1:80");
    let mut driver = with_lookup(two_addresses);
    pending(driver.submit(
        1,
        Op::Connect {
            host: "example".to_string(),
            port: 80,
        },
    ));
    // 解決の答えが届き、1つ目の番地への接続が待つまで進める
    while !driver
        .backend
        .calls
        .contains(&Call::Start(1, StepKind::Connect(a1)))
    {
        assert!(driver.wait(None).unwrap());
    }
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.cancel(1);
    assert_eq!(driver.pending(), 1);
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Done(Err(io::ErrorKind::ConnectionRefused.into())),
    });
    let done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    assert!(done[0].cancelled);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "cancelled"
    );
    assert_eq!(
        calls(&driver.backend),
        [
            &Call::Socket(a1),
            &Call::Start(1, StepKind::Connect(a1)),
            &Call::Cancel(1, StepKind::Connect(a1)),
            &Call::Close(0),
        ]
    );
}

#[test]
fn a_connect_to_an_ip_address_looks_up_nothing() {
    let peer = addr("192.0.2.9:7");
    let mut driver = driver();
    driver
        .backend
        .script(1, [Reply::Done(Ok(Outcome::Connected))]);
    match driver.submit(
        1,
        Op::Connect {
            host: "192.0.2.9".to_string(),
            port: 7,
        },
    ) {
        Submit::Done(Ok(Done::Stream { peer: got, .. })) => assert_eq!(got, peer),
        other => panic!("{other:?}"),
    }
    assert_eq!(driver.backend.spawned.get(), 0);
}

#[test]
fn a_retry_delivered_twice_completes_once() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver
        .backend
        .script(1, [Reply::Pending(Hold::None), Reply::Fill(b"hi".to_vec())]);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.events.extend([retry(1), retry(1)]);
    assert!(driver.wait(None).unwrap());
    let done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(2));
    assert_eq!(&data[..2], b"hi");
    assert_eq!(driver.pending(), 0);
    let recvs = calls(&driver.backend)
        .into_iter()
        .filter(|c| **c == Call::Start(1, StepKind::Recv))
        .count();
    assert_eq!(recvs, 2);
}

#[test]
fn a_cancel_done_completes_at_once() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.cancel(1);
    let done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert!(done[0].cancelled);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "cancelled"
    );
    assert!(
        !driver
            .backend
            .calls
            .iter()
            .any(|c| matches!(c, Call::Wait(_)))
    );
    assert_eq!(driver.pending(), 0);
}

#[test]
fn a_wait_after_a_cancel_returns_true() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.cancel(1);
    assert!(driver.wait(None).unwrap());
    // 届いた完了があるので、OS を1回見るだけで待たない
    let waits: Vec<&Call> = driver
        .backend
        .calls
        .iter()
        .filter(|c| matches!(c, Call::Wait(_)))
        .collect();
    assert_eq!(waits, [&Call::Wait(Some(Duration::ZERO))]);
    assert_eq!(completions(&mut driver).len(), 1);
}

#[test]
fn a_second_op_in_one_direction_is_invalid_input() {
    let (mut a, mut b, mut c) = ([0u8; 4], [0u8; 4], [0u8; 4]);
    let mut driver = driver();
    let h = listen(&mut driver);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut a),
        },
    ));
    let e = error_of(driver.submit(
        2,
        Op::Read {
            handle: h,
            buf: buf(&mut b),
        },
    ));
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    let e = error_of(driver.submit(3, Op::Readable { handle: h }));
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    pending(driver.submit(
        4,
        Op::Write {
            handle: h,
            buf: buf(&mut c),
        },
    ));
    assert_eq!(driver.pending(), 2);
}

#[test]
fn closing_a_handle_with_a_pending_op_closes_after_the_op() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver
        .backend
        .script(1, [Reply::Pending(Hold::None), Reply::Fill(b"ab".to_vec())]);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.close(h);
    assert!(!driver.backend.calls.contains(&Call::Close(0)));
    let e = error_of(driver.submit(2, Op::Readable { handle: h }));
    assert_eq!(e.to_string(), "no such handle");
    driver.backend.events.push(retry(1));
    let done = settle(&mut driver);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(2));
    let closes = driver
        .backend
        .calls
        .iter()
        .filter(|c| **c == Call::Close(0))
        .count();
    assert_eq!(closes, 1);
    assert_eq!(driver.backend.calls.last(), Some(&Call::Close(0)));
}

#[test]
fn quiesce_closes_the_stream_of_a_discarded_accept() {
    let mut driver = driver();
    let listener = listen(&mut driver);
    pending(driver.submit(1, Op::Accept { listener }));
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Done(Ok(Outcome::Accepted {
            sock: FakeSock(9),
            peer: addr("192.0.2.3:1"),
        })),
    });
    assert!(driver.wait(None).unwrap());
    assert_eq!(driver.pending(), 1);
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    let closes = driver
        .backend
        .calls
        .iter()
        .filter(|c| **c == Call::Close(9))
        .count();
    assert_eq!(closes, 1);
}

#[test]
fn a_failed_wait_breaks_the_driver() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.wait_error = Some(io::Error::other("the poller failed"));
    assert_eq!(
        driver.wait(None).unwrap_err().to_string(),
        "the poller failed"
    );
    let waits = driver.backend.calls.len();
    assert_eq!(
        driver.wait(Some(0)).unwrap_err().to_string(),
        "the poller failed"
    );
    let e = error_of(driver.submit(
        2,
        Op::Write {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    assert_eq!(e.to_string(), "the poller failed");
    let e = error_of(driver.submit(3, Op::Job(Box::new(|| Ok(0)))));
    assert_eq!(e.to_string(), "the poller failed");
    driver.cancel(1);
    assert_eq!(driver.backend.calls.len(), waits);
    assert!(completions(&mut driver).is_empty());
    driver.quiesce();
    assert_eq!(driver.pending(), 1);
    assert_eq!(driver.backend.spawned.get(), 0);
}

#[test]
fn quiesce_breaks_on_a_cancel_that_never_completes() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(1, [Reply::Pending(Hold::Memory)]);
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.quiesce();
    assert_eq!(driver.now(), Duration::from_secs(1));
    assert_eq!(driver.pending(), 1);
    assert_eq!(
        driver.wait(None).unwrap_err().to_string(),
        "a cancelled op did not complete"
    );
    // 落とす前に取り消しの終わりを届ける。届かなければ `Drop` はプロセスを止める
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Cancelled,
    });
    drop(driver);
}

#[test]
fn a_held_op_completed_within_the_limit_lets_drop_return() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(1, [Reply::Pending(Hold::Memory)]);
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Cancelled,
    });
    drop(driver);
}

#[test]
fn dropping_a_driver_closes_the_socket_of_a_held_connect() {
    let mut driver = driver();
    driver.backend.script(1, [Reply::Pending(Hold::Memory)]);
    pending(driver.submit(
        1,
        Op::Connect {
            host: "192.0.2.9".to_string(),
            port: 7,
        },
    ));
    let closed = Arc::clone(&driver.backend.closed);
    // `Drop` の取り消しは偽の backend の既定の `Cancel::Done` でその場で終わる
    drop(driver);
    assert_eq!(*closed.lock().unwrap(), [0]);
}

/// OS が貸した buffer に書きうる読みを `key` で出し、そのハンドルを返す。
fn held_read(driver: &mut Driver<Fake>, key: OpKey, data: &mut [u8]) -> IoHandle {
    let h = listen(driver);
    driver.backend.script(key, [Reply::Pending(Hold::Memory)]);
    pending(driver.submit(
        key,
        Op::Read {
            handle: h,
            buf: buf(data),
        },
    ));
    h
}

fn starts(fake: &Fake) -> usize {
    fake.calls
        .iter()
        .filter(|c| matches!(c, Call::Start(..)))
        .count()
}

fn cancels(fake: &Fake) -> usize {
    fake.calls
        .iter()
        .filter(|c| matches!(c, Call::Cancel(..)))
        .count()
}

#[test]
fn a_retry_for_a_cancelled_held_op_ends_cancelled() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    held_read(&mut driver, 1, &mut data);
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.cancel(1);
    driver.backend.events.push(retry(1));
    let done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    assert!(done[0].cancelled);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "cancelled"
    );
    assert_eq!(starts(&driver.backend), 1);
}

#[test]
fn a_retry_for_a_held_op_restarts_the_step() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(
        1,
        [Reply::Pending(Hold::Memory), Reply::Fill(b"ab".to_vec())],
    );
    pending(driver.submit(
        1,
        Op::Read {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.events.push(retry(1));
    let done = settle(&mut driver);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(2));
    assert_eq!(starts(&driver.backend), 2);
    assert_eq!(&data[..2], b"ab");
}

/// 取り消しを頼んだ書きの欄に、`cap` のうち `n` バイトを書き終えた event が届いたときの完了。
fn write_completed_after_a_cancel(n: usize) -> (Completion, usize) {
    let mut data = [1u8; 4];
    let mut driver = driver();
    let h = listen(&mut driver);
    driver.backend.script(1, [Reply::Pending(Hold::Memory)]);
    pending(driver.submit(
        1,
        Op::Write {
            handle: h,
            buf: buf(&mut data),
        },
    ));
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.cancel(1);
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Done(Ok(Outcome::Size(n))),
    });
    let mut done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    (done.remove(0), starts(&driver.backend))
}

#[test]
fn a_full_write_completed_after_a_cancel_succeeds() {
    let (done, _) = write_completed_after_a_cancel(4);
    assert_eq!(done.result.as_ref().unwrap(), &Done::Size(4));
    assert!(!done.cancelled);
}

#[test]
fn a_short_write_completed_after_a_cancel_ends_cancelled() {
    let (done, starts) = write_completed_after_a_cancel(2);
    assert!(done.cancelled);
    // 取り消した書きは続きを出さない
    assert_eq!(starts, 1);
}

#[test]
fn closing_a_handle_with_a_held_op_closes_after_the_event() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    let h = held_read(&mut driver, 1, &mut data);
    driver.close(h);
    assert!(!driver.backend.calls.contains(&Call::Close(0)));
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Done(Ok(Outcome::Size(1))),
    });
    let done = settle(&mut driver);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(1));
    assert_eq!(driver.backend.calls.last(), Some(&Call::Close(0)));
}

#[test]
fn drop_does_not_cancel_a_cancelled_held_op_twice() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    held_read(&mut driver, 1, &mut data);
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.cancel(1);
    assert_eq!(cancels(&driver.backend), 1);
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Cancelled,
    });
    let keys = Arc::clone(&driver.backend.cancelled_keys);
    drop(driver);
    assert_eq!(*keys.lock().unwrap(), [1]);
}

#[test]
fn drop_after_a_broken_quiesce_cancels_every_slot_once() {
    let mut d1 = [0u8; 4];
    let mut d2 = [0u8; 4];
    let mut driver = driver();
    held_read(&mut driver, 1, &mut d1);
    held_read(&mut driver, 2, &mut d2);
    driver
        .backend
        .cancels
        .push_back(Err(io::Error::other("refused")));
    driver.quiesce();
    assert_eq!(driver.pending(), 2);
    assert_eq!(driver.wait(None).unwrap_err().to_string(), "refused");
    // 断られた欄も、印だけ立った欄も、落とすときに1回ずつ取り消しを出す
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    for key in [1, 2] {
        driver.backend.events.push(Event {
            key,
            arrival: Arrival::Cancelled,
        });
    }
    let keys = Arc::clone(&driver.backend.cancelled_keys);
    let closed = Arc::clone(&driver.backend.closed);
    drop(driver);
    // `quiesce` がどちらの鍵で断られたかは表の順による
    let keys = keys.lock().unwrap().clone();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains(&1) && keys.contains(&2));
    let mut closed = closed.lock().unwrap().clone();
    closed.sort();
    assert_eq!(closed, [0, 1]);
}

#[test]
fn a_driver_broken_by_a_failed_wait_still_reads_the_ends_on_drop() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    held_read(&mut driver, 1, &mut data);
    driver.backend.wait_error = Some(io::Error::other("the poller failed"));
    assert!(driver.wait(None).is_err());
    driver.backend.cancels.push_back(Ok(Cancel::Pending));
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Cancelled,
    });
    drop(driver);
}

#[test]
fn an_event_for_an_unknown_key_closes_only_an_accepted_sock() {
    let mut driver = driver();
    let _listener = listen(&mut driver);
    driver.backend.events.push(Event {
        key: 7,
        arrival: Arrival::Done(Ok(Outcome::Accepted {
            sock: FakeSock(9),
            peer: addr("192.0.2.3:1"),
        })),
    });
    assert!(driver.wait(Some(u64::MAX)).unwrap());
    assert!(completions(&mut driver).is_empty());
    assert_eq!(driver.pending(), 0);
    let closes: Vec<&Call> = calls(&driver.backend)
        .into_iter()
        .filter(|c| matches!(c, Call::Close(_)))
        .collect();
    assert_eq!(closes, [&Call::Close(9)]);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "the backend reported a cancel the core did not ask for")]
fn a_cancelled_arrival_without_a_cancel_is_a_contract_violation() {
    let mut data = [0u8; 4];
    let mut driver = driver();
    held_read(&mut driver, 1, &mut data);
    driver.backend.events.push(Event {
        key: 1,
        arrival: Arrival::Cancelled,
    });
    let _ = driver.wait(None);
}

#[test]
fn a_job_panic_is_raised_on_the_driver_thread() {
    let mut driver = driver();
    pending(driver.submit(1, Op::Job(Box::new(|| panic!("boom")))));
    pending(driver.submit(2, Op::Job(Box::new(|| Ok(7)))));
    let mut done = Vec::new();
    let payload = loop {
        match panic::catch_unwind(AssertUnwindSafe(|| driver.wait(None))) {
            Ok(r) => {
                assert!(r.unwrap());
                driver.completions(&mut done);
            }
            Err(payload) => break payload,
        }
    };
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"boom"));
    done.extend(settle(&mut driver));
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].key, 2);
    assert_eq!(done[0].result.as_ref().unwrap(), &Done::Size(7));
    assert_eq!(driver.pending(), 0);
}

#[test]
fn a_job_whose_thread_cannot_start_completes_once_with_an_error() {
    let mut driver = driver();
    driver.backend.spawn_error = true;
    pending(driver.submit(1, Op::Job(Box::new(|| Ok(1)))));
    assert!(driver.wait(None).unwrap());
    let done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "no threads"
    );
    assert!(!driver.wait(None).unwrap());
}

#[test]
fn quiesce_waits_for_a_running_job() {
    let (tx, rx) = mpsc::channel::<()>();
    let ran = Arc::new(AtomicBool::new(false));
    let mut driver = driver();
    let job_ran = Arc::clone(&ran);
    pending(driver.submit(
        1,
        Op::Job(Box::new(move || {
            rx.recv().unwrap();
            job_ran.store(true, Ordering::SeqCst);
            Ok(3)
        })),
    ));
    // 門は `quiesce` が待ち始めた後に開く。遅れは検出力のためだけで、待つ driver ではいつも仕事が先に終わる
    let opener = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        tx.send(()).unwrap();
    });
    driver.quiesce();
    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(driver.pending(), 0);
    assert!(completions(&mut driver).is_empty());
    opener.join().unwrap();
}

static SLOW: Gate = Gate::new();

fn slow_or_fast(host: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    if host == "slow" {
        SLOW.pass();
        return Ok(vec![addr("192.0.2.1:80")]);
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "fast"))
}

#[test]
fn a_late_answer_of_a_cancelled_lookup_does_not_hit_the_reused_key() {
    let mut driver = with_lookup(slow_or_fast);
    let connect = |host: &str| Op::Connect {
        host: host.to_string(),
        port: 80,
    };
    pending(driver.submit(5, connect("slow")));
    driver.cancel(5);
    let done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert!(done[0].cancelled);
    pending(driver.submit(5, connect("fast")));
    let done = settle(&mut driver);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.as_ref().unwrap_err().to_string(), "fast");
    SLOW.open();
    driver.backend.finished.reach(2);
    // 期限があるので `Ok(false)` の近道を通らず、遅れた答えを引き取る
    assert!(driver.wait(Some(0)).unwrap());
    assert!(completions(&mut driver).is_empty());
    assert_eq!(driver.pending(), 0);
    assert!(calls(&driver.backend).is_empty());
}

static QUIESCED: Gate = Gate::new();

fn gated(_: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    QUIESCED.pass();
    Ok(vec![addr("192.0.2.1:80")])
}

#[test]
fn quiesce_abandons_a_lookup_and_drops_its_late_answer() {
    let mut driver = with_lookup(gated);
    pending(driver.submit(
        1,
        Op::Connect {
            host: "example".to_string(),
            port: 80,
        },
    ));
    driver.quiesce();
    assert_eq!(driver.pending(), 0);
    QUIESCED.open();
    driver.backend.finished.reach(1);
    assert!(driver.wait(Some(0)).unwrap());
    assert!(completions(&mut driver).is_empty());
    assert!(calls(&driver.backend).is_empty());
}

static NINE: [Gate; 9] = [const { Gate::new() }; 9];

fn ninefold(host: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    NINE[host.parse::<usize>().unwrap()].pass();
    Err(io::Error::new(io::ErrorKind::NotFound, host.to_string()))
}

#[test]
fn a_ninth_lookup_starts_when_an_answer_is_taken() {
    let mut driver = with_lookup(ninefold);
    for i in 0..9 {
        pending(driver.submit(
            i,
            Op::Connect {
                host: i.to_string(),
                port: 80,
            },
        ));
    }
    assert_eq!(driver.backend.spawned.get(), 8);
    NINE[0].open();
    driver.backend.finished.reach(1);
    assert!(driver.wait(Some(0)).unwrap());
    assert_eq!(driver.backend.spawned.get(), 9);
    let mut done = completions(&mut driver);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.as_ref().unwrap_err().to_string(), "0");
    for gate in &NINE[1..] {
        gate.open();
    }
    done.extend(settle(&mut driver));
    assert_eq!(done.len(), 9);
    driver.backend.finished.reach(9);
}

static STRANDED: [Gate; 9] = [const { Gate::new() }; 9];

fn first_answers(host: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    let i = host.parse::<usize>().unwrap();
    STRANDED[i].pass();
    if i == 0 {
        return Ok(vec![addr("192.0.2.1:80")]);
    }
    Err(io::Error::new(io::ErrorKind::NotFound, host.to_string()))
}

#[test]
fn a_lookup_that_cannot_start_while_an_answer_is_taken_completes() {
    let mut driver = with_lookup(first_answers);
    for i in 0..9 {
        pending(driver.submit(
            i,
            Op::Connect {
                host: i.to_string(),
                port: 80,
            },
        ));
    }
    assert_eq!(driver.backend.spawned.get(), 8);
    // 9本目は、1本目の答えを引き取ったときに始めようとして失敗する
    driver.backend.spawn_error = true;
    STRANDED[0].open();
    driver.backend.finished.reach(1);
    // 起こしは消費済みとする。1本目の答えは同じ引き取りの中で接続になって待つので、失敗の答えを拾わずに眠ると
    // 何も起こさない
    driver.backend.bell.clear();
    let mut done = Vec::new();
    while done.is_empty() {
        assert!(driver.wait(None).unwrap());
        driver.completions(&mut done);
    }
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].key, 8);
    assert_eq!(
        done[0].result.as_ref().unwrap_err().to_string(),
        "no threads"
    );
    assert!(
        driver
            .backend
            .calls
            .contains(&Call::Start(0, StepKind::Connect(addr("192.0.2.1:80"))))
    );
    driver.cancel(0);
    for gate in &STRANDED[1..8] {
        gate.open();
    }
    done.extend(settle(&mut driver));
    assert_eq!(done.len(), 9);
    driver.backend.finished.reach(8);
}

#[test]
fn sockets_off_rejects_before_looking_up_the_handle() {
    let mut driver: Driver<Fake<false>> = Driver::new(Fake::new(), no_lookup);
    let unsupported = |submit: Submit| {
        assert_eq!(error_of(submit).kind(), io::ErrorKind::Unsupported);
    };
    unsupported(driver.submit(
        1,
        Op::Listen {
            addr: addr("127.0.0.1:0"),
        },
    ));
    unsupported(driver.submit(2, Op::Accept { listener: 0 }));
    unsupported(driver.submit(
        3,
        Op::Connect {
            host: "example".to_string(),
            port: 80,
        },
    ));
    unsupported(driver.submit(
        4,
        Op::Connect {
            host: "127.0.0.1".to_string(),
            port: 80,
        },
    ));
    assert_eq!(
        driver.shutdown_write(0).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        driver.set_nodelay(0, true).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    let e = error_of(driver.submit(5, Op::Readable { handle: 0 }));
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(driver.backend.calls.is_empty());
    assert_eq!(driver.backend.spawned.get(), 0);
}

mod pool {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use eml_rt::{Done, IoDriver, Op};

    use super::super::backend::Backend;
    use super::super::inbox::{Answer, Inbox};
    use super::super::pool::Pool;
    use super::fake::Fake;
    use super::{driver, pending, settle};

    #[test]
    fn a_job_beyond_the_limit_does_not_block_submit() {
        let fake: Fake = Fake::new();
        let inbox = Inbox::new(fake.waker());
        let mut pool = Pool::new(Arc::clone(&inbox), 2);
        let (started_tx, started) = mpsc::channel();
        let mut gates = Vec::new();
        for i in 0..3u64 {
            let (gate, gate_rx) = mpsc::channel::<()>();
            gates.push(gate);
            let started_tx = started_tx.clone();
            pool.submit(
                &fake,
                i,
                Box::new(move || {
                    started_tx.send(i).unwrap();
                    gate_rx.recv().unwrap();
                    Ok(usize::try_from(i).unwrap())
                }),
            );
        }
        let first = started.recv().unwrap();
        let second = started.recv().unwrap();
        // 2本のスレッドがどちらも門で止まっているので、3つ目は列で待つ
        assert!(started.try_recv().is_err());
        gates[usize::try_from(first).unwrap()].send(()).unwrap();
        let third = started.recv().unwrap();
        assert_eq!(first + second + third, 3);
        for gate in &gates {
            let _ = gate.send(());
        }
        let mut answers = std::collections::VecDeque::new();
        while answers.len() < 3 {
            inbox.take(&mut answers);
            if answers.len() < 3 {
                fake.bell.wait();
            }
        }
        let mut sizes: Vec<usize> = answers
            .into_iter()
            .map(|(_, answer)| match answer {
                Answer::Job(Ok(Ok(n))) => n,
                _ => panic!("a job failed"),
            })
            .collect();
        sizes.sort();
        assert_eq!(sizes, [0, 1, 2]);
        assert_eq!(fake.spawned.get(), 2);
        drop(pool);
        assert_eq!(fake.finished.get(), 2);
    }

    #[test]
    fn two_jobs_submitted_to_one_idle_thread_run_together() {
        let mut driver = driver();
        pending(driver.submit(0, Op::Job(Box::new(|| Ok(0)))));
        assert_eq!(settle(&mut driver).len(), 1);
        // 空いたスレッドを作るためだけに眠る。合否には効かない
        thread::sleep(Duration::from_millis(5));
        let (tx, rx) = mpsc::channel::<()>();
        pending(driver.submit(
            1,
            Op::Job(Box::new(move || {
                rx.recv().unwrap();
                Ok(1)
            })),
        ));
        pending(driver.submit(
            2,
            Op::Job(Box::new(move || {
                tx.send(()).unwrap();
                Ok(2)
            })),
        ));
        let done = settle(&mut driver);
        assert_eq!(done.len(), 2);
        assert!(done.iter().all(|c| matches!(c.result, Ok(Done::Size(_)))));
    }
}
