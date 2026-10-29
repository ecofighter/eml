//! テストの driver の性質。時計は終わる操作がないときだけ進み、決着の組は seed で順だけが変わる。

use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eml_io::TestDriver;
use eml_rt::{Completion, IoDriver, Op, OpKey, RawBuf};

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
    assert_eq!(*done[0].result.as_ref().unwrap(), 2);
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
    assert_eq!(*done[0].result.as_ref().unwrap(), 0);

    let mut room = [0u8; 4];
    let read = driver.submit(Op::Read {
        handle: b,
        buf: raw(&mut room),
    });
    let done = collect(&mut driver);
    assert_eq!(keys(&done), [read]);
    assert_eq!(*done[0].result.as_ref().unwrap(), 0);
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
        assert_eq!(*done[0].result.as_ref().unwrap(), expected);
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
    assert_eq!(*done[0].result.as_ref().unwrap(), 3);
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
