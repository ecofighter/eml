use eml_interp::RuntimeError;
use eml_lir::Program;
use eml_test_support::execute;

/// 手書きの LIR を verifier に通してから `debug_heap` 付きで実行し、出力と結果を返す。
pub fn run_lir(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    execute(program, true)
}

/// verifier を通さずに実行する。verifier が拒む IR で、インタプリタ自身の検査を確かめるテストのためである。
pub fn run_lir_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    execute(parse(text), true)
}

pub fn parse(text: &str) -> Program {
    eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}
