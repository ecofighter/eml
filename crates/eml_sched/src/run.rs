//! 1つのrunのworkerが分け合う状態 (docs/spec/runtime.mdの「もの」「runの手順」)。1つのrunの間だけある。

use eml_rt::{Control, Stop};

use crate::error::RunFault;
use crate::rank::Ranked;

/// runが止まった理由と、それを最初に記録したworker。
#[derive(Debug, PartialEq, Eq)]
pub struct Stopped {
    pub worker: u32,
    pub cause: Cause,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cause {
    /// エンジンが`Halt::Stopped`を知らせた。誤りはそのworkerの`Retired`が持つ。
    Halt,
    /// スケジューラ自身の理由。
    Fault(RunFault),
}

/// workerにループを出させるビット。`NOTICE`は知らせを取るよう求めるだけである。
pub(crate) const STOP_BITS: u32 =
    Control::INTERRUPT | Control::TERMINATE | Control::MEMORY | Control::END;

/// ループがpanicしたworkerが記録するもの。ループとrunのcallbackの両方が書く。
pub(crate) const PANICKED: Cause = Cause::Fault(RunFault::Internal("a worker thread panicked"));

pub(crate) struct Run {
    /// 最初に記録した止まりが勝つ。
    stop: Ranked<Option<Stopped>>,
}

impl Run {
    pub(crate) fn new() -> Run {
        Run {
            stop: Ranked::new(None),
        }
    }

    /// 止まりがまだ記録されていなければ、`cause`を記録する。
    pub(crate) fn record(&self, worker: u32, cause: Cause) {
        let mut stop = self.stop.hold();
        if stop.is_none() {
            *stop = Some(Stopped { worker, cause });
        }
    }

    /// ループが出た理由を、止まりがまだ記録されていなければ記録する。`bits`は、ループが`Stop`の手順のために読んだ止め
    /// る
    /// 語である。順は`eml_rt`の`stop_reason`と同じで、語は読み直さない。その間にほかのスレッドが語を変えうるからであ
    /// る。
    /// ENDだけでは何も記録しない。ENDは理由にならない。立てた者が理由を記録したか、runが成功したかのどちらかだからで
    /// ある
    /// (docs/spec/runtime.mdの「workerのループ」)。
    pub(crate) fn record_bits(&self, worker: u32, bits: u32) {
        let stop = if bits & Control::INTERRUPT != 0 {
            Stop::Interrupted
        } else if bits & Control::TERMINATE != 0 {
            Stop::Terminated
        } else if bits & Control::MEMORY != 0 {
            Stop::MemoryLimit
        } else {
            return;
        };
        self.record(worker, Cause::Fault(RunFault::Stopped(stop)));
    }

    pub(crate) fn take_stop(&self) -> Option<Stopped> {
        self.stop.hold().take()
    }
}
