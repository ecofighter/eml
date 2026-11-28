//! 合わせた席で1つのworkerを動かすループとrunの手順 (docs/spec/runtime.mdの「workerのループ」「runの手順」)。

// SAFETY: このファイルの`unsafe`の呼び出しは、項のヒープの操作と`support`の呼び出しである。ヒープの操作は、項が借りた
// `Ctx`の上で、項が自分で確保した語に対して行う。`WorkerCore`を通して置く語はない。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use eml_rt::{Checked, Control, Ctx, Stop, Word};
use eml_sched::error::RunFault;
use eml_sched::exec::{Env, Halt, Place, Workers};
use eml_sched::job::{Item, Root};
use eml_sched::policy::InOrder;
use eml_sched::run::{Cause, Stopped};
use eml_sched::script::Script;

use crate::support::{control, one, run};

#[test]
fn a_synchronous_main_runs_once() {
    let calls = AtomicU32::new(0);
    let scripts = one(|_, _, item| {
        assert_eq!(item, Item::Root(Root::Main));
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    });
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(outcome.stop, None);
    assert_eq!(outcome.exit, None);
    assert_eq!(outcome.retired.len(), 1);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// 項が核と`Ctx`に届くのは`enter`が貸す借用を通してだけで、項は両方を使う。`drive`が`enter`をまたいで参照を持っていれ
/// ば、Miriの下ではこれらの借用がその参照を無効にする (docs/spec/runtime.mdの「信頼の一覧」のSM-5)。
#[test]
fn enter_gets_the_only_borrow() {
    let scripts = one(|core, ctx, _| {
        assert_eq!(core.index(), 0);
        assert_eq!(core.control().worker_bits(0) & Control::END, 0);
        // SAFETY: 語はこの項自身の物体で、1回だけ放す。
        unsafe {
            let w = ctx.heap_mut().alloc_int(i64::MAX);
            ctx.heap_mut().decref(w).unwrap();
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    let mut ctx = outcome.retired.into_iter().next().unwrap().retired;
    assert!(ctx.leaks().is_empty());
}

#[test]
fn a_halt_from_main_ends_the_run() {
    let scripts = one(|_, _, _| Err(Halt::Stopped));
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(
        outcome.stop,
        Some(Stopped {
            worker: 0,
            cause: Cause::Halt
        })
    );
    assert_eq!(outcome.retired.len(), 1);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// mainが残した物体は、戻ってきた`Ctx`の中にある。つまり、それは項が借りた`Ctx`である。
#[test]
fn the_outcome_carries_the_retired_ctx() {
    let kept = Mutex::new(None::<Word>);
    let scripts = one(|_, ctx, _| {
        // SAFETY: 物体は1回の呼び出しで作り、テストがそのただ1つの参照を持つ。
        let w = unsafe { ctx.heap_mut() }.alloc_int(i64::MAX);
        *kept.lock().unwrap() = Some(w);
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    let mut retired = outcome.retired;
    assert_eq!(retired.len(), 1);
    let retiree = retired.pop().unwrap();
    assert_eq!(retiree.place, Place::Worker(0));
    let mut ctx: Ctx<Checked> = retiree.retired;
    assert_eq!(ctx.leaks().into_iter().collect::<Vec<_>>(), [("BigInt", 1)]);
    let w = kept.lock().unwrap().take().unwrap();
    // SAFETY: `w`は、mainがこのヒープに確保して残した物体で、1回だけ放す。
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    assert!(ctx.leaks().is_empty());
}

/// `run`を呼ばずに戻るfactory。runはdocs/spec/runtime.mdの「runの手順」の誤りで終わり、ENDは下りる。
#[test]
fn a_factory_that_never_runs_ends_the_run() {
    struct Never;
    impl Workers for Never {
        type Exec = Script<'static, Checked, InOrder>;
        type Retired = ();
        fn policy(&self, _: Place) -> InOrder {
            InOrder
        }
        unsafe fn on_worker(
            &self,
            _: Env<'_, InOrder>,
            _: impl FnOnce(*mut Self::Exec) -> Result<(), Halt>,
        ) {
        }
    }
    let control = control(1);
    let outcome = run(&Never, &control);
    assert_eq!(
        outcome.stop,
        Some(Stopped {
            worker: 0,
            cause: Cause::Fault(RunFault::Internal("a worker left without running"))
        })
    );
    assert_eq!(outcome.retired.len(), 1);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// runの前に立てた止めるビット。ループは最初の手順で出て、mainは動かず、止まりが記録される。
#[test]
fn a_stop_bit_before_main_leaves_main_unrun() {
    let calls = AtomicU32::new(0);
    let scripts = one(|_, _, _| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    });
    let control = control(1);
    control.interrupt();
    let outcome = run(&scripts, &control);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        outcome.stop,
        Some(Stopped {
            worker: 0,
            cause: Cause::Fault(RunFault::Stopped(Stop::Interrupted))
        })
    );
    assert_eq!(control.worker_bits(0) & Control::END, 0);
    control.clear();
}

/// INTERRUPT、TERMINATE、MEMORYのうちこの順で最初に立っているものが、止まりを名指す。
#[test]
fn the_first_stop_bit_in_order_names_the_stop() {
    let terminate: fn(&Control) = Control::terminate;
    let interrupt: fn(&Control) = Control::interrupt;
    for (raise, stop) in [
        (vec![terminate], Stop::Terminated),
        (vec![terminate, interrupt], Stop::Interrupted),
    ] {
        let scripts = one(|_, _, _| Ok(()));
        let control = control(1);
        for raise in raise {
            raise(&control);
        }
        let outcome = run(&scripts, &control);
        assert_eq!(
            outcome.stop,
            Some(Stopped {
                worker: 0,
                cause: Cause::Fault(RunFault::Stopped(stop))
            })
        );
        control.clear();
    }
}

/// ENDは理由にならない。立てた者が理由を記録したか、runが成功したかのどちらかである。ENDを立てて戻る項は、runに止まり
/// を残さない。
#[test]
fn end_alone_is_not_a_stop() {
    let scripts = one(|core, _, _| {
        core.control().end_run();
        Ok(())
    });
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(outcome.stop, None);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}
