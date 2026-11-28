pub mod c;

use std::sync::Arc;

use eml_lir::{Program, Trusted};
use eml_rt::{Control, Fast, Limits};
use eml_sched::config::RunStats;
use eml_test_support::execute;
use eml_vm::{CodeBase, CompileError, RuntimeError, disassemble};

pub fn parse(text: &str) -> Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

pub fn trusted(text: &str) -> Trusted {
    trust(parse(text))
}

pub fn trust(program: Program) -> Trusted {
    // SAFETY: 手書きのLIRの約束はテストが負う。テストは検査モードでも動かして確かめる
    unsafe { Trusted::new(program) }.unwrap_or_else(|error| panic!("{error}"))
}

/// verifierが拒むIRを`Trusted::unverified`で包む。使うのは、拒まれるIRの上でVM自身の見張りか検査モードを確かめる
/// テストだけである (docs/implementation/testing.mdの「`eml_vm` のテスト」)。
pub fn unverified(text: &str) -> Trusted {
    let program = parse(text);
    assert!(
        eml_lir::verify(&program).is_err(),
        "the verifier accepts this program; use `run_lir`"
    );
    // SAFETY: テストは、わざと入れる誤りのほかは実行の前提を守る。その誤りはどれも、VMの見張りか検査モードが
    // 報告するか、物体を手放さずに残すだけである (P6)
    unsafe { Trusted::unverified(program) }
}

/// プログラム全体を、上限のない `Control` を持つ速いモードの新しい `CodeBase` に変換する。逆アセンブラのテストのため。
pub fn compile(program: &Trusted) -> Result<CodeBase<Fast>, CompileError> {
    let mut code = CodeBase::new(Arc::new(Control::new(Limits::default())));
    code.load(program, &eml_vm::Linked::default())?;
    Ok(code)
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
    run_both(&trusted(text))
}

/// 構造体を持つ手書きの LIR を、box の挿入から reuse までのパスに通して verifier にかける。構造体はまだソースから
/// 作られないので、VM の複数の値の命令と構造体の変換はこの形で確かめる。
pub fn lowered(text: &str) -> Trusted {
    let mut program = parse(text);
    eml_lir::passes_until(&mut program, eml_lir::Pass::Reuse);
    trust(program)
}

/// `lowered` の LIR を `run_lir_stats` と同じく実行する。
pub fn run_lowered_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    run_both(&lowered(text))
}

/// `lowered` の LIR の逆アセンブル。
pub fn disasm_lowered(text: &str) -> String {
    disasm_program(&lowered(text))
}

fn run_both(program: &Trusted) -> (String, Result<RunStats, RuntimeError>) {
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

/// verifierが拒むIRを`unverified`で読み込み、検査モードだけで実行する。
pub fn run_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    let (out, result) = execute(
        &unverified(text),
        &eml_vm::RunConfig::default().with_debug_heap(true),
    );
    (out, result.map(|_| ()))
}

/// verifierを通したLIRの逆アセンブル。
pub fn disasm(text: &str) -> String {
    disasm_program(&trusted(text))
}

fn disasm_program(program: &Trusted) -> String {
    let code = compile(program).unwrap_or_else(|error| panic!("{error}"));
    disassemble(&code)
}

/// `run_lir`と同じく両方のモードで実行するが、`execute`を通さずに合わせた`Machine` (`Seat::Merged`) で動かす。
/// セッションの根に触れるコードは、REPLと同じ合わせたrunでしか動かないためである (docs/spec/vm.mdの
/// 「誤り、止めること、終わり」)。
pub fn run_lir_merged(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = trusted(text);
    let (out, result) = run_merged(&program, true);
    if result.is_ok() {
        let (fast_out, fast) = run_merged(&program, false);
        assert_eq!(fast, Ok(()), "the fast mode failed");
        assert_eq!(fast_out, out, "the fast mode printed something else");
    }
    (out, result)
}

/// `execute`と同じ手順 (読み込み、実行、閉じる) を、合わせた`Machine`で行う。
fn run_merged(program: &Trusted, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let config = eml_vm::RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_seat(eml_sched::config::Seat::Merged);
    let (host, out) = eml_vm::capture_host(Default::default());
    let mut machine = eml_vm::AnyMachine::new(&config, host);
    let mut run = || {
        let loaded = machine
            .load(program, &eml_vm::Linked::default())
            .map_err(|error| eml_vm::compile_error(program, error))?;
        machine.run(loaded)?;
        Ok::<(), RuntimeError>(())
    };
    let result = run().and_then(|()| machine.close());
    (out.contents(), result)
}
