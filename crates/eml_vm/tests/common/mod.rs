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
    let (out, result) = run_both(&verified(text));
    (out, result.map(|_| ()))
}

/// verifier を通さずに `run_lir` と同じく両方のモードで実行し、検査モードの `RunStats` を返す。今の verifier は box の
/// 挿入の後の段で `apply` の引数と結果を `tobj` と比べるので、型どおりの `apply` を書いた IR を通さないためである。
pub fn run_typed_apply(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    run_both(&parse(text))
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
