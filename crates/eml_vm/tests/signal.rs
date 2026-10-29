//! シグナルを届ける道 (docs/spec/vm.md の「タスクとスケジュール」と「上限と割り込み」)。本物の driver の上で流し、
//! テストのスレッドがハンドラの代わりに `Control` を書く。

use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use eml_vm::{Clock, Control, Limits, RunConfig, Sched};

/// 保留のシグナルは監視のスレッドが中継し、待つタスクがいなければ集める地点で run を止める。自己パイプがないので、
/// 監視のスレッドは刻みで保留を見に行く。
#[test]
fn a_relayed_signal_without_a_waiter_stops_the_run() {
    let program = eml_test_support::lir(
        "\
import Async
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> Async.sleep_ms 2000)
",
    );
    let control = Arc::new(Control::new(Limits::default()));
    // `begin_run` はシグナルの語に触れないので、run の前の保留が残る
    control.defer(Control::TERMINATE);
    let config = RunConfig::default()
        .with_clock(Clock::Real)
        .with_sched(Sched::Normal)
        .with_control(Arc::clone(&control));
    // 番犬は失敗の条件にだけ使う。割り込みで止まれば、下の検査が落ちる
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = {
        let control = Arc::clone(&control);
        thread::spawn(move || {
            let start = Instant::now();
            while finished.recv_timeout(Duration::from_millis(1))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                if start.elapsed() >= Duration::from_secs(3) {
                    control.interrupt();
                    return;
                }
            }
        })
    };
    let (host, _) = eml_vm::capture_host(Default::default());
    let result = eml_vm::execute(&program, &config, host);
    drop(done);
    watchdog.join().unwrap();
    let error = result.unwrap_err();
    assert!(error.terminated(), "{error:?}");
    assert_eq!(control.pending_signals(), 0);
}
