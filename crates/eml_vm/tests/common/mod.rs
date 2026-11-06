pub mod c;

use std::sync::Arc;

use eml_lir::Program;
use eml_rt::{Control, Fast, Limits};
use eml_test_support::execute;
use eml_vm::{CodeBase, CompileError, RunStats, RuntimeError, disassemble};

pub fn parse(text: &str) -> Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

/// プログラム全体を、上限のない `Control` を持つ速いモードの新しい `CodeBase` に変換する。逆アセンブラのテストのため。
pub fn compile(program: &Program) -> Result<CodeBase<Fast>, CompileError> {
    let mut code = CodeBase::new(Arc::new(Control::new(Limits::default())));
    code.load(program, &eml_vm::Linked::default())?;
    Ok(code)
}

fn verified(text: &str) -> Program {
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    program
}

/// 手書きの LIR を verifier に通し、検査モードで実行する。通ったときは速いモードでも実行し、出力が同じことを確かめる。
/// 速いモードは前提を確かめないので、検査モードが通らなかった IR では走らせない
/// (docs/implementation/testing.md の「`eml_vm` のテスト」)。
pub fn run_lir(text: &str) -> (String, Result<(), RuntimeError>) {
    let (out, result) = run_lir_stats(text);
    (out, result.map(|_| ()))
}

/// `run_lir` と同じく実行し、検査モードの `RunStats` を返す。
pub fn run_lir_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    run_both(&verified(text))
}

/// 構造体を持つ手書きの LIR を、box の挿入から reuse までのパスに通して verifier にかける。構造体はまだソースから
/// 作られないので、VM の複数の値の命令と構造体の変換はこの形で確かめる。
pub fn lowered(text: &str) -> Program {
    let mut program = parse(text);
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    program
}

/// `lowered` の LIR を `run_lir_stats` と同じく実行する。
pub fn run_lowered_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    run_both(&lowered(text))
}

/// `lowered` の LIR の逆アセンブル。
pub fn disasm_lowered(text: &str) -> String {
    disasm_unverified_program(&lowered(text))
}

fn run_both(program: &Program) -> (String, Result<RunStats, RuntimeError>) {
    let (out, result) = execute(program, &eml_vm::RunConfig::default().with_debug_heap(true));
    if result.is_ok() {
        let (fast_out, fast) = execute(
            program,
            &eml_vm::RunConfig::default().with_debug_heap(false),
        );
        assert_eq!(fast.map(|_| ()), Ok(()), "the fast mode failed");
        assert_eq!(fast_out, out, "the fast mode printed something else");
    }
    (out, result)
}

/// verifier を通さずに、検査モードだけで実行する。verifier が拒む IR で、VM 自身の見張りを確かめるテストのためである。
pub fn run_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    let (out, result) = execute(
        &parse(text),
        &eml_vm::RunConfig::default().with_debug_heap(true),
    );
    (out, result.map(|_| ()))
}

/// verifier を通した LIR の逆アセンブル。
pub fn disasm(text: &str) -> String {
    disasm_unverified_program(&verified(text))
}

/// verifier を通さない逆アセンブル。コード生成だけを確かめるテストのためである。
pub fn disasm_unverified(text: &str) -> String {
    disasm_unverified_program(&parse(text))
}

fn disasm_unverified_program(program: &Program) -> String {
    let code = compile(program).unwrap_or_else(|error| panic!("{error:?}"));
    disassemble(&code)
}

/// `run_lir`と同じく両方のモードで実行するが、`execute`を通さずに合わせた`Machine` (`Seat::Merged`) で動かす。
/// セッションの根に触れるコードは、REPLと同じ合わせたrunでしか動かないためである (docs/spec/vm.mdの
/// 「誤り、止めること、終わり」)。
pub fn run_lir_merged(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = verified(text);
    let (out, result) = run_merged(&program, true);
    if result.is_ok() {
        let (fast_out, fast) = run_merged(&program, false);
        assert_eq!(fast, Ok(()), "the fast mode failed");
        assert_eq!(fast_out, out, "the fast mode printed something else");
    }
    (out, result)
}

/// 書いた行ごとに、書いたスレッドを覚えるHost。`panic_on`の行を書こうとするとpanicする。どのスレッドがプログラムの
/// コードを動かしたかを確かめるためにある。
pub struct RecordingHost {
    inner: eml_vm::CaptureHost,
    lines: std::sync::Mutex<Vec<(String, std::thread::ThreadId)>>,
    panic_on: Option<&'static str>,
}

impl RecordingHost {
    /// Hostと、その標準出力の写し。
    pub fn new(panic_on: Option<&'static str>) -> (Arc<RecordingHost>, eml_vm::Capture) {
        let inner = eml_vm::CaptureHost::new(std::path::PathBuf::new());
        let out = inner.output();
        let host = Arc::new(RecordingHost {
            inner,
            lines: Default::default(),
            panic_on,
        });
        (host, out)
    }

    pub fn host_ref(self: &Arc<RecordingHost>) -> eml_rt::HostRef {
        eml_rt::HostRef::new(Arc::clone(self) as Arc<dyn eml_rt::Host>)
    }

    /// 書いた行と、書いたスレッド。
    pub fn lines(&self) -> Vec<(String, std::thread::ThreadId)> {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl eml_rt::Host for RecordingHost {
    fn write_line(&self, stream: eml_rt::Stream, line: &str) -> std::io::Result<()> {
        if self.panic_on == Some(line) {
            panic!("{line}");
        }
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((line.to_string(), std::thread::current().id()));
        self.inner.write_line(stream, line)
    }
    fn write(&self, stream: eml_rt::Stream, bytes: &[u8]) -> std::io::Result<()> {
        self.inner.write(stream, bytes)
    }
    fn flush(&self) -> std::io::Result<()> {
        self.inner.flush()
    }
    fn open(&self, path: &str) -> std::io::Result<eml_rt::Opened> {
        self.inner.open(path)
    }
}

/// `execute`と同じ手順 (読み込み、実行、閉じる) を、合わせた`Machine`で行う。
fn run_merged(program: &Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let config = eml_vm::RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_seat(eml_vm::Seat::Merged);
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = eml_vm::AnyMachine::new(&config, host);
    let mut run = || {
        let loaded = machine
            .load(program, &eml_vm::Linked::default())
            .map_err(|error| eml_vm::compile_error(program, error))?;
        machine.run(loaded.entry)?;
        Ok::<(), RuntimeError>(())
    };
    let result = run().and_then(|()| machine.close());
    (out.contents(), result)
}
