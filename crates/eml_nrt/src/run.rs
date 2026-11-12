//! 実行の駆動。workerのスレッド、実行の始めと終わり (docs/spec/native.md の「実行の始めと終わり」) と、スタックの形
//! (docs/spec/native.md の「安全点」) を持つ。
#![allow(unsafe_code)]

use std::panic::resume_unwind;
use std::sync::Arc;

use eml_rt::{Control, Ctx, Fast, HostRef, Word};
use eml_sched::config::{Finished, RunConfig, RunStats};
use eml_sched::error::{RunFault, RuntimeError};

use crate::arch;
use crate::census::census;
use crate::constants::{self, Datum};
use crate::enter::{FATAL_FAULT, FATAL_PANICKED, FATAL_RECORDED, Fatal, detached, enter};
use crate::image::{
    Census, DATUM_BIG_INT, DATUM_FLOAT, DATUM_STRING, DatumRow, Image, Options, Start,
};
use crate::worker::{NATIVE_CODE, Worker};

/// workerのスタックに予約するバイト数。予約するだけで、ページはスタックが伸びるにつれて確定する。
pub const WORKER_STACK: usize = 1 << 30;
/// スタックの最も低いアドレス (`base`) から`hard`までのバイト数。
pub const STACK_HARD_MARGIN: usize = 1 << 20;
/// `hard`から、ふつうに動くときの上限`real`までのバイト数。VMの余白の2^18語をバイトにしたものである。
pub const LIMIT_HEADROOM_BYTES: usize = 2 << 20;
/// runの唯一のworkerの番号。止める語もこの番号のものを読む。
pub(crate) const WORKER: usize = 0;

fn fault(fault: RunFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: NATIVE_CODE.to_string(),
        at: None,
    }
}

/// `image`のプログラムを、新しいworkerのスレッドで`start`から走らせ、終わるのを待つ。
///
/// # Safety
/// `image`の表は呼び出しの間生きていて変わらず、どの`Str`もUTF-8を持つ。どの`DatumRow.slot`も、ほかの誰も使わない
/// 書ける語である。
/// `start.thunk`を`start.entry`と`start.argv`で呼ぶことは、`image`のためにコンパイルしたemlのコードへの正しい呼び出しで
/// あるか、そのコードの規則を守る代役を走らせる。代役は`ctx`をworkerのスレッドの上でだけ使い、emlのコードかエントリの
/// 呼び出しをまたいでそれへの参照を持たない。panicで巻き戻らない。`eml_fatal`で終わりうる呼び出しをまたいで、デストラクタを
/// 持つ値を生かしておかない。emlのコードが残しうる状態の`ctx`で戻る。`start.argv`は引数と結果の場所を持つ。
pub unsafe fn run(
    image: &Image,
    start: &Start,
    config: &RunConfig,
    host: HostRef,
    options: Options,
) -> Result<Finished, RuntimeError> {
    let control = &config.control;
    // 終わりの欄はrunごとに開く (docs/spec/vm.md の「誤り、止めること、終わり」)
    control.begin_run();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("eml-worker-0".to_string())
            .stack_size(options.stack)
            // SAFETY: 呼ぶ側の約束
            .spawn_scoped(scope, || unsafe {
                on_worker(image, start, control, host, options)
            })
            .map_err(|_| fault(RunFault::Internal("the worker thread cannot be started")))?;
        worker
            .join()
            .unwrap_or_else(|payload| resume_unwind(payload))
    })
}

/// # Safety
/// `run`と同じ。
unsafe fn on_worker(
    image: &Image,
    start: &Start,
    control: &Arc<Control>,
    host: HostRef,
    options: Options,
) -> Result<Finished, RuntimeError> {
    let top = arch::stack_pointer();
    let mut ctx = Ctx::<Fast>::for_worker(Arc::clone(control), host, WORKER);
    // スタックポインタから予約の大きさを引いた値は、本当の`base`より、この関数の上にあるフレームの分だけ低いことが
    // ある。その差は余白に収まる。上限を置くのはここの1回だけである (docs/spec/native.md の「安全点」)
    let base = top.saturating_sub(options.stack);
    ctx.set_stack_limit(base + STACK_HARD_MARGIN + LIMIT_HEADROOM_BYTES);
    // SAFETY: 呼ぶ側の約束
    unsafe { fill_data(&mut ctx, control, image.data.as_slice()) }?;
    // SAFETY: 呼ぶ側の約束
    let mut worker = unsafe { Worker::new(image) };
    ctx.set_runtime((&raw mut worker).cast());
    let raw = &raw mut ctx;
    // SAFETY: 呼ぶ側の約束。`ctx`と`worker`への参照をエントリをまたいで持たない
    let entered = unsafe { enter(raw, start.thunk, start.entry, start.argv) };
    ctx.set_runtime(std::ptr::null_mut());
    debug_assert!(detached(&ctx), "the context is still entered");
    match entered {
        // SAFETY: 呼ぶ側の約束
        Ok(()) => unsafe { finish(&mut ctx, &mut worker, start, options) },
        Err(fatal) => {
            let error = fatal_error(&mut ctx, &mut worker, fatal);
            // 失敗したrunも出力を書き出す。出力と誤りの順を保つためである
            let _ = ctx.flush();
            Err(error)
        }
    }
}

/// 読み込み時の値を作り、最初のエントリの前にどのslotも書く (docs/spec/native.md の「`Image`」)。
///
/// # Safety
/// `data`の文字列は正しく、どのslotもほかの誰も使わない書ける語である。
unsafe fn fill_data(
    ctx: &mut Ctx<Fast>,
    control: &Arc<Control>,
    data: &[DatumRow],
) -> Result<(), RuntimeError> {
    let data_of = |row: &DatumRow| match row.kind {
        // SAFETY: 呼ぶ側の約束
        DATUM_STRING => Ok(Datum::String(unsafe { row.text.as_str() })),
        DATUM_BIG_INT => Ok(Datum::BigInt(row.bits as i64)),
        DATUM_FLOAT => Ok(Datum::Float(row.bits)),
        _ => Err(fault(RunFault::Internal("a datum has an unknown kind"))),
    };
    let datums = data.iter().map(data_of).collect::<Result<Vec<_>, _>>()?;
    let built = constants::build(control, &datums)
        .map_err(|_| fault(RunFault::Internal("the constants cannot be frozen")))?;
    if let Some(region) = built.region {
        // SAFETY: 塊を付けても、`Ctx`の持つ語は変わらない
        unsafe { ctx.heap_mut() }.attach(region);
    }
    for (row, w) in data.iter().zip(built.words) {
        // SAFETY: 呼ぶ側の約束
        unsafe { row.slot.cast::<Word>().write(w) };
    }
    Ok(())
}

/// エントリが戻ったrunの終わり。`exit`の中断か結果の始末、出力、終わりの確かめ、censusを、VMの`Finish`の順で行う
/// (docs/spec/native.md の「実行の始めと終わり」)。
///
/// # Safety
/// `start.argv[0]`はエントリの結果を持つ。
unsafe fn finish(
    ctx: &mut Ctx<Fast>,
    worker: &mut Worker,
    start: &Start,
    options: Options,
) -> Result<Finished, RuntimeError> {
    if ctx.yielding() != 0 {
        let Some(code) = worker.exit else {
            return Err(fault(RunFault::Internal(
                "an abort reached the bottom of the task",
            )));
        };
        // 途中のフレームは巻き戻しで片付けてある。エントリの結果は仮の値なので手放さない。中断の途中の`evv`には意味が
        // ないので、空に戻す (docs/spec/vm.md の「extern の中断」)
        // SAFETY: 中断の途中で、`Ctx`がevidenceの列を持つ
        unsafe {
            ctx.yield_finish()
                .and_then(|_| ctx.evv_set(Word::UNIT))
                .map_err(|e| fault(e.into()))?;
        }
        // 先に書いた者がいても、その終了コードが勝つだけなので、結果は見ない (docs/spec/vm.md の「`Async` のタスク」)
        ctx.heap().control().exit(code);
    } else if start.result_rc {
        // SAFETY: 呼ぶ側の約束。結果はrunが持つ参照である
        unsafe {
            let result = start.argv.cast::<Word>().read();
            ctx.heap_mut().decref(result).map_err(|e| fault(e.into()))?;
        }
    }
    ctx.flush().map_err(|f| fault(f.into()))?;
    if !ctx.check_finished() {
        return Err(fault(RunFault::Internal(
            "the evidence list or a yield is left at the end",
        )));
    }
    let parked = ctx.take_fault();
    debug_assert_eq!(
        worker.panic.is_some(),
        parked == Some(eml_rt::Fault::Engine),
        "a parked panic and its marker go together"
    );
    if parked.is_some() {
        return Err(fault(RunFault::Internal(
            "a fault is left parked at the end",
        )));
    }
    if options.census == Census::Check {
        let left = census(ctx);
        if !left.is_empty() {
            return Err(RuntimeError::Leak(left));
        }
    }
    Ok(Finished {
        stats: RunStats::default(),
        exit: ctx.heap().control().exit_code(),
        preemptions: 0,
        workers: 0,
        placed: 0,
        placed_tasks: 0,
        pinned_tasks: 0,
        placed_jobs: 0,
        holds_raised: 0,
        holds_lowered: 0,
    })
}

/// `eml_fatal`で切ったrunの誤り。符号が決める場所から読む (docs/spec/native.md の「`eml_fatal`」)。
fn fatal_error(ctx: &mut Ctx<Fast>, worker: &mut Worker, fatal: Fatal) -> RuntimeError {
    match fatal.code {
        FATAL_RECORDED => worker
            .error
            .take()
            .unwrap_or_else(|| fault(RunFault::Internal("a recorded error is missing"))),
        FATAL_FAULT => match ctx.take_fault() {
            Some(f) => fault(RunFault::Extern(f)),
            None => fault(RunFault::Internal("a fatal fault is not parked")),
        },
        FATAL_PANICKED => fault(RunFault::Internal("a runtime entry panicked")),
        _ => fault(RunFault::Internal("a fatal code is unknown")),
    }
}
