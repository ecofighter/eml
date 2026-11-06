//! stdの行のプールの仕事。仕事はdriverのブロックする呼び出しのプールが走らせるRustの処理で、入力をすべて値で持ち、
//! 結果を値で返すので、プールはヒープに触れない。受け付けの行が仕事を`submit`に渡し、取り出しの行が`take`で結果を
//! 受け取る (docs/spec/vm.md の「プールの仕事」)。

use std::any::Any;
use std::io;
use std::panic::{self, AssertUnwindSafe};

use eml_rt::{Ctx, Mode, Op};

use crate::cells::{self, JobSource, Shared};
use crate::error::VmFault;
use crate::op::Slot;
use crate::sched::Scheduler;
use crate::vm::{Exec, Task, current_cell, here};

/// プールの仕事の取り出しの行が受け取るもの。
pub(crate) enum Outcome<T> {
    /// 仕事が最後まで走り、値を返した。
    Woke(T),
    /// driverが仕事を走らせられなかったか、仕事がpanicした。
    Failed(io::Error),
    /// タスクが取り消された。仕事が返した値はもう落としてある。
    Cancelled,
}

/// 仕事`job`を記録のhostのdriverのプールに出し、tokenを`dst`に書く。制約が`Ctx`、タスク、scheduler、登録表を
/// 借りることを拒むので、仕事は捕まえるものをすべて持つ。受け付けは取り消しの地点で、投げるなら仕事を走らせずに
/// 落とす (docs/spec/vm.md の「プールの仕事」)。
pub(crate) fn submit<R: Exec, T: Send + 'static>(
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    from: JobSource,
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<(), VmFault> {
    let op = Op::Job(Box::new(move || answer(job)));
    submit_op::<R>(ctx, task, sched, dst, from, op)
}

/// 作った仕事`op`を記録のhostのdriverのプールに出し、tokenを`dst`に書く。stdのプールの仕事も`blocking`のCの
/// 呼び出しもここから出すので、受け付けの手順は1か所にある。受け付けは取り消しの地点で、投げるなら`op`を走らせずに
/// 落とす (docs/spec/vm.md の「プールの仕事」)。
pub(crate) fn submit_op<R: Exec>(
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    from: JobSource,
    op: Op,
) -> Result<(), VmFault> {
    let me = current_cell(task)?;
    // 仕事は本物の時計のrunでは自分のdriverのプールで走る (docs/spec/vm.md の「時計と driver」)
    sched.own_driver()?;
    here!(sched, post, here);
    let token = cells::submit_job(
        &sched.cells,
        &here,
        ctx,
        sched.driver.as_deref_mut(),
        task.worker,
        task.depth_offset,
        me,
        from,
        op,
    )?;
    task.set_raw(dst, token);
    Ok(())
}

/// プールのスレッドで仕事を走らせ、結果を箱に入れる。箱はいつも`Result<T, String>`を持ち、中身は値か、捕まえた
/// panicの文言である。外のcrateのコード (rustls、OSの検証器) はpanicしうるが、panicはプロセスを終わらせずに、この
/// 操作だけを失敗させる。
fn answer<T: Send + 'static>(job: impl FnOnce() -> T) -> Box<dyn Any + Send> {
    let result: Result<T, String> =
        panic::catch_unwind(AssertUnwindSafe(job)).map_err(|payload| panic_message(&*payload));
    Box::new(result)
}

/// 捕まえたpanicの文言。文を持つのは`&str`と`String`の中身だけである。
fn panic_message(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
    match text {
        Some(text) => format!("a job panicked: {text}"),
        None => "a job panicked".into(),
    }
}

/// プールの仕事の記録を放し、結果を`Wake`で分ける。ほかの出どころの記録、`Woke`と`Cancelled`のほかの`Wake`、違う型の
/// 答えはVMの誤りである。取り消されたタスクは、値を受ける束縛がないので、持ち主のスレッドでここで値を落とす
/// (docs/spec/vm.md の「プールの仕事」)。
pub(crate) fn take<R: Exec, T: Send + 'static>(
    ctx: &mut Ctx<R::M>,
    task: &Task,
    shared: &Shared,
    token: i64,
    wake: i64,
    from: JobSource,
) -> Result<Outcome<T>, VmFault> {
    take_on(ctx, shared, token, task.worker, wake, from)
}

/// 論理的なworker ID`worker`のタスクの`take`。
fn take_on<M: Mode, T: Send + 'static>(
    ctx: &mut Ctx<M>,
    shared: &Shared,
    token: i64,
    worker: u32,
    wake: i64,
    from: JobSource,
) -> Result<Outcome<T>, VmFault> {
    let taken = cells::take_job(shared, ctx, token, worker)?;
    if taken.from != from {
        return Err(VmFault::Internal("an op is taken by another function"));
    }
    let cancelled = match wake {
        cells::WOKE => false,
        cells::CANCELLED => true,
        _ => return Err(VmFault::Internal("an op is taken with another wake")),
    };
    let answer = taken.answer::<Result<T, String>>()?;
    if cancelled {
        drop(answer);
        return Ok(Outcome::Cancelled);
    }
    Ok(match answer {
        Ok(Ok(value)) => Outcome::Woke(value),
        Ok(Err(panicked)) => Outcome::Failed(io::Error::other(panicked)),
        Err(e) => Outcome::Failed(e),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use eml_io::TestDriver;
    use eml_rt::{Checked, IoDriver};

    use super::*;
    use crate::cells::{Registry, running_task, solo, test_ctx};

    /// workerが1つのrunのworker 0で仕事`job`を受け付け、テストのdriverで決着させて、tokenを返す。
    fn settled(registry: &Shared, ctx: &mut Ctx<Checked>, job: Op) -> i64 {
        let scope = registry.lock().open_root(None, None).unwrap();
        let me = running_task(registry, scope);
        let mut driver = TestDriver::new(None);
        let token = cells::submit_job(
            registry,
            &solo(registry),
            ctx,
            Some(&mut driver),
            0,
            0,
            me,
            JobSource::C(3),
            job,
        )
        .unwrap();
        driver.wait(None).unwrap();
        cells::complete(
            registry,
            &solo(registry),
            ctx,
            &mut driver,
            0,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();
        token
    }

    /// 終わった仕事は、返した値で起きる。
    #[test]
    fn a_finished_job_wakes_with_its_value() {
        let mut ctx = test_ctx();
        let registry = Shared::new(Registry::new(Vec::new(), 1));
        let token = settled(&registry, &mut ctx, Op::Job(Box::new(|| answer(|| 42u64))));
        assert!(matches!(
            take_on::<_, u64>(&mut ctx, &registry, token, 0, cells::WOKE, JobSource::C(3)),
            Ok(Outcome::Woke(42))
        ));
    }

    /// `()`を返す仕事を`u64`として取り出すのは、箱が違う型を持つので、内部の誤りである。
    #[test]
    fn a_unit_job_taken_as_u64_is_an_internal_error() {
        let mut ctx = test_ctx();
        let registry = Shared::new(Registry::new(Vec::new(), 1));
        let token = settled(&registry, &mut ctx, Op::Job(Box::new(|| answer(|| ()))));
        assert!(matches!(
            take_on::<_, u64>(&mut ctx, &registry, token, 0, cells::WOKE, JobSource::C(3)),
            Err(VmFault::Internal(
                "an op completes with another kind of result"
            ))
        ));
        let leaks = ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// panicした仕事は、そのpanicの文を持って失敗する。文を持たない中身では、文言だけになる。
    #[test]
    fn a_panicking_job_fails_with_the_panic_text() {
        type Job = fn() -> u64;
        let jobs: [(Job, &str); 3] = [
            (|| panic!("boom"), "a job panicked: boom"),
            (
                || panic!("{} {}", "no", "roots"),
                "a job panicked: no roots",
            ),
            (|| panic::panic_any(7i32), "a job panicked"),
        ];
        for (job, message) in jobs {
            let mut ctx = test_ctx();
            let registry = Shared::new(Registry::new(Vec::new(), 1));
            let token = settled(&registry, &mut ctx, Op::Job(Box::new(move || answer(job))));
            let outcome =
                take_on::<_, u64>(&mut ctx, &registry, token, 0, cells::WOKE, JobSource::C(3));
            let Ok(Outcome::Failed(e)) = outcome else {
                panic!("expected a failure");
            };
            assert_eq!(e.kind(), io::ErrorKind::Other);
            assert_eq!(e.to_string(), message);
        }
    }

    /// 落とされたことを覚える値。
    struct Flag(Arc<AtomicBool>);

    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// 取り消された取り出しは、仕事が返した値を落とし、返さない。
    #[test]
    fn a_cancelled_take_drops_the_value() {
        let mut ctx = test_ctx();
        let registry = Shared::new(Registry::new(Vec::new(), 1));
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = Flag(Arc::clone(&dropped));
        let token = settled(
            &registry,
            &mut ctx,
            Op::Job(Box::new(move || answer(|| flag))),
        );
        assert!(!dropped.load(Ordering::SeqCst));
        let outcome = take_on::<_, Flag>(
            &mut ctx,
            &registry,
            token,
            0,
            cells::CANCELLED,
            JobSource::C(3),
        );
        assert!(matches!(outcome, Ok(Outcome::Cancelled)));
        assert!(dropped.load(Ordering::SeqCst));
    }

    /// ほかの出どころの記録は内部の誤りである。
    #[test]
    fn a_job_of_another_source_is_an_internal_error() {
        let mut ctx = test_ctx();
        let registry = Shared::new(Registry::new(Vec::new(), 1));
        let token = settled(&registry, &mut ctx, Op::Job(Box::new(|| answer(|| 1u64))));
        assert!(matches!(
            take_on::<_, u64>(&mut ctx, &registry, token, 0, cells::WOKE, JobSource::C(4)),
            Err(VmFault::Internal("an op is taken by another function"))
        ));
    }
}
