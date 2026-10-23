use std::fs;
use std::io::{self, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

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
/// 含まれるものだけを `run_all` で流す。
///
/// insta の `glob!` は失敗の数をプロセスで1つのスタック (insta 1.49 の `GLOB_STACK`) に数える。並んで走ると、
/// ほかのテストの失敗を自分の失敗として報告する。そのため UI テストとトランスクリプトのファイルは、`glob!` を使わずに
/// ここで集めて並べて流し、失敗をまとめて報告する (docs/implementation/testing.md の「UI テスト」)。
pub fn for_each_file(root: &Path, extension: &str, test: impl Fn(&Path) + Sync) {
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
    files.retain(|path| {
        let text = fs::read_to_string(path).unwrap();
        // shebang は1行目にしか書けないので、その後ろの行に印を置く
        let first = text
            .lines()
            .find(|line| !line.starts_with("#!"))
            .unwrap_or("");
        let skip = windows_skip(first, leader)
            .unwrap_or_else(|message| panic!("{}: {message}", shown(path)));
        !(cfg!(windows) && skip)
    });
    run_all(&files, test);
}

/// shebang を除いた1行目が `<leader> windows: skip (<理由>)` なら Windows で流さない (docs/implementation/testing.md の「OS ごとのテスト」)。
/// 綴りを誤った印が黙ってテストを流し続けないよう、`<leader> windows:` で始まる行は正しい形でなければ誤りにする。
pub fn windows_skip(first_line: &str, leader: &str) -> Result<bool, String> {
    let Some(rest) = first_line
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
            "the Windows mark is `{leader} windows: skip (<reason>)`, not `{first_line}`"
        )),
    }
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
