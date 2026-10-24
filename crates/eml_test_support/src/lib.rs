//! テストのためにパイプラインを組む処理 (診断のないことを確かめるものを含む)、診断を文字列にする処理 (docs/implementation/testing.md)。
//!
//! この crate は、テストする crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テスト
//! からだけである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。
//!
//! パイプラインは `eml_cli::Session` で組み、ここでは包むだけにする。CLI と同じ順で段階をつなぎ、同じ診断を集める
//! ため。段階は feature で選ぶ (`hir` < `types` < `core` < `run`)。各 crate は自分の段階までを有効にし、下流の crate に
//! テストを依存させない。

use std::fmt::Write;

#[cfg(feature = "hir")]
use eml_cli::Session;
use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles, sort_diagnostics};
#[cfg(feature = "run")]
use eml_vm::{RunConfig, RunStats, RuntimeError};

pub struct Parsed {
    pub files: SourceFiles,
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

/// `SourceFiles` は Clone できないので、ファイルは `Session` ごと持ち、メソッドで出す。
#[cfg(feature = "hir")]
pub struct Lowered {
    session: Session,
    pub program: eml_hir::Program,
    /// 読み込み、構文、def_map、HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "hir")]
impl Lowered {
    pub fn files(&self) -> &SourceFiles {
        self.session.files()
    }

    pub fn file(&self) -> FileId {
        self.session.entry()
    }
}

#[cfg(feature = "types")]
pub struct Checked {
    session: Session,
    pub program: eml_hir::Program,
    pub typed: eml_types::TypedProgram,
    /// 読み込み、構文、HIR、型の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(feature = "types")]
impl Checked {
    pub fn files(&self) -> &SourceFiles {
        self.session.files()
    }

    pub fn file(&self) -> FileId {
        self.session.entry()
    }
}

/// UI テスト以外のテストで登録する、入口のファイルの表示のパス。
pub const ENTRY_PATH: &str = "test.em";

pub fn source(text: &str) -> (SourceFiles, FileId) {
    let mut files = SourceFiles::new();
    let file = files.add(ENTRY_PATH, text);
    (files, file)
}

/// 構文の段だけを通す。どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。構文解析
/// するのは `SourceFiles` に保存したテキスト (先頭の BOM を除いたもの) である (docs/spec/lexical.md)。
pub fn parse(text: &str) -> Parsed {
    let (files, file) = source(text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    assert_eq!(
        parse.syntax().text().to_string(),
        files.text(file),
        "tree must be lossless"
    );
    sort_diagnostics(&mut diagnostics);
    Parsed {
        files,
        file,
        parse,
        diagnostics,
    }
}

/// `modules` は根からの相対パス (`"Report/Csv.em"`) と本文の組である。
#[cfg(feature = "hir")]
fn load(entry: &str, modules: &[(&str, &str)]) -> Session {
    Session::load(ENTRY_PATH, entry, &eml_hir::MemorySource(modules))
}

/// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替える。標準ライブラリの中の item の扱いを確かめるテストのため。
/// 並びは `Prelude.em` と、本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引くので、
/// 足りないと panic する。
#[cfg(feature = "hir")]
fn load_with_std(std: &[(&str, &str)], entry: &str) -> Session {
    Session::load_with_std(std, ENTRY_PATH, entry, &eml_hir::MemorySource(&[]))
}

#[cfg(feature = "hir")]
pub fn lower(text: &str) -> Lowered {
    lower_files(text, &[])
}

/// 診断には読み込みの段のものも入る。
#[cfg(feature = "hir")]
pub fn lower_files(entry: &str, modules: &[(&str, &str)]) -> Lowered {
    lower_session(load(entry, modules))
}

/// 埋め込んだ標準ライブラリの `name` (`"String.em"` など) の組。差し替えた木に本物のモジュールを足すのに使う。
#[cfg(feature = "hir")]
pub fn std_module(name: &str) -> (&'static str, &'static str) {
    *eml_hir::STD
        .iter()
        .find(|(path, _)| *path == name)
        .expect("an embedded std module")
}

#[cfg(feature = "hir")]
pub fn lower_with_std(std: &[(&str, &str)], entry: &str) -> Lowered {
    lower_session(load_with_std(std, entry))
}

#[cfg(feature = "hir")]
fn lower_session(session: Session) -> Lowered {
    let eml_cli::Lowered {
        program,
        diagnostics,
    } = session.lower();
    Lowered {
        session,
        program,
        diagnostics,
    }
}

#[cfg(feature = "hir")]
pub fn def_map(text: &str) -> (eml_hir::DefMap, Vec<String>) {
    def_map_files(text, &[])
}

/// 診断は、読み込みの段と def_map の段のものである。
#[cfg(feature = "hir")]
pub fn def_map_files(entry: &str, modules: &[(&str, &str)]) -> (eml_hir::DefMap, Vec<String>) {
    let session = load(entry, modules);
    let eml_cli::DefMapped {
        def_map,
        diagnostics,
    } = session.def_map();
    (def_map, short(session.files(), &diagnostics))
}

/// 前提として診断のないソースを使うテストのため。条件を緩めないよう、警告も1件として数える。
pub fn parse_clean(text: &str) -> Parsed {
    let parsed = parse(text);
    assert_clean(&parsed.files, &parsed.diagnostics);
    parsed
}

#[cfg(feature = "hir")]
pub fn lower_clean(text: &str) -> Lowered {
    let lowered = lower(text);
    assert_clean(lowered.files(), &lowered.diagnostics);
    lowered
}

fn assert_clean(files: &SourceFiles, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        short_text(files, diagnostics)
    );
}

#[cfg(feature = "types")]
pub fn check(text: &str) -> Checked {
    check_files(text, &[])
}

#[cfg(feature = "types")]
pub fn check_files(entry: &str, modules: &[(&str, &str)]) -> Checked {
    check_session(load(entry, modules))
}

/// 標準ライブラリを差し替えて型検査をする。並びの条件は `lower_with_std` と同じである。
#[cfg(feature = "types")]
pub fn check_with_std(std: &[(&str, &str)], entry: &str) -> Checked {
    check_session(load_with_std(std, entry))
}

#[cfg(feature = "types")]
fn check_session(session: Session) -> Checked {
    let eml_cli::Checked {
        program,
        typed,
        diagnostics,
    } = session.check();
    Checked {
        session,
        program,
        typed,
        diagnostics,
    }
}

#[cfg(feature = "core")]
pub fn lir(text: &str) -> eml_lir::Program {
    lir_files(text, &[])
}

#[cfg(feature = "core")]
pub fn lir_files(entry: &str, modules: &[(&str, &str)]) -> eml_lir::Program {
    compiled(load(entry, modules).compile())
}

/// 確かめたいパスの直後の LIR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn lir_until(text: &str, last: eml_lir::Pass) -> eml_lir::Program {
    lir_until_files(text, &[], last)
}

#[cfg(feature = "core")]
pub fn lir_until_files(
    entry: &str,
    modules: &[(&str, &str)],
    last: eml_lir::Pass,
) -> eml_lir::Program {
    compiled(load(entry, modules).compile_until(last))
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: eml_core_ir::Pass) -> eml_core_ir::Program {
    core_until_files(text, &[], last)
}

#[cfg(feature = "core")]
pub fn core_until_files(
    entry: &str,
    modules: &[(&str, &str)],
    last: eml_core_ir::Pass,
) -> eml_core_ir::Program {
    compiled(load(entry, modules).compile_core_until(last))
}

/// `eml run` と同じく、エラーがあれば IR を作らない。`main` がないこともエラーである。
#[cfg(feature = "core")]
fn compiled<P>(compiled: eml_cli::Compiled<P>) -> P {
    compiled
        .program
        .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics))
}

/// 実行のテストは、つねに検査モードで流す (docs/implementation/testing.md)。
#[cfg(feature = "run")]
pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    run_files(text, &[])
}

#[cfg(feature = "run")]
pub fn run_files(entry: &str, modules: &[(&str, &str)]) -> (String, Result<(), RuntimeError>) {
    let (out, result) = execute(&lir_files(entry, modules), &checked());
    (out, result.map(|_| ()))
}

/// `RunStats` (仕事の回数と、ヒープの物体とスタックの語の数の最大) も返す。数を比べるテスト (`eml_vm` の
/// `scaling.rs` と `bench.rs`) のため。
#[cfg(feature = "run")]
pub fn run_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    execute(&lir(text), &checked())
}

/// `run_stats` と同じく流し、スクリプトに引数 `args` を渡す (`Env.args`)。大きさを引数で受ける基準のプログラムを、
/// 大きさを変えて数えるテストのため。
#[cfg(feature = "run")]
pub fn run_stats_with_args(text: &str, args: &[&str]) -> (String, Result<RunStats, RuntimeError>) {
    let host = eml_cli::CaptureHost::new(std::path::PathBuf::new())
        .with_args(args.iter().map(std::ffi::OsString::from).collect());
    let out = host.output();
    let result =
        eml_cli::execute(&lir(text), &checked(), host.into_ref()).map(|finished| finished.stats);
    (out.contents(), result)
}

/// `run_stats` と同じく流し、標準入力に `input` を与える。標準入力を読むループを、入力の大きさを変えて数えるテストのため。
#[cfg(feature = "run")]
pub fn run_stats_with_stdin(text: &str, input: &[u8]) -> (String, Result<RunStats, RuntimeError>) {
    let host = eml_cli::CaptureHost::new(std::path::PathBuf::new()).with_stdin(input.to_vec());
    let out = host.output();
    let result =
        eml_cli::execute(&lir(text), &checked(), host.into_ref()).map(|finished| finished.stats);
    (out.contents(), result)
}

/// 上限のない新しい `Control` を持つ、検査モードの設定。止める理由のビットは `Control::clear` でしか下りないので、
/// 設定を使い回すと前の実行で立ったビットで次の実行が止まる。そのため実行ごとに作る。
#[cfg(feature = "run")]
fn checked() -> RunConfig {
    RunConfig::default().with_debug_heap(true)
}

/// LIR を実行する。出力はメモリの Host に捕まえ、相対パスはカレントディレクトリを基準にする。上限の値に頼るテストは、
/// `RunConfig` を自分で作って渡す。
#[cfg(feature = "run")]
pub fn execute(
    program: &eml_lir::Program,
    config: &RunConfig,
) -> (String, Result<RunStats, RuntimeError>) {
    let (host, out) = eml_cli::capture_host(std::path::PathBuf::new());
    let result = eml_cli::execute(program, config, host).map(|finished| finished.stats);
    (out.contents(), result)
}

/// 1件を `E0001 1:2 message` の1行にする。期待値を読みやすくするため、位置はバイトではなく行と列にする。
pub fn short(files: &SourceFiles, diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| format!("{} {} {}", d.code, position(files, &d.primary), d.message))
        .collect()
}

/// `full` と同じく文字列にして、段階の表示の後ろにそのまま足せるようにする。
pub fn short_text(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    short(files, diagnostics)
        .into_iter()
        .map(|line| line + "\n")
        .collect()
}

/// 診断の形式は段階のテストごとに違うので、区切り方だけをここでそろえる。
pub fn with_diagnostics(mut dump: String, diagnostics: &str) -> String {
    if !diagnostics.is_empty() {
        dump.push_str("---\n");
        dump.push_str(diagnostics);
    }
    dump
}

/// fix のある診断ごとに、先頭の行に続けて編集を `開始..終了 "置き換える文字列"` の形で並べる。fix のない診断は出さない。
pub fn fixes(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let Some(fix) = &d.fix else { continue };
        writeln!(
            out,
            "{} {} {}",
            d.code,
            position(files, &d.primary),
            fix.title
        )
        .unwrap();
        for edit in &fix.edits {
            let start = files.line_col(edit.file, edit.range.start());
            let end = files.line_col(edit.file, edit.range.end());
            writeln!(out, "  {start}..{end} {:?}", edit.replacement).unwrap();
        }
    }
    out
}

/// 1件を、先頭の行に続けてラベル、note、help を字下げした行にする。
pub fn full(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    report(diagnostics, |label| position(files, label).to_string())
}

/// 位置の前にファイルの表示のパスを付けた `full`。複数のモジュールのテストで、診断がどのファイルを指すかを読み分けるため。
pub fn full_with_paths(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    report(diagnostics, |label| located(files, label))
}

/// `test.em 1:2` の形の位置。
pub fn located(files: &SourceFiles, label: &Label) -> String {
    format!("{} {}", files.path(label.file), position(files, label))
}

fn report(diagnostics: &[Diagnostic], at: impl Fn(&Label) -> String) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let primary = at(&d.primary);
        writeln!(out, "{} {primary} {}", d.code, d.message).unwrap();
        writeln!(out, "  {primary} {}", d.primary.message).unwrap();
        for label in &d.secondary {
            writeln!(out, "  {} {}", at(label), label.message).unwrap();
        }
        for note in &d.notes {
            writeln!(out, "  note: {note}").unwrap();
        }
        for help in &d.help {
            writeln!(out, "  help: {help}").unwrap();
        }
    }
    out
}

fn position(files: &SourceFiles, label: &Label) -> LineCol {
    files.line_col(label.file, label.range.start())
}
