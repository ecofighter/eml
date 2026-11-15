//! 生成したコードが冷たい道で呼ぶサービス (docs/spec/native.md の「安全点」、「巻き戻しの地点」、「externの失敗」)。
#![allow(unsafe_code)]

use eml_rt::{Ctx, Fast, Fault, Stop, Word};
use eml_sched::error::RunFault;

use crate::arch;
use crate::enter::{Fatal, Raise};
use crate::image::{
    ACTION_CONTINUE, ENTRY_SITE, NO_EFFECT, PURITY_EFFECTFUL, PURITY_PURE, RAISE_IO, RAISE_PANIC,
};
use crate::run::WORKER;
use crate::worker::{self, At};

crate::entries! {
    pub(crate) const SERVICES;

    /// 入口の安全点の冷たい側。`function`は呼んだ関数のコードのアドレスで、`ep_helper`はその関数がEPの補助の関数か
    /// どうかである。
    ///
    /// # Safety
    /// `ctx.runtime()`は`worker::of`の前提を満たす`Worker`を指し、`ctx.jmp`は置いてある (`eml_enter`の中である)。
    /// このworkerのスレッドから呼ぶ。`run`が`enter`で入ったコードは、どちらも満たす (docs/spec/native.md の
    /// 「実行の始めと終わり」)。
    pub shim fn eml_nrt_safepoint(ctx, function: i64, ep_helper: i64) -> i64 {
        let _ = ep_helper;
        let at = Some(At { function: function as usize, site: None });
        loop {
            match ctx.poll(0) {
                Ok(()) => break,
                // 知らせではrunを止めない。知らせが立っている間に立った止めるビットを見落とさないよう、もう1度確かめる
                // 下ろすのは`run`が`for_worker`に渡した番号の語である。`Heap::worker()`は論理的なworker IDで、
                // 止める語の添字ではない (docs/spec/object-model.md の「論理的な worker ID」)
                Err(Stop::Notice) => ctx.heap().control().take_notice(WORKER),
                Err(stop) => {
                    // SAFETY: 呼ぶ側の約束。`ctx.runtime()`は`worker::of`の前提を満たす`Worker`を指す
                    return Err(unsafe { worker::record(ctx, RunFault::Stopped(stop), at) });
                }
            }
        }
        if arch::stack_pointer() < ctx.stack_limit() {
            // SAFETY: 上と同じ
            return Err(unsafe { worker::record(ctx, RunFault::Stopped(Stop::DepthLimit), at) });
        }
        Ok::<i64, Fatal>(ACTION_CONTINUE)
    }

    /// 置いた失敗を上げる。`function`は呼んだ関数のコードのアドレス、`site`はその関数の地点の添字か`ENTRY_SITE`で、
    /// `purity`は失敗した行の純粋さ (`PURITY_*`) である。失敗を捕まえるハンドラがあれば、その節の引数を`out`に書いて
    /// 類 (`RAISE_PANIC`か`RAISE_IO`) を返す。書いた語の参照はどれも`out`を通して節へ移り、サービスは何も持たない。
    /// なければ失敗をworkerの構造体に記録して、runを終える (docs/spec/native.md の「externの失敗」)。
    ///
    /// # Safety
    /// `eml_nrt_safepoint`と同じ。さらに、`out`は書ける4語を指す。中断も中断でないyieldも進んでいない。
    pub shim fn eml_nrt_failed(ctx, function: i64, site: i64, purity: i64, out: ptr) -> i64 {
        debug_assert!((PURITY_PURE..=PURITY_EFFECTFUL).contains(&purity), "a purity word is unknown");
        debug_assert!(
            site == ENTRY_SITE || u32::try_from(site).is_ok(),
            "a site word is neither ENTRY_SITE nor an index"
        );
        debug_assert_eq!(ctx.yielding(), 0, "a failure is raised while a yield is in flight");
        let site = match site {
            ENTRY_SITE => None,
            site => u32::try_from(site).ok(),
        };
        let at = Some(At { function: function as usize, site });
        let Some(fault) = ctx.take_fault() else {
            let fault = RunFault::Internal("a failure was raised with nothing parked");
            // SAFETY: 上と同じ
            return Err(unsafe { worker::record(ctx, fault, at) });
        };
        // SAFETY: 上と同じ
        let worker = unsafe { worker::of(ctx) };
        if fault == Fault::Engine && worker.panic.is_none() {
            let fault = RunFault::Internal("a parked panic has no message");
            // SAFETY: 上と同じ
            return Err(unsafe { worker::record(ctx, fault, at) });
        }
        let (panic_effect, io_effect) = worker.raise_effects();
        let class = class(&fault, purity);
        let effect = match class {
            Some(Class::Panic) => panic_effect,
            Some(Class::Io(_)) => io_effect,
            None => NO_EFFECT,
        };
        // `NO_EFFECT`は`u16`に収まらないので、ハンドラを探さない
        let found = match u16::try_from(effect) {
            // SAFETY: `ctx`は生きている
            Ok(number) => unsafe { ctx.ev_find(number) }.map_err(|error| error.park(ctx))?,
            Err(_) => None,
        };
        let (Some(class), Some(ev)) = (class, found) else {
            // SAFETY: 上と同じ
            return Err(unsafe { uncaught(ctx, fault, at) });
        };
        // 組み込みの`Panic`と`IO`の操作はどちらも1つだけである (`load`が確かめる)。evvはevidenceだけを持つ。
        // SAFETY: `ev`は`ev_find`が`dup`したevidenceで、この関数がその参照を持つ
        let clause = unsafe { ctx.ev_clause(ev, 0) }.map_err(|error| error.park(ctx))?;
        let message = match fault {
            // SAFETY: 上と同じ。語を置いたのは`park_panic`で、その参照は`take`でここへ移る
            Fault::Engine => unsafe { worker::of(ctx) }.panic.take().expect("checked above"),
            fault => ctx.fresh_string(&fault.to_string()),
        };
        let out = out.cast_mut();
        // SAFETY: 呼ぶ側の約束により、`out`は書ける4語を指す
        unsafe {
            out.write(clause);
            out.add(1).write(ev);
            match class {
                Class::Panic => {
                    out.add(2).write(message);
                    Ok::<i64, Fatal>(RAISE_PANIC)
                }
                Class::Io(kind) => {
                    out.add(2).write(Word::from_bits(kind as usize));
                    out.add(3).write(message);
                    Ok(RAISE_IO)
                }
            }
        }
    }

    /// 着地の印が中断の宛先のevidenceである地点の冷たい側。ヒープのダミーを返し、着地したフレームはそれを印を持つ
    /// `apply`の結果に束縛する (docs/spec/native.md の「巻き戻しの地点」)。
    ///
    /// # Safety
    /// `eml_nrt_safepoint`と同じ。
    pub shim fn eml_nrt_landed(ctx) -> obj {
        Ok::<Word, Fatal>(ctx.heap().dummy())
    }
}

/// 失敗を捕まえうるハンドラの類。`Io`は`IOErrorKind`のタグを持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Panic,
    Io(i64),
}

/// `purity`の行の失敗`fault`を捕まえうるハンドラの類。VMの`failed`と同じ規則で、`Pure`の行の失敗はどちらも捕まえず、
/// `try_io`は`Effectful`の行の失敗だけを捕まえる。エンジンのpanic (`panic`の行とCの関数の補助) はVMと同じく
/// 純粋さによらず`Panic`である (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
fn class(fault: &Fault, purity: i64) -> Option<Class> {
    let engine = *fault == Fault::Engine;
    if engine || (fault.panics() && purity != PURITY_PURE) {
        Some(Class::Panic)
    } else {
        fault
            .io_error_kind()
            .filter(|_| purity == PURITY_EFFECTFUL)
            .map(Class::Io)
    }
}

/// 捕まえるハンドラのない失敗を`at`の誤りとして記録する。エンジンのpanicは、置いた文言を写して手放す。
///
/// # Safety
/// `worker::of`と同じ。`fault`が`Fault::Engine`なら、workerの構造体は文言を持つ。
unsafe fn uncaught(ctx: &mut Ctx<Fast>, fault: Fault, at: Option<At>) -> Fatal {
    let fault = match fault {
        Fault::Engine => {
            // SAFETY: 呼ぶ側の約束
            let message = unsafe { worker::of(ctx) }.panic.take();
            let message = message.expect("an engine panic has a parked message");
            // SAFETY: 語を置いたのは`park_panic`で、その約束により、語は生きている文字列で、workerの構造体が参照を
            // 1つ持つ。`take`でその参照はここへ移り、`copy_string`が手放す
            match unsafe { ctx.copy_string(message, usize::MAX) } {
                Ok(text) => RunFault::Panicked(text),
                Err(fault) => RunFault::Extern(fault),
            }
        }
        fault => RunFault::Extern(fault),
    };
    // SAFETY: 呼ぶ側の約束
    unsafe { worker::record(ctx, fault, at) }
}

#[cfg(test)]
mod tests {
    use eml_rt::{ArithFault, IoOp, IoReason, Stop};

    use super::*;
    use crate::image::PURITY_PANICS;

    /// `Pure`の行の失敗は、`panic`の類でも`IO`の類でも捕まえない。
    #[test]
    fn a_pure_rows_fault_is_not_caught() {
        let overflow = Fault::Arith(ArithFault::IntegerOverflow);
        let missing = Fault::Io {
            op: IoOp::Open,
            name: "missing".into(),
            reason: IoReason::NotFound,
        };
        assert_eq!(class(&overflow, PURITY_PURE), None);
        assert_eq!(class(&missing, PURITY_PURE), None);
        assert_eq!(class(&overflow, PURITY_PANICS), Some(Class::Panic));
        assert_eq!(class(&missing, PURITY_PANICS), None);
        assert_eq!(
            class(&missing, PURITY_EFFECTFUL),
            Some(Class::Io(eml_rt::externs::IO_NOT_FOUND))
        );
        assert_eq!(
            class(&Fault::Stopped(Stop::MemoryLimit), PURITY_EFFECTFUL),
            None
        );
        assert_eq!(class(&Fault::Engine, PURITY_PURE), Some(Class::Panic));
    }
}
