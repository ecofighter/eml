use eml_core_ir::{Program, v2};
use eml_interp::{RunConfig, RuntimeError};
use eml_runtime::OutputSink;
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

fn parse(text: &str) -> Program {
    eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

/// v2 の Core IR のテキストを verifier に通してから `debug_heap` 付きで実行し、出力と結果を返す。
pub fn run_v2(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = parse_v2(text);
    eml_core_ir::v2::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    execute_v2(&program, true)
}

/// verifier を通さずに実行する。verifier が拒む IR で、インタプリタ自身の検査を確かめるテストのためである。
pub fn run_v2_unverified(text: &str) -> (String, Result<(), RuntimeError>) {
    execute_v2(&parse_v2(text), true)
}

/// パイプラインはまだ今の IR を実行するので、`eml_test_support` を通さずに v2 の機械を直接呼ぶ。
pub fn execute_v2(program: &v2::Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_interp::v2::run(program, &config, &sink);
    (captured.contents(), result.map(|_| ()))
}

pub fn parse_v2(text: &str) -> v2::Program {
    v2::parse(text).unwrap_or_else(|error| panic!("{error}"))
}
