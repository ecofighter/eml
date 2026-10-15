//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use eml_lir::Program;
use eml_rt::{Buffering, Checked, Control, Ctx, Fast, Limits, Mode, StdHost};

mod apply;
mod code;
mod compile;
mod data;
mod disasm;
mod error;
mod op;
mod prims;
mod vm;

pub use code::{CodeBase, CompileError, FnEntry};
pub use compile::compile;
pub use disasm::disassemble;
pub use eml_rt::HostRef;
pub use error::{SourceLocation, VmError, VmFault};
pub use vm::Task;

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大。検査モードだけで数える。欄は `eml_interp::RunStats`
/// と同じで、出どころは docs/spec/vm.md の「`RunStats`」にある。
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
}

/// VM だけが知る仕事の回数。検査モードだけで数える。
#[derive(Debug, Default)]
pub(crate) struct RunCounts {
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
}

/// 上限のない `Control` で実行する。上限の指定は S10b で `RunConfig` から渡す。
pub fn execute(program: &Program, debug_heap: bool, host: HostRef) -> Result<RunStats, VmError> {
    execute_with(
        program,
        debug_heap,
        host,
        Arc::new(Control::new(Limits::default())),
    )
}

/// セッションの `Control` を渡して実行する。割り込みと上限のテストが使う。
pub fn execute_with(
    program: &Program,
    debug_heap: bool,
    host: HostRef,
    control: Arc<Control>,
) -> Result<RunStats, VmError> {
    if debug_heap {
        run::<Checked>(program, host, control, check_heap)
    } else {
        run::<Fast>(program, host, control, |_, _| Ok(()))
    }
}

fn run<M: Mode>(
    program: &Program,
    host: HostRef,
    control: Arc<Control>,
    check: fn(&Ctx<M>, String) -> Result<(), VmError>,
) -> Result<RunStats, VmError> {
    let code =
        compile::<M>(program, control.clone()).map_err(|error| compile_error(program, error))?;
    let mut ctx = Ctx::<M>::new(control, host);
    // どの語も作る前に付けるので、`heap_mut` の前提 (Ctx の持つ語に触れない) を崩さない
    unsafe { ctx.heap_mut() }.attach(Arc::clone(&code.region));
    let mut task = Task::new(&code);
    let mut counts = RunCounts::default();
    let result = vm::run(&code, &mut ctx, &mut task, &mut counts);
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
    check(&ctx, code.function_name(task.last_ret))?;
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
    })
}

/// ヒープの不変条件の誤りには、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)。
fn check_heap(ctx: &Ctx<Checked>, last_function: String) -> Result<(), VmError> {
    let heap = ctx.heap();
    heap.check_invariants().map_err(|error| VmError::Fault {
        fault: error.into(),
        function: last_function,
        at: None,
    })?;
    let leaks = heap.leaks();
    if leaks.is_empty() {
        Ok(())
    } else {
        Err(VmError::Leak(
            leaks
                .into_iter()
                .map(|(name, n)| (name.to_string(), n))
                .collect(),
        ))
    }
}

/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。
fn compile_error(program: &Program, error: CompileError) -> VmError {
    let (what, function) = match error {
        CompileError::TooManySlots { function } => {
            ("a function needs more than 65535 slots", function)
        }
        CompileError::Freeze(_) => (
            "the constants cannot be frozen",
            program.function(program.entry).name.clone(),
        ),
    };
    VmError::Fault {
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
