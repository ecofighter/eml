//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置き、1つの
//! ファイルか、`main.em` をエントリとする1つのディレクトリである (docs/implementation/testing.md の「UI テスト」)。

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eml_cli::{
    CaptureHost, Control, Finished, FsProvider, Limits, RunConfig, RunStats, RuntimeError, Sched,
    Session, Signal, VirtualSignal, VmFault, default_limits, execute,
};
use eml_diagnostics::{has_errors, render};
use eml_rt::Fault;

use crate::common::{
    Marks, assert_filter_matches, for_each_file, is_entry, leading_marks, marks_of, run_all,
    temp_project, windows_skip, workers_skip,
};

fn ui_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap()
}

/// スナップショットの中のパスを安定させるため、エントリを `tests/ui` からの相対パスで渡す。モジュールの根はエントリの
/// ディレクトリである。
fn load(path: &Path, suffix: &str) -> Session {
    let relative = path
        .strip_prefix(ui_root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let root = path.parent().expect("a test file has a directory");
    let session = Session::load(
        &relative,
        &fs::read_to_string(path).unwrap(),
        &FsProvider::new(root),
    );
    // ディレクトリのテストの接尾辞は `.em` で終わらない (`classify`)
    if suffix.ends_with(".em") {
        assert_single_file(&session, suffix);
    }
    session
}

/// 単独のファイルのテストは、ユーザーのモジュールを import しない。分類のディレクトリにある別のテストのファイルを、
/// モジュールとして読んでしまうため (docs/implementation/testing.md の「UI テスト」)。標準ライブラリのモジュールは
/// いつも読み込むので数えない。
fn assert_single_file(session: &Session, suffix: &str) {
    let modules: Vec<&str> = session.user_module_names().collect();
    assert!(
        modules.len() <= 1,
        "{suffix} imports {}: a test that imports modules must be a directory with main.em",
        modules[1..].join(", ")
    );
}

/// 集めた `.em` の役割。
#[derive(Debug, PartialEq)]
enum Entry {
    /// テストのエントリ。スナップショットの名前の接尾辞を持つ。
    Test(String),
    /// ディレクトリのテストの `main.em` から読むモジュール。単独のテストにしない。
    Module,
}

/// 置き場所の規則を確かめて、`.em` の役割を決める。成功や失敗の確かめより先に呼び、置き場所の誤りを先に伝える。
/// 接尾辞は最上位のディレクトリからの相対パス (`basics/hello.em`。ディレクトリのテストは `names/import_cycle`) にする。
/// 名前がテストの置き場所だけで決まり、ほかのテストを足しても変わらないため。
fn classify(path: &Path, top: &str) -> Entry {
    let top_dir = ui_root().join(top);
    let relative = path.strip_prefix(&top_dir).unwrap();
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parts.as_slice() {
        [category, file] => {
            // `main.em` はディレクトリのテストのエントリの名前なので、分類の直下にあれば置き場所の誤りである
            assert!(
                file != "main.em",
                "tests/ui/{top}/{category}/main.em must be in a test directory: tests/ui/{top}/{category}/<name>/main.em"
            );
            Entry::Test(format!("{category}/{file}"))
        }
        [category, name, file] if file == "main.em" => Entry::Test(format!("{category}/{name}")),
        [category, name, _, ..] => {
            // `main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通らないようにする
            assert!(
                top_dir.join(category).join(name).join("main.em").is_file(),
                "{} belongs to no test: add tests/ui/{top}/{category}/{name}/main.em",
                relative.display()
            );
            Entry::Module
        }
        _ => panic!(
            "{} must be in a category directory under tests/ui/{top}/",
            relative.display()
        ),
    }
}

fn snapshot_settings(path: &Path, suffix: &str) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_input_file(path);
    settings.set_snapshot_suffix(suffix);
    settings
}

/// 診断のエラーなしでコンパイルし、LIR と、入力のファイルを置くディレクトリと、診断の表示を返す。入力のファイルは
/// テストの隣に置く (docs/implementation/testing.md の「UI テスト」)。
fn compile(path: &Path, suffix: &str) -> (eml_lir::Program, PathBuf, String) {
    let session = load(path, suffix);
    let compiled = session.compile();
    let stderr = render(&compiled.diagnostics, session.files());
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let root = path
        .parent()
        .expect("a test file has a directory")
        .to_path_buf();
    (program, root, stderr)
}

/// 実行の結果。`stderr` はプログラムと子のプロセスが標準エラーに書いたものである。`written` は `FileOut` (と、その上に
/// 書いた `Fs.write_text`) が書いたファイルを、スナップショットに足す形に並べたものである。UTF-8 でない中身は `escape_ascii` で書き、
/// 見出しに `(not UTF-8)` を付ける。`result` の `Ok` は `exit` の終了コードである。`finished` は成功した実行の
/// `RunStats` と横取りの数で、seed の run と比べるために持つ。
struct Ran {
    stdout: String,
    stderr: String,
    written: String,
    result: Result<Option<i64>, RuntimeError>,
    finished: Option<Finished>,
}

/// テストの入力のファイル `<名前>.em` の隣にある付属のファイルから、テストの Host を作る。`.args` は1行に1つの引数、
/// `.stdin` はそのままのバイト列、`.env` は `KEY=VALUE` の行で、どれも省ける (docs/implementation/testing.md の
/// 「UI テスト」)。
fn host_for(path: &Path, root: &Path) -> CaptureHost {
    let read = |extension: &str| fs::read(path.with_extension(extension)).ok();
    let lines = |bytes: Vec<u8>| -> Vec<String> {
        String::from_utf8(bytes)
            .expect("a side file is UTF-8")
            .lines()
            .map(str::to_string)
            .collect()
    };
    // 子として `eml` 自身を `PATH` から起こせるようにする
    let bin = Path::new(env!("CARGO_BIN_EXE_eml")).parent().unwrap();
    let mut host = CaptureHost::new(root.to_path_buf()).with_path_prefix(bin.to_path_buf());
    if let Some(args) = read("args") {
        host = host.with_args(lines(args).into_iter().map(OsString::from).collect());
    }
    if let Some(stdin) = read("stdin") {
        host = host.with_stdin(stdin);
    }
    if let Some(env) = read("env") {
        let env = lines(env)
            .into_iter()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("a `.env` line is KEY=VALUE");
                (key.to_string(), OsString::from(value))
            })
            .collect();
        host = host.with_env(env);
    }
    host
}

/// `eml run` と同じ既定の上限で流す。止める理由のビットは `Control::clear` でしか下りないので、実行ごとに新しい
/// `Control` を持つ設定を作る。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる。`marks` はファイルの
/// 先頭の印で、シグナルの予定と深さと確保量の上限をどの run にも同じように当てる (docs/implementation/testing.md の「UI テスト」)。
fn execute_in(
    program: &eml_lir::Program,
    path: &Path,
    root: &Path,
    marks: &Marks,
    debug_heap: bool,
    sched: Sched,
    workers: usize,
) -> Ran {
    let host = host_for(path, root);
    let (out, err, files) = (host.output(), host.errors(), host.written());
    let defaults = default_limits();
    let (max_bytes, soft_bytes) = marks
        .max_bytes
        .unwrap_or((defaults.max_bytes, defaults.soft_bytes));
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(Arc::new(Control::new(Limits {
            workers,
            max_depth: marks.max_depth.unwrap_or(defaults.max_depth),
            max_bytes,
            soft_bytes,
            ..defaults
        })))
        .with_sched(sched)
        .with_signals(marks.signals.clone());
    let outcome = execute(program, &config, host.into_ref());
    let finished = outcome.as_ref().ok().copied();
    let result = outcome.map(|finished| finished.exit);
    let written = files
        .files()
        .into_iter()
        .map(|(path, bytes)| match String::from_utf8(bytes) {
            Ok(text) => format!("--- written: {path} ---\n{text}"),
            Err(e) => format!(
                "--- written: {path} (not UTF-8) ---\n{}\n",
                e.as_bytes().escape_ascii()
            ),
        })
        .collect();
    Ran {
        stdout: out.contents(),
        stderr: err.contents(),
        written,
        result,
        finished,
    }
}

/// 検査モードの結果をスナップショットと比べた後で、同じプログラムを別の設定で流した結果 `other` が、同じ出力、
/// 同じ標準エラー、同じファイル、同じ終わり方になることを確かめる。`label` は別の設定の名前で、食い違ったときの文言に
/// 入る。`compare_stats` なら、スケジュールによらない `RunStats` も比べる。速いモードは仕事の回数を数えないので比べない。
fn assert_agrees(suffix: &str, label: &str, other: &Ran, checked: &Ran, compare_stats: Compare) {
    assert_eq!(
        other.stdout, checked.stdout,
        "{suffix}: {label} printed something else"
    );
    assert_eq!(
        other.stderr, checked.stderr,
        "{suffix}: {label} wrote another standard error"
    );
    assert_eq!(
        other.written, checked.written,
        "{suffix}: {label} wrote other files"
    );
    assert_eq!(
        other.result.as_ref().copied().map_err(ToString::to_string),
        checked
            .result
            .as_ref()
            .copied()
            .map_err(ToString::to_string),
        "{suffix}: {label} ended otherwise"
    );
    if compare_stats != Compare::Nothing {
        let stats = |ran: &Ran| {
            ran.finished
                .map(|finished| compare_stats.kept(finished.stats))
        };
        assert_eq!(
            stats(other),
            stats(checked),
            "{suffix}: {label} did other work"
        );
    }
}

/// 速いモードは前提を確かめないので、VM の誤りが未定義の動作になって同じプロセスのほかのファイルのテストを
/// 壊しうる。そのため検査モードの後に流し、検査モードが VM の誤りを見つけたときは流さない (`fast_mode_reports`。
/// docs/implementation/testing.md の「2つのモードで流す」)。
fn assert_fast_mode_agrees(
    suffix: &str,
    program: &eml_lir::Program,
    path: &Path,
    root: &Path,
    marks: &Marks,
    checked: &Ran,
) {
    let fast = execute_in(program, path, root, marks, false, Sched::Normal, 1);
    assert_agrees(suffix, "the fast mode", &fast, checked, Compare::Nothing);
}

/// seed の方針で流すテストの seed (docs/implementation/testing.md の「seed で流す」)。
const SEEDS: [u64; 2] = [1, 2026];

/// `par/` と `async/` を流す worker の数と方針。スナップショットと比べるのは N = 1 の run で、これらの run はその
/// 結果と比べる。`par/` のファイルは `Async` のタスクを始めないので、救済を強いる方針で流しても確かめることがない
/// (docs/implementation/testing.md の「seed で流す」)。
const WORKER_RUNS: [(&str, &[(usize, Sched)]); 2] = [
    ("par/", &[(2, Sched::Sequential), (8, Sched::Sequential)]),
    (
        "async/",
        &[
            (2, Sched::Sequential),
            (2, Sched::RescueAlways),
            (8, Sched::RescueAlways),
        ],
    ),
];

/// 分類 `suffix` の worker の run。
fn worker_runs(suffix: &str) -> &'static [(usize, Sched)] {
    WORKER_RUNS
        .iter()
        .find(|(category, _)| suffix.starts_with(category))
        .map_or(&[], |(_, runs)| runs)
}

/// 複数の worker で流すのは、`par/` と `async/` のうち印 `-- workers: skip (<理由>)` のないファイルである。印は
/// この2つの分類でしか意味がないので、ほかの分類の印は誤りにする (docs/implementation/testing.md の「seed で流す」)。
fn workers_run(suffix: &str, skip: bool) -> bool {
    let category = suffix.starts_with("par/") || suffix.starts_with("async/");
    assert!(
        category || !skip,
        "{suffix}: the workers mark is only for par/ and async/"
    );
    category && !skip
}

fn workers_label(workers: usize, sched: Sched, signals: &[VirtualSignal]) -> String {
    let marks = if signals.is_empty() {
        ""
    } else {
        " with the signal marks of the file"
    };
    format!(
        "{workers} workers under `Sched::{sched:?}` in checked mode on the virtual driver{marks}"
    )
}

/// 逐次の N = 1 の検査モードの結果を、`WORKER_RUNS` の検査モードの run と比べる。どの方針も横取りは求めない。
/// `par/` で比べる欄は seed の run と同じである。`async/` は、タスクを置く worker がスケジュールで決まるので
/// `RunStats` を比べず、代わりにタスクを始めた worker と違う worker に置いたことを確かめる。置かなければ worker 0 の
/// 上だけで流したことになる。`run/async/` の救済を強いる run では、救済でタスクを移したことも確かめる。移さなければ
/// 救済の道を通っていない (docs/implementation/testing.md の「seed で流す」)。
fn assert_workers_agree(
    suffix: &str,
    program: &eml_lir::Program,
    path: &Path,
    root: &Path,
    marks: &Marks,
    checked: &Ran,
) {
    for &(workers, sched) in worker_runs(suffix) {
        let ran = execute_in(program, path, root, marks, true, sched, workers);
        let compare = if suffix.starts_with("async/") {
            Compare::Nothing
        } else {
            compares_stats(checked, &ran)
        };
        let label = format!(
            "{}; {}",
            workers_label(workers, sched, &marks.signals),
            rerun_hint(path)
        );
        assert_agrees(suffix, &label, &ran, checked, compare);
        // `run-fail/` の run は誤りで終わるので `finished` を持たない
        if suffix.starts_with("async/")
            && let Some(finished) = ran.finished
        {
            assert!(
                finished.workers >= 1 && finished.placed_tasks > 0,
                "{suffix}: placed no task on another worker with {workers} workers; start a task (docs/implementation/testing.md の「seed で流す」)"
            );
            assert!(
                sched != Sched::RescueAlways || finished.rescued > 0,
                "{suffix}: rescued no task with {workers} workers under `Sched::RescueAlways`; start a task after the workers start (docs/implementation/testing.md の「seed で流す」)"
            );
        }
    }
}

/// seed の方針でも流すのは `par/` と `async/` の分類だけである。`par` も `Async` のタスクもないプログラムは
/// 切り替えないので、流しても確かめることがない。
fn seeded(suffix: &str) -> bool {
    suffix.starts_with("par/") || suffix.starts_with("async/")
}

/// シグナルの印は CLI で書けないので、印のあるファイルではそのことを添える。
fn seeded_label(seed: u64, signals: &[VirtualSignal]) -> String {
    let marks = if signals.is_empty() {
        ""
    } else {
        " with the signal marks of the file"
    };
    format!("seed {seed} in checked mode on the virtual driver{marks}")
}

/// 食い違ったときの文言に、`path` の run を流し直すコマンドを書く。seed と worker の方針の run は CLI で選べないので、
/// ファイルを流すテストごと流し直す。テストの名前は `path` の上の階層 (`run/` か `run-fail/`) で決まる。
fn rerun_hint(path: &Path) -> String {
    let test = if path.starts_with(ui_root().join("run-fail")) {
        "run_fail"
    } else {
        "run"
    };
    format!(
        "rerun with `EML_UI_FILTER=<file> cargo nextest run -p eml_cli --test integration -E 'test(=ui::{test})'`"
    )
}

/// 逐次の検査モードの結果を、seed ごとの検査モードの run と比べる。成功した run は、スケジュールによらない
/// `RunStats` も比べ、少なくとも1回横取りしたことを確かめる。横取りがなければ、逐次と同じ順で流しただけになる。
/// `async/` の run が `exit` で終わったときは `RunStats` を比べない。タスクの `exit` はほかのタスクをスケジュールで
/// 決まる位置で捨てるので、終わりの統計がその位置で変わる (docs/implementation/testing.md の「seed で流す」)。
/// 速いモードでは流さない。スケジューラの誤りが速いモードで未定義の動作になると、同じプロセスのほかのテストを壊す。
fn assert_seeds_agree(
    suffix: &str,
    program: &eml_lir::Program,
    path: &Path,
    root: &Path,
    marks: &Marks,
    checked: &Ran,
) {
    let compare_stats = !(suffix.starts_with("async/") && matches!(checked.result, Ok(Some(_))));
    for seed in SEEDS {
        let ran = execute_in(program, path, root, marks, true, Sched::Seeded(seed), 1);
        let compare = if compare_stats {
            compares_stats(checked, &ran)
        } else {
            Compare::Nothing
        };
        let label = format!(
            "{}; {}",
            seeded_label(seed, &marks.signals),
            rerun_hint(path)
        );
        assert_agrees(suffix, &label, &ran, checked, compare);
        if let Some(finished) = ran.finished {
            assert!(
                finished.preemptions > 0,
                "{suffix}: never preempted under seed {seed}; give a par left child, or an Async task while another task is ready, 16 or more safepoints (docs/implementation/testing.md の「seed で流す」)"
            );
        }
    }
}

/// 比べる `RunStats` の欄を選ぶ。取り消しの届いた run は比べない。届いた時点で、それぞれのタスクがした仕事が
/// 変わる。channel を使った run は、スケジュールによらない少しの欄だけを比べる。driver の操作を使った run は、
/// 仕事の回数を比べない (docs/implementation/testing.md の「seed で流す」)。
fn compares_stats(checked: &Ran, other: &Ran) -> Compare {
    let stats = |ran: &Ran| ran.finished.map(|f| f.stats).unwrap_or_default();
    let (a, b) = (stats(checked), stats(other));
    if a.cancels != 0 || b.cancels != 0 {
        return Compare::Nothing;
    }
    let channels = |s: &RunStats| s.channel_ops != 0 || s.channel_waits != 0;
    if channels(&a) || channels(&b) {
        Compare::ChannelIndependent
    } else if a.completions != 0 || b.completions != 0 {
        Compare::DriverIndependent
    } else {
        Compare::ScheduleIndependent
    }
}

/// seed の run と比べる `RunStats` の欄。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compare {
    Nothing,
    ScheduleIndependent,
    ChannelIndependent,
    DriverIndependent,
}

impl Compare {
    fn kept(self, stats: RunStats) -> RunStats {
        match self {
            Compare::Nothing => RunStats::default(),
            Compare::ScheduleIndependent => eml_test_support::schedule_independent(stats),
            Compare::ChannelIndependent => eml_test_support::channel_independent(stats),
            Compare::DriverIndependent => eml_test_support::driver_independent(stats),
        }
    }
}

#[test]
fn stats_are_compared_by_cancellations_and_channels() {
    let ran = |counts: Option<(u64, u64, u64)>| {
        let finished = counts.map(|(cancels, channel_ops, channel_waits)| {
            let mut stats = eml_cli::RunStats::default();
            stats.cancels = cancels;
            stats.channel_ops = channel_ops;
            stats.channel_waits = channel_waits;
            Finished {
                stats,
                exit: None,
                preemptions: 1,
                rescues: 0,
                rescued: 0,
                workers: 0,
                placed: 0,
                placed_tasks: 0,
                placed_jobs: 0,
            }
        });
        Ran {
            stdout: String::new(),
            stderr: String::new(),
            written: String::new(),
            result: Ok(None),
            finished,
        }
    };
    let (all, few, none) = (
        Compare::ScheduleIndependent,
        Compare::ChannelIndependent,
        Compare::Nothing,
    );
    let quiet = Some((0, 0, 0));
    assert_eq!(compares_stats(&ran(quiet), &ran(quiet)), all);
    assert_eq!(compares_stats(&ran(None), &ran(None)), all);
    assert_eq!(compares_stats(&ran(Some((1, 0, 0))), &ran(quiet)), none);
    assert_eq!(compares_stats(&ran(quiet), &ran(Some((1, 3, 0)))), none);
    assert_eq!(compares_stats(&ran(Some((0, 2, 0))), &ran(quiet)), few);
    assert_eq!(compares_stats(&ran(quiet), &ran(Some((0, 0, 1)))), few);
}

/// driver の完了がある run は仕事の回数を比べない。取り消しと channel の規則が先に効く。
#[test]
fn stats_of_driver_runs_are_compared_without_the_work_counts() {
    let ran = |cancels: u64, channel_ops: u64, completions: u64| {
        let mut stats = eml_cli::RunStats::default();
        stats.cancels = cancels;
        stats.channel_ops = channel_ops;
        stats.completions = completions;
        Ran {
            stdout: String::new(),
            stderr: String::new(),
            written: String::new(),
            result: Ok(None),
            finished: Some(Finished {
                stats,
                exit: None,
                preemptions: 1,
                rescues: 0,
                rescued: 0,
                workers: 0,
                placed: 0,
                placed_tasks: 0,
                placed_jobs: 0,
            }),
        }
    };
    let quiet = ran(0, 0, 0);
    assert_eq!(
        compares_stats(&ran(0, 0, 3), &quiet),
        Compare::DriverIndependent
    );
    assert_eq!(
        compares_stats(&quiet, &ran(0, 0, 3)),
        Compare::DriverIndependent
    );
    assert_eq!(
        compares_stats(&ran(0, 2, 3), &quiet),
        Compare::ChannelIndependent
    );
    assert_eq!(compares_stats(&ran(1, 0, 3), &quiet), Compare::Nothing);
    let mut stats = eml_cli::RunStats::default();
    stats.parks = 5;
    stats.completions = 6;
    stats.yields = 2;
    stats.allocations = 70;
    let kept = Compare::DriverIndependent.kept(stats);
    assert_eq!(
        (kept.parks, kept.completions, kept.yields, kept.allocations),
        (5, 6, 0, 0)
    );
}

/// 速いモードも同じ誤りを報告するか。漏れ、内部の誤り、ヒープの誤りは検査モードだけが見つける。これらは VM 自身の
/// 誤りを意味するので、速いモードで流すとメモリを壊すことがある。
fn fast_mode_reports(error: &RuntimeError) -> bool {
    !matches!(
        error,
        RuntimeError::Leak(_)
            | RuntimeError::Fault {
                fault: VmFault::Internal(_) | VmFault::Extern(Fault::Heap(_)),
                ..
            }
    )
}

#[test]
fn run() {
    for_each_file(&ui_root().join("run"), "em", true, |path, marks| {
        let Entry::Test(suffix) = classify(path, "run") else {
            return;
        };
        let (program, root, diagnostics) = compile(path, &suffix);
        let ran = execute_in(&program, path, &root, marks, true, Sched::Sequential, 1);
        assert!(ran.result.is_ok(), "{:?}\n{diagnostics}", ran.result);
        let (stdout, stderr, written) = (&ran.stdout, &ran.stderr, &ran.written);
        let exit = match ran.result {
            Ok(Some(code)) => format!("--- exit: {code} ---\n"),
            _ => String::new(),
        };
        snapshot_settings(path, &suffix).bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- stderr ---\n{diagnostics}{stderr}{written}{exit}"
            ));
        });
        assert_fast_mode_agrees(&suffix, &program, path, &root, marks, &ran);
        if seeded(&suffix) {
            assert_seeds_agree(&suffix, &program, path, &root, marks, &ran);
        }
        if workers_run(&suffix, marks.workers_skip) {
            assert_workers_agree(&suffix, &program, path, &root, marks, &ran);
        }
    });
}

#[test]
fn run_fail() {
    for_each_file(&ui_root().join("run-fail"), "em", true, run_fail_file);
}

/// `run-fail/` の1つのファイルを流す。worker の印の分類は、プログラムを流す前に確かめる。VM の誤りで終わるファイルは
/// 速いモードからあとの run を流さないので、そこで確かめると、ほかの分類の印が黙って通ってしまう。
fn run_fail_file(path: &Path, marks: &Marks) {
    let Entry::Test(suffix) = classify(path, "run-fail") else {
        return;
    };
    let workers = workers_run(&suffix, marks.workers_skip);
    let (program, root, diagnostics) = compile(path, &suffix);
    let ran = execute_in(&program, path, &root, marks, true, Sched::Sequential, 1);
    let Err(error) = &ran.result else {
        panic!("expected a runtime error");
    };
    let (stdout, stderr, written) = (&ran.stdout, &ran.stderr, &ran.written);
    // insta は名前のないスナップショットを囲む関数の名前で呼ぶので、テストの名前 `run_fail` を書く
    snapshot_settings(path, &suffix).bind(|| {
        insta::assert_snapshot!(
            "run_fail",
            format!(
                "--- stdout ---\n{stdout}--- stderr ---\n{diagnostics}{stderr}--- runtime error ---\n{error}\n{written}"
            )
        );
    });
    // VM の誤りの文言は worker ID を含みうる。ID はスケジュールで変わるので、seed の run とも比べない
    if fast_mode_reports(error) {
        assert_fast_mode_agrees(&suffix, &program, path, &root, marks, &ran);
        if seeded(&suffix) {
            assert_seeds_agree(&suffix, &program, path, &root, marks, &ran);
        }
        if workers {
            assert_workers_agree(&suffix, &program, path, &root, marks, &ran);
        }
    }
}

#[test]
fn check_fail() {
    for_each_file(&ui_root().join("check-fail"), "em", false, |path, _| {
        let Entry::Test(suffix) = classify(path, "check-fail") else {
            return;
        };
        let session = load(path, &suffix);
        let diagnostics = session.check().diagnostics;
        let rendered = render(&diagnostics, session.files());
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        snapshot_settings(path, &suffix).bind(|| {
            insta::assert_snapshot!(rendered);
        });
    });
}

#[test]
fn the_layout_decides_what_each_file_is() {
    let root = ui_root();
    assert_eq!(
        classify(&root.join("run/basics/hello.em"), "run"),
        Entry::Test("basics/hello.em".to_string())
    );
    assert_eq!(
        classify(&root.join("run/modules/qualified/main.em"), "run"),
        Entry::Test("modules/qualified".to_string())
    );
    assert_eq!(
        classify(&root.join("run/modules/qualified/Report/Csv.em"), "run"),
        Entry::Module
    );
}

#[test]
fn seeded_runs_cover_par_and_async() {
    assert!(seeded("par/x.em"));
    assert!(seeded("par/dir"));
    assert!(seeded("async/x.em"));
    assert!(seeded("async/dir"));
    assert!(!seeded("panic/x.em"));
    assert!(!seeded("parse/x.em"));
    assert!(!seeded("io/x.em"));
}

#[test]
fn workers_runs_cover_par_and_async() {
    assert!(workers_run("par/x.em", false));
    assert!(workers_run("par/dir", false));
    assert!(workers_run("async/x.em", false));
    assert!(workers_run("async/dir", false));
    assert!(workers_run("async/net_x.em", false));
    assert!(workers_run("async/chan_x.em", false));
    assert!(workers_run("async/counter.em", false));
    assert!(workers_run("async/signal_x.em", false));
    assert!(!workers_run("par/x.em", true));
    assert!(!workers_run("async/x.em", true));
    assert!(!workers_run("panic/x.em", false));
    assert!(!workers_run("parse/x.em", false));
    assert!(!workers_run("tls/x.em", false));
    assert_eq!(
        worker_runs("par/x.em"),
        [(2, Sched::Sequential), (8, Sched::Sequential)]
    );
    assert_eq!(
        worker_runs("async/x.em"),
        [
            (2, Sched::Sequential),
            (2, Sched::RescueAlways),
            (8, Sched::RescueAlways),
        ]
    );
    assert!(worker_runs("io/x.em").is_empty());
}

#[test]
#[should_panic(expected = "the workers mark is only for par/ and async/")]
fn a_workers_mark_outside_par_and_async_fails() {
    workers_run("io/x.em", true);
}

/// `run-fail/` のほかの分類の印は、プログラムを流す前に落ちる。そのため、速いモードが報告しない誤り (漏れや内部の
/// 誤り) で終わるファイルでも通らない。ファイルを読む前に落ちるので、無いファイルの名前で確かめる。
#[test]
#[should_panic(expected = "io/no_such_file.em: the workers mark is only for par/ and async/")]
fn a_workers_mark_in_another_run_fail_category_fails_before_running() {
    let marks = Marks {
        workers_skip: true,
        ..Marks::default()
    };
    run_fail_file(&ui_root().join("run-fail/io/no_such_file.em"), &marks);
}

#[test]
#[should_panic(expected = "workers marks are not supported here")]
fn a_workers_mark_in_a_transcript_fails() {
    marks_of("x.repl", "# workers: skip (x)\n> 1\n", "#", false);
}

#[test]
fn the_workers_mark_needs_a_reason() {
    assert_eq!(
        workers_skip("-- workers: skip (同じ期限のタイマーの順に頼る)", "--"),
        Ok(true)
    );
    assert_eq!(workers_skip("-- workers: skip (x)", "--"), Ok(true));
    assert_eq!(workers_skip("-- 説明", "--"), Ok(false));
    assert_eq!(workers_skip("-- windows: skip (x)", "--"), Ok(false));
    for wrong in [
        "-- workers: skip",
        "-- workers: skip ()",
        "-- workers: skip ( )",
        "-- workers:skip (x)",
    ] {
        assert_eq!(
            workers_skip(wrong, "--"),
            Err(format!(
                "the workers mark is `-- workers: skip (<reason>)`, not `{wrong}`"
            )),
            "{wrong}"
        );
        assert!(leading_marks(wrong, "--").is_err(), "{wrong}");
    }
    let marks = leading_marks(
        "#!/usr/bin/env eml\n-- workers: skip (x)\n-- signal: interrupt at 5\n-- windows: skip (y)\nmain () = ()\n",
        "--",
    );
    assert_eq!(
        marks,
        Ok(Marks {
            windows_skip: true,
            workers_skip: true,
            signals: vec![VirtualSignal {
                at_ms: 5,
                signal: Signal::Interrupt
            }],
            max_depth: None,
            max_bytes: None,
        })
    );
}

/// `Async.run` だけのプログラムはタスクを置かず、ハーネスが落とす。
#[test]
#[should_panic(expected = "placed no task on another worker")]
fn an_async_file_that_places_no_task_fails() {
    let source = "import Async\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> 1)))\n";
    let session = Session::load("run/async/short.em", source, &eml_cli::MemorySource(&[]));
    let program = session.compile().program.expect("the program compiles");
    let root = ui_root().join("run/async");
    let path = root.join("short.em");
    let checked = execute_in(
        &program,
        &path,
        &root,
        &Marks::default(),
        true,
        Sched::Sequential,
        1,
    );
    assert_workers_agree(
        "async/short.em",
        &program,
        &path,
        &root,
        &Marks::default(),
        &checked,
    );
}

#[test]
#[should_panic(expected = "seed 7 in checked mode on the virtual driver")]
fn a_seeded_mismatch_names_the_seed() {
    let ran = |stdout: &str| Ran {
        stdout: stdout.to_string(),
        stderr: String::new(),
        written: String::new(),
        result: Ok(None),
        finished: None,
    };
    assert_agrees(
        "par/x.em",
        &seeded_label(7, &[]),
        &ran("1\n"),
        &ran("2\n"),
        Compare::ScheduleIndependent,
    );
}

/// 左の子が短い `par` は seed で横取りされず、ハーネスがそのファイルを落とす。
#[test]
#[should_panic(expected = "never preempted under seed ")]
fn a_par_file_that_is_never_preempted_fails() {
    let source = "import Std.Par\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)\n  println (show (a + b))\n";
    let session = Session::load("run/par/short.em", source, &eml_cli::MemorySource(&[]));
    let program = session.compile().program.expect("the program compiles");
    let root = ui_root().join("run/par");
    let path = root.join("short.em");
    let checked = execute_in(
        &program,
        &path,
        &root,
        &Marks::default(),
        true,
        Sched::Sequential,
        1,
    );
    assert_seeds_agree(
        "par/short.em",
        &program,
        &path,
        &root,
        &Marks::default(),
        &checked,
    );
}

#[test]
#[should_panic(expected = "must be in a category directory")]
fn a_file_right_under_the_top_directory_fails() {
    classify(&ui_root().join("run/hello.em"), "run");
}

#[test]
#[should_panic(expected = "must be in a test directory")]
fn main_right_under_a_category_fails() {
    classify(&ui_root().join("run/modules/main.em"), "run");
}

#[test]
#[should_panic(expected = "belongs to no test")]
fn a_module_without_main_fails() {
    classify(&ui_root().join("run/modules/no_such_test/Lib.em"), "run");
}

#[test]
fn a_single_file_test_may_use_the_standard_library() {
    let session = Session::load("run/basics/hello.em", "", &eml_cli::MemorySource(&[]));
    assert!(session.module_names().count() > session.user_module_names().count());
    assert_single_file(&session, "basics/hello.em");
}

#[test]
#[should_panic(expected = "basics/imports.em imports Util")]
fn a_single_file_test_that_imports_a_module_fails() {
    let source = eml_cli::MemorySource(&[("Util.em", "")]);
    let session = Session::load("run/basics/imports.em", "import Util\n", &source);
    assert_single_file(&session, "basics/imports.em");
}

#[test]
fn every_failing_file_is_reported() {
    let dir = temp_project(
        "for-each-file",
        &[("pass.em", ""), ("fail_a.em", ""), ("fail_b.em", "")],
    );
    let files = [
        dir.join("fail_a.em"),
        dir.join("fail_b.em"),
        dir.join("pass.em"),
    ];
    let result = std::panic::catch_unwind(|| {
        run_all(&files, |path| {
            assert!(!path.ends_with("fail_a.em") && !path.ends_with("fail_b.em"));
        });
    });
    fs::remove_dir_all(&dir).unwrap();
    let payload = result.expect_err("two files failed");
    let message = payload.downcast_ref::<String>().unwrap();
    assert!(message.contains("2 of 3 files failed"), "{message}");
    assert!(message.contains("fail_a.em: "), "{message}");
    assert!(message.contains("fail_b.em: "), "{message}");
    assert!(!message.contains("pass.em"), "{message}");
}

#[test]
#[should_panic(expected = "matches no UI test or transcript")]
fn a_filter_that_matches_nothing_fails() {
    assert_filter_matches("no-such-file");
}

#[test]
fn the_windows_mark_is_the_first_line() {
    assert_eq!(
        windows_skip("-- windows: skip (sh を起動する)", "--"),
        Ok(true)
    );
    assert_eq!(windows_skip("# windows: skip (x)", "#"), Ok(true));
    assert_eq!(windows_skip("-- 説明", "--"), Ok(false));
    assert_eq!(windows_skip("# windows: skip (x)", "--"), Ok(false));
    assert!(windows_skip("-- windows: skip", "--").is_err());
    assert!(windows_skip("-- windows: skip ()", "--").is_err());
    assert!(windows_skip("-- windows:skip (x)", "--").is_err());
}

#[test]
fn signal_marks_are_parsed_in_order() {
    let marks = leading_marks(
        "-- signal: terminate at 100\n-- signal: interrupt at 5\nmain () = ()\n-- signal: interrupt at 7\n",
        "--",
    );
    let at = |at_ms, signal| VirtualSignal { at_ms, signal };
    assert_eq!(
        marks,
        Ok(Marks {
            windows_skip: false,
            workers_skip: false,
            signals: vec![at(100, Signal::Terminate), at(5, Signal::Interrupt)],
            max_depth: None,
            max_bytes: None,
        })
    );
}

#[test]
#[should_panic(
    expected = "the signal mark is `-- signal: <interrupt|terminate> at <ms>`, not `-- signal: terminate at +5`"
)]
fn a_malformed_signal_mark_panics() {
    marks_of("run/async/x.em", "-- signal: terminate at +5\n", "--", true);
}

#[test]
fn a_windows_mark_and_signal_marks_can_share_the_leading_lines() {
    let marks = leading_marks(
        "#!/usr/bin/env eml\n-- signal: interrupt at 5\n-- windows: skip (x)\n-- signal: terminate at 9\n",
        "--",
    )
    .unwrap();
    assert!(marks.windows_skip);
    assert_eq!(
        marks.signals,
        [
            VirtualSignal {
                at_ms: 5,
                signal: Signal::Interrupt
            },
            VirtualSignal {
                at_ms: 9,
                signal: Signal::Terminate
            },
        ]
    );
    for wrong in [
        "-- signal: Interrupt at 5",
        "-- signal: interrupt  at 5",
        "-- signal: interrupt at",
        "-- signal:interrupt at 5",
    ] {
        assert!(leading_marks(wrong, "--").is_err(), "{wrong}");
    }
}

#[test]
#[should_panic(expected = "x.repl: signal marks are not supported here")]
fn a_signal_mark_in_a_transcript_panics() {
    marks_of("x.repl", "# signal: interrupt at 5\n> 1\n", "#", false);
}

/// ディレクトリのテストのモジュールのファイルはエントリでないので、シグナルの印を書くと panic する。
#[test]
#[should_panic(expected = "run/async/dir/helper.em: signal marks are not supported here")]
fn a_signal_mark_in_a_module_file_panics() {
    let root = ui_root().join("run");
    assert!(is_entry(&root, &root.join("async/x.em"), "em"));
    assert!(is_entry(&root, &root.join("async/dir/main.em"), "em"));
    let module = root.join("async/dir/helper.em");
    marks_of(
        "run/async/dir/helper.em",
        "-- signal: interrupt at 5\n",
        "--",
        is_entry(&root, &module, "em"),
    );
}

#[test]
fn the_max_depth_mark_sets_the_limit() {
    let marks = leading_marks("-- max-depth: 4096\nmain () = ()\n", "--").unwrap();
    assert_eq!(marks.max_depth, Some(4096));
    assert_eq!(
        leading_marks("main () = ()\n", "--").unwrap().max_depth,
        None
    );
    let mixed = leading_marks(
        "#!/usr/bin/env eml\n-- signal: interrupt at 5\n-- max-depth: 8192\n-- windows: skip (x)\n",
        "--",
    )
    .unwrap();
    assert_eq!((mixed.max_depth, mixed.windows_skip), (Some(8192), true));

    let program = {
        let source = "main : Unit -> <IO, Panic> Unit\nmain () = println (show (deep 100000))\n\ndeep : Int -> <Panic> Int\ndeep n = if n == 0 then 0 else 1 + deep (n - 1)\n";
        let session = Session::load("run/basics/deep.em", source, &eml_cli::MemorySource(&[]));
        session.compile().program.expect("the program compiles")
    };
    let root = ui_root().join("run/basics");
    let path = root.join("deep.em");
    let limited = Marks {
        max_depth: Some(1024),
        ..Marks::default()
    };
    let ran = execute_in(&program, &path, &root, &limited, true, Sched::Sequential, 1);
    assert!(ran.result.is_err(), "the mark did not lower the limit");
    let free = execute_in(
        &program,
        &path,
        &root,
        &Marks::default(),
        true,
        Sched::Sequential,
        1,
    );
    assert!(free.result.is_ok(), "{:?}", free.result);
}

#[test]
fn a_malformed_max_depth_mark_panics() {
    for wrong in [
        "-- max-depth: 0",
        "-- max-depth: +5",
        "-- max-depth: -5",
        "-- max-depth: 5 ",
        "-- max-depth:5",
        "-- max-depth: ",
        "-- max-depth: 4k",
    ] {
        let message = leading_marks(wrong, "--").expect_err(wrong);
        assert_eq!(
            message,
            format!("the max-depth mark is `-- max-depth: <words>`, not `{wrong}`")
        );
    }
}

#[test]
#[should_panic(expected = "x.repl: max-depth marks are not supported here")]
fn a_max_depth_mark_in_a_transcript_panics() {
    marks_of("x.repl", "# max-depth: 4096\n> 1\n", "#", false);
}

#[test]
fn a_repeated_max_depth_or_max_bytes_mark_panics() {
    assert_eq!(
        leading_marks("-- max-depth: 4096\n-- max-depth: 8192\n", "--").unwrap_err(),
        "the max-depth mark is written twice, at `-- max-depth: 8192`"
    );
    let twice = "-- max-bytes: 8 soft 4\n-- signal: interrupt at 5\n-- max-bytes: 16 soft 4\n";
    assert_eq!(
        leading_marks(twice, "--").unwrap_err(),
        "the max-bytes mark is written twice, at `-- max-bytes: 16 soft 4`"
    );
}

#[test]
fn the_max_bytes_mark_sets_both_limits() {
    let marks =
        leading_marks("-- max-bytes: 268435456 soft 4194304\nmain () = ()\n", "--").unwrap();
    assert_eq!(marks.max_bytes, Some((268435456, 4194304)));
    assert_eq!(
        leading_marks("main () = ()\n", "--").unwrap().max_bytes,
        None
    );
    let mixed = leading_marks(
        "#!/usr/bin/env eml\n-- max-depth: 4096\n-- max-bytes: 100 soft 50\n-- windows: skip (x)\n",
        "--",
    )
    .unwrap();
    assert_eq!(
        (mixed.max_depth, mixed.max_bytes, mixed.windows_skip),
        (Some(4096), Some((100, 50)), true)
    );

    // 1 MiB の柔らかい上限と 64 MiB の上限の下で、標準の上限なら通るプログラムを流す
    let program = {
        let source = "main : Unit -> <IO, Panic> Unit\nmain () = println (show (length (build 3000000 [])))\n\nbuild : Int -> List Int -> <Panic> List Int\nbuild n acc = if n == 0 then acc else build (n - 1) (n :: acc)\n\nlength : List Int -> <Panic> Int\nlength xs = count xs 0\n\ncount : List Int -> Int -> <Panic> Int\ncount xs n = match xs with\n  | [] -> n\n  | _ :: rest -> count rest (n + 1)\n";
        let session = Session::load("run/basics/big.em", source, &eml_cli::MemorySource(&[]));
        session.compile().program.expect("the program compiles")
    };
    let root = ui_root().join("run/basics");
    let path = root.join("big.em");
    let limited = Marks {
        max_bytes: Some((8 << 20, 1 << 20)),
        ..Marks::default()
    };
    let ran = execute_in(
        &program,
        &path,
        &root,
        &limited,
        false,
        Sched::Sequential,
        1,
    );
    assert!(ran.result.is_err(), "the mark did not lower the hard limit");
    let free = execute_in(
        &program,
        &path,
        &root,
        &Marks::default(),
        false,
        Sched::Sequential,
        1,
    );
    assert!(free.result.is_ok(), "{:?}", free.result);
}

#[test]
fn a_malformed_max_bytes_mark_panics() {
    for wrong in [
        "-- max-bytes: 100",
        "-- max-bytes: 100 soft",
        "-- max-bytes: 100 soft ",
        "-- max-bytes: 100 soft 100",
        "-- max-bytes: 100 soft 200",
        "-- max-bytes: 100 soft 0",
        "-- max-bytes: 0 soft 0",
        "-- max-bytes: 1k soft 5",
        "-- max-bytes: 100 soft +5",
        "-- max-bytes: 100  soft 5",
        "-- max-bytes:100 soft 5",
    ] {
        let message = leading_marks(wrong, "--").expect_err(wrong);
        assert_eq!(
            message,
            format!("the max-bytes mark is `-- max-bytes: <bytes> soft <bytes>`, not `{wrong}`")
        );
    }
}

#[test]
#[should_panic(expected = "x.repl: max-bytes marks are not supported here")]
fn a_max_bytes_mark_in_a_transcript_panics() {
    marks_of("x.repl", "# max-bytes: 100 soft 50\n> 1\n", "#", false);
}
