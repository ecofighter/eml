use eml_interp::RuntimeError;
use eml_test_support::execute;

/// 手書きの Core IR を `debug_heap` 付きで実行し、出力と結果を返す。`verify` が真なら、先に verifier に通す。
/// verifier が拒む IR で実行時の検査を確かめるテストもあるので、通さずに実行することもできる。
pub fn run_core(text: &str, verify: bool) -> (String, Result<(), RuntimeError>) {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    if verify {
        eml_core_ir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    }
    execute(program, true)
}
