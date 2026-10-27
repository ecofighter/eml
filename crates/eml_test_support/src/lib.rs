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
use eml_vm::{CaptureHost, RunConfig, RunStats, RuntimeError};

pub struct Parsed {
    pub files: SourceFiles,
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

/// 段階の結果に、それを作った `Session` を添えたもの。`SourceFiles` は Clone できないので、ファイルは `Session` ごと
/// 持ち、メソッドで出す。段階の結果の欄は `Deref` で読む。
#[cfg(feature = "hir")]
pub struct WithSession<T> {
    session: Session,
    stage: T,
}

#[cfg(feature = "hir")]
impl<T> WithSession<T> {
    pub fn files(&self) -> &SourceFiles {
        self.session.files()
    }

    pub fn file(&self) -> FileId {
        self.session.entry()
    }

    /// 段階の結果の欄を手放して使うテストのため。
    pub fn into_stage(self) -> T {
        self.stage
    }
}

#[cfg(feature = "hir")]
impl<T> std::ops::Deref for WithSession<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.stage
    }
}

/// `entry` を入口にして IR を作る入力。`main` でない関数を入口にするテストのため。
#[cfg(feature = "core")]
impl WithSession<eml_cli::Checked> {
    pub fn input(&self, entry: eml_hir::FunctionId) -> eml_core_ir::Input<'_> {
        eml_core_ir::Input {
            hir: &self.program,
            typed: &self.typed,
            entry,
            files: self.files(),
        }
    }
}

/// HIR と、読み込み、構文、def_map、HIR の診断を、表示と同じ順 (`sort_diagnostics`) に並べたもの。
#[cfg(feature = "hir")]
pub type Lowered = WithSession<eml_cli::Lowered>;

/// HIR、型検査の結果と、読み込みから型までの診断を、表示と同じ順に並べたもの。
#[cfg(feature = "types")]
pub type Checked = WithSession<eml_cli::Checked>;

/// UI テスト以外のテストで登録する、入口のファイルの表示のパス。
pub const ENTRY_PATH: &str = "test.em";

/// パイプラインに通すソースの集まり。入口の本文だけ (`&str` から作る)、依存先のモジュールを足したもの (`with_modules`)、
/// 標準ライブラリを差し替えたもの (`with_std`) の3つがある。段階の関数はどれもこれを受け取るので、ソースの組み方と
/// 止める段階を別々に選べる。
#[cfg(feature = "hir")]
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    std: Option<&'a [(&'a str, &'a str)]>,
    entry: &'a str,
    modules: &'a [(&'a str, &'a str)],
}

#[cfg(feature = "hir")]
impl<'a> From<&'a str> for Sources<'a> {
    fn from(entry: &'a str) -> Sources<'a> {
        with_modules(entry, &[])
    }
}

#[cfg(feature = "hir")]
impl<'a> From<&'a String> for Sources<'a> {
    fn from(entry: &'a String) -> Sources<'a> {
        with_modules(entry, &[])
    }
}

/// 入口の本文と、依存先のモジュール。`modules` は根からの相対パス (`"Report/Csv.em"`) と本文の組である。
#[cfg(feature = "hir")]
pub fn with_modules<'a>(entry: &'a str, modules: &'a [(&'a str, &'a str)]) -> Sources<'a> {
    Sources {
        std: None,
        entry,
        modules,
    }
}

/// 標準ライブラリを `(ファイル名, 本文)` の並びに差し替える。標準ライブラリの中の item の扱いを確かめるテストのため。
/// 並びは `Prelude.em` と、本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引くので、
/// 足りないと panic する。
#[cfg(feature = "hir")]
pub fn with_std<'a>(std: &'a [(&'a str, &'a str)], entry: &'a str) -> Sources<'a> {
    Sources {
        std: Some(std),
        entry,
        modules: &[],
    }
}

#[cfg(feature = "hir")]
impl Sources<'_> {
    pub fn session(self) -> Session {
        let source = eml_hir::MemorySource(self.modules);
        match self.std {
            Some(std) => Session::load_with_std(std, ENTRY_PATH, self.entry, &source),
            None => Session::load(ENTRY_PATH, self.entry, &source),
        }
    }
}

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

/// 診断には読み込みの段のものも入る。
#[cfg(feature = "hir")]
pub fn lower<'a>(sources: impl Into<Sources<'a>>) -> Lowered {
    let session = sources.into().session();
    let stage = session.lower();
    WithSession { session, stage }
}

/// 埋め込んだ標準ライブラリの `name` (`"String.em"` など) の組。差し替えた木に本物のモジュールを足すのに使う。
#[cfg(feature = "hir")]
pub fn std_module(name: &str) -> (&'static str, &'static str) {
    *eml_hir::STD
        .iter()
        .find(|(path, _)| *path == name)
        .expect("an embedded std module")
}

/// 診断は、読み込みの段と def_map の段のものである。
#[cfg(feature = "hir")]
pub fn def_map<'a>(sources: impl Into<Sources<'a>>) -> (eml_hir::DefMap, Vec<String>) {
    let session = sources.into().session();
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
pub fn check<'a>(sources: impl Into<Sources<'a>>) -> Checked {
    checked_session(sources.into().session())
}

/// REPL のセッションの入力を読み、型検査までを通す。入力の関数を入口にした Core IR を見るテストのため。
#[cfg(feature = "types")]
pub fn check_inputs(inputs: &[eml_hir::SessionInput]) -> Checked {
    let (session, _) = Session::load_session(
        inputs,
        None,
        &eml_hir::MemorySource(&[]),
        &eml_hir::ImportCache::default(),
    );
    checked_session(session)
}

#[cfg(feature = "types")]
fn checked_session(session: Session) -> Checked {
    let stage = session.check();
    WithSession { session, stage }
}

#[cfg(feature = "core")]
pub fn lir<'a>(sources: impl Into<Sources<'a>>) -> eml_lir::Program {
    compiled(sources.into().session().compile())
}

/// 確かめたいパスの直後の LIR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn lir_until<'a>(sources: impl Into<Sources<'a>>, last: eml_lir::Pass) -> eml_lir::Program {
    compiled(
        sources
            .into()
            .session()
            .compile_with(|input| eml_core_ir::lower_until(input, last)),
    )
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until<'a>(
    sources: impl Into<Sources<'a>>,
    last: eml_core_ir::Pass,
) -> eml_core_ir::Program {
    compiled(
        sources
            .into()
            .session()
            .compile_with(|input| eml_core_ir::core_until(input, last)),
    )
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
pub fn run<'a>(sources: impl Into<Sources<'a>>) -> (String, Result<(), RuntimeError>) {
    let (out, result) = run_stats(sources);
    (out, result.map(|_| ()))
}

/// `RunStats` (仕事の回数と、ヒープの物体とスタックの語の数の最大) も返す。数を比べるテスト (`eml_vm` の
/// `scaling.rs` と `bench.rs`) のため。
#[cfg(feature = "run")]
pub fn run_stats<'a>(sources: impl Into<Sources<'a>>) -> (String, Result<RunStats, RuntimeError>) {
    run_stats_on(sources, capture())
}

/// `RunStats` のうち、スケジュールの方針で変わりうる欄を 0 にする。方針を変えた実行どうしは、残りの欄で比べる。
/// 0 にする欄とその理由は次のとおりで、比べる欄がスケジュールによらない理由は
/// docs/implementation/testing.md の「seed で流す」にある。
/// - `peak_objects`: 同時に生きている物体の数は、子の実行が交互に進む順で変わる
/// - `cancels`: 取り消しが届くと、届いた時点からそのタスクの仕事が変わる
#[cfg(feature = "run")]
pub fn schedule_independent(mut stats: RunStats) -> RunStats {
    stats.peak_objects = 0;
    stats.cancels = 0;
    stats
}

/// `run_stats` と同じく流し、Host を呼び出し側が作る。スクリプトの引数 (`Env.args`) や標準入力を、大きさを変えて
/// 与えるテストのため。
#[cfg(feature = "run")]
pub fn run_stats_on<'a>(
    sources: impl Into<Sources<'a>>,
    host: CaptureHost,
) -> (String, Result<RunStats, RuntimeError>) {
    execute_on(&lir(sources), &checked(), host)
}

/// 上限のない新しい `Control` を持つ、検査モードの設定。止める理由のビットは `Control::clear` でしか下りないので、
/// 設定を使い回すと前の実行で立ったビットで次の実行が止まる。そのため実行ごとに作る。
#[cfg(feature = "run")]
fn checked() -> RunConfig {
    RunConfig::default().with_debug_heap(true)
}

/// 出力をメモリに捕まえ、相対パスをカレントディレクトリを基準にする Host。
#[cfg(feature = "run")]
pub fn capture() -> CaptureHost {
    CaptureHost::new(std::path::PathBuf::new())
}

/// LIR を `capture()` の Host で実行する。上限の値に頼るテストは、`RunConfig` を自分で作って渡す。
#[cfg(feature = "run")]
pub fn execute(
    program: &eml_lir::Program,
    config: &RunConfig,
) -> (String, Result<RunStats, RuntimeError>) {
    execute_on(program, config, capture())
}

/// LIR を `host` で実行し、捕まえた標準出力を返す。
#[cfg(feature = "run")]
pub fn execute_on(
    program: &eml_lir::Program,
    config: &RunConfig,
    host: CaptureHost,
) -> (String, Result<RunStats, RuntimeError>) {
    let out = host.output();
    let result = eml_vm::execute(program, config, host.into_ref()).map(|finished| finished.stats);
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
