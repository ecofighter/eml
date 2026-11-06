use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};

use crate::common::temp_project;

/// UTF-8 でないバイト列の `OsString`。
#[cfg(unix)]
fn not_utf8() -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(vec![b'a', 0xff])
}

/// UTF-8 でないバイト列の `OsString`。孤立した surrogate は UTF-16 として正しくない。
#[cfg(windows)]
fn not_utf8() -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(&[0x61, 0xD800])
}

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
        "import FileIn\nmain : Unit -> <IO> Unit\nmain () =\n  let f = FileIn.open \"{}\"\n  let (text, f) = FileIn.read_all f\n  FileIn.close f\n  println text\n",
        input.display().to_string().replace('\\', "\\\\")
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

/// Windows の std の metadata は名前付きパイプを通常のファイルと見うるが、`Stream.open` はプールの仕事を出す前に
/// 通常のファイルでないと断る (docs/spec/stdlib.md の「`Std.Stream`」、docs/spec/vm.md の「時計と driver」)。
/// パイプのサーバーの端はこのテストが作り、`eml run` が同じ名前を読むために開く。
#[cfg(windows)]
#[test]
fn stream_open_refuses_a_named_pipe() {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_OUTBOUND;
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    let name = format!("\\\\.\\pipe\\eml-cli-test-{}", std::process::id());
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` は 0 で終わる UTF-16 で、OS は戻るまでの間だけ読む。`SECURITY_ATTRIBUTES` は null である
    let raw = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            std::ptr::null(),
        )
    };
    assert_ne!(
        raw,
        INVALID_HANDLE_VALUE,
        "{}",
        std::io::Error::last_os_error()
    );
    // SAFETY: `raw` は今作ったパイプで、ほかに所有者がいない
    let server = unsafe { OwnedHandle::from_raw_handle(raw) };
    let dir = std::env::temp_dir().join(format!("eml-cli-pipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("pipe.em");
    let source = format!(
        "import Async (Async)\nimport Stream\nmain : Unit -> <IO, Panic> Unit\nmain () = Async.run (fn () ->\n  match try_io (fn () -> Stream.close (Stream.open \"{}\")) with\n    | Ok _ -> println \"opened\"\n    | Err e -> println \"\\{{show e.kind}}: \\{{e.message}}\")\n",
        name.replace('\\', "\\\\")
    );
    std::fs::write(&program, source).unwrap();
    let output = eml(&["run", "--debug-heap", program.to_str().unwrap()]);
    drop(server);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("NotAvailable: cannot open `{name}`: not a regular file\n")
    );
}

#[test]
fn exit_ends_with_its_code_modulo_256() {
    let output = eml(&["run", "--debug-heap", "run/io/exit_code.em"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
    let dir = temp_project(
        "cli-exit",
        &[("main.em", "main : Unit -> <IO> Unit\nmain () = exit 259\n")],
    );
    let output = eml_in(&dir, &["main.em"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(3));
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
    // ディレクトリのないエントリのパスでは、根が作業ディレクトリになる
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
    // `import Server` はエントリを指す E1030 で、エントリを2回読んだ循環 (E1027) にしない (docs/spec/modules.md の「モジュール」)
    let dir = temp_project(
        "cli-entry-case",
        &[
            ("t/Server.em", "import Util\n"),
            ("t/Util.em", "import Server\n"),
        ],
    );
    let output = eml_in(&dir, &["check", "t/server.em"]);
    std::fs::remove_dir_all(&dir).unwrap();
    // 大文字小文字を区別するファイルシステムでは、エントリが読めない使い方の誤りになる
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
fn run_with_stats_prints_one_stats_line() {
    let plain = eml(&["run", "run/basics/hello.em"]);
    let output = eml(&["run", "--stats", "run/basics/hello.em"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, plain.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("stats: "))
        .collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    for name in ["parks=", "completions=", "preemptions="] {
        assert!(lines[0].contains(name), "{}", lines[0]);
    }
    assert!(!String::from_utf8_lossy(&plain.stderr).contains("stats: "));
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
spin : Int -> <IO, Panic> Unit
spin n =
  println (show n)
  spin (n + 1)

main : Unit -> <IO, Panic> Unit
main () = spin 0
";

/// 名前付きパイプ `fifo` を開いて全部読む。書き手が何も書かずに開いたままなら、読みの中で待ち続ける。
#[cfg(unix)]
const READ_FIFO: &str = "\
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open \"fifo\"
  let (text, f) = FileIn.read_all f
  FileIn.close f
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

#[cfg(unix)]
fn sigterm(child: &std::process::Child) {
    let pid = libc::pid_t::try_from(child.id()).unwrap();
    // 子はまだ待っていないので、終わっていても pid はほかのプロセスに使い回されていない
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
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

/// 左の子が回り続ける `par`。子は `never` の row なので出力できない。親が子の前に標準エラーへ1行書くので、テストは
/// その行を待ってから SIGINT を送れる。
#[cfg(unix)]
const SPIN_IN_PAR: &str = "\
import Std.Par

spin : Int -> <Panic> Int
spin n = spin (n + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let (x, y) = Par.par (fn () -> 1) (fn () -> 2)
  eprintln (show (x + y))
  let (a, b) = Par.par (fn () -> spin 0) (fn () -> spin 0)
  println (show (a + b))
";

/// 既定の方針 (`normal`) では、監視のスレッドとworkerはrunの始めに動き始める。合図の行は最初の`par`の後に書くので、
/// 回り続ける`par`のrunも1回目のSIGINTで次の安全点に止まり、終了コード130で終わる (docs/spec/vm.mdの「上限と割り込み」)。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_par_program_with_130() {
    use std::io::{BufRead, BufReader};
    use std::time::Instant;

    let dir = temp_project("cli-sigint-par", &[("main.em", SPIN_IN_PAR)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    let stderr = child.stderr.take().unwrap();
    let (lines, arrived) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = lines.send(());
            text.push_str(&line);
            text.push('\n');
        }
        text
    });
    if arrived
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_err()
    {
        kill_after_deadline(&mut child, "the program did not start");
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
    let stderr = reader.join().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{stderr}");
    assert!(stderr.contains("interrupted"), "{stderr}");
}

/// 眠るタスクしかない run は、空きのループで本物の時計の上限付きの眠りを繰り返す。ループが毎回割り込みを読むので、
/// 1回目の SIGINT で止まり、終了コードは 130 である (docs/spec/vm.md の「時計と driver」)。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_sleeping_program_with_130() {
    use std::io::{BufRead, BufReader};
    use std::time::Instant;

    let text = "\
import Async

main : Unit -> <IO, Panic> Unit
main () =
  eprintln \"sleeping\"
  Async.run (fn () -> Async.sleep_ms 100000)
";
    let dir = temp_project("cli-sigint-sleep", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    let stderr = child.stderr.take().unwrap();
    let (lines, arrived) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = lines.send(());
            text.push_str(&line);
            text.push('\n');
        }
        text
    });
    if arrived
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_err()
    {
        kill_after_deadline(&mut child, "the program did not start");
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
    let stderr = reader.join().unwrap();
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
    // 子の `FileIn.open` は書き手が開くまで待つ。読み手のいないパイプを書き込み用に非同期で開くと ENXIO なので、開けた
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
import Env

main : Unit -> <IO, Panic> Unit
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

/// `--workers` は `run` だけのフラグで、`EML_WORKERS` より先に読む。0 と数でない値は、フラグでも環境変数でも
/// 使い方の誤りである。値を取るフラグは値を次の引数か `=` の後に取り、ファイルは値でない最初の引数で、ファイルより
/// 後ろのフラグはスクリプトに渡す。消したフラグは知らないフラグと同じに扱う (docs/spec/stdlib.md の「起動」)。
#[test]
fn workers_flag_is_accepted_and_rejected() {
    let dir = temp_project("cli-workers", &[("main.em", SHOW_ARGS)]);
    // (引数, 環境変数 EML_WORKERS, 終了コード, 標準出力)
    let cases: [(&[&str], Option<&str>, i32, &str); 24] = [
        (&["run", "--workers", "1", "main.em"], None, 0, "[]\n"),
        (
            &["run", "--workers", "2", "main.em", "a"],
            None,
            0,
            "[\"a\"]\n",
        ),
        (&["run", "--workers=8", "main.em"], None, 0, "[]\n"),
        (
            &["run", "--debug-heap", "--workers", "3", "main.em"],
            None,
            0,
            "[]\n",
        ),
        (&["main.em"], Some("2"), 0, "[]\n"),
        // フラグが環境変数に勝つ
        (&["run", "--workers", "1", "main.em"], Some("0"), 0, "[]\n"),
        (
            &["run", "main.em", "--workers", "0"],
            None,
            0,
            "[\"--workers\", \"0\"]\n",
        ),
        (&["run", "--workers", "0", "main.em"], None, 2, ""),
        (&["run", "--workers", "many", "main.em"], None, 2, ""),
        (&["run", "--workers", "-1", "main.em"], None, 2, ""),
        (&["run", "--workers", "1025", "main.em"], None, 2, ""),
        (&["run", "main.em"], Some("0"), 2, ""),
        (&["run", "main.em"], Some("many"), 2, ""),
        (&["repl", "--workers", "2"], None, 2, ""),
        (
            &["run", "--workers", "1", "--debug-heap", "main.em"],
            None,
            0,
            "[]\n",
        ),
        (
            &["main.em", "--workers", "1"],
            None,
            0,
            "[\"--workers\", \"1\"]\n",
        ),
        (
            &["run", "main.em", "--workers"],
            None,
            0,
            "[\"--workers\"]\n",
        ),
        (
            &["run", "--workers", "1", "--workers", "2", "main.em"],
            None,
            2,
            "",
        ),
        (&["run", "--workers"], None, 2, ""),
        (&["run", "--workers", "1"], None, 2, ""),
        (&["--workers", "1", "main.em"], None, 2, ""),
        (&["check", "--workers", "1", "main.em"], None, 2, ""),
        (&["run", "--sched", "normal", "main.em"], None, 2, ""),
        (
            &["run", "main.em", "--sched", "bogus"],
            None,
            0,
            "[\"--sched\", \"bogus\"]\n",
        ),
    ];
    let dir_ref = &dir;
    let outputs: Vec<Output> = std::thread::scope(|scope| {
        let handles: Vec<_> = cases
            .iter()
            .map(|(args, env, _, _)| {
                scope.spawn(move || {
                    let mut command = Command::new(env!("CARGO_BIN_EXE_eml"));
                    command
                        .args(*args)
                        .current_dir(dir_ref)
                        .env_remove("EML_WORKERS");
                    if let Some(value) = env {
                        command.env("EML_WORKERS", value);
                    }
                    command.output().unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    std::fs::remove_dir_all(&dir).unwrap();
    for ((args, env, code, stdout), output) in cases.iter().zip(&outputs) {
        let what = format!("{args:?} EML_WORKERS={env:?}: {output:?}");
        assert_eq!(output.status.code(), Some(*code), "{what}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), *stdout, "{what}");
    }
}

/// `run` が持たないフラグも、ファイルより後ろならそのままスクリプトに渡す (docs/spec/stdlib.md の「起動」)。
#[test]
fn flags_after_the_file_go_to_the_script() {
    let dir = temp_project("cli-after-file", &[("main.em", SHOW_ARGS)]);
    let output = eml_in(&dir, &["run", "main.em", "--anything", "x"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[\"--anything\", \"x\"]\n"
    );
}

#[test]
fn a_script_reads_the_standard_input_and_writes_the_standard_error() {
    use std::io::Write;
    let text = "import Stdin\n\
main : Unit -> <IO, Panic> Unit
main () =
  eprintln \"to stderr\"
  let (line, h) = Stdin.read_line (Stdin.open ())
  println (show line)
  let (rest, h) = Stdin.read_all h
  Stdin.close h
  println rest
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

/// `Fs.write_text` は実際のディスクに書く。書くファイルを開けなければ `cannot open` の実行時エラーになる。
#[test]
fn a_script_writes_a_file_and_reports_a_failed_write() {
    let write = "import Fs\n\
main : Unit -> <IO> Unit
main () =
  Fs.write_text \"out.txt\" \"日本\\n\"
  println (Fs.read_text \"out.txt\")
";
    let fail = "import Fs\n\
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
        stderr.starts_with("runtime error: cannot open `missing-dir/x.txt`: not found"),
        "{stderr}"
    );
}

/// `FileOut` は実際のディスクに書く。`close` で書き出し、閉じないまま `exit` しても、ヒープを落とすときの破棄処理が
/// buffer を書き出す。
#[test]
fn a_file_out_is_written_out_at_exit_and_at_the_end() {
    let closed = "import FileOut\n\
main : Unit -> <IO> Unit
main () =
  FileOut.close (FileOut.write_line \"1\" (FileOut.create \"log.txt\"))
  FileOut.close (FileOut.write_line \"2\" (FileOut.append \"log.txt\"))
";
    let open = "import FileOut\n\
main : Unit -> <IO> Unit
main () =
  let f = FileOut.write_line \"kept\" (FileOut.create \"kept.txt\")
  exit 3
";
    let dir = temp_project("cli-file-out", &[("closed.em", closed), ("open.em", open)]);
    let closed = eml_in(&dir, &["closed.em"]);
    let open = eml_in(&dir, &["open.em"]);
    let log = std::fs::read_to_string(dir.join("log.txt"));
    let kept = std::fs::read_to_string(dir.join("kept.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(closed.status.code(), Some(0), "{closed:?}");
    assert_eq!(log.unwrap(), "1\n2\n");
    assert_eq!(open.status.code(), Some(3), "{open:?}");
    assert_eq!(kept.unwrap(), "kept\n");
}

/// テストのビルドの `eml` を先頭に置いた `PATH`。子のプロセスが `eml` を探せるようにする。
fn path_with_eml() -> OsString {
    let bin = Path::new(env!("CARGO_BIN_EXE_eml")).parent().unwrap();
    std::env::join_paths(
        std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap()
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
    let output = Command::new(&script)
        .args(["1", "-2"])
        .env("PATH", path_with_eml())
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[\"1\", \"-2\"]\n");
}

/// `eml run` の Host (`StdHost`) は、パイプラインの段を OS の pipe でつないで起動する。最後の段の標準出力は、`capture`
/// では pipe に、`run` では eml の標準出力にそのまま出る (docs/spec/stdlib.md の「`Proc`」)。
#[test]
fn a_pipeline_runs_through_the_real_host() {
    let text = "import Env\n\
import Proc ((.|))
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | [\"tag\"] -> tag (Stdin.open ())
    | _ ->
      let tag = `eml pipe.em tag`
      println (String.trim (Proc.capture (Proc.stdin_text \"a\\nb\\n\" (tag .| tag))))
      println (show (Proc.run (Proc.stdin_text \"c\\n\" (tag .| tag .| tag))))

tag : Stdin.Stdin -> <IO, Panic> Unit
tag h =
  match Stdin.read_line h with
    | (Some line, h) ->
      println \"> \\{line}\"
      tag h
    | (None, h) -> Stdin.close h
";
    let dir = temp_project("cli-pipeline", &[("pipe.em", text)]);
    let output = Command::new(env!("CARGO_BIN_EXE_eml"))
        .arg("pipe.em")
        .env("PATH", path_with_eml())
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "> > a\n> > b\n> > > c\n0\n"
    );
}

/// UTF-8 でない引数と環境変数の値は、読んだときに実行時エラーにする。
#[test]
fn arguments_and_variables_that_are_not_utf8_are_runtime_errors() {
    use std::ffi::OsStr;
    let get = "import Env\n\
main : Unit -> <IO> Unit
main () = println (show (Env.get \"BAD\"))
";
    let dir = temp_project("cli-not-utf8", &[("args.em", SHOW_ARGS), ("get.em", get)]);
    let bad = not_utf8();
    let bad = bad.as_os_str();
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

    let text = "import Proc\n\
main : Unit -> <IO, Panic> Unit
main () = println (show (Proc.run `sh -c 'echo started >&2; exec sleep 30'`))
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

/// `Exec.wait` で待つプログラムは、SIGINT を eml だけに送っても、次の安全点で止まり、終了コード 130 で終わる。待ちは
/// driver の中にあり、タスクのスレッドは OS の中で止まらないためである。巻き戻しが落とす子は kill するので、30 秒
/// 眠る子を待たない。子が標準エラーに書いた行を読んで、子が動いてから送る (docs/spec/stdlib.md の「`Std.Exec`」)。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_program_waiting_in_exec_wait_with_130() {
    use std::io::{BufRead, BufReader, Read};
    use std::time::{Duration, Instant};

    let text = "\
import Async (Async)
import Async
import Exec

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run (fn () -> Exec.run `sh -c 'echo started >&2; exec sleep 30'`)))
";
    let dir = temp_project("cli-sigint-exec", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut started = String::new();
    stderr.read_line(&mut started).unwrap();
    assert_eq!(started, "started\n");
    sigint(&child);
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
    assert!(rest.starts_with("runtime error: interrupted"), "{rest}");
}

/// 標準入力の読みの中で待つプログラムも、`FileIn.read_all` と同じく SIGINT で止まり、終了コードは 130 である。読みに入る
/// 直前に届いた SIGINT は読みを止めないので、終わるまで 100 ms ごとに送り続ける (2回目の SIGINT はその場で終わらせる。
/// `repeated_interrupts_end_a_program_waiting_in_a_read_with_130` と同じ)。
#[cfg(unix)]
#[test]
fn repeated_interrupts_end_a_program_waiting_for_the_standard_input_with_130() {
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    let text = "import Stdin\n\
main : Unit -> <IO, Panic> Unit
main () =
  eprintln \"ready\"
  let (line, h) = Stdin.read_line (Stdin.open ())
  Stdin.close h
  println (show line)
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

/// 待つタスクのない SIGTERM は、割り込みと同じ止め方で run を止め、終了コードは 143 である。眠るタスクしかない run は
/// 空きのループの上限付きの眠りの後に止める語を読むので、1回目の SIGTERM で止まる (docs/spec/vm.md の「上限と割り込み」)。
#[cfg(unix)]
#[test]
fn a_terminate_ends_an_idle_program_with_143() {
    use std::io::{BufRead, BufReader};
    use std::time::Instant;

    let text = "\
import Async

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    eprintln \"ready\"
    Async.sleep_ms 600000)
";
    let dir = temp_project("cli-sigterm-sleep", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    // ハンドラを入れる前の SIGTERM はプロセスを殺すので、スクリプトが書いた行を読んでから送る
    let mut ready = String::new();
    stderr.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        for line in stderr.lines().map_while(Result::ok) {
            text.push_str(&line);
            text.push('\n');
        }
        text
    });
    sigterm(&child);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not stop");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let stderr = reader.join().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(143), "{stderr}");
    // 安全点で止めたので、文言の後には止めたときの関数の名前が続く (docs/spec/vm.md の「誤り、止めること、終わり」)
    assert!(
        stderr.starts_with("runtime error: terminated in `"),
        "{stderr}"
    );
}

/// 2つの worker の `eml run` で、worker 1 に置いたタスクが眠る間に届いた、待つタスクのないシグナルは run を止める。
/// 終了コードはシグナルの種類で決まり、置き方によらない (docs/spec/vm.md の「上限と割り込み」)。
#[cfg(unix)]
fn an_unheard_signal_stops_a_program_with_a_task_on_worker_1(
    name: &str,
    send: fn(&std::process::Child),
    code: i32,
) {
    use std::time::Instant;

    let text = "\
import Async

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    eprintln \"ready\"
    Async.sleep_ms 600000)
  Async.await t)
";
    let dir = temp_project(name, &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "--workers", "2", "main.em"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = read_after_ready(&mut child);
    send(&child);
    let status = wait_for_exit(&mut child, deadline);
    let rest: Vec<String> = lines.iter().collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(code), "{rest:?}");
}

#[cfg(unix)]
#[test]
fn an_unheard_terminate_stops_a_program_with_a_task_on_worker_1_with_143() {
    an_unheard_signal_stops_a_program_with_a_task_on_worker_1("cli-sigterm-worker-1", sigterm, 143);
}

#[cfg(unix)]
#[test]
fn an_unheard_interrupt_stops_a_program_with_a_task_on_worker_1_with_130() {
    an_unheard_signal_stops_a_program_with_a_task_on_worker_1("cli-sigint-worker-1", sigint, 130);
}

/// 4つの worker の `eml run` (既定の方針と本物の時計) で、16 の計算のタスクと、ループバックのリスナーのタスクが
/// worker に散り、クライアントとの1往復を終える。各 worker が自分の driver を持つ run である (docs/spec/vm.md の
/// 「時計と driver」)。締め切りは止まりの見張りで、時間は確かめない。
#[cfg(unix)]
#[test]
fn eml_run_spreads_tasks_over_four_workers() {
    use std::io::Read;
    use std::time::Instant;

    let text = "\
import Async (Async)
import Async
import Bytes
import Net
import String

fib : Int -> <Panic> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

start_all : Int -> <IO, Panic, Async> List (Async.Task Int)
start_all n = if n == 0 then [] else Async.start (fn () -> fib (n + 4)) :: start_all (n - 1)

sum_all : List (Async.Task Int) -> Int -> <IO, Panic, Async> Int
sum_all ts acc = match ts with
  | [] -> acc
  | t :: rest -> sum_all rest (acc + Async.await t)

drain : Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
drain buf s =
  let n = Bytes.length buf
  let (buf, s) = Net.read_chunk buf s
  if Bytes.length buf == n then (buf, s) else drain buf s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen \"127.0.0.1\" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (buf, s) = drain (Bytes.of_string \"echo: \") s
    let (_, s) = Net.write_bytes buf s
    Net.close s)
  let ts = start_all 16
  let a = Net.connect \"127.0.0.1\" port
  let a = Net.shutdown_write (Net.write \"ping\" a)
  let (buf, a) = drain (Bytes.empty ()) a
  Net.close a
  Async.await server
  println (show (sum_all ts 0))
  println (String.decode buf))
";
    let dir = temp_project("cli-four-workers", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "--workers", "4", "main.em"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let status = wait_for_exit(&mut child, deadline);
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let stderr = stderr_of(&mut child);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert_eq!(stdout, "17703\necho: ping\n", "{stderr}");
}

/// 標準入力の読みの中で待つプログラムも SIGTERM で止まり、終了コードは 143 である。読みに入る直前に届いた SIGTERM は
/// 読みを止めないが、2回目の SIGTERM でハンドラがその場で終わらせ、終了コードは同じく 143 である。そのため、終わるまで
/// 100 ms ごとに送り続ける (`repeated_interrupts_end_a_program_waiting_for_the_standard_input_with_130` と同じ)。
#[cfg(unix)]
#[test]
fn repeated_terminates_end_a_program_waiting_for_the_standard_input_with_143() {
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    let text = "import Stdin\n\
main : Unit -> <IO, Panic> Unit
main () =
  eprintln \"ready\"
  let (line, h) = Stdin.read_line (Stdin.open ())
  Stdin.close h
  println (show line)
";
    let dir = temp_project("cli-sigterm-stdin", &[("main.em", text)]);
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
    // ハンドラを入れる前の SIGTERM はプロセスを殺すので、スクリプトが書いた行を読んでから送る
    let mut ready = String::new();
    BufReader::new(child.stderr.as_mut().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let status = loop {
        sigterm(&child);
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
    assert_eq!(status.code(), Some(143));
}

/// `ready` の行を読んでから、残りの標準エラーを別のスレッドで読み続ける。ハンドラを入れる前のシグナルはプロセスを殺すので、
/// シグナルを送るのは `ready` の後にする。
#[cfg(unix)]
fn read_after_ready(child: &mut std::process::Child) -> std::sync::mpsc::Receiver<String> {
    use std::io::{BufRead, BufReader};

    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut ready = String::new();
    stderr.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    let (lines, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in stderr.lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                return;
            }
        }
    });
    received
}

/// 子が終わるまで待つ。締め切りを過ぎたら子を殺して、テストを失敗にする。
#[cfg(unix)]
fn wait_for_exit(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
) -> std::process::ExitStatus {
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            kill_after_deadline(child, "the program did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// `eml run` では、SIGTERM は `Signal.wait Terminate` で待つタスクに届き、run は続く (docs/spec/stdlib.md の
/// 「`Std.Signal`」)。`eml run` の普通の方針で worker を1つに固め、`yield_now` で待ち手に順を渡すので、待ち手が
/// 待ちに入ってから `ready` を書く。待ち手が `Signal.wait` に着く前に監視のスレッドが横取りすると順が崩れるが、
/// そうなるのは、その数命令の間にプロセスが1刻み (10 ms) 以上止まったときだけである。
#[cfg(unix)]
#[test]
fn a_terminate_reaches_a_waiting_task() {
    use std::time::Instant;

    let text = "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Signal.wait Signal.Terminate
    eprintln \"got\")
  Async.yield_now ()
  eprintln \"ready\"
  Async.await t)
";
    let dir = temp_project("cli-sigterm-wait", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "--workers", "1", "main.em"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = read_after_ready(&mut child);
    sigterm(&child);
    let status = wait_for_exit(&mut child, deadline);
    let rest: Vec<String> = lines.iter().collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(0), "{rest:?}");
    assert_eq!(rest, ["got"]);
}

/// 届けた後のシグナルは1回目として扱う。待つタスクがもういないので、2回目の Ctrl-C は run を止め、終了コードは 130 で
/// ある (docs/spec/vm.md の「上限と割り込み」)。worker を1つに固め、`yield_now` で待ち手に順を渡す。順が崩れうるのは、
/// `a_terminate_reaches_a_waiting_task` と同じく、待ち手が待ちに入る前にプロセスが1刻み以上止まったときだけである。
#[cfg(unix)]
#[test]
fn an_interrupt_after_a_delivered_one_ends_the_program_with_130() {
    use std::time::Instant;

    let text = "\
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Signal.wait Signal.Interrupt
    eprintln \"got\")
  Async.yield_now ()
  eprintln \"ready\"
  Async.await t
  Async.sleep_ms 600000)
";
    let dir = temp_project("cli-sigint-delivered", &[("main.em", text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "--workers", "1", "main.em"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = read_after_ready(&mut child);
    sigint(&child);
    let got = lines.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    if got.as_deref() != Ok("got") {
        kill_after_deadline(&mut child, "the waiting task did not wake");
    }
    // `got` は届けた後にしか書かれず、届けた集める地点が見張りの語も空にしている
    sigint(&child);
    let status = wait_for_exit(&mut child, deadline);
    let rest: Vec<String> = lines.iter().collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{rest:?}");
    // 眠る main の空きのループが止めるので、誤りには切り替えに入った命令の位置が付き、文言の後にその関数の名前が続く
    // (docs/spec/vm.md の「時計と driver」)
    assert!(
        rest.first()
            .is_some_and(|line| line.starts_with("runtime error: interrupted in `")),
        "{rest:?}"
    );
}

/// 待つタスクがいても、worker が標準入力の読みで止まっていれば、シグナルは読みが戻るまで届かない。届けていない間の
/// 2回目のシグナルは、ハンドラがその場で終わらせる (docs/spec/vm.md の「上限と割り込み」)。worker を1つに固め、
/// `yield_now` で待ち手に順を渡す。横取りで待ち手が待ちに入る前に `ready` を書いても、worker は読みで止まって
/// 1回目のシグナルを聞けないので、終わり方は変わらない。
#[cfg(unix)]
fn a_second_signal_ends_a_program_whose_waiter_cannot_be_reached(
    name: &str,
    kind: &str,
    send: fn(&std::process::Child),
    code: i32,
) {
    use std::time::{Duration, Instant};

    let text = format!(
        "\
import Async
import Signal
import Stdin

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Async.spawn (fn () -> Signal.wait Signal.{kind})
  Async.yield_now ()
  eprintln \"ready\"
  let (line, h) = Stdin.read_line (Stdin.open ())
  Stdin.close h
  println (show line))
"
    );
    let dir = temp_project(name, &[("main.em", &text)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "--workers", "1", "main.em"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // 書き手を開いたままにして、読みを待たせる
    let writer = child.stdin.take();
    let lines = read_after_ready(&mut child);
    let status = loop {
        send(&child);
        std::thread::sleep(Duration::from_millis(100));
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not end");
        }
    };
    drop(writer);
    let rest: Vec<String> = lines.iter().collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(code), "{rest:?}");
    // ハンドラの `_exit` は何も書かない。止める語で止まったのなら、実行時エラーの文言が出る
    assert!(rest.is_empty(), "{rest:?}");
}

#[cfg(unix)]
#[test]
fn a_second_ctrl_c_ends_a_program_whose_waiter_cannot_be_reached() {
    a_second_signal_ends_a_program_whose_waiter_cannot_be_reached(
        "cli-sigint-unreached",
        "Interrupt",
        sigint,
        130,
    );
}

#[cfg(unix)]
#[test]
fn a_second_terminate_ends_a_program_whose_waiter_cannot_be_reached() {
    a_second_signal_ends_a_program_whose_waiter_cannot_be_reached(
        "cli-sigterm-unreached",
        "Terminate",
        sigterm,
        143,
    );
}

/// 標準出力を受け継ぐ `run (merge_stderr c)` の子は、標準エラーを eml の標準出力に書く。`before` が子の行より前に
/// 並ぶのは、起動の前に標準出力を書き出す規則による (docs/spec/stdlib.md の「`Proc`」の「起動」)。
#[test]
fn a_run_child_with_merged_errors_writes_them_to_the_standard_output() {
    let main = "import Proc\n\
main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  println \"exit \\{show (Proc.run (Proc.merge_stderr `eml run child.em`))}\"
";
    let child = "main : Unit -> <IO> Unit\nmain () =\n  eprintln \"child error\"\n  exit 3\n";
    let dir = temp_project("cli-merged-run", &[("main.em", main), ("child.em", child)]);
    let output = Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "main.em"])
        .env("PATH", path_with_eml())
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "before\nchild error\nexit 3\n"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

/// 動いている OS の C の数学のライブラリ。`--link` の VALUE に渡す。
#[cfg(target_os = "linux")]
pub const LIBM: &str = "libm.so.6";
#[cfg(target_os = "macos")]
pub const LIBM: &str = "libm.dylib";
#[cfg(windows)]
pub const LIBM: &str = "ucrtbase.dll";

/// つなげないプログラムのファイルは `tests/ffi` に置き、UI テストには入れない。つなぐ失敗は実行時エラーでないので、
/// UI の run-fail の道に載らないためである。
fn eml_ffi(args: &[&str]) -> Output {
    eml_in(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ffi")),
        args,
    )
}

#[test]
fn a_c_program_runs_through_the_binary() {
    let output = eml(&["run", "run/ffi/libm_sqrt.em"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

/// `--link KEY=VALUE` は、`link` に KEY と書いたライブラリの代わりに VALUE を開く (docs/spec/vm.md の「`--link`」)。
#[test]
fn link_override_replaces_the_library() {
    let main = "link on linux \"eml_override_me\"
link on macos \"eml_override_me\"
link on windows \"eml_override_me\"

extern \"C\" pure sqrt : Float -> Float

main : Unit -> <IO> Unit
main () = println (show (sqrt 16.0))
";
    let dir = temp_project("cli-link-override", &[("main.em", main)]);
    let overridden = format!("eml_override_me={LIBM}");
    let replaced = eml_in(&dir, &["run", "--link", &overridden, "main.em"]);
    let written = eml_in(&dir, &["run", "main.em"]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(replaced.status.code(), Some(0), "{replaced:?}");
    assert_eq!(String::from_utf8_lossy(&replaced.stdout), "4.0\n");
    assert_eq!(String::from_utf8_lossy(&replaced.stderr), "");
    assert_eq!(written.status.code(), Some(1), "{written:?}");
    assert_eq!(
        String::from_utf8_lossy(&written.stderr),
        "link error: library \"eml_override_me\" was not found\n"
    );
}

/// `run` は、どの `link` の item にも書いていない KEY を書き誤りとして断る。C の関数のないプログラムでも同じである。
#[test]
fn an_unknown_link_key_is_a_link_error() {
    for file in ["run/ffi/libm_sqrt.em", "run/basics/comments_only.em"] {
        let output = eml(&["run", "--link", "eml_unwritten=libm.so.6", file]);
        assert_eq!(output.status.code(), Some(1), "{file}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "", "{file}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "link error: --link names no library \"eml_unwritten\"\n",
            "{file}"
        );
    }
}

/// `=` のない値と、空の KEY か VALUE は、`run` でも `repl` でも使い方の誤りである (docs/spec/stdlib.md の「起動」)。
#[test]
fn a_malformed_link_flag_is_a_usage_error() {
    for value in ["libm", "=libm", "libm="] {
        for command in ["run", "repl"] {
            let output = eml(&[command, "--link", value, "run/ffi/libm_sqrt.em"]);
            assert_eq!(
                output.status.code(),
                Some(2),
                "{command} {value}: {output:?}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains(&format!("`{value}` is not KEY=VALUE")),
                "{command} {value}: {stderr}"
            );
        }
    }
}

/// `eml check` はライブラリを開かないので、開けないライブラリを書いたファイルも通る。
#[test]
fn check_loads_no_library() {
    let output = eml_ffi(&["check", "missing_library.em"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

/// つなぐ失敗の文言は OS によらない (docs/spec/vm.md の「ライブラリと記号」)。
#[test]
fn a_missing_library_is_a_link_error() {
    let output = eml_ffi(&["run", "missing_library.em"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "link error: library \"eml_no_such_library\" was not found\n"
    );
}

#[test]
fn a_missing_symbol_is_a_link_error() {
    let output = eml_ffi(&["run", "missing_symbol.em"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "link error: symbol \"eml_no_such_symbol\" was not found\n"
    );
}

/// BLAS を開く例のファイルは `tests/blas` に置く。ライブラリを入れていない機械では流せないので、UI テストには入れない。
fn eml_blas(file: &str) -> Output {
    eml_in(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/blas")),
        &["run", file],
    )
}

#[test]
#[ignore = "needs a BLAS library"]
fn blas_dgemm() {
    let output = eml_blas("dgemm.em");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "19\n22\n43\n50\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
#[ignore = "needs Accelerate"]
#[cfg(target_os = "macos")]
fn blas_dgesv() {
    let output = eml_blas("dgesv_ilp64.em");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n14\n0\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}
