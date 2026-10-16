use std::path::Path;
use std::process::{Command, Output};

use crate::common::temp_project;

fn eml(args: &[&str]) -> Output {
    eml_in(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ui")),
        args,
    )
}

fn eml_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn check_succeeds_on_a_valid_file() {
    let output = eml(&["check", "run/basics/comments_only.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}

#[test]
fn check_fails_with_diagnostics() {
    let output = eml(&["check", "check-fail/syntax/multiple_errors.em"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("[E0001]"));
}

#[test]
fn run_succeeds_on_a_valid_file() {
    let output = eml(&["run", "--debug-heap", "run/basics/comments_only.em"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn run_does_not_execute_a_file_with_errors() {
    let output = eml(&["run", "check-fail/syntax/multiple_errors.em"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn unreadable_file_is_a_usage_error() {
    let output = eml(&["check", "does/not/exist.em"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read `does/not/exist.em`"));
}

#[test]
fn missing_arguments_are_a_usage_error() {
    assert_eq!(eml(&[]).status.code(), Some(2));
    assert_eq!(eml(&["check"]).status.code(), Some(2));
    assert_eq!(eml(&["frobnicate"]).status.code(), Some(2));
}

#[test]
fn non_utf8_file_is_a_usage_error() {
    let path = std::env::temp_dir().join(format!("eml-cli-test-{}.em", std::process::id()));
    std::fs::write(&path, [0x66, 0x6e, 0xff, 0xfe]).unwrap();
    let output = eml(&["check", path.to_str().unwrap()]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn run_fails_without_main_but_check_succeeds() {
    let path = std::env::temp_dir().join(format!("eml-cli-no-main-{}.em", std::process::id()));
    std::fs::write(&path, "f : Int -> Int\nf x = x\n").unwrap();
    let path_text = path.to_str().unwrap();
    assert_eq!(eml(&["check", path_text]).status.code(), Some(0));
    let output = eml(&["run", path_text]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("E2003"));
}

#[test]
fn runtime_errors_exit_with_one() {
    let output = eml(&["run", "--debug-heap", "run-fail/basics/division_by_zero.em"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "runtime error: division by zero\n  at run-fail/basics/division_by_zero.em:3:16"
        ),
        "{stderr}"
    );
}

#[test]
fn run_prints_the_program_output() {
    let output = eml(&["run", "--debug-heap", "run/basics/hello.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
}

#[test]
fn run_opens_an_absolute_path_as_is() {
    // UI テストは基準ディレクトリからの相対パスだけを使うので、絶対パスはここで確かめる (docs/spec/effects.md)
    let dir = std::env::temp_dir().join(format!("eml-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    std::fs::write(&input, "from an absolute path").unwrap();
    let program = dir.join("absolute.em");
    let source = format!(
        "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"{}\"\n  let (f, text) = Fs.read_all f\n  Fs.close f\n  println text\n",
        input.display()
    );
    std::fs::write(&program, source).unwrap();
    let output = eml(&["run", "--debug-heap", program.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "from an absolute path\n"
    );
}

#[test]
fn a_missing_module_is_a_diagnostic_error() {
    let dir = temp_project("cli-missing", &[("main.em", "import Missing\n")]);
    let output = eml_in(&dir, &["check", "main.em"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("[E1026]"));
}

#[test]
fn modules_are_read_from_the_entry_directory() {
    let dir = temp_project(
        "cli-root",
        &[
            (
                "app/main.em",
                "import Report.Csv\n\nmain : Unit -> <IO> Unit\nmain () = println \"ok\"\n",
            ),
            ("app/Report/Csv.em", "pub x : Int\nx = 1\n"),
        ],
    );
    let nested = eml_in(&dir, &["run", "--debug-heap", "app/main.em"]);
    // ディレクトリのない入口のパスでは、根が作業ディレクトリになる
    let bare = eml_in(&dir.join("app"), &["check", "main.em"]);
    let dotted = eml_in(&dir, &["check", "./app/main.em"]);
    let absolute_path = dir.join("app/main.em");
    let absolute = eml_in(&dir, &["check", absolute_path.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(dotted.status.code(), Some(0), "{dotted:?}");
    assert_eq!(absolute.status.code(), Some(0), "{absolute:?}");
    assert_eq!(nested.status.code(), Some(0), "{nested:?}");
    assert_eq!(String::from_utf8_lossy(&nested.stdout), "ok\n");
    assert_eq!(bare.status.code(), Some(0), "{bare:?}");
}

#[test]
fn a_miscased_entry_path_is_still_the_entry() {
    // 大文字小文字を区別しないファイルシステムでは `t/server.em` で `t/Server.em` を開ける。そのときも依存先の
    // `import Server` は入口を指す E1030 で、入口を2回読んだ循環 (E1027) にしない (docs/spec/modules.md の「モジュール」)
    let dir = temp_project(
        "cli-entry-case",
        &[
            ("t/Server.em", "import Util\n"),
            ("t/Util.em", "import Server\n"),
        ],
    );
    let output = eml_in(&dir, &["check", "t/server.em"]);
    std::fs::remove_dir_all(&dir).unwrap();
    // 大文字小文字を区別するファイルシステムでは、入口が読めない使い方の誤りになる
    if output.status.code() == Some(2) {
        return;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(stderr.matches("[E1030]").count(), 1, "{stderr}");
    assert!(!stderr.contains("[E1027]"), "{stderr}");
    assert!(stderr.contains("t/Server.em"), "{stderr}");
}

#[test]
fn run_in_fast_mode_prints_the_program_output() {
    let output = eml(&["run", "run/basics/hello.em"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
}

#[test]
fn a_runtime_error_in_fast_mode_exits_with_one() {
    let output = eml(&["run", "run-fail/basics/division_by_zero.em"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("runtime error: division by zero\n  at "),
        "{stderr}"
    );
}

/// 終わらない自己末尾呼び出しのループで、行を出力し続ける。
#[cfg(unix)]
const SPIN: &str = "\
spin : Int -> <IO> Unit
spin n =
  println (show n)
  spin (n + 1)

main : Unit -> <IO> Unit
main () = spin 0
";

/// 名前付きパイプ `fifo` を開いて全部読む。書き手が何も書かずに開いたままなら、読みの中で待ち続ける。
#[cfg(unix)]
const READ_FIFO: &str = "\
main : Unit -> <IO> Unit
main () =
  let f = Fs.open \"fifo\"
  let (f, text) = Fs.read_all f
  Fs.close f
  println text
";

/// テストが止まらずに CI を待たせないための締め切り。
#[cfg(unix)]
pub(crate) const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(unix)]
fn spawn_run(dir: &Path, stdout: std::process::Stdio) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "main.em"])
        .current_dir(dir)
        .stdout(stdout)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

#[cfg(unix)]
pub(crate) fn sigint(child: &std::process::Child) {
    let pid = libc::pid_t::try_from(child.id()).unwrap();
    // 子はまだ待っていないので、終わっていても pid はほかのプロセスに使い回されていない
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
}

/// 締め切りを過ぎたら子を殺して、テストを失敗にする。
#[cfg(unix)]
pub(crate) fn kill_after_deadline(child: &mut std::process::Child, what: &str) -> ! {
    let _ = child.kill();
    let _ = child.wait();
    panic!("{what} within {DEADLINE:?}");
}

#[cfg(unix)]
fn stderr_of(child: &mut std::process::Child) -> String {
    use std::io::Read;
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    stderr
}

/// 1回目の SIGINT で、VM は次の安全点で止まり、終了コード 130 で終わる。標準出力はパイプなので 64 KiB ごとにしか
/// 書かれない。パイプが詰まると子は `write` の中で止まって安全点に来ないので、別のスレッドで読み続ける。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_running_program_with_130() {
    use std::io::Read;
    use std::time::Instant;

    let dir = temp_project("cli-sigint-loop", &[("main.em", SPIN)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::piped());
    let mut stdout = child.stdout.take().unwrap();
    let (first_output, arrived) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        while let Ok(n @ 1..) = stdout.read(&mut buffer) {
            let _ = first_output.send(n);
        }
    });
    if arrived
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_err()
    {
        kill_after_deadline(&mut child, "no output arrived");
    }
    sigint(&child);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not stop");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    reader.join().unwrap();
    let stderr = stderr_of(&mut child);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{stderr}");
    assert!(stderr.contains("interrupted"), "{stderr}");
}

/// 読みの中で待つプログラムは、1回目の SIGINT で読みが `EINTR` で戻って止まり、終了コードは 130 である。読みに入る
/// 直前に届いた SIGINT は読みを止めないが、2回目の SIGINT でハンドラがその場で終わらせ、終了コードは同じく 130 である
/// (docs/spec/vm.md の「上限と割り込み」)。続けて届いた2つの SIGINT は1つにまとまることがあるので、終わるまで
/// 100 ms ごとに送り続ける。
#[cfg(unix)]
#[test]
fn repeated_interrupts_end_a_program_waiting_in_a_read_with_130() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};

    let dir = temp_project("cli-sigint-fifo", &[("main.em", READ_FIFO)]);
    let fifo = dir.join("fifo");
    let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    // 子の `Fs.open` は書き手が開くまで待つ。読み手のいないパイプを書き込み用に非同期で開くと ENXIO なので、開けた
    // 時点で子は実行を始めている。何も書かずに開いたままにして、子を読みの中で待たせる
    let writer = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
        {
            Ok(writer) => break writer,
            Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("the program ended before it opened the pipe: {status}");
                }
                if Instant::now() >= deadline {
                    kill_after_deadline(&mut child, "the program did not open the pipe");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
    };
    let status = loop {
        sigint(&child);
        std::thread::sleep(Duration::from_millis(100));
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not end");
        }
    };
    drop(writer);
    let stderr = stderr_of(&mut child);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{stderr}");
}

/// 引数を1行に書く。
const SHOW_ARGS: &str = "\
main : Unit -> <IO> Unit
main () = println (show (Env.args ()))
";

/// `eml FILE ARGS...` は `eml run FILE ARGS...` と同じで、ファイルより後ろの引数は `-` で始まってもそのまま渡す
/// (docs/spec/stdlib.md の「起動」)。
#[test]
fn a_file_runs_directly_with_its_arguments() {
    let dir = temp_project("cli-args", &[("main.em", SHOW_ARGS)]);
    let direct = eml_in(&dir, &["main.em", "a", "-b", "--c"]);
    let run = eml_in(
        &dir,
        &["run", "--debug-heap", "main.em", "x", "--debug-heap"],
    );
    let check = eml_in(&dir, &["check", "main.em", "--", "x"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(direct.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout),
        "[\"a\", \"-b\", \"--c\"]\n"
    );
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "[\"x\", \"--debug-heap\"]\n"
    );
    // サブコマンドの名前はファイルとして読まない
    assert_eq!(check.status.code(), Some(2));
}

/// ファイルの直後の引数が `-` で始まっても、clap のフラグとして読まずにスクリプトへ渡す (docs/spec/stdlib.md の「起動」)。
#[test]
fn a_hyphen_led_first_argument_reaches_the_script() {
    let dir = temp_project("cli-hyphen", &[("main.em", SHOW_ARGS)]);
    let cases: [(&[&str], &str); 4] = [
        (&["main.em", "--help"], "[\"--help\"]\n"),
        (&["main.em", "--debug-heap"], "[\"--debug-heap\"]\n"),
        (&["main.em", "--", "x"], "[\"--\", \"x\"]\n"),
        (&["run", "--debug-heap", "main.em", "-h"], "[\"-h\"]\n"),
    ];
    let outputs: Vec<_> = cases.iter().map(|(args, _)| eml_in(&dir, args)).collect();
    std::fs::remove_dir_all(&dir).unwrap();
    for ((args, expected), output) in cases.iter().zip(&outputs) {
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            *expected,
            "{args:?}"
        );
    }
}

/// `-` で始まる最初の引数はフラグとして読むので、綴りを誤ったフラグは使い方の誤りである。
#[test]
fn a_misspelled_flag_is_a_usage_error() {
    assert_eq!(eml(&["--verison"]).status.code(), Some(2));
    assert_eq!(eml(&["--version"]).status.code(), Some(0));
}

#[test]
fn a_script_reads_the_standard_input_and_writes_the_standard_error() {
    use std::io::Write;
    let text = "\
main : Unit -> <IO> Unit
main () =
  eprintln \"to stderr\"
  println (show (Stdin.read_line ()))
  println (Stdin.read_all ())
";
    let dir = temp_project("cli-stdin", &[("main.em", text)]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .arg("main.em")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"first\r\nrest\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Some \"first\"\nrest\n\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "to stderr\n");
}

/// `Fs.write_text` は実際のディスクに書き、書けなければ `cannot write` の実行時エラーになる。
#[test]
fn a_script_writes_a_file_and_reports_a_failed_write() {
    let write = "\
main : Unit -> <IO> Unit
main () =
  Fs.write_text \"out.txt\" \"日本\\n\"
  println (Fs.read_text \"out.txt\")
";
    let fail = "\
main : Unit -> <IO> Unit
main () = Fs.write_text \"missing-dir/x.txt\" \"x\"
";
    let dir = temp_project("cli-write", &[("write.em", write), ("fail.em", fail)]);
    let ok = eml_in(&dir, &["write.em"]);
    let bad = eml_in(&dir, &["fail.em"]);
    let written = std::fs::read_to_string(dir.join("out.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ok.stdout), "日本\n\n");
    assert_eq!(written.unwrap(), "日本\n");
    assert_eq!(bad.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(
        stderr.starts_with("runtime error: cannot write `missing-dir/x.txt`: not found"),
        "{stderr}"
    );
}

/// 1行目が `#!/usr/bin/env eml` の実行できるファイルは、そのまま動かせる。
#[cfg(unix)]
#[test]
fn a_script_with_a_shebang_runs_directly() {
    use std::os::unix::fs::PermissionsExt;
    let text = format!("#!/usr/bin/env eml\n{SHOW_ARGS}");
    let dir = temp_project("cli-shebang", &[("script.em", &text)]);
    let script = dir.join("script.em");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin = Path::new(env!("CARGO_BIN_EXE_eml")).parent().unwrap();
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let output = Command::new(&script)
        .args(["1", "-2"])
        .env("PATH", path)
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[\"1\", \"-2\"]\n");
}

/// UTF-8 でない引数と環境変数の値は、読んだときに実行時エラーにする。
#[cfg(unix)]
#[test]
fn arguments_and_variables_that_are_not_utf8_are_runtime_errors() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let get = "\
main : Unit -> <IO> Unit
main () = println (show (Env.get \"BAD\"))
";
    let dir = temp_project("cli-not-utf8", &[("args.em", SHOW_ARGS), ("get.em", get)]);
    let bad = OsStr::from_bytes(b"a\xff");
    let eml = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_eml"));
        command.current_dir(&dir);
        command
    };
    let args = eml()
        .args([OsStr::new("args.em"), OsStr::new("ok"), bad])
        .output()
        .unwrap();
    let get = eml().arg("get.em").env("BAD", bad).output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(args.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&args.stderr)
            .starts_with("runtime error: argument 1 is not valid UTF-8\n"),
        "{args:?}"
    );
    assert_eq!(get.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&get.stderr)
            .starts_with("runtime error: environment variable `BAD` is not valid UTF-8\n"),
        "{get:?}"
    );
}

/// 子のプロセスを待つ間に SIGINT が来ると、子が終わった後で止まり、終了コード 130 で終わる。子は自分のプロセス
/// グループに置き、端末の Ctrl-C と同じくグループ全体に送る。子も SIGINT で終わるので、eml は待ちから戻る。起動と同時に
/// 届いた SIGINT は子に届かないことがあるので、子が標準エラーに書いた行を読んで、子が動いてから送る
/// (docs/spec/stdlib.md の「`Proc`」)。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_program_waiting_for_a_child_with_130() {
    use std::io::{BufRead, BufReader, Read};
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    let text = "\
main : Unit -> <IO> Unit
main () = println (show (Proc.run \"sh\" [\"-c\", \"echo started >&2; exec sleep 30\"]))
";
    let dir = temp_project("cli-sigint-child", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .arg("main.em")
        .current_dir(&dir)
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut started = String::new();
    stderr.read_line(&mut started).unwrap();
    assert_eq!(started, "started\n");
    let group = libc::pid_t::try_from(child.id()).unwrap();
    // 子はまだ待っていないので、グループの番号はほかのグループに使い回されていない
    assert_eq!(unsafe { libc::kill(-group, libc::SIGINT) }, 0);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not stop");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut rest = String::new();
    stderr.read_to_string(&mut rest).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{rest}");
    assert!(rest.starts_with("runtime error: interrupted\n"), "{rest}");
}

/// 標準入力の読みの中で待つプログラムも、`Fs.read_all` と同じく SIGINT で止まり、終了コードは 130 である。読みに入る
/// 直前に届いた SIGINT は読みを止めないので、終わるまで 100 ms ごとに送り続ける (2回目の SIGINT はその場で終わらせる。
/// `repeated_interrupts_end_a_program_waiting_in_a_read_with_130` と同じ)。
#[cfg(unix)]
#[test]
fn repeated_interrupts_end_a_program_waiting_for_the_standard_input_with_130() {
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    let text = "\
main : Unit -> <IO> Unit
main () =
  eprintln \"ready\"
  println (show (Stdin.read_line ()))
";
    let dir = temp_project("cli-sigint-stdin", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .arg("main.em")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // 書き手を開いたままにして、読みを待たせる
    let writer = child.stdin.take();
    // ハンドラを入れる前の SIGINT はプロセスを殺すので、スクリプトが書いた行を読んでから送る
    let mut ready = String::new();
    BufReader::new(child.stderr.as_mut().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let status = loop {
        sigint(&child);
        std::thread::sleep(Duration::from_millis(100));
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not end");
        }
    };
    drop(writer);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130));
}
