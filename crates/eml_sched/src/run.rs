//! 1つのrunのworkerが分け合う状態 (docs/spec/runtime.mdの「もの」「runの手順」)。1つのrunの間だけある。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use eml_rt::{Control, Stop, Word};

use crate::cells::{CellRef, Post, Registry, Shared, Woken};
use crate::error::RunFault;
use crate::rank::{self, Ranked};

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
    /// runの登録表とそのmail。
    shared: Arc<Shared>,
    /// 最初に記録した止まりが勝つ。
    stop: Ranked<Option<Stopped>>,
    /// workerごとの、そのworkerが止まりを記録したか。記録したworkerのループだけが、これを止める事実として読む。
    recorded: Box<[AtomicBool]>,
    /// runの時計が仮想の時計か。runの間変わらない。
    virtual_clock: bool,
}

impl Run {
    pub(crate) fn new(workers: usize, virtual_clock: bool) -> Run {
        Run {
            shared: Arc::new(Shared::new(Registry::new(Vec::new(), workers))),
            stop: Ranked::new(None),
            recorded: (0..workers).map(|_| AtomicBool::new(false)).collect(),
            virtual_clock,
        }
    }

    pub(crate) fn virtual_clock(&self) -> bool {
        self.virtual_clock
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// 止まりがまだ記録されていなければ、`cause`を記録する。
    pub(crate) fn record(&self, worker: u32, cause: Cause) {
        let mut stop = self.stop.hold();
        if stop.is_none() {
            *stop = Some(Stopped { worker, cause });
        }
        self.recorded[worker as usize].store(true, Ordering::Relaxed);
    }

    /// worker `worker`が止まりを記録した。そのworkerは次の手順でループを出る。ほかのworkerはこれを読まず、ENDで出る
    /// (docs/spec/runtime.mdの「workerのループ」)。
    pub(crate) fn stopped(&self, worker: u32) -> bool {
        self.recorded[worker as usize].load(Ordering::Relaxed)
    }

    /// ループが出た理由を、止まりがまだ記録されていなければ記録する。`bits`は、ループが`Stop`の手順のために読んだ止める
    /// 語である。順は`eml_rt`の`stop_reason`と同じで、語は読み直さない。その間にほかのスレッドが語を変えうるからである。
    /// ENDだけでは何も記録しない。ENDは理由にならない。立てた者が理由を記録したか、runが成功したかのどちらかだからで
    /// ある (docs/spec/runtime.mdの「workerのループ」)。
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

/// `Signs::current`の、項を動かしていない値。セルの番号の即値は、使い切ったgenerationを持たないので、これにならない。
pub const NO_CELL: u64 = u64::MAX;

/// workerが何をしているか (docs/spec/runtime.mdの「フック」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    /// 項かループを動かす。
    Run,
    /// 空きで、眠った形にある。worker 0では判定である。
    Idle,
    /// driverの操作を待つ。
    Io,
}

/// workerごとのフックの値。runの外でも読めるように、自分の`Arc`に置く。読み手が届くのは、workerごとに2つのアトミックな
/// 値だけで、ヒープの語には届かない。スケジューラのどの判断もこれを読まない。
pub struct Signs {
    signs: Box<[Sign]>,
}

struct Sign {
    current: AtomicU64,
    phase: AtomicU8,
}

impl Signs {
    pub(crate) fn new(workers: usize) -> Signs {
        Signs {
            signs: (0..workers)
                .map(|_| Sign {
                    current: AtomicU64::new(NO_CELL),
                    phase: AtomicU8::new(Phase::Run as u8),
                })
                .collect(),
        }
    }

    /// worker `k`が動かしている項。タスクのセルの番号の即値か、`NO_CELL`である。
    pub fn current(&self, k: u32) -> u64 {
        self.signs[k as usize].current.load(Ordering::Relaxed)
    }

    pub fn phase(&self, k: u32) -> Phase {
        match self.signs[k as usize].phase.load(Ordering::Relaxed) {
            0 => Phase::Run,
            1 => Phase::Idle,
            _ => Phase::Io,
        }
    }

    pub(crate) fn set_current(&self, k: u32, current: u64) {
        self.signs[k as usize]
            .current
            .store(current, Ordering::Relaxed);
    }

    pub(crate) fn set_phase(&self, k: u32, phase: Phase) {
        self.signs[k as usize]
            .phase
            .store(phase as u8, Ordering::Relaxed);
    }
}

/// worker `from`の`Here`の宛先。`cells`は登録表を持ったまま、ほかのworkerに知らせを送るときにだけ呼ぶ。runのworker
/// は1つなので、どの呼び出しも内部の誤りである。置く閉包の語には触れず、誤りで終わったrunの語として残す。
pub(crate) struct Mailer {
    run: Arc<Run>,
    from: u32,
}

impl Mailer {
    pub(crate) fn new(run: Arc<Run>, from: u32) -> Mailer {
        Mailer { run, from }
    }

    fn unsent(&self) {
        let _registry = rank::under_registry();
        self.run.record(
            self.from,
            Cause::Fault(RunFault::Internal(
                "a notice is sent to another worker of a run of one worker",
            )),
        );
    }
}

impl Post for Mailer {
    fn wake(&self, _k: u32, _w: Woken) {
        self.unsent();
    }

    fn cancel(&self, _k: u32, _cell: CellRef) {
        self.unsent();
    }

    fn place(&self, _k: u32, _cell: CellRef, _worker: u32, _closure: Word) {
        self.unsent();
    }

    fn submit(&self, _k: u32, _op: CellRef) {
        self.unsent();
    }

    fn cancel_io(&self, _k: u32, _op: CellRef) {
        self.unsent();
    }

    fn completion(&self, _k: u32, _op: CellRef) {
        self.unsent();
    }
}
