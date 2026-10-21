pub mod programs;

use eml_core_ir::{Pass, Program, parse, pretty, pretty_with_positions};

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
    core_text_files(text, &[], last)
}

/// `modules` は根からの相対パスと本文の組である (`eml_test_support::core_until_files`)。
pub fn core_text_files(entry: &str, modules: &[(&str, &str)], last: Pass) -> String {
    let shown = pretty(&eml_test_support::core_until_files(entry, modules, last));
    read_back(&shown, last, pretty);
    shown
}

/// extern の呼び出しの位置も表示する。
pub fn core_text_with_positions(text: &str) -> String {
    let shown = pretty_with_positions(&eml_test_support::core_until(text, Pass::Translate));
    read_back(&shown, Pass::Translate, pretty_with_positions);
    shown
}

/// 表示した IR を読み直し、同じ表示に戻ることと、読み直した IR が同じ段の verifier を通ることを確かめる。`pretty` と
/// `parse` が同じ誤りをして、違うプログラムで表示だけがそろうことを見つけるためである。
fn read_back(shown: &str, last: Pass, show: fn(&Program) -> String) {
    let parsed = parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(show(&parsed), shown, "the printed Core IR must read back");
    let verified = match last {
        Pass::Translate | Pass::Optimize => eml_core_ir::verify_translated(&parsed),
        Pass::Evidence => eml_core_ir::verify_evidence(&parsed),
    };
    if let Err(error) = verified {
        panic!("the Core IR read back must verify: {error}\n{shown}");
    }
}

/// 誤りのないプログラムを、`last` のパスの直後の LIR にして表示する。
pub fn lir_text(text: &str, last: eml_lir::Pass) -> String {
    lir_text_files(text, &[], last)
}

/// `modules` は根からの相対パスと本文の組である (`eml_test_support::lir_until_files`)。
pub fn lir_text_files(entry: &str, modules: &[(&str, &str)], last: eml_lir::Pass) -> String {
    let shown = eml_lir::pretty(&eml_test_support::lir_until_files(entry, modules, last));
    lir_read_back(&shown, last);
    shown
}

/// `read_back` の LIR の側。
fn lir_read_back(shown: &str, last: eml_lir::Pass) {
    let parsed = eml_lir::parse(shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(
        eml_lir::pretty(&parsed),
        shown,
        "the printed LIR must read back"
    );
    let verified = match last {
        eml_lir::Pass::Boxing => eml_lir::verify_scopes(&parsed),
        eml_lir::Pass::Flatten | eml_lir::Pass::Contract => eml_lir::verify_flattened(&parsed),
        eml_lir::Pass::Perceus | eml_lir::Pass::Reuse => eml_lir::verify(&parsed),
    };
    if let Err(error) = verified {
        panic!("the LIR read back must verify: {error}\n{shown}");
    }
}

/// 名前で選んだ関数の表示。ほかの関数 (`entry$main` など) を期待値から外すため。内部の関数は `internal fn` の行から
/// 始まる。
pub fn function(shown: &str, name: &str) -> String {
    let header = shown
        .find(&format!("fn {name}("))
        .or_else(|| shown.find(&format!("fn {name:?}(")))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
    let start = shown[..header].rfind('\n').map_or(0, |newline| newline + 1);
    let end = shown[start..]
        .find("\n}\n")
        .expect("a function ends with `}`")
        + start
        + 3;
    shown[start..end].to_string()
}

/// REPL のセッションの入力を入力 1, 2, … として読み、最後の入力を入口にして、`last` のパスの直後の Core IR にして
/// 表示する。
pub fn session_core_text(inputs: &[&str], last: Pass) -> String {
    let (program, typed, files) = session(inputs);
    let entry = program
        .input()
        .expect("the last input is an input function");
    let shown = pretty(&eml_core_ir::core_until(
        &program, &typed, entry, &files, last,
    ));
    read_back(&shown, last, pretty);
    shown
}

/// `session_core_text` の、Perceus の後の LIR の形。
pub fn session_lir_text(inputs: &[&str]) -> String {
    let (program, typed, files) = session(inputs);
    let entry = program
        .input()
        .expect("the last input is an input function");
    let shown = eml_lir::pretty(&eml_core_ir::lower(&program, &typed, entry, &files));
    lir_read_back(&shown, eml_lir::Pass::Reuse);
    shown
}

fn session(
    inputs: &[&str],
) -> (
    eml_hir::Program,
    eml_types::TypedProgram,
    eml_diagnostics::SourceFiles,
) {
    let inputs: Vec<eml_hir::SessionInput> = inputs
        .iter()
        .zip(1..)
        .map(|(&text, number)| eml_hir::SessionInput {
            number,
            text,
            form: eml_hir::InputForm::Typed,
        })
        .collect();
    let (loaded, mut diagnostics, _) = eml_hir::load_session(
        eml_hir::STD,
        &inputs,
        "",
        "",
        &NoModules,
        &eml_hir::ImportCache::default(),
    );
    let (def_map, found) = eml_hir::def_map(&loaded.modules);
    diagnostics.extend(found);
    let (program, found) = eml_hir::lower(&def_map, &loaded.modules);
    diagnostics.extend(found);
    let (typed, found) = eml_types::check(&program, &loaded.files);
    diagnostics.extend(found);
    let errors: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| format!("{} {}", d.code, d.message))
        .collect();
    assert!(errors.is_empty(), "the session has errors: {errors:?}");
    (program, typed, loaded.files)
}

/// セッションのテストは import を書かない。
struct NoModules;

impl eml_hir::ModuleSource for NoModules {
    fn read(&self, _: &eml_hir::ModulePath) -> Result<String, eml_hir::ReadError> {
        Err(eml_hir::ReadError::NotFound)
    }
}

/// 関数の `switch` のうち、EP が yield の地点の後に置く確認 (`prim yielding()` の結果での `switch`) を除いた数。
/// IO の extern も yield の地点なので、文の `if` が作る `switch` だけを数えるときに使う
/// (docs/spec/core-ir.md の「IO の extern」)。
pub fn if_switches(function: &eml_lir::LirFn) -> usize {
    let checks: std::collections::HashSet<eml_lir::VarId> = function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            eml_lir::Stmt::Let {
                var,
                rhs: eml_lir::Rhs::Prim(eml_lir::Prim::Yielding, _),
            } => Some(*var),
            _ => None,
        })
        .collect();
    function
        .blocks
        .iter()
        .filter(|block| match &block.term {
            eml_lir::Term::Switch {
                scrutinee: eml_lir::Atom::Var(var),
                ..
            } => !checks.contains(var),
            term => matches!(term, eml_lir::Term::Switch { .. }),
        })
        .count()
}
