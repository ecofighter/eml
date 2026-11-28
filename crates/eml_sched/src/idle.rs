//! 空きのworkerのインタプリタ。`eml_steps::verdict`を行う (docs/spec/runtime.mdの「判定」)。runのworkerが1つなので、
//! worker 0の眠りは判定だけである。判定の事実は読むが、その待ちはまだ書いていない。

use eml_rt::Control;
use eml_steps::Label;
use eml_steps::verdict::{self, At, Effect, Pc, RootFact, Seen};

use crate::cells::CellRef;
use crate::core::WorkerCore;
use crate::error::RunFault;
use crate::policy::{Point, Policy};
use crate::rank;
use crate::run::{Cause, Phase, STOP_BITS};

/// 判定の終わり。
pub(crate) enum Verdict {
    /// 根のスコープが閉じた。
    RootClosed,
    /// ループに戻る。止まりを記録したなら、ループはそれを次の手順で読む。
    Again,
}

/// worker `core`の判定を、根のスコープ`scope`について終わりまで動かす。どの効果も、行う前に方針に知らせる。
pub(crate) fn sleep<P: Policy>(core: &WorkerCore<P>, scope: CellRef) -> Verdict {
    let k = core.index();
    core.signs().set_phase(k, Phase::Idle);
    let mut pc = Pc {
        at: At::Start,
        virtual_clock: core.run().virtual_clock(),
        stalled: false,
    };
    let mut seen = quiet();
    let verdict = loop {
        let (next, effect) = verdict::step(pc, seen);
        core.at(Point::Kernel(Label::Verdict(effect)));
        match perform(core, scope, effect) {
            Ok(Some(read)) => {
                pc = next;
                seen = read;
            }
            Ok(None) => break Verdict::RootClosed,
            Err(fault) => {
                if let Some(fault) = fault {
                    core.run().record(k, Cause::Fault(fault));
                }
                break Verdict::Again;
            }
        }
    };
    core.signs().set_phase(k, Phase::Run);
    verdict
}

/// 何も読んでいない事実。判定の最初の効果は何も読まない。
fn quiet() -> Seen {
    Seen {
        others_running: false,
        listed: false,
        timer: false,
        ops: false,
        signal_waiters: false,
        root: RootFact::Open,
        inbox: false,
        woken: false,
        mail: false,
        notice: false,
        stop: false,
        holds: false,
        owed: false,
        stalled: false,
    }
}

/// 効果を1つ行い、そこで読んだものを返す。根が閉じたら`Ok(None)`を、判定が終わってループに戻るなら`Err`を返す。
/// `Err(Some(fault))`はrunを終わらせる誤りである。
///
/// 定数の事実は、合わせた席でworkerが1つのrunでは何も立てられないものだけである。`others_running`と`listed`は、ほか
/// のworkerも開いた一覧もないから、`inbox`と`woken`は、知らせを送るほかのworkerがないから、`owed`は、`running`を0
/// にするほかのworkerがないから、`holds`は、メインのworkerがないからである。
fn perform<P: Policy>(
    core: &WorkerCore<P>,
    scope: CellRef,
    effect: Effect,
) -> Result<Option<Seen>, Option<RunFault>> {
    let k = core.index();
    let read = quiet();
    match effect {
        Effect::Count => Ok(Some(Seen {
            others_running: false,
            ..read
        })),
        Effect::Lists => Ok(Some(Seen {
            listed: false,
            ..read
        })),
        Effect::Registry => {
            let mut registry = rank::registry(core.shared());
            let root = match registry.closed(scope) {
                Ok(true) => RootFact::Closed,
                Ok(false) => RootFact::Open,
                Err(fault) => return Err(Some(fault.into())),
            };
            Ok(Some(Seen {
                timer: registry.deadlines().next().is_some(),
                ops: registry.in_flight_all() > 0,
                signal_waiters: registry.relay && registry.signal_bits() != 0,
                root,
                ..read
            }))
        }
        Effect::Own => {
            let bits = core.control().worker_bits(k as usize);
            Ok(Some(Seen {
                mail: core.shared().mail.pending(),
                notice: bits & Control::NOTICE != 0,
                stop: bits & STOP_BITS != 0 || core.run().stopped(k),
                ..read
            }))
        }
        Effect::RootClosed => Ok(None),
        Effect::Deadlock => Err(Some(RunFault::Deadlock)),
        Effect::Restart | Effect::Stop => Err(None),
        // 事実は読むが、それが求める待ちはまだ書いていない
        Effect::Wait | Effect::WaitUntil | Effect::Advance => Err(Some(RunFault::Internal(
            "the verdict asked for a wait this scheduler does not have yet",
        ))),
    }
}
