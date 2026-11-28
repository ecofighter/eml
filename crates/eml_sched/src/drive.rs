//! workerのループ。`eml_steps::plan::next`のただ1つのインタプリタで、`Exec::enter`を呼ぶただ1つのものである
//! (docs/spec/runtime.mdの「workerのループ」)。

use eml_rt::{Control, Ctx, Mode, Word};
use eml_steps::Label;
use eml_steps::plan::{self, Facts, RootState, Seat, Step};

use crate::cells::CellRef;
use crate::core::WorkerCore;
use crate::error::RunFault;
use crate::exec::{Exec, Halt};
use crate::idle::{self, Verdict};
use crate::job::{Item, Root};
use crate::policy::{Point, Policy};
use crate::run::{Cause, NO_CELL, PANICKED, Phase, STOP_BITS};

/// ループが自分で持つ事実。どれも、ループが行った手順だけが書く。
struct Pass {
    root: RootState,
    /// 根のスコープ。`main`が戻ったときに1回だけ読む。
    scope: Option<CellRef>,
    /// 今の通りで、まだ動かしていない項の数。受け箱の手順が書き、`Ready`が減らす。
    left: u64,
    /// 最後の受け箱の手順かループの始めの後に、それぞれの源を見たか。
    detached: bool,
    foreign: bool,
    looked: bool,
}

/// 手順の後にループがすること。
enum Flow {
    Next,
    Leave(Result<(), Halt>),
}

/// worker `this`のループを、戻るまで動かす。`enter`の`Halt`は記録して、変えずに返す。止める事実で出るループは、止まり
/// がまだ記録されていなければ、止めるビットのそれを記録する。
///
/// # Safety
/// `this`はrunのcallbackが受け取ったポインタで、呼ぶのは`sequence::run`である。`Exec`の約束が成り立つ
/// (docs/spec/runtime.mdの「信頼の一覧」のSM-5)。
pub(crate) unsafe fn drive<E: Exec>(this: *mut E) -> Result<(), Halt> {
    // workerは項の間をこのIDで過ごす。新しいヒープでは0で、貸した`Ctx`ではセッションの今のIDである
    // SAFETY: この関数の`# Safety`による。閉包は借用を残さない (SM-5)。
    let (k, control, home) = unsafe {
        E::with(this, |core, ctx| {
            (core.index(), core.control().clone(), ctx.heap().worker())
        })
    };
    let mut pass = Pass {
        root: RootState::Main,
        scope: None,
        left: 0,
        detached: false,
        foreign: false,
        looked: false,
    };
    loop {
        // 手順と、それが記録する止まりは、止める語のこの1回の読みから決める
        let bits = control.worker_bits(k as usize);
        // SAFETY: この関数の`# Safety`による (SM-5)。
        let facts = unsafe {
            E::with(this, |core, _| Facts {
                seat: Seat::Worker0,
                stop: bits & STOP_BITS != 0 || core.run().stopped(k),
                root: pass.root,
                inbox: core.shared().mail.pending() || bits & Control::NOTICE != 0,
                pass: pass.left,
                ready_empty: core.ready_len() == 0,
                own: false,
                detached: pass.detached,
                foreign: pass.foreign,
                looked: pass.looked,
            })
        };
        // SAFETY: この関数の`# Safety`による。
        match unsafe { perform(this, k, home, bits, plan::next(facts), &mut pass) } {
            Flow::Next => {}
            Flow::Leave(result) => return result,
        }
    }
}

/// ループの手順を1つ行う。
///
/// # Safety
/// `drive`と同じ。
unsafe fn perform<E: Exec>(
    this: *mut E,
    k: u32,
    home: u32,
    bits: u32,
    step: Step,
    pass: &mut Pass,
) -> Flow {
    // SAFETY: `drive`の`# Safety`による。`at`は借用を残さない (SM-5)。
    unsafe { E::with(this, |core, _| core.at(Point::Kernel(Label::Loop(step)))) };
    let item = match step {
        Step::Stop => {
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            unsafe { E::with(this, |core, _| core.run().record_bits(k, bits)) };
            return Flow::Leave(Ok(()));
        }
        Step::Return => return Flow::Leave(Ok(())),
        Step::Root(plan::Root::Main) => {
            // SAFETY: `drive`の`# Safety`による。
            let result = unsafe { run_item(this, k, home, Item::Root(Root::Main)) };
            // `main`が根のスコープを置かなかったrunは、`main`で終わる
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            pass.scope = unsafe { E::with(this, |core, _| core.root()) };
            pass.root = match pass.scope {
                Some(_) => RootState::Open,
                None => RootState::None,
            };
            return leave_on_halt(result);
        }
        Step::Root(plan::Root::Result) => {
            pass.root = RootState::Done;
            Item::Root(Root::Result)
        }
        Step::Inbox => {
            // SAFETY: `drive`の`# Safety`による。`ctx`は`with`が貸したこのworkerの`Ctx`である (SM-5)。
            unsafe {
                E::with(this, |core, ctx| {
                    if let Err(fault) = core.take_mail(ctx) {
                        core.run().record(k, Cause::Fault(fault));
                    }
                    pass.left = core.ready_len() as u64;
                })
            };
            pass.detached = false;
            pass.foreign = false;
            pass.looked = false;
            return Flow::Next;
        }
        Step::Ready => {
            pass.left -= 1;
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            match unsafe { E::with(this, |core, _| core.take_ready()) } {
                Some(entry) => Item::Entry(entry),
                None => {
                    // SAFETY: `drive`の`# Safety`による (SM-5)。
                    unsafe {
                        E::with(this, |core, _| {
                            core.run().record(
                                k,
                                Cause::Fault(RunFault::Internal("a ready pass outlives its queue")),
                            )
                        })
                    };
                    return Flow::Next;
                }
            }
        }
        // 仕事の一覧も、ほかのworkerも、driverもないので、見る源は空である
        Step::Detached => {
            pass.detached = true;
            return Flow::Next;
        }
        Step::Foreign => {
            pass.foreign = true;
            return Flow::Next;
        }
        Step::Look => {
            pass.looked = true;
            return Flow::Next;
        }
        Step::Idle => {
            let Some(scope) = pass.scope else {
                // SAFETY: `drive`の`# Safety`による (SM-5)。
                unsafe {
                    E::with(this, |core, _| {
                        core.run().record(
                            k,
                            Cause::Fault(RunFault::Internal(
                                "the loop is idle without a root scope",
                            )),
                        )
                    })
                };
                return Flow::Next;
            };
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            let verdict = unsafe { E::with(this, |core, _| idle::sleep(core, scope)) };
            if let Verdict::RootClosed = verdict {
                pass.root = RootState::Closed;
            }
            return Flow::Next;
        }
    };
    // SAFETY: `drive`の`# Safety`による。
    leave_on_halt(unsafe { run_item(this, k, home, item) })
}

fn leave_on_halt(result: Result<(), Halt>) -> Flow {
    match result {
        Ok(()) => Flow::Next,
        Err(halt) => Flow::Leave(Err(halt)),
    }
}

/// 項を開き、`enter`で動かし、閉じる。`enter`の`Halt`は記録して返す。`Halt::Stopped`の後はworkerのIDに戻るだけで、
/// `Halt::Panicked`の後は`Ctx`に触れない。
///
/// # Safety
/// `drive`と同じ。
unsafe fn run_item<E: Exec>(this: *mut E, k: u32, home: u32, item: Item) -> Result<(), Halt> {
    let (id, current) = match &item {
        Item::Root(_) => (home, NO_CELL),
        Item::Entry(entry) => (entry.worker, entry.cell.encode() as u64),
    };
    // SAFETY: `drive`の`# Safety`による。`ctx`は`with`が貸したこのworkerの`Ctx`である (SM-5)。
    unsafe { E::with(this, |core, ctx| open(core, ctx, id, current)) };
    // SAFETY: `drive`の`# Safety`による。呼び出しをまたいで、`this`から導いたものは持たない (SM-5)。
    let result = unsafe { E::enter(this, item) };
    match result {
        // SAFETY: `drive`の`# Safety`による (SM-5)。
        Ok(()) => unsafe { E::with(this, |core, ctx| close(core, ctx, home)) },
        Err(halt) => {
            let cause = match halt {
                Halt::Stopped => Cause::Halt,
                Halt::Panicked => PANICKED,
            };
            // SAFETY: `drive`の`# Safety`による (SM-5)。`Halt::Panicked`の後は核にだけ触れる。`Halt::Stopped`の後は
            // ヒープが操作の間にあるので (SM-5)、`Ctx`の不変条件が成り立つ。入れる語は即値である。`evv`が`()`なら、
            // 切り替えた文脈が求めるのはyieldの語だけで、それは不変条件がもう満たしており、どれもworkerを読まない。
            // 戻る語は止まった項の`evv`で、放さない。タスクのIDの節をworkerのIDの下で放すと検査モードでは
            // `WrongOwner`になり、止まったrunのヒープは回復が取り替えるか落とすからである
            // (docs/spec/vm.mdの「`par` の fork と join」と同じ)。
            unsafe {
                E::with(this, |core, ctx| {
                    core.signs().set_current(k, NO_CELL);
                    core.run().record(k, cause);
                    if halt == Halt::Stopped {
                        let _abandoned = ctx.switch_task(Word::UNIT, home, 0);
                    }
                })
            };
        }
    }
    result
}

/// 項の始め。項は空の`evv`で、yieldの途中でなく、置いた失敗なしに始まる。これはどのビルドでも確かめる。速いモードの
/// `switch_task`は、切り替えた後の文脈で`Ctx`の持つ語を新しいIDが持つことを求め、空の`evv`とyieldの途中でないことが
/// それを満たすからである。
fn open<M: Mode, P: Policy>(core: &WorkerCore<P>, ctx: &mut Ctx<M>, id: u32, current: u64) {
    assert!(
        ctx.evv_word() == Word::UNIT,
        "an item opens with a non-empty evidence vector"
    );
    assert_eq!(ctx.yielding(), 0, "an item opens with a yield in flight");
    assert!(
        ctx.take_fault().is_none(),
        "an item opens with a parked fault"
    );
    let k = core.index();
    core.signs().set_current(k, current);
    core.signs().set_phase(k, Phase::Run);
    // SAFETY: `ctx`はこのworkerのスレッドの`Ctx`で、`with`が貸したものである (SM-5)。入れるのは即値で、確かめたとおり
    // 戻る`evv`は即値の`()`、yieldの状態の語は`()`なので、`switch_task`の`# Safety`が成り立つ。
    let _unit = unsafe { ctx.switch_task(Word::UNIT, id, 0) };
}

/// 項の終わり。workerのIDに戻り、ほかのスレッドから受けた解放を取り込む。戻る前の確かめは`open`と同じ理由による。
fn close<M: Mode, P: Policy>(core: &WorkerCore<P>, ctx: &mut Ctx<M>, home: u32) {
    assert!(
        ctx.evv_word() == Word::UNIT,
        "an item closes with a non-empty evidence vector"
    );
    assert_eq!(ctx.yielding(), 0, "an item closes with a yield in flight");
    core.signs().set_current(core.index(), NO_CELL);
    // SAFETY: `open`と同じ。`with`が貸した`Ctx`で (SM-5)、即値の`evv`を即値に替え、yieldの途中ではない。
    let _unit = unsafe { ctx.switch_task(Word::UNIT, home, 0) };
    ctx.reclaim_if_due();
}
