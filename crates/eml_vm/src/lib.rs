//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use eml_lir::Program;
use eml_rt::{Buffering, Checked, Ctx, Fast, Mode, StdHost};

mod apply;
mod code;
mod compile;
mod data;
mod disasm;
mod error;
mod op;
mod vm;

pub use code::{CodeBase, CompileError, FnEntry};
pub use compile::compile;
pub use disasm::disassemble;
pub use eml_rt::{Control, HostRef, Limits, Stop};
pub use error::{RuntimeError, SourceLocation, VmFault};
pub use vm::Task;

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大。検査モードだけで数える。
/// 各欄の意味と出どころは docs/spec/vm.md の「`RunStats`」にある。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunStats {
    pub evidence_steps: u64,
    pub yields: u64,
    pub yield_frames: u64,
    pub string_bytes_copied: u64,
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub boxes: u64,
    pub unboxes: u64,
    pub peak_objects: u64,
    /// タスクのスタックで同時に使った語の数の最大。
    pub peak_stack: u64,
}

/// VM だけが知る仕事の回数。検査モードだけで数える。
#[derive(Debug, Default)]
pub(crate) struct RunCounts {
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
}

/// 実行の設定。上限は `Control` が持つので、ここに上限の欄は置かない。`file_root` は Host が持つ
/// (docs/spec/vm.md の「置き場所」)。
#[derive(Clone)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// 止める理由のビットは `Control::clear` でしか下りないので、1つの `Control` は1回の実行にだけ使う。`clone` した
    /// 設定は同じ `Control` を指す。
    pub control: Arc<Control>,
}

// `Control` は `Default` を持たないので、手で書く
impl Default for RunConfig {
    fn default() -> RunConfig {
        RunConfig {
            debug_heap: false,
            control: Arc::new(Control::new(Limits::default())),
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
}

/// プログラムを1回だけ実行する。新しい `Machine` に読み込み、入口を実行し、閉じる。
pub fn execute(
    program: &Program,
    config: &RunConfig,
    host: HostRef,
) -> Result<RunStats, RuntimeError> {
    let mut machine = AnyMachine::new(config, host);
    let loaded = machine
        .load(program)
        .map_err(|error| compile_error(program, error))?;
    let stats = machine.run(loaded.entry)?;
    machine.close()?;
    Ok(stats)
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
    /// 実行の後の確かめ。検査モードではヒープの不変条件を確かめる。
    after_run: fn(&Ctx<M>, String) -> Result<(), RuntimeError>,
    /// 閉じるときの確かめ。検査モードでは残った物体をリークとして返す。
    at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
}

impl Machine<Fast> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Fast> {
        Machine::with_checks(control, host, |_, _| Ok(()), |_| Ok(()))
    }
}

impl Machine<Checked> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Checked> {
        Machine::with_checks(control, host, check_invariants, check_leaks)
    }
}

impl<M: Mode> Machine<M> {
    fn with_checks(
        control: Arc<Control>,
        host: HostRef,
        after_run: fn(&Ctx<M>, String) -> Result<(), RuntimeError>,
        at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
    ) -> Machine<M> {
        Machine {
            code: CodeBase::new(Arc::clone(&control)),
            max_depth: control.limits().max_depth,
            ctx: Ctx::new(control, host),
            after_run,
            at_close,
        }
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
    /// 根は手放さず、次の実行に残す。続けられる誤りの後は evv と yield の状態を空に戻す。スタックと evv に残った語は
    /// 手放さずにヒープに残る (docs/spec/vm.md の「追記」)。
    pub fn run(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
        let result = self.run_entry(entry);
        if let Err(error) = &result
            && !error.is_broken()
        {
            self.ctx.abandon();
        }
        result
    }

    fn run_entry(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
        let (code, ctx) = (&self.code, &mut self.ctx);
        let mut task = Task::new(code, entry, self.max_depth);
        let mut counts = RunCounts::default();
        let result = vm::run(code, ctx, &mut task, &mut counts);
        // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
        let flushed = ctx.flush();
        result.map_err(|failure| code.error(failure, &task))?;
        flushed.map_err(|fault| {
            code.error(
                vm::Failure {
                    fault: fault.into(),
                    pc: task.last_ret,
                },
                &task,
            )
        })?;
        if !ctx.check_finished() {
            return Err(code.error(
                vm::Failure {
                    fault: VmFault::Internal("the evidence list or a yield is left at the end"),
                    pc: task.last_ret,
                },
                &task,
            ));
        }
        (self.after_run)(ctx, code.function_name(task.last_ret, &task))?;
        let (c, h) = (ctx.stats(), ctx.heap().stats());
        Ok(RunStats {
            evidence_steps: c.evidence_steps,
            yields: c.yields,
            yield_frames: c.yield_frames,
            string_bytes_copied: c.string_bytes_copied,
            rc_increments: h.rc_increments,
            rc_decrements: h.rc_decrements,
            boxes: counts.boxes,
            unboxes: counts.unboxes,
            peak_objects: h.peak_live,
            // スタックの長さは縮めないので、長さがそのまま使った語の数の最大である。統計は検査モードだけで数える
            peak_stack: if M::CHECKED {
                task.stack.len() as u64
            } else {
                0
            },
        })
    }

    /// 実行の状態を閉じる。根の参照は VM が持つので先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、
    /// 残った物体をリークとして返す。
    pub fn close(mut self) -> Result<(), RuntimeError> {
        let close_error = |error: eml_rt::HeapError| RuntimeError::Fault {
            fault: VmFault::Extern(error.into()),
            function: "<close>".to_string(),
            at: None,
        };
        for root in self.ctx.take_roots().into_iter().flatten() {
            // 根の語は根が持つ参照で、ほかの語には触れないので、`heap_mut` の前提を崩さない
            unsafe { self.ctx.heap_mut().decref(root) }.map_err(close_error)?;
        }
        (self.after_run)(&self.ctx, "<close>".to_string())?;
        (self.at_close)(&self.ctx)
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
            AnyMachine::Checked(Machine::<Checked>::new(control, host))
        } else {
            AnyMachine::Fast(Machine::<Fast>::new(control, host))
        }
    }

    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
        match self {
            AnyMachine::Fast(machine) => machine.load(program),
            AnyMachine::Checked(machine) => machine.load(program),
        }
    }

    pub fn run(&mut self, entry: u32) -> Result<RunStats, RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.run(entry),
            AnyMachine::Checked(machine) => machine.run(entry),
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
fn check_invariants(ctx: &Ctx<Checked>, last_function: String) -> Result<(), RuntimeError> {
    ctx.heap()
        .check_invariants()
        .map_err(|error| RuntimeError::Fault {
            fault: error.into(),
            function: last_function,
            at: None,
        })
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

/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。
fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
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
    };
    RuntimeError::Fault {
        fault: VmFault::Internal(what),
        function,
        at: None,
    }
}

/// 書いたバイト列を覚える書き込み先。テストが出力を読むために使う。
#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub fn contents(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8(bytes.clone()).expect("the output is UTF-8")
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 標準出力をメモリに捕まえる Host。`file_root` は `Fs.open` の相対パスの基準である。
pub fn capture_host(file_root: PathBuf) -> (HostRef, Capture) {
    let out = Capture::default();
    let host = StdHost::new(
        Box::new(out.clone()),
        Box::new(io::sink()),
        file_root,
        Buffering::Block,
    );
    (HostRef::new(Arc::new(host)), out)
}

/// 標準出力と標準エラーに書く Host。
pub fn stdio_host(file_root: PathBuf) -> HostRef {
    HostRef::new(Arc::new(StdHost::stdio(file_root)))
}
