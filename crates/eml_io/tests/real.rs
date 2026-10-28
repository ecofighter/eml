//! 本物の driver の時計、待ち、プールの仕事、ストリーム。時間は下限だけを確かめる。

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use eml_io::RealDriver;
use eml_rt::{Completion, IoDriver, Op, OpKey};

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

/// 走っている仕事を持つ driver を落とすと、落とすことが仕事の終わりを待つ。
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
    assert_eq!(completion.result.unwrap(), 42);
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
        assert_eq!(complete(&mut driver, &mut stash, key).result.unwrap(), i);
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
    assert_eq!(complete(&mut driver, &mut stash, write).result.unwrap(), 1);
    assert_eq!(complete(&mut driver, &mut stash, probe).result.unwrap(), 0);
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
    assert_eq!(complete(&mut driver, &mut stash, write).result.unwrap(), 5);
    assert_eq!(complete(&mut driver, &mut stash, probe).result.unwrap(), 0);
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: a,
        buf: raw(&mut room),
    });
    assert_eq!(complete(&mut driver, &mut stash, read).result.unwrap(), 5);
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
    assert_eq!(complete(&mut driver, &mut stash, write).result.unwrap(), 6);
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: b,
        buf: raw(&mut room),
    });
    assert_eq!(complete(&mut driver, &mut stash, read).result.unwrap(), 6);
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
    assert_eq!(complete(&mut driver, &mut stash, probe).result.unwrap(), 0);
    let mut room = [0u8; 16];
    let read = driver.submit(Op::Read {
        handle: a,
        buf: raw(&mut room),
    });
    assert_eq!(complete(&mut driver, &mut stash, read).result.unwrap(), 0);
}

/// 相手が読まない端に書くと、2回目の書きは送りの buffer が空くまで終わらない。`quiesce` は、終わらない読みと書きを
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
