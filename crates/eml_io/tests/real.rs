//! 本物の driver の時計と、まだ扱わない操作。

use std::io::ErrorKind;
use std::time::Duration;

use eml_io::RealDriver;
use eml_rt::{IoDriver, Op};

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
fn a_real_driver_without_work_or_deadline_cannot_progress() {
    let mut driver = RealDriver::new().unwrap();
    assert!(!driver.wait(None).unwrap());
}

#[test]
fn a_submitted_op_completes_as_unsupported() {
    let mut driver = RealDriver::new().unwrap();
    let key = driver.submit(Op::Job(Box::new(|| Ok(1))));
    assert_eq!(driver.pending(), 1);
    assert!(driver.wait(None).unwrap());
    let mut out = Vec::new();
    driver.completions(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].key, key);
    assert_eq!(
        out[0].result.as_ref().unwrap_err().kind(),
        ErrorKind::Unsupported
    );
    assert_eq!(driver.pending(), 0);
}

#[test]
fn a_real_driver_has_no_pairs() {
    let mut driver = RealDriver::new().unwrap();
    assert_eq!(driver.pair().unwrap_err().kind(), ErrorKind::Unsupported);
}
