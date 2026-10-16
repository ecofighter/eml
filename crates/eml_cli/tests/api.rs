use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use eml_cli::{
    Capture, Control, FsProvider, HostRef, Limits, ModulePath, ModuleSource, Outcome, ReadError,
    Repl, RunConfig, RuntimeError, Session, Stop, VmFault, capture_host, execute,
};
use eml_diagnostics::{Diagnostic, has_errors};
use eml_rt::{Fault, Host, Opened, Stream};
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

/// REPL の確保量の上限は `eml run` の半分 (物理メモリの 40%) である。回復が古いヒープと写しを同時に持っても、
/// `eml run` と同じ 80% に収めるため (docs/spec/repl.md の「コマンドライン」)。
#[test]
fn repl_limits_leave_room_for_recovery() {
    let run = eml_cli::default_limits();
    let repl = eml_cli::repl_limits();
    assert_eq!(repl.max_depth, run.max_depth);
    assert_eq!(repl.max_evv, usize::MAX);
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(repl.max_bytes > 0);
        assert_eq!(repl.max_bytes * 2, run.max_bytes);
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
/// ビットが残っていれば止まる。回復が古いヒープのページを `Control` に戻すので、その後に新しいページの要る入力も
/// 上限で止まらない (docs/spec/object-model.md の「実行の制御」)。
#[test]
fn the_session_continues_after_the_memory_limit() {
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
        assert_eq!(
            console.eval("let kept = build 50000 []").0,
            Outcome::Accepted,
            "debug_heap = {debug_heap}"
        );
        assert_eq!(console.repl.close().leaks, None);
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

/// 出力を集め、腕を立てた後の最初の行で割り込みを立てる Host。入力を流している途中に Ctrl-C が来たときを表す。
struct Interrupting {
    control: Arc<Control>,
    armed: AtomicBool,
    out: Mutex<String>,
}

impl Host for Interrupting {
    fn write_line(&self, _: Stream, line: &str) -> io::Result<()> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.control.interrupt();
        }
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        out.push_str(line);
        out.push('\n');
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn open(&self, _: &str) -> io::Result<Opened> {
        Err(io::ErrorKind::NotFound.into())
    }
}

/// 出力し続ける入力を途中で割り込むと、その入力は割り込まれた `Failed` になる。回復が根の値を写し、スタックに
/// 残った物体を捨てるので、前の束縛は読め、検査モードの `close` はリークを報告しない
/// (docs/spec/vm.md の「追記」)。
#[test]
fn an_interrupted_input_fails_and_the_session_continues() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(Arc::clone(&control));
        let host = Arc::new(Interrupting {
            control,
            armed: AtomicBool::new(false),
            out: Mutex::new(String::new()),
        });
        let mut repl = Repl::new(
            &config,
            Box::new(MemorySource(&[])),
            "",
            HostRef::new(host.clone()),
        );
        for input in [
            "let kept = [1, 2, 3]",
            "forever : List Int -> <IO> Unit\nforever xs =\n  println \"line\"\n  forever (1 :: xs)",
        ] {
            assert_eq!(repl.eval(input).outcome, Outcome::Accepted, "{input}");
        }
        host.armed.store(true, Ordering::SeqCst);
        let outcome = repl.eval("forever []").outcome;
        let Outcome::Failed(error) = outcome else {
            panic!("expected an interrupt, got {outcome:?} (debug_heap = {debug_heap})");
        };
        assert!(error.interrupted(), "{error}");
        host.out.lock().unwrap().clear();
        assert_eq!(repl.eval("kept").outcome, Outcome::Accepted);
        assert_eq!(*host.out.lock().unwrap(), "[1, 2, 3]\n");
        assert_eq!(repl.close().leaks, None, "debug_heap = {debug_heap}");
    }
}

/// 手放されると数を足す読み手。`File` が閉じたことを数で確かめる。
struct CountedReader {
    inner: io::Cursor<Vec<u8>>,
    drops: Arc<AtomicUsize>,
}

impl io::Read for CountedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl io::BufRead for CountedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        self.inner.consume(n)
    }
}

impl Drop for CountedReader {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// `open` で割り込みを立て、手放された回数を数える読み手を返す Host。ファイルを開いた直後に Ctrl-C が来たときを表す。
struct InterruptingOpen {
    control: Arc<Control>,
    drops: Arc<AtomicUsize>,
}

impl Host for InterruptingOpen {
    fn write_line(&self, _: Stream, _: &str) -> io::Result<()> {
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    fn open(&self, _: &str) -> io::Result<Opened> {
        self.control.interrupt();
        Ok(Opened {
            reader: Box::new(CountedReader {
                inner: io::Cursor::new(b"abc".to_vec()),
                drops: Arc::clone(&self.drops),
            }),
            size: Some(3),
        })
    }
}

/// 割り込みで止まった入力が開いた `File` は、`eval` から戻った時点で閉じていて、検査モードの `close` もリークを
/// 報告しない。`Fs.read_all` の中で止まったときは `read_all` が `File` を手放す (`Extern` の誤り)。`File` を渡す
/// 呼び出しの安全点で止まったときは `File` が失敗した実行のスタックに残り、回復が古いヒープを捨てるときに破棄処理が
/// 閉じる (docs/spec/object-model.md の「セッションの根」)。
#[test]
fn a_file_left_open_by_an_interrupt_is_closed_by_recovery() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(Arc::clone(&control));
        let drops = Arc::new(AtomicUsize::new(0));
        let host = InterruptingOpen {
            control,
            drops: Arc::clone(&drops),
        };
        let mut repl = Repl::new(
            &config,
            Box::new(MemorySource(&[])),
            "",
            HostRef::new(Arc::new(host)),
        );
        for input in [
            "keep : Fs.File -> Fs.File\nkeep f = f",
            "read : Unit -> <IO> String\nread () =\n  let f = Fs.open \"a.txt\"\n  \
             let (f, text) = Fs.read_all f\n  Fs.close f\n  text",
            "read_kept : Unit -> <IO> String\nread_kept () =\n  let f = keep (Fs.open \"a.txt\")\n  \
             let (f, text) = Fs.read_all f\n  Fs.close f\n  text",
        ] {
            let evaluated = repl.eval(input);
            assert_eq!(
                evaluated.outcome,
                Outcome::Accepted,
                "{input}: {:?}",
                codes(&evaluated.diagnostics)
            );
        }
        let mut interrupted = |input: &str| {
            let outcome = repl.eval(input).outcome;
            let Outcome::Failed(error) = outcome else {
                panic!("expected an interrupt, got {outcome:?} (debug_heap = {debug_heap})");
            };
            assert!(error.interrupted(), "{error}");
            error
        };
        let error = interrupted("read ()");
        assert!(
            matches!(
                error,
                RuntimeError::Fault {
                    fault: VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1, "debug_heap = {debug_heap}");
        let error = interrupted("read_kept ()");
        assert!(
            matches!(
                error,
                RuntimeError::Fault {
                    fault: VmFault::Stopped(Stop::Interrupted),
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 2, "debug_heap = {debug_heap}");
        assert_eq!(repl.close().leaks, None, "debug_heap = {debug_heap}");
    }
}

/// `read` で割り込みを立てる読み込み先。前端の検査の間に Ctrl-C が来たときを表す。
struct InterruptingSource {
    control: Arc<Control>,
}

impl ModuleSource for InterruptingSource {
    fn read(&self, path: &ModulePath) -> Result<String, ReadError> {
        self.control.interrupt();
        if path.0 == ["Util"] {
            Ok("pub two : Int -> Int\ntwo x = x + 2\n".to_string())
        } else {
            Err(ReadError::NotFound)
        }
    }
}

/// 実行を始める前に立った割り込みは、入力の種類によらず入力を取り消す。定義だけの入力も受け付けないので、後の入力から
/// その関数は見えない (docs/spec/repl.md の「割り込み」)。止める語を読むのは `load` の前の1か所で、式の入力も同じ所を
/// 通る。式の入力は import を持てず、前端の検査の間に割り込みを立てる決まった道がないので、定義の入力で確かめる。
#[test]
fn an_interrupt_before_the_run_cancels_the_input() {
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    let mut console = Console::with_source(&config, Box::new(InterruptingSource { control }));
    assert_eq!(
        console.eval("import Util\none : Int -> Int\none x = Util.two x - 1"),
        (Outcome::Interrupted, String::new(), vec![])
    );
    assert_eq!(console.eval("one 1").2, ["E1001"]);
}

/// 下の回復のテストが使う定義。`build` も `total` も末尾呼び出しなので、長いリストでも VM のスタックが
/// 伸びない。
const BUILD: &str = "build : Int -> List Int -> List Int\n\
                     build n acc = if n == 0 then acc else build (n - 1) (n :: acc)";
const TOTAL: &str = "total : List Int -> Int -> Int\n\
                     total xs acc = match xs with\n  | [] -> acc\n  | x :: rest -> total rest (acc + x)";
/// 引数を左から評価するので、`1 / 0` で止まるときに作ったリストがスタックに残る。回復しなければ検査モードの `close`
/// がそれをリークとして報告する。
const HOLDING: &str = "total (build 100 []) (1 / 0)";

/// 2つのモードの `Console` に `inputs` を流し、どれも受け付けられることを確かめる。
fn consoles_with(inputs: &[&str]) -> Vec<(Console, Arc<Control>, bool)> {
    [true, false]
        .into_iter()
        .map(|debug_heap| {
            let control = Arc::new(Control::new(Limits::default()));
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_control(Arc::clone(&control));
            let mut console = Console::new(&config);
            for input in inputs {
                let (outcome, _, codes) = console.eval(input);
                assert_eq!(outcome, Outcome::Accepted, "{input}: {codes:?}");
            }
            (console, control, debug_heap)
        })
        .collect()
}

fn assert_failed(console: &mut Console, input: &str) {
    let (outcome, _, _) = console.eval(input);
    assert!(
        matches!(outcome, Outcome::Failed(_)),
        "{input}: {outcome:?}"
    );
}

/// 失敗する入力を続けても、前の束縛は正しく読め、確保量は増えない。回復が毎回古いヒープのページを `Control` に
/// 戻す (docs/spec/object-model.md の「回復」)。
#[test]
fn repeated_failures_keep_the_bindings_and_the_memory() {
    for (mut console, control, debug_heap) in
        consoles_with(&[BUILD, TOTAL, "let kept = build 1000 []"])
    {
        let fail = "total (build 20000 []) (1 / 0)";
        assert_failed(&mut console, fail);
        let after_first = control.bytes();
        for _ in 0..30 {
            assert_failed(&mut console, fail);
        }
        assert!(
            control.bytes() <= after_first,
            "{} > {after_first} (debug_heap = {debug_heap})",
            control.bytes()
        );
        assert_eq!(console.eval("total kept 0").1, "500500\n");
        assert_eq!(
            console.repl.close().leaks,
            None,
            "debug_heap = {debug_heap}"
        );
    }
}

/// 回復は根から届く物体を明示のスタックでたどるので、長いリストの根でも Rust のスタックが溢れない
/// (docs/spec/object-model.md の「回復」)。
#[test]
fn a_long_list_survives_recovery() {
    for (mut console, _, debug_heap) in consoles_with(&[BUILD, TOTAL, "let xs = build 200000 []"]) {
        assert_failed(&mut console, HOLDING);
        assert_eq!(
            console.eval("total xs 0").1,
            "20000100000\n",
            "debug_heap = {debug_heap}"
        );
        assert_eq!(
            console.repl.close().leaks,
            None,
            "debug_heap = {debug_heap}"
        );
    }
}

/// 2つの根が同じ物体を指していても、回復はそれを1つだけ写し、どちらの根からも同じ値が読める
/// (docs/spec/object-model.md の「回復」)。
#[test]
fn objects_shared_between_roots_survive_recovery() {
    for (mut console, _, debug_heap) in
        consoles_with(&[BUILD, TOTAL, "let a = [1, 2, 3]", "let b = (a, a)"])
    {
        assert_failed(&mut console, HOLDING);
        assert_eq!(console.eval("b").1, "([1, 2, 3], [1, 2, 3])\n");
        assert_eq!(console.eval("a").1, "[1, 2, 3]\n");
        assert_eq!(
            console.repl.close().leaks,
            None,
            "debug_heap = {debug_heap}"
        );
    }
}

/// 失敗した `let` の後も、同じ名前を束縛し直して読め、`close` はリークを報告しない
/// (docs/spec/object-model.md の「セッションの根」)。
#[test]
fn a_failed_let_leaves_no_leak() {
    for (mut console, _, debug_heap) in consoles_with(&[BUILD, TOTAL]) {
        assert_failed(&mut console, "let x = total (build 10 []) (1 / 0)");
        assert_failed(&mut console, "let x = 1 / 0");
        assert_eq!(console.eval("let x = build 3 []").0, Outcome::Accepted);
        assert_eq!(console.eval("x").1, "[1, 2, 3]\n");
        assert_eq!(
            console.repl.close().leaks,
            None,
            "debug_heap = {debug_heap}"
        );
    }
}

/// 余りの容量を持つ文字列 (補間が作る) と `BigInt` の根も、回復の後に同じ値が読める
/// (docs/spec/object-model.md の「種類」)。
#[test]
fn strings_and_big_integers_survive_recovery() {
    for (mut console, _, debug_heap) in consoles_with(&[
        BUILD,
        TOTAL,
        "let n = 4611686018427387903 + 1",
        "let s = \"日本語 \\{n} 文字\"",
    ]) {
        assert_failed(&mut console, HOLDING);
        assert_eq!(console.eval("n").1, "4611686018427387904\n");
        assert_eq!(console.eval("s").1, "\"日本語 4611686018427387904 文字\"\n");
        assert_eq!(
            console.repl.close().leaks,
            None,
            "debug_heap = {debug_heap}"
        );
    }
}

/// `:load` のファイルの診断は、ファイルのパスを指す (docs/spec/repl.md の「ファイルの読み込み」)。
#[test]
fn a_file_reports_its_errors_under_its_path() {
    let (host, _) = capture_host(Default::default());
    let (_, evaluated) = Repl::with_file(
        &RunConfig::default(),
        "app/main.em",
        "f : Int\nf = nope\n",
        Box::new(MemorySource(&[])),
        host,
    );
    assert_eq!(evaluated.outcome, Outcome::Rejected);
    let paths: Vec<&str> = evaluated
        .diagnostics
        .iter()
        .map(|d| evaluated.files.path(d.primary.file))
        .collect();
    assert_eq!(paths, ["app/main.em"]);
}

#[test]
fn an_empty_file_leaves_an_empty_session() {
    let (host, _) = capture_host(Default::default());
    let (_, evaluated) = Repl::with_file(
        &RunConfig::default(),
        "empty.em",
        "-- nothing\n",
        Box::new(MemorySource(&[])),
        host,
    );
    assert_eq!(evaluated.outcome, Outcome::Empty);
    assert!(evaluated.diagnostics.is_empty());
}

/// 2つ目以降の入力も、ファイルをファイルとして読み直す。ファイルの関数の実行時エラーはファイルのパスを指し、ファイルの
/// 警告は繰り返さない (spec の C1)。
#[test]
fn later_inputs_keep_the_file_path_and_its_warnings_once() {
    let file = "half : Int -> Int\nhalf n = match n with\n  | x -> 10 / x\n  | 0 -> 0\n";
    let (host, _) = capture_host(Default::default());
    let (mut repl, evaluated) = Repl::with_file(
        &RunConfig::default(),
        "lib/half.em",
        file,
        Box::new(MemorySource(&[])),
        host,
    );
    assert_eq!(evaluated.outcome, Outcome::Accepted);
    assert_eq!(codes(&evaluated.diagnostics), ["E4004"]);
    let evaluated = repl.eval("half 0");
    assert_eq!(codes(&evaluated.diagnostics), Vec::<String>::new());
    let Outcome::Failed(error) = evaluated.outcome else {
        panic!("a runtime error: {:?}", evaluated.outcome);
    };
    assert!(error.to_string().contains("lib/half.em:3:"), "{error}");
}

/// ファイルを受け付けなかったセッションでも、後の入力はファイルを import できない。
#[test]
fn a_rejected_file_still_cannot_be_imported() {
    let (host, _) = capture_host(Default::default());
    let (mut repl, evaluated) = Repl::with_file(
        &RunConfig::default(),
        "Tool.em",
        "f : Int\nf = nope\n",
        Box::new(MemorySource(&[("Tool.em", "pub g : Int\ng = 1\n")])),
        host,
    );
    assert_eq!(evaluated.outcome, Outcome::Rejected);
    assert!(codes(&repl.eval("import Tool").diagnostics).contains(&"E1030".to_string()));
}

/// import を読む間に割り込まれたファイルは受け付けず、セッションは空のままである。
#[test]
fn an_interrupted_file_is_not_accepted() {
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    let (host, _) = capture_host(Default::default());
    let (mut repl, evaluated) = Repl::with_file(
        &config,
        "main.em",
        "import Util\none : Int -> Int\none x = Util.two x - 1\n",
        Box::new(InterruptingSource { control }),
        host,
    );
    assert_eq!(evaluated.outcome, Outcome::Interrupted);
    assert_eq!(codes(&repl.eval("one 1").diagnostics), ["E1001"]);
}

/// `:type` は入力を受け付けず、番号を進めない。後で定義し直した item の古い版の番号は、受け付けた入力の番号のままである。
#[test]
fn type_of_neither_accepts_nor_numbers_its_question() {
    let mut console = Console::new(&RunConfig::default());
    console.eval("data Shape = | Circle");
    console.eval("let s = Circle");
    let typed = console.repl.type_of("s");
    assert_eq!(typed.ty.as_deref(), Some("Shape"));
    assert_eq!(console.repl.type_of("data X = | X").ty, None);
    console.eval("data Shape = | Square");
    assert_eq!(console.repl.type_of("s").ty.as_deref(), Some("Shape#1"));
    // 問い合わせは受け付けないので、名前は残らない
    console.repl.type_of("let t = 1 in t");
    assert_eq!(console.eval("t").2, ["E1001"]);
}

/// 後で報告する誤り (線形性) だけの問い合わせでも、型を返さない。
#[test]
fn type_of_returns_no_type_when_any_error_is_found() {
    let console = Console::new(&RunConfig::default());
    let typed = console.repl.type_of("let f = Fs.open \"x\" in (f, f)");
    assert!(has_errors(&typed.diagnostics), "{:?}", typed.diagnostics);
    assert_eq!(typed.ty, None);
}

/// 入力を待つ間に立った割り込みは、`:type` の始めに下ろす (docs/spec/repl.md の「割り込み」)。
#[test]
fn type_of_clears_a_stale_interrupt() {
    let control = Arc::new(Control::new(Limits::default()));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    let console = Console::new(&config);
    control.interrupt();
    assert_eq!(console.repl.type_of("1 + 1").ty.as_deref(), Some("Int"));
    assert_eq!(control.stop_bits() & Control::INTERRUPT, 0);
}
