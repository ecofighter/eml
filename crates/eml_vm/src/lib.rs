//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::sync::Arc;

use eml_lir::Program;
use eml_rt::{Checked, Ctx, Fast, Fault, HeapError, Mode, Stats};

use crate::vm::ExecWith;

mod apply;
mod cells;
mod code;
mod compile;
mod data;
mod disasm;
mod error;
mod hosts;
mod monitor;
mod op;
mod sched;
mod vm;

pub use code::{CodeBase, CompileError, FnEntry};
pub use disasm::disassemble;
pub use eml_rt::{Control, HostRef, Limits, Stop};
pub use error::{RuntimeError, SourceLocation, VmFault};
pub use hosts::{Capture, CaptureHost, Written, capture_host, script_host, stdio_host};
pub use sched::{Normal, Policy, Sched, Scheduler, Seeded, Sequential};
pub use vm::Task;

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大。検査モードだけで数える。
/// 各欄の意味と出どころは docs/spec/vm.md の「`RunStats`」にある。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunStats {
    pub evidence_steps: u64,
    pub yields: u64,
    pub yield_frames: u64,
    /// 中断の巻き戻しで外したフレームの数。`ApplyRest` の枠も数える。
    pub unwound_frames: u64,
    pub string_bytes_copied: u64,
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub boxes: u64,
    pub unboxes: u64,
    pub allocations: u64,
    /// token の箱を使った `con` の回数。空の token で確保した `con` は数えない。
    pub reuses: u64,
    pub peak_objects: u64,
    /// タスクのスタックで同時に使った語の数の最大。
    pub peak_stack: u64,
}

/// 1回の実行の結果。`exit` で終わったら、その終了コードを持つ (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    pub stats: RunStats,
    pub exit: Option<i64>,
    /// 方針が安全点で始めたタスクの切り替えの数 (docs/spec/vm.md の「タスクとスケジュール」)。
    pub preemptions: u64,
}

/// VM だけが知る仕事の回数。検査モードだけで数える。
#[derive(Debug, Default)]
pub(crate) struct RunCounts {
    pub(crate) unwound_frames: u64,
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
    pub(crate) reuses: u64,
}

/// 実行の設定。上限は `Control` が持つので、ここに上限の欄は置かない。`file_root` は Host が持つ
/// (docs/spec/vm.md の「置き場所」)。
#[derive(Clone)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// 止める理由のビットは `Control::clear` でしか下りないので、1つの `Control` は1回の実行にだけ使う。終わりの欄は
    /// 実行の始めに開くので、使い回しの妨げにならない。`clone` した設定は同じ `Control` を指す。
    pub control: Arc<Control>,
    /// タスクを切り替える方針。既定は横取りしない `Sched::Sequential` である。
    pub sched: Sched,
}

// `Control` は `Default` を持たないので、手で書く
impl Default for RunConfig {
    fn default() -> RunConfig {
        RunConfig {
            debug_heap: false,
            control: Arc::new(Control::new(Limits::default())),
            sched: Sched::Sequential,
        }
    }
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> RunConfig {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_control(mut self, control: Arc<Control>) -> RunConfig {
        self.control = control;
        self
    }

    pub fn with_sched(mut self, sched: Sched) -> RunConfig {
        self.sched = sched;
        self
    }
}

/// プログラムを1回だけ実行する。新しい `Machine` に読み込み、入口を実行し、閉じる。
pub fn execute(
    program: &Program,
    config: &RunConfig,
    host: HostRef,
) -> Result<Finished, RuntimeError> {
    let mut machine = AnyMachine::new(config, host);
    let loaded = machine
        .load(program)
        .map_err(|error| compile_error(program, error))?;
    let finished = machine.run(loaded.entry)?;
    machine.close()?;
    Ok(finished)
}

/// `Machine::load` の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loaded {
    /// 読み込んだプログラムの入口の関数の、`CodeBase` での番号。
    pub entry: u32,
}

/// 入力をまたいで `CodeBase` と `Ctx` を持つ実行の状態 (docs/spec/vm.md の「追記」)。`load` でプログラムを追記し、
/// `run` で入口の関数を実行する。モードごとの確かめは、作るときに `Machine<Fast>::new` と `Machine<Checked>::new` が
/// 決める。
pub struct Machine<M: Mode> {
    code: CodeBase<M>,
    ctx: Ctx<M>,
    max_depth: usize,
    sched: Sched,
    /// 実行と回復の後の確かめ。検査モードではヒープの不変条件を確かめる。
    check_heap: fn(&Ctx<M>) -> Result<(), HeapError>,
    /// 閉じるときの確かめ。検査モードでは残った物体をリークとして返す。
    at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
    /// 回復で捨てたヒープが実行でした仕事。
    earlier: HeapWork,
    /// 今のヒープが回復で写すためにした仕事。`RunStats` は実行の仕事だけを数えるので引く。
    copying: HeapWork,
    /// 根の表のうち、印を付け終えた先頭の根の数。実行をまたいで持ち、回復で 0 に戻す
    /// (docs/spec/object-model.md の「セッションの根」)。
    shared_roots: usize,
}

impl Machine<Fast> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Fast> {
        Machine::with_checks(control, host, |_| Ok(()), |_| Ok(()))
    }
}

impl Machine<Checked> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Checked> {
        Machine::with_checks(
            control,
            host,
            |ctx| ctx.heap().check_invariants(),
            check_leaks,
        )
    }
}

impl<M: Mode> Machine<M> {
    fn with_checks(
        control: Arc<Control>,
        host: HostRef,
        check_heap: fn(&Ctx<M>) -> Result<(), HeapError>,
        at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
    ) -> Machine<M> {
        Machine {
            code: CodeBase::new(Arc::clone(&control)),
            max_depth: control.limits().max_depth,
            ctx: Ctx::new(control, host),
            sched: Sched::Sequential,
            check_heap,
            at_close,
            earlier: HeapWork::default(),
            copying: HeapWork::default(),
            shared_roots: 0,
        }
    }

    /// 実行ごとに作るスケジューラの方針を選ぶ。既定は `Sched::Sequential` である。
    pub fn with_sched(mut self, sched: Sched) -> Machine<M> {
        self.sched = sched;
        self
    }

    pub fn code(&self) -> &CodeBase<M> {
        &self.code
    }

    /// プログラムを追記する。誤りを返した後の `Machine` は使い続けない (`CodeBase::load`)。
    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
        let entry = self.code.load(program)?;
        if let Some(region) = &self.code.region {
            // 入力の境目で、Ctx の持つ語には触れずに塊を付け替えるだけなので、`heap_mut` の前提を崩さない。新しい塊は
            // 前の塊の子孫である (docs/spec/object-model.md の「凍結した領域」)
            unsafe { self.ctx.heap_mut() }.attach(Arc::clone(region));
        }
        Ok(Loaded { entry })
    }

    /// 入口の関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがこの実行の値である。
    /// 根は手放さず、次の実行に残す。続けられる誤りの後と `exit` が実行を捨てた後は、根から届く物体を新しいヒープに
    /// 写して古いヒープを捨て、元の誤りか `exit` の `Finished` を返す。回復が失敗したら `Unrecovered` を返す
    /// (docs/spec/vm.md の「追記」)。
    pub fn run(&mut self, entry: u32) -> Result<Finished, RuntimeError> {
        self.run_recovering(entry, None)
            .map(|(finished, _)| finished)
    }

    /// 文字列を返す入口の関数を実行し、結果の文字列の先頭 `max_chars` 文字を写して返す。REPL が式の入力の値を書いた
    /// 文字列を受け取るのに使う。LIR は入口が文字列を返すことを印さないので、呼ぶ側が文字列を返す入口にだけ使う。誤りと
    /// 回復は `run` と同じである。`exit` で終わった実行は結果を返さないので、文字列は `None` になる
    /// (docs/spec/vm.md の「追記」)。
    pub fn run_shown(
        &mut self,
        entry: u32,
        max_chars: usize,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        self.run_recovering(entry, Some(max_chars))
    }

    /// `line` を Host の標準出力に1行として書き、書き出す。REPL が整形した結果を書くのに使う。
    pub fn write_line(&mut self, line: &str) -> Result<(), Fault> {
        self.ctx.print_line(line)
    }

    fn run_recovering(
        &mut self,
        entry: u32,
        shown: Option<usize>,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        match self.run_entry(entry, shown) {
            Ok(Ran::Done(finished, text)) => Ok((finished, text)),
            // 捨てた run のヒープには片付けていないタスクの物体が残るので、誤りの後と同じく入れ替える
            // (docs/spec/vm.md の「追記」)
            Ok(Ran::Abandoned(finished, cause)) => match self.recover() {
                Ok(()) => Ok((finished, None)),
                Err(reason) => Err(RuntimeError::Unrecovered {
                    cause: Box::new(cause),
                    reason,
                }),
            },
            Err(cause) if !cause.is_broken() => Err(match self.recover() {
                Ok(()) => cause,
                Err(reason) => RuntimeError::Unrecovered {
                    cause: Box::new(cause),
                    reason,
                },
            }),
            Err(cause) => Err(cause),
        }
    }

    fn recover(&mut self) -> Result<(), HeapError> {
        let old = self.heap_work();
        // 根の語は根が持つ参照で、根から届く物体は失敗した実行が書き換えないので、`recover` の前提を守る
        unsafe { self.ctx.recover() }?;
        // 回復は共有の根を局所の物体として写すので、次の fork はどの根にも印を付け直す
        self.shared_roots = 0;
        self.earlier = old;
        self.copying = HeapWork::of(self.ctx.heap().stats());
        (self.check_heap)(&self.ctx)
    }

    /// `Machine` を作ってからの、実行がヒープでした仕事。
    fn heap_work(&self) -> HeapWork {
        let now = HeapWork::of(self.ctx.heap().stats());
        HeapWork {
            rc_increments: self.earlier.rc_increments + now.rc_increments
                - self.copying.rc_increments,
            rc_decrements: self.earlier.rc_decrements + now.rc_decrements
                - self.copying.rc_decrements,
            allocations: self.earlier.allocations + now.allocations - self.copying.allocations,
            peak_objects: self.earlier.peak_objects.max(now.peak_objects),
        }
    }

    fn run_entry(&mut self, entry: u32, shown: Option<usize>) -> Result<Ran, RuntimeError> {
        // 終わりの欄は run ごとに開く (docs/spec/vm.md の「誤り、止めること、終わり」)
        self.ctx.heap().control().begin_run();
        let (code, ctx) = (&self.code, &mut self.ctx);
        let mut task = Task::new(code, entry, self.max_depth);
        // 実行の後の誤りは、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)
        let after = |fault: VmFault, task: &Task| {
            code.error(
                vm::Failure {
                    fault,
                    pc: task.last_ret,
                },
                task,
            )
        };
        if shown.is_some() {
            if !code.functions[entry as usize].ret_rc {
                return Err(code.error(
                    vm::Failure {
                        fault: VmFault::Internal("a shown entry does not return a string"),
                        pc: task.pc,
                    },
                    &task,
                ));
            }
            task.keep_result = true;
        }
        let mut counts = RunCounts::default();
        let start = Start {
            max_depth: self.max_depth,
            shared_roots: self.shared_roots,
        };
        let (result, scheduled) = match self.sched {
            Sched::Normal => {
                let policy = Normal::new(Arc::clone(ctx.heap().control()));
                run_scheduled(code, ctx, &mut task, policy, start, &mut counts)
            }
            Sched::Sequential => {
                run_scheduled(code, ctx, &mut task, Sequential, start, &mut counts)
            }
            Sched::Seeded(seed) => {
                run_scheduled(code, ctx, &mut task, Seeded::new(seed), start, &mut counts)
            }
        };
        self.shared_roots = scheduled.shared_roots;
        // 結果は `Finish` が手放さずに残した、VM の持つ参照である。書き出しの前に写すので、写すときの誤りも
        // 書き出しの誤りより先に報告する (docs/spec/vm.md の「誤り、止めること、終わり」)
        // `exit` で終わった実行の入口の結果はダミーなので写さない
        let exit = ctx.heap().control().exit_code();
        let copied = match (&result, shown, exit) {
            (Ok(()), Some(max_chars), None) => {
                Some(unsafe { ctx.copy_string(task.get(0), max_chars) })
            }
            _ => None,
        };
        // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
        let flushed = ctx.flush();
        if let Err(
            cause @ RuntimeError::Fault {
                fault: VmFault::Stopped(Stop::Ended),
                ..
            },
        ) = result
        {
            // タスクの底の `exit` が run を捨てた。終わりの検査はせず、その時点の数で `exit` を報告する
            // (docs/spec/vm.md の「誤り、止めること、終わり」)
            flushed.map_err(|fault| after(fault.into(), &task))?;
            let Some(code) = exit else {
                return Err(after(
                    VmFault::Internal("a run ended without an exit"),
                    &task,
                ));
            };
            let finished = Finished {
                stats: self.run_stats(&counts, scheduled.peak_stack),
                exit: Some(code),
                preemptions: scheduled.preemptions,
            };
            return Ok(Ran::Abandoned(finished, cause));
        }
        result?;
        let text = copied
            .transpose()
            .map_err(|fault| after(fault.into(), &task))?;
        flushed.map_err(|fault| after(fault.into(), &task))?;
        if !ctx.check_finished() {
            return Err(after(
                VmFault::Internal("the evidence list or a yield is left at the end"),
                &task,
            ));
        }
        (self.check_heap)(ctx)
            .map_err(|error| heap_fault(error, code.function_name(task.last_ret, &task)))?;
        let finished = Finished {
            stats: self.run_stats(&counts, scheduled.peak_stack),
            exit,
            preemptions: scheduled.preemptions,
        };
        Ok(Ran::Done(finished, text))
    }

    /// `Machine` を作ってからの積算の `RunStats`。`peak_stack` だけがこの実行の値である。
    fn run_stats(&self, counts: &RunCounts, peak_stack: u64) -> RunStats {
        let c = self.ctx.stats();
        let h = self.heap_work();
        RunStats {
            evidence_steps: c.evidence_steps,
            yields: c.yields,
            yield_frames: c.yield_frames,
            unwound_frames: counts.unwound_frames,
            string_bytes_copied: c.string_bytes_copied,
            rc_increments: h.rc_increments,
            rc_decrements: h.rc_decrements,
            boxes: counts.boxes,
            unboxes: counts.unboxes,
            allocations: h.allocations,
            reuses: counts.reuses,
            peak_objects: h.peak_objects,
            // 統計は検査モードだけで数える
            peak_stack: if M::CHECKED { peak_stack } else { 0 },
        }
    }

    /// 実行の状態を閉じる。根の参照は VM が持つので先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、
    /// 残った物体をリークとして返す。
    pub fn close(mut self) -> Result<(), RuntimeError> {
        let close_error = |error| heap_fault(error, "<close>".to_string());
        for root in self.ctx.take_roots().into_iter().flatten() {
            // 根の語は根が持つ参照で、ほかの語には触れないので、`heap_mut` の前提を崩さない
            unsafe { self.ctx.heap_mut().decref(root) }.map_err(close_error)?;
        }
        (self.check_heap)(&self.ctx).map_err(close_error)?;
        (self.at_close)(&self.ctx)
    }
}

/// `run_entry` の結果。
enum Ran {
    /// 入口の関数が戻った。`exit` で終わったときは、`main` の底が終わりの欄に書いてある。
    Done(Finished, Option<String>),
    /// `main` でないタスクの底の `exit` が run を捨てた。誤りは回復が失敗したときの `cause` に使う。
    Abandoned(Finished, RuntimeError),
}

/// 実行が終わった後に、捨てるスケジューラから読むもの。
struct Scheduled {
    preemptions: u64,
    peak_stack: u64,
    shared_roots: usize,
}

/// スケジューラを作るときに `Machine` から渡すもの。
#[derive(Clone, Copy)]
struct Start {
    max_depth: usize,
    shared_roots: usize,
}

/// 方針 `P` のスケジューラを作って `task` から実行する。方針を型の引数にして、方針ごとに VM の繰り返しを作る
/// (docs/spec/vm.md の「タスクとスケジュール」)。
fn run_scheduled<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    policy: P,
    start: Start,
    counts: &mut RunCounts,
) -> (Result<(), RuntimeError>, Scheduled) {
    let main_worker = ctx.heap().worker();
    task.worker = main_worker;
    let mut sched = Scheduler::new(policy, main_worker, start.max_depth);
    sched.shared_roots = start.shared_roots;
    let result = vm::run::<ExecWith<M, P>>(code, ctx, task, &mut sched, counts);
    let scheduled = Scheduled {
        preemptions: sched.preemptions(),
        peak_stack: sched.peak_stack(task),
        shared_roots: sched.shared_roots,
    };
    (result, scheduled)
}

/// `RunStats` に入るヒープの仕事の数 (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, Default)]
struct HeapWork {
    rc_increments: u64,
    rc_decrements: u64,
    allocations: u64,
    peak_objects: u64,
}

impl HeapWork {
    fn of(stats: Stats) -> HeapWork {
        HeapWork {
            rc_increments: stats.rc_increments,
            rc_decrements: stats.rc_decrements,
            allocations: stats.allocations,
            peak_objects: stats.peak_live,
        }
    }
}

/// 実行の前にモードを選ぶ呼び出し側のための `Machine`。
pub enum AnyMachine {
    Fast(Machine<Fast>),
    Checked(Machine<Checked>),
}

impl AnyMachine {
    pub fn new(config: &RunConfig, host: HostRef) -> AnyMachine {
        let control = Arc::clone(&config.control);
        if config.debug_heap {
            AnyMachine::Checked(Machine::<Checked>::new(control, host).with_sched(config.sched))
        } else {
            AnyMachine::Fast(Machine::<Fast>::new(control, host).with_sched(config.sched))
        }
    }

    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
        match self {
            AnyMachine::Fast(machine) => machine.load(program),
            AnyMachine::Checked(machine) => machine.load(program),
        }
    }

    pub fn run(&mut self, entry: u32) -> Result<Finished, RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.run(entry),
            AnyMachine::Checked(machine) => machine.run(entry),
        }
    }

    pub fn run_shown(
        &mut self,
        entry: u32,
        max_chars: usize,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.run_shown(entry, max_chars),
            AnyMachine::Checked(machine) => machine.run_shown(entry, max_chars),
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), Fault> {
        match self {
            AnyMachine::Fast(machine) => machine.write_line(line),
            AnyMachine::Checked(machine) => machine.write_line(line),
        }
    }

    pub fn close(self) -> Result<(), RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.close(),
            AnyMachine::Checked(machine) => machine.close(),
        }
    }
}

/// ヒープの不変条件の誤りには、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)。
fn heap_fault(error: HeapError, last_function: String) -> RuntimeError {
    RuntimeError::Fault {
        fault: error.into(),
        function: last_function,
        at: None,
    }
}

fn check_leaks(ctx: &Ctx<Checked>) -> Result<(), RuntimeError> {
    let leaks = ctx.heap().leaks();
    if leaks.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::Leak(
            leaks
                .into_iter()
                .map(|(name, n)| (name.to_string(), n))
                .collect(),
        ))
    }
}

/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。REPL は `Machine::load` の誤りをこれで報告する。
pub fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
    let (what, function) = match error {
        CompileError::TooManySlots { function } => {
            ("a function needs more than 65535 slots", function)
        }
        CompileError::Freeze(_) => (
            "the constants cannot be frozen",
            program.function(program.entry).name.clone(),
        ),
        CompileError::Mismatch { function } => {
            ("a function with a known name has other code", function)
        }
        CompileError::MultiValueEntry { function } => {
            ("an entry function returns more than one value", function)
        }
        CompileError::MissingFork { function } => (
            "a program that may run a par lacks a fork function",
            function,
        ),
        CompileError::MissingEntry { function } => (
            "a program that runs Async tasks lacks a runtime entry",
            function,
        ),
    };
    RuntimeError::Fault {
        fault: VmFault::Internal(what),
        function,
        at: None,
    }
}
