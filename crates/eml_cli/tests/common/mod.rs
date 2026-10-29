use std::fs;
use std::io::{self, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use eml_cli::{Signal, VirtualSignal};

/// 1つのテストが多くのファイルを流すので、この時間を超えて走るファイルの名前を出し、どこで止まったかを分かるようにする
/// (docs/implementation/testing.md の「UI テスト」)。
const SLOW: Duration = Duration::from_secs(10);

fn collect(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            out.push(path);
        }
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn shown(path: &Path) -> String {
    path.strip_prefix(workspace())
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// フィルタの打ち間違いで、どのテストも0個のファイルを流して通ってしまわないようにする。環境変数はプロセスで1つ
/// なので、読むのは呼ぶ側にし、この関数を直接試せるようにする。
pub fn assert_filter_matches(filter: &str) {
    let mut all = Vec::new();
    collect(&workspace().join("tests/ui"), "em", &mut all);
    collect(
        &workspace().join("crates/eml_cli/tests/repl"),
        "repl",
        &mut all,
    );
    assert!(
        all.iter().any(|path| shown(path).contains(filter)),
        "EML_UI_FILTER={filter} matches no UI test or transcript"
    );
}

/// `root` の下の `.<extension>` を名前の順に集め、`EML_UI_FILTER` の部分文字列がワークスペースからの相対パスに
/// 含まれるものだけを `run_all` で流す。`test` にはファイルとその先頭の印のシグナルの予定を渡す。シグナルの印を
/// 許すのは `signals` が真のときだけである (docs/implementation/testing.md の「UI テスト」)。
///
/// insta の `glob!` は失敗の数をプロセスで1つのスタック (insta 1.49 の `GLOB_STACK`) に数える。並んで走ると、
/// ほかのテストの失敗を自分の失敗として報告する。そのため UI テストとトランスクリプトのファイルは、`glob!` を使わずに
/// ここで集めて並べて流し、失敗をまとめて報告する (docs/implementation/testing.md の「UI テスト」)。
pub fn for_each_file(
    root: &Path,
    extension: &str,
    signals: bool,
    test: impl Fn(&Path, &[VirtualSignal]) + Sync,
) {
    let mut files = Vec::new();
    collect(root, extension, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no .{extension} file under {}",
        root.display()
    );
    if let Ok(filter) = std::env::var("EML_UI_FILTER") {
        assert_filter_matches(&filter);
        files.retain(|path| shown(path).contains(&filter));
    }
    let leader = if extension == "repl" { "#" } else { "--" };
    let mut marked: Vec<(PathBuf, Marks)> = files
        .into_iter()
        .map(|path| {
            let marks = marks_of(
                &shown(&path),
                &fs::read_to_string(&path).unwrap(),
                leader,
                signals && is_entry(root, &path, extension),
            );
            (path, marks)
        })
        .collect();
    marked.retain(|(_, marks)| !(cfg!(windows) && marks.windows_skip));
    let files: Vec<PathBuf> = marked.iter().map(|(path, _)| path.clone()).collect();
    run_all(&files, |path| {
        let (_, marks) = marked
            .iter()
            .find(|(p, _)| p == path)
            .expect("a file to run was collected");
        test(path, &marks.signals);
    });
}

/// ファイルの先頭の印の行が書いたもの (docs/implementation/testing.md の「OS ごとのテスト」と「UI テスト」)。
#[derive(Debug, Default, PartialEq)]
pub struct Marks {
    pub windows_skip: bool,
    /// 行の順のシグナルの予定。
    pub signals: Vec<VirtualSignal>,
}

/// テストのエントリか。分類の直下のファイルか、ディレクトリのテストの `main.<extension>` である。ディレクトリの
/// テストのほかのファイルは import するモジュールで、シグナルの印を書けない (docs/implementation/testing.md の「UI テスト」)。
pub fn is_entry(root: &Path, path: &Path, extension: &str) -> bool {
    let depth = path
        .strip_prefix(root)
        .map_or(0, |relative| relative.components().count());
    depth == 2
        || path
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == format!("main.{extension}"))
}

/// `leading_marks` の誤りと、許さない場所のシグナルの印で panic する。`shown` は文言に出すファイルの名前である。
pub fn marks_of(shown: &str, text: &str, leader: &str, signals: bool) -> Marks {
    let marks = leading_marks(text, leader).unwrap_or_else(|message| panic!("{shown}: {message}"));
    if !signals && !marks.signals.is_empty() {
        panic!("{shown}: signal marks are not supported here");
    }
    marks
}

/// 先頭の印の行を読む。印の行は、shebang の行があればその後ろから続く、`<leader> windows:` か `<leader> signal:` で
/// 始まる行の並びで、2種類をどの順に混ぜてもよい。shebang は1行目にしか書けないので、印はその後ろに置く。
pub fn leading_marks(text: &str, leader: &str) -> Result<Marks, String> {
    let mut lines = text.lines().peekable();
    let _shebang = lines.next_if(|line| line.starts_with("#!"));
    let mut marks = Marks::default();
    let starts = |line: &str, kind: &str| {
        line.strip_prefix(leader)
            .is_some_and(|rest| rest.starts_with(kind))
    };
    for line in lines {
        if starts(line, " windows:") {
            marks.windows_skip |= windows_skip(line, leader)?;
        } else if starts(line, " signal:") {
            marks.signals.push(signal_mark(line, leader)?);
        } else {
            break;
        }
    }
    Ok(marks)
}

/// 先頭の印の行の1つが `<leader> windows: skip (<理由>)` なら Windows で流さない (docs/implementation/testing.md の「OS ごとのテスト」)。
/// 綴りを誤った印が黙ってテストを流し続けないよう、`<leader> windows:` で始まる行は正しい形でなければ誤りにする。
pub fn windows_skip(line: &str, leader: &str) -> Result<bool, String> {
    let Some(rest) = line
        .strip_prefix(leader)
        .and_then(|rest| rest.strip_prefix(" windows:"))
    else {
        return Ok(false);
    };
    match rest
        .strip_prefix(" skip (")
        .and_then(|reason| reason.strip_suffix(')'))
    {
        Some(reason) if !reason.trim().is_empty() => Ok(true),
        _ => Err(format!(
            "the Windows mark is `{leader} windows: skip (<reason>)`, not `{line}`"
        )),
    }
}

/// `<leader> signal: <interrupt|terminate> at <ms>` の印を読む。`ms` は10進の数字だけで、空白は1つずつである。
/// 綴りを誤った印が黙って予定を落とさないよう、`<leader> signal:` で始まる行は正しい形でなければ誤りにする。
pub fn signal_mark(line: &str, leader: &str) -> Result<VirtualSignal, String> {
    line.strip_prefix(leader)
        .and_then(|rest| rest.strip_prefix(" signal: "))
        .and_then(|rest| rest.split_once(" at "))
        .and_then(|(kind, ms)| {
            let signal = match kind {
                "interrupt" => Signal::Interrupt,
                "terminate" => Signal::Terminate,
                _ => return None,
            };
            // `u64::from_str` は先頭の `+` も読むので、数字だけの綴りに限る
            ms.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| ms.parse().ok())
                .flatten()
                .map(|at_ms| VirtualSignal { at_ms, signal })
        })
        .ok_or_else(|| {
            format!(
                "the signal mark is `{leader} signal: <interrupt|terminate> at <ms>`, not `{line}`"
            )
        })
}

/// 失敗したファイルをすべて挙げるため、ファイルごとの panic を捕まえて最後まで流し、終わりに1回だけ panic する。
pub fn run_all(files: &[PathBuf], test: impl Fn(&Path) + Sync) {
    // `RUST_TEST_THREADS=1` で、1つずつ流した時間を測ったり失敗を読んだりできるようにする
    let threads = std::env::var("RUST_TEST_THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
        .min(files.len());
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let running: Vec<Mutex<Option<(usize, Instant)>>> =
        (0..threads).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        let (done, wait) = mpsc::channel::<()>();
        let running = &running;
        scope.spawn(move || {
            let mut reported = vec![usize::MAX; threads];
            while let Err(RecvTimeoutError::Timeout) = wait.recv_timeout(Duration::from_millis(200))
            {
                for (worker, slot) in running.iter().enumerate() {
                    let current = *slot.lock().unwrap();
                    if let Some((index, start)) = current
                        && start.elapsed() >= SLOW
                        && reported[worker] != index
                    {
                        reported[worker] = index;
                        // libtest は止まったテストの出力を出さないので、標準エラーに直接書く。nextest の下では、
                        // テストを止めたときの要約か `--no-capture` で見える
                        let _ = writeln!(
                            io::stderr(),
                            "still running after {}s: {}",
                            SLOW.as_secs(),
                            shown(&files[index])
                        );
                    }
                }
            }
        });
        let workers: Vec<_> = running
            .iter()
            .map(|slot| {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = files.get(index) else { break };
                        *slot.lock().unwrap() = Some((index, Instant::now()));
                        let result = catch_unwind(AssertUnwindSafe(|| test(path)));
                        *slot.lock().unwrap() = None;
                        if let Err(payload) = result {
                            let message = payload
                                .downcast_ref::<String>()
                                .map(String::as_str)
                                .or_else(|| payload.downcast_ref::<&str>().copied())
                                .unwrap_or("(no message)")
                                .to_string();
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("{}: {message}", shown(path)));
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        drop(done);
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {} files failed:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

/// 一時ディレクトリに `(根からの相対パス, 本文)` のファイルを置く。結合テストは1つのプロセスの中で並んで走るので、
/// `name` でディレクトリを分ける。
pub fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eml-cli-{name}-{}", std::process::id()));
    // 前の実行が途中で止まって残したファイルを読まないため
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}
