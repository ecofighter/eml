use std::sync::Arc;

use eml_lir::Program;
use eml_rt::{Control, Fast, Limits};
use eml_test_support::execute_vm;
use eml_vm::{VmError, compile, disassemble};

pub fn parse(text: &str) -> Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

fn verified(text: &str) -> Program {
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    program
}

/// 手書きの LIR を verifier に通し、検査モードで実行する。通ったときは速いモードでも実行し、出力が同じことを確かめる。
/// 速いモードは前提を確かめないので、検査モードが通らなかった IR では走らせない (docs/spec/vm.md の「検証とテスト」)。
pub fn run_lir(text: &str) -> (String, Result<(), VmError>) {
    let program = verified(text);
    let (out, result) = execute_vm(&program, true);
    if result.is_ok() {
        let (fast_out, fast) = execute_vm(&program, false);
        assert_eq!(fast.map(|_| ()), Ok(()), "the fast mode failed");
        assert_eq!(fast_out, out, "the fast mode printed something else");
    }
    (out, result.map(|_| ()))
}

/// verifier を通さずに、検査モードだけで実行する。verifier が拒む IR で、VM 自身の見張りを確かめるテストのためである。
pub fn run_unverified(text: &str) -> (String, Result<(), VmError>) {
    let (out, result) = execute_vm(&parse(text), true);
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
