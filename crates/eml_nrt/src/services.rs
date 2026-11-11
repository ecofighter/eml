//! 生成したコードが冷たい道で呼ぶサービス (docs/spec/native.md の「安全点」、「externの失敗」)。
#![allow(unsafe_code)]

use eml_rt::{Fault, Stop};
use eml_sched::error::RunFault;

use crate::arch;
use crate::enter::Fatal;
use crate::image::{ACTION_CONTINUE, ENTRY_SITE, PURITY_EFFECTFUL, PURITY_PURE};
use crate::run::WORKER;
use crate::worker::{self, At};

crate::entries! {
    pub(crate) const SERVICES;

    /// 入口の安全点の冷たい側。`function`は呼んだ関数のコードのアドレスで、`ep_helper`はその関数がEPの補助の関数か
    /// どうかである。
    ///
    /// # Safety
    /// `ctx`は`run`の`enter`の中にあり (`ctx.runtime()`はそのrunの`Worker`を指し、`ctx.jmp`は置いてある)、
    /// このworkerのスレッドから呼ぶ (docs/spec/native.md の「実行の始めと終わり」)。
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
                    // SAFETY: サービスは`run`が入ったコードからだけ呼ぶ (docs/spec/native.md の「実行の始めと終わり」)
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
    /// `purity`は失敗した行の純粋さ (`PURITY_*`) である。
    ///
    /// # Safety
    /// `eml_nrt_safepoint`と同じ。
    pub shim fn eml_nrt_failed(ctx, function: i64, site: i64, purity: i64) -> i64 {
        // 失敗を捕まえるハンドラはないので、どの失敗もrunを終える
        debug_assert!((PURITY_PURE..=PURITY_EFFECTFUL).contains(&purity), "a purity word is unknown");
        let _ = purity;
        let fault = match ctx.take_fault() {
            // SAFETY: 上と同じ
            Some(Fault::Engine) => match unsafe { worker::of(ctx) }.panic.take() {
                // SAFETY: 語を置いたのは`park_panic`で、その約束により、語は生きている文字列で、workerの構造体が
                // 参照を1つ持つ。`take`でその参照はここへ移る
                Some(message) => match unsafe { ctx.copy_string(message, usize::MAX) } {
                    Ok(text) => RunFault::Panicked(text),
                    Err(fault) => RunFault::Extern(fault),
                },
                None => RunFault::Internal("a parked panic has no message"),
            },
            Some(fault) => RunFault::Extern(fault),
            None => RunFault::Internal("a failure was raised with nothing parked"),
        };
        debug_assert!(
            site == ENTRY_SITE || u32::try_from(site).is_ok(),
            "a site word is neither ENTRY_SITE nor an index"
        );
        let site = match site {
            ENTRY_SITE => None,
            site => u32::try_from(site).ok(),
        };
        let at = Some(At { function: function as usize, site });
        // SAFETY: 上と同じ
        Err::<i64, Fatal>(unsafe { worker::record(ctx, fault, at) })
    }
}
