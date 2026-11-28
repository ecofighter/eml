//! workerのループ。`eml_steps::plan::next`のただ1つのインタプリタで、`Exec::enter`を呼ぶただ1つのものである
//! (docs/spec/runtime.mdの「workerのループ」)。

use eml_steps::Label;
use eml_steps::plan::{self, Facts, RootState, Seat, Step};

use crate::error::RunFault;
use crate::exec::{Exec, Halt};
use crate::job::{Item, Root};
use crate::policy::Point;
use crate::run::{Cause, PANICKED, STOP_BITS};

/// 手順の後にループがすること。
enum Flow {
    Next,
    Leave(Result<(), Halt>),
}

/// worker `this`のループを、戻るまで動かす。`enter`の`Halt`は記録して、変えずに返す。止めるビットで出るループは、止ま
/// りがまだ記録されていなければそれを記録する。
///
/// # Safety
/// `this`はrunのcallbackが受け取ったポインタで、呼ぶのは`sequence::run`である。`Exec`の約束が成り立つ
/// (docs/spec/runtime.mdの「信頼の一覧」のSM-5)。
pub(crate) unsafe fn drive<E: Exec>(this: *mut E) -> Result<(), Halt> {
    // SAFETY: この関数の`# Safety`による。閉包は借用を残さない (SM-5)。
    let (k, control) = unsafe { E::with(this, |core, _| (core.index(), core.control().clone())) };
    let mut root = RootState::Main;
    loop {
        // 手順と、それが記録する止まりは、止める語のこの1回の読みから決める
        let bits = control.worker_bits(k as usize);
        let facts = Facts {
            seat: Seat::Worker0,
            stop: bits & STOP_BITS != 0,
            root,
            inbox: false,
            pass: 0,
            ready_empty: true,
            own: false,
            detached: false,
            foreign: false,
            looked: false,
        };
        // SAFETY: この関数の`# Safety`による。
        match unsafe { perform(this, k, bits, plan::next(facts), &mut root) } {
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
    bits: u32,
    step: Step,
    root: &mut RootState,
) -> Flow {
    // SAFETY: `drive`の`# Safety`による。`at`は借用を残さない (SM-5)。
    unsafe { E::with(this, |core, _| core.at(Point::Kernel(Label::Loop(step)))) };
    match step {
        Step::Stop => {
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            unsafe { E::with(this, |core, _| core.run().record_bits(k, bits)) };
            Flow::Leave(Ok(()))
        }
        Step::Root(plan::Root::Main) => {
            *root = RootState::None;
            // SAFETY: `drive`の`# Safety`による。呼び出しをまたいで、`this`から導いたものは持たない (SM-5)。
            match unsafe { E::enter(this, Item::Root(Root::Main)) } {
                Ok(()) => Flow::Next,
                Err(halt) => {
                    let cause = match halt {
                        Halt::Stopped => Cause::Halt,
                        Halt::Panicked => PANICKED,
                    };
                    // SAFETY: `drive`の`# Safety`による。触れるのは核だけで、panicした`Halt`が残したヒープには触れな
                    // い (SM-5)。
                    unsafe { E::with(this, |core, _| core.run().record(k, cause)) };
                    Flow::Leave(Err(halt))
                }
            }
        }
        Step::Return => Flow::Leave(Ok(())),
        Step::Root(plan::Root::Result)
        | Step::Inbox
        | Step::Ready
        | Step::Detached
        | Step::Foreign
        | Step::Look
        | Step::Idle => {
            // SAFETY: `drive`の`# Safety`による (SM-5)。
            unsafe {
                E::with(this, |core, _| {
                    core.run().record(
                        k,
                        Cause::Fault(RunFault::Internal(
                            "the loop reached a step it does not take",
                        )),
                    )
                })
            };
            Flow::Leave(Ok(()))
        }
    }
}
