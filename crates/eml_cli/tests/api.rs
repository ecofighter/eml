use std::sync::{Arc, Mutex, PoisonError};

use eml_cli::{
    Capture, Control, FsProvider, Limits, ModulePath, ModuleSource, Outcome, ReadError, Repl,
    RunConfig, Session, capture_host, execute,
};
use eml_diagnostics::{Diagnostic, has_errors};
use eml_test_support::MemorySource;

use crate::common::temp_project;

/// import のない入口だけのセッション。
fn single(text: &str) -> Session {
    Session::load("a.em", text, &MemorySource(&[]))
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(|d| d.code.to_string()).collect()
}

fn module_path(segments: &[&str]) -> ModulePath {
    ModulePath(segments.iter().map(|segment| segment.to_string()).collect())
}

#[test]
fn each_stage_collects_the_diagnostics_of_the_stages_before_it() {
    // `nope` は def_map の段の E1001、`h` は型の E2001、`g` は HIR の E1001、`€` は字句の E0001 になる
    let source = MemorySource(&[("Util.em", "")]);
    let text = "import Util (nope)\n\nh : Int\nh = \"s\"\n\nf : Int -> Int\nf x = g x\n€";
    let session = Session::load("a.em", text, &source);
    assert_eq!(codes(&session.def_map().diagnostics), ["E1001", "E0001"]);
    assert_eq!(
        codes(&session.lower().diagnostics),
        ["E1001", "E1001", "E0001"]
    );
    assert_eq!(
        codes(&session.check().diagnostics),
        ["E1001", "E2001", "E1001", "E0001"]
    );
}

#[test]
fn load_with_std_reads_the_given_standard_library() {
    let prelude = format!("{}\npub extra : Int\nextra = 1\n", eml_hir::PRELUDE_SOURCE);
    let session = Session::load_with_std(
        &[("Prelude.em", &prelude), eml_hir::STD[1]],
        "a.em",
        "f : Int\nf = extra",
        &MemorySource(&[]),
    );
    assert!(session.check().diagnostics.is_empty());
}

/// 使われない `"unused"` を持ち、`s` を2回使うプログラム。
const TWICE: &str = "twice : String -> String\ntwice s =\n  let _ = \"unused\"\n  s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"x\")";

#[test]
fn compile_core_until_stops_after_the_named_pass() {
    // translate の直後の Core IR には、contract が消す前の `"unused"` が残る
    let session = single(TWICE);
    let shown = |last| eml_core_ir::pretty(&session.compile_core_until(last).program.unwrap());
    assert!(shown(eml_core_ir::Pass::Translate).contains("\"unused\""));
}

#[test]
fn compile_until_stops_after_the_named_pass() {
    // contract が使われない `"unused"` を消す。`s` を2回使うので、Perceus の後にだけ `dup` が入る
    let session = single(TWICE);
    let shown = |last| eml_lir::pretty(&session.compile_until(last).program.unwrap());
    assert!(!shown(eml_lir::Pass::Contract).contains("\"unused\""));
    assert!(!shown(eml_lir::Pass::Contract).contains("dup"));
    assert!(shown(eml_lir::Pass::Perceus).contains("dup"));
    assert_eq!(
        shown(eml_lir::Pass::Perceus),
        eml_lir::pretty(&session.compile().program.unwrap())
    );
}

#[test]
fn compile_returns_no_program_when_there_are_errors() {
    let compiled = single("€").compile();
    assert!(has_errors(&compiled.diagnostics));
    assert!(compiled.program.is_none());
}

#[test]
fn compile_returns_a_program_without_errors() {
    let compiled = single("main : Unit -> <IO> Unit\nmain () = ()").compile();
    assert!(compiled.diagnostics.is_empty());
    assert!(compiled.program.is_some());
}

#[test]
fn execute_runs_a_compiled_program() {
    let program = single("main : Unit -> <IO> Unit\nmain () = ()")
        .compile()
        .program
        .unwrap();
    let (host, _) = capture_host(std::path::PathBuf::new());
    assert_eq!(
        execute(&program, &RunConfig::default(), host).map(|_| ()),
        Ok(())
    );
}

#[test]
fn compile_reports_a_missing_main() {
    let compiled = single("f : Int -> Int\nf x = x").compile();
    assert_eq!(codes(&compiled.diagnostics), ["E2003"]);
    assert!(compiled.program.is_none());
}

#[test]
fn check_accepts_a_file_without_main() {
    assert!(
        single("f : Int -> Int\nf x = x")
            .check()
            .diagnostics
            .is_empty()
    );
}

#[test]
fn a_session_registers_the_prelude_for_rendering() {
    let session = single("main : Unit -> <IO> Unit\nmain () = println \"x\"");
    assert!(session.check().diagnostics.is_empty());
    let prelude = session.prelude();
    assert_eq!(session.files().path(prelude), eml_hir::PRELUDE_PATH);
    assert_eq!(session.files().text(prelude), eml_hir::PRELUDE_SOURCE);
    // Prelude の範囲を指す診断を表示しても panic しない
    let label =
        eml_diagnostics::Label::new(prelude, eml_diagnostics::TextRange::empty(0.into()), "here");
    let diagnostic =
        eml_diagnostics::Diagnostic::error(eml_diagnostics::NOT_YET_SUPPORTED, "test", label);
    let rendered = eml_diagnostics::render(&[diagnostic], session.files());
    assert!(rendered.contains("<std>/Prelude.em"));
}

#[test]
fn the_prelude_alone_has_no_diagnostics() {
    assert!(single("").check().diagnostics.is_empty());
}

#[test]
fn user_module_names_leave_out_the_standard_library() {
    let source = MemorySource(&[("Util.em", "")]);
    let session = Session::load("main.em", "import Util\n", &source);
    assert_eq!(session.module_names().next(), Some("Prelude"));
    assert_eq!(
        session.user_module_names().collect::<Vec<_>>(),
        ["Main", "Util"]
    );
}

#[test]
fn a_session_registers_a_dependency_under_the_entry_directory() {
    let source = MemorySource(&[("Report/Csv.em", "x : Int\nx = y")]);
    let session = Session::load("app/main.em", "import Report.Csv\n", &source);
    let diagnostics = session.check().diagnostics;
    assert_eq!(codes(&diagnostics), ["E1001"]);
    assert_eq!(
        session.files().path(diagnostics[0].primary.file),
        "app/Report/Csv.em"
    );
    assert_eq!(session.files().path(session.entry()), "app/main.em");
}

#[test]
fn a_missing_main_points_at_the_entry() {
    let source = MemorySource(&[("Util.em", "pub x : Int\nx = 1")]);
    let session = Session::load("app/main.em", "import Util\n", &source);
    let compiled = session.compile();
    assert_eq!(codes(&compiled.diagnostics), ["E2003"]);
    assert_eq!(compiled.diagnostics[0].primary.file, session.entry());
}

#[test]
fn an_imported_main_does_not_start_the_program() {
    // `main` は入口のモジュールからだけ探すので、import した `main` があっても E2003 にする (docs/spec/types.md)
    let source = MemorySource(&[("Util.em", "pub main : Unit -> <IO> Unit\nmain () = ()")]);
    let session = Session::load("main.em", "import Util (main)\n", &source);
    let compiled = session.compile();
    assert_eq!(codes(&compiled.diagnostics), ["E2003"]);
    assert_eq!(
        compiled.diagnostics[0].message,
        "the entry module does not define `main`"
    );
}

#[test]
fn fs_provider_reads_a_module_under_the_root() {
    let dir = temp_project("fs-read", &[("Report/Csv.em", "pub x : Int\nx = 1\n")]);
    let read = FsProvider::new(&dir).read(&module_path(&["Report", "Csv"]));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(read, Ok("pub x : Int\nx = 1\n".to_string()));
}

#[test]
fn fs_provider_matches_the_case_of_every_segment() {
    let dir = temp_project("fs-case", &[("report/Csv.em", ""), ("Util/csv.em", "")]);
    let provider = FsProvider::new(&dir);
    let directory = provider.read(&module_path(&["Report", "Csv"]));
    let file = provider.read(&module_path(&["Util", "Csv"]));
    let missing = provider.read(&module_path(&["Util", "Json"]));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        directory,
        Err(ReadError::Unreadable(
            "the file is named `report`, and module paths match the case of file names exactly"
                .to_string()
        ))
    );
    assert_eq!(
        file,
        Err(ReadError::Unreadable(
            "the file is named `csv.em`, and module paths match the case of file names exactly"
                .to_string()
        ))
    );
    assert_eq!(missing, Err(ReadError::NotFound));
}

#[test]
fn a_module_in_a_differently_cased_directory_is_not_found() {
    let dir = temp_project("case", &[("report/Csv.em", "pub x : Int\nx = 1\n")]);
    let session = Session::load("main.em", "import Report.Csv\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    let diagnostics = session.check().diagnostics;
    assert_eq!(codes(&diagnostics), ["E1026"]);
    assert_eq!(
        diagnostics[0].message,
        "cannot read module `Report.Csv`: the file is named `report`, and module paths match the case of file names exactly"
    );
}

#[test]
fn a_dependency_that_is_not_utf8_cannot_be_read() {
    let dir = temp_project("utf8", &[]);
    std::fs::write(dir.join("Bad.em"), [0x66, 0x6e, 0xff, 0xfe]).unwrap();
    let session = Session::load("main.em", "import Bad\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    let diagnostics = session.check().diagnostics;
    assert_eq!(codes(&diagnostics), ["E1026"]);
    assert!(
        diagnostics[0]
            .message
            .contains("stream did not contain valid UTF-8"),
        "{}",
        diagnostics[0].message
    );
}

#[test]
fn a_directory_named_like_a_module_file_cannot_be_read() {
    let dir = temp_project("dir-module", &[("Csv.em/x.em", "")]);
    let session = Session::load("main.em", "import Csv\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(codes(&session.check().diagnostics), ["E1026"]);
}

#[test]
fn files_that_are_not_imported_are_not_read() {
    // 根の下の .em をすべて読むのではなく、import をたどる (docs/spec/modules.md)
    let dir = temp_project("unrelated", &[("Broken.em", "€")]);
    let session = Session::load("main.em", "f : Int\nf = 1\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(session.check().diagnostics.is_empty());
}

/// 読み込んだセッションを、別のスレッドに渡して使える (docs/implementation/architecture.md の「CLI と lib API」)。
#[test]
fn session_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Session>();
}

#[test]
fn execute_counts_its_work() {
    let program = single(TWICE).compile().program.unwrap();
    let (host, output) = capture_host(std::path::PathBuf::new());
    let config = RunConfig::default().with_debug_heap(true);
    let stats: eml_cli::RunStats = execute(&program, &config, host).unwrap();
    assert_eq!(output.contents(), "xx\n");
    assert!(stats.string_bytes_copied > 0);
}

/// 深さの上限は 2^27 語である。確保量の上限は物理メモリの 80% で、物理メモリを読める macOS と Linux では有限である。
/// 既定の上限まで実際に再帰するテストは 1 GiB を使うので置かない。
#[test]
fn default_limits_bound_the_depth_and_the_memory() {
    let limits = eml_cli::default_limits();
    assert_eq!(limits.max_depth, 1 << 27);
    assert_eq!(limits.max_evv, usize::MAX);
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(limits.max_bytes > 0);
        assert!(limits.max_bytes < usize::MAX);
    }
}

/// 入力ごとの表示を読むための、`Repl` と出力の組。
struct Console {
    repl: Repl,
    out: Capture,
    read: usize,
}

impl Console {
    fn new(config: &RunConfig) -> Console {
        Console::with_source(config, Box::new(MemorySource(&[])))
    }

    fn with_source(config: &RunConfig, source: Box<dyn ModuleSource + Send>) -> Console {
        let (host, out) = capture_host(Default::default());
        Console {
            repl: Repl::new(config, source, "", host),
            out,
            read: 0,
        }
    }

    /// 入力を評価し、結果と、その入力の表示と、診断の番号を返す。
    fn eval(&mut self, text: &str) -> (Outcome, String, Vec<String>) {
        let evaluated = self.repl.eval(text);
        let all = self.out.contents();
        let output = all[self.read..].to_string();
        self.read = all.len();
        (evaluated.outcome, output, codes(&evaluated.diagnostics))
    }
}

/// `Session` と同じく、後で並列のコンパイルやクエリに載せる妨げにしない (docs/spec/modules.md の「REPL のセッション」)。
#[test]
fn a_repl_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Repl>();
}

#[test]
fn an_expression_prints_its_value() {
    let mut console = Console::new(&RunConfig::default());
    assert_eq!(
        console.eval("1 + 2"),
        (Outcome::Accepted, "3\n".to_string(), vec![])
    );
}

#[test]
fn a_definition_is_kept_for_later_inputs() {
    let mut console = Console::new(&RunConfig::default());
    assert_eq!(
        console.eval("inc : Int -> Int\ninc x = x + 1"),
        (Outcome::Accepted, String::new(), vec![])
    );
    assert_eq!(console.eval("inc 41").1, "42\n");
}

#[test]
fn a_rejected_input_leaves_the_session_unchanged() {
    let mut console = Console::new(&RunConfig::default());
    assert_eq!(
        console.eval("bad : Int -> Int\nbad x = x + \"s\""),
        (Outcome::Rejected, String::new(), vec!["E2001".to_string()])
    );
    assert_eq!(console.eval("bad 1").2, ["E1001"]);
}

#[test]
fn diagnostics_point_into_the_files_of_their_evaluation() {
    let (host, _) = capture_host(Default::default());
    let mut repl = Repl::new(&RunConfig::default(), Box::new(MemorySource(&[])), "", host);
    assert_eq!(repl.eval("1 + 1").outcome, Outcome::Accepted);
    let evaluated = repl.eval("nope");
    let primary = &evaluated.diagnostics[0].primary;
    assert_eq!(evaluated.files.path(primary.file), "<input 2>");
}

#[test]
fn an_input_of_only_comments_does_nothing() {
    let (host, _) = capture_host(Default::default());
    let mut repl = Repl::new(&RunConfig::default(), Box::new(MemorySource(&[])), "", host);
    assert_eq!(repl.eval("").outcome, Outcome::Empty);
    assert_eq!(repl.eval("-- a note").outcome, Outcome::Empty);
    // 空の入力は番号を進めないので、次の入力が1番になる
    let evaluated = repl.eval("nope");
    let primary = &evaluated.diagnostics[0].primary;
    assert_eq!(evaluated.files.path(primary.file), "<input 1>");
}

#[test]
fn a_runtime_error_does_not_end_the_session() {
    let mut console = Console::new(&RunConfig::default());
    let (outcome, _, _) = console.eval("1 / 0");
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(console.eval("let x = 5").0, Outcome::Accepted);
    assert_eq!(console.eval("x * 2").1, "10\n");
}

#[test]
fn a_failed_let_binds_nothing() {
    let mut console = Console::new(&RunConfig::default());
    let (outcome, _, _) = console.eval("let y = 1 / 0");
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(console.eval("y").2, ["E1001"]);
}

/// 止める理由のビットは `eval` の始めに下りる (docs/spec/object-model.md の「実行の制御」)。
#[test]
fn each_input_clears_the_stop_bits() {
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    let mut console = Console::new(&config);
    control.interrupt();
    assert_eq!(console.eval("1 + 2").1, "3\n");
}

/// 確保量の上限で止まると `MEMORY` のビットが残る。`()` は何も確保しないが、`entry$` の呼び出しの安全点を通るので、
/// ビットが残っていれば止まる。失敗した実行が残したページは確保量に数えられたままなので、大きな確保をする入力は、
/// S11b の回復までまた止まる (docs/spec/object-model.md の「実行の制御」)。
#[test]
fn the_memory_stop_bit_is_cleared_for_the_next_input() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits {
            max_bytes: 1 << 22,
            ..Limits::default()
        }));
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(control);
        let mut console = Console::new(&config);
        let build = "build : Int -> List Int -> List Int\n\
                     build n acc = if n == 0 then acc else build (n - 1) (n :: acc)";
        assert_eq!(console.eval(build).0, Outcome::Accepted);
        let (outcome, _, _) = console.eval("let big = build 1000000 []");
        let Outcome::Failed(error) = outcome else {
            panic!("expected the memory limit, got {outcome:?} (debug_heap = {debug_heap})");
        };
        assert!(
            error
                .to_string()
                .starts_with("the memory limit is exceeded"),
            "{error}"
        );
        assert_eq!(
            console.eval("()").0,
            Outcome::Accepted,
            "debug_heap = {debug_heap}"
        );
    }
}

#[test]
fn close_finds_no_leaks_after_clean_inputs() {
    let mut console = Console::new(&RunConfig::default().with_debug_heap(true));
    for input in [
        "pair : Int -> (Int, String)\npair n = (n, \"x\")",
        "let p = pair 1",
        "p",
        "let q = [p, p]",
        "q",
    ] {
        assert_eq!(console.eval(input).0, Outcome::Accepted, "{input}");
    }
    assert_eq!(console.repl.close().leaks, None);
}

/// テストの間に中身を書き換えられる `ModuleSource`。
#[derive(Clone, Default)]
struct Changing(Arc<Mutex<Vec<(String, String)>>>);

impl Changing {
    fn set(&self, path: &str, text: &str) {
        let mut files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        files.retain(|(file, _)| file != path);
        files.push((path.to_string(), text.to_string()));
    }
}

impl ModuleSource for Changing {
    fn read(&self, path: &ModulePath) -> Result<String, ReadError> {
        let files = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        files
            .iter()
            .find(|(file, _)| *file == path.file_path())
            .map(|(_, text)| text.clone())
            .ok_or(ReadError::NotFound)
    }
}

/// 読み直すと、同じ名前の関数が違うコードになるので、最初に読んだテキストを使い続ける
/// (docs/spec/modules.md の「REPL のセッション」)。
#[test]
fn an_imported_module_keeps_the_text_read_first() {
    let source = Changing::default();
    source.set("Util.em", "pub v : Int\nv = 1");
    let mut console = Console::with_source(&RunConfig::default(), Box::new(source.clone()));
    assert_eq!(console.eval("import Util").0, Outcome::Accepted);
    assert_eq!(console.eval("Util.v").1, "1\n");
    source.set("Util.em", "pub v : Int\nv = 2");
    assert_eq!(console.eval("Util.v").1, "1\n");
}

/// 受け付けなかった入力が読んだものは、キャッシュに残さない。
#[test]
fn a_rejected_import_reads_the_module_again() {
    let source = Changing::default();
    let mut console = Console::with_source(&RunConfig::default(), Box::new(source.clone()));
    assert_eq!(console.eval("import Later").2, ["E1026"]);
    source.set("Later.em", "pub w : Int\nw = 7");
    assert_eq!(console.eval("import Later").0, Outcome::Accepted);
    assert_eq!(console.eval("Later.w").1, "7\n");
}

/// 警告は、受け付けたときに一度だけ返す。セッション全体を入力ごとに検査し直すので、前の入力の警告も毎回出るため。
#[test]
fn a_warning_is_reported_once() {
    let mut console = Console::new(&RunConfig::default());
    let (outcome, _, codes) =
        console.eval("f : Int -> Int\nf x = match x with\n  | _ -> 0\n  | 1 -> 1");
    assert_eq!(
        (outcome, codes),
        (Outcome::Accepted, vec!["E4004".to_string()])
    );
    assert_eq!(
        console.eval("f 3"),
        (Outcome::Accepted, "0\n".to_string(), vec![])
    );
}

/// handler の中で実行時エラーが起きても、evv が空に戻り、次の入力のエフェクトが正しく動く
/// (docs/spec/vm.md の「追記」)。
#[test]
fn a_runtime_error_inside_a_handler_leaves_no_handler_behind() {
    for config in [
        RunConfig::default(),
        RunConfig::default().with_debug_heap(true),
    ] {
        let mut console = Console::new(&config);
        let (outcome, _, codes) = console.eval("effect Counter where\n  tick : Unit -> Int");
        assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
        let (outcome, _, _) = console.eval("handle 1 / (tick () - 5) with\n  | tick () k -> k 5");
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        // evv が残っていれば、表示の後の終わりの確かめで `Broken` になる
        let (outcome, output, codes) =
            console.eval("handle tick () + tick () with\n  | tick () k -> k 2");
        assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
        assert_eq!(output, "4\n");
    }
}

/// 実行時エラーの前に書いた表示は、エラーより前に host に届く。
#[test]
fn output_before_a_runtime_error_is_flushed() {
    let mut console = Console::new(&RunConfig::default());
    let (outcome, _, codes) =
        console.eval("boom : Unit -> <IO> Int\nboom () =\n  println \"before\"\n  1 / 0");
    assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
    let (outcome, output, _) = console.eval("boom ()");
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(output, "before\n");
}

/// 入力を 100 個重ねても、どの入力も前の束縛を正しく読む。入力ごとに全体を検査し直す費用の目安でもある。
#[test]
fn a_hundred_inputs_keep_every_binding() {
    let mut console = Console::new(&RunConfig::default());
    for n in 1..=100 {
        let (outcome, _, codes) = console.eval(&format!("let x{n} = {n}"));
        assert_eq!(outcome, Outcome::Accepted, "{codes:?}");
    }
    let sum = (1..=100)
        .map(|n| format!("x{n}"))
        .collect::<Vec<_>>()
        .join(" + ");
    assert_eq!(console.eval(&sum).1, "5050\n");
}

/// 多バイトの文字を含む文字列を束縛し、表示できる。
#[test]
fn multibyte_text_round_trips() {
    let mut console = Console::new(&RunConfig::default());
    assert_eq!(console.eval("let s = \"日本語\"").0, Outcome::Accepted);
    assert_eq!(console.eval("s").1, "\"日本語\"\n");
}
