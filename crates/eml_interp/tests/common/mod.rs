use eml_core_ir::Program;
use eml_interp::RuntimeError;
use eml_test_support::execute;

/// 手書きの Core IR を verifier に通してから `debug_heap` 付きで実行し、出力と結果を返す。
pub fn run_core(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = parse(text);
    eml_core_ir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    execute(program, true)
}

/// verifier を通さずに実行する。verifier が拒む IR で、インタプリタ自身の検査を確かめるテストのためである。
pub fn run_core_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    execute(parse(text), true)
}

pub fn parse(text: &str) -> Program {
    eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}
