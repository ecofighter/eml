use std::sync::Arc;

use eml_lir::Program;
use eml_rt::{Control, Fast, Limits};
use eml_test_support::execute;
use eml_vm::{RunStats, RuntimeError, compile, disassemble};

pub fn parse(text: &str) -> Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
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
    let control = Arc::new(Control::new(Limits::default()));
    let code = compile::<Fast>(program, control).unwrap_or_else(|error| panic!("{error:?}"));
    disassemble(&code)
}
